//! Chunk-streamed Lua feature meshes for the packed terrain renderer.

use bevy::{
    platform::collections::HashMap,
    prelude::*,
    tasks::{AsyncComputeTaskPool, Task, futures::check_ready},
};
use rumpel_prelude::ChunkPos;
use rumpel_prelude::*;
use rumpel_world::chunk::WorldEditStore;
use std::sync::Arc;

use crate::{
    packed_quad_pipeline::packed_view_radius_chunks,
    terrain_feature_overlay::{FeatureOverlayContext, build_lua_feature_mesh_for_chunk},
    voxel_material::{VoxelQuadMaterial, load_block_atlas},
};

const DESPAWN_RADIUS_EXTRA_CHUNKS: i32 = 2;
const OVERLAY_CHUNKS_PER_FRAME: usize = 6;
const OVERLAY_UPLOADS_PER_FRAME: usize = 4;

pub struct PackedFeatureOverlayPlugin;

impl Plugin for PackedFeatureOverlayPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PackedFeatureOverlayState>()
            .init_resource::<PackedFeatureOverlayMetrics>()
            .add_systems(Startup, init_packed_feature_overlay_assets)
            .add_systems(
                Update,
                (
                    handle_packed_feature_overlay_tasks,
                    stream_packed_feature_overlay,
                )
                    .chain(),
            );
    }
}

#[derive(Component)]
struct ChunkFeatureOverlayRoot {
    chunk: IVec2,
}

#[derive(Component)]
struct FeatureOverlayBuildTask(Task<BuiltChunkFeatureOverlay>);

struct BuiltChunkFeatureOverlay {
    chunk: IVec2,
    mesh: Option<Mesh>,
}

#[derive(Clone, Copy)]
struct PendingFeatureChunk {
    pos: IVec2,
    distance_sq: i32,
}

#[derive(Resource)]
struct PackedFeatureOverlayAssets {
    material: Handle<VoxelQuadMaterial>,
}

#[derive(Resource, Default)]
pub(crate) struct PackedFeatureOverlayState {
    pub(crate) loaded: HashMap<IVec2, Entity>,
    pub(crate) building: HashMap<IVec2, Entity>,
    pending: Vec<PendingFeatureChunk>,
    last_center: Option<IVec2>,
    /// Generation from `WorldEditStore` last seen by the invalidation system.
    last_seen_edit_generation: u64,
}

#[derive(Resource, Default, Clone, Copy, Debug)]
pub struct PackedFeatureOverlayMetrics {
    pub loaded_chunks: usize,
    pub building_chunks: usize,
    pub pending_chunks: usize,
}

fn init_packed_feature_overlay_assets(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut materials: ResMut<Assets<VoxelQuadMaterial>>,
) {
    let atlas = load_block_atlas(&asset_server);
    commands.insert_resource(PackedFeatureOverlayAssets {
        material: materials.add(VoxelQuadMaterial { atlas }),
    });
}

