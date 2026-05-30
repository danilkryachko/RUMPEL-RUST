use bevy::{prelude::*, window::PresentMode, winit::WinitSettings};
use profiling::RumpelClientProfilingPlugin;
use rumpel_player::{Player, PlayerCamera, RumpelPlayerPlugin};
use rumpel_prelude::*;

mod profiling;

const STARTUP_RENDERED_CHUNK_TARGET: usize = 384;
const STARTUP_WARMUP_TIMEOUT_SECS: f32 = 10.0;

fn main() {
    let block_registry = BlockRegistry::default();
    let voxel_world_config = RumpelVoxelWorld::from_registry(&block_registry);

    App::new()
        .insert_resource(block_registry)
        .insert_resource(voxel_world_config.clone())
        .insert_resource(WinitSettings::continuous())
        .insert_resource(ClearColor(Color::srgb(0.529, 0.808, 0.922)))
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                present_mode: PresentMode::AutoNoVsync,
                ..default()
            }),
            ..default()
        }))
        .add_plugins(VoxelWorldPlugin::<RumpelVoxelWorld>::with_config(voxel_world_config.clone()))
        .add_plugins(RumpelPlayerPlugin)
        .add_plugins(rumpel_debug::RumpelDebugPlugin)
        .add_plugins(RumpelClientProfilingPlugin)
        .init_state::<GameState>()
        .init_resource::<StartupChunkWarmup>()
        .add_systems(Startup, setup_camera_and_light)
        .add_systems(
            OnEnter(GameState::Loading),
            (rumpel_modding::load_lua_mods, spawn_loading_overlay).chain(),
        )
        .add_systems(
            Update,
            warmup_startup_chunks.run_if(in_state(GameState::Loading)),
        )
        .run();
}

#[derive(Resource, Default)]
struct StartupChunkWarmup {
    elapsed_seconds: f32,
}

#[derive(Component)]
struct LoadingChunksText;

fn setup_camera_and_light(mut commands: Commands) {
    // Add sun light for PBR rendering
    commands.spawn((
        DirectionalLight {
            shadows_enabled: true,
            illuminance: 12000.0, // Sunlight in lux
            ..default()
        },
        Transform::from_xyz(100.0, 250.0, 100.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));

    // Configure global ambient light in Bevy 0.18
    commands.insert_resource(GlobalAmbientLight {
        color: Color::WHITE,
        brightness: 600.0, // Ambient brightness in lux
        affects_lightmapped_meshes: true,
    });

    // Camera/Player
    commands
        .spawn((
            Player,
            Transform::from_xyz(8.0, 80.0, 24.0),
            GlobalTransform::default(),
            Visibility::default(),
            InheritedVisibility::default(),
            ViewVisibility::default(),
        ))
        .with_children(|parent| {
            parent.spawn((
                Camera3d::default(),
                Msaa::Off,
                Transform::from_xyz(0.0, 0.5, 0.0).with_rotation(Quat::from_rotation_x(-0.3)),
                GlobalTransform::default(),
                Visibility::default(),
                InheritedVisibility::default(),
                ViewVisibility::default(),
                PlayerCamera,
                VoxelWorldCamera::<RumpelVoxelWorld>::default(),
                AmbientLight {
                    color: Color::WHITE,
                    brightness: 600.0,
                    affects_lightmapped_meshes: true,
                },
            ));
        });

}

fn spawn_loading_overlay(
    mut commands: Commands,
    mut warmup: ResMut<StartupChunkWarmup>,
    camera_query: Query<Entity, With<PlayerCamera>>,
) {
    warmup.elapsed_seconds = 0.0;
    let ui_camera = camera_query.iter().next();

    let mut entity = commands.spawn((
        Text::new(format!("Loading chunks 0/{STARTUP_RENDERED_CHUNK_TARGET}")),
        TextFont::from_font_size(24.0),
        TextColor(Color::srgb(0.9, 0.95, 0.9)),
        Node {
            position_type: PositionType::Absolute,
            left: percent(50),
            bottom: px(42),
            padding: UiRect::axes(px(12), px(7)),
            ..default()
        },
        BackgroundColor(Color::srgba(0.02, 0.025, 0.02, 0.78)),
        DespawnOnExit(GameState::Loading),
        LoadingChunksText,
    ));

    if let Some(cam) = ui_camera {
        entity.insert(UiTargetCamera(cam));
        info!("MAIN: Attached UiTargetCamera({:?}) to loading overlay", cam);
    } else {
        info!("MAIN: Spawning loading overlay without UiTargetCamera (camera not found)");
    }
}

fn warmup_startup_chunks(
    mut commands: Commands,
    time: Res<Time>,
    mut warmup: ResMut<StartupChunkWarmup>,
    rendered_chunks: Query<(), With<VoxelChunk<RumpelVoxelWorld>>>,
    mut loading_text: Query<(Entity, &mut Text), With<LoadingChunksText>>,
    camera_query: Query<Entity, With<PlayerCamera>>,
    target_camera_query: Query<(), With<UiTargetCamera>>,
    mut next_state: ResMut<NextState<GameState>>,
) {
    warmup.elapsed_seconds += time.delta_secs();
    let rendered_chunk_count = rendered_chunks.iter().count();

    for (entity, mut text) in &mut loading_text {
        text.0 = format!(
            "Loading chunks {}/{STARTUP_RENDERED_CHUNK_TARGET}",
            rendered_chunk_count.min(STARTUP_RENDERED_CHUNK_TARGET)
        );

        if target_camera_query.get(entity).is_err() {
            if let Some(cam) = camera_query.iter().next() {
                commands.entity(entity).insert(UiTargetCamera(cam));
                info!("MAIN: Attached UiTargetCamera({:?}) to loading overlay during warmup", cam);
            }
        }
    }

    if rendered_chunk_count >= STARTUP_RENDERED_CHUNK_TARGET
        || warmup.elapsed_seconds >= STARTUP_WARMUP_TIMEOUT_SECS
    {
        next_state.set(GameState::InGame);
    }
}
