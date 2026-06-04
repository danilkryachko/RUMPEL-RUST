set shell := ["bash", "-eu", "-o", "pipefail", "-c"]

default:
    just --list

fmt:
    cargo fmt --all

fmt-check:
    cargo fmt --all -- --check

check:
    cargo check --workspace --all-targets

check-cached:
    CARGO_INCREMENTAL=0 cargo check --workspace --all-targets

clippy:
    cargo clippy --workspace --all-targets --all-features -- -D warnings

test:
    cargo test --workspace

verify: fmt-check check clippy test

dev:
    RUMPEL_RENDER_MODE=packed RUMPEL_CAMERA_LOCK=0 cargo run -p rumpel_client

dev-cached:
    RUMPEL_RENDER_MODE=packed RUMPEL_CAMERA_LOCK=0 CARGO_INCREMENTAL=0 cargo run -p rumpel_client

dev-gui:
    RUMPEL_RENDER_MODE=packed RUMPEL_CAMERA_LOCK=0 ./scripts/run_client_macos_gui.sh packed

macos-app:
    ./scripts/prepare_client_macos_app.sh

# Legacy surface/compute render modes (disabled in RumpelRenderPlugin).
# profile-client:
#     RUMPEL_RENDER_MODE=surface ... ./scripts/run_client_macos_gui.sh surface
# dev-gpu-compute:
#     RUMPEL_RENDER_MODE=compute ...
# profile-gpu-compute:
#     ...
# profile-gpu-compute-stress:
#     ...

profile-client: profile-packed

dev-packed:
    RUMPEL_RENDER_MODE=packed cargo run -p rumpel_client

# dev-surface-gui:
#     RUMPEL_RENDER_MODE=surface ...

dev-packed-gui:
    RUMPEL_RENDER_MODE=packed RUMPEL_CAMERA_LOCK=0 ./scripts/run_client_macos_gui.sh packed

dev-packed-gpu-generated:
    RUMPEL_RENDER_MODE=packed RUMPEL_PACKED_GPU_GENERATION=1 RUMPEL_PACKED_GPU_CULL=1 RUMPEL_CAMERA_LOCK=0 ./scripts/run_client_macos_gui.sh packed

dev-packed-gpu-generated-gui:
    RUMPEL_RENDER_MODE=packed RUMPEL_PACKED_GPU_GENERATION=1 RUMPEL_PACKED_GPU_CULL=1 RUMPEL_CAMERA_LOCK=1 ./scripts/run_client_macos_gui.sh packed

# dev-packed-material-gui:
#     RUMPEL_RENDER_MODE=packed_material ...

# capture-surface-gui:
#     RUMPEL_RENDER_MODE=surface ...

capture-packed-gui:
    RUMPEL_RENDER_MODE=packed RUMPEL_PACKED_VIEW_RADIUS=16 RUMPEL_CAMERA_LOCK=1 RUMPEL_CAMERA_CLEARANCE=56 RUMPEL_CAMERA_PITCH_RADIANS=-0.36 RUMPEL_CAMERA_YAW_RADIANS=0 RUMPEL_GUI_CAPTURE=1 RUMPEL_GUI_CAPTURE_DELAY=22 ./scripts/run_client_macos_gui.sh packed

# capture-packed-material-gui:
#     RUMPEL_RENDER_MODE=packed_material ...

# capture-compute-gui:
#     RUMPEL_RENDER_MODE=compute ...

capture-render-baseline-gui: capture-packed-gui

profile-packed:
    RUST_LOG=info,wgpu=error,bevy_asset=error RUMPEL_CLIENT_BUILD_PROFILE=${RUMPEL_CLIENT_BUILD_PROFILE:-release} RUMPEL_RENDER_MODE=packed RUMPEL_PACKED_VIEW_RADIUS=16 RUMPEL_PRESENT_MODE=immediate RUMPEL_FRAME_LATENCY=1 RUMPEL_PROFILE_SECONDS=10 RUMPEL_PROFILE_WARMUP_SECONDS=4 RUMPEL_PROFILE_READY_GATE=1 RUMPEL_PROFILE_AUTOPILOT=1 RUMPEL_PROFILE_LOG_INTERVAL=1 RUMPEL_CAMERA_LOCK=0 RUMPEL_GUI_WAIT=1 RUMPEL_GUI_TERMINAL_AUTO_CLOSE=1 ./scripts/run_client_macos_gui.sh packed

