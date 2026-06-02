#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && /bin/pwd -P)"
repo_dir="$(cd -- "$script_dir/.." && /bin/pwd -P)"
cd "$repo_dir"

input="${1:-}"
state_file="${RUMPEL_PROFILE_STATE_FILE:-$repo_dir/.ai_tasks/last_gui_run.env}"
allow_latest_fallback=0

read_env_value() {
    local key="$1"
    local file="$2"
    awk -F= -v key="$key" '$1 == key { print substr($0, index($0, "=") + 1); exit }' "$file"
}

if [[ -z "$input" ]]; then
    if [[ ! -f "$state_file" ]]; then
        echo "profile state file not found: $state_file" >&2
        exit 66
    fi
    input="$state_file"
    allow_latest_fallback=1
fi

profile_log="$input"
screenshot=""
stderr_log=""
fallback_from=""
if [[ -f "$input" ]] && grep -q '^STDOUT_LOG=' "$input"; then
    profile_log="$(read_env_value STDOUT_LOG "$input")"
    stderr_log="$(read_env_value STDERR_LOG "$input")"
    screenshot="$(read_env_value SCREENSHOT "$input")"
    allow_latest_fallback=1
elif [[ -f "$state_file" ]]; then
    state_stdout="$(read_env_value STDOUT_LOG "$state_file")"
    if [[ "$state_stdout" == "$profile_log" ]]; then
        stderr_log="$(read_env_value STDERR_LOG "$state_file")"
        screenshot="$(read_env_value SCREENSHOT "$state_file")"
    fi
fi

if [[ ! -f "$profile_log" ]]; then
    if [[ "$allow_latest_fallback" != "1" ]]; then
        echo "profile log not found: $profile_log" >&2
        exit 66
    fi
    latest_profile_log="$(
        find "$repo_dir/.ai_tasks" -type f -name '*.stdout.log' -exec stat -f '%m %N' {} + 2>/dev/null \
            | sort -nr \
            | while IFS=' ' read -r _ candidate; do
                if grep -q '^profile end ' "$candidate"; then
                    printf '%s\n' "$candidate"
                    break
                fi
            done
    )"
    if [[ -n "$latest_profile_log" ]]; then
        fallback_from="$profile_log"
        profile_log="$latest_profile_log"
        stderr_log=""
        screenshot=""
    else
        echo "profile log not found: $profile_log" >&2
        exit 66
    fi
fi

if [[ -z "$stderr_log" && "$profile_log" == *.stdout.log ]]; then
    inferred_stderr_log="${profile_log%.stdout.log}.stderr.log"
    if [[ -f "$inferred_stderr_log" ]]; then
        stderr_log="$inferred_stderr_log"
    fi
fi

