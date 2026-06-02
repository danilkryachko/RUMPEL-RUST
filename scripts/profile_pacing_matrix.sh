#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && /bin/pwd -P)"
repo_dir="$(cd -- "$script_dir/.." && /bin/pwd -P)"
cd "$repo_dir"

stamp="$(date +%Y%m%d_%H%M%S)"
matrix_dir="${RUMPEL_PACING_LOG_DIR:-$repo_dir/.ai_tasks/pacing_matrix_$stamp}"
mkdir -p "$matrix_dir"

present_modes="${RUMPEL_PACING_PRESENT_MODES:-auto-no-vsync immediate mailbox fifo-relaxed fifo}"
frame_latencies="${RUMPEL_PACING_FRAME_LATENCIES:-1 2 3 default}"
profile_seconds="${RUMPEL_PACING_PROFILE_SECONDS:-10}"
warmup_seconds="${RUMPEL_PACING_WARMUP_SECONDS:-4}"
measured_seconds="${RUMPEL_PACING_MEASURED_SECONDS:-}"
log_interval="${RUMPEL_PACING_LOG_INTERVAL:-2}"
slow_frame_ms="${RUMPEL_PACING_SLOW_FRAME_MS:-16}"
camera_lock="${RUMPEL_PACING_CAMERA_LOCK:-0}"
autopilot="${RUMPEL_PACING_AUTOPILOT:-1}"
prepare_only="${RUMPEL_PACING_PREPARE_ONLY:-0}"
launcher="${RUMPEL_PACING_LAUNCHER:-terminal}"
gui_launch_method="${RUMPEL_PACING_GUI_LAUNCH_METHOD:-auto}"
rust_log="${RUMPEL_PACING_RUST_LOG:-bevy_render::view::window=info,wgpu=error,bevy_asset=error}"
ready_gate="${RUMPEL_PACING_READY_GATE:-1}"
ready_stable_frames="${RUMPEL_PACING_READY_STABLE_FRAMES:-30}"
ready_frame_ms="${RUMPEL_PACING_READY_FRAME_MS:-25}"
ready_max_extra_seconds="${RUMPEL_PACING_READY_MAX_EXTRA_SECONDS:-8}"
autopilot_preroll_seconds="${RUMPEL_PACING_AUTOPILOT_PREROLL_SECONDS:-2}"
window_width="${RUMPEL_PACING_WINDOW_WIDTH:-}"
window_height="${RUMPEL_PACING_WINDOW_HEIGHT:-}"
shadows="${RUMPEL_PACING_SHADOWS:-}"
shadow_values="${RUMPEL_PACING_SHADOW_VALUES:-${shadows:-default}}"
debug_hud="${RUMPEL_PACING_DEBUG_HUD:-}"
debug_hud_values="${RUMPEL_PACING_DEBUG_HUD_VALUES:-${debug_hud:-default}}"
repeats="${RUMPEL_PACING_REPEATS:-1}"
summary_file="$matrix_dir/summary.txt"
rollup_file="$matrix_dir/rollup.txt"

case "$launcher" in
    terminal | app)
        ;;
    *)
        echo "unsupported RUMPEL_PACING_LAUNCHER='$launcher' (use terminal or app)" >&2
        exit 64
        ;;
esac

if [[ "${RUMPEL_CLIENT_SKIP_BUILD:-0}" != "1" ]]; then
    RUMPEL_CLIENT_BUILD_PROFILE=release cargo build -p rumpel_client --release
fi

if ! [[ "$repeats" =~ ^[0-9]+$ ]] || (( repeats < 1 )); then
    echo "RUMPEL_PACING_REPEATS must be a positive integer" >&2
    exit 64
fi

client_profile_seconds="$profile_seconds"
if [[ -n "$measured_seconds" ]]; then
    if ! [[ "$measured_seconds" =~ ^[0-9]+([.][0-9]+)?$ ]]; then
        echo "RUMPEL_PACING_MEASURED_SECONDS must be a non-negative number" >&2
        exit 64
    fi
    client_profile_seconds="$(awk -v warmup="$warmup_seconds" -v measured="$measured_seconds" 'BEGIN { printf "%.3f", warmup + measured }')"
fi

window_size="default"
if [[ -n "$window_width" || -n "$window_height" ]]; then
    if [[ -z "$window_width" || -z "$window_height" ]]; then
        echo "RUMPEL_PACING_WINDOW_WIDTH and RUMPEL_PACING_WINDOW_HEIGHT must be set together" >&2
        exit 64
    fi
    window_size="${window_width}x${window_height}"
fi