profile-packed-stationary:
    RUST_LOG=info,wgpu=error,bevy_asset=error RUMPEL_CLIENT_BUILD_PROFILE=${RUMPEL_CLIENT_BUILD_PROFILE:-release} RUMPEL_RENDER_MODE=packed RUMPEL_PACKED_VIEW_RADIUS=16 RUMPEL_PRESENT_MODE=immediate RUMPEL_FRAME_LATENCY=1 RUMPEL_PROFILE_SECONDS=10 RUMPEL_PROFILE_WARMUP_SECONDS=4 RUMPEL_PROFILE_READY_GATE=1 RUMPEL_PROFILE_AUTOPILOT=0 RUMPEL_PROFILE_LOG_INTERVAL=1 RUMPEL_CAMERA_LOCK=1 RUMPEL_CAMERA_CLEARANCE=56 RUMPEL_CAMERA_PITCH_RADIANS=-0.36 RUMPEL_CAMERA_YAW_RADIANS=0 RUMPEL_GUI_WAIT=1 RUMPEL_GUI_TERMINAL_AUTO_CLOSE=1 ./scripts/run_client_macos_gui.sh packed

# profile-packed-material:
#     RUMPEL_RENDER_MODE=packed_material ...

profile-render-baseline: profile-packed

profile-packed-low-latency:
    RUST_LOG=info,wgpu=error,bevy_asset=error RUMPEL_CLIENT_BUILD_PROFILE=${RUMPEL_CLIENT_BUILD_PROFILE:-release} RUMPEL_RENDER_MODE=packed RUMPEL_PACKED_VIEW_RADIUS=16 RUMPEL_PRESENT_MODE=immediate RUMPEL_FRAME_LATENCY=1 RUMPEL_PROFILE_SECONDS=10 RUMPEL_PROFILE_WARMUP_SECONDS=4 RUMPEL_PROFILE_READY_GATE=1 RUMPEL_PROFILE_AUTOPILOT=1 RUMPEL_PROFILE_LOG_INTERVAL=1 RUMPEL_CAMERA_LOCK=0 RUMPEL_GUI_WAIT=1 RUMPEL_GUI_TERMINAL_AUTO_CLOSE=1 ./scripts/run_client_macos_gui.sh packed

profile-packed-default-latency:
    RUST_LOG=info,wgpu=error,bevy_asset=error RUMPEL_CLIENT_BUILD_PROFILE=${RUMPEL_CLIENT_BUILD_PROFILE:-release} RUMPEL_RENDER_MODE=packed RUMPEL_PACKED_VIEW_RADIUS=16 RUMPEL_PRESENT_MODE=immediate RUMPEL_FRAME_LATENCY=default RUMPEL_PROFILE_SECONDS=10 RUMPEL_PROFILE_WARMUP_SECONDS=4 RUMPEL_PROFILE_READY_GATE=1 RUMPEL_PROFILE_AUTOPILOT=1 RUMPEL_PROFILE_LOG_INTERVAL=1 RUMPEL_CAMERA_LOCK=0 RUMPEL_GUI_WAIT=1 RUMPEL_GUI_TERMINAL_AUTO_CLOSE=1 ./scripts/run_client_macos_gui.sh packed

profile-packed-low-res:
    RUST_LOG=info,wgpu=error,bevy_asset=error RUMPEL_CLIENT_BUILD_PROFILE=${RUMPEL_CLIENT_BUILD_PROFILE:-release} RUMPEL_RENDER_MODE=packed RUMPEL_PACKED_VIEW_RADIUS=16 RUMPEL_PRESENT_MODE=immediate RUMPEL_FRAME_LATENCY=1 RUMPEL_WINDOW_WIDTH=800 RUMPEL_WINDOW_HEIGHT=450 RUMPEL_PROFILE_SECONDS=10 RUMPEL_PROFILE_WARMUP_SECONDS=4 RUMPEL_PROFILE_READY_GATE=1 RUMPEL_PROFILE_AUTOPILOT=1 RUMPEL_PROFILE_LOG_INTERVAL=1 RUMPEL_CAMERA_LOCK=0 RUMPEL_GUI_WAIT=1 RUMPEL_GUI_TERMINAL_AUTO_CLOSE=1 ./scripts/run_client_macos_gui.sh packed

