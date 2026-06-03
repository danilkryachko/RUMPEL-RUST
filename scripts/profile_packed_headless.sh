#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && /bin/pwd -P)"
repo_dir="$(cd -- "$script_dir/.." && /bin/pwd -P)"
cd "$repo_dir"

build_profile="${RUMPEL_CLIENT_BUILD_PROFILE:-release}"
target_profile="debug"
cargo_args=("-p" "rumpel_client")
case "$build_profile" in
    dev | debug)
        ;;
    release)
        target_profile="release"
        cargo_args+=("--release")
        ;;
    *)
        echo "unsupported RUMPEL_CLIENT_BUILD_PROFILE='$build_profile' (use dev, debug, or release)" >&2
        exit 64
        ;;
esac

if [[ "${RUMPEL_CLIENT_SKIP_BUILD:-0}" != "1" ]]; then
    cargo build "${cargo_args[@]}"
fi

binary="$repo_dir/target/$target_profile/rumpel_client"
if [[ ! -x "$binary" ]]; then
    echo "client binary is missing or not executable: $binary" >&2
    exit 66
fi

stamp="$(date +%Y%m%d_%H%M%S)"
log_dir="${RUMPEL_CLIENT_LOG_DIR:-$repo_dir/.ai_tasks}"
mkdir -p "$log_dir"
stdout_log="$log_dir/rumpel_client_packed_headless_${stamp}.stdout.log"
stderr_log="$log_dir/rumpel_client_packed_headless_${stamp}.stderr.log"
state_file="$log_dir/last_headless_run.env"

export RUST_LOG="${RUST_LOG:-info,wgpu=error,bevy_asset=error}"
export RUMPEL_CLIENT_WORKING_DIR="$repo_dir"
export RUMPEL_RENDER_MODE="${RUMPEL_RENDER_MODE:-packed}"
export RUMPEL_HEADLESS_RENDER="${RUMPEL_HEADLESS_RENDER:-1}"
export RUMPEL_HEADLESS_WAIT_MS="${RUMPEL_HEADLESS_WAIT_MS:-0}"
export RUMPEL_RENDER_GPU_FRAME_TIMESTAMPS="${RUMPEL_RENDER_GPU_FRAME_TIMESTAMPS:-0}"
export RUMPEL_PACKED_VIEW_RADIUS="${RUMPEL_PACKED_VIEW_RADIUS:-16}"
export RUMPEL_PACKED_GPU_GENERATION="${RUMPEL_PACKED_GPU_GENERATION:-0}"
export RUMPEL_PACKED_GPU_GENERATION_REGION_RADIUS="${RUMPEL_PACKED_GPU_GENERATION_REGION_RADIUS:-1}"
export RUMPEL_PACKED_GPU_CULL="${RUMPEL_PACKED_GPU_CULL:-0}"
export RUMPEL_PACKED_GPU_TIMESTAMPS="${RUMPEL_PACKED_GPU_TIMESTAMPS:-0}"
export RUMPEL_PRESENT_MODE="${RUMPEL_PRESENT_MODE:-immediate}"
export RUMPEL_FRAME_LATENCY="${RUMPEL_FRAME_LATENCY:-default}"
export RUMPEL_PROFILE_SECONDS="${RUMPEL_PROFILE_SECONDS:-10}"
export RUMPEL_PROFILE_WARMUP_SECONDS="${RUMPEL_PROFILE_WARMUP_SECONDS:-4}"
export RUMPEL_PROFILE_AUTOPILOT="${RUMPEL_PROFILE_AUTOPILOT:-1}"
export RUMPEL_PROFILE_LOG_INTERVAL="${RUMPEL_PROFILE_LOG_INTERVAL:-1}"
export RUMPEL_CAMERA_LOCK="${RUMPEL_CAMERA_LOCK:-0}"

{
    printf 'STDOUT_LOG=%s\n' "$stdout_log"
    printf 'STDERR_LOG=%s\n' "$stderr_log"
    printf 'BUILD_PROFILE=%s\n' "$build_profile"
    printf 'RUMPEL_RENDER_MODE=%s\n' "$RUMPEL_RENDER_MODE"
    printf 'RUMPEL_HEADLESS_RENDER=%s\n' "$RUMPEL_HEADLESS_RENDER"
    printf 'RUMPEL_HEADLESS_WAIT_MS=%s\n' "$RUMPEL_HEADLESS_WAIT_MS"
    printf 'RUMPEL_RENDER_GPU_FRAME_TIMESTAMPS=%s\n' "$RUMPEL_RENDER_GPU_FRAME_TIMESTAMPS"
    printf 'RUMPEL_PACKED_GPU_TIMESTAMPS=%s\n' "$RUMPEL_PACKED_GPU_TIMESTAMPS"
    printf 'RUMPEL_PACKED_VIEW_RADIUS=%s\n' "$RUMPEL_PACKED_VIEW_RADIUS"
    printf 'RUMPEL_PACKED_GPU_GENERATION=%s\n' "$RUMPEL_PACKED_GPU_GENERATION"
    printf 'RUMPEL_PACKED_GPU_GENERATION_REGION_RADIUS=%s\n' "$RUMPEL_PACKED_GPU_GENERATION_REGION_RADIUS"
    printf 'RUMPEL_PACKED_GPU_CULL=%s\n' "$RUMPEL_PACKED_GPU_CULL"
    printf 'RUMPEL_PROFILE_SECONDS=%s\n' "$RUMPEL_PROFILE_SECONDS"
    printf 'RUMPEL_PROFILE_WARMUP_SECONDS=%s\n' "$RUMPEL_PROFILE_WARMUP_SECONDS"
} > "$state_file"

set +e
"$binary" > "$stdout_log" 2> "$stderr_log"
status=$?
set -e

printf 'EXIT_STATUS=%s\n' "$status" >> "$state_file"

if [[ "$status" -ne 0 ]]; then
    echo "headless packed profile failed with status $status" >&2
    echo "stdout: $stdout_log" >&2
    echo "stderr: $stderr_log" >&2
    exit "$status"
fi

"$repo_dir/scripts/summarize_profile_log.sh" "$stdout_log"
