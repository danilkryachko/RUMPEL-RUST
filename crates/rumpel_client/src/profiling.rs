use bevy::{
    app::AppExit,
    diagnostic::{DiagnosticsStore, FrameTimeDiagnosticsPlugin},
    prelude::*,
};
use rumpel_player::Player;
// Removed legacy voxel world prelude

const PROFILE_SECONDS_ENV: &str = "RUMPEL_PROFILE_SECONDS";
const PROFILE_AUTOPILOT_ENV: &str = "RUMPEL_PROFILE_AUTOPILOT";
const PROFILE_LOG_INTERVAL_ENV: &str = "RUMPEL_PROFILE_LOG_INTERVAL";
const DEFAULT_PROFILE_LOG_INTERVAL_SECONDS: f32 = 1.0;
const MIN_FPS_WARMUP_SECONDS: f32 = 1.0;
const AUTOPILOT_SPEED: f32 = 80.0;

pub struct RumpelClientProfilingPlugin;

impl Plugin for RumpelClientProfilingPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ProfilingRun>()
            .add_systems(Startup, announce_profiling_run)
            .add_systems(Update, (profile_autopilot, log_profile_metrics).chain());
    }
}

#[derive(Resource)]
struct ProfilingRun {
    enabled: bool,
    autopilot: bool,
    duration_seconds: f32,
    log_interval_seconds: f32,
    elapsed_seconds: f32,
    next_log_seconds: f32,
    sample_count: u32,
    min_fps: f64,
    min_raw_fps: f32,
}

impl Default for ProfilingRun {
    fn default() -> Self {
        let duration_seconds = std::env::var(PROFILE_SECONDS_ENV)
            .ok()
            .and_then(|value| value.parse::<f32>().ok())
            .unwrap_or(0.0)
            .max(0.0);
        let log_interval_seconds = std::env::var(PROFILE_LOG_INTERVAL_ENV)
            .ok()
            .and_then(|value| value.parse::<f32>().ok())
            .unwrap_or(DEFAULT_PROFILE_LOG_INTERVAL_SECONDS)
            .max(0.1);

        Self {
            enabled: duration_seconds > 0.0,
            autopilot: env_flag(PROFILE_AUTOPILOT_ENV),
            duration_seconds,
            log_interval_seconds,
            elapsed_seconds: 0.0,
            next_log_seconds: 0.0,
            sample_count: 0,
            min_fps: f64::MAX,
            min_raw_fps: f32::MAX,
        }
    }
}

fn announce_profiling_run(profiling: Res<ProfilingRun>) {
    if !profiling.enabled {
        return;
    }

    println!(
        "profile start duration={:.1}s autopilot={} interval={:.1}s",
        profiling.duration_seconds, profiling.autopilot, profiling.log_interval_seconds
    );
}

fn profile_autopilot(
    time: Res<Time>,
    profiling: Res<ProfilingRun>,
    mut player_query: Query<&mut Transform, With<Player>>,
) {
    if !profiling.enabled || !profiling.autopilot {
        return;
    }

    let Ok(mut player_transform) = player_query.single_mut() else {
        return;
    };

    let elapsed = profiling.elapsed_seconds;
    let direction = Vec3::new((elapsed * 0.35).cos(), 0.0, (elapsed * 0.35).sin()).normalize();
    player_transform.translation += direction * AUTOPILOT_SPEED * time.delta_secs();
}

fn log_profile_metrics(
    time: Res<Time>,
    diagnostics: Res<DiagnosticsStore>,
    // TODO: Track GPU chunk query
    player_query: Query<&Transform, With<Player>>,
    mut profiling: ResMut<ProfilingRun>,
    mut app_exit: MessageWriter<AppExit>,
) {
    if !profiling.enabled {
        return;
    }

    profiling.elapsed_seconds += time.delta_secs();
    let fps = diagnostics
        .get(&FrameTimeDiagnosticsPlugin::FPS)
        .and_then(|diagnostic| diagnostic.smoothed())
        .unwrap_or(0.0);
    if fps > 0.0 && profiling.elapsed_seconds >= MIN_FPS_WARMUP_SECONDS {
        profiling.min_fps = profiling.min_fps.min(fps);
    }
    let raw_fps = if time.delta_secs() > 0.0 {
        1.0 / time.delta_secs()
    } else {
        0.0
    };
    if raw_fps > 0.0 && profiling.elapsed_seconds >= MIN_FPS_WARMUP_SECONDS {
        profiling.min_raw_fps = profiling.min_raw_fps.min(raw_fps);
    }

    let rendered_chunk_count = 0; // TODO: Track GPU chunks

    if profiling.elapsed_seconds >= profiling.next_log_seconds {
        profiling.sample_count += 1;
        profiling.next_log_seconds = profiling.elapsed_seconds + profiling.log_interval_seconds;

        let player_position = player_query
            .single()
            .map(|transform| transform.translation)
            .unwrap_or(Vec3::ZERO);

        println!(
            "profile sample={} t={:.1}s fps={:.1} raw_fps={:.1} chunks={} player=({:.1},{:.1},{:.1})",
            profiling.sample_count,
            profiling.elapsed_seconds,
            fps,
            raw_fps,
            rendered_chunk_count,
            player_position.x,
            player_position.y,
            player_position.z
        );
    }

    if profiling.elapsed_seconds >= profiling.duration_seconds {
        let min_fps = if profiling.min_fps == f64::MAX {
            0.0
        } else {
            profiling.min_fps
        };
        let min_raw_fps = if profiling.min_raw_fps == f32::MAX {
            0.0
        } else {
            profiling.min_raw_fps
        };

        println!(
            "profile end samples={} duration={:.1}s min_fps={:.1} min_raw_fps={:.1}",
            profiling.sample_count,
            profiling.elapsed_seconds,
            min_fps,
            min_raw_fps
        );
        app_exit.write(AppExit::Success);
    }
}

fn env_flag(name: &str) -> bool {
    std::env::var(name).is_ok_and(|value| {
        matches!(
            value.to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        )
    })
}
