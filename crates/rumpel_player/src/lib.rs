use bevy::input::mouse::MouseMotion;
use bevy::prelude::*;
use bevy::window::{CursorGrabMode, PrimaryWindow};
use rumpel_prelude::*;

#[derive(Component)]
pub struct Player;

#[derive(Component)]
pub struct PlayerCamera;

pub struct RumpelPlayerPlugin;

impl Plugin for RumpelPlayerPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (player_look, player_move, cursor_grab_system).run_if(in_state(GameState::InGame)),
        );
    }
}

pub fn player_look(
    mut mouse_motion_events: EventReader<MouseMotion>,
    mut query: Query<&mut Transform, With<PlayerCamera>>,
    window_query: Query<&Window, With<PrimaryWindow>>,
) {
    let Ok(window) = window_query.get_single() else {
        return;
    };
    if window.cursor.grab_mode == CursorGrabMode::None {
        return;
    }

    let mut delta: Vec2 = Vec2::ZERO;
    for event in mouse_motion_events.read() {
        delta += event.delta;
    }

    if delta == Vec2::ZERO {
        return;
    }

    let sensitivity = 0.003;
    for mut transform in query.iter_mut() {
        let (mut yaw, mut pitch, _) = transform.rotation.to_euler(EulerRot::YXZ);
        yaw -= delta.x * sensitivity;
        pitch -= delta.y * sensitivity;

        // Ограничиваем наклон вверх/вниз
        pitch = pitch.clamp(-1.54, 1.54);

        transform.rotation = Quat::from_euler(EulerRot::YXZ, yaw, pitch, 0.0);
    }
}

pub fn player_move(
    keyboard_input: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    mut query: Query<&mut Transform, With<Player>>,
    camera_query: Query<&Transform, (With<PlayerCamera>, Without<Player>)>,
) {
    let Ok(mut transform) = query.get_single_mut() else {
        return;
    };
    let Ok(camera_transform) = camera_query.get_single() else {
        return;
    };

    let mut direction = Vec3::ZERO;
    let forward = camera_transform.forward();
    let right = camera_transform.right();

    // Плоское движение (без полета вверх/вниз от взгляда)
    let forward = Vec3::new(forward.x, 0.0, forward.z).normalize_or_zero();
    let right = *right; // Convert Dir3 to Vec3

    if keyboard_input.pressed(KeyCode::KeyW) {
        direction += forward;
    }
    if keyboard_input.pressed(KeyCode::KeyS) {
        direction -= forward;
    }
    if keyboard_input.pressed(KeyCode::KeyA) {
        direction -= right;
    }
    if keyboard_input.pressed(KeyCode::KeyD) {
        direction += right;
    }

    if keyboard_input.pressed(KeyCode::Space) {
        direction += Vec3::Y;
    }
    if keyboard_input.pressed(KeyCode::ShiftLeft) {
        direction -= Vec3::Y;
    }

    if direction != Vec3::ZERO {
        let speed = 20.0;
        transform.translation += direction.normalize() * speed * time.delta_seconds();
    }
}

pub fn cursor_grab_system(
    mut window_query: Query<&mut Window, With<PrimaryWindow>>,
    mouse: Res<ButtonInput<MouseButton>>,
    key: Res<ButtonInput<KeyCode>>,
) {
    let Ok(mut window) = window_query.get_single_mut() else {
        return;
    };

    if mouse.just_pressed(MouseButton::Left) {
        window.cursor.grab_mode = CursorGrabMode::Locked;
        window.cursor.visible = false;
    }

    if key.just_pressed(KeyCode::Escape) {
        window.cursor.grab_mode = CursorGrabMode::None;
        window.cursor.visible = true;
    }
}
