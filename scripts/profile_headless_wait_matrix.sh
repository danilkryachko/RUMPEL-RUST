#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && /bin/pwd -P)"
repo_dir="$(cd -- "$script_dir/.." && /bin/pwd -P)"
cd "$repo_dir"

stamp="$(date +%Y%m%d_%H%M%S)"
matrix_dir="${RUMPEL_HEADLESS_WAIT_MATRIX_DIR:-$repo_dir/.ai_tasks/headless_wait_matrix_$stamp}"
wait_values="${RUMPEL_HEADLESS_WAIT_MATRIX:-0 1 2}"
mkdir -p "$matrix_dir"

summary_file="$matrix_dir/summary.txt"
printf 'matrix_dir=%s\n' "$matrix_dir" > "$summary_file"
printf 'waits=%s\n' "$wait_values" >> "$summary_file"
printf 'wait_ms avg_raw_fps worst_frame_ms frames_ge_16ms frames_ge_25ms worst_prepare_us worst_prepare_resources_us worst_prepare_view_uniforms_us worst_prepare_core_depth_textures_us worst_prepare_core_transmission_textures_us worst_prepare_prepass_textures_us worst_prepare_resources_other_us worst_prepare_resources_flush_us worst_before_render_system_us worst_render_system_us worst_graph_tail_us worst_camera_driver_us worst_core3d_us worst_render_render_us packed_render_us log\n' >> "$summary_file"

first_run=1
for wait_ms in $wait_values; do
    wait_label="${wait_ms//./_}"
    run_dir="$matrix_dir/wait_${wait_label}ms"
    mkdir -p "$run_dir"

    skip_build="${RUMPEL_CLIENT_SKIP_BUILD:-}"
    if [[ -z "$skip_build" ]]; then
        if [[ "$first_run" == "1" ]]; then
            skip_build=0
        else
            skip_build=1
        fi
    fi

    first_run=0
    run_summary="$run_dir/summary.txt"

    if ! RUMPEL_CLIENT_LOG_DIR="$run_dir" \
        RUMPEL_HEADLESS_WAIT_MS="$wait_ms" \
        RUMPEL_CLIENT_SKIP_BUILD="$skip_build" \
        RUMPEL_PROFILE_SECONDS="${RUMPEL_PROFILE_SECONDS:-8}" \
        RUMPEL_PROFILE_WARMUP_SECONDS="${RUMPEL_PROFILE_WARMUP_SECONDS:-4}" \
        RUMPEL_PACKED_GPU_TIMESTAMPS="${RUMPEL_PACKED_GPU_TIMESTAMPS:-0}" \
        "$repo_dir/scripts/profile_packed_headless.sh" > "$run_summary"; then
        printf '%s failed failed failed failed failed failed failed failed failed failed failed failed failed failed failed failed failed failed failed %s\n' "$wait_ms" "$run_summary" >> "$summary_file"
        continue
    fi

    awk -v wait_ms="$wait_ms" -v summary_path="$run_summary" '
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

/^profile_log=/ {
    log_path = substr($0, index($0, "=") + 1)
}

/^end / {
    end_line = $0
}

/^worst_packed / {
    worst_packed_line = $0
}

END {
    if (end_line == "") {
        printf "%s missing missing missing missing missing missing missing missing missing missing missing missing missing missing missing missing missing missing missing %s\n", wait_ms, summary_path
        exit
    }
    printf "%s %s %s %s %s %s %s %s %s %s %s %s %s %s %s %s %s %s %s %s %s\n",
        wait_ms,
        value(end_line, "avg_raw_fps", "?"),
        value(end_line, "worst_frame_ms", "?"),
        value(end_line, "frames_ge_16ms", "?"),
        value(end_line, "frames_ge_25ms", "?"),
        value(end_line, "worst_render_prepare_us", "?"),
        value(end_line, "worst_render_prepare_resources_us", "?"),
        value(end_line, "worst_render_prepare_view_uniforms_us", "?"),
        value(end_line, "worst_render_prepare_core_depth_textures_us", "?"),
        value(end_line, "worst_render_prepare_core_transmission_textures_us", "?"),
        value(end_line, "worst_render_prepare_prepass_textures_us", "?"),
        value(end_line, "worst_render_prepare_resources_other_us", "?"),
        value(end_line, "worst_render_prepare_resources_flush_us", "?"),
        value(end_line, "worst_render_before_render_system_us", "?"),
        value(end_line, "worst_render_system_us", "?"),
        value(end_line, "worst_render_graph_tail_us", "?"),
        value(end_line, "worst_render_camera_driver_us", "?"),
        value(end_line, "worst_render_core3d_us", "?"),
        value(end_line, "worst_render_render_us", "?"),
        value(worst_packed_line, "render_us", "?"),
        log_path
}
' "$run_summary" >> "$summary_file"
done

cat "$summary_file"