read_env_value() {
    local key="$1"
    local file="$2"
    awk -F= -v key="$key" '$1 == key { print substr($0, index($0, "=") + 1); exit }' "$file"
}

summary_value() {
    local file="$1"
    local key="$2"
    awk -v key="$key" '
        function value(line, key, fallback, marker, rest, parts) {
            marker = key "="
            if (index(line, marker) == 0) {
                return fallback
            }
            rest = substr(line, index(line, marker) + length(marker))
            split(rest, parts, " ")
            return parts[1]
        }
        $1 == "run" { run_line = $0 }
        $1 == "ready" { ready_line = $0 }
        index($0, "present_mode_fallback=") == 1 { fallback_line = $0 }
        $1 == "end" { end_line = $0 }
        $1 == "samples_seen=" || index($0, "samples_seen=") == 1 { samples_line = $0 }
        $1 == "worst_packed" { packed_line = $0 }
        END {
            if (key == "present_mode") print value(run_line, key, "?")
            else if (key == "frame_latency") print value(run_line, key, "?")
            else if (key == "shadows") print value(run_line, key, "?")
            else if (key == "debug_hud") print value(run_line, key, "?")
            else if (key == "window_size") print value(run_line, key, "?")
            else if (key == "present_mode_fallback") print value(fallback_line, key, "-")
            else if (key == "ready_status") {
                ready_status = value(ready_line, "status", "")
                if (ready_status == "") print value(end_line, key, "?")
                else print ready_status
            }
            else if (key == "ready_t") {
                ready_t = value(ready_line, "t", "")
                if (ready_t == "") print value(end_line, key, "?")
                else print ready_t
            }
            else if (key == "measured_duration") print value(end_line, key, "?")
            else if (key == "slow_frames_seen") print value(samples_line, key, "?")
            else if (key == "max_slow_frame_ms") print value(samples_line, key, "?")
            else if (key ~ /^packed_/) print value(packed_line, substr(key, 8), "?")
            else print value(end_line, key, "?")
        }
	    ' "$file"
}

write_rollup() {
    local input_file="$1"
    local output_file="$2"

    awk '
        function numeric(value, cleaned) {
            cleaned = value
            gsub(/s$/, "", cleaned)
            if (cleaned == "" || cleaned == "?" || cleaned == "-") {
                return 0
            }
            return cleaned + 0
        }
        function update_max(bucket, value) {
            return value > bucket ? value : bucket
        }
        $1 == "shadow" && $2 == "debug_hud" && $3 == "repeat" {
            in_rows = 1
            next
        }
        !in_rows || NF == 0 {
            next
        }
        {
            key = $1 SUBSEP $2 SUBSEP $4 SUBSEP $5 SUBSEP $6
            if (!seen[key]) {
                seen[key] = 1
                keys[++key_count] = key
            }

            row_count[key]++
            if ($22 != "ok") {
                non_ok_count[key]++
                next
            }

            ok_count[key]++
            if ($7 == "ready") {
                ready_count[key]++
            } else if ($7 == "timeout") {
                timeout_count[key]++
            }

            avg_fps = numeric($10)
            sum_avg_fps[key] += avg_fps
            if (ok_count[key] == 1 || avg_fps < min_avg_fps[key]) {
                min_avg_fps[key] = avg_fps
            }

            max_worst_ms[key] = update_max(max_worst_ms[key], numeric($11))
            sum_ge16[key] += numeric($12)
            sum_ge25[key] += numeric($13)
            max_tail_us[key] = update_max(max_tail_us[key], numeric($14))
            max_render_sched_us[key] = update_max(max_render_sched_us[key], numeric($15))
            max_manage_views_us[key] = update_max(max_manage_views_us[key], numeric($16))
            max_prep_window_us[key] = update_max(max_prep_window_us[key], numeric($17))
            max_core3d_us[key] = update_max(max_core3d_us[key], numeric($18))
            sum_slow[key] += numeric($19)
            max_packed_us[key] = update_max(max_packed_us[key], numeric($20))
            max_gpu_cull_us[key] = update_max(max_gpu_cull_us[key], numeric($21))
        }
        END {
            if (key_count == 0) {
                print "rollup_status=no_rows"
                exit
            }

            printf "%-8s %-9s %-15s %-13s %-18s %-6s %-4s %-7s %-6s %-8s %-11s %-11s %-11s %-10s %-10s %-10s %-10s %-17s %-16s %-15s %-15s %-12s %-13s\n",
                "shadow", "debug_hud", "present", "latency", "fallback", "rows", "ok", "non_ok", "ready", "timeout", "avg_min", "avg_avg", "worst_max", "ge16_sum", "ge25_sum", "slow_sum", "tail_max", "render_sched_max", "manage_views_max", "prep_window_max", "core3d_max", "packed_max", "gpu_cull_max"

            for (i = 1; i <= key_count; i++) {
                key = keys[i]
                split(key, fields, SUBSEP)
                if (ok_count[key] == 0) {
                    printf "%-8s %-9s %-15s %-13s %-18s %-6d %-4d %-7d %-6d %-8d %-11s %-11s %-11s %-10s %-10s %-10s %-10s %-17s %-16s %-15s %-15s %-12s %-13s\n",
                        fields[1], fields[2], fields[3], fields[4], fields[5],
                        row_count[key], 0, non_ok_count[key], 0, 0,
                        "?", "?", "?", "?", "?", "?", "?", "?", "?", "?", "?", "?", "?"
                    continue
                }

                printf "%-8s %-9s %-15s %-13s %-18s %-6d %-4d %-7d %-6d %-8d %-11.1f %-11.1f %-11.2f %-10d %-10d %-10d %-10d %-17d %-16d %-15d %-15d %-12d %-13d\n",
                    fields[1], fields[2], fields[3], fields[4], fields[5],
                    row_count[key], ok_count[key], non_ok_count[key],
                    ready_count[key], timeout_count[key],
                    min_avg_fps[key], sum_avg_fps[key] / ok_count[key], max_worst_ms[key],
                    sum_ge16[key], sum_ge25[key], sum_slow[key], max_tail_us[key],
                    max_render_sched_us[key], max_manage_views_us[key], max_prep_window_us[key],
                    max_core3d_us[key], max_packed_us[key], max_gpu_cull_us[key]
            }
        }
    ' "$input_file" > "$output_file"
}

