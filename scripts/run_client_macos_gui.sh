#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && /bin/pwd -P)"
repo_dir="$(cd -- "$script_dir/.." && /bin/pwd -P)"
cd "$repo_dir"

mode="${1:-${RUMPEL_RENDER_MODE:-surface}}"
case "$mode" in
    surface | compute | packed | packed_material)
        ;;
    *)
        echo "unsupported render mode '$mode' (use surface, compute, packed, or packed_material)" >&2
        exit 64
        ;;
esac

build_profile="${RUMPEL_CLIENT_BUILD_PROFILE:-dev}"
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
stdout_log="$log_dir/rumpel_client_${mode}_${stamp}.stdout.log"
stderr_log="$log_dir/rumpel_client_${mode}_${stamp}.stderr.log"
status_file="$log_dir/rumpel_client_${mode}_${stamp}.status"
pid_file="$log_dir/rumpel_client_${mode}_${stamp}.pid"
state_file="${RUMPEL_PROFILE_STATE_FILE:-$log_dir/last_gui_run.env}"
mkdir -p "$(dirname "$state_file")"
run_script="/tmp/rumpel_client_${mode}_${stamp}.sh"
command_script="/tmp/rumpel_client_${mode}_${stamp}.command"
screenshot="$log_dir/rumpel_client_${mode}_${stamp}.png"
tmp_screenshot="/tmp/rumpel_client_${mode}_${stamp}.png"
capture="${RUMPEL_GUI_CAPTURE:-0}"
capture_delay="${RUMPEL_GUI_CAPTURE_DELAY:-12}"
camera_lock="${RUMPEL_CAMERA_LOCK:-1}"
rust_log="${RUST_LOG:-info,wgpu=error,bevy_asset=error}"
profile_seconds="${RUMPEL_PROFILE_SECONDS:-}"
gui_wait="${RUMPEL_GUI_WAIT:-}"
if [[ -z "$gui_wait" ]]; then
    if [[ -n "$profile_seconds" || "$capture" == "1" ]]; then
        gui_wait=1
    else
        gui_wait=0
    fi
fi
terminal_auto_close="${RUMPEL_GUI_TERMINAL_AUTO_CLOSE:-$gui_wait}"
launch_method="${RUMPEL_GUI_LAUNCH_METHOD:-auto}"
terminal_title="RUMPEL_CLIENT_${mode}_${stamp}"
timeout_seconds="${RUMPEL_GUI_TIMEOUT_SECONDS:-}"
if [[ -z "$timeout_seconds" ]]; then
    timeout_seconds="$(awk -v profile="${profile_seconds:-0}" -v delay="$capture_delay" 'BEGIN { timeout = profile + delay + 45; if (timeout < 60) timeout = 60; printf "%d", timeout }')"
fi

case "$launch_method" in
    auto | command | ui)
        ;;
    *)
        echo "unsupported RUMPEL_GUI_LAUNCH_METHOD='$launch_method' (use auto, command, or ui)" >&2
        exit 64
        ;;
esac

quote() {
    printf '%q' "$1"
}

applescript_string() {
    local value="$1"
    value="${value//\\/\\\\}"
    value="${value//\"/\\\"}"
    printf '%s' "$value"
}

close_terminal_by_title() {
    local title="$1"
    local escaped_title
    escaped_title="$(applescript_string "$title")"
    osascript \
        -e 'tell application "System Events"' \
        -e 'tell process "Terminal"' \
        -e "set targetTitle to \"$escaped_title\"" \
        -e 'repeat with windowIndex from (count of windows) to 1 by -1' \
        -e 'try' \
        -e 'set targetWindow to window windowIndex' \
        -e 'set windowName to name of targetWindow as text' \
        -e 'if windowName contains targetTitle then' \
        -e 'perform action "AXRaise" of targetWindow' \
        -e 'click button 1 of targetWindow' \
        -e 'delay 0.1' \
        -e 'if (count of sheets of targetWindow) > 0 then' \
        -e 'set targetSheet to sheet 1 of targetWindow' \
        -e 'if exists button "Прервать" of targetSheet then click button "Прервать" of targetSheet' \
        -e 'if exists button "Terminate" of targetSheet then click button "Terminate" of targetSheet' \
        -e 'if exists button "Close" of targetSheet then click button "Close" of targetSheet' \
        -e 'if exists button "Закрыть" of targetSheet then click button "Закрыть" of targetSheet' \
        -e 'end if' \
        -e 'end if' \
        -e 'end try' \
        -e 'end repeat' \
        -e 'end tell' \
        -e 'end tell' >/dev/null 2>&1 || true
}

