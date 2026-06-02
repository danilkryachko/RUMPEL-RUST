#!/usr/bin/env bash
# Multi-camera visual/profile compare for CPU-built packed quads vs GPU-generated packed quads.
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && /bin/pwd -P)"
repo_dir="$(cd -- "$script_dir/.." && /bin/pwd -P)"
cd "$repo_dir"

stamp="$(date +%Y%m%d_%H%M%S)"
compare_dir="${RUMPEL_GENERATED_VISUAL_COMPARE_DIR:-$repo_dir/.ai_tasks/generated_visual_compare_$stamp}"
mkdir -p "$compare_dir"

profile_seconds="${RUMPEL_GENERATED_VISUAL_PROFILE_SECONDS:-${RUMPEL_PROFILE_SECONDS:-5}}"
warmup_seconds="${RUMPEL_GENERATED_VISUAL_WARMUP_SECONDS:-${RUMPEL_PROFILE_WARMUP_SECONDS:-2}}"
capture_delay="${RUMPEL_GENERATED_VISUAL_CAPTURE_DELAY:-8}"
view_radius="${RUMPEL_GENERATED_VISUAL_VIEW_RADIUS:-16}"
region_radius="${RUMPEL_GENERATED_VISUAL_REGION_RADIUS:-1}"
presets="${RUMPEL_GENERATED_VISUAL_PRESETS:-horizon ridge beach}"

if [[ "${RUMPEL_CLIENT_SKIP_BUILD:-0}" != "1" ]]; then
    echo "==> building release client for generated visual compare"
    cargo build -p rumpel_client --release
fi

extract_env_value() {
    local key="$1"
    local file="$2"
    awk -F= -v key="$key" '$1 == key { print substr($0, index($0, "=") + 1); exit }' "$file"
}

extract_profile_field() {
    local log="$1"
    local prefix="$2"
    local field="$3"
    awk -v prefix="$prefix" -v field="$field" '
        $0 ~ ("^" prefix) {
            for (i = 1; i <= NF; i += 1) {
                split($i, kv, "=");
                if (kv[1] == field) {
                    print kv[2];
                    exit;
                }
            }
        }
    ' "$log"
}

require_positive_int() {
    local value="$1"
    local label="$2"
    local preset="$3"
    local variant="$4"
    if [[ ! "$value" =~ ^[0-9]+$ ]]; then
        echo "$preset/$variant expected numeric $label, got '${value:-missing}'" >&2
        exit 66
    fi
    if (( value <= 0 )); then
        echo "$preset/$variant expected positive $label, got '$value'" >&2
        exit 66
    fi
}

camera_env_for_preset() {
    local preset="$1"
    case "$preset" in
        horizon)
            printf '%s\n' \
                RUMPEL_CAMERA_START_X=0 \
                RUMPEL_CAMERA_START_Z=0 \
                RUMPEL_CAMERA_CLEARANCE=56 \
                RUMPEL_CAMERA_PITCH_RADIANS=-0.36 \
                RUMPEL_CAMERA_YAW_RADIANS=0
            ;;
        ridge)
            printf '%s\n' \
                RUMPEL_CAMERA_START_X=512 \
                RUMPEL_CAMERA_START_Z=384 \
                RUMPEL_CAMERA_CLEARANCE=70 \
                RUMPEL_CAMERA_PITCH_RADIANS=-0.45 \
                RUMPEL_CAMERA_YAW_RADIANS=0.8
            ;;
        beach)
            printf '%s\n' \
                RUMPEL_CAMERA_START_X=-179 \
                RUMPEL_CAMERA_START_Z=-512 \
                RUMPEL_CAMERA_CLEARANCE=44 \
                RUMPEL_CAMERA_PITCH_RADIANS=-0.38 \
                RUMPEL_CAMERA_YAW_RADIANS=1.2
            ;;
        *)
            echo "unsupported generated visual preset '$preset' (use horizon, ridge, beach)" >&2
            exit 64
            ;;
    esac
}

