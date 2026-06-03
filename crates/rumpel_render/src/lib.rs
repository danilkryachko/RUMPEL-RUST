use bevy::prelude::*;

pub mod packed_quad_buffer;
pub mod packed_quad_gpu_generation;
pub mod packed_quad_material;
pub mod packed_quad_pipeline;
pub mod packed_quad_renderer;
pub mod surface_streaming;
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

        if render_mode == RumpelRenderMode::PackedPrototype
            || render_mode == RumpelRenderMode::PackedMaterial
            || env_flag("RUMPEL_PACKED_QUAD_DEBUG")
            || env_flag("RUMPEL_PACKED_QUAD_RENDERER")
        {
            info!("packed quad pipeline/debug mode: enabled");
            app.add_plugins(packed_quad_pipeline::PackedQuadPipelinePlugin);

            if render_mode == RumpelRenderMode::PackedPrototype {
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
            } else if render_mode == RumpelRenderMode::PackedMaterial {
                // Инициализация ресурсов для материального режима
                app.init_resource::<packed_quad_pipeline::PackedQuadStreamingState>();
                app.init_resource::<packed_quad_pipeline::PackedMaterialEntities>();
                app.add_plugins(packed_quad_material::PackedVoxelMaterialPlugin);
                app.add_systems(
                    Update,
                    (
                        packed_quad_pipeline::handle_packed_quad_build_tasks,
                        packed_quad_pipeline::compact_pending_packed_regions,
                        packed_quad_pipeline::stream_packed_quad_chunks,
                        packed_quad_pipeline::sync_packed_material_entities,
                    )
                        .chain()
                        .run_if(in_state(rumpel_prelude::GameState::InGame)),
                );
            } else {
                app.add_systems(
                    Update,
                    packed_quad_pipeline::setup_packed_quad_debug_producer
                        .run_if(in_state(rumpel_prelude::GameState::Loading)),
                );
            }
        }

        if render_mode == RumpelRenderMode::PackedPrototype
            || env_flag("RUMPEL_PACKED_QUAD_RENDERER")
        {
            info!("packed quad renderer mode: enabled");
            app.add_plugins(packed_quad_renderer::PackedQuadRendererPlugin);
        }

        match render_mode {
            RumpelRenderMode::Surface => {
                info!("rumpel render mode: surface streaming");
                app.add_plugins((
                    voxel_material::VoxelQuadMaterialPlugin,
                    surface_streaming::SurfaceStreamingPlugin,
                ));
            }
            RumpelRenderMode::ComputePrototype => {
                info!("rumpel render mode: GPU compute prototype");
                app.add_plugins((
                    voxel_material::VoxelQuadMaterialPlugin,
                    voxel_compute::VoxelComputePlugin,
                ));
            }
            RumpelRenderMode::PackedPrototype => {
                info!("rumpel render mode: PackedVoxelQuad benchmark");
            }
            RumpelRenderMode::PackedMaterial => {
                info!("rumpel render mode: PackedVoxelQuad Custom Material Pipeline");
            }
        }
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
            Self::ComputePrototype
        } else {
            Self::Surface
        }
    }

    pub fn from_mode_value(value: &str) -> Self {
        match value.to_ascii_lowercase().as_str() {
            "surface" | "cpu" | "cpu-surface" => Self::Surface,
            "compute" | "compute-prototype" | "gpu" | "gpu-compute" => Self::ComputePrototype,
            "packed" | "packed-quad" | "packed-quads" | "packed-renderer" => Self::PackedPrototype,
            "packed_material" | "packed-material" | "material" | "material-packed" => {
                Self::PackedMaterial
            }
            unknown => {
                warn!(
                    render_mode = unknown,
                    "unknown RUMPEL_RENDER_MODE; falling back to surface streaming"
                );
                Self::Surface
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
            RumpelRenderMode::from_mode_value("surface"),
            RumpelRenderMode::Surface
        );
        assert_eq!(
            RumpelRenderMode::from_mode_value("CPU-surface"),
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
            RumpelRenderMode::from_mode_value("GPU-COMPUTE"),
            RumpelRenderMode::ComputePrototype
        );
        assert_eq!(
            RumpelRenderMode::from_mode_value("gpu"),
            RumpelRenderMode::ComputePrototype
        );

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
            RumpelRenderMode::from_mode_value("packed_material"),
            RumpelRenderMode::PackedMaterial
        );
        assert_eq!(
            RumpelRenderMode::from_mode_value("packed-material"),
            RumpelRenderMode::PackedMaterial
        );

        // Unknown mode values keep the explicit surface baseline.
        assert_eq!(
            RumpelRenderMode::from_mode_value("unknown_mode"),
            RumpelRenderMode::Surface
        );
    }
}