{
    echo "#!/usr/bin/env bash"
    echo "set -euo pipefail"
    echo "cd $(quote "$repo_dir")"
    echo "terminal_title=$(quote "$terminal_title")"
    echo "terminal_auto_close=$(quote "$terminal_auto_close")"
    echo "status_file=$(quote "$status_file")"
    echo "pid_file=$(quote "$pid_file")"
    echo "printf '\\033]0;%s\\007' \"\$terminal_title\""
    echo "close_terminal_window() {"
    echo "    if [[ \"\$terminal_auto_close\" != \"1\" ]]; then"
    echo "        return"
    echo "    fi"
    echo "    osascript \\"
    echo "        -e 'tell application \"System Events\"' \\"
    echo "        -e 'tell process \"Terminal\"' \\"
    echo "        -e 'set targetTitle to \"$(applescript_string "$terminal_title")\"' \\"
    echo "        -e 'repeat with windowIndex from (count of windows) to 1 by -1' \\"
    echo "        -e 'try' \\"
    echo "        -e 'set targetWindow to window windowIndex' \\"
    echo "        -e 'set windowName to name of targetWindow as text' \\"
    echo "        -e 'if windowName contains targetTitle then' \\"
    echo "        -e 'perform action \"AXRaise\" of targetWindow' \\"
    echo "        -e 'click button 1 of targetWindow' \\"
    echo "        -e 'delay 0.1' \\"
    echo "        -e 'if (count of sheets of targetWindow) > 0 then' \\"
    echo "        -e 'set targetSheet to sheet 1 of targetWindow' \\"
    echo "        -e 'if exists button \"Прервать\" of targetSheet then click button \"Прервать\" of targetSheet' \\"
    echo "        -e 'if exists button \"Terminate\" of targetSheet then click button \"Terminate\" of targetSheet' \\"
    echo "        -e 'if exists button \"Close\" of targetSheet then click button \"Close\" of targetSheet' \\"
    echo "        -e 'if exists button \"Закрыть\" of targetSheet then click button \"Закрыть\" of targetSheet' \\"
    echo "        -e 'end if' \\"
    echo "        -e 'end if' \\"
    echo "        -e 'end try' \\"
    echo "        -e 'end repeat' \\"
    echo "        -e 'end tell' \\"
    echo "        -e 'end tell' >/dev/null 2>&1 || true"
    echo "}"
    echo "write_status() {"
    echo "    printf '%s\\n' \"\$1\" > \"\$status_file\""
    echo "}"
    echo "export RUST_LOG=$(quote "$rust_log")"
    echo "export RUMPEL_RENDER_MODE=$(quote "$mode")"
    echo "export RUMPEL_CAMERA_LOCK=$(quote "$camera_lock")"
    echo "export RUMPEL_CLIENT_WORKING_DIR=$(quote "$repo_dir")"
    echo "# Clear managed toggles unless this launcher explicitly forwards them."
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
        RUMPEL_PACKED_GPU_CULL \
        RUMPEL_PACKED_CPU_VISIBLE_COMPACT \
        RUMPEL_PACKED_GPU_TIMESTAMPS \
        RUMPEL_RENDER_GPU_FRAME_TIMESTAMPS \
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
        RUMPEL_SPLIT_DISPLAY \
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
        RUMPEL_PROFILE_AUTOPILOT_PREROLL_SECONDS; do
        if [[ -n "${!name+x}" ]]; then
            echo "export $name=$(quote "${!name}")"
        else
            echo "unset $name"
        fi
    done
    if [[ "$capture" == "1" ]]; then
        echo "$(quote "$binary") > $(quote "$stdout_log") 2> $(quote "$stderr_log") &"
        echo "client_pid=\$!"
        echo "printf '%s\\n' \"\$client_pid\" > \"\$pid_file\""
        echo "caffeinate_pid="
        echo "if command -v caffeinate >/dev/null 2>&1; then"
        echo "    caffeinate -dimsu -w \"\$client_pid\" &"
        echo "    caffeinate_pid=\$!"
        echo "fi"
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
        echo "osascript <<'APPLESCRIPT' >/dev/null 2>&1 || true"
        echo "tell application \"System Events\""
        echo "    if exists process \"Terminal\" then set visible of process \"Terminal\" to false"
        echo "    if exists process \"rumpel_client\" then set frontmost of process \"rumpel_client\" to true"
        echo "    if exists process \"rumpel_client_bin\" then set frontmost of process \"rumpel_client_bin\" to true"
        echo "end tell"
        echo "APPLESCRIPT"
        echo "sleep 0.2"
        echo "if [[ \"\$window_id\" =~ ^[0-9]+$ ]]; then"
        echo "    screencapture -x -l \"\$window_id\" $(quote "$tmp_screenshot") || screencapture -x $(quote "$tmp_screenshot")"
        echo "else"
        echo "    screencapture -x $(quote "$tmp_screenshot")"
        echo "fi"
        echo "cp $(quote "$tmp_screenshot") $(quote "$screenshot")"
        echo "echo screenshot: $(quote "$screenshot")"
        echo "if kill -0 \"\$client_pid\" 2>/dev/null; then"
        echo "    kill \"\$client_pid\""
        echo "    wait \"\$client_pid\" || true"
        echo "else"
        echo "    wait \"\$client_pid\" || true"
        echo "fi"
        echo "if [[ -n \"\$caffeinate_pid\" ]]; then"
        echo "    wait \"\$caffeinate_pid\" || true"
        echo "fi"
        echo "write_status 0"
        echo "close_terminal_window"
        echo "exit 0"
    elif [[ "$gui_wait" == "1" || "$terminal_auto_close" == "1" ]]; then
        echo "run_status=0"
        echo "$(quote "$binary") > $(quote "$stdout_log") 2> $(quote "$stderr_log") &"
        echo "client_pid=\$!"
        echo "printf '%s\\n' \"\$client_pid\" > \"\$pid_file\""
        echo "wait \"\$client_pid\" || run_status=\$?"
        echo "write_status \"\$run_status\""
        echo "close_terminal_window"
        echo "exit \"\$run_status\""
    else
        echo "exec $(quote "$binary") > $(quote "$stdout_log") 2> $(quote "$stderr_log")"
    fi
} > "$run_script"
chmod +x "$run_script"
cp "$run_script" "$command_script"
chmod +x "$command_script"

