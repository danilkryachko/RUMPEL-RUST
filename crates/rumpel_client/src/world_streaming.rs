use bevy::prelude::*;
use bevy::tasks::{AsyncComputeTaskPool, Task, futures::check_ready};
use rumpel_player::Player;
use rumpel_prelude::*;
use std::collections::HashMap;

const STREAMING_RADIUS_CHUNKS: i32 = 32;
const PREWARM_RADIUS_CHUNKS: i32 = 6;
const MAX_CHUNK_MESH_TASKS_PER_FRAME: usize = 8;
const MAX_PENDING_CHUNK_TASKS: usize = 32;
const MAX_READY_CHUNK_INSERTS_PER_FRAME: usize = 4;

pub struct RumpelWorldStreamingPlugin;

impl Plugin for RumpelWorldStreamingPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<RumpelWorldStreamingStats>()
            .init_resource::<TargetChunkSet>()
            .add_systems(Startup, setup_terrain_material)
            .add_systems(
                Update,
                (schedule_chunk_mesh_tasks, insert_ready_chunk_meshes).chain(),
            );
    }
}

#[derive(Resource)]
struct TerrainChunkMaterial(Handle<StandardMaterial>);

#[derive(Resource, Default)]
struct TargetChunkSet(HashMap<ChunkPos, rumpel_render::TerrainMeshDetail>);

#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct StreamedChunkDetail {
    pub sample_step: usize,
}

impl From<rumpel_render::TerrainMeshDetail> for StreamedChunkDetail {
    fn from(detail: rumpel_render::TerrainMeshDetail) -> Self {
        Self {
            sample_step: detail.sample_step,
        }
    }
}