present_mode_fallback="-"
compute_parity="-"
compute_lifecycle="-"
compute_edits="-"
world_edits="-"
compute_direct="-"
if [[ -n "$stderr_log" && -f "$stderr_log" ]]; then
    present_mode_fallback="$(
        awk '
            match($0, /PresentMode [[:alnum:]-]+ requested but not available\. Falling back to [[:alnum:]-]+/) {
                message = substr($0, RSTART, RLENGTH)
                split(message, parts, " ")
                fallback = parts[2] "->" parts[10]
            }
            END {
                if (fallback != "") print fallback
                else print "-"
            }
        ' "$stderr_log"
    )"
    compute_parity="$(
        awk '
            function strip_ansi(line) {
                gsub(/\033\[[0-9;]*m/, "", line)
                return line
            }
            function value(line, key, fallback, marker, parts, field_count, i) {
                marker = key "="
                field_count = split(line, parts, " ")
                for (i = 1; i <= field_count; i += 1) {
                    if (index(parts[i], marker) == 1) {
                        return substr(parts[i], length(marker) + 1)
                    }
                }
                return fallback
            }
            {
                line = strip_ansi($0)
                if (index(line, "voxel compute queue generated from rumpel_world") != 0) {
                    queue_seen += 1
                    queue_chunks = value(line, "chunks", "?")
                } else if (index(line, "voxel compute queue chunk prepared") != 0) {
                    prepared += 1
                    solid_blocks = value(line, "solid_blocks", "0") + 0
                    expected_vertices = value(line, "expected_vertices", "0") + 0
                    expected_indices = value(line, "expected_indices", "0") + 0
                    prepared_solid_blocks += solid_blocks
                    prepared_vertices += expected_vertices
                    prepared_indices += expected_indices
                    if (solid_blocks == 0) {
                        zero_solid += 1
                    }
                    if (expected_indices > max_expected_indices) {
                        max_expected_indices = expected_indices
                    }
                } else if (index(line, "voxel compute parity contract matched") != 0) {
                    matched += 1
                    matched_vertices += value(line, "vertices", "0") + 0
                    matched_indices += value(line, "indices", "0") + 0
                } else if (index(line, "voxel compute parity contract mismatch") != 0) {
                    mismatch += 1
                } else if (index(line, "voxel compute mesh counters need attention") != 0) {
                    attention += 1
                } else if (index(line, "voxel compute counter readback failed") != 0) {
                    readback_failed += 1
                }
            }
            END {
                if (queue_seen || prepared || matched || mismatch || attention || readback_failed) {
                    readback_results = matched + mismatch + attention + readback_failed
                    printf "queue_chunks=%s prepared=%d matched=%d mismatch=%d attention=%d readback_failed=%d readback_results=%d prepared_solid_blocks=%d prepared_vertices=%d prepared_indices=%d matched_vertices=%d matched_indices=%d zero_solid=%d max_expected_indices=%d\n",
                        (queue_chunks == "" ? "?" : queue_chunks), prepared, matched, mismatch, attention, readback_failed,
                        readback_results, prepared_solid_blocks, prepared_vertices, prepared_indices,
                        matched_vertices, matched_indices, zero_solid, max_expected_indices
                } else {
                    print "-"
                }
            }
        ' "$stderr_log"
    )"
    compute_lifecycle="$(
        awk '
            function strip_ansi(line) {
                gsub(/\033\[[0-9;]*m/, "", line)
                return line
            }
            function value(line, key, fallback, marker, parts, field_count, i) {
                marker = key "="
                field_count = split(line, parts, " ")
                for (i = 1; i <= field_count; i += 1) {
                    if (index(parts[i], marker) == 1) {
                        return substr(parts[i], length(marker) + 1)
                    }
                }
                return fallback
            }
            {
                line = strip_ansi($0)
                if (index(line, "voxel compute lifecycle summary") != 0) {
                    summaries += 1
                    pending = value(line, "pending", "0") + 0
                    building = value(line, "building", "0") + 0
                    loaded = value(line, "loaded", "0") + 0
                    total = value(line, "total", "0") + 0
                    queued = value(line, "queued_this_frame", "0") + 0
                    submitted = value(line, "submitted_this_frame", "0") + 0
                    invalidated = value(line, "invalidated_this_frame", "0") + 0
                    rebuilds = value(line, "rebuilds_this_frame", "0") + 0
                    evicted_lifecycle = value(line, "evicted_lifecycle_this_frame", "0") + 0
                    evicted_buffers = value(line, "evicted_buffers_this_frame", "0") + 0
                    cancelled_readbacks = value(line, "cancelled_readbacks_this_frame", "0") + 0
                    owned_output_buffers = value(line, "owned_output_buffers_this_frame", "0") + 0
                    owned_output_bytes = value(line, "owned_output_bytes_this_frame", "0") + 0
                    queued_sum += queued
                    submitted_sum += submitted
                    invalidated_sum += invalidated
                    rebuilds_sum += rebuilds
                    evicted_lifecycle_sum += evicted_lifecycle
                    evicted_buffers_sum += evicted_buffers
                    cancelled_readbacks_sum += cancelled_readbacks
                    owned_output_buffers_sum += owned_output_buffers
                    owned_output_bytes_sum += owned_output_bytes
                    if (building > max_building) {
                        max_building = building
                    }
                }
            }
            END {
                if (summaries) {
                    printf "summaries=%d pending=%d building=%d loaded=%d total=%d queued_sum=%d submitted_sum=%d invalidated_sum=%d rebuilds_sum=%d evicted_lifecycle_sum=%d evicted_buffers_sum=%d cancelled_readbacks_sum=%d owned_output_buffers_sum=%d owned_output_bytes_sum=%d max_building=%d\n",
                        summaries, pending, building, loaded, total, queued_sum, submitted_sum,
                        invalidated_sum, rebuilds_sum, evicted_lifecycle_sum, evicted_buffers_sum,
                        cancelled_readbacks_sum, owned_output_buffers_sum, owned_output_bytes_sum,
                        max_building
                } else {
                    print "-"
                }
            }
        ' "$stderr_log"
    )"
    compute_edits="$(
        awk '
            function strip_ansi(line) {
                gsub(/\033\[[0-9;]*m/, "", line)
                return line
            }
            function value(line, key, fallback, marker, parts, field_count, i) {
                marker = key "="
                field_count = split(line, parts, " ")
                for (i = 1; i <= field_count; i += 1) {
                    if (index(parts[i], marker) == 1) {
                        return substr(parts[i], length(marker) + 1)
                    }
                }
                return fallback
            }
            {
                line = strip_ansi($0)
                if (index(line, "voxel compute block edits applied") != 0) {
                    summaries += 1
                    applied = value(line, "applied_edits", "0") + 0
                    ignored = value(line, "ignored_edits", "0") + 0
                    touched = value(line, "touched_chunks", "0") + 0
                    applied_sum += applied
                    ignored_sum += ignored
                    if (touched > max_touched_chunks) {
                        max_touched_chunks = touched
                    }
                }
            }
            END {
                if (summaries) {
                    printf "summaries=%d applied_sum=%d ignored_sum=%d max_touched_chunks=%d\n",
                        summaries, applied_sum, ignored_sum, max_touched_chunks
                } else {
                    print "-"
                }
            }
        ' "$stderr_log"
    )"
    world_edits="$(
        awk '
            function strip_ansi(line) {
                gsub(/\033\[[0-9;]*m/, "", line)
                return line
            }
            function value(line, key, fallback, marker, parts, field_count, i) {
                marker = key "="
                field_count = split(line, parts, " ")
                for (i = 1; i <= field_count; i += 1) {
                    if (index(parts[i], marker) == 1) {
                        return substr(parts[i], length(marker) + 1)
                    }
                }
                return fallback
            }
            {
                line = strip_ansi($0)
                if (index(line, "world block edits stored") != 0) {
                    summaries += 1
                    stored = value(line, "stored_edits", "0") + 0
                    ignored = value(line, "ignored_edits", "0") + 0
                    generation = value(line, "store_generation", "0") + 0
                    edits = value(line, "store_edits", "0") + 0
                    stored_sum += stored
                    ignored_sum += ignored
                    if (generation > max_store_generation) {
                        max_store_generation = generation
                    }
                    if (edits > max_store_edits) {
                        max_store_edits = edits
                    }
                }
            }
            END {
                if (summaries) {
                    printf "summaries=%d stored_sum=%d ignored_sum=%d max_store_generation=%d max_store_edits=%d\n",
                        summaries, stored_sum, ignored_sum, max_store_generation, max_store_edits
                } else {
                    print "-"
                }
            }
        ' "$stderr_log"
    )"
    compute_direct="$(
        awk '
            function strip_ansi(line) {
                gsub(/\033\[[0-9;]*m/, "", line)
                return line
            }
            function value(line, key, fallback, marker, parts, field_count, i) {
                marker = key "="
                field_count = split(line, parts, " ")
                for (i = 1; i <= field_count; i += 1) {
                    if (index(parts[i], marker) == 1) {
                        return substr(parts[i], length(marker) + 1)
                    }
                }
                return fallback
            }
            {
                line = strip_ansi($0)
                if (index(line, "voxel compute direct render summary") != 0) {
                    summaries += 1
                    views = value(line, "views", "0") + 0
                    chunks = value(line, "chunks_drawn", "0") + 0
                    draw_calls = value(line, "draw_calls", "0") + 0
                    indices = value(line, "indices_drawn", "0") + 0
                    arena_slots = value(line, "arena_slots", "0") + 0
                    indirect_commands = value(line, "indirect_commands", "0") + 0
                    cull_enabled = value(line, "cull_enabled", "false")
                    cull_count_supported = value(line, "cull_count_supported", "false")
                    cull_compact_enabled = value(line, "cull_compact_enabled", "false")
                    cull_candidates = value(line, "cull_candidate_commands", "0") + 0
                    cull_visible = value(line, "cull_visible_commands", "0") + 0
                    cull_culled = value(line, "cull_culled_commands", "0") + 0
                    draw_mode = value(line, "draw_mode", "-")
                    skipped = value(line, "skipped_without_bind_group", "0") + 0
                    if (views > max_views) {
                        max_views = views
                    }
                    if (chunks > max_chunks_drawn) {
                        max_chunks_drawn = chunks
                    }
                    if (draw_calls > max_draw_calls) {
                        max_draw_calls = draw_calls
                    }
                    if (indices > max_indices_drawn) {
                        max_indices_drawn = indices
                    }
                    if (arena_slots > max_arena_slots) {
                        max_arena_slots = arena_slots
                    }
                    if (indirect_commands > max_indirect_commands) {
                        max_indirect_commands = indirect_commands
                    }
                    if (cull_enabled == "true") {
                        cull_enabled_seen = 1
                    }
                    if (cull_count_supported == "true") {
                        cull_count_supported_seen = 1
                    }
                    if (cull_compact_enabled == "true") {
                        cull_compact_enabled_seen = 1
                    }
                    if (cull_candidates > max_cull_candidates) {
                        max_cull_candidates = cull_candidates
                    }
                    if (cull_visible > max_cull_visible) {
                        max_cull_visible = cull_visible
                    }
                    if (cull_culled > max_cull_culled) {
                        max_cull_culled = cull_culled
                    }
                    cull_culled_sum += cull_culled
                    if (draw_mode != "-") {
                        draw_modes[draw_mode] = 1
                    }
                    skipped_sum += skipped
                }
            }
            END {
                if (summaries) {
                    modes = "-"
                    for (mode in draw_modes) {
                        modes = (modes == "-" ? mode : modes "," mode)
                    }
                    printf "summaries=%d max_views=%d max_chunks_drawn=%d max_draw_calls=%d max_indices_drawn=%d max_arena_slots=%d max_indirect_commands=%d cull_enabled=%d cull_count_supported=%d cull_compact_enabled=%d max_cull_candidates=%d max_cull_visible=%d max_cull_culled=%d cull_culled_sum=%d draw_modes=%s skipped_without_bind_group_sum=%d\n",
                        summaries, max_views, max_chunks_drawn, max_draw_calls, max_indices_drawn,
                        max_arena_slots, max_indirect_commands, cull_enabled_seen,
                        cull_count_supported_seen, cull_compact_enabled_seen,
                        max_cull_candidates, max_cull_visible, max_cull_culled, cull_culled_sum,
                        modes, skipped_sum
                } else {
                    print "-"
                }
            }
        ' "$stderr_log"
    )"
