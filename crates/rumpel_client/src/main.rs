use bevy::prelude::*;
use rumpel_blocks::BlockRegistry;
use rumpel_player::{Player, PlayerCamera, RumpelPlayerPlugin};
use rumpel_world::world_gen;
use rumpel_coords::ChunkPos;
use rumpel_render;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_plugins(RumpelPlayerPlugin)
        .init_resource::<BlockRegistry>()
        .add_systems(Startup, setup)
        .run();
}

fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    registry: Res<BlockRegistry>,
) {
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

    // Generate test chunk 0,0
    let chunk = world_gen::generate_chunk(ChunkPos::new(0, 0), &registry);
    let mesh = rumpel_render::mesh_chunk(&chunk, &registry);
    
    commands.spawn(PbrBundle {
        mesh: meshes.add(mesh),
        material: materials.add(StandardMaterial {
            base_color: Color::WHITE,
            ..default()
        }),
        transform: Transform::from_xyz(0.0, 0.0, 0.0),
        ..default()
    });
}