cat > "$state_file" <<EOF
MODE=$mode
RUN_SCRIPT=$run_script
STDOUT_LOG=$stdout_log
STDERR_LOG=$stderr_log
STATUS_FILE=$status_file
PID_FILE=$pid_file
COMMAND_SCRIPT=$command_script
SCREENSHOT=$screenshot
REPO_DIR=$repo_dir
CAPTURE=$capture
CAPTURE_DELAY=$capture_delay
GUI_WAIT=$gui_wait
TERMINAL_AUTO_CLOSE=$terminal_auto_close
TERMINAL_TITLE=$terminal_title
LAUNCH_METHOD=$launch_method
EOF

terminal_app="/System/Applications/Utilities/Terminal.app"
terminal_bundle_id="com.apple.Terminal"
launch_script_command="/bin/bash $(quote "$run_script"); exit"
escaped_launch_script_command="$(applescript_string "$launch_script_command")"
top_level_launch_command="tell application id \"$terminal_bundle_id\" to do script \"$escaped_launch_script_command\""

launch_terminal_by_command_file() {
    /usr/bin/open -a "$terminal_app" "$command_script"
}

launch_terminal_by_ui() {
    /usr/bin/open -Ra "$terminal_app" >/dev/null 2>&1 || true
    osascript \
        -e "set launchCommand to \"$escaped_launch_script_command\"" \
        -e 'set oldClipboard to the clipboard' \
        -e 'set the clipboard to launchCommand' \
        -e 'tell application "Terminal" to activate' \
        -e 'tell application "System Events"' \
        -e 'keystroke "n" using command down' \
        -e 'delay 0.2' \
        -e 'keystroke "v" using command down' \
        -e 'key code 36' \
        -e 'end tell' \
        -e 'delay 0.2' \
        -e 'set the clipboard to oldClipboard'
}

