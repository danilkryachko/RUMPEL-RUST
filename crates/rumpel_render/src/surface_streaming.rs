use bevy::{
    platform::collections::HashMap,
    prelude::*,
    tasks::{AsyncComputeTaskPool, Task, futures::check_ready},
};
use noise::Perlin;
use rumpel_prelude::*;
use std::time::Instant;

use crate::{
    RenderedChunk, RenderedChunkCount,
    voxel_material::{
        ATTRIBUTE_VOXEL_REPEAT_UV, ATTRIBUTE_VOXEL_TILE, VoxelQuadMaterial, load_block_atlas,
    },
};

const VIEW_RADIUS_CHUNKS: i32 = 16;
const DESPAWN_RADIUS_CHUNKS: i32 = VIEW_RADIUS_CHUNKS + 2;
const REGIONS_PER_FRAME: usize = 12;
const REGION_MESH_UPLOADS_PER_FRAME: usize = 4;
const REGION_SIZE_CHUNKS: i32 = 4;
const REGION_CHUNK_COUNT: usize = (REGION_SIZE_CHUNKS * REGION_SIZE_CHUNKS) as usize;
const TERRAIN_CHUNK_SIZE: i32 = CHUNK_SIZE as i32;
const MID_LOD_DISTANCE_CHUNKS: i32 = 4;
const LOW_LOD_DISTANCE_CHUNKS: i32 = 8;
const FAR_LOD_DISTANCE_CHUNKS: i32 = 12;

pub struct SurfaceStreamingPlugin;

impl Plugin for SurfaceStreamingPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SurfaceStreamingState>()
            .init_resource::<SurfaceStreamingMetrics>()
            .add_systems(
                Update,
                (handle_region_mesh_tasks, stream_surface_chunks).chain(),
            );
    }
}

#[derive(Component)]
pub struct SurfaceChunkMesh {
    pub pos: IVec2,
}

#[derive(Component)]
struct SurfaceMeshBuildTask(Task<SurfaceRegionMesh>);

#[derive(Clone, Copy)]
struct PendingRegion {
    pos: IVec2,
    nearest_distance_sq: i32,
}

