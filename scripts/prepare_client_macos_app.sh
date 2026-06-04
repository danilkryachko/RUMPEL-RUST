#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && /bin/pwd -P)"
repo_dir="$(cd -- "$script_dir/.." && /bin/pwd -P)"
cd "$repo_dir"

build_profile="${RUMPEL_CLIENT_BUILD_PROFILE:-dev}"
target_profile="debug"
cargo_args=("-p" "rumpel_client")
mode="${RUMPEL_RENDER_MODE:-packed}"

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

case "$mode" in
    packed)
        ;;
    # surface | compute | packed_material)
    #     ;;
    *)
        echo "unsupported RUMPEL_RENDER_MODE='$mode' (active mode: packed)" >&2
        exit 64
        ;;
esac

if [[ "${RUMPEL_CLIENT_SKIP_BUILD:-0}" != "1" ]]; then
    cargo build "${cargo_args[@]}"
fi

binary="$repo_dir/target/$target_profile/rumpel_client"
if [[ ! -x "$binary" ]]; then
    echo "built client binary is missing or not executable: $binary" >&2
    exit 66
fi

stamp="$(date +%Y%m%d_%H%M%S)"
bundle_stamp="${stamp//_/}"
bundle_id="dev.rumpelrust.client.$bundle_stamp"
app_dir="$repo_dir/target/macos/RumpelRustClient-$stamp.app"
contents_dir="$app_dir/Contents"
macos_dir="$contents_dir/MacOS"
mkdir -p "$macos_dir"
cp "$binary" "$macos_dir/rumpel_client_bin"
chmod +x "$macos_dir/rumpel_client_bin"

log_dir="${RUMPEL_CLIENT_LOG_DIR:-$repo_dir/.ai_tasks}"
mkdir -p "$log_dir"
stdout_log="$log_dir/rumpel_client_${mode}_${stamp}.stdout.log"
stderr_log="$log_dir/rumpel_client_${mode}_${stamp}.stderr.log"
state_file="$log_dir/last_macos_app.env"
launch_script="/tmp/rumpel_client_app_${mode}_${stamp}.sh"
screenshot="$log_dir/rumpel_client_${mode}_${stamp}.png"
tmp_screenshot="/tmp/rumpel_client_${mode}_${stamp}.png"
capture="${RUMPEL_GUI_CAPTURE:-0}"
capture_delay="${RUMPEL_GUI_CAPTURE_DELAY:-12}"
camera_lock="${RUMPEL_CAMERA_LOCK:-1}"
rust_log="${RUST_LOG:-info,wgpu=error,bevy_asset=error}"
profile_seconds="${RUMPEL_PROFILE_SECONDS:-0}"
timeout_seconds="${RUMPEL_GUI_TIMEOUT_SECONDS:-}"
if [[ -z "$timeout_seconds" ]]; then
    timeout_seconds="$(awk -v profile="$profile_seconds" -v delay="$capture_delay" 'BEGIN { timeout = profile + delay + 45; if (timeout < 60) timeout = 60; printf "%d", timeout }')"
fi

quote() {
    printf '%q' "$1"
}

append_open_env_line() {
    local key="$1"
    local value="$2"
    echo "open_args+=(--env $(quote "$key=$value"))"
}

