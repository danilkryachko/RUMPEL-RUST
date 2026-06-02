use bevy::{
    app::ScheduleRunnerPlugin,
    camera::RenderTarget,
    core_pipeline::prepass::DepthPrepass,
    pbr::{DistanceFog, FogFalloff},
    prelude::*,
    render::experimental::occlusion_culling::OcclusionCulling,
    render::render_resource::TextureFormat,
    window::{ExitCondition, PresentMode, WindowResolution},
    winit::{WinitPlugin, WinitSettings},
};
use profiling::RumpelClientProfilingPlugin;
use rumpel_player::{Player, PlayerCamera, RumpelPlayerPlugin};
use rumpel_prelude::*;
use std::{num::NonZeroU32, path::Path, time::Duration};

mod profiling;

const DEPTH_PREPASS_ENV: &str = "RUMPEL_DEPTH_PREPASS";
const OCCLUSION_CULLING_ENV: &str = "RUMPEL_OCCLUSION_CULLING";
const CAMERA_LOCK_ENV: &str = "RUMPEL_CAMERA_LOCK";
const PACKED_CAMERA_LOCK_ENV: &str = "RUMPEL_PACKED_CAMERA_LOCK";
const PRESENT_MODE_ENV: &str = "RUMPEL_PRESENT_MODE";
const FRAME_LATENCY_ENV: &str = "RUMPEL_FRAME_LATENCY";
const WINDOW_WIDTH_ENV: &str = "RUMPEL_WINDOW_WIDTH";
const WINDOW_HEIGHT_ENV: &str = "RUMPEL_WINDOW_HEIGHT";
const SHADOWS_ENV: &str = "RUMPEL_SHADOWS";
const GPU_PREFLIGHT_ENV: &str = "RUMPEL_GPU_PREFLIGHT";
const CLIENT_WORKING_DIR_ENV: &str = "RUMPEL_CLIENT_WORKING_DIR";
const HEADLESS_RENDER_ENV: &str = "RUMPEL_HEADLESS_RENDER";
const SPLIT_DISPLAY_ENV: &str = rumpel_render::split_display::SPLIT_DISPLAY_ENV;
const HEADLESS_WAIT_MS_ENV: &str = "RUMPEL_HEADLESS_WAIT_MS";
const HEADLESS_RENDER_WIDTH_ENV: &str = "RUMPEL_HEADLESS_RENDER_WIDTH";
const HEADLESS_RENDER_HEIGHT_ENV: &str = "RUMPEL_HEADLESS_RENDER_HEIGHT";
const CAMERA_START_X_ENV: &str = "RUMPEL_CAMERA_START_X";
const CAMERA_START_Z_ENV: &str = "RUMPEL_CAMERA_START_Z";
const CAMERA_CLEARANCE_ENV: &str = "RUMPEL_CAMERA_CLEARANCE";
const CAMERA_PITCH_RADIANS_ENV: &str = "RUMPEL_CAMERA_PITCH_RADIANS";
const CAMERA_YAW_RADIANS_ENV: &str = "RUMPEL_CAMERA_YAW_RADIANS";
const START_PLAYER_X: f32 = 8.0;
const START_PLAYER_Z: f32 = 24.0;
const START_PLAYER_CLEARANCE: f32 = 8.0;
const START_CAMERA_PITCH_RADIANS: f32 = -0.65;
const COMPUTE_START_PLAYER_CLEARANCE: f32 = 18.0;
const COMPUTE_START_CAMERA_PITCH_RADIANS: f32 = -0.42;
const PACKED_START_PLAYER_CLEARANCE: f32 = 56.0;
const PACKED_START_CAMERA_PITCH_RADIANS: f32 = -0.36;
const DEFAULT_PRESENT_MODE: PresentMode = PresentMode::Immediate;
const DEFAULT_FRAME_LATENCY: Option<NonZeroU32> = NonZeroU32::new(1);
const DEFAULT_HEADLESS_WAIT_MS: f64 = 0.0;
const DEFAULT_HEADLESS_RENDER_WIDTH: u32 = 1920;
const DEFAULT_HEADLESS_RENDER_HEIGHT: u32 = 1080;

