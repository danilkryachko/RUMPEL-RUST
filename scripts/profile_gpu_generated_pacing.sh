#!/usr/bin/env bash
# Compare present/latency for gpu-generated packed GUI profile (ADR-003 window baseline).
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && /bin/pwd -P)"
repo_dir="$(cd -- "$script_dir/.." && /bin/pwd -P)"
cd "$repo_dir"

stamp="$(date +%Y%m%d_%H%M%S)"
out_dir="${RUMPEL_GPU_GENERATED_PACING_DIR:-$repo_dir/.ai_tasks/gpu_generated_pacing_$stamp}"
mkdir -p "$out_dir"

present_modes="${RUMPEL_GPU_GENERATED_PACING_PRESENT_MODES:-immediate auto-no-vsync fifo-relaxed}"
frame_latencies="${RUMPEL_GPU_GENERATED_PACING_FRAME_LATENCIES:-1 default}"

if [[ "${RUMPEL_CLIENT_SKIP_BUILD:-0}" != "1" ]]; then
    RUMPEL_CLIENT_BUILD_PROFILE=release cargo build -p rumpel_client --release
fi

summary="$out_dir/summary.txt"
{
    echo "gpu_generated_pacing_dir=$out_dir"
    echo "present_modes=$present_modes"
    echo "frame_latencies=$frame_latencies"
    echo
    printf '%-15s %-10s %-10s %-10s %-10s %-12s %-10s\n' \
        "present" "latency" "avg_raw" "ge25" "worst_ms" "prep_win_us" "status"
} >"$summary"

for present_mode in $present_modes; do
    for frame_latency in $frame_latencies; do
        run_name="${present_mode}_${frame_latency}"
        launch_log="$out_dir/launch_${run_name}.log"
        run_status="ok"
        if ! env \
            RUMPEL_CLIENT_BUILD_PROFILE=release \
            RUMPEL_CLIENT_SKIP_BUILD=1 \
            RUMPEL_CLIENT_LOG_DIR="$out_dir" \
            RUST_LOG=info,wgpu=error,bevy_asset=error \
            RUMPEL_RENDER_MODE=packed \
            RUMPEL_PACKED_VIEW_RADIUS=16 \
            RUMPEL_PACKED_GPU_GENERATION=1 \
            RUMPEL_PACKED_GPU_CULL=1 \
            RUMPEL_PRESENT_MODE="$present_mode" \
            RUMPEL_FRAME_LATENCY="$frame_latency" \
            RUMPEL_PROFILE_SECONDS=14 \
            RUMPEL_PROFILE_WARMUP_SECONDS=6 \
            RUMPEL_PROFILE_SETTLE_SECONDS=2 \
            RUMPEL_PROFILE_READY_GATE=1 \
            RUMPEL_PROFILE_AUTOPILOT=1 \
            RUMPEL_PROFILE_LOG_INTERVAL=1 \
            RUMPEL_CAMERA_LOCK=0 \
            RUMPEL_GUI_WAIT=1 \
            RUMPEL_GUI_TERMINAL_AUTO_CLOSE=1 \
            "$repo_dir/scripts/run_client_macos_gui.sh" packed >"$launch_log" 2>&1; then
            run_status="launch_failed"
        fi

        stdout_path=""
        if [[ -f "$out_dir/last_gui_run.env" ]]; then
            stdout_path="$(awk -F= '$1=="STDOUT_LOG"{print $2; exit}' "$out_dir/last_gui_run.env")"
        fi
        avg_raw="?"
        ge25="?"
        worst_ms="?"
        prep_win="?"
        if [[ -n "$stdout_path" && -f "$stdout_path" ]]; then
            end_line="$(grep '^profile end ' "$stdout_path" | tail -1 || true)"
            if [[ -n "$end_line" ]]; then
                avg_raw="$(echo "$end_line" | sed -n 's/.*avg_raw_fps=\([^ ]*\).*/\1/p')"
                ge25="$(echo "$end_line" | sed -n 's/.*frames_ge_25ms=\([^ ]*\).*/\1/p')"
                worst_ms="$(echo "$end_line" | sed -n 's/.*worst_frame_ms=\([^ ]*\).*/\1/p')"
                prep_win="$(echo "$end_line" | sed -n 's/.*worst_render_prepare_windows_us=\([^ ]*\).*/\1/p')"
            fi
        fi
        printf '%-15s %-10s %-10s %-10s %-10s %-12s %-10s\n' \
            "$present_mode" "$frame_latency" "$avg_raw" "$ge25" "$worst_ms" "$prep_win" "$run_status" >>"$summary"
        echo "run present=$present_mode latency=$frame_latency status=$run_status avg_raw_fps=$avg_raw ge25=$ge25" >>"$summary"
    done
done

echo "Wrote $summary"
cat "$summary"