{
    echo "#!/usr/bin/env bash"
    echo "set -euo pipefail"
    echo "bundle_id=$(quote "$bundle_id")"
    echo "timeout_seconds=$(quote "$timeout_seconds")"
    echo "open_args=(/usr/bin/open -n -W)"
    echo "open_args+=(--stdout $(quote "$stdout_log"))"
    echo "open_args+=(--stderr $(quote "$stderr_log"))"
    append_open_env_line "RUST_LOG" "$rust_log"
    append_open_env_line "RUMPEL_RENDER_MODE" "$mode"
    append_open_env_line "RUMPEL_CAMERA_LOCK" "$camera_lock"
    append_open_env_line "RUMPEL_CLIENT_WORKING_DIR" "$repo_dir"
    for name in \
        RUMPEL_PACKED_MIN_CELL_SIZE \
        RUMPEL_PACKED_VIEW_RADIUS \
        RUMPEL_PACKED_LOD \
        RUMPEL_PACKED_FACE_DEBUG \
        RUMPEL_PACKED_TOP_ONLY \
        RUMPEL_PACKED_FACE_RANGE_CULL \
        RUMPEL_PACKED_FACE_RANGE_MIN_QUADS \
        RUMPEL_PACKED_MAX_BUILDS_PER_FRAME \
        RUMPEL_PACKED_MAX_COMPLETIONS_PER_FRAME \
        RUMPEL_PACKED_MAX_REBUILDS_PER_FRAME \
        RUMPEL_PACKED_MAX_COMPACTIONS_PER_FRAME \
        RUMPEL_PACKED_MAX_BUILD_TASKS \
        RUMPEL_PACKED_ADAPTIVE_STREAMING \
        RUMPEL_PACKED_DEFER_COMPACTION \
        RUMPEL_PACKED_GPU_GENERATION \
        RUMPEL_PACKED_GPU_GENERATION_REGION_RADIUS \
        RUMPEL_PACKED_GPU_CULL \
        RUMPEL_PACKED_CPU_VISIBLE_COMPACT \
        RUMPEL_PACKED_GPU_TIMESTAMPS \
        RUMPEL_PACKED_ARENA_HEADROOM \
        RUMPEL_PACKED_FOG_START \
        RUMPEL_PACKED_FOG_END \
        RUMPEL_GPU_COUNTERS \
        RUMPEL_GPU_COMPUTE_QUEUE_RADIUS \
        RUMPEL_GPU_COMPUTE_MAX_JOBS_PER_FRAME \
        RUMPEL_COMPUTE_DIRECT_RENDER \
        RUMPEL_COMPUTE_DIRECT_INDIRECT \
        RUMPEL_COMPUTE_DIRECT_MULTI_INDIRECT \
        RUMPEL_COMPUTE_DIRECT_GPU_CULL \
        RUMPEL_COMPUTE_DIRECT_GPU_CULL_COMPACT \
        RUMPEL_PRESENT_MODE \
        RUMPEL_FRAME_LATENCY \
        RUMPEL_WINDOW_WIDTH \
        RUMPEL_WINDOW_HEIGHT \
        RUMPEL_SHADOWS \
        RUMPEL_DEBUG_HUD \
        RUMPEL_DEPTH_PREPASS \
        RUMPEL_OCCLUSION_CULLING \
        RUMPEL_CAMERA_START_X \
        RUMPEL_CAMERA_START_Z \
        RUMPEL_CAMERA_CLEARANCE \
        RUMPEL_CAMERA_PITCH_RADIANS \
        RUMPEL_CAMERA_YAW_RADIANS \
        RUMPEL_PROFILE_SECONDS \
        RUMPEL_PROFILE_AUTOPILOT \
        RUMPEL_PROFILE_LOG_INTERVAL \
        RUMPEL_PROFILE_SLOW_FRAME_MS \
        RUMPEL_PROFILE_WARMUP_SECONDS \
        RUMPEL_PROFILE_READY_GATE \
        RUMPEL_PROFILE_READY_STABLE_FRAMES \
        RUMPEL_PROFILE_READY_FRAME_MS \
        RUMPEL_PROFILE_READY_MAX_EXTRA_SECONDS \
        RUMPEL_PROFILE_AUTOPILOT_PREROLL_SECONDS \
        RUMPEL_PROFILE_SETTLE_SECONDS; do
        if [[ -n "${!name+x}" ]]; then
            append_open_env_line "$name" "${!name}"
        fi
    done
    echo "open_args+=($(quote "$app_dir"))"
    if [[ "$capture" == "1" ]]; then
        echo '"${open_args[@]}" &'
        echo "open_pid=\$!"
        echo "sleep $(quote "$capture_delay")"
        echo "window_id=\$(osascript <<'APPLESCRIPT' 2>/dev/null || true"
        echo "tell application \"System Events\""
        echo "    if exists process \"rumpel_client\" then"
        echo "        tell process \"rumpel_client\""
        echo "            set frontmost to true"
        echo "            if (count of windows) > 0 then"
        echo "                try"
        echo "                    value of attribute \"AXWindowNumber\" of window 1"
        echo "                on error"
        echo "                    \"\""
        echo "                end try"
        echo "            end if"
        echo "        end tell"
        echo "    else if exists process \"rumpel_client_bin\" then"
        echo "        tell process \"rumpel_client_bin\""
        echo "            set frontmost to true"
        echo "            if (count of windows) > 0 then"
        echo "                try"
        echo "                    value of attribute \"AXWindowNumber\" of window 1"
        echo "                on error"
        echo "                    \"\""
        echo "                end try"
        echo "            end if"
        echo "        end tell"
        echo "    end if"
        echo "end tell"
        echo "APPLESCRIPT"
        echo ")"
        echo "sleep 0.2"
        echo "if [[ \"\$window_id\" =~ ^[0-9]+$ ]]; then"
        echo "    screencapture -x -l \"\$window_id\" $(quote "$tmp_screenshot") || screencapture -x $(quote "$tmp_screenshot")"
        echo "else"
        echo "    screencapture -x $(quote "$tmp_screenshot")"
        echo "fi"
        echo "cp $(quote "$tmp_screenshot") $(quote "$screenshot")"
        echo "echo screenshot: $(quote "$screenshot")"
        echo "killall rumpel_client_bin >/dev/null 2>&1 || true"
        echo "wait \"\$open_pid\" || true"
    else
        echo "run_status=0"
        echo "if command -v timeout >/dev/null 2>&1; then"
        echo "    timeout \"\$timeout_seconds\" \"\${open_args[@]}\" || run_status=\$?"
        echo "elif command -v gtimeout >/dev/null 2>&1; then"
        echo "    gtimeout \"\$timeout_seconds\" \"\${open_args[@]}\" || run_status=\$?"
        echo "else"
        echo "    \"\${open_args[@]}\" || run_status=\$?"
        echo "fi"
        echo "if [[ \"\$run_status\" -eq 124 ]]; then"
        echo "    echo \"macOS app open timed out after \${timeout_seconds}s: \$bundle_id\" >&2"
        echo "    launchctl print gui/\"\$(id -u)\" 2>/dev/null | grep -n \"\$bundle_id\" >&2 || true"
        echo "fi"
        echo "exit \"\$run_status\""
    fi
} > "$launch_script"
chmod +x "$launch_script"