struct SurfaceRegionMesh {
    pos: IVec2,
    mesh: Mesh,
    textured: bool,
    build_us: u64,
    vertex_count: usize,
    index_count: usize,
    lod_step: usize,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct SurfaceColumn {
    local_x: usize,
    local_z: usize,
    cell_size: usize,
    height: i32,
    top_block: BlockId,
}

#[derive(Clone, Copy)]
struct SurfaceWall {
    low_y: i32,
    high_y: i32,
    cell_size: usize,
}

#[derive(Clone, Copy)]
struct SideWallQuad {
    x: f32,
    z: f32,
    width: f32,
    depth: f32,
    low_y: f32,
    high_y: f32,
    face: Face,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct WallMaskCell {
    block: BlockId,
    cell_size: usize,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct TopMaskKey {
    height: i32,
    block: BlockId,
    cell_size: usize,
}

#[derive(Clone, Copy)]
struct MaskRect {
    x: usize,
    y: usize,
    width: usize,
    height: usize,
}

struct BinaryMask2d {
    width: usize,
    height: usize,
    words_per_row: usize,
    words: Vec<u64>,
}

impl BinaryMask2d {
    fn new(width: usize, height: usize) -> Self {
        let words_per_row = width.div_ceil(u64::BITS as usize).max(1);
        Self {
            width,
            height,
            words_per_row,
            words: vec![0; words_per_row * height],
        }
    }

    fn set(&mut self, x: usize, y: usize) {
        if x >= self.width || y >= self.height {
            return;
        }
        let index = self.word_index(x, y);
        self.words[index] |= 1_u64 << (x % u64::BITS as usize);
    }

    fn get(&self, x: usize, y: usize) -> bool {
        if x >= self.width || y >= self.height {
            return false;
        }
        let index = self.word_index(x, y);
        (self.words[index] & (1_u64 << (x % u64::BITS as usize))) != 0
    }

    fn drain_greedy_rects(&mut self) -> Vec<MaskRect> {
        let mut rects = Vec::new();
        while let Some((x, y)) = self.first_set() {
            let width = self.run_width(x, y);
            let mut height = 1;
            while y + height < self.height && self.row_has_run(x, y + height, width) {
                height += 1;
            }
            self.clear_rect(MaskRect {
                x,
                y,
                width,
                height,
            });
            rects.push(MaskRect {
                x,
                y,
                width,
                height,
            });
        }
        rects
    }

    fn first_set(&self) -> Option<(usize, usize)> {
        for y in 0..self.height {
            let row_start = y * self.words_per_row;
            for word_x in 0..self.words_per_row {
                let word = self.words[row_start + word_x];
                if word == 0 {
                    continue;
                }
                let bit = usize::try_from(word.trailing_zeros()).unwrap_or(0);
                let x = word_x * u64::BITS as usize + bit;
                if x < self.width {
                    return Some((x, y));
                }
            }
        }
        None
    }

    fn run_width(&self, x: usize, y: usize) -> usize {
        let mut width = 0;
        while x + width < self.width && self.get(x + width, y) {
            width += 1;
        }
        width
    }

    fn row_has_run(&self, x: usize, y: usize, width: usize) -> bool {
        (0..width).all(|offset| self.get(x + offset, y))
    }

    fn clear_rect(&mut self, rect: MaskRect) {
        for y in rect.y..rect.y + rect.height {
            for x in rect.x..rect.x + rect.width {
                let index = self.word_index(x, y);
                self.words[index] &= !(1_u64 << (x % u64::BITS as usize));
            }
        }
    }

    fn word_index(&self, x: usize, y: usize) -> usize {
        y * self.words_per_row + x / u64::BITS as usize
    }
}

struct WallMaskLine<'a> {
    columns: &'a [Option<SurfaceColumn>],
    grid_size: UVec2,
    axis: WallAxis,
    fixed_cell: usize,
    face: Face,
    region_origin: IVec2,
    perlin: &'a Perlin,
    textured: bool,
    context: &'a SurfaceBuildContext,
}

#[derive(Clone, Copy)]
enum WallAxis {
    X,
    Z,
}

#[derive(Resource, Default)]
struct SurfaceStreamingState {
    loaded: HashMap<IVec2, Entity>,
    building: HashMap<IVec2, Entity>,
    pending: Vec<PendingRegion>,
    last_center: Option<IVec2>,
    terrain_material: Option<Handle<VoxelQuadMaterial>>,
}

#[derive(Resource, Default, Clone, Copy, Debug)]
pub struct SurfaceStreamingMetrics {
    pub loaded_regions: usize,
    pub rendered_chunks: usize,
    pub building_regions: usize,
    pub pending_regions: usize,
    pub spawned_regions_last_frame: usize,
    pub uploaded_regions_last_frame: usize,
    pub discarded_finished_regions_last_frame: usize,
    pub despawned_loaded_last_frame: usize,
    pub despawned_building_last_frame: usize,
    pub stream_system_us_last_frame: u64,
    pub upload_system_us_last_frame: u64,
    pub completed_build_us_last_frame_sum: u64,
    pub completed_build_us_last_frame_max: u64,
    pub completed_vertices_last_frame: usize,
    pub completed_indices_last_frame: usize,
    pub completed_lod_step_max: usize,
    pub completed_textured_last_frame: usize,
    pub total_spawned_regions: u64,
    pub total_uploaded_regions: u64,
    pub total_discarded_finished_regions: u64,
    pub total_despawned_loaded_regions: u64,
    pub total_despawned_building_regions: u64,
    pub total_stream_system_us: u64,
    pub total_upload_system_us: u64,
    pub total_completed_build_us: u64,
    pub total_completed_vertices: u64,
    pub total_completed_indices: u64,
}

fn update_surface_queue_metrics(
    metrics: &mut SurfaceStreamingMetrics,
    state: &SurfaceStreamingState,
) {
    metrics.loaded_regions = state.loaded.len();
    metrics.rendered_chunks = state.loaded.len() * REGION_CHUNK_COUNT;
    metrics.building_regions = state.building.len();
    metrics.pending_regions = state.pending.len();
}

fn elapsed_us(start: Instant) -> u64 {
    u64::try_from(start.elapsed().as_micros()).unwrap_or(u64::MAX)
}

fn handle_region_mesh_tasks(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<VoxelQuadMaterial>>,
    mut state: ResMut<SurfaceStreamingState>,
    mut metrics: ResMut<SurfaceStreamingMetrics>,
    mut tasks: Query<(Entity, &SurfaceChunkMesh, &mut SurfaceMeshBuildTask)>,
) {
    let system_start = Instant::now();
    metrics.uploaded_regions_last_frame = 0;
    metrics.discarded_finished_regions_last_frame = 0;
    metrics.upload_system_us_last_frame = 0;
    metrics.completed_build_us_last_frame_sum = 0;
    metrics.completed_build_us_last_frame_max = 0;
    metrics.completed_vertices_last_frame = 0;
    metrics.completed_indices_last_frame = 0;
    metrics.completed_lod_step_max = 0;
    metrics.completed_textured_last_frame = 0;

    let terrain_material = terrain_surface_material(&mut state, &asset_server, &mut materials);
    let center = state.last_center;
    let mut uploaded_regions = 0;

    for (entity, chunk, mut task) in &mut tasks {
        if uploaded_regions >= REGION_MESH_UPLOADS_PER_FRAME {
            break;
        }

        let Some(region_mesh) = check_ready(&mut task.0) else {
            continue;
        };
        let SurfaceRegionMesh {
            pos,
            mesh,
            textured,
            build_us,
            vertex_count,
            index_count,
            lod_step,
        } = region_mesh;

        state.building.remove(&chunk.pos);

        let still_wanted = center.is_some_and(|center| {
            region_nearest_distance_sq(center, pos) <= DESPAWN_RADIUS_CHUNKS * DESPAWN_RADIUS_CHUNKS
        });
        if state.loaded.contains_key(&pos) || !still_wanted {
            commands.entity(entity).despawn();
            metrics.discarded_finished_regions_last_frame += 1;
            metrics.total_discarded_finished_regions += 1;
            continue;
        }

        let handle = meshes.add(mesh);
        commands
            .entity(entity)
            .remove::<SurfaceMeshBuildTask>()
            .insert((
                RenderedChunk,
                RenderedChunkCount(REGION_CHUNK_COUNT),
                Mesh3d(handle),
                MeshMaterial3d(terrain_material.clone()),
                Transform::from_xyz(
                    (pos.x * REGION_SIZE_CHUNKS * TERRAIN_CHUNK_SIZE) as f32,
                    0.0,
                    (pos.y * REGION_SIZE_CHUNKS * TERRAIN_CHUNK_SIZE) as f32,
                ),
            ));
        state.loaded.insert(pos, entity);
        uploaded_regions += 1;
        metrics.uploaded_regions_last_frame += 1;
        metrics.completed_build_us_last_frame_sum += build_us;
        metrics.completed_build_us_last_frame_max =
            metrics.completed_build_us_last_frame_max.max(build_us);
        metrics.completed_vertices_last_frame += vertex_count;
        metrics.completed_indices_last_frame += index_count;
        metrics.completed_lod_step_max = metrics.completed_lod_step_max.max(lod_step);
        metrics.completed_textured_last_frame += usize::from(textured);
        metrics.total_uploaded_regions += 1;
        metrics.total_completed_build_us += build_us;
        metrics.total_completed_vertices += u64::try_from(vertex_count).unwrap_or(u64::MAX);
        metrics.total_completed_indices += u64::try_from(index_count).unwrap_or(u64::MAX);
    }

    update_surface_queue_metrics(&mut metrics, &state);
    metrics.upload_system_us_last_frame = elapsed_us(system_start);
    metrics.total_upload_system_us += metrics.upload_system_us_last_frame;
}

fn stream_surface_chunks(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut materials: ResMut<Assets<VoxelQuadMaterial>>,
    registry: Res<BlockRegistry>,
    camera_query: Query<&GlobalTransform, With<Camera3d>>,
    mut state: ResMut<SurfaceStreamingState>,
    mut metrics: ResMut<SurfaceStreamingMetrics>,
) {
    let system_start = Instant::now();
    metrics.spawned_regions_last_frame = 0;
    metrics.stream_system_us_last_frame = 0;
    metrics.despawned_loaded_last_frame = 0;
    metrics.despawned_building_last_frame = 0;

    let Some(camera_transform) = camera_query.iter().next() else {
        return;
    };
    let camera_pos = camera_transform.translation();
    let center = IVec2::new(
        (camera_pos.x / CHUNK_SIZE as f32).floor() as i32,
        (camera_pos.z / CHUNK_SIZE as f32).floor() as i32,
    );

    if state.last_center != Some(center) {
        let despawn_counts = despawn_far_regions(center, &mut commands, &mut state);
        metrics.despawned_loaded_last_frame = despawn_counts.loaded;
        metrics.despawned_building_last_frame = despawn_counts.building;
        metrics.total_despawned_loaded_regions +=
            u64::try_from(despawn_counts.loaded).unwrap_or(u64::MAX);
        metrics.total_despawned_building_regions +=
            u64::try_from(despawn_counts.building).unwrap_or(u64::MAX);
        rebuild_pending_regions(center, &mut state);
        state.last_center = Some(center);
    }

    let _ = terrain_surface_material(&mut state, &asset_server, &mut materials);
    let build_context = SurfaceBuildContext::from_registry(&registry);
    let thread_pool = AsyncComputeTaskPool::get();

    let mut spawned_regions = 0;
    while spawned_regions < REGIONS_PER_FRAME {
        let Some(region) = state.pending.pop() else {
            break;
        };
        if state.loaded.contains_key(&region.pos) || state.building.contains_key(&region.pos) {
            continue;
        }

        let context = build_context.clone();
        let lod_step = lod_step_for_distance_sq(region.nearest_distance_sq);
        let task = thread_pool.spawn(async move {
            let build_start = Instant::now();
            let textured = true;
            let (mesh, stats) = build_surface_region_mesh(region.pos, lod_step, textured, &context);
            SurfaceRegionMesh {
                pos: region.pos,
                mesh,
                textured,
                build_us: elapsed_us(build_start),
                vertex_count: stats.vertices,
                index_count: stats.indices,
                lod_step,
            }
        });

        let entity = commands
            .spawn((
                SurfaceChunkMesh { pos: region.pos },
                SurfaceMeshBuildTask(task),
            ))
            .id();
        state.building.insert(region.pos, entity);
        spawned_regions += 1;
    }

    metrics.spawned_regions_last_frame = spawned_regions;
    metrics.total_spawned_regions += u64::try_from(spawned_regions).unwrap_or(u64::MAX);
    update_surface_queue_metrics(&mut metrics, &state);
    metrics.stream_system_us_last_frame = elapsed_us(system_start);
    metrics.total_stream_system_us += metrics.stream_system_us_last_frame;
}

fn terrain_surface_material(
    state: &mut SurfaceStreamingState,
    asset_server: &AssetServer,
    materials: &mut Assets<VoxelQuadMaterial>,
) -> Handle<VoxelQuadMaterial> {
    state
        .terrain_material
        .get_or_insert_with(|| {
            materials.add(VoxelQuadMaterial {
                atlas: load_block_atlas(asset_server),
            })
        })
        .clone()
}

fn rebuild_pending_regions(center: IVec2, state: &mut SurfaceStreamingState) {
    let radius_sq = VIEW_RADIUS_CHUNKS * VIEW_RADIUS_CHUNKS;
    let mut wanted = HashMap::<IVec2, i32>::default();

    for dz in -VIEW_RADIUS_CHUNKS..=VIEW_RADIUS_CHUNKS {
        for dx in -VIEW_RADIUS_CHUNKS..=VIEW_RADIUS_CHUNKS {
            let distance_sq = dx * dx + dz * dz;
            if distance_sq > radius_sq {
                continue;
            }

            let chunk_pos = center + IVec2::new(dx, dz);
            let region_pos = chunk_to_region(chunk_pos);
            wanted
                .entry(region_pos)
                .and_modify(|nearest_distance_sq| {
                    *nearest_distance_sq = (*nearest_distance_sq).min(distance_sq);
                })
                .or_insert(distance_sq);
        }
    }

    let mut pending: Vec<_> = wanted
        .into_iter()
        .filter_map(|(pos, nearest_distance_sq)| {
            (!state.loaded.contains_key(&pos) && !state.building.contains_key(&pos)).then_some(
                PendingRegion {
                    pos,
                    nearest_distance_sq,
                },
            )
        })
        .collect();

    pending.sort_by(|left, right| {
        right
            .nearest_distance_sq
            .cmp(&left.nearest_distance_sq)
            .then_with(|| right.pos.y.cmp(&left.pos.y))
            .then_with(|| right.pos.x.cmp(&left.pos.x))
    });

    state.pending = pending;
}

#[derive(Default)]
struct SurfaceDespawnCounts {
    loaded: usize,
    building: usize,
}

fn despawn_far_regions(
    center: IVec2,
    commands: &mut Commands,
    state: &mut SurfaceStreamingState,
) -> SurfaceDespawnCounts {
    let despawn_radius_sq = DESPAWN_RADIUS_CHUNKS * DESPAWN_RADIUS_CHUNKS;
    let mut loaded_to_despawn = Vec::new();
    let mut building_to_despawn = Vec::new();

    for (&region_pos, &entity) in &state.loaded {
        if region_nearest_distance_sq(center, region_pos) > despawn_radius_sq {
            loaded_to_despawn.push((region_pos, entity));
        }
    }
    for (&region_pos, &entity) in &state.building {
        if region_nearest_distance_sq(center, region_pos) > despawn_radius_sq {
            building_to_despawn.push((region_pos, entity));
        }
    }

    let counts = SurfaceDespawnCounts {
        loaded: loaded_to_despawn.len(),
        building: building_to_despawn.len(),
    };

    for (region_pos, entity) in loaded_to_despawn {
        commands.entity(entity).despawn();
        state.loaded.remove(&region_pos);
    }
    for (region_pos, entity) in building_to_despawn {
        commands.entity(entity).despawn();
        state.building.remove(&region_pos);
    }

    counts
}

fn chunk_to_region(chunk_pos: IVec2) -> IVec2 {
    IVec2::new(
        chunk_pos.x.div_euclid(REGION_SIZE_CHUNKS),
        chunk_pos.y.div_euclid(REGION_SIZE_CHUNKS),
    )
}

fn region_nearest_distance_sq(center: IVec2, region_pos: IVec2) -> i32 {
    let min = region_pos * REGION_SIZE_CHUNKS;
    let max = min + IVec2::splat(REGION_SIZE_CHUNKS - 1);
    let nearest = IVec2::new(center.x.clamp(min.x, max.x), center.y.clamp(min.y, max.y));
    let offset = nearest - center;
    offset.x * offset.x + offset.y * offset.y
}

fn lod_step_for_distance_sq(distance_sq: i32) -> usize {
    if distance_sq >= FAR_LOD_DISTANCE_CHUNKS * FAR_LOD_DISTANCE_CHUNKS {
        8
    } else if distance_sq >= LOW_LOD_DISTANCE_CHUNKS * LOW_LOD_DISTANCE_CHUNKS {
        4
    } else if distance_sq >= MID_LOD_DISTANCE_CHUNKS * MID_LOD_DISTANCE_CHUNKS {
        2
    } else {
        1
    }
}

fn build_surface_region_mesh(
    region_pos: IVec2,
    requested_lod_step: usize,
    textured: bool,
    context: &SurfaceBuildContext,
) -> (Mesh, SurfaceMeshStats) {
    let columns_per_region = (REGION_SIZE_CHUNKS as usize) * CHUNK_SIZE;
    let lod_step = requested_lod_step.clamp(1, CHUNK_SIZE);
    let estimated_columns = columns_per_region * columns_per_region;
    let estimated_lod_columns = estimated_columns / (lod_step * lod_step).max(1);
    let mut buffers = SurfaceMeshBuffers::with_block_capacity(estimated_lod_columns);
    let perlin = Perlin::new(1337);

    let region_origin_x = region_pos.x * REGION_SIZE_CHUNKS * TERRAIN_CHUNK_SIZE;
    let region_origin_z = region_pos.y * REGION_SIZE_CHUNKS * TERRAIN_CHUNK_SIZE;
    let lua_chunk_offset = lua_chunk_offset_in_region(region_pos);

    let cells_x = columns_per_region.div_ceil(lod_step);
    let cells_z = columns_per_region.div_ceil(lod_step);
    let mut columns = vec![None; cells_x * cells_z];

    for cell_z in 0..cells_z {
        let local_z = cell_z * lod_step;
        for cell_x in 0..cells_x {
            let local_x = cell_x * lod_step;
            let cell_size = lod_step
                .min(columns_per_region - local_x)
                .min(columns_per_region - local_z);

            if lua_chunk_offset.is_some_and(|offset| {
                cell_intersects_lua_chunk(local_x, local_z, cell_size, offset)
            }) {
                continue;
            }

            let world_x = region_origin_x + local_x as i32;
            let world_z = region_origin_z + local_z as i32;
            let height = terrain_height_with_noise(world_x, world_z, &perlin) as i32;
            let top_block = if height <= 14 {
                context.sand
            } else {
                context.grass
            };

            columns[cell_z * cells_x + cell_x] = Some(SurfaceColumn {
                local_x,
                local_z,
                cell_size,
                height,
                top_block,
            });
        }
    }

    add_binary_greedy_top_faces(
        &mut buffers,
        &columns,
        UVec2::new(cells_x as u32, cells_z as u32),
        textured,
        context,
    );

    add_greedy_surface_walls(
        &mut buffers,
        &columns,
        UVec2::new(cells_x as u32, cells_z as u32),
        IVec2::new(region_origin_x, region_origin_z),
        &perlin,
        textured,
        context,
    );

    if let Some(offset) = lua_chunk_offset {
        add_lua_world_gen_chunk(&mut buffers, offset, textured, context);
    }

    let stats = buffers.stats();
    (buffers.into_mesh(), stats)
}

fn add_binary_greedy_top_faces(
    buffers: &mut SurfaceMeshBuffers,
    columns: &[Option<SurfaceColumn>],
    grid_size: UVec2,
    textured: bool,
    context: &SurfaceBuildContext,
) {
    let cells_x = grid_size.x as usize;
    let cells_z = grid_size.y as usize;
    let mut masks = HashMap::<TopMaskKey, BinaryMask2d>::default();

    for cell_z in 0..cells_z {
        for cell_x in 0..cells_x {
            let Some(column) = columns[cell_z * cells_x + cell_x] else {
                continue;
            };
            let key = TopMaskKey {
                height: column.height,
                block: column.top_block,
                cell_size: column.cell_size,
            };
            masks
                .entry(key)
                .or_insert_with(|| BinaryMask2d::new(cells_x, cells_z))
                .set(cell_x, cell_z);
        }
    }

    for (key, mut mask) in masks {
        for rect in mask.drain_greedy_rects() {
            let Some(column) = columns[rect.y * cells_x + rect.x] else {
                continue;
            };
            buffers.add_top_tile(
                column.local_x as f32,
                column.height as f32,
                column.local_z as f32,
                (rect.width * key.cell_size) as f32,
                (rect.height * key.cell_size) as f32,
                context.face_style(key.block, FaceTexture::Top, textured),
            );
        }
    }
}

fn add_greedy_surface_walls(
    buffers: &mut SurfaceMeshBuffers,
    columns: &[Option<SurfaceColumn>],
    grid_size: UVec2,
    region_origin: IVec2,
    perlin: &Perlin,
    textured: bool,
    context: &SurfaceBuildContext,
) {
    let cells_x = grid_size.x as usize;
    let cells_z = grid_size.y as usize;

    for face in [Face::North, Face::South] {
        for cell_z in 0..cells_z {
            add_greedy_wall_mask_line(
                buffers,
                WallMaskLine {
                    columns,
                    grid_size: UVec2::new(cells_x as u32, cells_z as u32),
                    axis: WallAxis::X,
                    fixed_cell: cell_z,
                    face,
                    region_origin,
                    perlin,
                    textured,
                    context,
                },
            );
        }
    }

    for face in [Face::West, Face::East] {
        for cell_x in 0..cells_x {
            add_greedy_wall_mask_line(
                buffers,
                WallMaskLine {
                    columns,
                    grid_size: UVec2::new(cells_x as u32, cells_z as u32),
                    axis: WallAxis::Z,
                    fixed_cell: cell_x,
                    face,
                    region_origin,
                    perlin,
                    textured,
                    context,
                },
            );
        }
    }
}

fn add_greedy_wall_mask_line(buffers: &mut SurfaceMeshBuffers, line: WallMaskLine<'_>) {
    let cells_x = line.grid_size.x as usize;
    let cells_z = line.grid_size.y as usize;
    let axis_len = match line.axis {
        WallAxis::X => cells_x,
        WallAxis::Z => cells_z,
    };
    let mut spans = vec![None; axis_len];
    let mut min_y = i32::MAX;
    let mut max_y = i32::MIN;

    for (axis_cell, span) in spans.iter_mut().enumerate() {
        let (cell_x, cell_z) = wall_mask_cell_coords(line.axis, axis_cell, line.fixed_cell);
        let Some(column) = line.columns[cell_z * cells_x + cell_x] else {
            continue;
        };
        let Some(wall) =
            surface_wall_for_column(column, line.face, line.region_origin, line.perlin)
        else {
            continue;
        };
        min_y = min_y.min(wall.low_y);
        max_y = max_y.max(wall.high_y);
        *span = Some((column, wall));
    }

    if max_y <= min_y {
        return;
    }
    let Ok(mask_height) = usize::try_from(max_y - min_y) else {
        return;
    };
    if mask_height == 0 {
        return;
    }

    let mut masks = Vec::<(WallMaskCell, BinaryMask2d)>::new();
    for (axis_cell, span) in spans.iter().enumerate() {
        let Some((column, wall)) = span else {
            continue;
        };
        for y in wall.low_y..wall.high_y {
            let Some(block) = surface_wall_block_at_y(*column, y, line.context) else {
                continue;
            };
            let Ok(mask_y) = usize::try_from(y - min_y) else {
                continue;
            };
            let cell = WallMaskCell {
                block,
                cell_size: wall.cell_size,
            };
            let mask_index = masks
                .iter()
                .position(|(key, _mask)| *key == cell)
                .unwrap_or_else(|| {
                    masks.push((cell, BinaryMask2d::new(axis_len, mask_height)));
                    masks.len() - 1
                });
            masks[mask_index].1.set(axis_cell, mask_y);
        }
    }

    for (cell, mut mask) in masks {
        for rect in mask.drain_greedy_rects() {
            let Some((column, wall)) = spans[rect.x] else {
                continue;
            };
            let low_y = min_y + i32::try_from(rect.y).unwrap_or(0);
            let high_y = low_y + i32::try_from(rect.height).unwrap_or(0);
            let quad = match line.axis {
                WallAxis::X => SideWallQuad {
                    x: column.local_x as f32,
                    z: column.local_z as f32,
                    width: (rect.width * wall.cell_size) as f32,
                    depth: wall.cell_size as f32,
                    low_y: low_y as f32,
                    high_y: high_y as f32,
                    face: line.face,
                },
                WallAxis::Z => SideWallQuad {
                    x: column.local_x as f32,
                    z: column.local_z as f32,
                    width: wall.cell_size as f32,
                    depth: (rect.width * wall.cell_size) as f32,
                    low_y: low_y as f32,
                    high_y: high_y as f32,
                    face: line.face,
                },
            };
            buffers.add_side_wall(
                quad,
                line.context
                    .face_style(cell.block, FaceTexture::Side, line.textured),
            );
        }
    }
}

fn wall_mask_cell_coords(axis: WallAxis, axis_cell: usize, fixed_cell: usize) -> (usize, usize) {
    match axis {
        WallAxis::X => (axis_cell, fixed_cell),
        WallAxis::Z => (fixed_cell, axis_cell),
    }
}

fn surface_wall_for_column(
    column: SurfaceColumn,
    face: Face,
    region_origin: IVec2,
    perlin: &Perlin,
) -> Option<SurfaceWall> {
    let offset = match face {
        Face::North => IVec2::new(0, -1),
        Face::South => IVec2::new(0, 1),
        Face::West => IVec2::new(-1, 0),
        Face::East => IVec2::new(1, 0),
        Face::Top => return None,
    };
    let world_x = region_origin.x + column.local_x as i32;
    let world_z = region_origin.y + column.local_z as i32;
    let neighbor_height = terrain_height_with_noise(
        world_x + offset.x * column.cell_size as i32,
        world_z + offset.y * column.cell_size as i32,
        perlin,
    ) as i32;
    if neighbor_height >= column.height {
        return None;
    }

    Some(SurfaceWall {
        low_y: neighbor_height,
        high_y: column.height,
        cell_size: column.cell_size,
    })
}

fn surface_wall_block_at_y(
    column: SurfaceColumn,
    y: i32,
    context: &SurfaceBuildContext,
) -> Option<BlockId> {
    surface_wall_block_for_layer(
        column.top_block,
        column.height,
        y,
        column.cell_size,
        context.world.palette,
        context.sand,
    )
}

fn surface_wall_block_for_layer(
    top_block: BlockId,
    surface_height: i32,
    y: i32,
    cell_size: usize,
    palette: TerrainBlockPalette,
    sand: BlockId,
) -> Option<BlockId> {
    if top_block == sand {
        return Some(sand);
    }

    let surface_height = usize::try_from(surface_height).ok()?;
    let y = usize::try_from(y).ok()?;
    if cell_size > 1 {
        return if y + 1 >= surface_height {
            Some(palette.grass)
        } else {
            Some(palette.dirt)
        };
    }

    let block = terrain_block_at_height(y, surface_height, palette);
    (block != palette.air).then_some(block)
}

fn cell_intersects_lua_chunk(
    local_x: usize,
    local_z: usize,
    cell_size: usize,
    offset: UVec2,
) -> bool {
    let offset_x = offset.x as usize;
    let offset_z = offset.y as usize;
    let cell_max_x = local_x + cell_size;
    let cell_max_z = local_z + cell_size;
    let chunk_max_x = offset_x + CHUNK_SIZE;
    let chunk_max_z = offset_z + CHUNK_SIZE;

    local_x < chunk_max_x && cell_max_x > offset_x && local_z < chunk_max_z && cell_max_z > offset_z
}

#[derive(Clone)]
struct SurfaceBuildContext {
    world: WorldGenerationContext,
    textures: SurfaceTexturePalette,
    colors: HashMap<BlockId, [f32; 4]>,
    air: BlockId,
    grass: BlockId,
    sand: BlockId,
}

impl SurfaceBuildContext {
    fn from_registry(registry: &BlockRegistry) -> Self {
        let world = WorldGenerationContext::from_registry(registry);

        Self {
            textures: SurfaceTexturePalette::from_registry(registry),
            colors: block_colors_from_registry(registry),
            air: world.palette.air,
            grass: world.palette.grass,
            sand: registry.get_id("sand").unwrap_or(world.palette.dirt),
            world,
        }
    }