fn handle_packed_feature_overlay_tasks(
    mut commands: Commands,
    overlay_assets: Res<PackedFeatureOverlayAssets>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut state: ResMut<PackedFeatureOverlayState>,
    mut metrics: ResMut<PackedFeatureOverlayMetrics>,
    mut tasks: Query<(
        Entity,
        &ChunkFeatureOverlayRoot,
        &mut FeatureOverlayBuildTask,
    )>,
) {
    let center = state.last_center;
    let view_radius = packed_view_radius_chunks();
    let despawn_radius_sq =
        (view_radius + DESPAWN_RADIUS_EXTRA_CHUNKS) * (view_radius + DESPAWN_RADIUS_EXTRA_CHUNKS);
    let mut uploaded = 0;

    for (entity, root, mut task) in &mut tasks {
        if uploaded >= OVERLAY_UPLOADS_PER_FRAME {
            break;
        }
        let Some(built) = check_ready(&mut task.0) else {
            continue;
        };
        state.building.remove(&root.chunk);

        let still_wanted = center
            .is_some_and(|center| chunk_distance_sq(center, built.chunk) <= despawn_radius_sq);
        if state.loaded.contains_key(&built.chunk) || !still_wanted {
            commands.entity(entity).despawn();
            continue;
        }

        let Some(mesh) = built.mesh else {
            commands.entity(entity).despawn();
            continue;
        };

        let chunk_origin = chunk_world_origin(built.chunk);
        commands
            .entity(entity)
            .remove::<FeatureOverlayBuildTask>()
            .insert((
                Transform::from_translation(chunk_origin),
                GlobalTransform::default(),
                Visibility::default(),
                InheritedVisibility::default(),
                ViewVisibility::default(),
                Mesh3d(meshes.add(mesh)),
                MeshMaterial3d(overlay_assets.material.clone()),
            ));

        state.loaded.insert(built.chunk, entity);
        uploaded += 1;
    }

    metrics.loaded_chunks = state.loaded.len();
    metrics.building_chunks = state.building.len();
    metrics.pending_chunks = state.pending.len();
}

fn stream_packed_feature_overlay(
    mut commands: Commands,
    registry: Res<BlockRegistry>,
    edit_store: Res<WorldEditStore>,
    camera_query: Query<&GlobalTransform, With<Camera3d>>,
    mut state: ResMut<PackedFeatureOverlayState>,
    mut metrics: ResMut<PackedFeatureOverlayMetrics>,
) {
    let Some(camera_transform) = camera_query.iter().next() else {
        return;
    };
    let camera_pos = camera_transform.translation();
    let center = IVec2::new(
        (camera_pos.x / CHUNK_SIZE as f32).floor() as i32,
        (camera_pos.z / CHUNK_SIZE as f32).floor() as i32,
    );

    if state.last_center != Some(center) {
        despawn_far_feature_overlay(center, &mut commands, &mut state);
        rebuild_pending_feature_overlay(center, &mut state);
        state.last_center = Some(center);
    }

    let world = Arc::new(FeatureOverlayContext::from_registry(&registry));
    let edit_store = Arc::new(edit_store.clone());
    let thread_pool = AsyncComputeTaskPool::get();

    let mut spawned = 0;
    while spawned < OVERLAY_CHUNKS_PER_FRAME {
        let Some(pending) = state.pending.pop() else {
            break;
        };
        if state.loaded.contains_key(&pending.pos) || state.building.contains_key(&pending.pos) {
            continue;
        }

        let chunk_pos = pending.pos;
        let world = Arc::clone(&world);
        let edit_store = Arc::clone(&edit_store);
        let task = thread_pool.spawn(async move {
            build_chunk_feature_overlay_mesh(chunk_pos, &world, edit_store.as_ref())
        });

        let entity = commands
            .spawn((
                ChunkFeatureOverlayRoot { chunk: chunk_pos },
                FeatureOverlayBuildTask(task),
            ))
            .id();
        state.building.insert(chunk_pos, entity);
        spawned += 1;
    }

    metrics.loaded_chunks = state.loaded.len();
    metrics.building_chunks = state.building.len();
    metrics.pending_chunks = state.pending.len();
}

fn build_chunk_feature_overlay_mesh(
    chunk: IVec2,
    world: &FeatureOverlayContext,
    edit_store: &WorldEditStore,
) -> BuiltChunkFeatureOverlay {
    let chunk_pos = ChunkPos {
        x: chunk.x,
        z: chunk.y,
    };
    let origin_x = chunk.x * CHUNK_SIZE as i32;
    let origin_z = chunk.y * CHUNK_SIZE as i32;
    let mesh = build_lua_feature_mesh_for_chunk(chunk_pos, origin_x, origin_z, world, edit_store);

    BuiltChunkFeatureOverlay { chunk, mesh }
}