fi

awk -v log_path="$profile_log" -v screenshot_path="$screenshot" -v fallback_from="$fallback_from" -v present_mode_fallback="$present_mode_fallback" -v compute_parity="$compute_parity" -v compute_lifecycle="$compute_lifecycle" -v compute_edits="$compute_edits" -v world_edits="$world_edits" -v compute_direct="$compute_direct" '
function value(line, key, fallback, marker, parts, field_count, i) {
    marker = key "="
    field_count = split(line, parts, " ")
    for (i = 1; i <= field_count; i += 1) {
        if (index(parts[i], marker) == 1) {
            return substr(parts[i], length(marker) + 1)
        }
    }
    return fallback
}

function append_row(text) {
    sample_rows = sample_rows text "\n"
}

function append_slow(text) {
    slow_rows = slow_rows text "\n"
}

BEGIN {
    sample_rows = ""
    slow_rows = ""
    sample_count = 0
    slow_count = 0
    measured_sample_count = 0
    max_slow_ms = 0
    max_interval_ms = 0
    max_packed_prepare_us = 0
    max_packed_view_prepare_us = 0
    max_packed_stream_us = 0
    max_packed_build_task_us = 0
    max_packed_compaction_us = 0
    max_packed_render_node_us = 0
    max_packed_render_gpu_pass_us = 0
    max_packed_gpu_cull_node_us = 0
    max_packed_gpu_cull_input_commands = 0
    max_packed_gpu_cull_est_visible_commands = 0
    max_packed_gpu_cull_est_visible_quads = 0
    max_packed_cpu_visible_commands = 0
    max_packed_visible_quads = 0
    max_packed_uploaded_quads = 0
    max_packed_uploaded_this_frame = 0
    max_packed_pending_builds = 0
    max_packed_pending_region_rebuilds = 0
    max_packed_arena_used_quads = 0
    max_packed_arena_slot_quads = 0
    max_packed_chunk_ranges = 0
    max_packed_resident_ranges = 0
    max_packed_tombstone_ranges = 0
    max_packed_tombstone_capacity_quads = 0
    max_packed_dirty_ranges = 0
    max_packed_dirty_range_quads = 0
    packed_gpu_cull_enabled_seen = 0
    packed_gpu_cull_count_supported_seen = 0
    packed_gpu_cull_compact_enabled_seen = 0
    packed_cpu_visible_compact_seen = 0
    max_measured_packed_prepare_us = 0
    max_measured_packed_view_prepare_us = 0
    max_measured_packed_stream_us = 0
    max_measured_packed_build_task_us = 0
    max_measured_packed_compaction_us = 0
    max_measured_packed_render_node_us = 0
    max_measured_packed_render_gpu_pass_us = 0
    max_measured_packed_gpu_cull_node_us = 0
    max_measured_packed_gpu_cull_input_commands = 0
    max_measured_packed_gpu_cull_est_visible_commands = 0
    max_measured_packed_gpu_cull_est_visible_quads = 0
    max_measured_packed_cpu_visible_commands = 0
    max_measured_packed_visible_quads = 0
    max_measured_packed_uploaded_quads = 0
    max_measured_packed_uploaded_this_frame = 0
    max_measured_packed_pending_builds = 0
    max_measured_packed_pending_region_rebuilds = 0
    max_measured_packed_arena_used_quads = 0
    max_measured_packed_arena_slot_quads = 0
    max_measured_packed_chunk_ranges = 0
    max_measured_packed_resident_ranges = 0
    max_measured_packed_tombstone_ranges = 0
    max_measured_packed_tombstone_capacity_quads = 0
    max_measured_packed_dirty_ranges = 0
    max_measured_packed_dirty_range_quads = 0
    measured_packed_gpu_cull_enabled_seen = 0
    measured_packed_gpu_cull_count_supported_seen = 0
    measured_packed_gpu_cull_compact_enabled_seen = 0
    measured_packed_cpu_visible_compact_seen = 0
}

/^profile start / {
    start_line = $0
    next
}

/^profile ready / {
    ready_line = $0
    next
}