    fn face_style(&self, block: BlockId, face: FaceTexture, textured: bool) -> FaceStyle {
        FaceStyle {
            tile: self.textures.face_tile(block, face),
            color: if textured {
                [1.0, 1.0, 1.0, 1.0]
            } else {
                self.colors
                    .get(&block)
                    .copied()
                    .unwrap_or([0.45, 0.45, 0.45, 1.0])
            },
        }
    }
}

fn block_colors_from_registry(registry: &BlockRegistry) -> HashMap<BlockId, [f32; 4]> {
    let mut colors = HashMap::default();
    for id in 0..=u8::MAX {
        let block_id = BlockId::from(id);
        if let Some(block) = registry.get_block(block_id) {
            colors.insert(
                block_id,
                [block.color.0, block.color.1, block.color.2, block.color.3],
            );
        }
    }
    colors
}

fn lua_chunk_offset_in_region(region_pos: IVec2) -> Option<UVec2> {
    let region_chunk_origin = region_pos * REGION_SIZE_CHUNKS;
    let local_chunk = IVec2::ZERO - region_chunk_origin;

    (local_chunk.x >= 0
        && local_chunk.x < REGION_SIZE_CHUNKS
        && local_chunk.y >= 0
        && local_chunk.y < REGION_SIZE_CHUNKS)
        .then(|| UVec2::new(local_chunk.x as u32, local_chunk.y as u32) * CHUNK_SIZE as u32)
}

fn add_lua_world_gen_chunk(
    buffers: &mut SurfaceMeshBuffers,
    offset: UVec2,
    textured: bool,
    context: &SurfaceBuildContext,
) {
    let chunk = generate_chunk_with_context(ChunkPos::new(0, 0), &context.world);
    let base_x = offset.x as usize;
    let base_z = offset.y as usize;

    for z in 0..CHUNK_SIZE {
        for x in 0..CHUNK_SIZE {
            for y in 0..CHUNK_SIZE {
                let block = chunk.get_block(x, y, z);
                if block == context.air {
                    continue;
                }

                let world_x = (base_x + x) as f32;
                let world_y = y as f32;
                let world_z = (base_z + z) as f32;

                if y + 1 >= CHUNK_SIZE || chunk.get_block(x, y + 1, z) == context.air {
                    buffers.add_block_face(
                        world_x,
                        world_y,
                        world_z,
                        Face::Top,
                        context.face_style(block, FaceTexture::Top, textured),
                    );
                }

                let side_faces = [
                    (x.checked_sub(1), Some(z), Face::West),
                    ((x + 1 < CHUNK_SIZE).then_some(x + 1), Some(z), Face::East),
                    (Some(x), z.checked_sub(1), Face::North),
                    (Some(x), (z + 1 < CHUNK_SIZE).then_some(z + 1), Face::South),
                ];

                for (neighbor_x, neighbor_z, face) in side_faces {
                    let exposed = neighbor_x
                        .zip(neighbor_z)
                        .is_none_or(|(nx, nz)| chunk.get_block(nx, y, nz) == context.air);
                    if exposed {
                        buffers.add_block_face(
                            world_x,
                            world_y,
                            world_z,
                            face,
                            context.face_style(block, FaceTexture::Side, textured),
                        );
                    }
                }
            }
        }
    }
}

#[derive(Clone)]
struct SurfaceTexturePalette {
    blocks: HashMap<BlockId, [u32; 3]>,
    fallback: [u32; 3],
}

impl SurfaceTexturePalette {
    fn from_registry(registry: &BlockRegistry) -> Self {
        let fallback = [3, 3, 3];
        let mut blocks = HashMap::default();

        if let Ok(mappings) = registry.texture_mappings.read() {
            for (&block_id, &textures) in mappings.iter() {
                blocks.insert(block_id, textures);
            }
        }

        Self { blocks, fallback }
    }

