//! Offscreen game render with a dedicated window display pipeline (Method A).
//!
//! When enabled via `RUMPEL_SPLIT_DISPLAY=1`, the player `Camera3d` renders the full
//! `Core3d` graph into an intermediate `Image`, and a lightweight `Camera2d` presents
//! that texture to the primary window with a fullscreen [`Sprite`] (Core2d on swapchain).

use bevy::{
    camera::{OrthographicProjection, Projection, RenderTarget, ScalingMode},
    prelude::*,
    render::render_resource::{Extent3d, TextureFormat},
    sprite::Sprite,
    window::PrimaryWindow,
};

pub const SPLIT_DISPLAY_ENV: &str = "RUMPEL_SPLIT_DISPLAY";

const GAME_CAMERA_ORDER: isize = 0;
const DISPLAY_CAMERA_ORDER: isize = 100;

/// Marker on the `Camera2d` that presents the offscreen game target to the window.
#[derive(Component)]
pub struct SplitDisplayPresentCamera;

/// Marker on the game `Camera3d` that must render into [`GameSceneRenderTarget`].
#[derive(Component)]
pub struct SplitDisplayGameView;

#[derive(Component)]
struct SplitDisplayBlitSprite;

#[derive(Component)]
struct SplitDisplayGameCameraAttached;

#[derive(Resource, Clone)]
pub struct GameSceneRenderTarget {
    pub image: Handle<Image>,
    pub size: UVec2,
}

#[derive(Resource)]
struct SplitDisplayEntities {
    blit_sprite: Entity,
}

/// Registers split-display systems. Call from `rumpel_client` after `DefaultPlugins`.
pub fn install_split_display(app: &mut App) {
    if !split_display_enabled_from_env() {
        return;
    }

    app.add_systems(
        PostStartup,
        (bootstrap_split_display, attach_split_display_game_camera).chain(),
    )
    .add_systems(Update, maintain_game_scene_render_target_size);
}

pub fn split_display_enabled_from_env() -> bool {
    env_flag(SPLIT_DISPLAY_ENV)
}

pub fn game_scene_render_target(
    images: &mut Assets<Image>,
    width: u32,
    height: u32,
) -> GameSceneRenderTarget {
    let size = UVec2::new(width.max(1), height.max(1));
    let image = images.add(new_game_scene_image(size));
    GameSceneRenderTarget { image, size }
}

fn new_game_scene_image(size: UVec2) -> Image {
    Image::new_target_texture(size.x, size.y, TextureFormat::bevy_default(), None)
}

fn present_camera_projection() -> Projection {
    Projection::Orthographic(OrthographicProjection {
        scaling_mode: ScalingMode::WindowSize,
        ..OrthographicProjection::default_2d()
    })
}

fn bootstrap_split_display(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    primary_window: Query<&Window, With<PrimaryWindow>>,
) {
    let Ok(window) = primary_window.single() else {
        warn!("split display: primary window missing; display pipeline not created");
        return;
    };

    let width = window.physical_width().max(1);
    let height = window.physical_height().max(1);
    let game_target = game_scene_render_target(&mut images, width, height);
    let blit_size = Vec2::new(width as f32, height as f32);

    commands.insert_resource(game_target.clone());
    let blit_sprite = commands
        .spawn((
            Sprite {
                image: game_target.image.clone(),
                custom_size: Some(blit_size),
                color: Color::WHITE,
                ..default()
            },
            Transform::IDENTITY,
            GlobalTransform::default(),
            Visibility::default(),
            InheritedVisibility::default(),
            ViewVisibility::default(),
            SplitDisplayBlitSprite,
        ))
        .id();

    commands.spawn((
        Camera2d,
        Camera {
            order: DISPLAY_CAMERA_ORDER,
            clear_color: ClearColorConfig::Custom(Color::BLACK),
            ..default()
        },
        present_camera_projection(),
        Msaa::Off,
        SplitDisplayPresentCamera,
    ));

    commands.insert_resource(SplitDisplayEntities { blit_sprite });

    info!(
        width,
        height, "split display: game scene renders offscreen; Core2d presents to window"
    );
}

type PendingSplitDisplayGameCameraQuery<'w, 's> = Query<
    'w,
    's,
    Entity,
    (
        With<SplitDisplayGameView>,
        With<Camera3d>,
        Without<SplitDisplayGameCameraAttached>,
    ),
>;

fn attach_split_display_game_camera(
    mut commands: Commands,
    game_target: Res<GameSceneRenderTarget>,
    game_cameras: PendingSplitDisplayGameCameraQuery<'_, '_>,
) {
    for entity in &game_cameras {
        commands.entity(entity).insert((
            RenderTarget::Image(game_target.image.clone().into()),
            Camera {
                order: GAME_CAMERA_ORDER,
                ..default()
            },
            SplitDisplayGameCameraAttached,
        ));
    }
}

fn maintain_game_scene_render_target_size(
    mut images: ResMut<Assets<Image>>,
    mut sprites: Query<&mut Sprite, With<SplitDisplayBlitSprite>>,
    mut game_target: ResMut<GameSceneRenderTarget>,
    entities: Option<Res<SplitDisplayEntities>>,
    primary_window: Query<&Window, With<PrimaryWindow>>,
) {
    let Some(entities) = entities else {
        return;
    };

    let Ok(window) = primary_window.single() else {
        return;
    };

    let width = window.physical_width().max(1);
    let height = window.physical_height().max(1);
    let new_size = UVec2::new(width, height);
    if new_size == game_target.size {
        return;
    }

    if let Some(image) = images.get_mut(&game_target.image) {
        image.resize(Extent3d {
            width: new_size.x,
            height: new_size.y,
            depth_or_array_layers: 1,
        });
    }

    if let Ok(mut sprite) = sprites.get_mut(entities.blit_sprite) {
        sprite.custom_size = Some(Vec2::new(width as f32, height as f32));
    }

    game_target.size = new_size;

    info!(
        width,
        height, "split display: resized offscreen game target and blit sprite"
    );
}

fn env_flag(name: &str) -> bool {
    std::env::var(name).is_ok_and(|value| {
        matches!(
            value.to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_display_env_constant_matches_runtime_key() {
        assert_eq!(SPLIT_DISPLAY_ENV, "RUMPEL_SPLIT_DISPLAY");
    }

    #[test]
    fn present_camera_projection_uses_window_size_scaling() {
        let Projection::Orthographic(projection) = present_camera_projection() else {
            panic!("expected orthographic projection for split display present camera");
        };
        assert!(matches!(projection.scaling_mode, ScalingMode::WindowSize));
    }
}