fn main() {
    apply_client_working_dir_override();
    if env_flag_default(GPU_PREFLIGHT_ENV, true) {
        preflight_gpu_adapter();
    }

    let block_registry = BlockRegistry::default();
    let headless_render = headless_render_enabled();
    let mut default_plugins = DefaultPlugins
        .set(ImagePlugin::default_nearest())
        .set(AssetPlugin {
            file_path: asset_file_path(),
            ..default()
        })
        .set(window_plugin(headless_render));
    if headless_render {
        default_plugins = default_plugins.disable::<WinitPlugin>();
    }

    let mut app = App::new();
    app.insert_resource(block_registry)
        .insert_resource(WinitSettings::continuous())
        .insert_resource(ClearColor(Color::srgb(0.529, 0.808, 0.922)))
        .add_plugins(default_plugins)
        .add_plugins(RumpelPlayerPlugin)
        .add_plugins(rumpel_debug::RumpelDebugPlugin)
        .add_plugins(RumpelClientProfilingPlugin)
        .add_plugins(rumpel_render::RumpelRenderPlugin)
        .init_state::<GameState>()
        .init_resource::<RumpelTime>()
        .add_systems(Startup, setup_camera_and_light)
        .add_systems(
            OnEnter(GameState::Loading),
            (
                rumpel_modding::load_lua_mods,
                enter_game_without_startup_preload,
            )
                .chain(),
        )
        .add_systems(
            Update,
            update_day_night_cycle.run_if(in_state(GameState::InGame)),
        );
    if headless_render {
        app.add_plugins(ScheduleRunnerPlugin::run_loop(headless_wait_duration()));
    } else if split_display_enabled() {
        info!("MAIN: Split display pipeline enabled (offscreen game + Core2d present)");
        rumpel_render::split_display::install_split_display(&mut app);
    }
    app.run();
}

fn apply_client_working_dir_override() {
    let Ok(path) = std::env::var(CLIENT_WORKING_DIR_ENV) else {
        return;
    };
    if path.trim().is_empty() {
        return;
    }
    if let Err(error) = std::env::set_current_dir(&path) {
        eprintln!("failed to set {CLIENT_WORKING_DIR_ENV}='{path}': {error}");
        std::process::exit(66);
    }
}

fn preflight_gpu_adapter() {
    let descriptor = wgpu::InstanceDescriptor::from_env_or_default();
    let instance = wgpu::Instance::new(&descriptor);
    let adapters = instance.enumerate_adapters(descriptor.backends);
    if adapters.is_empty() {
        eprintln!(
            "wgpu did not expose a GPU adapter before Bevy startup. \
             On macOS, run the client from an interactive GUI terminal or launch the generated \
             app bundle so Metal is available to the process."
        );
        std::process::exit(78);
    }
}

fn asset_file_path() -> String {
    std::env::var(CLIENT_WORKING_DIR_ENV)
        .ok()
        .filter(|path| !path.trim().is_empty())
        .map(|path| {
            Path::new(&path)
                .join("assets")
                .to_string_lossy()
                .into_owned()
        })
        .unwrap_or_else(|| "assets".to_string())
}

fn window_plugin(headless_render: bool) -> WindowPlugin {
    if headless_render {
        WindowPlugin {
            primary_window: None,
            exit_condition: ExitCondition::DontExit,
            ..default()
        }
    } else {
        let mode = if std::env::var("RUMPEL_FULLSCREEN")
            .is_ok_and(|val| val == "1" || val == "true")
        {
            bevy::window::WindowMode::BorderlessFullscreen(bevy::window::MonitorSelection::Primary)
        } else {
            bevy::window::WindowMode::Windowed
        };
        let mut window = Window {
            present_mode: present_mode_from_env(),
            desired_maximum_frame_latency: frame_latency_from_env(),
            mode,
            ..default()
        };
        if let Some(resolution) = window_resolution_from_env() {
            window.resolution = resolution;
        }
        WindowPlugin {
            primary_window: Some(window),
            ..default()
        }
    }
}

fn present_mode_from_env() -> PresentMode {
    std::env::var(PRESENT_MODE_ENV).map_or(DEFAULT_PRESENT_MODE, |value| {
        present_mode_from_value(&value)
    })
}

