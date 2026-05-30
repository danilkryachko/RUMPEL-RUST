use bevy::prelude::*;
use rumpel_player::{Player, PlayerCamera, RumpelPlayerPlugin};
use rumpel_prelude::*;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_plugins(RumpelPlayerPlugin)
        .add_plugins(rumpel_debug::RumpelDebugPlugin)
        .init_state::<GameState>()
        .enable_state_scoped_entities::<GameState>()
        .init_resource::<BlockRegistry>()
        .add_systems(Startup, setup_camera_and_light)
        .add_systems(
            OnEnter(GameState::Loading),
            (rumpel_modding::load_lua_mods, trigger_world_generation).chain(),
        )
        .observe(generate_world_and_start)
        .run();
}

#[derive(Event)]
pub struct SpawnTestWorld;

fn trigger_world_generation(mut commands: Commands) {
    commands.trigger(SpawnTestWorld);
}

fn setup_camera_and_light(mut commands: Commands) {
    // Light
    commands.spawn(PointLightBundle {
        point_light: PointLight {
            shadows_enabled: true,
            ..default()
        },
        transform: Transform::from_xyz(8.0, 60.0, 8.0),
        ..default()
    });

    // Camera/Player
    commands
        .spawn((
            Player,
            TransformBundle::from(Transform::from_xyz(8.0, 50.0, 24.0)),
            VisibilityBundle::default(),
        ))
        .with_children(|parent| {
            parent.spawn((
                Camera3dBundle {
                    transform: Transform::from_xyz(0.0, 0.5, 0.0),
                    ..default()
                },
                PlayerCamera,
            ));
        });
}

fn generate_world_and_start(
    _trigger: Trigger<SpawnTestWorld>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    registry: Res<BlockRegistry>,
    mut next_state: ResMut<NextState<GameState>>,
) {
    // Generate test chunk 0,0
    let chunk = generate_chunk(ChunkPos::new(0, 0), &registry);
    let mesh = rumpel_render::mesh_chunk(&chunk, &registry);

    commands.spawn((
        PbrBundle {
            mesh: meshes.add(mesh),
            material: materials.add(StandardMaterial {
                base_color: Color::WHITE,
                ..default()
            }),
            transform: Transform::from_xyz(0.0, 0.0, 0.0),
            ..default()
        },
        StateScoped(GameState::InGame),
    ));

    // World generated, switch to InGame
    next_state.set(GameState::InGame);
}