/^profile sample=/ {
    sample_count += 1
    frame_ms = value($0, "frame_ms", "?")
    interval_worst_ms = value($0, "interval_worst_frame_ms", "0")
    if (interval_worst_ms + 0 > max_interval_ms + 0) {
        max_interval_ms = interval_worst_ms
        max_interval_line = $0
    }
    latest_sample = $0
    packed_prepare_us = value($0, "packed_prepare_us", "0") + 0
    packed_view_prepare_us = value($0, "packed_view_prepare_us", "0") + 0
    packed_stream_us = value($0, "packed_stream_us", "0") + 0
    packed_build_task_us = value($0, "packed_build_task_us", "0") + 0
    packed_compaction_us = value($0, "packed_compaction_us", "0") + 0
    packed_render_node_us = value($0, "packed_render_node_us", "0") + 0
    packed_render_gpu_pass_us = value($0, "packed_render_gpu_pass_us", "0") + 0
    packed_gpu_cull_node_us = value($0, "packed_gpu_cull_node_us", "0") + 0
    packed_gpu_cull_input_commands = value($0, "packed_gpu_cull_input_commands", "0") + 0
    packed_gpu_cull_est_visible_commands = value($0, "packed_gpu_cull_est_visible_commands", "0") + 0
    packed_gpu_cull_est_visible_quads = value($0, "packed_gpu_cull_est_visible_quads", "0") + 0
    packed_cpu_visible_commands = value($0, "packed_cpu_visible_commands", "0") + 0
    packed_visible_quads = value($0, "packed_visible_quads", "0") + 0
    packed_uploaded_quads = value($0, "packed_uploaded_quads", "0") + 0
    packed_uploaded_this_frame = value($0, "uploaded_this_frame", "0") + 0
    packed_pending_builds = value($0, "pending_builds", "0") + 0
    packed_pending_region_rebuilds = value($0, "pending_region_rebuilds", "0") + 0
    packed_arena_used_quads = value($0, "arena_used_quads", "0") + 0
    packed_arena_slot_quads = value($0, "arena_slot_quads", "0") + 0
    packed_chunk_ranges = value($0, "packed_chunk_ranges", "0") + 0
    packed_resident_ranges = value($0, "packed_resident_ranges", "0") + 0
    packed_tombstone_ranges = value($0, "packed_tombstone_ranges", "0") + 0
    packed_tombstone_capacity_quads = value($0, "packed_tombstone_capacity_quads", "0") + 0
    packed_dirty_ranges = value($0, "packed_dirty_ranges", "0") + 0
    packed_dirty_range_quads = value($0, "packed_dirty_range_quads", "0") + 0
    if (packed_prepare_us > max_packed_prepare_us) max_packed_prepare_us = packed_prepare_us
    if (packed_view_prepare_us > max_packed_view_prepare_us) max_packed_view_prepare_us = packed_view_prepare_us
    if (packed_stream_us > max_packed_stream_us) max_packed_stream_us = packed_stream_us
    if (packed_build_task_us > max_packed_build_task_us) max_packed_build_task_us = packed_build_task_us
    if (packed_compaction_us > max_packed_compaction_us) max_packed_compaction_us = packed_compaction_us
    if (packed_render_node_us > max_packed_render_node_us) max_packed_render_node_us = packed_render_node_us
    if (packed_render_gpu_pass_us > max_packed_render_gpu_pass_us) max_packed_render_gpu_pass_us = packed_render_gpu_pass_us
    if (packed_gpu_cull_node_us > max_packed_gpu_cull_node_us) max_packed_gpu_cull_node_us = packed_gpu_cull_node_us
    if (packed_gpu_cull_input_commands > max_packed_gpu_cull_input_commands) max_packed_gpu_cull_input_commands = packed_gpu_cull_input_commands
    if (packed_gpu_cull_est_visible_commands > max_packed_gpu_cull_est_visible_commands) max_packed_gpu_cull_est_visible_commands = packed_gpu_cull_est_visible_commands
    if (packed_gpu_cull_est_visible_quads > max_packed_gpu_cull_est_visible_quads) max_packed_gpu_cull_est_visible_quads = packed_gpu_cull_est_visible_quads
    if (packed_cpu_visible_commands > max_packed_cpu_visible_commands) max_packed_cpu_visible_commands = packed_cpu_visible_commands
    if (packed_visible_quads > max_packed_visible_quads) max_packed_visible_quads = packed_visible_quads
    if (packed_uploaded_quads > max_packed_uploaded_quads) max_packed_uploaded_quads = packed_uploaded_quads
    if (packed_uploaded_this_frame > max_packed_uploaded_this_frame) max_packed_uploaded_this_frame = packed_uploaded_this_frame
    if (packed_pending_builds > max_packed_pending_builds) max_packed_pending_builds = packed_pending_builds
    if (packed_pending_region_rebuilds > max_packed_pending_region_rebuilds) max_packed_pending_region_rebuilds = packed_pending_region_rebuilds
    if (packed_arena_used_quads > max_packed_arena_used_quads) max_packed_arena_used_quads = packed_arena_used_quads
    if (packed_arena_slot_quads > max_packed_arena_slot_quads) max_packed_arena_slot_quads = packed_arena_slot_quads
    if (packed_chunk_ranges > max_packed_chunk_ranges) max_packed_chunk_ranges = packed_chunk_ranges
    if (packed_resident_ranges > max_packed_resident_ranges) max_packed_resident_ranges = packed_resident_ranges
    if (packed_tombstone_ranges > max_packed_tombstone_ranges) max_packed_tombstone_ranges = packed_tombstone_ranges
    if (packed_tombstone_capacity_quads > max_packed_tombstone_capacity_quads) max_packed_tombstone_capacity_quads = packed_tombstone_capacity_quads
    if (packed_dirty_ranges > max_packed_dirty_ranges) max_packed_dirty_ranges = packed_dirty_ranges
    if (packed_dirty_range_quads > max_packed_dirty_range_quads) max_packed_dirty_range_quads = packed_dirty_range_quads
    if (value($0, "packed_gpu_cull_enabled", "false") == "true") packed_gpu_cull_enabled_seen = 1
    if (value($0, "packed_gpu_cull_count_supported", "false") == "true") packed_gpu_cull_count_supported_seen = 1
    if (value($0, "packed_gpu_cull_compact_enabled", "false") == "true") packed_gpu_cull_compact_enabled_seen = 1
    if (value($0, "packed_cpu_visible_compact_enabled", "false") == "true") packed_cpu_visible_compact_seen = 1
    if (ready_line != "") {
        measured_sample_count += 1
        if (packed_prepare_us > max_measured_packed_prepare_us) max_measured_packed_prepare_us = packed_prepare_us
        if (packed_view_prepare_us > max_measured_packed_view_prepare_us) max_measured_packed_view_prepare_us = packed_view_prepare_us
        if (packed_stream_us > max_measured_packed_stream_us) max_measured_packed_stream_us = packed_stream_us
        if (packed_build_task_us > max_measured_packed_build_task_us) max_measured_packed_build_task_us = packed_build_task_us
        if (packed_compaction_us > max_measured_packed_compaction_us) max_measured_packed_compaction_us = packed_compaction_us
        if (packed_render_node_us > max_measured_packed_render_node_us) max_measured_packed_render_node_us = packed_render_node_us
        if (packed_render_gpu_pass_us > max_measured_packed_render_gpu_pass_us) max_measured_packed_render_gpu_pass_us = packed_render_gpu_pass_us
        if (packed_gpu_cull_node_us > max_measured_packed_gpu_cull_node_us) max_measured_packed_gpu_cull_node_us = packed_gpu_cull_node_us
        if (packed_gpu_cull_input_commands > max_measured_packed_gpu_cull_input_commands) max_measured_packed_gpu_cull_input_commands = packed_gpu_cull_input_commands
        if (packed_gpu_cull_est_visible_commands > max_measured_packed_gpu_cull_est_visible_commands) max_measured_packed_gpu_cull_est_visible_commands = packed_gpu_cull_est_visible_commands
        if (packed_gpu_cull_est_visible_quads > max_measured_packed_gpu_cull_est_visible_quads) max_measured_packed_gpu_cull_est_visible_quads = packed_gpu_cull_est_visible_quads
        if (packed_cpu_visible_commands > max_measured_packed_cpu_visible_commands) max_measured_packed_cpu_visible_commands = packed_cpu_visible_commands
        if (packed_visible_quads > max_measured_packed_visible_quads) max_measured_packed_visible_quads = packed_visible_quads
        if (packed_uploaded_quads > max_measured_packed_uploaded_quads) max_measured_packed_uploaded_quads = packed_uploaded_quads
        if (packed_uploaded_this_frame > max_measured_packed_uploaded_this_frame) max_measured_packed_uploaded_this_frame = packed_uploaded_this_frame
        if (packed_pending_builds > max_measured_packed_pending_builds) max_measured_packed_pending_builds = packed_pending_builds
        if (packed_pending_region_rebuilds > max_measured_packed_pending_region_rebuilds) max_measured_packed_pending_region_rebuilds = packed_pending_region_rebuilds
        if (packed_arena_used_quads > max_measured_packed_arena_used_quads) max_measured_packed_arena_used_quads = packed_arena_used_quads
        if (packed_arena_slot_quads > max_measured_packed_arena_slot_quads) max_measured_packed_arena_slot_quads = packed_arena_slot_quads
        if (packed_chunk_ranges > max_measured_packed_chunk_ranges) max_measured_packed_chunk_ranges = packed_chunk_ranges
        if (packed_resident_ranges > max_measured_packed_resident_ranges) max_measured_packed_resident_ranges = packed_resident_ranges
        if (packed_tombstone_ranges > max_measured_packed_tombstone_ranges) max_measured_packed_tombstone_ranges = packed_tombstone_ranges
        if (packed_tombstone_capacity_quads > max_measured_packed_tombstone_capacity_quads) max_measured_packed_tombstone_capacity_quads = packed_tombstone_capacity_quads
        if (packed_dirty_ranges > max_measured_packed_dirty_ranges) max_measured_packed_dirty_ranges = packed_dirty_ranges
        if (packed_dirty_range_quads > max_measured_packed_dirty_range_quads) max_measured_packed_dirty_range_quads = packed_dirty_range_quads
        if (value($0, "packed_gpu_cull_enabled", "false") == "true") measured_packed_gpu_cull_enabled_seen = 1
        if (value($0, "packed_gpu_cull_count_supported", "false") == "true") measured_packed_gpu_cull_count_supported_seen = 1
        if (value($0, "packed_gpu_cull_compact_enabled", "false") == "true") measured_packed_gpu_cull_compact_enabled_seen = 1
        if (value($0, "packed_cpu_visible_compact_enabled", "false") == "true") measured_packed_cpu_visible_compact_seen = 1
    }
    row = "sample t=" value($0, "t", "?") " raw_fps=" value($0, "raw_fps", "?") " frame_ms=" frame_ms " bevy_delta_ms=" value($0, "bevy_delta_ms", "?") " frame_wall_us=" value($0, "frame_wall_us", "?") " frame_main_us=" value($0, "frame_main_us", "?") " frame_tail_us=" value($0, "frame_tail_us", "?") " render_schedule_us=" value($0, "render_schedule_us", "?") " render_camera_driver_us=" value($0, "render_camera_driver_us", "?") " render_gpu_camera_driver_us=" value($0, "render_gpu_camera_driver_us", "?") " render_gpu_camera_driver_raw_delta=" value($0, "render_gpu_camera_driver_raw_delta", "?") " render_gpu_camera_driver_readbacks=" value($0, "render_gpu_camera_driver_readbacks", "?") " render_gpu_camera_driver_zero_deltas=" value($0, "render_gpu_camera_driver_zero_deltas", "?") " render_gpu_camera_driver_map_failures=" value($0, "render_gpu_camera_driver_map_failures", "?") " render_gpu_camera_driver_pending_readback=" value($0, "render_gpu_camera_driver_pending_readback", "?") " render_graph_tail_us=" value($0, "render_graph_tail_us", "?") " render_core3d_us=" value($0, "render_core3d_us", "?") " render_render_us=" value($0, "render_render_us", "?") " render_before_render_system_us=" value($0, "render_before_render_system_us", "?") " render_system_us=" value($0, "render_system_us", "?") " render_system_tail_us=" value($0, "render_system_tail_us", "?") " render_prepare_us=" value($0, "render_prepare_us", "?") " render_prepare_resources_us=" value($0, "render_prepare_resources_us", "?") " render_prepare_view_uniforms_us=" value($0, "render_prepare_view_uniforms_us", "?") " render_prepare_core_depth_textures_us=" value($0, "render_prepare_core_depth_textures_us", "?") " render_prepare_core_transmission_textures_us=" value($0, "render_prepare_core_transmission_textures_us", "?") " render_prepare_prepass_textures_us=" value($0, "render_prepare_prepass_textures_us", "?") " render_prepare_resources_other_us=" value($0, "render_prepare_resources_other_us", "?") " render_prepare_resources_collect_us=" value($0, "render_prepare_resources_collect_us", "?") " render_prepare_resources_flush_us=" value($0, "render_prepare_resources_flush_us", "?") " render_prepare_bind_groups_us=" value($0, "render_prepare_bind_groups_us", "?") " render_prepare_after_bind_groups_us=" value($0, "render_prepare_after_bind_groups_us", "?") " render_queue_us=" value($0, "render_queue_us", "?") " render_manage_views_us=" value($0, "render_manage_views_us", "?") " render_prepare_windows_us=" value($0, "render_prepare_windows_us", "?") " gpu_frame_ts_req=" value($0, "render_gpu_frame_timestamps_requested", "?") " gpu_frame_ts_sup=" value($0, "render_gpu_frame_timestamps_supported", "?") " interval_worst_ms=" interval_worst_ms " ge16=" value($0, "interval_frames_ge_16ms", "?") " ge25=" value($0, "interval_frames_ge_25ms", "?") " ge33=" value($0, "interval_frames_ge_33ms", "?") " chunks=" value($0, "chunks", "?") " visible_quads=" value($0, "packed_visible_quads", "?") " uploaded_quads=" value($0, "packed_uploaded_quads", "?") " pending_builds=" value($0, "pending_builds", "?") " pending_region_rebuilds=" value($0, "pending_region_rebuilds", "?") " arena_used=" value($0, "arena_used_quads", "?") " arena_slot=" value($0, "arena_slot_quads", "?") " chunk_ranges=" value($0, "packed_chunk_ranges", "?") " resident_ranges=" value($0, "packed_resident_ranges", "?") " tombstone_ranges=" value($0, "packed_tombstone_ranges", "?") " tombstone_cap=" value($0, "packed_tombstone_capacity_quads", "?") " dirty_ranges=" value($0, "packed_dirty_ranges", "?") " dirty_quads=" value($0, "packed_dirty_range_quads", "?") " stream_us=" value($0, "packed_stream_us", "?") " build_us=" value($0, "packed_build_task_us", "?") " compaction_us=" value($0, "packed_compaction_us", "?") " render_us=" value($0, "packed_render_node_us", "?") " gpu_pass_us=" value($0, "packed_render_gpu_pass_us", "?") " cpu_cmds=" value($0, "packed_cpu_visible_commands", "?") " material_entities=" value($0, "packed_material_entities", "?") " material_sync_us=" value($0, "packed_material_sync_us", "?")
    append_row(row)
    next
}