fn present_mode_from_value(value: &str) -> PresentMode {
    match value.to_ascii_lowercase().as_str() {
        "auto-vsync" | "auto_vsync" => PresentMode::AutoVsync,
        "auto-no-vsync" | "auto_no_vsync" | "no-vsync" | "no_vsync" => PresentMode::AutoNoVsync,
        "fifo" | "vsync" => PresentMode::Fifo,
        "fifo-relaxed" | "fifo_relaxed" => PresentMode::FifoRelaxed,
        "immediate" => PresentMode::Immediate,
        "mailbox" => PresentMode::Mailbox,
        unknown => {
            warn!(
                present_mode = unknown,
                "unknown RUMPEL_PRESENT_MODE; using immediate"
            );
            DEFAULT_PRESENT_MODE
        }
    }
}

fn frame_latency_from_env() -> Option<NonZeroU32> {
    let Ok(value) = std::env::var(FRAME_LATENCY_ENV) else {
        return DEFAULT_FRAME_LATENCY;
    };
    match parse_frame_latency_value(&value) {
        Ok(frame_latency) => frame_latency,
        Err(()) => {
            warn!(
                frame_latency = value.trim(),
                "unknown RUMPEL_FRAME_LATENCY; using measured immediate/1 baseline"
            );
            DEFAULT_FRAME_LATENCY
        }
    }
}

fn parse_frame_latency_value(value: &str) -> Result<Option<NonZeroU32>, ()> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Ok(DEFAULT_FRAME_LATENCY);
    }

    match trimmed.to_ascii_lowercase().as_str() {
        "0" | "default" | "none" | "bevy" => Ok(None),
        _ => trimmed
            .parse::<u32>()
            .ok()
            .and_then(NonZeroU32::new)
            .map(Some)
            .ok_or(()),
    }
}

fn window_resolution_from_env() -> Option<WindowResolution> {
    let width = non_empty_env(WINDOW_WIDTH_ENV);
    let height = non_empty_env(WINDOW_HEIGHT_ENV);
    match (width, height) {
        (None, None) => None,
        (Some(width), Some(height)) => match parse_window_size_values(&width, &height) {
            Ok((width, height)) => {
                info!(
                    width,
                    height, "MAIN: primary window resolution overridden for profiling"
                );
                Some(WindowResolution::new(width, height))
            }
            Err(()) => {
                warn!(
                    width = width.trim(),
                    height = height.trim(),
                    "invalid RUMPEL_WINDOW_WIDTH/RUMPEL_WINDOW_HEIGHT; using Bevy default window resolution"
                );
                None
            }
        },
        (width, height) => {
            warn!(
                width = width.as_deref().unwrap_or(""),
                height = height.as_deref().unwrap_or(""),
                "RUMPEL_WINDOW_WIDTH and RUMPEL_WINDOW_HEIGHT must be set together; using Bevy default window resolution"
            );
            None
        }
    }
}

fn parse_window_size_values(width: &str, height: &str) -> Result<(u32, u32), ()> {
    Ok((
        parse_window_dimension_value(width)?,
        parse_window_dimension_value(height)?,
    ))
}

fn parse_window_dimension_value(value: &str) -> Result<u32, ()> {
    value
        .trim()
        .parse::<u32>()
        .ok()
        .filter(|value| *value > 0)
        .ok_or(())
}

fn non_empty_env(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn env_flag_default(name: &str, default: bool) -> bool {
    std::env::var(name).map_or(default, |value| match value.to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => true,
        "0" | "false" | "no" | "off" => false,
        _ => default,
    })
}

#[derive(Component)]
pub struct SunLight;

#[derive(Component)]
pub struct MoonLight;

type SunQuery<'w, 's> = Query<
    'w,
    's,
    (&'static mut Transform, &'static mut DirectionalLight),
    (With<SunLight>, Without<MoonLight>),
>;
type MoonQuery<'w, 's> = Query<
    'w,
    's,
    (&'static mut Transform, &'static mut DirectionalLight),
    (With<MoonLight>, Without<SunLight>),
>;

