use bevy::{
    diagnostic::{DiagnosticsStore, FrameTimeDiagnosticsPlugin},
    ecs::system::SystemParam,
    prelude::*,
};
use rumpel_player::{Player, PlayerPhysics};
use rumpel_render::{RenderedChunkCount, surface_streaming::SurfaceStreamingMetrics};

const FPS_PENDING_TEXT: &str = "FPS -- | Chunks --";
const HUD_FONT_SIZE: f32 = 8.0;

#[derive(Component)]
pub(crate) struct FpsHudText;

pub(crate) fn spawn_fps_hud() {
    // Spawned dynamically in update_debug_hud to ensure UI Camera exists and is targeted
}

#[expect(
    clippy::too_many_arguments,
    reason = "Bevy system receives independent ECS params directly for scheduler access."
)]
pub(crate) fn update_debug_hud(
    mut commands: Commands,
    diagnostics: Res<DiagnosticsStore>,
    mut fps_text: Query<&mut Text, With<FpsHudText>>,
    chunk_meshes: Query<&RenderedChunkCount>,
    player_mode: Query<&PlayerPhysics, With<Player>>,
    surface_metrics: Option<Res<SurfaceStreamingMetrics>>,
    packed_stats: Option<Res<rumpel_render::packed_quad_pipeline::PackedQuadPipelineStats>>,
    cameras: DebugHudCameras,
) {
    let ui_camera = cameras.player.iter().next();

    if fps_text.is_empty() {
        info!("HUD: Spawning FPS HUD dynamically!");
        let mut entity = commands.spawn((
            Text::new(FPS_PENDING_TEXT),
            TextFont::from_font_size(HUD_FONT_SIZE),
            TextColor(Color::srgb(0.86, 0.96, 0.88)),
            Node {
                position_type: PositionType::Absolute,
                right: px(6),
                top: px(5),
                padding: UiRect::axes(px(4), px(2)),
                min_width: px(41),
                ..default()
            },
            BackgroundColor(Color::srgba(0.02, 0.025, 0.02, 0.72)),
            FpsHudText,
        ));
        if let Some(cam) = ui_camera {
            entity.insert(UiTargetCamera(cam));
            info!(
                "HUD: Spawned FPS HUD and attached UiTargetCamera({:?}) to player camera",
                cam
            );
        } else {
            info!("HUD: Spawned FPS HUD without UiTargetCamera (camera not found)");
        }
        return;
    }

    let fps_diag = diagnostics.get(&FrameTimeDiagnosticsPlugin::FPS);
    if fps_diag.is_none() {
        warn!("HUD: FrameTimeDiagnosticsPlugin::FPS is None!");
    }
    let Some(fps) = fps_diag.and_then(|diagnostic| diagnostic.smoothed()) else {
        return;
    };
    let active_chunk_count = chunk_meshes.iter().map(|count| count.0).sum::<usize>();
    let surface = surface_metrics.as_deref().copied().unwrap_or_default();
    let mode_label = player_mode
        .iter()
        .next()
        .map(|physics| {
            if physics.game_mode.is_creative() {
                "Cre"
            } else {
                "Surv"
            }
        })
        .unwrap_or("--");

    for mut text in &mut fps_text {
        let mut text_str = format!(
            "FPS {fps:>5.1} | Chunks {active_chunk_count} | {mode_label} | Q {}/{} | Up {}",
            surface.pending_regions, surface.building_regions, surface.uploaded_regions_last_frame
        );

        if let Some(stats) = &packed_stats {
            let mode_name = match stats.draw_mode {
                rumpel_render::packed_quad_pipeline::PACKED_DRAW_MODE_GPU_GENERATED => {
                    "gpu-generated"
                }
                rumpel_render::packed_quad_pipeline::PACKED_DRAW_MODE_MATERIAL => "material",
                rumpel_render::packed_quad_pipeline::PACKED_DRAW_MODE_MULTI_INDIRECT => {
                    "multi-indirect"
                }
                rumpel_render::packed_quad_pipeline::PACKED_DRAW_MODE_INDIRECT => "indirect",
                _ => "direct",
            };
            if stats.draw_mode == rumpel_render::packed_quad_pipeline::PACKED_DRAW_MODE_MATERIAL {
                text_str.push_str(&format!(
                    " | Packed Q {}/{} | Mode: {} (Ents: {}, Draws est: {}, Sync: {}us) | Up {} | Bytes {}",
                    stats.visible_quads,
                    stats.quads,
                    mode_name,
                    stats.material_entities,
                    stats.render_draw_calls,
                    stats.material_sync_us,
                    stats.uploaded_quads,
                    stats.uploaded_bytes
                ));
            } else {
                let visible_commands = if stats.draw_mode
                    == rumpel_render::packed_quad_pipeline::PACKED_DRAW_MODE_GPU_GENERATED
                {
                    stats.generated_regions_visible
                } else {
                    stats.cpu_visible_commands
                };
                let mut packed_suffix = format!(
                    " | Packed Q {}/{} | Mode: {} (Cmds: {}, Vis: {}, Draws: {}, CPU: {}us) | Up {} | Bytes {}",
                    stats.visible_quads,
                    stats.quads,
                    mode_name,
                    stats.indirect_draw_commands,
                    visible_commands,
                    stats.render_draw_calls,
                    stats.render_node_us,
                    stats.uploaded_quads,
                    stats.uploaded_bytes
                );
                if stats.draw_mode
                    == rumpel_render::packed_quad_pipeline::PACKED_DRAW_MODE_GPU_GENERATED
                {
                    packed_suffix.push_str(&format!(
                        " | Gen {}/{} Vis {}",
                        stats.generated_regions_active,
                        stats.generated_regions_loaded,
                        stats.generated_regions_visible
                    ));
                }
                text_str.push_str(&packed_suffix);
            }
        }

        text.0 = text_str;
    }
}

#[derive(SystemParam)]
pub(crate) struct DebugHudCameras<'w, 's> {
    player: Query<'w, 's, Entity, With<rumpel_player::PlayerCamera>>,
}

pub(crate) fn debug_camera_components(world: &World, query: Query<Entity, With<Camera>>) {
    for entity in &query {
        let has_camera3d = world.entity(entity).contains::<Camera3d>();
        let has_camera2d = world.entity(entity).contains::<Camera2d>();
        let has_camera = world.entity(entity).contains::<Camera>();
        let has_render_graph = world
            .entity(entity)
            .contains::<bevy::render::camera::CameraRenderGraph>();
        let has_voxel_world_camera = false;
        let has_player_camera = world
            .entity(entity)
            .contains::<rumpel_player::PlayerCamera>();
        let has_parent = world.entity(entity).contains::<ChildOf>();

        info!(
            "CAMERA_DEBUG: Entity {:?} components status: has_camera3d={}, has_camera2d={}, has_camera={}, has_render_graph={}, has_voxel_world_camera={}, has_player_camera={}, has_parent={}",
            entity,
            has_camera3d,
            has_camera2d,
            has_camera,
            has_render_graph,
            has_voxel_world_camera,
            has_player_camera,
            has_parent
        );
    }
}
