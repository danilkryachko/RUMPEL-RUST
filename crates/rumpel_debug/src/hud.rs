use bevy::{
    diagnostic::{DiagnosticsStore, FrameTimeDiagnosticsPlugin},
    prelude::*,
};
use rumpel_prelude::{VoxelChunk, RumpelVoxelWorld};

const FPS_PENDING_TEXT: &str = "FPS -- | Chunks -- (3D Voxel World)";

#[derive(Component)]
pub(crate) struct FpsHudText;

pub(crate) fn spawn_fps_hud() {
    // Spawned dynamically in update_debug_hud to ensure UI Camera exists and is targeted
}

pub(crate) fn update_debug_hud(
    mut commands: Commands,
    diagnostics: Res<DiagnosticsStore>,
    chunks_query: Query<(), With<VoxelChunk<RumpelVoxelWorld>>>,
    mut fps_text: Query<&mut Text, With<FpsHudText>>,
    camera_query: Query<Entity, With<rumpel_player::PlayerCamera>>,
) {
    let ui_camera = camera_query.iter().next();

    if fps_text.is_empty() {
        info!("HUD: Spawning FPS HUD dynamically!");
        let mut entity = commands.spawn((
            Text::new(FPS_PENDING_TEXT),
            TextFont::from_font_size(16.0),
            TextColor(Color::srgb(0.86, 0.96, 0.88)),
            Node {
                position_type: PositionType::Absolute,
                right: px(12),
                top: px(10),
                padding: UiRect::axes(px(8), px(4)),
                min_width: px(82),
                ..default()
            },
            BackgroundColor(Color::srgba(0.02, 0.025, 0.02, 0.72)),
            FpsHudText,
        ));
        if let Some(cam) = ui_camera {
            entity.insert(UiTargetCamera(cam));
            info!("HUD: Spawned FPS HUD and attached UiTargetCamera({:?}) to player camera", cam);
        } else {
            info!("HUD: Spawned FPS HUD without UiTargetCamera (camera not found)");
        }
        return;
    }

    let fps_diag = diagnostics.get(&FrameTimeDiagnosticsPlugin::FPS);
    if fps_diag.is_none() {
        warn!("HUD: FrameTimeDiagnosticsPlugin::FPS is None!");
    }
    let Some(fps) = fps_diag
        .and_then(|diagnostic| diagnostic.smoothed())
    else {
        return;
    };
    let active_chunk_count = chunks_query.iter().count();

    for mut text in &mut fps_text {
        text.0 = format!(
            "FPS {fps:>5.1} | Chunks {active_chunk_count} loaded (3D Voxel World)"
        );
        info!("HUD: Updated text to: {}", text.0);
    }
}

pub(crate) fn debug_camera_components(
    world: &World,
    query: Query<Entity, With<Camera>>,
) {
    for entity in &query {
        let has_camera3d = world.entity(entity).contains::<Camera3d>();
        let has_camera2d = world.entity(entity).contains::<Camera2d>();
        let has_camera = world.entity(entity).contains::<Camera>();
        let has_render_graph = world.entity(entity).contains::<bevy::render::camera::CameraRenderGraph>();
        let has_voxel_world_camera = world.entity(entity).contains::<rumpel_prelude::VoxelWorldCamera<RumpelVoxelWorld>>();
        let has_player_camera = world.entity(entity).contains::<rumpel_player::PlayerCamera>();
        let has_parent = world.entity(entity).contains::<ChildOf>();

        info!(
            "CAMERA_DEBUG: Entity {:?} components status: has_camera3d={}, has_camera2d={}, has_camera={}, has_render_graph={}, has_voxel_world_camera={}, has_player_camera={}, has_parent={}",
            entity, has_camera3d, has_camera2d, has_camera, has_render_graph, has_voxel_world_camera, has_player_camera, has_parent
        );
    }
}