    fn face_tile(&self, block: BlockId, face: FaceTexture) -> u32 {
        let textures = self.blocks.get(&block).unwrap_or(&self.fallback);
        textures[face.index()]
    }
}

#[derive(Clone, Copy)]
struct FaceStyle {
    tile: u32,
    color: [f32; 4],
}

#[derive(Clone, Copy)]
enum Face {
    Top,
    North,
    South,
    West,
    East,
}

enum FaceTexture {
    Top,
    Side,
}

impl FaceTexture {
    fn index(self) -> usize {
        match self {
            Self::Top => 0,
            Self::Side => 1,
        }
    }
}

struct SurfaceMeshBuffers {
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    colors: Vec<[f32; 4]>,
    repeat_uvs: Vec<[f32; 2]>,
    tile_ids: Vec<u32>,
    indices: Vec<u32>,
}

struct SurfaceMeshStats {
    vertices: usize,
    indices: usize,
}

impl SurfaceMeshBuffers {
    fn with_block_capacity(blocks: usize) -> Self {
        Self {
            positions: Vec::with_capacity(blocks * 8),
            normals: Vec::with_capacity(blocks * 8),
            colors: Vec::with_capacity(blocks * 8),
            repeat_uvs: Vec::with_capacity(blocks * 8),
            tile_ids: Vec::with_capacity(blocks * 8),
            indices: Vec::with_capacity(blocks * 18),
        }
    }