/// Despawn entities for chunks dirtied by world block edits and re-queue them for rebuild.
pub(crate) fn invalidate_edited_overlay_chunks(
    edit_store: Res<WorldEditStore>,
    mut commands: Commands,
    mut state: ResMut<PackedFeatureOverlayState>,
) {
    let current_gen = edit_store.generation();
    if current_gen == state.last_seen_edit_generation {
        return;
    }
    let old_gen = state.last_seen_edit_generation;
    state.last_seen_edit_generation = current_gen;

    let dirty =
        crate::feature_decor_invalidation::dirty_layer_chunks_since(
            &edit_store,
            old_gen,
            &state.loaded,
            &state.building,
        );

    for pos in dirty {
        if let Some(entity) = state.loaded.remove(&pos) {
            commands.entity(entity).despawn();
        }
        if let Some(entity) = state.building.remove(&pos) {
            commands.entity(entity).despawn();
        }
        if let Some(center) = state.last_center {
            let distance_sq = {
                let dx = center.x - pos.x;
                let dz = center.y - pos.y;
                dx * dx + dz * dz
            };
            let view_radius = packed_view_radius_chunks();
            if distance_sq <= view_radius * view_radius {
                state.pending.push(PendingFeatureChunk { pos, distance_sq });
            }
        }
    }
}

fn rebuild_pending_feature_overlay(center: IVec2, state: &mut PackedFeatureOverlayState) {
    let view_radius = packed_view_radius_chunks();
    let radius_sq = view_radius * view_radius;
    let mut wanted = HashMap::<IVec2, i32>::default();

    for dz in -view_radius..=view_radius {
        for dx in -view_radius..=view_radius {
            let distance_sq = dx * dx + dz * dz;
            if distance_sq > radius_sq {
                continue;
            }
            let chunk_pos = center + IVec2::new(dx, dz);
            wanted.insert(chunk_pos, distance_sq);
        }
    }

    let mut pending: Vec<_> = wanted
        .into_iter()
        .filter_map(|(pos, distance_sq)| {
            (!state.loaded.contains_key(&pos) && !state.building.contains_key(&pos))
                .then_some(PendingFeatureChunk { pos, distance_sq })
        })
        .collect();

    pending.sort_by(|left, right| {
        left.distance_sq
            .cmp(&right.distance_sq)
            .then_with(|| left.pos.y.cmp(&right.pos.y))
            .then_with(|| left.pos.x.cmp(&right.pos.x))
    });
    state.pending = pending;
}

fn despawn_far_feature_overlay(
    center: IVec2,
    commands: &mut Commands,
    state: &mut PackedFeatureOverlayState,
) {
    let view_radius = packed_view_radius_chunks();
    let despawn_radius_sq =
        (view_radius + DESPAWN_RADIUS_EXTRA_CHUNKS) * (view_radius + DESPAWN_RADIUS_EXTRA_CHUNKS);

    let loaded: Vec<_> = state
        .loaded
        .iter()
        .filter(|(pos, _)| chunk_distance_sq(center, **pos) > despawn_radius_sq)
        .map(|(pos, entity)| (*pos, *entity))
        .collect();
    for (pos, entity) in loaded {
        commands.entity(entity).despawn();
        state.loaded.remove(&pos);
    }

    let building: Vec<_> = state
        .building
        .iter()
        .filter(|(pos, _)| chunk_distance_sq(center, **pos) > despawn_radius_sq)
        .map(|(pos, entity)| (*pos, *entity))
        .collect();
    for (pos, entity) in building {
        commands.entity(entity).despawn();
        state.building.remove(&pos);
    }
}

fn chunk_distance_sq(center: IVec2, chunk: IVec2) -> i32 {
    let dx = center.x - chunk.x;
    let dz = center.y - chunk.y;
    dx * dx + dz * dz
}

fn chunk_world_origin(chunk: IVec2) -> Vec3 {
    Vec3::new(
        chunk.x as f32 * CHUNK_SIZE as f32,
        0.0,
        chunk.y as f32 * CHUNK_SIZE as f32,
    )
}