/^profile slow_frame / {
    slow_count += 1
    frame_ms = value($0, "frame_ms", "0")
    if (frame_ms + 0 > max_slow_ms + 0) {
        max_slow_ms = frame_ms
        max_slow_line = $0
    }
    row = "slow t=" value($0, "t", "?") " frame_ms=" frame_ms " bevy_delta_ms=" value($0, "bevy_delta_ms", "?") " raw_fps=" value($0, "raw_fps", "?") " chunks=" value($0, "chunks", "?") " frame_wall_us=" value($0, "frame_wall_us", "?") " frame_main_us=" value($0, "frame_main_us", "?") " frame_tail_us=" value($0, "frame_tail_us", "?") " render_schedule_us=" value($0, "render_schedule_us", "?") " render_camera_driver_us=" value($0, "render_camera_driver_us", "?") " render_gpu_camera_driver_us=" value($0, "render_gpu_camera_driver_us", "?") " render_gpu_camera_driver_raw_delta=" value($0, "render_gpu_camera_driver_raw_delta", "?") " render_gpu_camera_driver_readbacks=" value($0, "render_gpu_camera_driver_readbacks", "?") " render_gpu_camera_driver_zero_deltas=" value($0, "render_gpu_camera_driver_zero_deltas", "?") " render_gpu_camera_driver_map_failures=" value($0, "render_gpu_camera_driver_map_failures", "?") " render_gpu_camera_driver_pending_readback=" value($0, "render_gpu_camera_driver_pending_readback", "?") " render_graph_tail_us=" value($0, "render_graph_tail_us", "?") " render_core3d_us=" value($0, "render_core3d_us", "?") " render_render_us=" value($0, "render_render_us", "?") " render_before_render_system_us=" value($0, "render_before_render_system_us", "?") " render_system_us=" value($0, "render_system_us", "?") " render_system_tail_us=" value($0, "render_system_tail_us", "?") " render_prepare_us=" value($0, "render_prepare_us", "?") " render_prepare_resources_us=" value($0, "render_prepare_resources_us", "?") " render_prepare_view_uniforms_us=" value($0, "render_prepare_view_uniforms_us", "?") " render_prepare_core_depth_textures_us=" value($0, "render_prepare_core_depth_textures_us", "?") " render_prepare_core_transmission_textures_us=" value($0, "render_prepare_core_transmission_textures_us", "?") " render_prepare_prepass_textures_us=" value($0, "render_prepare_prepass_textures_us", "?") " render_prepare_resources_other_us=" value($0, "render_prepare_resources_other_us", "?") " render_prepare_resources_collect_us=" value($0, "render_prepare_resources_collect_us", "?") " render_prepare_resources_flush_us=" value($0, "render_prepare_resources_flush_us", "?") " render_prepare_bind_groups_us=" value($0, "render_prepare_bind_groups_us", "?") " render_prepare_after_bind_groups_us=" value($0, "render_prepare_after_bind_groups_us", "?") " render_queue_us=" value($0, "render_queue_us", "?") " render_manage_views_us=" value($0, "render_manage_views_us", "?") " render_prepare_windows_us=" value($0, "render_prepare_windows_us", "?") " gpu_frame_ts_req=" value($0, "render_gpu_frame_timestamps_requested", "?") " gpu_frame_ts_sup=" value($0, "render_gpu_frame_timestamps_supported", "?") " visible_quads=" value($0, "packed_visible_quads", "?") " uploaded_quads=" value($0, "packed_uploaded_quads", "?") " pending_builds=" value($0, "pending_builds", "?") " pending_region_rebuilds=" value($0, "pending_region_rebuilds", "?") " prepare_us=" value($0, "prepare_us", "?") " view_prepare_us=" value($0, "view_prepare_us", "?") " stream_us=" value($0, "stream_us", "?") " build_us=" value($0, "build_task_us", "?") " compaction_us=" value($0, "compaction_us", "?") " uploaded_this_frame=" value($0, "uploaded_this_frame", "?") " render_us=" value($0, "render_node_us", "?") " gpu_pass_us=" value($0, "packed_render_gpu_pass_us", "?") " cpu_cmds=" value($0, "cpu_visible_commands", "?") " material_entities=" value($0, "material_entities", "?") " material_sync_us=" value($0, "material_sync_us", "?")
    append_slow(row)
    next
}

