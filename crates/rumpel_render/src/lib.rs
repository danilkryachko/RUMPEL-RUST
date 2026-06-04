use bevy::prelude::*;

pub mod feature_decor_invalidation;
pub mod legacy_render_modes;
pub mod packed_feature_overlay;
pub mod packed_quad_buffer;
pub mod packed_quad_gpu_generation;
pub mod packed_quad_material;
pub mod packed_quad_pipeline;
pub mod packed_quad_renderer;
pub mod surface_decor;
pub mod surface_streaming;
pub mod terrain_feature_overlay;
pub mod voxel_compute;
pub mod voxel_material;
pub mod voxel_packed_quads;

#[derive(Component)]
pub struct RenderedChunk;

#[derive(Component)]
pub struct RenderedChunkCount(pub usize);

pub struct RumpelRenderPlugin;

impl Plugin for RumpelRenderPlugin {
    fn build(&self, app: &mut App) {
        let render_mode = RumpelRenderMode::from_env();
        app.insert_resource(render_mode);

        if render_mode == RumpelRenderMode::Surface
            || render_mode == RumpelRenderMode::ComputePrototype
        {
            legacy_render_modes::add_legacy_render_plugins(app, render_mode);
            return;
        }

        info!("packed quad pipeline: enabled");
        app.add_plugins(packed_quad_pipeline::PackedQuadPipelinePlugin);
        app.init_resource::<packed_quad_pipeline::PackedQuadStreamingState>();
        if packed_quad_gpu_generation::packed_gpu_generation_enabled_from_env() {
            app.init_resource::<packed_quad_gpu_generation::GeneratedRegionCache>();
            app.add_systems(
                Update,
                packed_quad_pipeline::update_packed_gpu_generation_regions
                    .after(rumpel_world::chunk::record_world_block_edits)
                    .run_if(in_state(rumpel_prelude::GameState::InGame)),
            );
        } else {
            app.add_systems(
                Update,
                (
                    packed_quad_pipeline::handle_packed_quad_build_tasks,
                    packed_quad_pipeline::compact_pending_packed_regions,
                    packed_quad_pipeline::stream_packed_quad_chunks,
                )
                    .chain()
                    .run_if(in_state(rumpel_prelude::GameState::InGame)),
            );
        }

        if env_flag("RUMPEL_PACKED_QUAD_DEBUG") || env_flag("RUMPEL_PACKED_QUAD_RENDERER") {
            app.add_systems(
                Update,
                packed_quad_pipeline::setup_packed_quad_debug_producer
                    .run_if(in_state(rumpel_prelude::GameState::Loading)),
            );
        }

        info!("packed quad renderer: enabled");
        app.add_plugins(packed_quad_renderer::PackedQuadRendererPlugin);

        info!("packed lua feature overlay + decor: enabled");
        app.add_plugins((
            voxel_material::VoxelQuadMaterialPlugin,
            packed_feature_overlay::PackedFeatureOverlayPlugin,
            surface_decor::SurfaceDecorPlugin,
        ));

        app.add_systems(
            Update,
            (
                packed_feature_overlay::invalidate_edited_overlay_chunks,
                surface_decor::invalidate_edited_decor_chunks,
            )
                .after(rumpel_world::chunk::record_world_block_edits)
                .run_if(in_state(rumpel_prelude::GameState::InGame)),
        );

        info!("rumpel render mode: packed");
    }
}

const RENDER_MODE_ENV: &str = "RUMPEL_RENDER_MODE";
const COMPUTE_PROTOTYPE_ENV: &str = "RUMPEL_COMPUTE_PROTOTYPE";

#[derive(Resource, Clone, Copy, Debug, Eq, PartialEq)]
pub enum RumpelRenderMode {
    Surface,
    ComputePrototype,
    PackedPrototype,
    PackedMaterial,
}

impl RumpelRenderMode {
    pub fn from_env() -> Self {
        if let Ok(value) = std::env::var(RENDER_MODE_ENV) {
            return Self::from_mode_value(&value);
        }

        if env_flag(COMPUTE_PROTOTYPE_ENV) {
            warn!(
                "{COMPUTE_PROTOTYPE_ENV} is deprecated; using packed renderer (set {RENDER_MODE_ENV}=packed explicitly)"
            );
        }

        Self::PackedPrototype
    }

    pub fn from_mode_value(value: &str) -> Self {
        match value.to_ascii_lowercase().as_str() {
            "packed" | "packed-quad" | "packed-quads" | "packed-renderer" => Self::PackedPrototype,
            "surface" | "cpu" | "cpu-surface" => {
                warn!(render_mode = value, "legacy surface render mode requested");
                Self::Surface
            }
            "compute" | "compute-prototype" | "gpu" | "gpu-compute" => {
                warn!(render_mode = value, "legacy compute render mode requested");
                Self::ComputePrototype
            }
            "packed_material" | "packed-material" | "material" | "material-packed" => {
                warn!(
                    render_mode = value,
                    "packed-material mode is not implemented; using packed renderer"
                );
                Self::PackedPrototype
            }
            unknown => {
                warn!(
                    render_mode = unknown,
                    "unknown RUMPEL_RENDER_MODE; falling back to packed renderer"
                );
                Self::PackedPrototype
            }
        }
    }
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
    fn test_rumpel_render_mode_parsing() {
        assert_eq!(
            RumpelRenderMode::from_mode_value("packed"),
            RumpelRenderMode::PackedPrototype
        );
        assert_eq!(
            RumpelRenderMode::from_mode_value("packed-quads"),
            RumpelRenderMode::PackedPrototype
        );
        assert_eq!(
            RumpelRenderMode::from_mode_value("packed-renderer"),
            RumpelRenderMode::PackedPrototype
        );

        assert_eq!(
            RumpelRenderMode::from_mode_value("surface"),
            RumpelRenderMode::Surface
        );
        assert_eq!(
            RumpelRenderMode::from_mode_value("cpu"),
            RumpelRenderMode::Surface
        );
        assert_eq!(
            RumpelRenderMode::from_mode_value("compute"),
            RumpelRenderMode::ComputePrototype
        );
        assert_eq!(
            RumpelRenderMode::from_mode_value("gpu"),
            RumpelRenderMode::ComputePrototype
        );
        assert_eq!(
            RumpelRenderMode::from_mode_value("packed_material"),
            RumpelRenderMode::PackedPrototype
        );

        assert_eq!(
            RumpelRenderMode::from_mode_value("unknown_mode"),
            RumpelRenderMode::PackedPrototype
        );
    }
}