focus_terminal() {
    osascript -e "tell application id \"$terminal_bundle_id\" to activate" >/dev/null 2>&1 || true
}

if [[ "${RUMPEL_GUI_PREPARE_ONLY:-0}" == "1" ]]; then
    echo "prepared macOS GUI client run: mode=$mode"
    echo "run script: $run_script"
    echo "stdout log: $stdout_log"
    echo "stderr log: $stderr_log"
    echo "status file: $status_file"
    echo "profile summary: $repo_dir/scripts/summarize_profile_log.sh $stdout_log"
    if [[ "$capture" == "1" ]]; then
        echo "screenshot: $screenshot"
    fi
    echo "state file: $state_file"
    echo "command file launch: /usr/bin/open -a $(quote "$terminal_app") $(quote "$command_script")"
    echo "launch with: /bin/bash $(quote "$run_script") from an interactive GUI Terminal"
    echo "codex top-level launch: osascript -e $(quote "$top_level_launch_command")"
    exit 0
fi

launch_status=1
launch_error_log="$log_dir/rumpel_client_${mode}_${stamp}.launch_errors.log"
: > "$launch_error_log"

if [[ "$launch_method" == "auto" || "$launch_method" == "command" ]]; then
    if launch_terminal_by_command_file >>"$launch_error_log" 2>&1; then
        launch_status=0
    else
        echo "Terminal command-file launch failed" >&2
        cat "$launch_error_log" >&2 || true
    fi
fi

if [[ "$launch_status" -ne 0 && ( "$launch_method" == "auto" || "$launch_method" == "ui" ) ]]; then
    for attempt in 1 2 3 4 5; do
        if launch_terminal_by_ui >>"$launch_error_log" 2>&1; then
            launch_status=0
            break
        fi
        echo "Terminal UI launch attempt $attempt failed; retrying..." >&2
        sleep 1
    done
fi

if [[ "$launch_status" -ne 0 ]]; then
    echo "failed to launch macOS GUI client through Terminal after retries" >&2
    echo "prepared run script remains available: $run_script" >&2
    echo "prepared command file remains available: $command_script" >&2
    echo "stdout log: $stdout_log" >&2
    echo "stderr log: $stderr_log" >&2
    echo "launch error log: $launch_error_log" >&2
    echo "codex top-level launch: osascript -e $(quote "$top_level_launch_command")" >&2
    exit 69
fi

focus_terminal

echo "launched macOS GUI client: mode=$mode"
echo "stdout log: $stdout_log"
echo "stderr log: $stderr_log"
echo "status file: $status_file"
echo "profile summary: $repo_dir/scripts/summarize_profile_log.sh $stdout_log"
if [[ "$capture" == "1" ]]; then
    echo "screenshot: $screenshot"
fi
echo "state file: $state_file"

if [[ "$gui_wait" != "1" ]]; then
    exit 0
fi

started_at="$(date +%s)"
while [[ ! -f "$status_file" ]]; do
    now="$(date +%s)"
    if (( now - started_at > timeout_seconds )); then
        echo "macOS GUI client timed out after ${timeout_seconds}s" >&2
        close_terminal_by_title "$terminal_title"
        if [[ -f "$pid_file" ]]; then
            client_pid="$(cat "$pid_file")"
            if [[ "$client_pid" =~ ^[0-9]+$ ]]; then
                kill "$client_pid" >/dev/null 2>&1 || true
            fi
        fi
        exit 70
    fi
    sleep 1
done

run_status="$(cat "$status_file")"
if [[ ! "$run_status" =~ ^[0-9]+$ ]]; then
    echo "invalid macOS GUI client status: $run_status" >&2
    exit 70
fi
if [[ "$run_status" -ne 0 ]]; then
    echo "macOS GUI client exited with status $run_status" >&2
    if [[ -s "$stderr_log" ]]; then
        tail -80 "$stderr_log" >&2 || true
    fi
    exit "$run_status"
fi

echo "macOS GUI client completed: status=$run_status"