/^profile end / {
    end_line = $0
    next
}

/^profile worst_packed / {
    worst_packed_line = $0
    next
}

END {
    print "profile_log=" log_path
    if (fallback_from != "") {
        print "profile_log_fallback_from=" fallback_from
    }
    if (screenshot_path != "") {
        print "screenshot_visual_only=" screenshot_path
    }
    print "present_mode_fallback=" present_mode_fallback
    print "compute_parity=" compute_parity
    print "compute_lifecycle=" compute_lifecycle
    print "compute_edits=" compute_edits
    print "world_edits=" world_edits
    print "compute_direct=" compute_direct

    if (start_line == "" && end_line == "" && sample_count == 0) {
        print "profile_status=no_profile_lines_found"
        exit
    }

    if (start_line != "") {
        print "run render_mode=" value(start_line, "render_mode", "?") " render_target=" value(start_line, "render_target", "?") " headless_wait_ms=" value(start_line, "headless_wait_ms", "?") " gpu_frame_timestamps=" value(start_line, "gpu_frame_timestamps", "?") " present_mode=" value(start_line, "present_mode", "?") " frame_latency=" value(start_line, "frame_latency", "?") " window_size=" value(start_line, "window_size", "?") " shadows=" value(start_line, "shadows", "?") " debug_hud=" value(start_line, "debug_hud", "?") " duration=" value(start_line, "duration", "?") " warmup=" value(start_line, "warmup", "?") " measured_target=" value(start_line, "measured_target", "?") " ready_gate=" value(start_line, "ready_gate", "?") " ready_stable_frames=" value(start_line, "ready_stable_frames", "?") " ready_frame_ms=" value(start_line, "ready_frame_ms", "?") " ready_max_extra=" value(start_line, "ready_max_extra", "?") " autopilot=" value(start_line, "autopilot", "?") " autopilot_preroll=" value(start_line, "autopilot_preroll", "?") " slow_frame_ms=" value(start_line, "slow_frame_ms", "?")
    }

    if (ready_line != "") {
        print "ready t=" value(ready_line, "t", "?") " status=" value(ready_line, "status", "?") " stable_frames=" value(ready_line, "stable_frames", "?") " required_stable_frames=" value(ready_line, "required_stable_frames", "?") " frame_ms=" value(ready_line, "frame_ms", "?") " ready_frame_ms=" value(ready_line, "ready_frame_ms", "?") " measured_target=" value(ready_line, "measured_target", "?") " pending_builds=" value(ready_line, "pending_builds", "?") " pending_region_rebuilds=" value(ready_line, "pending_region_rebuilds", "?") " stream_spawned_builds=" value(ready_line, "stream_spawned_builds", "?") " stream_rebuild_regions=" value(ready_line, "stream_rebuild_regions", "?") " built_this_frame=" value(ready_line, "built_this_frame", "?") " uploaded_this_frame=" value(ready_line, "uploaded_this_frame", "?") " compacted_regions=" value(ready_line, "compacted_regions", "?")
    }

    if (end_line != "") {
        print "end samples=" value(end_line, "samples", "?") " measured_duration=" value(end_line, "measured_duration", "?") " ready_status=" value(end_line, "ready_status", "?") " ready_t=" value(end_line, "ready_t", "?") " measured_frames=" value(end_line, "measured_frames", "?") " avg_raw_fps=" value(end_line, "avg_raw_fps", "?") " min_raw_fps=" value(end_line, "min_raw_fps", "?") " worst_frame_ms=" value(end_line, "worst_frame_ms", "?") " worst_frame_t=" value(end_line, "worst_frame_t", "?") " worst_frame_wall_us=" value(end_line, "worst_frame_wall_us", "?") " worst_frame_main_us=" value(end_line, "worst_frame_main_us", "?") " worst_frame_tail_us=" value(end_line, "worst_frame_tail_us", "?") " worst_render_schedule_us=" value(end_line, "worst_render_schedule_us", "?") " worst_render_camera_driver_us=" value(end_line, "worst_render_camera_driver_us", "?") " worst_render_gpu_camera_driver_us=" value(end_line, "worst_render_gpu_camera_driver_us", "?") " worst_render_gpu_camera_driver_raw_delta=" value(end_line, "worst_render_gpu_camera_driver_raw_delta", "?") " worst_render_gpu_camera_driver_readbacks=" value(end_line, "worst_render_gpu_camera_driver_readbacks", "?") " worst_render_gpu_camera_driver_zero_deltas=" value(end_line, "worst_render_gpu_camera_driver_zero_deltas", "?") " worst_render_gpu_camera_driver_map_failures=" value(end_line, "worst_render_gpu_camera_driver_map_failures", "?") " worst_render_gpu_camera_driver_pending_readback=" value(end_line, "worst_render_gpu_camera_driver_pending_readback", "?") " worst_render_graph_tail_us=" value(end_line, "worst_render_graph_tail_us", "?") " worst_render_core3d_us=" value(end_line, "worst_render_core3d_us", "?") " worst_render_render_us=" value(end_line, "worst_render_render_us", "?") " worst_render_before_render_system_us=" value(end_line, "worst_render_before_render_system_us", "?") " worst_render_system_us=" value(end_line, "worst_render_system_us", "?") " worst_render_system_tail_us=" value(end_line, "worst_render_system_tail_us", "?") " worst_render_prepare_us=" value(end_line, "worst_render_prepare_us", "?") " worst_render_prepare_resources_us=" value(end_line, "worst_render_prepare_resources_us", "?") " worst_render_prepare_view_uniforms_us=" value(end_line, "worst_render_prepare_view_uniforms_us", "?") " worst_render_prepare_core_depth_textures_us=" value(end_line, "worst_render_prepare_core_depth_textures_us", "?") " worst_render_prepare_core_transmission_textures_us=" value(end_line, "worst_render_prepare_core_transmission_textures_us", "?") " worst_render_prepare_prepass_textures_us=" value(end_line, "worst_render_prepare_prepass_textures_us", "?") " worst_render_prepare_resources_other_us=" value(end_line, "worst_render_prepare_resources_other_us", "?") " worst_render_prepare_resources_collect_us=" value(end_line, "worst_render_prepare_resources_collect_us", "?") " worst_render_prepare_resources_flush_us=" value(end_line, "worst_render_prepare_resources_flush_us", "?") " worst_render_prepare_bind_groups_us=" value(end_line, "worst_render_prepare_bind_groups_us", "?") " worst_render_prepare_after_bind_groups_us=" value(end_line, "worst_render_prepare_after_bind_groups_us", "?") " worst_render_queue_us=" value(end_line, "worst_render_queue_us", "?") " worst_render_manage_views_us=" value(end_line, "worst_render_manage_views_us", "?") " worst_render_prepare_windows_us=" value(end_line, "worst_render_prepare_windows_us", "?") " gpu_frame_ts_req=" value(end_line, "worst_render_gpu_frame_timestamps_requested", "?") " gpu_frame_ts_sup=" value(end_line, "worst_render_gpu_frame_timestamps_supported", "?") " frames_ge_16ms=" value(end_line, "frames_ge_16ms", "?") " frames_ge_25ms=" value(end_line, "frames_ge_25ms", "?") " frames_ge_33ms=" value(end_line, "frames_ge_33ms", "?")
    }

    print "samples_seen=" sample_count " max_interval_worst_ms=" max_interval_ms " slow_frames_seen=" slow_count " max_slow_frame_ms=" max_slow_ms
    if (sample_count > 0) {
        print "packed_hotpath max_prepare_us=" max_packed_prepare_us " max_view_prepare_us=" max_packed_view_prepare_us " max_stream_us=" max_packed_stream_us " max_build_us=" max_packed_build_task_us " max_compaction_us=" max_packed_compaction_us " max_render_us=" max_packed_render_node_us " max_gpu_pass_us=" max_packed_render_gpu_pass_us " max_gpu_cull_us=" max_packed_gpu_cull_node_us " max_gpu_cull_input_cmds=" max_packed_gpu_cull_input_commands " max_gpu_cull_visible_cmds=" max_packed_gpu_cull_est_visible_commands " max_gpu_cull_visible_quads=" max_packed_gpu_cull_est_visible_quads " gpu_cull_enabled=" packed_gpu_cull_enabled_seen " gpu_cull_count_supported=" packed_gpu_cull_count_supported_seen " gpu_cull_compact_enabled=" packed_gpu_cull_compact_enabled_seen " cpu_visible_compact=" packed_cpu_visible_compact_seen " max_cpu_cmds=" max_packed_cpu_visible_commands " max_visible_quads=" max_packed_visible_quads " max_uploaded_quads=" max_packed_uploaded_quads " max_uploaded_this_frame=" max_packed_uploaded_this_frame " max_pending_builds=" max_packed_pending_builds " max_pending_region_rebuilds=" max_packed_pending_region_rebuilds " max_arena_used_quads=" max_packed_arena_used_quads " max_arena_slot_quads=" max_packed_arena_slot_quads " max_chunk_ranges=" max_packed_chunk_ranges " max_resident_ranges=" max_packed_resident_ranges " max_tombstone_ranges=" max_packed_tombstone_ranges " max_tombstone_capacity_quads=" max_packed_tombstone_capacity_quads " max_dirty_ranges=" max_packed_dirty_ranges " max_dirty_range_quads=" max_packed_dirty_range_quads
    }
    if (measured_sample_count > 0) {
        print "packed_hotpath_measured samples=" measured_sample_count " max_prepare_us=" max_measured_packed_prepare_us " max_view_prepare_us=" max_measured_packed_view_prepare_us " max_stream_us=" max_measured_packed_stream_us " max_build_us=" max_measured_packed_build_task_us " max_compaction_us=" max_measured_packed_compaction_us " max_render_us=" max_measured_packed_render_node_us " max_gpu_pass_us=" max_measured_packed_render_gpu_pass_us " max_gpu_cull_us=" max_measured_packed_gpu_cull_node_us " max_gpu_cull_input_cmds=" max_measured_packed_gpu_cull_input_commands " max_gpu_cull_visible_cmds=" max_measured_packed_gpu_cull_est_visible_commands " max_gpu_cull_visible_quads=" max_measured_packed_gpu_cull_est_visible_quads " gpu_cull_enabled=" measured_packed_gpu_cull_enabled_seen " gpu_cull_count_supported=" measured_packed_gpu_cull_count_supported_seen " gpu_cull_compact_enabled=" measured_packed_gpu_cull_compact_enabled_seen " cpu_visible_compact=" measured_packed_cpu_visible_compact_seen " max_cpu_cmds=" max_measured_packed_cpu_visible_commands " max_visible_quads=" max_measured_packed_visible_quads " max_uploaded_quads=" max_measured_packed_uploaded_quads " max_uploaded_this_frame=" max_measured_packed_uploaded_this_frame " max_pending_builds=" max_measured_packed_pending_builds " max_pending_region_rebuilds=" max_measured_packed_pending_region_rebuilds " max_arena_used_quads=" max_measured_packed_arena_used_quads " max_arena_slot_quads=" max_measured_packed_arena_slot_quads " max_chunk_ranges=" max_measured_packed_chunk_ranges " max_resident_ranges=" max_measured_packed_resident_ranges " max_tombstone_ranges=" max_measured_packed_tombstone_ranges " max_tombstone_capacity_quads=" max_measured_packed_tombstone_capacity_quads " max_dirty_ranges=" max_measured_packed_dirty_ranges " max_dirty_range_quads=" max_measured_packed_dirty_range_quads
    }

    if (worst_packed_line != "") {
        print "worst_packed visible_quads=" value(worst_packed_line, "visible_quads", "?") " uploaded_quads=" value(worst_packed_line, "uploaded_quads", "?") " pending_builds=" value(worst_packed_line, "pending_builds", "?") " pending_region_rebuilds=" value(worst_packed_line, "pending_region_rebuilds", "?") " prepare_us=" value(worst_packed_line, "prepare_us", "?") " view_prepare_us=" value(worst_packed_line, "view_prepare_us", "?") " stream_us=" value(worst_packed_line, "stream_us", "?") " build_us=" value(worst_packed_line, "build_task_us", "?") " compaction_us=" value(worst_packed_line, "compaction_us", "?") " uploaded_this_frame=" value(worst_packed_line, "uploaded_this_frame", "?") " render_us=" value(worst_packed_line, "render_node_us", "?") " gpu_pass_us=" value(worst_packed_line, "packed_render_gpu_pass_us", "?") " gpu_cull_us=" value(worst_packed_line, "gpu_cull_node_us", "?") " cpu_cmds=" value(worst_packed_line, "cpu_visible_commands", "?") " material_entities=" value(worst_packed_line, "material_entities", "?") " material_sync_us=" value(worst_packed_line, "material_sync_us", "?")
    }

    if (slow_rows != "") {
        print "slow_frames:"
        printf "%s", slow_rows
    }

    if (sample_rows != "") {
        print "sample_timeline:"
        printf "%s", sample_rows
    }
}
' "$profile_log"