impl StreamedChunkDetail {
    fn matches(self, target: rumpel_render::TerrainMeshDetail) -> bool {
        self.sample_step == target.sample_step
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct MacroChunkPos {
    pub x: i32,
    pub z: i32,
}

impl MacroChunkPos {
    pub fn from_chunk_pos(pos: ChunkPos) -> Self {
        Self {
            x: pos.x.div_euclid(4),
            z: pos.z.div_euclid(4),
        }
    }
}

#[derive(Component)]
pub(crate) struct RumpelMacroChunkEntity {
    pub pos: MacroChunkPos,
    pub constituent_chunks: Vec<ChunkPos>,
}

#[derive(Component)]
struct PendingMacroChunkMesh {
    pos: MacroChunkPos,
    chunks: Vec<ChunkPos>,
    detail: rumpel_render::TerrainMeshDetail,
    task: Task<Mesh>,
}

#[derive(Component)]
struct PendingChunkMesh {
    pos: ChunkPos,
    detail: rumpel_render::TerrainMeshDetail,
    task: Task<Mesh>,
}

fn setup_terrain_material(mut commands: Commands, mut materials: ResMut<Assets<StandardMaterial>>) {
    commands.insert_resource(TerrainChunkMaterial(materials.add(StandardMaterial {
        base_color: Color::WHITE,
        unlit: true,
        ..default()
    })));
}

fn schedule_chunk_mesh_tasks(
    mut commands: Commands,
    registry: Res<BlockRegistry>,
    player_query: Query<&Transform, With<Player>>,
    chunk_query: Query<(Entity, &RumpelChunkEntity, &StreamedChunkDetail)>,
    pending_query: Query<(Entity, &PendingChunkMesh)>,
    macro_chunk_query: Query<(Entity, &RumpelMacroChunkEntity, &StreamedChunkDetail)>,
    pending_macro_query: Query<(Entity, &PendingMacroChunkMesh)>,
    mut stats: ResMut<RumpelWorldStreamingStats>,
    mut target_resource: ResMut<TargetChunkSet>,
) {
    let Ok(player_transform) = player_query.single() else {
        return;
    };

    let center = WorldPos {
        position: player_transform.translation,
    }
    .to_chunk_pos(CHUNK_SIZE as i32);

    let target_chunks = target_chunk_positions(center);
    let target_map = target_chunks.iter().copied().collect::<HashMap<_, _>>();
    target_resource.0 = target_map.clone();

    // Separate individual targets (LOD=1, LOD=2) and batched targets (LOD=4)
    let mut individual_targets = Vec::new();
    let mut batched_targets_by_macro = HashMap::<MacroChunkPos, Vec<ChunkPos>>::new();

    for &(pos, detail) in &target_chunks {
        if detail.sample_step == 4 {
            let macro_pos = MacroChunkPos::from_chunk_pos(pos);
            batched_targets_by_macro.entry(macro_pos).or_default().push(pos);
        } else {
            individual_targets.push((pos, detail));
        }
    }

    let pending_individual_map = pending_query
        .iter()
        .map(|(_, pending)| (pending.pos, pending.detail))
        .collect::<HashMap<_, _>>();

    let pending_macro_map = pending_macro_query
        .iter()
        .map(|(_, pending)| (pending.pos, pending.chunks.clone()))
        .collect::<HashMap<_, _>>();

    let active_individual_map = chunk_query
        .iter()
        .map(|(_, chunk, detail)| (chunk.pos, *detail))
        .collect::<HashMap<_, _>>();

    let active_macro_map = macro_chunk_query
        .iter()
        .map(|(_, mc, detail)| (mc.pos, (mc.constituent_chunks.clone(), *detail)))
        .collect::<HashMap<_, _>>();

    // 1. Despawn out-of-range individual active chunks
    let mut despawned_chunks = 0usize;
    for (entity, chunk, _) in &chunk_query {
        let in_target_and_not_lod4 = target_map.get(&chunk.pos).is_some_and(|detail| detail.sample_step != 4);
        if !in_target_and_not_lod4 {
            commands.entity(entity).despawn();
            despawned_chunks += 1;
        }
    }

    // 2. Despawn out-of-range pending individual chunk tasks
    let mut despawned_pending_chunks = 0usize;
    for (entity, pending) in &pending_query {
        let Some(target_detail) = target_map.get(&pending.pos) else {
            commands.entity(entity).despawn();
            despawned_pending_chunks += 1;
            continue;
        };
        if target_detail.sample_step == 4 || pending.detail.sample_step != target_detail.sample_step {
            commands.entity(entity).despawn();
            despawned_pending_chunks += 1;
        }
    }

    // 3. Despawn out-of-range or obsolete active macro-chunks
    let mut despawned_macro_chunks = 0usize;
    for (entity, macro_chunk, _) in &macro_chunk_query {
        let Some(target_constituents) = batched_targets_by_macro.get(&macro_chunk.pos) else {
            commands.entity(entity).despawn();
            despawned_macro_chunks += 1;
            continue;
        };
        let mut match_ok = macro_chunk.constituent_chunks.len() == target_constituents.len();
        if match_ok {
            for p in &macro_chunk.constituent_chunks {
                if !target_constituents.contains(p) {
                    match_ok = false;
                    break;
                }
            }
        }
        if !match_ok {
            commands.entity(entity).despawn();
            despawned_macro_chunks += 1;
        }
    }

    // 4. Despawn out-of-range or obsolete pending macro-chunk tasks
    let mut despawned_pending_macro = 0usize;
    for (entity, pending) in &pending_macro_query {
        let Some(target_constituents) = batched_targets_by_macro.get(&pending.pos) else {
            commands.entity(entity).despawn();
            despawned_pending_macro += 1;
            continue;
        };
        let mut match_ok = pending.chunks.len() == target_constituents.len();
        if match_ok {
            for p in &pending.chunks {
                if !target_constituents.contains(p) {
                    match_ok = false;
                    break;
                }
            }
        }
        if !match_ok {
            commands.entity(entity).despawn();
            despawned_pending_macro += 1;
        }
    }

    // Check budget limit
    let total_pending = pending_individual_map.len().saturating_sub(despawned_pending_chunks)
        + pending_macro_map.len().saturating_sub(despawned_pending_macro);
    let available_task_slots = MAX_PENDING_CHUNK_TASKS.saturating_sub(total_pending);

    if available_task_slots == 0 {
        let active_count = active_individual_map.len().saturating_sub(despawned_chunks)
            + active_macro_map.len().saturating_sub(despawned_macro_chunks);
        stats.active_chunks = active_count;
        stats.pending_chunks = total_pending;
        stats.target_chunks = target_chunks.len();
        stats.queued_chunks = target_chunks.len().saturating_sub(active_count + total_pending);
        return;
    }

    let mesh_palette = rumpel_render::TerrainMeshPalette::from_registry(&registry);
    let thread_pool = AsyncComputeTaskPool::get();
    let mut scheduled_tasks = 0usize;

    // A. Schedule individual targets (LOD=1, LOD=2)
    for (pos, target_detail) in &individual_targets {
        if scheduled_tasks >= available_task_slots || scheduled_tasks >= MAX_CHUNK_MESH_TASKS_PER_FRAME {
            break;
        }

        let pos = *pos;
        let active_satisfied = active_individual_map
            .get(&pos)
            .is_some_and(|detail| detail.matches(*target_detail));
        let pending_satisfied = pending_individual_map
            .get(&pos)
            .is_some_and(|detail| detail.sample_step == target_detail.sample_step);

        if active_satisfied || pending_satisfied {
            continue;
        }

        let target_detail_val = *target_detail;
        let task = thread_pool.spawn(async move {
            rumpel_render::mesh_terrain_chunk_with_detail(pos, mesh_palette, target_detail_val)
        });

        commands.spawn(PendingChunkMesh {
            pos,
            detail: *target_detail,
            task,
        });
        scheduled_tasks += 1;
    }

    // B. Schedule macro-chunk targets (LOD=4)
    for (macro_pos, target_constituents) in &batched_targets_by_macro {
        if scheduled_tasks >= available_task_slots || scheduled_tasks >= MAX_CHUNK_MESH_TASKS_PER_FRAME {
            break;
        }

        let macro_pos = *macro_pos;
        let target_constituents = target_constituents.clone();

        let active_satisfied = active_macro_map.get(&macro_pos).is_some_and(|(constituents, _)| {
            let mut ok = constituents.len() == target_constituents.len();
            if ok {
                for p in constituents {
                    if !target_constituents.contains(p) {
                        ok = false;
                        break;
                    }
                }
            }
            ok
        });

        let pending_satisfied = pending_macro_map.get(&macro_pos).is_some_and(|constituents| {
            let mut ok = constituents.len() == target_constituents.len();
            if ok {
                for p in constituents {
                    if !target_constituents.contains(p) {
                        ok = false;
                        break;
                    }
                }
            }
            ok
        });

        if active_satisfied || pending_satisfied {
            continue;
        }

        let target_constituents_clone = target_constituents.clone();
        let task = thread_pool.spawn(async move {
            rumpel_render::mesh_terrain_macro_chunk_with_detail(
                &target_constituents_clone,
                mesh_palette,
                rumpel_render::TerrainMeshDetail::new(4),
            )
        });

        commands.spawn(PendingMacroChunkMesh {
            pos: macro_pos,
            chunks: target_constituents,
            detail: rumpel_render::TerrainMeshDetail::new(4),
            task,
        });
        scheduled_tasks += 1;
    }

    let active_after = active_individual_map.len().saturating_sub(despawned_chunks)
        + active_macro_map.len().saturating_sub(despawned_macro_chunks);
    let pending_after = total_pending + scheduled_tasks;

    stats.active_chunks = active_after;
    stats.pending_chunks = pending_after;
    stats.target_chunks = target_chunks.len();
    stats.queued_chunks = target_chunks.len().saturating_sub(active_after + pending_after);
}

fn insert_ready_chunk_meshes(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    material: Res<TerrainChunkMaterial>,
    target_chunks: Res<TargetChunkSet>,
    mut pending_query: Query<(Entity, &mut PendingChunkMesh)>,
    mut pending_macro_query: Query<(Entity, &mut PendingMacroChunkMesh)>,
    active_query: Query<(Entity, &RumpelChunkEntity), With<StreamedChunkDetail>>,
    active_macro_query: Query<(Entity, &RumpelMacroChunkEntity), With<StreamedChunkDetail>>,
    mut stats: ResMut<RumpelWorldStreamingStats>,
) {
    let mut inserted_chunks = 0usize;

    // 1. Insert individual chunks (LOD=1, LOD=2)
    for (entity, mut pending) in &mut pending_query {
        if inserted_chunks >= MAX_READY_CHUNK_INSERTS_PER_FRAME {
            break;
        }

        let Some(target_detail) = target_chunks.0.get(&pending.pos) else {
            commands.entity(entity).despawn();
            continue;
        };

        if pending.detail.sample_step != target_detail.sample_step {
            commands.entity(entity).despawn();
            continue;
        }

        let Some(mesh) = check_ready(&mut pending.task) else {
            continue;
        };

        commands.entity(entity).despawn();
        for (active_entity, active_chunk) in &active_query {
            if active_chunk.pos == pending.pos {
                commands.entity(active_entity).despawn();
            }
        }

        commands.spawn((
            Mesh3d(meshes.add(mesh)),
            MeshMaterial3d(material.0.clone()),
            Transform::from_xyz(
                (pending.pos.x * CHUNK_SIZE as i32) as f32,
                0.0,
                (pending.pos.z * CHUNK_SIZE as i32) as f32,
            ),
            RumpelChunkEntity { pos: pending.pos },
            StreamedChunkDetail::from(pending.detail),
        ));

        inserted_chunks += 1;
    }

    // 2. Insert macro-chunks (LOD=4)
    for (entity, mut pending) in &mut pending_macro_query {
        if inserted_chunks >= MAX_READY_CHUNK_INSERTS_PER_FRAME {
            break;
        }

        // Verify still valid
        let mut target_constituents = Vec::new();
        for (pos, detail) in &target_chunks.0 {
            if detail.sample_step == 4 {
                let m_pos = MacroChunkPos::from_chunk_pos(*pos);
                if m_pos == pending.pos {
                    target_constituents.push(*pos);
                }
            }
        }

        let match_ok = pending.chunks.len() == target_constituents.len()
            && pending.chunks.iter().all(|p| target_constituents.contains(p));

        if !match_ok {
            commands.entity(entity).despawn();
            continue;
        }

        let Some(mesh) = check_ready(&mut pending.task) else {
            continue;
        };

        commands.entity(entity).despawn();
        for (active_entity, active_mc) in &active_macro_query {
            if active_mc.pos == pending.pos {
                commands.entity(active_entity).despawn();
            }
        }

        commands.spawn((
            Mesh3d(meshes.add(mesh)),
            MeshMaterial3d(material.0.clone()),
            Transform::IDENTITY,
            RumpelMacroChunkEntity {
                pos: pending.pos,
                constituent_chunks: pending.chunks.clone(),
            },
            StreamedChunkDetail::from(pending.detail),
        ));

        inserted_chunks += 1;
    }

    stats.active_chunks += inserted_chunks;
    stats.pending_chunks = stats.pending_chunks.saturating_sub(inserted_chunks);
    stats.queued_chunks = stats
        .target_chunks
        .saturating_sub(stats.active_chunks + stats.pending_chunks);
}

fn target_chunk_positions(center: ChunkPos) -> Vec<(ChunkPos, rumpel_render::TerrainMeshDetail)> {
    let mut positions = Vec::new();

    for radius in 0..=STREAMING_RADIUS_CHUNKS {
        for dx in -radius..=radius {
            for dz in -radius..=radius {
                if dx.abs().max(dz.abs()) != radius {
                    continue;
                }

                let distance_squared = dx * dx + dz * dz;
                if radius > PREWARM_RADIUS_CHUNKS
                    && distance_squared > STREAMING_RADIUS_CHUNKS * STREAMING_RADIUS_CHUNKS
                {
                    continue;
                }

                positions.push((
                    ChunkPos::new(center.x + dx, center.z + dz),
                    terrain_detail_for_distance(distance_squared),
                ));
            }
        }
    }

    positions.sort_by_key(|(pos, detail)| {
        let dx = pos.x - center.x;
        let dz = pos.z - center.z;
        let distance_squared = dx * dx + dz * dz;
        (streaming_priority(*detail), distance_squared)
    });

    positions
}

fn streaming_priority(detail: rumpel_render::TerrainMeshDetail) -> u8 {
    match detail.sample_step {
        1 => 0,
        4 => 1,
        2 => 2,
        _ => 3,
    }
}

fn terrain_detail_for_distance(distance_squared: i32) -> rumpel_render::TerrainMeshDetail {
    if distance_squared <= 6 * 6 {
        rumpel_render::TerrainMeshDetail::new(1)
    } else if distance_squared <= 16 * 16 {
        rumpel_render::TerrainMeshDetail::new(2)
    } else {
        rumpel_render::TerrainMeshDetail::new(4)
    }
}
