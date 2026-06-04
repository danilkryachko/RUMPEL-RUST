//! Legacy render-mode plugins wired up when `RUMPEL_RENDER_MODE` is explicitly set to
//! `surface` or `compute`.
//!
//! These modes are unsupported for production use; the packed renderer is the default and
//! recommended path. See ADR-002 and ADR-004 for the roadmap.

use bevy::prelude::*;

use crate::RumpelRenderMode;

/// Register legacy render plugins for non-packed modes.
///
/// Call this from `RumpelRenderPlugin::build` *instead of* the packed-pipeline setup when
/// `render_mode` is `Surface` or `ComputePrototype`. For `PackedPrototype` / `PackedMaterial`
/// this is a no-op; callers should add packed plugins directly.
pub fn add_legacy_render_plugins(app: &mut App, mode: RumpelRenderMode) {
    match mode {
        RumpelRenderMode::Surface => {
            info!("rumpel render mode: surface (legacy)");
            app.add_plugins((
                crate::voxel_material::VoxelQuadMaterialPlugin,
                crate::surface_streaming::SurfaceStreamingPlugin,
                crate::surface_decor::SurfaceDecorPlugin,
            ));
        }
        RumpelRenderMode::ComputePrototype => {
            info!("rumpel render mode: compute prototype (legacy)");
            app.add_plugins((
                crate::voxel_material::VoxelQuadMaterialPlugin,
                crate::voxel_compute::VoxelComputePlugin,
            ));
        }
        RumpelRenderMode::PackedPrototype | RumpelRenderMode::PackedMaterial => {
            // Packed plugins are registered by the caller; nothing to do here.
        }
    }
}