common_env=(
    RUST_LOG=info,wgpu=error,bevy_asset=error
    RUMPEL_CLIENT_BUILD_PROFILE=release
    RUMPEL_RENDER_MODE=packed
    RUMPEL_PACKED_VIEW_RADIUS="$view_radius"
    RUMPEL_PRESENT_MODE=immediate
    RUMPEL_FRAME_LATENCY=1
    RUMPEL_PROFILE_SECONDS="$profile_seconds"
    RUMPEL_PROFILE_WARMUP_SECONDS="$warmup_seconds"
    RUMPEL_PROFILE_READY_GATE=1
    RUMPEL_PROFILE_AUTOPILOT=0
    RUMPEL_PROFILE_LOG_INTERVAL=1
    RUMPEL_CAMERA_LOCK=1
    RUMPEL_GUI_CAPTURE=1
    RUMPEL_GUI_CAPTURE_DELAY="$capture_delay"
    RUMPEL_GUI_WAIT=1
    RUMPEL_GUI_TERMINAL_AUTO_CLOSE=1
    RUMPEL_CLIENT_SKIP_BUILD=1
    RUMPEL_CLIENT_LOG_DIR="$compare_dir"
)

run_case() {
    local preset="$1"
    local variant="$2"
    local state_file="$compare_dir/${preset}_${variant}.env"
    local summary_file="$compare_dir/${preset}_${variant}.summary.txt"
    local camera_env=()
    local variant_env=()
    mapfile -t camera_env < <(camera_env_for_preset "$preset")

    case "$variant" in
        cpu)
            variant_env=(RUMPEL_PACKED_GPU_GENERATION=0)
            ;;
        generated)
            variant_env=(
                RUMPEL_PACKED_GPU_GENERATION=1
                RUMPEL_PACKED_GPU_GENERATION_REGION_RADIUS="$region_radius"
                RUMPEL_PACKED_GPU_CULL=1
            )
            ;;
        *)
            echo "unsupported generated visual variant '$variant'" >&2
            exit 64
            ;;
    esac

    echo "==> generated visual compare: preset=$preset variant=$variant"
    env \
        "${common_env[@]}" \
        "${camera_env[@]}" \
        "${variant_env[@]}" \
        RUMPEL_PROFILE_STATE_FILE="$state_file" \
        "$repo_dir/scripts/run_client_macos_gui.sh" packed

    local stdout_log
    local screenshot
    stdout_log="$(extract_env_value STDOUT_LOG "$state_file")"
    screenshot="$(extract_env_value SCREENSHOT "$state_file")"
    if [[ ! -f "$stdout_log" ]]; then
        echo "missing profile log for $preset/$variant: $stdout_log" >&2
        exit 66
    fi
    if [[ ! -s "$screenshot" ]]; then
        echo "missing screenshot for $preset/$variant: $screenshot" >&2
        exit 66
    fi

    local draw_mode
    local uploaded_quads
    draw_mode="$(extract_profile_field "$stdout_log" "profile worst_packed" "draw_mode")"
    uploaded_quads="$(extract_profile_field "$stdout_log" "profile worst_packed" "uploaded_quads")"
    if [[ "$variant" == "cpu" && "$draw_mode" == "gpu-generated" ]]; then
        echo "$preset/$variant unexpectedly used gpu-generated draw mode" >&2
        exit 66
    fi
    if [[ "$variant" == "generated" ]]; then
        if [[ "$draw_mode" != "gpu-generated" ]]; then
            echo "$preset/$variant did not enable gpu-generated draw mode (packed_draw_mode=${draw_mode:-missing})" >&2
            exit 66
        fi
        if [[ "$uploaded_quads" != "0" ]]; then
            echo "$preset/$variant expected zero CPU uploaded quads, got ${uploaded_quads:-missing}" >&2
            exit 66
        fi
        if [[ "$(extract_profile_field "$stdout_log" "profile worst_packed" "gpu_cull_enabled")" != "true" ]]; then
            echo "$preset/$variant did not enable packed GPU cull" >&2
            exit 66
        fi
        require_positive_int \
            "$(extract_profile_field "$stdout_log" "profile worst_packed" "gpu_cull_input_commands")" \
            packed_gpu_cull_input_commands \
            "$preset" \
            "$variant"
        require_positive_int \
            "$(extract_profile_field "$stdout_log" "profile worst_packed" "gpu_cull_est_visible_commands")" \
            packed_gpu_cull_est_visible_commands \
            "$preset" \
            "$variant"
        require_positive_int \
            "$(extract_profile_field "$stdout_log" "profile worst_packed" "gpu_cull_est_visible_quads")" \
            packed_gpu_cull_est_visible_quads \
            "$preset" \
            "$variant"
        require_positive_int \
            "$(extract_profile_field "$stdout_log" "profile worst_packed" "generated_regions_loaded")" \
            generated_regions_loaded \
            "$preset" \
            "$variant"
        require_positive_int \
            "$(extract_profile_field "$stdout_log" "profile worst_packed" "generated_regions_active")" \
            generated_regions_active \
            "$preset" \
            "$variant"
        require_positive_int \
            "$(extract_profile_field "$stdout_log" "profile worst_packed" "generated_regions_visible")" \
            generated_regions_visible \
            "$preset" \
            "$variant"
    fi

    "$repo_dir/scripts/summarize_profile_log.sh" "$state_file" > "$summary_file"
    printf '%s\n' "$stdout_log" > "$compare_dir/${preset}_${variant}.stdout.path"
    printf '%s\n' "$screenshot" > "$compare_dir/${preset}_${variant}.screenshot.path"
}

