use bevy::input::mouse::MouseMotion;
use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions, PrimaryWindow};
use rumpel_prelude::*;

const PLAYER_MOVE_SPEED: f32 = 60.0;

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
    mut mouse_motion_events: MessageReader<MouseMotion>,
    mut query: Query<&mut Transform, With<PlayerCamera>>,
    cursor_query: Query<&CursorOptions, With<PrimaryWindow>>,
) {
    let Ok(cursor_options) = cursor_query.single() else {
        return;
    };
    if cursor_options.grab_mode == CursorGrabMode::None {
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
    let Ok(mut transform) = query.single_mut() else {
        return;
    };
    let Ok(camera_transform) = camera_query.single() else {
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
        transform.translation += direction.normalize() * PLAYER_MOVE_SPEED * time.delta_secs();
    }
}

pub fn cursor_grab_system(
    mut cursor_query: Query<&mut CursorOptions, With<PrimaryWindow>>,
    mouse: Res<ButtonInput<MouseButton>>,
    key: Res<ButtonInput<KeyCode>>,
) {
    let Ok(mut cursor_options) = cursor_query.single_mut() else {
        return;
    };

    if mouse.just_pressed(MouseButton::Left) {
        cursor_options.grab_mode = CursorGrabMode::Locked;
        cursor_options.visible = false;
    }

    if key.just_pressed(KeyCode::Escape) {
        cursor_options.grab_mode = CursorGrabMode::None;
        cursor_options.visible = true;
    }
}