{
    echo "pacing_matrix_dir=$matrix_dir"
    echo "profile_seconds=$profile_seconds warmup_seconds=$warmup_seconds measured_seconds=${measured_seconds:-duration-minus-warmup} client_profile_seconds=$client_profile_seconds log_interval=$log_interval slow_frame_ms=$slow_frame_ms camera_lock=$camera_lock autopilot=$autopilot launcher=$launcher gui_launch_method=$gui_launch_method rust_log=$rust_log ready_gate=$ready_gate ready_stable_frames=$ready_stable_frames ready_frame_ms=$ready_frame_ms ready_max_extra_seconds=$ready_max_extra_seconds autopilot_preroll_seconds=$autopilot_preroll_seconds window_size=$window_size shadow_values=$shadow_values debug_hud_values=$debug_hud_values repeats=$repeats"
    echo "present_modes=$present_modes"
    echo "frame_latencies=$frame_latencies"
    echo
    printf '%-8s %-9s %-8s %-15s %-13s %-18s %-8s %-8s %-8s %-11s %-14s %-13s %-13s %-17s %-16s %-15s %-15s %-15s %-11s %-12s %-13s %-10s\n' \
        "shadow" "debug_hud" "repeat" "present" "latency" "fallback" "ready" "ready_t" "meas_s" "avg_fps" "worst_ms" "ge16" "ge25" "tail_us" "render_sched_us" "manage_views_us" "prep_window_us" "core3d_us" "slow" "packed_us" "gpu_cull_us" "status"
} > "$summary_file"