fn setup_camera_and_light(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    render_mode: Option<Res<rumpel_render::RumpelRenderMode>>,
) {
    let render_mode_value = render_mode.as_ref().map(|mode| **mode);
    let shadows_enabled = shadows_enabled_for_render_mode(render_mode_value);
    if !shadows_enabled {
        info!("MAIN: Directional shadows disabled for this render configuration");
    }

    // 1. Spawn Sun directional light (active during day)
    let mut sun = commands.spawn((
        DirectionalLight {
            shadows_enabled,
            illuminance: 12000.0, // Sunlight in lux
            ..default()
        },
        Transform::from_xyz(100.0, 250.0, 100.0).looking_at(Vec3::ZERO, Vec3::Y),
        SunLight,
    ));
    if shadows_enabled {
        sun.insert(terrain_shadow_config());
    }

    // 2. Spawn Moon directional light (active during night)
    let mut moon = commands.spawn((
        DirectionalLight {
            shadows_enabled,
            illuminance: 0.0,                    // Dark initially
            color: Color::srgb(0.75, 0.85, 1.0), // Faint cool-blue moonlight
            ..default()
        },
        Transform::from_xyz(-100.0, -250.0, -100.0).looking_at(Vec3::ZERO, Vec3::Y),
        MoonLight,
    ));
    if shadows_enabled {
        moon.insert(terrain_shadow_config());
    }

    // Configure global ambient light in Bevy 0.18.
    commands.insert_resource(GlobalAmbientLight {
        color: Color::WHITE,
        brightness: 600.0, // Ambient brightness in lux
        affects_lightmapped_meshes: true,
    });

    // Camera/player.
    let enable_depth_prepass = env_flag(DEPTH_PREPASS_ENV) || env_flag(OCCLUSION_CULLING_ENV);
    let enable_occlusion_culling = env_flag(OCCLUSION_CULLING_ENV);
    let camera_lock = camera_lock_enabled();
    if camera_lock {
        info!(
            "MAIN: Camera lock enabled; player movement, look, interaction, and profiling autopilot are disabled"
        );
    }

    let (default_clearance, default_pitch) = if render_mode_value
        .is_some_and(|mode| mode == rumpel_render::RumpelRenderMode::ComputePrototype)
    {
        (
            COMPUTE_START_PLAYER_CLEARANCE,
            COMPUTE_START_CAMERA_PITCH_RADIANS,
        )
    } else if render_mode_value
        .is_some_and(|mode| mode == rumpel_render::RumpelRenderMode::PackedPrototype)
    {
        (
            PACKED_START_PLAYER_CLEARANCE,
            PACKED_START_CAMERA_PITCH_RADIANS,
        )
    } else {
        (START_PLAYER_CLEARANCE, START_CAMERA_PITCH_RADIANS)
    };
    let start_x = env_f32(CAMERA_START_X_ENV).unwrap_or(START_PLAYER_X);
    let start_z = env_f32(CAMERA_START_Z_ENV).unwrap_or(START_PLAYER_Z);
    let start_clearance = env_f32(CAMERA_CLEARANCE_ENV).unwrap_or(default_clearance);
    let camera_pitch = env_f32(CAMERA_PITCH_RADIANS_ENV).unwrap_or(default_pitch);
    let camera_yaw = env_f32(CAMERA_YAW_RADIANS_ENV).unwrap_or(0.0);
    let start_y = terrain_height_at(start_x as i32, start_z as i32) as f32 + start_clearance;
    let camera_rotation = Quat::from_rotation_y(camera_yaw) * Quat::from_rotation_x(camera_pitch);
    let headless_render_target =
        headless_render_enabled().then(|| create_headless_render_target(&mut images));

    commands
        .spawn((
            Player,
            Transform::from_xyz(start_x, start_y, start_z),
            GlobalTransform::default(),
            Visibility::default(),
            InheritedVisibility::default(),
            ViewVisibility::default(),
        ))
        .with_children(|parent| {
            let mut camera = parent.spawn((
                Camera3d::default(),
                Transform::from_xyz(0.0, 0.5, 0.0).with_rotation(camera_rotation),
                GlobalTransform::default(),
                Visibility::default(),
                InheritedVisibility::default(),
                ViewVisibility::default(),
                PlayerCamera,
                Msaa::Off,
                AmbientLight {
                    color: Color::WHITE,
                    brightness: 600.0,
                    affects_lightmapped_meshes: true,
                },
                DistanceFog {
                    color: Color::srgb(0.529, 0.808, 0.922), // Day sky blue
                    falloff: FogFalloff::Linear {
                        start: 150.0,
                        end: 350.0,
                    },
                    ..default()
                },
            ));
            if let Some(render_target) = headless_render_target {
                camera.insert(render_target);
            } else if split_display_enabled() {
                camera.insert(rumpel_render::split_display::SplitDisplayGameView);
            }
            if enable_depth_prepass {
                camera.insert(DepthPrepass);
            }
            if enable_occlusion_culling {
                camera.insert(OcclusionCulling);
            }
        });
}