for preset in $presets; do
    run_case "$preset" cpu
    run_case "$preset" generated
done

report="$compare_dir/report.txt"
{
    printf 'generated_visual_compare_dir=%s\n' "$compare_dir"
    printf 'profile_seconds=%s\n' "$profile_seconds"
    printf 'warmup_seconds=%s\n' "$warmup_seconds"
    printf 'capture_delay=%s\n' "$capture_delay"
    printf 'view_radius=%s\n' "$view_radius"
    printf 'generated_region_radius=%s\n' "$region_radius"
    printf 'presets=%s\n' "$presets"
    printf '\n'
    printf 'preset variant render_target ready_status avg_raw_fps worst_frame_ms frames_ge_25ms generated_regions_loaded generated_regions_active generated_regions_visible generated_update_us generated_update_skipped generated_cache_hits generated_cache_misses generated_cache_invalidated generated_cache_evicted visible_quads uploaded_quads indirect_draw_commands gpu_cull_input_commands gpu_cull_visible_commands gpu_cull_visible_quads cpu_visible_commands screenshot log summary\n'
    for preset in $presets; do
        for variant in cpu generated; do
            stdout_log="$(cat "$compare_dir/${preset}_${variant}.stdout.path")"
            screenshot="$(cat "$compare_dir/${preset}_${variant}.screenshot.path")"
            summary="$compare_dir/${preset}_${variant}.summary.txt"
            printf '%s %s %s %s %s %s %s %s %s %s %s %s %s\n' \
                "$preset" \
                "$variant" \
                "$(extract_profile_field "$stdout_log" "profile start" "render_target")" \
                "$(extract_profile_field "$stdout_log" "profile end" "ready_status")" \
                "$(extract_profile_field "$stdout_log" "profile end" "avg_raw_fps")" \
                "$(extract_profile_field "$stdout_log" "profile end" "worst_frame_ms")" \
                "$(extract_profile_field "$stdout_log" "profile end" "frames_ge_25ms")" \
                "$(extract_profile_field "$stdout_log" "profile worst_packed" "generated_regions_loaded")" \
                "$(extract_profile_field "$stdout_log" "profile worst_packed" "generated_regions_active")" \
                "$(extract_profile_field "$stdout_log" "profile worst_packed" "generated_regions_visible")" \
                "$(extract_profile_field "$stdout_log" "profile worst_packed" "generated_update_us")" \
                "$(extract_profile_field "$stdout_log" "profile worst_packed" "generated_update_skipped")" \
                "$(extract_profile_field "$stdout_log" "profile worst_packed" "generated_cache_hits")" \
                "$(extract_profile_field "$stdout_log" "profile worst_packed" "generated_cache_misses")" \
                "$(extract_profile_field "$stdout_log" "profile worst_packed" "generated_cache_invalidated")" \
                "$(extract_profile_field "$stdout_log" "profile worst_packed" "generated_cache_evicted")" \
                "$(extract_profile_field "$stdout_log" "profile worst_packed" "visible_quads")" \
                "$(extract_profile_field "$stdout_log" "profile worst_packed" "uploaded_quads")" \
                "$(extract_profile_field "$stdout_log" "profile worst_packed" "indirect_draw_commands")" \
                "$(extract_profile_field "$stdout_log" "profile worst_packed" "gpu_cull_input_commands")" \
                "$(extract_profile_field "$stdout_log" "profile worst_packed" "gpu_cull_est_visible_commands")" \
                "$(extract_profile_field "$stdout_log" "profile worst_packed" "gpu_cull_est_visible_quads")" \
                "$(extract_profile_field "$stdout_log" "profile worst_packed" "cpu_visible_commands")" \
                "$screenshot" \
                "$stdout_log" \
                "$summary"
        done
    done
} | tee "$report"

echo
echo "Report: $report"