profile-packed-no-shadows:
    RUST_LOG=info,wgpu=error,bevy_asset=error RUMPEL_CLIENT_BUILD_PROFILE=${RUMPEL_CLIENT_BUILD_PROFILE:-release} RUMPEL_RENDER_MODE=packed RUMPEL_PACKED_VIEW_RADIUS=16 RUMPEL_PRESENT_MODE=immediate RUMPEL_FRAME_LATENCY=1 RUMPEL_SHADOWS=0 RUMPEL_PROFILE_SECONDS=10 RUMPEL_PROFILE_WARMUP_SECONDS=4 RUMPEL_PROFILE_READY_GATE=1 RUMPEL_PROFILE_AUTOPILOT=1 RUMPEL_PROFILE_LOG_INTERVAL=1 RUMPEL_CAMERA_LOCK=0 RUMPEL_GUI_WAIT=1 RUMPEL_GUI_TERMINAL_AUTO_CLOSE=1 ./scripts/run_client_macos_gui.sh packed

profile-packed-shadows:
    RUST_LOG=info,wgpu=error,bevy_asset=error RUMPEL_CLIENT_BUILD_PROFILE=${RUMPEL_CLIENT_BUILD_PROFILE:-release} RUMPEL_RENDER_MODE=packed RUMPEL_PACKED_VIEW_RADIUS=16 RUMPEL_PRESENT_MODE=immediate RUMPEL_FRAME_LATENCY=1 RUMPEL_SHADOWS=1 RUMPEL_PROFILE_SECONDS=10 RUMPEL_PROFILE_WARMUP_SECONDS=4 RUMPEL_PROFILE_READY_GATE=1 RUMPEL_PROFILE_AUTOPILOT=1 RUMPEL_PROFILE_LOG_INTERVAL=1 RUMPEL_CAMERA_LOCK=0 RUMPEL_GUI_WAIT=1 RUMPEL_GUI_TERMINAL_AUTO_CLOSE=1 ./scripts/run_client_macos_gui.sh packed

profile-packed-no-hud:
    RUST_LOG=info,wgpu=error,bevy_asset=error RUMPEL_CLIENT_BUILD_PROFILE=${RUMPEL_CLIENT_BUILD_PROFILE:-release} RUMPEL_RENDER_MODE=packed RUMPEL_PACKED_VIEW_RADIUS=16 RUMPEL_PRESENT_MODE=immediate RUMPEL_FRAME_LATENCY=1 RUMPEL_DEBUG_HUD=0 RUMPEL_PROFILE_SECONDS=10 RUMPEL_PROFILE_WARMUP_SECONDS=4 RUMPEL_PROFILE_READY_GATE=1 RUMPEL_PROFILE_AUTOPILOT=1 RUMPEL_PROFILE_LOG_INTERVAL=1 RUMPEL_CAMERA_LOCK=0 RUMPEL_GUI_WAIT=1 RUMPEL_GUI_TERMINAL_AUTO_CLOSE=1 ./scripts/run_client_macos_gui.sh packed

profile-packed-gpu-timestamps:
    RUST_LOG=info,wgpu=error,bevy_asset=error RUMPEL_CLIENT_BUILD_PROFILE=${RUMPEL_CLIENT_BUILD_PROFILE:-release} RUMPEL_RENDER_MODE=packed RUMPEL_PACKED_VIEW_RADIUS=16 RUMPEL_PRESENT_MODE=immediate RUMPEL_FRAME_LATENCY=1 RUMPEL_PACKED_GPU_TIMESTAMPS=1 RUMPEL_PROFILE_SECONDS=12 RUMPEL_PROFILE_WARMUP_SECONDS=4 RUMPEL_PROFILE_READY_GATE=1 RUMPEL_PROFILE_AUTOPILOT=1 RUMPEL_PROFILE_LOG_INTERVAL=1 RUMPEL_CAMERA_LOCK=0 RUMPEL_GUI_WAIT=1 RUMPEL_GUI_TERMINAL_AUTO_CLOSE=1 ./scripts/run_client_macos_gui.sh packed