fn shadows_enabled_for_render_mode(render_mode: Option<rumpel_render::RumpelRenderMode>) -> bool {
    let default_enabled = !matches!(
        render_mode,
        Some(rumpel_render::RumpelRenderMode::PackedPrototype)
    );
    env_flag_default(SHADOWS_ENV, default_enabled)
}

fn terrain_shadow_config() -> bevy::light::CascadeShadowConfig {
    bevy::light::CascadeShadowConfigBuilder {
        num_cascades: 4,
        minimum_distance: 0.1,
        maximum_distance: 350.0, // Matches full render range to the horizon
        first_cascade_far_bound: 10.0,
        overlap_proportion: 0.2,
    }
    .build()
}

fn enter_game_without_startup_preload(mut next_state: ResMut<NextState<GameState>>) {
    info!("MAIN: Startup chunk preload disabled; entering game immediately");
    next_state.set(GameState::InGame);
}

fn create_headless_render_target(images: &mut Assets<Image>) -> RenderTarget {
    let (width, height) = headless_render_size();
    let target_image =
        Image::new_target_texture(width, height, TextureFormat::bevy_default(), None);
    let target_handle = images.add(target_image);
    info!(
        width,
        height, "MAIN: Headless render target enabled; primary window disabled"
    );
    RenderTarget::Image(target_handle.into())
}

fn lerp_color(c1: Color, c2: Color, t: f32) -> Color {
    let c1_rgba = c1.to_srgba();
    let c2_rgba = c2.to_srgba();
    let r = c1_rgba.red + (c2_rgba.red - c1_rgba.red) * t;
    let g = c1_rgba.green + (c2_rgba.green - c1_rgba.green) * t;
    let b = c1_rgba.blue + (c2_rgba.blue - c1_rgba.blue) * t;
    let a = c1_rgba.alpha + (c2_rgba.alpha - c1_rgba.alpha) * t;
    Color::srgba(r, g, b, a)
}

fn env_flag(name: &str) -> bool {
    std::env::var(name).is_ok_and(|value| {
        matches!(
            value.to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        )
    })
}

fn env_f32(name: &str) -> Option<f32> {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse::<f32>().ok())
        .filter(|value| value.is_finite())
}

fn env_u32(name: &str) -> Option<u32> {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse::<u32>().ok())
        .filter(|value| *value > 0)
}

fn camera_lock_enabled() -> bool {
    env_flag(CAMERA_LOCK_ENV) || env_flag(PACKED_CAMERA_LOCK_ENV)
}

fn headless_render_enabled() -> bool {
    env_flag(HEADLESS_RENDER_ENV)
}

fn split_display_enabled() -> bool {
    env_flag(SPLIT_DISPLAY_ENV) && !headless_render_enabled()
}

fn headless_wait_duration() -> Duration {
    let wait_ms = std::env::var(HEADLESS_WAIT_MS_ENV)
        .ok()
        .and_then(|value| value.parse::<f64>().ok())
        .filter(|value| value.is_finite() && *value >= 0.0)
        .unwrap_or(DEFAULT_HEADLESS_WAIT_MS);
    Duration::from_secs_f64(wait_ms / 1000.0)
}

fn headless_render_size() -> (u32, u32) {
    (
        env_u32(HEADLESS_RENDER_WIDTH_ENV).unwrap_or(DEFAULT_HEADLESS_RENDER_WIDTH),
        env_u32(HEADLESS_RENDER_HEIGHT_ENV).unwrap_or(DEFAULT_HEADLESS_RENDER_HEIGHT),
    )
}

