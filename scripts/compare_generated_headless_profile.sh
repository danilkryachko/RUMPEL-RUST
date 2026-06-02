#!/usr/bin/env bash
# Headless A/B profile for CPU-built packed quads vs GPU-generated packed quads.
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && /bin/pwd -P)"
repo_dir="$(cd -- "$script_dir/.." && /bin/pwd -P)"
cd "$repo_dir"

stamp="$(date +%Y%m%d_%H%M%S)"
compare_dir="${RUMPEL_GENERATED_HEADLESS_COMPARE_DIR:-$repo_dir/.ai_tasks/generated_headless_compare_$stamp}"
mkdir -p "$compare_dir"

profile_seconds="${RUMPEL_GENERATED_HEADLESS_PROFILE_SECONDS:-${RUMPEL_PROFILE_SECONDS:-8}}"
warmup_seconds="${RUMPEL_GENERATED_HEADLESS_WARMUP_SECONDS:-${RUMPEL_PROFILE_WARMUP_SECONDS:-3}}"
view_radius="${RUMPEL_GENERATED_HEADLESS_VIEW_RADIUS:-16}"
region_radius="${RUMPEL_GENERATED_HEADLESS_REGION_RADIUS:-1}"

if [[ "${RUMPEL_CLIENT_SKIP_BUILD:-0}" != "1" ]]; then
    echo "==> building release client for generated headless compare"
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
    local variant="$3"
    if [[ ! "$value" =~ ^[0-9]+$ ]]; then
        echo "$variant expected numeric $label, got '${value:-missing}'" >&2
        exit 66
    fi
    if (( value <= 0 )); then
        echo "$variant expected positive $label, got '$value'" >&2
        exit 66
    fi
}

common_env=(
    RUST_LOG=info,wgpu=error,bevy_asset=error
    RUMPEL_CLIENT_BUILD_PROFILE=release
    RUMPEL_CLIENT_SKIP_BUILD=1
    RUMPEL_CLIENT_LOG_DIR="$compare_dir"
    RUMPEL_RENDER_MODE=packed
    RUMPEL_HEADLESS_RENDER=1
    RUMPEL_HEADLESS_WAIT_MS=0
    RUMPEL_PACKED_VIEW_RADIUS="$view_radius"
    RUMPEL_PRESENT_MODE=immediate
    RUMPEL_FRAME_LATENCY=1
    RUMPEL_PROFILE_SECONDS="$profile_seconds"
    RUMPEL_PROFILE_WARMUP_SECONDS="$warmup_seconds"
    RUMPEL_PROFILE_READY_GATE=1
    RUMPEL_PROFILE_AUTOPILOT=1
    RUMPEL_PROFILE_LOG_INTERVAL=1
    RUMPEL_CAMERA_LOCK=0
)

