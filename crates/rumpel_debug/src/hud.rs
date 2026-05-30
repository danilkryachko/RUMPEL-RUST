use bevy::{
    diagnostic::{DiagnosticsStore, FrameTimeDiagnosticsPlugin},
    prelude::*,
};
use rumpel_prelude::{VoxelChunk, RumpelVoxelWorld};

const FPS_PENDING_TEXT: &str = "FPS -- | Chunks -- (3D Voxel World)";

#[derive(Component)]
pub(crate) struct FpsHudText;

pub(crate) fn spawn_fps_hud(mut commands: Commands) {
    commands.spawn((
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
}

pub(crate) fn update_debug_hud(
    diagnostics: Res<DiagnosticsStore>,
    chunks_query: Query<(), With<VoxelChunk<RumpelVoxelWorld>>>,
    mut fps_text: Query<&mut Text, With<FpsHudText>>,
) {
    let Some(fps) = diagnostics
        .get(&FrameTimeDiagnosticsPlugin::FPS)
        .and_then(|diagnostic| diagnostic.smoothed())
    else {
        return;
    };
    let active_chunk_count = chunks_query.iter().count();

    for mut text in &mut fps_text {
        text.0 = format!(
            "FPS {fps:>5.1} | Chunks {active_chunk_count} loaded (3D Voxel World)"
        );
    }
}