cat > "$contents_dir/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN"
  "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleExecutable</key>
  <string>rumpel_client_bin</string>
  <key>CFBundleIdentifier</key>
  <string>$bundle_id</string>
  <key>CFBundleName</key>
  <string>Rumpel Rust Client</string>
  <key>CFBundlePackageType</key>
  <string>APPL</string>
  <key>CFBundleVersion</key>
  <string>1</string>
</dict>
</plist>
EOF
printf 'APPL????' > "$contents_dir/PkgInfo"

xattr -cr "$app_dir" 2>/dev/null || true
if command -v codesign >/dev/null 2>&1; then
    codesign --force --deep --sign - "$app_dir" >/dev/null 2>&1 || true
fi

cat > "$state_file" <<EOF
APP_DIR=$app_dir
RUN_SCRIPT=$launch_script
STDOUT_LOG=$stdout_log
STDERR_LOG=$stderr_log
SCREENSHOT=$screenshot
REPO_DIR=$repo_dir
CAPTURE=$capture
CAPTURE_DELAY=$capture_delay
BUNDLE_ID=$bundle_id
TIMEOUT_SECONDS=$timeout_seconds
EOF

echo "prepared macOS app: $app_dir"
echo "run script: $launch_script"
echo "stdout log: $stdout_log"
echo "stderr log: $stderr_log"
echo "profile summary: $repo_dir/scripts/summarize_profile_log.sh $stdout_log"
if [[ "$capture" == "1" ]]; then
    echo "screenshot: $screenshot"
fi
echo "state file: $state_file"
echo "launch with: $(quote "$launch_script")"