fn update_day_night_cycle(
    time: Res<Time>,
    mut rumpel_time: Option<ResMut<RumpelTime>>,
    mut clear_color: ResMut<ClearColor>,
    mut ambient_light: ResMut<GlobalAmbientLight>,
    mut sun_query: SunQuery,
    mut moon_query: MoonQuery,
    mut camera_query: Query<&mut DistanceFog, With<PlayerCamera>>,
) {
    let mut is_raining = false;
    let mut flash = 0.0;

    let (t, sun_angle) = if let Some(ref mut rt) = rumpel_time {
        // Decay lightning flash at high FPS
        if rt.lightning_flash > 0.0 {
            let dt = time.delta_secs();
            rt.lightning_flash = (rt.lightning_flash - dt * 6.0).max(0.0);
        }
        is_raining = rt.is_raining;
        flash = rt.lightning_flash;
        (rt.elapsed_time, rt.sun_angle)
    } else {
        let t = time.elapsed_secs() * 0.052;
        (t, t.sin())
    };

    let radius = 180.0;

    // Gloom factor during heavy rainstorm
    let gloom_factor = if is_raining { 0.20 } else { 1.0 };

    // 1. Update Sun Transform and lighting (active during day)
    if let Some((mut transform, mut light)) = sun_query.iter_mut().next() {
        let x = t.cos() * radius + 16.0;
        let y = t.sin() * radius + 16.0;
        let z = 16.0;

        *transform = Transform::from_xyz(x, y, z).looking_at(Vec3::new(16.0, 16.0, 16.0), Vec3::Y);

        if sun_angle > 0.0 {
            // Day time: Sunlight intensity up to 12000 lux
            light.illuminance = sun_angle * 12000.0 * gloom_factor;

            // Warm orange glow during sunset/sunrise, crisp white at noon
            let warm_factor = sun_angle.min(0.25) / 0.25;
            light.color = lerp_color(Color::srgb(1.0, 0.45, 0.15), Color::WHITE, warm_factor);
        } else {
            // Sun is below horizon: completely dark
            light.illuminance = 0.0;
        }
    }

    // 2. Update Moon Transform and lighting (active during night)
    if let Some((mut transform, mut light)) = moon_query.iter_mut().next() {
        let t_moon = t + std::f32::consts::PI;
        let x = t_moon.cos() * radius + 16.0;
        let y = t_moon.sin() * radius + 16.0;
        let z = 16.0;

        *transform = Transform::from_xyz(x, y, z).looking_at(Vec3::new(16.0, 16.0, 16.0), Vec3::Y);

        if sun_angle < 0.0 {
            // Night time: Moonlight intensity up to 800 lux
            let moon_angle = -sun_angle;
            light.illuminance = moon_angle * 800.0 * gloom_factor;

            // Deep night-blue moonlit transitions
            let cool_factor = moon_angle.min(0.25) / 0.25;
            light.color = lerp_color(
                Color::srgb(0.5, 0.5, 0.75),
                Color::srgb(0.75, 0.85, 1.0),
                cool_factor,
            );
        } else {
            // Moon is below horizon: completely dark
            light.illuminance = 0.0;
        }
    }

    // Adjust global ambient light
    if is_raining {
        if sun_angle > 0.0 {
            // Dark, gloomy rainstorm daytime ambient
            ambient_light.brightness = 100.0;
            ambient_light.color = Color::srgb(0.12, 0.15, 0.22);
        } else {
            // Almost pitch-black storm nighttime ambient
            ambient_light.brightness = 20.0;
            ambient_light.color = Color::srgb(0.04, 0.06, 0.10);
        }
    } else if sun_angle > 0.0 {
        let factor = sun_angle.min(0.3) / 0.3;
        ambient_light.brightness = sun_angle * 450.0 + 150.0;
        ambient_light.color = lerp_color(Color::srgb(0.85, 0.55, 0.35), Color::WHITE, factor);
    } else {
        // Dark, soft blue ambient light at night, slightly scaled by moon altitude
        let moon_angle = -sun_angle;
        ambient_light.brightness = moon_angle * 80.0 + 40.0;
        ambient_light.color = Color::srgb(0.08, 0.12, 0.22);
    }

    // Transition sky clear color
    if is_raining {
        if sun_angle > 0.15 {
            // Heavy slate-grey storm sky
            clear_color.0 = Color::srgb(0.15, 0.18, 0.22);
        } else if sun_angle > -0.15 {
            // Stormy sunset transition
            let factor = (sun_angle + 0.15) / 0.30;
            clear_color.0 = lerp_color(
                Color::srgb(0.01, 0.01, 0.03),
                Color::srgb(0.20, 0.12, 0.16),
                factor,
            );
        } else {
            clear_color.0 = Color::srgb(0.01, 0.01, 0.03);
        }
    } else if sun_angle > 0.15 {
        // Crisp sky blue
        clear_color.0 = Color::srgb(0.529, 0.808, 0.922);
    } else if sun_angle > -0.15 {
        // Sunset / sunrise warm gradient
        let factor = (sun_angle + 0.15) / 0.30;
        clear_color.0 = lerp_color(
            Color::srgb(0.01, 0.01, 0.03),
            Color::srgb(0.88, 0.36, 0.18),
            factor,
        );
    } else {
        // Night sky deep navy/black
        clear_color.0 = Color::srgb(0.01, 0.01, 0.03);
    }

    // 3. Adjust fog settings based on sun angle and weather
    if let Some(mut fog) = camera_query.iter_mut().next() {
        if is_raining {
            // Thick, blinding storm fog!
            fog.color = Color::srgb(0.15, 0.18, 0.22);
            fog.falloff = FogFalloff::Linear {
                start: 30.0,
                end: 120.0,
            };
        } else if sun_angle > 0.15 {
            // Day time: light sky blue fog
            fog.color = Color::srgb(0.529, 0.808, 0.922);
            fog.falloff = FogFalloff::Linear {
                start: 150.0,
                end: 350.0,
            };
        } else if sun_angle > -0.15 {
            // Sunrise/sunset twilight: rich warm orange cozy mist
            let factor = (sun_angle + 0.15) / 0.30;
            fog.color = lerp_color(
                Color::srgb(0.01, 0.01, 0.03), // Deep night black
                Color::srgb(0.88, 0.36, 0.18), // Warm sunset orange
                factor,
            );
            // Bring fog closer to create a cozy sunset/sunrise mist layer!
            let start = 60.0 + factor * 90.0;
            let end = 200.0 + factor * 150.0;
            fog.falloff = FogFalloff::Linear { start, end };
        } else {
            // Night time: deep navy/black atmospheric dark fog
            fog.color = Color::srgb(0.01, 0.01, 0.03);
            fog.falloff = FogFalloff::Linear {
                start: 100.0,
                end: 300.0,
            };
        }

        // Apply high-intensity full screen lightning flash overlay if active
        if flash > 0.0 {
            // Flash ambient brightness by adding massive lux
            ambient_light.brightness += flash * 15000.0;

            // Blindingly white-blue sky flash
            clear_color.0 = lerp_color(clear_color.0, Color::srgb(0.9, 0.95, 1.0), flash);

            // Flash fog color and extend falloff limits (blinds the screen but opens up visual depth)
            fog.color = lerp_color(fog.color, Color::srgb(0.9, 0.95, 1.0), flash);
            let start_val = 30.0 + flash * 120.0;
            let end_val = 120.0 + flash * 230.0;
            fog.falloff = FogFalloff::Linear {
                start: start_val,
                end: end_val,
            };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn present_mode_default_uses_measured_pacing_baseline() {
        assert_eq!(DEFAULT_PRESENT_MODE, PresentMode::Immediate);
    }

    #[test]
    fn frame_latency_default_uses_measured_low_latency_baseline() {
        assert_eq!(DEFAULT_FRAME_LATENCY, NonZeroU32::new(1));
    }

    #[test]
    fn present_mode_parser_accepts_supported_values() {
        assert_eq!(present_mode_from_value("immediate"), PresentMode::Immediate);
        assert_eq!(
            present_mode_from_value("auto-no-vsync"),
            PresentMode::AutoNoVsync
        );
        assert_eq!(
            present_mode_from_value("auto_no_vsync"),
            PresentMode::AutoNoVsync
        );
        assert_eq!(present_mode_from_value("fifo"), PresentMode::Fifo);
        assert_eq!(
            present_mode_from_value("unknown-mode"),
            DEFAULT_PRESENT_MODE
        );
    }

    #[test]
    fn frame_latency_parser_accepts_supported_values() {
        assert_eq!(parse_frame_latency_value("default").unwrap(), None);
        assert_eq!(parse_frame_latency_value("none").unwrap(), None);
        assert_eq!(parse_frame_latency_value("0").unwrap(), None);
        assert_eq!(parse_frame_latency_value("2").unwrap(), NonZeroU32::new(2));
        assert!(parse_frame_latency_value("unknown").is_err());
    }

    #[test]
    fn window_size_parser_requires_positive_dimensions() {
        assert_eq!(
            parse_window_size_values("1280", "720").unwrap(),
            (1280, 720)
        );
        assert_eq!(
            parse_window_size_values(" 1920 ", " 1080 ").unwrap(),
            (1920, 1080)
        );
        assert!(parse_window_size_values("0", "720").is_err());
        assert!(parse_window_size_values("1280", "invalid").is_err());
    }
}