profile-packed-frame-gpu-timestamps:
    RUST_LOG=info,wgpu=error,bevy_asset=error RUMPEL_CLIENT_BUILD_PROFILE=${RUMPEL_CLIENT_BUILD_PROFILE:-release} RUMPEL_RENDER_MODE=packed RUMPEL_PACKED_VIEW_RADIUS=16 RUMPEL_PRESENT_MODE=immediate RUMPEL_FRAME_LATENCY=1 RUMPEL_RENDER_GPU_FRAME_TIMESTAMPS=1 RUMPEL_PROFILE_SECONDS=12 RUMPEL_PROFILE_WARMUP_SECONDS=4 RUMPEL_PROFILE_READY_GATE=1 RUMPEL_PROFILE_AUTOPILOT=1 RUMPEL_PROFILE_LOG_INTERVAL=1 RUMPEL_CAMERA_LOCK=0 RUMPEL_GUI_WAIT=1 RUMPEL_GUI_TERMINAL_AUTO_CLOSE=1 ./scripts/run_client_macos_gui.sh packed

profile-packed-split-display:
    @echo "split/custom present experiments removed; use profile-packed (window baseline)"

compare-split-display:
    @echo "split/custom present experiments removed; use profile-packed (window baseline)"

profile-packed-custom-present:
    @echo "split/custom present experiments removed; use profile-packed (window baseline)"

compare-present-methods:
    @echo "split/custom present experiments removed; use profile-packed (window baseline)"

compare-present-methods-median:
    @echo "split/custom present experiments removed; use profile-packed (window baseline)"

compare-present-methods-legacy:
    @echo "split/custom present experiments removed; use profile-packed (window baseline)"

profile-packed-headless:
    ./scripts/profile_packed_headless.sh

profile-packed-headless-gpu-timestamps:
    RUMPEL_PACKED_GPU_TIMESTAMPS=1 ./scripts/profile_packed_headless.sh

profile-packed-headless-frame-gpu-timestamps:
    RUMPEL_RENDER_GPU_FRAME_TIMESTAMPS=1 ./scripts/profile_packed_headless.sh

profile-packed-headless-all-gpu-timestamps:
    RUMPEL_RENDER_GPU_FRAME_TIMESTAMPS=1 RUMPEL_PACKED_GPU_TIMESTAMPS=1 ./scripts/profile_packed_headless.sh

profile-packed-headless-wait-matrix:
    ./scripts/profile_headless_wait_matrix.sh

profile-packed-gpu-cull:
    RUST_LOG=info,wgpu=error,bevy_asset=error RUMPEL_CLIENT_BUILD_PROFILE=${RUMPEL_CLIENT_BUILD_PROFILE:-release} RUMPEL_RENDER_MODE=packed RUMPEL_PACKED_VIEW_RADIUS=16 RUMPEL_PRESENT_MODE=immediate RUMPEL_FRAME_LATENCY=1 RUMPEL_PACKED_GPU_CULL=1 RUMPEL_PACKED_GPU_TIMESTAMPS=1 RUMPEL_PROFILE_SECONDS=12 RUMPEL_PROFILE_WARMUP_SECONDS=4 RUMPEL_PROFILE_READY_GATE=1 RUMPEL_PROFILE_AUTOPILOT=1 RUMPEL_PROFILE_LOG_INTERVAL=1 RUMPEL_CAMERA_LOCK=0 RUMPEL_GUI_WAIT=1 RUMPEL_GUI_TERMINAL_AUTO_CLOSE=1 ./scripts/run_client_macos_gui.sh packed

