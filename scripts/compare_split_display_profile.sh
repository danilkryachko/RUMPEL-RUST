#!/usr/bin/env bash
# A/B GUI profile: monolithic window present vs split display (Method A).
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && /bin/pwd -P)"
repo_dir="$(cd -- "$script_dir/.." && /bin/pwd -P)"
cd "$repo_dir"

stamp="$(date +%Y%m%d_%H%M%S)"
compare_dir="${RUMPEL_SPLIT_DISPLAY_COMPARE_DIR:-$repo_dir/.ai_tasks/split_display_compare_$stamp}"
mkdir -p "$compare_dir"

echo "==> building release client for split display compare"
cargo build -p rumpel_client --release

profile_seconds="${RUMPEL_PROFILE_SECONDS:-10}"
warmup_seconds="${RUMPEL_PROFILE_WARMUP_SECONDS:-4}"

common_env=(
    RUST_LOG=info,wgpu=error,bevy_asset=error
    RUMPEL_CLIENT_BUILD_PROFILE="${RUMPEL_CLIENT_BUILD_PROFILE:-release}"
    RUMPEL_RENDER_MODE=packed
    RUMPEL_PACKED_VIEW_RADIUS=16
    RUMPEL_PRESENT_MODE=immediate
    RUMPEL_FRAME_LATENCY=1
    RUMPEL_PROFILE_SECONDS="$profile_seconds"
    RUMPEL_PROFILE_WARMUP_SECONDS="$warmup_seconds"
    RUMPEL_PROFILE_READY_GATE=1
    RUMPEL_PROFILE_AUTOPILOT=1
    RUMPEL_PROFILE_LOG_INTERVAL=1
    RUMPEL_CAMERA_LOCK=0
    RUMPEL_GUI_WAIT=1
    RUMPEL_GUI_TERMINAL_AUTO_CLOSE=1
)

extract_profile_field() {
    local log="$1"
    local prefix="$2"
    local field="$3"
    awk -v prefix="$prefix" -v field="$field" '
        $0 ~ ("^" prefix) {
            for (i = 1; i <= NF; i++) {
                split($i, kv, "=");
                if (kv[1] == field) {
                    print kv[2];
                    exit;
                }
            }
        }
    ' "$log"
}

run_variant() {
    local tag="$1"
    shift
    local state_file="$compare_dir/${tag}.env"
    mkdir -p "$(dirname "$state_file")"
    echo "==> split display compare: $tag"
    env \
        "${common_env[@]}" \
        RUMPEL_CLIENT_SKIP_BUILD=1 \
        RUMPEL_PROFILE_STATE_FILE="$state_file" \
        "$@" \
        "$repo_dir/scripts/run_client_macos_gui.sh" packed
    local stdout_log
    stdout_log="$(awk -F= '$1 == "STDOUT_LOG" { print substr($0, index($0, "=") + 1); exit }' "$state_file")"
    if [[ ! -f "$stdout_log" ]]; then
        echo "missing profile log for $tag: $stdout_log" >&2
        exit 66
    fi
    "$repo_dir/scripts/summarize_profile_log.sh" "$state_file" > "$compare_dir/${tag}.summary.txt"
    printf '%s\n' "$stdout_log" > "$compare_dir/${tag}.stdout.path"
}

run_variant baseline
run_variant split RUMPEL_SPLIT_DISPLAY=1

baseline_log="$(cat "$compare_dir/baseline.stdout.path")"
split_log="$(cat "$compare_dir/split.stdout.path")"

split_render_target="$(extract_profile_field "$split_log" "profile start" "render_target")"
if [[ "$split_render_target" != "split_display" ]]; then
    echo "split variant did not enable Method A (render_target=$split_render_target); expected split_display" >&2
    exit 66
fi

report="$compare_dir/report.txt"
{
    printf 'split_display_compare_dir=%s\n' "$compare_dir"
    printf 'baseline_log=%s\n' "$baseline_log"
    printf 'split_log=%s\n' "$split_log"
    printf '\n'
    printf 'render_target baseline=%s split=%s\n' \
        "$(extract_profile_field "$baseline_log" "profile start" "render_target")" \
        "$(extract_profile_field "$split_log" "profile start" "render_target")"
    for field in ready_status avg_raw_fps worst_frame_ms frames_ge_16ms frames_ge_25ms worst_render_prepare_windows_us worst_render_core3d_us worst_render_render_us; do
        baseline_value="$(extract_profile_field "$baseline_log" "profile end" "$field")"
        split_value="$(extract_profile_field "$split_log" "profile end" "$field")"
        printf '%s baseline=%s split=%s\n' "$field" "${baseline_value:--}" "${split_value:--}"
    done
} | tee "$report"

echo
echo "Summaries:"
echo "  $compare_dir/baseline.summary.txt"
echo "  $compare_dir/split.summary.txt"
echo "Report: $report"