    fn into_mesh(self) -> Mesh {
        Mesh::new(
            bevy::render::render_resource::PrimitiveTopology::TriangleList,
            bevy::asset::RenderAssetUsages::RENDER_WORLD,
        )
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, self.positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, self.normals)
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, self.colors)
        .with_inserted_attribute(ATTRIBUTE_VOXEL_REPEAT_UV, self.repeat_uvs)
        .with_inserted_attribute(ATTRIBUTE_VOXEL_TILE, self.tile_ids)
        .with_inserted_indices(bevy::mesh::Indices::U32(self.indices))
    }

    fn stats(&self) -> SurfaceMeshStats {
        SurfaceMeshStats {
            vertices: self.positions.len(),
            indices: self.indices.len(),
        }
    }

    fn add_block_face(&mut self, x: f32, y: f32, z: f32, face: Face, style: FaceStyle) {
        let (corners, normal) = match face {
            Face::Top => (
                [
                    [x, y + 1.0, z],
                    [x, y + 1.0, z + 1.0],
                    [x + 1.0, y + 1.0, z + 1.0],
                    [x + 1.0, y + 1.0, z],
                ],
                [0.0, 1.0, 0.0],
            ),
            Face::North => (
                [
                    [x + 1.0, y, z],
                    [x, y, z],
                    [x, y + 1.0, z],
                    [x + 1.0, y + 1.0, z],
                ],
                [0.0, 0.0, -1.0],
            ),
            Face::South => (
                [
                    [x, y, z + 1.0],
                    [x + 1.0, y, z + 1.0],
                    [x + 1.0, y + 1.0, z + 1.0],
                    [x, y + 1.0, z + 1.0],
                ],
                [0.0, 0.0, 1.0],
            ),
            Face::West => (
                [
                    [x, y, z],
                    [x, y, z + 1.0],
                    [x, y + 1.0, z + 1.0],
                    [x, y + 1.0, z],
                ],
                [-1.0, 0.0, 0.0],
            ),
            Face::East => (
                [
                    [x + 1.0, y, z + 1.0],
                    [x + 1.0, y, z],
                    [x + 1.0, y + 1.0, z],
                    [x + 1.0, y + 1.0, z + 1.0],
                ],
                [1.0, 0.0, 0.0],
            ),
        };

        let repeat_uvs = match face {
            Face::Top => quad_repeat_uvs(1.0, 1.0),
            Face::North | Face::South | Face::West | Face::East => side_repeat_uvs(face, 1.0, 1.0),
        };
        self.add_quad(corners, normal, repeat_uvs, style);
    }