for shadow_value in $shadow_values; do
    shadow_env="$shadow_value"
    if [[ "$shadow_value" == "default" || "$shadow_value" == "auto" ]]; then
        shadow_env=""
    fi
    for debug_hud_value in $debug_hud_values; do
        debug_hud_env="$debug_hud_value"
        if [[ "$debug_hud_value" == "default" || "$debug_hud_value" == "auto" ]]; then
            debug_hud_env=""
        fi
    for present_mode in $present_modes; do
        for frame_latency in $frame_latencies; do
            repeat_index=1
            while (( repeat_index <= repeats )); do
        run_name="${present_mode}_${frame_latency}_shadows-${shadow_value}_hud-${debug_hud_value}_r${repeat_index}"
        prepare_log="$matrix_dir/prepare_${run_name}.log"
        launch_log="$matrix_dir/launch_${run_name}.log"
        run_status="ok"

        if [[ "$launcher" == "terminal" ]]; then
            if [[ "$prepare_only" == "1" ]]; then
                env \
                    RUMPEL_CLIENT_BUILD_PROFILE=release \
                    RUMPEL_CLIENT_SKIP_BUILD=1 \
                    RUMPEL_CLIENT_LOG_DIR="$matrix_dir" \
                    RUMPEL_RENDER_MODE=packed \
                    RUST_LOG="$rust_log" \
                    RUMPEL_PRESENT_MODE="$present_mode" \
                    RUMPEL_FRAME_LATENCY="$frame_latency" \
                    RUMPEL_PROFILE_SECONDS="$client_profile_seconds" \
                    RUMPEL_PROFILE_WARMUP_SECONDS="$warmup_seconds" \
                    RUMPEL_PROFILE_READY_GATE="$ready_gate" \
                    RUMPEL_PROFILE_READY_STABLE_FRAMES="$ready_stable_frames" \
                    RUMPEL_PROFILE_READY_FRAME_MS="$ready_frame_ms" \
                    RUMPEL_PROFILE_READY_MAX_EXTRA_SECONDS="$ready_max_extra_seconds" \
                    RUMPEL_PROFILE_AUTOPILOT_PREROLL_SECONDS="$autopilot_preroll_seconds" \
                    RUMPEL_PROFILE_AUTOPILOT="$autopilot" \
                    RUMPEL_PROFILE_LOG_INTERVAL="$log_interval" \
                    RUMPEL_PROFILE_SLOW_FRAME_MS="$slow_frame_ms" \
                    RUMPEL_WINDOW_WIDTH="$window_width" \
                    RUMPEL_WINDOW_HEIGHT="$window_height" \
                    RUMPEL_SHADOWS="$shadow_env" \
                    RUMPEL_DEBUG_HUD="$debug_hud_env" \
                    RUMPEL_CAMERA_LOCK="$camera_lock" \
                    RUMPEL_GUI_WAIT=1 \
                    RUMPEL_GUI_TERMINAL_AUTO_CLOSE=1 \
                    RUMPEL_GUI_LAUNCH_METHOD="$gui_launch_method" \
                    RUMPEL_GUI_PREPARE_ONLY=1 \
                    "$repo_dir/scripts/run_client_macos_gui.sh" packed >"$prepare_log"
                run_status="prepared"
            elif ! env \
                RUMPEL_CLIENT_BUILD_PROFILE=release \
                RUMPEL_CLIENT_SKIP_BUILD=1 \
                RUMPEL_CLIENT_LOG_DIR="$matrix_dir" \
                RUMPEL_RENDER_MODE=packed \
                RUST_LOG="$rust_log" \
                RUMPEL_PRESENT_MODE="$present_mode" \
                RUMPEL_FRAME_LATENCY="$frame_latency" \
                RUMPEL_PROFILE_SECONDS="$client_profile_seconds" \
                RUMPEL_PROFILE_WARMUP_SECONDS="$warmup_seconds" \
                RUMPEL_PROFILE_READY_GATE="$ready_gate" \
                RUMPEL_PROFILE_READY_STABLE_FRAMES="$ready_stable_frames" \
                RUMPEL_PROFILE_READY_FRAME_MS="$ready_frame_ms" \
                RUMPEL_PROFILE_READY_MAX_EXTRA_SECONDS="$ready_max_extra_seconds" \
                RUMPEL_PROFILE_AUTOPILOT_PREROLL_SECONDS="$autopilot_preroll_seconds" \
                RUMPEL_PROFILE_AUTOPILOT="$autopilot" \
                RUMPEL_PROFILE_LOG_INTERVAL="$log_interval" \
                RUMPEL_PROFILE_SLOW_FRAME_MS="$slow_frame_ms" \
                RUMPEL_WINDOW_WIDTH="$window_width" \
                RUMPEL_WINDOW_HEIGHT="$window_height" \
                RUMPEL_SHADOWS="$shadow_env" \
                RUMPEL_DEBUG_HUD="$debug_hud_env" \
                RUMPEL_CAMERA_LOCK="$camera_lock" \
                RUMPEL_GUI_WAIT=1 \
                RUMPEL_GUI_TERMINAL_AUTO_CLOSE=1 \
                RUMPEL_GUI_LAUNCH_METHOD="$gui_launch_method" \
                "$repo_dir/scripts/run_client_macos_gui.sh" packed >"$launch_log" 2>&1; then
                run_status="launch_failed"
            fi
            state_file="$matrix_dir/last_gui_run.env"
        else
            env \
                RUMPEL_CLIENT_BUILD_PROFILE=release \
                RUMPEL_CLIENT_SKIP_BUILD=1 \
                RUMPEL_CLIENT_LOG_DIR="$matrix_dir" \
                RUMPEL_RENDER_MODE=packed \
                RUST_LOG="$rust_log" \
                RUMPEL_PRESENT_MODE="$present_mode" \
                RUMPEL_FRAME_LATENCY="$frame_latency" \
                RUMPEL_PROFILE_SECONDS="$client_profile_seconds" \
                RUMPEL_PROFILE_WARMUP_SECONDS="$warmup_seconds" \
                RUMPEL_PROFILE_READY_GATE="$ready_gate" \
                RUMPEL_PROFILE_READY_STABLE_FRAMES="$ready_stable_frames" \
                RUMPEL_PROFILE_READY_FRAME_MS="$ready_frame_ms" \
                RUMPEL_PROFILE_READY_MAX_EXTRA_SECONDS="$ready_max_extra_seconds" \
                RUMPEL_PROFILE_AUTOPILOT_PREROLL_SECONDS="$autopilot_preroll_seconds" \
                RUMPEL_PROFILE_AUTOPILOT="$autopilot" \
                RUMPEL_PROFILE_LOG_INTERVAL="$log_interval" \
                RUMPEL_PROFILE_SLOW_FRAME_MS="$slow_frame_ms" \
                RUMPEL_WINDOW_WIDTH="$window_width" \
                RUMPEL_WINDOW_HEIGHT="$window_height" \
                RUMPEL_SHADOWS="$shadow_env" \
                RUMPEL_DEBUG_HUD="$debug_hud_env" \
                RUMPEL_CAMERA_LOCK="$camera_lock" \
                "$repo_dir/scripts/prepare_client_macos_app.sh" >"$prepare_log"

            state_file="$matrix_dir/last_macos_app.env"
            run_script="$(read_env_value RUN_SCRIPT "$state_file")"
            if [[ "$prepare_only" == "1" ]]; then
                run_status="prepared"
            elif ! "$run_script" >"$launch_log" 2>&1; then
                run_status="launch_failed"
            fi
        fi

        stdout_log="$(read_env_value STDOUT_LOG "$state_file")"
        stderr_log="$(read_env_value STDERR_LOG "$state_file")"
        run_summary="$matrix_dir/summary_${run_name}.txt"

        if [[ "$run_status" == "ok" ]] && "$repo_dir/scripts/summarize_profile_log.sh" "$stdout_log" >"$run_summary"; then
            printf '%-8s %-9s %-8s %-15s %-13s %-18s %-8s %-8s %-8s %-11s %-14s %-13s %-13s %-17s %-16s %-15s %-15s %-15s %-11s %-12s %-13s %-10s\n' \
                "$(summary_value "$run_summary" shadows)" \
                "$(summary_value "$run_summary" debug_hud)" \
                "$repeat_index" \
                "$(summary_value "$run_summary" present_mode)" \
                "$(summary_value "$run_summary" frame_latency)" \
                "$(summary_value "$run_summary" present_mode_fallback)" \
                "$(summary_value "$run_summary" ready_status)" \
                "$(summary_value "$run_summary" ready_t)" \
                "$(summary_value "$run_summary" measured_duration)" \
                "$(summary_value "$run_summary" avg_raw_fps)" \
                "$(summary_value "$run_summary" worst_frame_ms)" \
                "$(summary_value "$run_summary" frames_ge_16ms)" \
                "$(summary_value "$run_summary" frames_ge_25ms)" \
                "$(summary_value "$run_summary" worst_frame_tail_us)" \
                "$(summary_value "$run_summary" worst_render_schedule_us)" \
                "$(summary_value "$run_summary" worst_render_manage_views_us)" \
                "$(summary_value "$run_summary" worst_render_prepare_windows_us)" \
                "$(summary_value "$run_summary" worst_render_core3d_us)" \
                "$(summary_value "$run_summary" slow_frames_seen)" \
                "$(summary_value "$run_summary" packed_render_us)" \
                "$(summary_value "$run_summary" packed_gpu_cull_us)" \
                "$run_status" >> "$summary_file"
        else
            printf '%-8s %-9s %-8s %-15s %-13s %-18s %-8s %-8s %-8s %-11s %-14s %-13s %-13s %-17s %-16s %-15s %-15s %-15s %-11s %-12s %-13s %-10s\n' \
                "$shadow_value" "$debug_hud_value" "$repeat_index" "$present_mode" "$frame_latency" "-" "?" "?" "?" "?" "?" "?" "?" "?" "?" "?" "?" "?" "?" "?" "?" "$run_status" >> "$summary_file"
            if [[ -f "$stderr_log" ]]; then
                tail -80 "$stderr_log" > "$matrix_dir/stderr_tail_${run_name}.log" || true
            fi
        fi
                repeat_index=$((repeat_index + 1))
            done
        done
    done
    done
done

write_rollup "$summary_file" "$rollup_file"
cat "$summary_file"
echo
cat "$rollup_file"
