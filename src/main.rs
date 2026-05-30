#![deny(clippy::pedantic)]
#![allow(clippy::needless_pass_by_value)] // Common in Bevy
#![allow(clippy::type_complexity)] // Common in Bevy
#![allow(clippy::module_name_repetitions)]

use bevy::prelude::*;
use bevy::input::mouse::MouseMotion;
use bevy::window::{CursorGrabMode, PrimaryWindow};

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_systems(Startup, setup)
        .add_systems(Update, (player_look, player_move, cursor_grab_system))
        .run();
}

#[derive(Component)]
struct Player;

#[derive(Component)]
struct PlayerCamera;

const MOVE_SPEED: f32 = 5.0;
const MOUSE_SENSITIVITY: f32 = 0.002;

fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    // Light
    commands.spawn(PointLightBundle {
        point_light: PointLight {
            shadows_enabled: true,
            ..default()
        },
        transform: Transform::from_xyz(4.0, 8.0, 4.0),
        ..default()
    });

    // Camera/Player
    commands
        .spawn((
            Player,
            TransformBundle::from(Transform::from_xyz(0.0, 2.0, 5.0)),
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

    // Test "Chunk" (16x16 plane of blocks)
    let block_mesh = meshes.add(Cuboid::new(1.0, 1.0, 1.0));
    let block_material = materials.add(Color::srgb(0.2, 0.8, 0.2)); // Grass-like green

    for x in -8..8 {
        for z in -8..8 {
            commands.spawn(PbrBundle {
                mesh: block_mesh.clone(),
                material: block_material.clone(),
                transform: Transform::from_xyz(x as f32, 0.0, z as f32),
                ..default()
            });
        }
    }
}

fn player_look(
    mut mouse_events: EventReader<MouseMotion>,
    mut query: Query<&mut Transform, With<PlayerCamera>>,
) {
    let mut delta = Vec2::ZERO;
    for event in mouse_events.read() {
        delta += event.delta;
    }

    if delta == Vec2::ZERO {
        return;
    }

    for mut transform in &mut query {
        let (mut yaw, mut pitch, _) = transform.rotation.to_euler(EulerRot::YXZ);
        
        yaw -= delta.x * MOUSE_SENSITIVITY;
        pitch -= delta.y * MOUSE_SENSITIVITY;
        
        // Clamp pitch to look straight up and down
        pitch = pitch.clamp(-std::f32::consts::FRAC_PI_2 + 0.01, std::f32::consts::FRAC_PI_2 - 0.01);
        
        transform.rotation = Quat::from_euler(EulerRot::YXZ, yaw, pitch, 0.0);
    }
}

fn player_move(
    keyboard_input: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    mut query: Query<&mut Transform, With<Player>>,
    camera_query: Query<&Transform, (With<PlayerCamera>, Without<Player>)>,
) {
    let Ok(mut transform) = query.get_single_mut() else { return };
    let Ok(camera_transform) = camera_query.get_single() else { return };

    let mut direction = Vec3::ZERO;

    // Movement based on camera's forward and right vectors (ignoring vertical pitch for movement)
    let forward = camera_transform.forward();
    let forward_flat = Vec3::new(forward.x, 0.0, forward.z).normalize_or_zero();
    let right = camera_transform.right();
    let right_flat = Vec3::new(right.x, 0.0, right.z).normalize_or_zero();

    if keyboard_input.pressed(KeyCode::KeyW) {
        direction += forward_flat;
    }
    if keyboard_input.pressed(KeyCode::KeyS) {
        direction -= forward_flat;
    }
    if keyboard_input.pressed(KeyCode::KeyD) {
        direction += right_flat;
    }
    if keyboard_input.pressed(KeyCode::KeyA) {
        direction -= right_flat;
    }
    if keyboard_input.pressed(KeyCode::Space) {
        direction += Vec3::Y;
    }
    if keyboard_input.pressed(KeyCode::ShiftLeft) {
        direction -= Vec3::Y;
    }

    if direction != Vec3::ZERO {
        transform.translation += direction.normalize() * MOVE_SPEED * time.delta_seconds();
    }
}

fn cursor_grab_system(
    mut windows: Query<&mut Window, With<PrimaryWindow>>,
    btn: Res<ButtonInput<MouseButton>>,
    key: Res<ButtonInput<KeyCode>>,
) {
    let Ok(mut window) = windows.get_single_mut() else { return };

    if btn.just_pressed(MouseButton::Left) {
        window.cursor.grab_mode = CursorGrabMode::Locked;
        window.cursor.visible = false;
    }

    if key.just_pressed(KeyCode::Escape) {
        window.cursor.grab_mode = CursorGrabMode::None;
        window.cursor.visible = true;
    }
}