run_variant() {
    local variant="$1"
    shift
    local state_file="$compare_dir/${variant}.env"
    local summary_file="$compare_dir/${variant}.summary.txt"
    echo "==> generated headless compare: variant=$variant"
    env \
        "${common_env[@]}" \
        "$@" \
        "$repo_dir/scripts/profile_packed_headless.sh" > "$compare_dir/${variant}.runner.log"

    local last_state="$compare_dir/last_headless_run.env"
    if [[ ! -f "$last_state" ]]; then
        echo "missing headless state file for $variant: $last_state" >&2
        exit 66
    fi
    cp "$last_state" "$state_file"

    local stdout_log
    stdout_log="$(extract_env_value STDOUT_LOG "$state_file")"
    if [[ ! -f "$stdout_log" ]]; then
        echo "missing profile log for $variant: $stdout_log" >&2
        exit 66
    fi
    "$repo_dir/scripts/summarize_profile_log.sh" "$state_file" > "$summary_file"
    printf '%s\n' "$stdout_log" > "$compare_dir/${variant}.stdout.path"

    local draw_mode
    draw_mode="$(extract_profile_field "$stdout_log" "profile worst_packed" "draw_mode")"
    if [[ "$variant" == "cpu" && "$draw_mode" == "gpu-generated" ]]; then
        echo "$variant unexpectedly used gpu-generated draw mode" >&2
        exit 66
    fi
    if [[ "$variant" == "generated" ]]; then
        if [[ "$draw_mode" != "gpu-generated" ]]; then
            echo "$variant did not enable gpu-generated draw mode (draw_mode=${draw_mode:-missing})" >&2
            exit 66
        fi
        if [[ "$(extract_profile_field "$stdout_log" "profile worst_packed" "uploaded_quads")" != "0" ]]; then
            echo "$variant expected zero CPU uploaded quads" >&2
            exit 66
        fi
        if [[ "$(extract_profile_field "$stdout_log" "profile worst_packed" "gpu_cull_enabled")" != "true" ]]; then
            echo "$variant did not enable packed GPU cull" >&2
            exit 66
        fi
        require_positive_int \
            "$(extract_profile_field "$stdout_log" "profile worst_packed" "gpu_cull_input_commands")" \
            gpu_cull_input_commands \
            "$variant"
        require_positive_int \
            "$(extract_profile_field "$stdout_log" "profile worst_packed" "gpu_cull_est_visible_commands")" \
            gpu_cull_est_visible_commands \
            "$variant"
        require_positive_int \
            "$(extract_profile_field "$stdout_log" "profile worst_packed" "gpu_cull_est_visible_quads")" \
            gpu_cull_est_visible_quads \
            "$variant"
        require_positive_int \
            "$(extract_profile_field "$stdout_log" "profile worst_packed" "generated_regions_loaded")" \
            generated_regions_loaded \
            "$variant"
        require_positive_int \
            "$(extract_profile_field "$stdout_log" "profile worst_packed" "generated_regions_active")" \
            generated_regions_active \
            "$variant"
        require_positive_int \
            "$(extract_profile_field "$stdout_log" "profile worst_packed" "generated_regions_visible")" \
            generated_regions_visible \
            "$variant"
    fi
}

run_variant cpu \
    RUMPEL_PACKED_GPU_GENERATION=0 \
    RUMPEL_PACKED_GPU_CULL=0
run_variant generated \
    RUMPEL_PACKED_GPU_GENERATION=1 \
    RUMPEL_PACKED_GPU_GENERATION_REGION_RADIUS="$region_radius" \
    RUMPEL_PACKED_GPU_CULL=1

report="$compare_dir/report.txt"
{
    printf 'generated_headless_compare_dir=%s\n' "$compare_dir"
    printf 'profile_seconds=%s\n' "$profile_seconds"
    printf 'warmup_seconds=%s\n' "$warmup_seconds"
    printf 'view_radius=%s\n' "$view_radius"
    printf 'generated_region_radius=%s\n' "$region_radius"
    printf '\n'
    printf 'variant ready_status avg_raw_fps worst_frame_ms frames_ge_25ms draw_mode generated_regions_loaded generated_regions_active generated_regions_visible generated_update_us generated_update_skipped generated_cache_hits generated_cache_misses generated_cache_invalidated generated_cache_evicted visible_quads uploaded_quads indirect_draw_commands gpu_cull_input_commands gpu_cull_visible_commands gpu_cull_visible_quads cpu_visible_commands log summary\n'
    for variant in cpu generated; do
        stdout_log="$(cat "$compare_dir/${variant}.stdout.path")"
        summary="$compare_dir/${variant}.summary.txt"
        printf '%s %s %s %s %s %s %s %s %s %s %s %s %s %s %s\n' \
            "$variant" \
            "$(extract_profile_field "$stdout_log" "profile end" "ready_status")" \
            "$(extract_profile_field "$stdout_log" "profile end" "avg_raw_fps")" \
            "$(extract_profile_field "$stdout_log" "profile end" "worst_frame_ms")" \
            "$(extract_profile_field "$stdout_log" "profile end" "frames_ge_25ms")" \
            "$(extract_profile_field "$stdout_log" "profile worst_packed" "draw_mode")" \
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
            "$stdout_log" \
            "$summary"
    done
} | tee "$report"

echo
echo "Report: $report"