    fn add_top_tile(&mut self, x: f32, y: f32, z: f32, width: f32, depth: f32, style: FaceStyle) {
        self.add_quad(
            [
                [x, y, z],
                [x, y, z + depth],
                [x + width, y, z + depth],
                [x + width, y, z],
            ],
            [0.0, 1.0, 0.0],
            quad_repeat_uvs(width, depth),
            style,
        );
    }

    fn add_side_wall(&mut self, wall: SideWallQuad, style: FaceStyle) {
        let SideWallQuad {
            x,
            z,
            width,
            depth,
            low_y,
            high_y,
            face,
        } = wall;
        let height = (high_y - low_y).max(1.0);
        let (corners, normal) = match face {
            Face::North => (
                [
                    [x + width, low_y, z],
                    [x, low_y, z],
                    [x, high_y, z],
                    [x + width, high_y, z],
                ],
                [0.0, 0.0, -1.0],
            ),
            Face::South => (
                [
                    [x, low_y, z + depth],
                    [x + width, low_y, z + depth],
                    [x + width, high_y, z + depth],
                    [x, high_y, z + depth],
                ],
                [0.0, 0.0, 1.0],
            ),
            Face::West => (
                [
                    [x, low_y, z],
                    [x, low_y, z + depth],
                    [x, high_y, z + depth],
                    [x, high_y, z],
                ],
                [-1.0, 0.0, 0.0],
            ),
            Face::East => (
                [
                    [x + width, low_y, z + depth],
                    [x + width, low_y, z],
                    [x + width, high_y, z],
                    [x + width, high_y, z + depth],
                ],
                [1.0, 0.0, 0.0],
            ),
            Face::Top => return,
        };

        let repeat_width = if matches!(face, Face::North | Face::South) {
            width
        } else {
            depth
        };
        self.add_quad(
            corners,
            normal,
            side_repeat_uvs(face, repeat_width, height),
            style,
        );
    }