profile-packed-gpu-cull-stationary:
    RUST_LOG=info,wgpu=error,bevy_asset=error RUMPEL_CLIENT_BUILD_PROFILE=${RUMPEL_CLIENT_BUILD_PROFILE:-release} RUMPEL_RENDER_MODE=packed RUMPEL_PACKED_VIEW_RADIUS=16 RUMPEL_PRESENT_MODE=immediate RUMPEL_FRAME_LATENCY=1 RUMPEL_PACKED_GPU_CULL=1 RUMPEL_PROFILE_SECONDS=10 RUMPEL_PROFILE_WARMUP_SECONDS=4 RUMPEL_PROFILE_READY_GATE=1 RUMPEL_PROFILE_AUTOPILOT=0 RUMPEL_PROFILE_LOG_INTERVAL=1 RUMPEL_CAMERA_LOCK=1 RUMPEL_CAMERA_CLEARANCE=56 RUMPEL_CAMERA_PITCH_RADIANS=-0.36 RUMPEL_CAMERA_YAW_RADIANS=0 RUMPEL_GUI_WAIT=1 RUMPEL_GUI_TERMINAL_AUTO_CLOSE=1 ./scripts/run_client_macos_gui.sh packed

profile-packed-gpu-generated:
    RUST_LOG=info,wgpu=error,bevy_asset=error RUMPEL_CLIENT_BUILD_PROFILE=${RUMPEL_CLIENT_BUILD_PROFILE:-release} RUMPEL_RENDER_MODE=packed RUMPEL_PACKED_VIEW_RADIUS=16 RUMPEL_PACKED_GPU_GENERATION=1 RUMPEL_PACKED_GPU_CULL=1 RUMPEL_PRESENT_MODE=auto-no-vsync RUMPEL_FRAME_LATENCY=1 RUMPEL_PROFILE_SECONDS=14 RUMPEL_PROFILE_WARMUP_SECONDS=6 RUMPEL_PROFILE_SETTLE_SECONDS=2 RUMPEL_PROFILE_READY_GATE=1 RUMPEL_PROFILE_AUTOPILOT=1 RUMPEL_PROFILE_LOG_INTERVAL=1 RUMPEL_CAMERA_LOCK=0 RUMPEL_GUI_WAIT=1 RUMPEL_GUI_TERMINAL_AUTO_CLOSE=1 ./scripts/run_client_macos_gui.sh packed

profile-packed-gpu-generated-pacing:
    ./scripts/profile_gpu_generated_pacing.sh

profile-packed-face-cull:
    RUST_LOG=info,wgpu=error,bevy_asset=error RUMPEL_CLIENT_BUILD_PROFILE=${RUMPEL_CLIENT_BUILD_PROFILE:-release} RUMPEL_RENDER_MODE=packed RUMPEL_PACKED_VIEW_RADIUS=16 RUMPEL_PRESENT_MODE=immediate RUMPEL_FRAME_LATENCY=1 RUMPEL_PACKED_FACE_RANGE_CULL=1 RUMPEL_PACKED_FACE_RANGE_MIN_QUADS=4096 RUMPEL_PROFILE_SECONDS=10 RUMPEL_PROFILE_WARMUP_SECONDS=4 RUMPEL_PROFILE_READY_GATE=1 RUMPEL_PROFILE_AUTOPILOT=1 RUMPEL_PROFILE_LOG_INTERVAL=1 RUMPEL_CAMERA_LOCK=0 RUMPEL_GUI_WAIT=1 RUMPEL_GUI_TERMINAL_AUTO_CLOSE=1 ./scripts/run_client_macos_gui.sh packed

profile-summary log=".ai_tasks/last_gui_run.env":
    ./scripts/summarize_profile_log.sh "{{log}}"

profile-summary-headless:
    ./scripts/summarize_profile_log.sh ".ai_tasks/last_headless_run.env"

profile-pacing-matrix:
    ./scripts/profile_pacing_matrix.sh

sccache-stats:
    sccache --show-stats

release:
    RUMPEL_RENDER_MODE=packed RUMPEL_CAMERA_LOCK=0 cargo run -p rumpel_client --release

deny:
    cargo deny check

machete:
    cargo machete

changelog:
    git-cliff -o CHANGELOG.md

new-module name:
    ./scripts/new_module.sh "{{name}}"