    fn add_quad(
        &mut self,
        corners: [[f32; 3]; 4],
        normal: [f32; 3],
        repeat_uvs: [[f32; 2]; 4],
        style: FaceStyle,
    ) {
        let base = self.positions.len() as u32;
        self.positions.extend(corners);
        self.normals.extend([normal; 4]);
        self.colors.extend([style.color; 4]);
        self.repeat_uvs.extend(repeat_uvs);
        self.tile_ids.extend([style.tile; 4]);
        self.indices
            .extend([base, base + 1, base + 2, base, base + 2, base + 3]);
    }
}

fn quad_repeat_uvs(width: f32, height: f32) -> [[f32; 2]; 4] {
    [[0.0, 0.0], [0.0, height], [width, height], [width, 0.0]]
}

fn side_repeat_uvs(face: Face, width: f32, height: f32) -> [[f32; 2]; 4] {
    match face {
        Face::North | Face::East => [[width, height], [0.0, height], [0.0, 0.0], [width, 0.0]],
        Face::South | Face::West => [[0.0, height], [width, height], [width, 0.0], [0.0, 0.0]],
        Face::Top => quad_repeat_uvs(width, height),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn binary_mask_merges_solid_rectangle() {
        let mut mask = BinaryMask2d::new(8, 4);
        for y in 1..3 {
            for x in 2..6 {
                mask.set(x, y);
            }
        }

        let rects = mask.drain_greedy_rects();

        assert_eq!(rects.len(), 1);
        let rect = rects[0];
        assert_eq!((rect.x, rect.y, rect.width, rect.height), (2, 1, 4, 2));
    }

    #[test]
    fn binary_mask_keeps_separate_regions_apart() {
        let mut mask = BinaryMask2d::new(70, 3);
        mask.set(0, 0);
        mask.set(1, 0);
        mask.set(65, 2);
        mask.set(66, 2);

        let rects = mask.drain_greedy_rects();

        assert_eq!(rects.len(), 2);
        assert!(
            rects
                .iter()
                .any(|rect| (rect.x, rect.y, rect.width, rect.height) == (0, 0, 2, 1))
        );
        assert!(
            rects
                .iter()
                .any(|rect| (rect.x, rect.y, rect.width, rect.height) == (65, 2, 2, 1))
        );
    }

    #[test]
    fn side_repeat_uvs_keep_side_texture_cap_on_top_edge() {
        assert_eq!(
            side_repeat_uvs(Face::North, 3.0, 5.0),
            [[3.0, 5.0], [0.0, 5.0], [0.0, 0.0], [3.0, 0.0]]
        );
        assert_eq!(
            side_repeat_uvs(Face::South, 3.0, 5.0),
            [[0.0, 5.0], [3.0, 5.0], [3.0, 0.0], [0.0, 0.0]]
        );
    }

    #[test]
    fn surface_wall_block_at_y_matches_voxel_terrain_layers() {
        let palette = TerrainBlockPalette {
            air: 0,
            dirt: 2,
            grass: 1,
            stone: 3,
        };
        let sand = 4;

        assert_eq!(
            surface_wall_block_for_layer(palette.grass, 12, 11, 1, palette, sand),
            Some(palette.grass)
        );
        assert_eq!(
            surface_wall_block_for_layer(palette.grass, 12, 10, 1, palette, sand),
            Some(palette.dirt)
        );
        assert_eq!(
            surface_wall_block_for_layer(palette.grass, 12, 7, 1, palette, sand),
            Some(palette.stone)
        );
        assert_eq!(
            surface_wall_block_for_layer(sand, 12, 7, 1, palette, sand),
            Some(sand)
        );
    }

    #[test]
    fn surface_wall_block_at_y_keeps_lod_slopes_earth_toned() {
        let palette = TerrainBlockPalette {
            air: 0,
            dirt: 2,
            grass: 1,
            stone: 3,
        };
        let sand = 4;

        assert_eq!(
            surface_wall_block_for_layer(palette.grass, 20, 19, 4, palette, sand),
            Some(palette.grass)
        );
        assert_eq!(
            surface_wall_block_for_layer(palette.grass, 20, 8, 4, palette, sand),
            Some(palette.dirt)
        );
    }
}
