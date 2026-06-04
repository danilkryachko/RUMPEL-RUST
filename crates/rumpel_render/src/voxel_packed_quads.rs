use noise::Perlin;
use rumpel_blocks::{AIR_BLOCK_ID, BlockId};
use rumpel_prelude::ChunkPos;
use rumpel_world::chunk::{CHUNK_HEIGHT, CHUNK_SIZE, ChunkData, WorldEditStore};
use rumpel_world::world_gen::{
    TerrainBlockPalette, WorldGenerationContext, terrain_perlin,
    terrain_surface_cell_height_from_world_cached, terrain_surface_cell_height_with_edits,
    terrain_surface_cell_sample_from_world_cached, terrain_surface_cell_sample_with_edits,
    terrain_surface_wall_block_at_y,
};

use crate::packed_quad_gpu_generation::{
    PackedGpuSurfaceColumn, packed_gpu_generation_columns_per_chunk,
};

/// Represents the six cardinal directions of a voxel face.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum PackedVoxelFace {
    /// Positive X direction (+X)
    PlusX = 0,
    /// Negative X direction (-X)
    MinusX = 1,
    /// Positive Y direction (+Y)
    PlusY = 2,
    /// Negative Y direction (-Y)
    MinusY = 3,
    /// Positive Z direction (+Z)
    PlusZ = 4,
    /// Negative Z direction (-Z)
    MinusZ = 5,
}

/// A compact, GPU-friendly representation of a voxel quad.
/// This structure is memory-aligned and designed as a stable 16-byte Pod ABI data contract
/// for future vertex pulling and MultiDrawIndirect (MDI) shaders.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct PackedVoxelQuad {
    /// The [x, y, z] chunk-local coordinates of the quad's origin block.
    pub origin: [u16; 3],
    /// The [width, height] of the merged quad in its 2D slice plane.
    pub size: [u16; 2],
    /// The BlockId of the block.
    pub block_id: u16,
    /// Packed metadata:
    /// - bits 0..3: face (3 bits)
    /// - bits 3..8: lod (5 bits)
    /// - bits 8..32: flags/material (24 bits)
    pub meta: u32,
}

// Manually implement Pod and Zeroable from bytemuck for safety and zero features dependency overhead.
unsafe impl bytemuck::Zeroable for PackedVoxelQuad {}
unsafe impl bytemuck::Pod for PackedVoxelQuad {}

impl PackedVoxelQuad {
    /// Creates a new `PackedVoxelQuad` with fully packed metadata.
    pub fn new(
        origin: [u16; 3],
        size: [u16; 2],
        block_id: u16,
        face: u8,
        lod: u8,
        flags: u32,
    ) -> Self {
        let face_part = (face as u32) & 0x7;
        let lod_part = ((lod as u32) & 0x1F) << 3;
        let flags_part = (flags & 0xFF_FFFF) << 8;
        let meta = face_part | lod_part | flags_part;

        Self {
            origin,
            size,
            block_id,
            meta,
        }
    }

    /// Extract the face direction of the quad.
    #[inline]
    pub fn face(&self) -> u8 {
        (self.meta & 0x7) as u8
    }

    /// Extract the Level-of-Detail (LOD) level.
    #[inline]
    pub fn lod(&self) -> u8 {
        ((self.meta >> 3) & 0x1F) as u8
    }

    /// Extract custom flags or material properties.
    #[inline]
    pub fn flags(&self) -> u32 {
        (self.meta >> 8) & 0xFF_FFFF
    }
}

/// Stores optional references to adjacent chunks for neighbor-aware boundary culling.
#[derive(Default, Clone, Copy)]
pub struct ChunkNeighbors<'a> {
    pub plus_x: Option<&'a ChunkData>,
    pub minus_x: Option<&'a ChunkData>,
    pub plus_y: Option<&'a ChunkData>,
    pub minus_y: Option<&'a ChunkData>,
    pub plus_z: Option<&'a ChunkData>,
    pub minus_z: Option<&'a ChunkData>,
}

/// Helper function to check if a block face in a given direction is visible.
/// A face is visible if the current block is solid (not air) and the adjacent
/// block in the face direction is air. Neighbors outside the chunk boundaries
/// are checked using the provided `ChunkNeighbors` if available.
#[inline]
fn is_face_visible(
    chunk: &ChunkData,
    neighbors: &ChunkNeighbors,
    x: i32,
    y: i32,
    z: i32,
    face: PackedVoxelFace,
) -> bool {
    let block = chunk.get_block(x as usize, y as usize, z as usize);
    if block == AIR_BLOCK_ID {
        return false;
    }

    let (nx, ny, nz) = match face {
        PackedVoxelFace::PlusX => (x + 1, y, z),
        PackedVoxelFace::MinusX => (x - 1, y, z),
        PackedVoxelFace::PlusY => (x, y + 1, z),
        PackedVoxelFace::MinusY => (x, y - 1, z),
        PackedVoxelFace::PlusZ => (x, y, z + 1),
        PackedVoxelFace::MinusZ => (x, y, z - 1),
    };

    if nx >= 0
        && nx < CHUNK_SIZE as i32
        && ny >= 0
        && ny < CHUNK_HEIGHT as i32
        && nz >= 0
        && nz < CHUNK_SIZE as i32
    {
        chunk.get_block(nx as usize, ny as usize, nz as usize) == AIR_BLOCK_ID
    } else {
        // Boundary face! Resolve using ChunkNeighbors
        let neighbor_chunk = match face {
            PackedVoxelFace::PlusX => neighbors.plus_x,
            PackedVoxelFace::MinusX => neighbors.minus_x,
            PackedVoxelFace::PlusY => neighbors.plus_y,
            PackedVoxelFace::MinusY => neighbors.minus_y,
            PackedVoxelFace::PlusZ => neighbors.plus_z,
            PackedVoxelFace::MinusZ => neighbors.minus_z,
        };

        if let Some(nc) = neighbor_chunk {
            let rx = if nx < 0 {
                CHUNK_SIZE - 1
            } else if nx >= CHUNK_SIZE as i32 {
                0
            } else {
                nx as usize
            };

            let ry = if ny < 0 {
                CHUNK_HEIGHT - 1
            } else if ny >= CHUNK_HEIGHT as i32 {
                0
            } else {
                ny as usize
            };

            let rz = if nz < 0 {
                CHUNK_SIZE - 1
            } else if nz >= CHUNK_SIZE as i32 {
                0
            } else {
                nz as usize
            };

            nc.get_block(rx, ry, rz) == AIR_BLOCK_ID
        } else {
            // "если соседнего чанка нет, boundary face считается visible;"
            true
        }
    }
}

/// Maps 2D slice sweep coordinate to 3D block coordinate based on face direction.
#[inline]
fn to_3d_coords(p: usize, u: usize, v: usize, face: PackedVoxelFace) -> (i32, i32, i32) {
    match face {
        PackedVoxelFace::PlusX | PackedVoxelFace::MinusX => (p as i32, u as i32, v as i32),
        PackedVoxelFace::PlusY | PackedVoxelFace::MinusY => (u as i32, p as i32, v as i32),
        PackedVoxelFace::PlusZ | PackedVoxelFace::MinusZ => (u as i32, v as i32, p as i32),
    }
}

/// Builds a deterministic list of packed voxel quads for the given chunk,
/// incorporating neighbor-aware boundary culling and 2D Greedy Merging on each slice plane.
pub fn build_packed_quads_for_chunk_with_neighbors(
    chunk: &ChunkData,
    neighbors: ChunkNeighbors,
) -> Vec<PackedVoxelQuad> {
    let mut quads = Vec::new();

    let faces = [
        PackedVoxelFace::PlusX,
        PackedVoxelFace::MinusX,
        PackedVoxelFace::PlusY,
        PackedVoxelFace::MinusY,
        PackedVoxelFace::PlusZ,
        PackedVoxelFace::MinusZ,
    ];

    for &face in &faces {
        let p_max = CHUNK_SIZE;
        let u_max = CHUNK_SIZE;
        let v_max = CHUNK_SIZE;

        for p in 0..p_max {
            let mut visited = [false; CHUNK_SIZE * CHUNK_SIZE];

            for v in 0..v_max {
                for u in 0..u_max {
                    let idx = v * CHUNK_SIZE + u;
                    if visited[idx] {
                        continue;
                    }

                    let (x, y, z) = to_3d_coords(p, u, v, face);

                    if is_face_visible(chunk, &neighbors, x, y, z, face) {
                        let block_id = chunk.get_block(x as usize, y as usize, z as usize);

                        // 1. Find maximum width we can merge along the U axis
                        let mut width = 1;
                        while u + width < u_max {
                            let next_u = u + width;
                            let next_idx = v * CHUNK_SIZE + next_u;
                            if visited[next_idx] {
                                break;
                            }

                            let (nx, ny, nz) = to_3d_coords(p, next_u, v, face);
                            if is_face_visible(chunk, &neighbors, nx, ny, nz, face)
                                && chunk.get_block(nx as usize, ny as usize, nz as usize)
                                    == block_id
                            {
                                width += 1;
                            } else {
                                break;
                            }
                        }

                        // 2. Find maximum height we can merge along the V axis
                        let mut height = 1;
                        'outer: while v + height < v_max {
                            for du in 0..width {
                                let curr_u = u + du;
                                let curr_v = v + height;
                                let curr_idx = curr_v * CHUNK_SIZE + curr_u;
                                if visited[curr_idx] {
                                    break 'outer;
                                }

                                let (nx, ny, nz) = to_3d_coords(p, curr_u, curr_v, face);
                                if !is_face_visible(chunk, &neighbors, nx, ny, nz, face)
                                    || chunk.get_block(nx as usize, ny as usize, nz as usize)
                                        != block_id
                                {
                                    break 'outer;
                                }
                            }
                            height += 1;
                        }

                        // 3. Mark all merged blocks in this 2D quad as visited
                        for dv in 0..height {
                            for du in 0..width {
                                let curr_idx = (v + dv) * CHUNK_SIZE + (u + du);
                                visited[curr_idx] = true;
                            }
                        }

                        // 4. Record the merged 2D quad
                        quads.push(PackedVoxelQuad::new(
                            [x as u16, y as u16, z as u16],
                            [width as u16, height as u16],
                            block_id,
                            face as u8,
                            0,
                            0,
                        ));
                    } else {
                        visited[idx] = true;
                    }
                }
            }
        }
    }

    quads
}

#[derive(Clone, Copy)]
struct SurfacePackedColumn {
    x: usize,
    z: usize,
    width: usize,
    depth: usize,
    height: usize,
    top_block: BlockId,
}

#[derive(Clone, Copy)]
struct SurfaceColumnSource<'a> {
    chunk_pos: ChunkPos,
    world_origin_x: i32,
    world_origin_z: i32,
    context: &'a WorldGenerationContext,
    requested_cell_size: usize,
    sand_block: BlockId,
    edit_store: &'a WorldEditStore,
    perlin: &'a Perlin,
    has_edits: bool,
}

impl<'a> SurfaceColumnSource<'a> {
    fn new(
        chunk_pos: ChunkPos,
        context: &'a WorldGenerationContext,
        requested_cell_size: usize,
        sand_block: BlockId,
        edit_store: &'a WorldEditStore,
        perlin: &'a Perlin,
        has_edits: bool,
    ) -> Self {
        Self {
            chunk_pos,
            world_origin_x: chunk_pos.x * CHUNK_SIZE as i32,
            world_origin_z: chunk_pos.z * CHUNK_SIZE as i32,
            context,
            requested_cell_size,
            sand_block,
            edit_store,
            perlin,
            has_edits,
        }
    }
}

const PACKED_QUAD_FLAG_SIDE_BLENDS_TOP_TILE: u32 = 1;

/// Builds packed quads for the visible heightmap shell of one terrain chunk column.
///
/// Unlike the full voxel-chunk mesher, this builder is not limited to the local
/// 0..32 Y slice. Quad origins keep the real world terrain height in local Y,
/// while X/Z remain chunk-local so the packed region translation can still place
/// the batch in world space.
#[must_use]
pub fn build_surface_packed_quads_for_chunk(
    chunk_pos: ChunkPos,
    context: &WorldGenerationContext,
    requested_cell_size: usize,
    sand_block: BlockId,
) -> Vec<PackedVoxelQuad> {
    let empty_edits = WorldEditStore::default();
    let perlin = terrain_perlin();
    let has_edits = !empty_edits.is_empty();
    let source = SurfaceColumnSource::new(
        chunk_pos,
        context,
        requested_cell_size,
        sand_block,
        &empty_edits,
        &perlin,
        has_edits,
    );
    let columns = build_surface_packed_columns_for_chunk_with_perlin(source);
    let cell_size = requested_cell_size.clamp(1, CHUNK_SIZE);

    let mut quads = Vec::with_capacity(columns.len() * 3);
    let lod = lod_for_surface_cell_size(cell_size);

    for column in &columns {
        push_surface_packed_top(&mut quads, *column, lod);
        for face in [
            PackedVoxelFace::PlusX,
            PackedVoxelFace::MinusX,
            PackedVoxelFace::PlusZ,
            PackedVoxelFace::MinusZ,
        ] {
            push_surface_packed_wall(
                &mut quads,
                *column,
                face,
                chunk_pos,
                &perlin,
                context,
                &empty_edits,
                has_edits,
                lod,
            );
        }
    }

    compact_packed_quads(&mut quads);
    quads
}

#[must_use]
pub fn build_surface_gpu_generation_columns_for_chunk(
    chunk_pos: ChunkPos,
    context: &WorldGenerationContext,
    requested_cell_size: usize,
    sand_block: BlockId,
    edit_store: &WorldEditStore,
) -> Vec<PackedGpuSurfaceColumn> {
    let mut gpu_columns =
        Vec::with_capacity(packed_gpu_generation_columns_per_chunk(requested_cell_size));
    append_surface_gpu_generation_columns_for_chunk(
        &mut gpu_columns,
        chunk_pos,
        context,
        requested_cell_size,
        sand_block,
        edit_store,
    );
    gpu_columns
}

pub fn append_surface_gpu_generation_columns_for_chunk(
    gpu_columns: &mut Vec<PackedGpuSurfaceColumn>,
    chunk_pos: ChunkPos,
    context: &WorldGenerationContext,
    requested_cell_size: usize,
    sand_block: BlockId,
    edit_store: &WorldEditStore,
) {
    append_surface_gpu_generation_columns_for_chunk_with_local_offset(
        gpu_columns,
        chunk_pos,
        context,
        requested_cell_size,
        sand_block,
        edit_store,
        [0, 0],
    );
}

pub fn append_surface_gpu_generation_columns_for_chunk_with_local_offset(
    gpu_columns: &mut Vec<PackedGpuSurfaceColumn>,
    chunk_pos: ChunkPos,
    context: &WorldGenerationContext,
    requested_cell_size: usize,
    sand_block: BlockId,
    edit_store: &WorldEditStore,
    local_offset: [usize; 2],
) {
    let perlin = terrain_perlin();
    let has_edits = !edit_store.is_empty();
    let source = SurfaceColumnSource::new(
        chunk_pos,
        context,
        requested_cell_size,
        sand_block,
        edit_store,
        &perlin,
        has_edits,
    );
    for_each_surface_packed_column_for_chunk(source, |column| {
        let neighbor_heights = surface_neighbor_heights(column, source);
        gpu_columns.push(PackedGpuSurfaceColumn::from_parts(
            [
                column.x.saturating_add(local_offset[0]),
                column.z.saturating_add(local_offset[1]),
                column.width,
                column.depth,
            ],
            [
                column.height,
                neighbor_heights[0],
                neighbor_heights[1],
                neighbor_heights[2],
                neighbor_heights[3],
            ],
            column.top_block,
        ));
    });
}

fn build_surface_packed_columns_for_chunk_with_perlin(
    source: SurfaceColumnSource<'_>,
) -> Vec<SurfacePackedColumn> {
    let mut columns = Vec::with_capacity(packed_gpu_generation_columns_per_chunk(
        source.requested_cell_size,
    ));
    for_each_surface_packed_column_for_chunk(source, |column| columns.push(column));
    columns
}

fn for_each_surface_packed_column_for_chunk(
    source: SurfaceColumnSource<'_>,
    mut visit: impl FnMut(SurfacePackedColumn),
) {
    let cell_size = source.requested_cell_size.clamp(1, CHUNK_SIZE);
    let cells_x = CHUNK_SIZE.div_ceil(cell_size);
    let cells_z = CHUNK_SIZE.div_ceil(cell_size);
    let chunk_edited = source.has_edits && source.edit_store.chunk_revision(source.chunk_pos) > 0;

    for cell_z in 0..cells_z {
        let z = cell_z * cell_size;
        for cell_x in 0..cells_x {
            let x = cell_x * cell_size;
            let width = cell_size.min(CHUNK_SIZE - x);
            let depth = cell_size.min(CHUNK_SIZE - z);
            let world_x = source.world_origin_x + x as i32;
            let world_z = source.world_origin_z + z as i32;
            let sample = if chunk_edited {
                terrain_surface_cell_sample_with_edits(
                    world_x,
                    world_z,
                    width,
                    depth,
                    source.context.palette,
                    source.sand_block,
                    source.edit_store,
                    source.perlin,
                )
            } else {
                terrain_surface_cell_sample_from_world_cached(
                    world_x,
                    world_z,
                    width,
                    depth,
                    source.context,
                    source.edit_store,
                )
            };

            visit(SurfacePackedColumn {
                x,
                z,
                width,
                depth,
                height: sample.height,
                top_block: sample.top_block,
            });
        }
    }
}

fn lod_for_surface_cell_size(cell_size: usize) -> u8 {
    match cell_size {
        0 | 1 => 0,
        2 => 1,
        3 | 4 => 2,
        _ => 3,
    }
}

fn push_surface_packed_top(quads: &mut Vec<PackedVoxelQuad>, column: SurfacePackedColumn, lod: u8) {
    if column.height == 0 || column.top_block == AIR_BLOCK_ID {
        return;
    }

    quads.push(PackedVoxelQuad::new(
        [
            column.x as u16,
            column.height.saturating_sub(1) as u16,
            column.z as u16,
        ],
        [column.width as u16, column.depth as u16],
        column.top_block,
        PackedVoxelFace::PlusY as u8,
        lod,
        0,
    ));
}

#[allow(clippy::too_many_arguments)]
fn push_surface_packed_wall(
    quads: &mut Vec<PackedVoxelQuad>,
    column: SurfacePackedColumn,
    face: PackedVoxelFace,
    chunk_pos: ChunkPos,
    perlin: &Perlin,
    context: &WorldGenerationContext,
    edit_store: &WorldEditStore,
    has_edits: bool,
    lod: u8,
) {
    let neighbor_height = surface_neighbor_height(
        column, face, chunk_pos, context, edit_store, perlin, has_edits,
    );
    if neighbor_height >= column.height {
        return;
    }

    let mut y = neighbor_height;
    while y < column.height {
        let block = surface_wall_block(column, y, context.palette);
        let segment_start = y;
        y += 1;
        while y < column.height && surface_wall_block(column, y, context.palette) == block {
            y += 1;
        }
        if block == AIR_BLOCK_ID {
            continue;
        }
        push_surface_wall_segment(quads, column, face, segment_start, y, block, lod);
    }
}

fn surface_neighbor_height(
    column: SurfacePackedColumn,
    face: PackedVoxelFace,
    chunk_pos: ChunkPos,
    context: &WorldGenerationContext,
    edit_store: &WorldEditStore,
    perlin: &Perlin,
    has_edits: bool,
) -> usize {
    let world_x = chunk_pos.x * CHUNK_SIZE as i32 + column.x as i32;
    let world_z = chunk_pos.z * CHUNK_SIZE as i32 + column.z as i32;
    let (sample_x, sample_z) = match face {
        PackedVoxelFace::PlusX => (world_x + column.width as i32, world_z),
        PackedVoxelFace::MinusX => (world_x - column.width as i32, world_z),
        PackedVoxelFace::PlusZ => (world_x, world_z + column.depth as i32),
        PackedVoxelFace::MinusZ => (world_x, world_z - column.depth as i32),
        PackedVoxelFace::PlusY | PackedVoxelFace::MinusY => return column.height,
    };

    surface_neighbor_sample_height(
        sample_x, sample_z, column, context, edit_store, perlin, has_edits,
    )
}

fn surface_neighbor_heights(
    column: SurfacePackedColumn,
    source: SurfaceColumnSource<'_>,
) -> [usize; 4] {
    let world_x = source.world_origin_x + column.x as i32;
    let world_z = source.world_origin_z + column.z as i32;

    if !source.has_edits {
        return [
            terrain_surface_cell_height_from_world_cached(
                world_x + column.width as i32,
                world_z,
                column.width,
                column.depth,
                source.context,
                source.edit_store,
            ),
            terrain_surface_cell_height_from_world_cached(
                world_x - column.width as i32,
                world_z,
                column.width,
                column.depth,
                source.context,
                source.edit_store,
            ),
            terrain_surface_cell_height_from_world_cached(
                world_x,
                world_z + column.depth as i32,
                column.width,
                column.depth,
                source.context,
                source.edit_store,
            ),
            terrain_surface_cell_height_from_world_cached(
                world_x,
                world_z - column.depth as i32,
                column.width,
                column.depth,
                source.context,
                source.edit_store,
            ),
        ];
    }

    [
        surface_neighbor_sample_height(
            world_x + column.width as i32,
            world_z,
            column,
            source.context,
            source.edit_store,
            source.perlin,
            source.has_edits,
        ),
        surface_neighbor_sample_height(
            world_x - column.width as i32,
            world_z,
            column,
            source.context,
            source.edit_store,
            source.perlin,
            source.has_edits,
        ),
        surface_neighbor_sample_height(
            world_x,
            world_z + column.depth as i32,
            column,
            source.context,
            source.edit_store,
            source.perlin,
            source.has_edits,
        ),
        surface_neighbor_sample_height(
            world_x,
            world_z - column.depth as i32,
            column,
            source.context,
            source.edit_store,
            source.perlin,
            source.has_edits,
        ),
    ]
}

fn surface_neighbor_sample_height(
    sample_x: i32,
    sample_z: i32,
    column: SurfacePackedColumn,
    context: &WorldGenerationContext,
    edit_store: &WorldEditStore,
    perlin: &Perlin,
    has_edits: bool,
) -> usize {
    if !has_edits {
        return terrain_surface_cell_height_from_world_cached(
            sample_x,
            sample_z,
            column.width,
            column.depth,
            context,
            edit_store,
        );
    }

    let neighbor_chunk = ChunkPos::new(
        sample_x.div_euclid(CHUNK_SIZE as i32),
        sample_z.div_euclid(CHUNK_SIZE as i32),
    );
    if edit_store.chunk_revision(neighbor_chunk) > 0 {
        terrain_surface_cell_height_with_edits(
            sample_x,
            sample_z,
            column.width,
            column.depth,
            context.palette,
            edit_store,
            perlin,
        )
    } else {
        terrain_surface_cell_height_from_world_cached(
            sample_x,
            sample_z,
            column.width,
            column.depth,
            context,
            edit_store,
        )
    }
}

fn surface_wall_block(
    column: SurfacePackedColumn,
    y: usize,
    palette: TerrainBlockPalette,
) -> BlockId {
    terrain_surface_wall_block_at_y(
        column.top_block,
        column.height,
        y,
        column.width,
        column.depth,
        palette,
    )
}

fn push_surface_wall_segment(
    quads: &mut Vec<PackedVoxelQuad>,
    column: SurfacePackedColumn,
    face: PackedVoxelFace,
    low_y: usize,
    high_y: usize,
    block: BlockId,
    lod: u8,
) {
    let height = high_y.saturating_sub(low_y);
    if height == 0 {
        return;
    }

    let (origin, size) = match face {
        PackedVoxelFace::PlusX => (
            [
                (column.x + column.width - 1) as u16,
                low_y as u16,
                column.z as u16,
            ],
            [height as u16, column.depth as u16],
        ),
        PackedVoxelFace::MinusX => (
            [column.x as u16, low_y as u16, column.z as u16],
            [height as u16, column.depth as u16],
        ),
        PackedVoxelFace::PlusZ => (
            [
                column.x as u16,
                low_y as u16,
                (column.z + column.depth - 1) as u16,
            ],
            [column.width as u16, height as u16],
        ),
        PackedVoxelFace::MinusZ => (
            [column.x as u16, low_y as u16, column.z as u16],
            [column.width as u16, height as u16],
        ),
        PackedVoxelFace::PlusY | PackedVoxelFace::MinusY => return,
    };

    let flags = surface_wall_segment_flags(column, block, lod);

    quads.push(PackedVoxelQuad::new(
        origin, size, block, face as u8, lod, flags,
    ));
}

fn surface_wall_segment_flags(column: SurfacePackedColumn, block: BlockId, lod: u8) -> u32 {
    if lod > 0 && block == column.top_block {
        PACKED_QUAD_FLAG_SIDE_BLENDS_TOP_TILE
    } else {
        0
    }
}

#[derive(Clone, Copy)]
struct LodColumn {
    x: usize,
    z: usize,
    width: usize,
    depth: usize,
    height: usize,
    plus_x_edge_min_height: usize,
    minus_x_edge_min_height: usize,
    plus_z_edge_min_height: usize,
    minus_z_edge_min_height: usize,
    block_id: u16,
}

fn lod_surface_height_at(chunk: &ChunkData, x: usize, z: usize) -> (usize, u16) {
    for y in (0..CHUNK_HEIGHT).rev() {
        let block = chunk.get_block(x, y, z);
        if block != AIR_BLOCK_ID {
            return (y + 1, block);
        }
    }

    (0, AIR_BLOCK_ID)
}

fn lod_edge_min_height(
    chunk: &ChunkData,
    x: usize,
    z: usize,
    width: usize,
    depth: usize,
    face: PackedVoxelFace,
) -> usize {
    let max_x = (x + width).min(CHUNK_SIZE);
    let max_z = (z + depth).min(CHUNK_SIZE);
    let mut min_height = usize::MAX;

    match face {
        PackedVoxelFace::PlusX => {
            let edge_x = max_x.saturating_sub(1);
            for sample_z in z..max_z {
                let (height, _) = lod_surface_height_at(chunk, edge_x, sample_z);
                min_height = min_height.min(height);
            }
        }
        PackedVoxelFace::MinusX => {
            for sample_z in z..max_z {
                let (height, _) = lod_surface_height_at(chunk, x, sample_z);
                min_height = min_height.min(height);
            }
        }
        PackedVoxelFace::PlusZ => {
            let edge_z = max_z.saturating_sub(1);
            for sample_x in x..max_x {
                let (height, _) = lod_surface_height_at(chunk, sample_x, edge_z);
                min_height = min_height.min(height);
            }
        }
        PackedVoxelFace::MinusZ => {
            for sample_x in x..max_x {
                let (height, _) = lod_surface_height_at(chunk, sample_x, z);
                min_height = min_height.min(height);
            }
        }
        _ => return 0,
    }

    if min_height == usize::MAX {
        0
    } else {
        min_height
    }
}

fn lod_column_from_chunk(
    chunk: &ChunkData,
    x: usize,
    z: usize,
    width: usize,
    depth: usize,
) -> Option<LodColumn> {
    let max_x = (x + width).min(CHUNK_SIZE);
    let max_z = (z + depth).min(CHUNK_SIZE);
    let mut height = 0;
    let mut block_id = AIR_BLOCK_ID;

    for sample_z in z..max_z {
        for sample_x in x..max_x {
            let (column_height, column_block_id) = lod_surface_height_at(chunk, sample_x, sample_z);
            if column_height > height {
                height = column_height;
                block_id = column_block_id;
            }
        }
    }

    let width = max_x.saturating_sub(x).max(1);
    let depth = max_z.saturating_sub(z).max(1);

    (height > 0).then_some(LodColumn {
        x,
        z,
        width,
        depth,
        height,
        plus_x_edge_min_height: lod_edge_min_height(
            chunk,
            x,
            z,
            width,
            depth,
            PackedVoxelFace::PlusX,
        ),
        minus_x_edge_min_height: lod_edge_min_height(
            chunk,
            x,
            z,
            width,
            depth,
            PackedVoxelFace::MinusX,
        ),
        plus_z_edge_min_height: lod_edge_min_height(
            chunk,
            x,
            z,
            width,
            depth,
            PackedVoxelFace::PlusZ,
        ),
        minus_z_edge_min_height: lod_edge_min_height(
            chunk,
            x,
            z,
            width,
            depth,
            PackedVoxelFace::MinusZ,
        ),
        block_id,
    })
}

fn lod_boundary_neighbor_edge_height(
    neighbor: Option<&ChunkData>,
    face: PackedVoxelFace,
    column: LodColumn,
) -> usize {
    let Some(neighbor) = neighbor else {
        return 0;
    };

    let (x, z, width, depth) = match face {
        PackedVoxelFace::PlusX => (0, column.z, 1, column.depth),
        PackedVoxelFace::MinusX => (CHUNK_SIZE.saturating_sub(1), column.z, 1, column.depth),
        PackedVoxelFace::PlusZ => (column.x, 0, column.width, 1),
        PackedVoxelFace::MinusZ => (column.x, CHUNK_SIZE.saturating_sub(1), column.width, 1),
        _ => return 0,
    };

    let neighbor_face = match face {
        PackedVoxelFace::PlusX => PackedVoxelFace::MinusX,
        PackedVoxelFace::MinusX => PackedVoxelFace::PlusX,
        PackedVoxelFace::PlusZ => PackedVoxelFace::MinusZ,
        PackedVoxelFace::MinusZ => PackedVoxelFace::PlusZ,
        _ => return 0,
    };

    lod_edge_min_height(neighbor, x, z, width, depth, neighbor_face)
}

fn push_lod_wall(
    quads: &mut Vec<PackedVoxelQuad>,
    column: LodColumn,
    neighbor_height: usize,
    face: PackedVoxelFace,
    lod: u8,
) {
    if neighbor_height >= column.height {
        return;
    }
    let wall_height = column.height - neighbor_height;
    if wall_height == 0 {
        return;
    }

    let (origin, size) = match face {
        PackedVoxelFace::PlusX => (
            [
                (column.x + column.width - 1) as u16,
                neighbor_height as u16,
                column.z as u16,
            ],
            [wall_height as u16, column.depth as u16],
        ),
        PackedVoxelFace::MinusX => (
            [column.x as u16, neighbor_height as u16, column.z as u16],
            [wall_height as u16, column.depth as u16],
        ),
        PackedVoxelFace::PlusZ => (
            [
                column.x as u16,
                neighbor_height as u16,
                (column.z + column.depth - 1) as u16,
            ],
            [column.width as u16, wall_height as u16],
        ),
        PackedVoxelFace::MinusZ => (
            [column.x as u16, neighbor_height as u16, column.z as u16],
            [column.width as u16, wall_height as u16],
        ),
        _ => return,
    };

    quads.push(PackedVoxelQuad::new(
        origin,
        size,
        column.block_id,
        face as u8,
        lod,
        0,
    ));
}

/// Builds coarse terrain packed quads for distant chunks. This intentionally
/// approximates only the visible terrain shell, preserving the packed quad ABI
/// while cutting far chunk geometry.
pub fn build_lod_packed_quads_for_chunk_with_neighbors(
    chunk: &ChunkData,
    neighbors: ChunkNeighbors,
    requested_cell_size: usize,
) -> Vec<PackedVoxelQuad> {
    let cell_size = requested_cell_size.clamp(2, CHUNK_SIZE);
    let lod = match cell_size {
        0 | 1 => 0,
        2 => 1,
        3 | 4 => 2,
        _ => 3,
    };
    let cells_x = CHUNK_SIZE.div_ceil(cell_size);
    let cells_z = CHUNK_SIZE.div_ceil(cell_size);
    let mut columns = vec![None; cells_x * cells_z];

    for cell_z in 0..cells_z {
        for cell_x in 0..cells_x {
            let x = cell_x * cell_size;
            let z = cell_z * cell_size;
            columns[cell_z * cells_x + cell_x] =
                lod_column_from_chunk(chunk, x, z, cell_size, cell_size);
        }
    }

    let mut quads = Vec::with_capacity(cells_x * cells_z * 3);

    for cell_z in 0..cells_z {
        for cell_x in 0..cells_x {
            let Some(column) = columns[cell_z * cells_x + cell_x] else {
                continue;
            };

            quads.push(PackedVoxelQuad::new(
                [column.x as u16, (column.height - 1) as u16, column.z as u16],
                [column.width as u16, column.depth as u16],
                column.block_id,
                PackedVoxelFace::PlusY as u8,
                lod,
                0,
            ));

            let plus_x_edge_height = if cell_x + 1 < cells_x {
                columns[cell_z * cells_x + cell_x + 1]
                    .map(|column| column.minus_x_edge_min_height)
                    .unwrap_or(0)
            } else {
                lod_boundary_neighbor_edge_height(neighbors.plus_x, PackedVoxelFace::PlusX, column)
            };
            let minus_x_edge_height = if cell_x > 0 {
                columns[cell_z * cells_x + cell_x - 1]
                    .map(|column| column.plus_x_edge_min_height)
                    .unwrap_or(0)
            } else {
                lod_boundary_neighbor_edge_height(
                    neighbors.minus_x,
                    PackedVoxelFace::MinusX,
                    column,
                )
            };
            let plus_z_edge_height = if cell_z + 1 < cells_z {
                columns[(cell_z + 1) * cells_x + cell_x]
                    .map(|column| column.minus_z_edge_min_height)
                    .unwrap_or(0)
            } else {
                lod_boundary_neighbor_edge_height(neighbors.plus_z, PackedVoxelFace::PlusZ, column)
            };
            let minus_z_edge_height = if cell_z > 0 {
                columns[(cell_z - 1) * cells_x + cell_x]
                    .map(|column| column.plus_z_edge_min_height)
                    .unwrap_or(0)
            } else {
                lod_boundary_neighbor_edge_height(
                    neighbors.minus_z,
                    PackedVoxelFace::MinusZ,
                    column,
                )
            };

            push_lod_wall(
                &mut quads,
                column,
                plus_x_edge_height,
                PackedVoxelFace::PlusX,
                lod,
            );
            push_lod_wall(
                &mut quads,
                column,
                minus_x_edge_height,
                PackedVoxelFace::MinusX,
                lod,
            );
            push_lod_wall(
                &mut quads,
                column,
                plus_z_edge_height,
                PackedVoxelFace::PlusZ,
                lod,
            );
            push_lod_wall(
                &mut quads,
                column,
                minus_z_edge_height,
                PackedVoxelFace::MinusZ,
                lod,
            );
        }
    }

    compact_packed_quads(&mut quads);
    quads
}

/// Legacy builder function that delegates to build_packed_quads_for_chunk_with_neighbors
/// with completely empty (None) neighbors.
pub fn build_packed_quads_for_chunk(chunk: &ChunkData) -> Vec<PackedVoxelQuad> {
    build_packed_quads_for_chunk_with_neighbors(chunk, ChunkNeighbors::default())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct MergeKey {
    face: u8,
    block_id: u16,
    meta: u32,
    p: u16,
    line: u16,
    span: u16,
}

#[inline]
fn quad_axes(quad: PackedVoxelQuad) -> (u16, u16, u16, u16, u16) {
    match quad.face() {
        face if face == PackedVoxelFace::PlusX as u8 || face == PackedVoxelFace::MinusX as u8 => (
            quad.origin[0],
            quad.origin[1],
            quad.origin[2],
            quad.size[0],
            quad.size[1],
        ),
        face if face == PackedVoxelFace::PlusY as u8 || face == PackedVoxelFace::MinusY as u8 => (
            quad.origin[1],
            quad.origin[0],
            quad.origin[2],
            quad.size[0],
            quad.size[1],
        ),
        _ => (
            quad.origin[2],
            quad.origin[0],
            quad.origin[1],
            quad.size[0],
            quad.size[1],
        ),
    }
}

#[inline]
fn quad_with_axes(
    template: PackedVoxelQuad,
    p: u16,
    u: u16,
    v: u16,
    size_u: u16,
    size_v: u16,
) -> PackedVoxelQuad {
    let origin = match template.face() {
        face if face == PackedVoxelFace::PlusX as u8 || face == PackedVoxelFace::MinusX as u8 => {
            [p, u, v]
        }
        face if face == PackedVoxelFace::PlusY as u8 || face == PackedVoxelFace::MinusY as u8 => {
            [u, p, v]
        }
        _ => [u, v, p],
    };

    PackedVoxelQuad {
        origin,
        size: [size_u, size_v],
        block_id: template.block_id,
        meta: template.meta,
    }
}

fn horizontal_merge_key(quad: PackedVoxelQuad) -> MergeKey {
    let (p, _u, v, _size_u, size_v) = quad_axes(quad);
    MergeKey {
        face: quad.face(),
        block_id: quad.block_id,
        meta: quad.meta,
        p,
        line: v,
        span: size_v,
    }
}

fn vertical_merge_key(quad: PackedVoxelQuad) -> MergeKey {
    let (p, u, _v, size_u, _size_v) = quad_axes(quad);
    MergeKey {
        face: quad.face(),
        block_id: quad.block_id,
        meta: quad.meta,
        p,
        line: u,
        span: size_u,
    }
}

fn merge_sorted_quads(
    quads: &mut Vec<PackedVoxelQuad>,
    key_fn: impl Fn(PackedVoxelQuad) -> MergeKey,
    axis_is_u: bool,
) -> bool {
    quads.sort_by_key(|quad| {
        let key = key_fn(*quad);
        let (_p, u, v, size_u, size_v) = quad_axes(*quad);
        (
            key,
            if axis_is_u { u } else { v },
            if axis_is_u { size_u } else { size_v },
        )
    });

    let original_len = quads.len();
    let mut merged = Vec::with_capacity(quads.capacity());

    for quad in quads.iter().copied() {
        let Some(last) = merged.last_mut() else {
            merged.push(quad);
            continue;
        };

        if key_fn(*last) != key_fn(quad) {
            merged.push(quad);
            continue;
        }

        let (last_p, last_u, last_v, last_size_u, last_size_v) = quad_axes(*last);
        let (_p, u, v, size_u, size_v) = quad_axes(quad);
        let can_merge = if axis_is_u {
            last_u.checked_add(last_size_u).is_some_and(|end| end == u)
                && last_v == v
                && last_size_v == size_v
        } else {
            last_v.checked_add(last_size_v).is_some_and(|end| end == v)
                && last_u == u
                && last_size_u == size_u
        };

        if can_merge {
            let new_size_u = if axis_is_u {
                last_size_u.checked_add(size_u)
            } else {
                Some(last_size_u)
            };
            let new_size_v = if axis_is_u {
                Some(last_size_v)
            } else {
                last_size_v.checked_add(size_v)
            };
            if let (Some(new_size_u), Some(new_size_v)) = (new_size_u, new_size_v) {
                *last = quad_with_axes(*last, last_p, last_u, last_v, new_size_u, new_size_v);
            } else {
                merged.push(quad);
            }
        } else {
            merged.push(quad);
        }
    }

    let changed = merged.len() != original_len;
    *quads = merged;
    changed
}

/// Merges adjacent coplanar packed quads after chunks have been grouped into a
/// larger region. This preserves the 16-byte quad ABI and repeat-UV semantics.
pub fn compact_packed_quads(quads: &mut Vec<PackedVoxelQuad>) {
    if quads.len() < 2 {
        return;
    }

    let reserved_capacity = quads.capacity();
    for _ in 0..4 {
        let horizontal_changed = merge_sorted_quads(quads, horizontal_merge_key, true);
        let vertical_changed = merge_sorted_quads(quads, vertical_merge_key, false);
        if !horizontal_changed && !vertical_changed {
            break;
        }
    }

    if quads.capacity() < reserved_capacity {
        quads.reserve_exact(reserved_capacity - quads.capacity());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rumpel_blocks::{BlockData, BlockRegistry};
    use rumpel_world::world_gen::{
        terrain_height_with_noise, terrain_surface_cell_height_from_world_cached,
        terrain_surface_cell_height_with_noise, terrain_surface_cell_sample_with_noise,
        terrain_surface_shell_height_with_noise,
    };

    #[test]
    fn test_packed_voxel_quad_abi() {
        assert_eq!(std::mem::size_of::<PackedVoxelQuad>(), 16);
        assert_eq!(std::mem::align_of::<PackedVoxelQuad>(), 4);

        let quads = vec![
            PackedVoxelQuad::new([1, 2, 3], [4, 5], 42, 2, 1, 100),
            PackedVoxelQuad::new([10, 20, 30], [40, 50], 999, 5, 0, 99999),
        ];

        let bytes = bytemuck::cast_slice::<PackedVoxelQuad, u8>(&quads);
        assert_eq!(bytes.len(), quads.len() * 16);

        // Roundtrip check: cast bytes back to PackedVoxelQuad and assert equality
        let cast_quads = bytemuck::cast_slice::<u8, PackedVoxelQuad>(bytes);
        assert_eq!(quads, cast_quads);
    }

    #[test]
    fn surface_packed_quads_keep_real_terrain_heights() {
        rumpel_world::chunk_gen_cache::reset_chunk_generation_cache();
        let registry = test_block_registry();
        let context = WorldGenerationContext::from_registry(&registry);
        let sand_block = registry.get_id("sand").unwrap_or(context.palette.dirt);
        let chunk_pos = (-8..=8)
            .flat_map(|z| (-8..=8).map(move |x| ChunkPos::new(x, z)))
            .find(|pos| max_sampled_surface_height(*pos, &context) >= CHUNK_SIZE - 1)
            .expect("cached Lua terrain should include a tall surface column");

        let expected_max_height = max_sampled_surface_height(chunk_pos, &context);
        let quads = build_surface_packed_quads_for_chunk(chunk_pos, &context, 1, sand_block);
        let max_top_height = quads
            .iter()
            .filter(|quad| quad.face() == PackedVoxelFace::PlusY as u8)
            .map(|quad| usize::from(quad.origin[1]) + 1)
            .max()
            .expect("surface chunk should emit top quads");

        assert_eq!(max_top_height, expected_max_height);
        assert!(max_top_height >= CHUNK_SIZE - 1);
    }

    #[test]
    fn surface_packed_quads_emit_layered_side_materials() {
        let registry = test_block_registry();
        let context = WorldGenerationContext::from_registry(&registry);
        let column = SurfacePackedColumn {
            x: 0,
            z: 0,
            width: 1,
            depth: 1,
            height: 20,
            top_block: context.palette.grass,
        };

        assert_eq!(
            surface_wall_block(column, 0, context.palette),
            context.palette.stone
        );
        assert_eq!(
            surface_wall_block(column, 18, context.palette),
            context.palette.dirt
        );
        assert_eq!(
            surface_wall_block(column, 19, context.palette),
            context.palette.grass
        );
    }

    #[test]
    fn surface_packed_lod_walls_keep_grass_cap_at_cell_depth() {
        let registry = test_block_registry();
        let context = WorldGenerationContext::from_registry(&registry);
        let column = SurfacePackedColumn {
            x: 0,
            z: 0,
            width: 4,
            depth: 4,
            height: 24,
            top_block: context.palette.grass,
        };

        assert_eq!(
            surface_wall_block(column, 19, context.palette),
            context.palette.dirt
        );
        assert_eq!(
            surface_wall_block(column, 20, context.palette),
            context.palette.grass
        );
        assert_eq!(
            surface_wall_block(column, 23, context.palette),
            context.palette.grass
        );
    }

    #[test]
    fn surface_packed_lod_grass_walls_mark_top_tile_flag() {
        let registry = test_block_registry();
        let context = WorldGenerationContext::from_registry(&registry);
        let column = SurfacePackedColumn {
            x: 0,
            z: 0,
            width: 2,
            depth: 2,
            height: 20,
            top_block: context.palette.grass,
        };
        let mut quads = Vec::new();

        push_surface_wall_segment(
            &mut quads,
            column,
            PackedVoxelFace::PlusX,
            18,
            20,
            context.palette.grass,
            1,
        );
        push_surface_wall_segment(
            &mut quads,
            column,
            PackedVoxelFace::PlusX,
            0,
            18,
            context.palette.dirt,
            1,
        );

        assert_eq!(quads[0].flags(), PACKED_QUAD_FLAG_SIDE_BLENDS_TOP_TILE);
        assert_eq!(quads[1].flags(), 0);
    }

    #[test]
    fn generated_surface_columns_match_cpu_packed_columns_across_regions() {
        let registry = test_block_registry();
        let context = WorldGenerationContext::from_registry(&registry);
        let sand_block = registry.get_id("sand").unwrap_or(context.palette.dirt);
        let perlin = terrain_perlin();

        for chunk_pos in [
            ChunkPos::new(-8, -8),
            ChunkPos::new(-3, 5),
            ChunkPos::new(0, 0),
            ChunkPos::new(7, -4),
        ] {
            for cell_size in [1, 2, 4, 8] {
                let edit_store = WorldEditStore::default();
                let has_edits = !edit_store.is_empty();
                let cpu_columns =
                    build_surface_packed_columns_for_chunk_with_perlin(SurfaceColumnSource::new(
                        chunk_pos,
                        &context,
                        cell_size,
                        sand_block,
                        &edit_store,
                        &perlin,
                        has_edits,
                    ));
                let gpu_columns = build_surface_gpu_generation_columns_for_chunk(
                    chunk_pos,
                    &context,
                    cell_size,
                    sand_block,
                    &edit_store,
                );

                assert_eq!(cpu_columns.len(), gpu_columns.len());
                for (cpu, gpu) in cpu_columns.iter().zip(gpu_columns.iter()) {
                    assert_eq!(
                        gpu.local,
                        [
                            cpu.x as u32,
                            cpu.z as u32,
                            cpu.width as u32,
                            cpu.depth as u32
                        ]
                    );
                    assert_eq!(gpu.heights[0], cpu.height as u32);
                    assert_eq!(
                        gpu.heights[1],
                        surface_neighbor_height(
                            *cpu,
                            PackedVoxelFace::PlusX,
                            chunk_pos,
                            &context,
                            &edit_store,
                            &perlin,
                            has_edits,
                        ) as u32
                    );
                    assert_eq!(
                        gpu.heights[2],
                        surface_neighbor_height(
                            *cpu,
                            PackedVoxelFace::MinusX,
                            chunk_pos,
                            &context,
                            &edit_store,
                            &perlin,
                            has_edits,
                        ) as u32
                    );
                    assert_eq!(
                        gpu.heights[3],
                        surface_neighbor_height(
                            *cpu,
                            PackedVoxelFace::PlusZ,
                            chunk_pos,
                            &context,
                            &edit_store,
                            &perlin,
                            has_edits,
                        ) as u32
                    );
                    assert_eq!(
                        gpu.material[0],
                        surface_neighbor_height(
                            *cpu,
                            PackedVoxelFace::MinusZ,
                            chunk_pos,
                            &context,
                            &edit_store,
                            &perlin,
                            has_edits,
                        ) as u32
                    );
                    assert_eq!(gpu.material[1], u32::from(cpu.top_block));
                }
            }
        }
    }

    #[test]
    fn generated_surface_columns_are_deterministic_between_runs() {
        let registry = test_block_registry();
        let context = WorldGenerationContext::from_registry(&registry);
        let sand_block = registry.get_id("sand").unwrap_or(context.palette.dirt);
        let chunk_pos = ChunkPos::new(11, -9);

        let edit_store = WorldEditStore::default();
        let first = build_surface_gpu_generation_columns_for_chunk(
            chunk_pos,
            &context,
            4,
            sand_block,
            &edit_store,
        );
        let second = build_surface_gpu_generation_columns_for_chunk(
            chunk_pos,
            &context,
            4,
            sand_block,
            &edit_store,
        );

        assert_eq!(first, second);
    }

    #[test]
    fn generated_surface_columns_apply_local_offset_without_repacking() {
        let registry = test_block_registry();
        let context = WorldGenerationContext::from_registry(&registry);
        let sand_block = registry.get_id("sand").unwrap_or(context.palette.dirt);
        let chunk_pos = ChunkPos::new(2, -3);
        let edit_store = WorldEditStore::default();
        let base = build_surface_gpu_generation_columns_for_chunk(
            chunk_pos,
            &context,
            4,
            sand_block,
            &edit_store,
        );
        let mut offset = Vec::new();

        append_surface_gpu_generation_columns_for_chunk_with_local_offset(
            &mut offset,
            chunk_pos,
            &context,
            4,
            sand_block,
            &edit_store,
            [64, 96],
        );

        assert_eq!(offset.len(), base.len());
        for (base_column, offset_column) in base.iter().zip(offset.iter()) {
            assert_eq!(offset_column.local[0], base_column.local[0] + 64);
            assert_eq!(offset_column.local[1], base_column.local[1] + 96);
            assert_eq!(&offset_column.local[2..], &base_column.local[2..]);
            assert_eq!(offset_column.heights, base_column.heights);
            assert_eq!(offset_column.material, base_column.material);
        }
    }

    #[test]
    fn generated_surface_columns_apply_world_edit_heights() {
        let registry = test_block_registry();
        let context = WorldGenerationContext::from_registry(&registry);
        let sand_block = registry.get_id("sand").unwrap_or(context.palette.dirt);
        let perlin = terrain_perlin();
        let chunk_pos = ChunkPos::new(0, 0);
        let (local_x, local_z, baseline) = (0..CHUNK_SIZE)
            .find_map(|local_z| {
                (0..CHUNK_SIZE).find_map(|local_x| {
                    let global_x = chunk_pos.x * CHUNK_SIZE as i32 + local_x as i32;
                    let global_z = chunk_pos.z * CHUNK_SIZE as i32 + local_z as i32;
                    let sample = terrain_surface_cell_sample_with_noise(
                        global_x,
                        global_z,
                        1,
                        1,
                        context.palette,
                        sand_block,
                        &perlin,
                    );
                    (sample.height.saturating_add(64) < CHUNK_HEIGHT)
                        .then_some((local_x, local_z, sample))
                })
            })
            .expect("test terrain should contain an editable low surface inside the origin chunk");
        // Lift the edit above the procedural shell while keeping it inside
        // the surface edit scan window used by the world sampler. The 5x5
        // shell kernel still detects the change after integer rounding.
        let edit_y = baseline.height.saturating_add(20).min(CHUNK_HEIGHT - 1);
        let mut edit_store = WorldEditStore::default();
        let edit_index = rumpel_world::chunk::ChunkData::get_index(local_x, edit_y, local_z);

        assert!(
            edit_store.apply_edit(
                rumpel_world::chunk::WorldBlockEdit::from_single_chunk_index(
                    edit_index,
                    context.palette.grass,
                )
                .expect("editable test surface should fit inside one chunk"),
            )
        );
        let global_x = chunk_pos.x * CHUNK_SIZE as i32 + local_x as i32;
        let global_z = chunk_pos.z * CHUNK_SIZE as i32 + local_z as i32;
        let edited_sample = terrain_surface_cell_sample_with_edits(
            global_x,
            global_z,
            1,
            1,
            context.palette,
            sand_block,
            &edit_store,
            &perlin,
        );
        assert!(edited_sample.height > baseline.height);

        let gpu_columns = build_surface_gpu_generation_columns_for_chunk(
            chunk_pos,
            &context,
            1,
            sand_block,
            &edit_store,
        );
        let edited_column = gpu_columns
            .iter()
            .find(|column| column.local[0] == local_x as u32 && column.local[1] == local_z as u32)
            .expect("edited surface column should be generated");

        assert_eq!(edited_column.heights[0], edited_sample.height as u32);
        assert_eq!(
            edited_column.material[1],
            u32::from(edited_sample.top_block)
        );
    }

    #[test]
    fn surface_shell_height_smoothing_reduces_adjacent_step_noise() {
        let perlin = terrain_perlin();
        let mut raw_delta_sum = 0usize;
        let mut shell_delta_sum = 0usize;

        for z in -64..64 {
            for x in -64..64 {
                let raw_height = terrain_height_with_noise(x, z, &perlin);
                let shell_height = terrain_surface_shell_height_with_noise(x, z, &perlin);

                for (sample_x, sample_z) in [(x + 1, z), (x, z + 1)] {
                    raw_delta_sum +=
                        raw_height.abs_diff(terrain_height_with_noise(sample_x, sample_z, &perlin));
                    shell_delta_sum += shell_height.abs_diff(
                        terrain_surface_shell_height_with_noise(sample_x, sample_z, &perlin),
                    );
                }
            }
        }

        assert!(shell_delta_sum < raw_delta_sum);
    }

    #[test]
    fn surface_cell_height_averages_covered_shell_samples() {
        let perlin = terrain_perlin();
        let global_x = 17;
        let global_z = -23;
        let width = 4;
        let depth = 3;
        let mut expected_sum = 0;

        for offset_z in 0..depth {
            for offset_x in 0..width {
                expected_sum += terrain_surface_shell_height_with_noise(
                    global_x + offset_x as i32,
                    global_z + offset_z as i32,
                    &perlin,
                );
            }
        }

        assert_eq!(
            terrain_surface_cell_height_with_noise(global_x, global_z, width, depth, &perlin),
            (expected_sum + (width * depth) / 2) / (width * depth)
        );
    }

    fn max_sampled_surface_height(chunk_pos: ChunkPos, context: &WorldGenerationContext) -> usize {
        let edit_store = WorldEditStore::default();
        let mut max_height = 0;
        for z in 0..CHUNK_SIZE {
            for x in 0..CHUNK_SIZE {
                let world_x = chunk_pos.x * CHUNK_SIZE as i32 + x as i32;
                let world_z = chunk_pos.z * CHUNK_SIZE as i32 + z as i32;
                max_height = max_height.max(terrain_surface_cell_height_from_world_cached(
                    world_x,
                    world_z,
                    1,
                    1,
                    context,
                    &edit_store,
                ));
            }
        }
        max_height
    }

    fn test_block_registry() -> BlockRegistry {
        let mut registry = BlockRegistry::default();
        for id in ["air", "dirt", "grass", "stone", "sand"] {
            registry.register_block(BlockData {
                id: id.to_string(),
                name: id.to_string(),
                is_solid: id != "air",
                is_transparent: id == "air",
                color: (1.0, 1.0, 1.0, 1.0),
                gravity_affected: false,
                wind_animated: false,
                strength: 1.0,
            });
        }
        registry
    }

    #[test]
    fn test_neighbor_plus_x_solid_culls_face() {
        let mut chunk = ChunkData::default();
        chunk.set_block(31, 10, 10, 3); // Solid block at boundary x=31

        let mut neighbor_x = ChunkData::default();
        neighbor_x.set_block(0, 10, 10, 3); // Solid block in +X neighbor at x=0

        let neighbors = ChunkNeighbors {
            plus_x: Some(&neighbor_x),
            ..Default::default()
        };

        let quads = build_packed_quads_for_chunk_with_neighbors(&chunk, neighbors);

        // Since +X neighbor has a solid block at x=0, the +X face of block at x=31 must be culled.
        let mut plus_x_found = false;
        for q in &quads {
            if q.origin == [31, 10, 10] && q.face() == PackedVoxelFace::PlusX as u8 {
                plus_x_found = true;
            }
        }
        assert!(
            !plus_x_found,
            "PlusX face should have been culled by solid neighbor"
        );
    }

    #[test]
    fn test_missing_neighbor_plus_x_keeps_face() {
        let mut chunk = ChunkData::default();
        chunk.set_block(31, 10, 10, 3); // Solid block at boundary x=31

        // No neighbors provided
        let neighbors = ChunkNeighbors::default();

        let quads = build_packed_quads_for_chunk_with_neighbors(&chunk, neighbors);

        // PlusX face (face 0) should be visible and generated since neighbor is None.
        let mut plus_x_found = false;
        for q in &quads {
            if q.origin == [31, 10, 10] && q.face() == PackedVoxelFace::PlusX as u8 {
                plus_x_found = true;
            }
        }
        assert!(
            plus_x_found,
            "PlusX face should be generated when neighbor is None"
        );
    }

    #[test]
    fn test_air_in_neighbor_keeps_face() {
        let mut chunk = ChunkData::default();
        chunk.set_block(31, 10, 10, 3); // Solid block at boundary x=31

        let neighbor_x = ChunkData::default(); // default chunk has only air blocks (AIR_BLOCK_ID = 0)

        let neighbors = ChunkNeighbors {
            plus_x: Some(&neighbor_x),
            ..Default::default()
        };

        let quads = build_packed_quads_for_chunk_with_neighbors(&chunk, neighbors);

        // PlusX face (face 0) should be visible and generated since neighbor at x=0 is air.
        let mut plus_x_found = false;
        for q in &quads {
            if q.origin == [31, 10, 10] && q.face() == PackedVoxelFace::PlusX as u8 {
                plus_x_found = true;
            }
        }
        assert!(
            plus_x_found,
            "PlusX face should be generated when neighbor contains air"
        );
    }

    #[test]
    fn test_neighbor_aware_determinism() {
        let mut chunk = ChunkData::default();
        chunk.set_block(31, 10, 10, 3);
        chunk.set_block(0, 12, 12, 3);

        let mut neighbor_x = ChunkData::default();
        neighbor_x.set_block(0, 10, 10, 3);

        let mut neighbor_minus_x = ChunkData::default();
        neighbor_minus_x.set_block(31, 12, 12, 3);

        let neighbors = ChunkNeighbors {
            plus_x: Some(&neighbor_x),
            minus_x: Some(&neighbor_minus_x),
            ..Default::default()
        };

        let quads_first = build_packed_quads_for_chunk_with_neighbors(&chunk, neighbors);
        let quads_second = build_packed_quads_for_chunk_with_neighbors(&chunk, neighbors);

        assert_eq!(quads_first, quads_second);
    }

    #[test]
    fn test_empty_chunk_gives_zero_quads() {
        let chunk = ChunkData::default();
        let quads = build_packed_quads_for_chunk(&chunk);
        assert_eq!(quads.len(), 0);
    }

    #[test]
    fn test_single_block_gives_six_quads() {
        let mut chunk = ChunkData::default();
        chunk.set_block(4, 4, 4, 3); // Spawn stone at (4, 4, 4)

        let quads = build_packed_quads_for_chunk(&chunk);
        assert_eq!(quads.len(), 6);

        // Verify that all 6 directions are represented with size [1, 1]
        let mut faces_found = [false; 6];
        for quad in &quads {
            assert_eq!(quad.origin, [4, 4, 4]);
            assert_eq!(quad.size, [1, 1]);
            assert_eq!(quad.block_id, 3);
            faces_found[quad.face() as usize] = true;
        }
        assert!(faces_found.iter().all(|&found| found));
    }

    #[test]
    fn test_two_adjacent_solid_blocks_cull_internal_face() {
        let mut chunk = ChunkData::default();
        // Spawn two stone blocks adjacent along the X axis
        chunk.set_block(10, 10, 10, 3);
        chunk.set_block(11, 10, 10, 3);

        let quads = build_packed_quads_for_chunk(&chunk);

        // Naive meshing would produce 12 faces.
        // Culling internal faces should give 10 faces.
        // In our 2D greedy merge:
        // - PlusX/MinusX faces are naive (since width/height along Y/Z is 1):
        //   - Block 10 has visible MinusX face (origin: [10, 10, 10], face: MinusX)
        //   - Block 11 has visible PlusX face (origin: [11, 10, 10], face: PlusX)
        //   - Internal PlusX (of 10) and MinusX (of 11) are culled!
        // - Horizontal faces (+Y, -Y, +Z, -Z) are greedy merged:
        //   - 4 horizontal faces of Block 10 and 11 merge along X into 4 quads of size [2, 1]!
        // So total quads: 2 (X-perpendicular) + 4 (merged horizontal) = 6 quads!
        assert_eq!(quads.len(), 6);

        let mut merged_horizontal_count = 0;
        let mut x_face_count = 0;

        for quad in &quads {
            if quad.face() == PackedVoxelFace::PlusX as u8
                || quad.face() == PackedVoxelFace::MinusX as u8
            {
                assert_eq!(quad.size, [1, 1]);
                x_face_count += 1;
            } else {
                assert_eq!(quad.size, [2, 1]);
                merged_horizontal_count += 1;
            }
        }
        assert_eq!(x_face_count, 2);
        assert_eq!(merged_horizontal_count, 4);
    }

    #[test]
    fn test_full_solid_chunk_generates_only_six_quads_with_2d_greedy() {
        let mut chunk = ChunkData::default();
        for x in 0..CHUNK_SIZE {
            for y in 0..CHUNK_SIZE {
                for z in 0..CHUNK_SIZE {
                    chunk.set_block(x, y, z, 3);
                }
            }
        }

        let quads = build_packed_quads_for_chunk(&chunk);

        // Full 2D greedy meshing of a completely solid chunk generates exactly 6 quads,
        // one for each of the 6 outer boundaries of size [32, 32].
        assert_eq!(quads.len(), 6);

        for quad in &quads {
            assert_eq!(quad.size, [32, 32]);
        }
    }

    #[test]
    fn test_flat_4x4_plate_top_face_merges_completely() {
        let mut chunk = ChunkData::default();
        // Place a 4x4 flat platform of stone blocks at y = 10, x = 2..6, z = 3..7
        for x in 2..6 {
            for z in 3..7 {
                chunk.set_block(x, 10, z, 3);
            }
        }

        let quads = build_packed_quads_for_chunk(&chunk);

        // Find the quad corresponding to the top (+Y, face 2) of this platform.
        // All 16 top faces should be merged into a single quad of size [4, 4] at origin [2, 10, 3].
        let mut top_quad = None;
        for q in &quads {
            if q.face() == PackedVoxelFace::PlusY as u8 && q.origin == [2, 10, 3] {
                top_quad = Some(*q);
            }
        }

        let quad = top_quad.expect("Should have found top face quad at [2, 10, 3]");
        assert_eq!(quad.size, [4, 4]);
        assert_eq!(quad.block_id, 3);
    }

    #[test]
    fn test_compact_packed_quads_merges_chunk_boundary_top_faces() {
        let mut quads = vec![
            PackedVoxelQuad::new([0, 10, 0], [32, 32], 3, PackedVoxelFace::PlusY as u8, 0, 0),
            PackedVoxelQuad::new([32, 10, 0], [32, 32], 3, PackedVoxelFace::PlusY as u8, 0, 0),
            PackedVoxelQuad::new([0, 10, 32], [32, 32], 3, PackedVoxelFace::PlusY as u8, 0, 0),
            PackedVoxelQuad::new(
                [32, 10, 32],
                [32, 32],
                3,
                PackedVoxelFace::PlusY as u8,
                0,
                0,
            ),
        ];

        compact_packed_quads(&mut quads);

        assert_eq!(quads.len(), 1);
        assert_eq!(quads[0].origin, [0, 10, 0]);
        assert_eq!(quads[0].size, [64, 64]);
        assert_eq!(quads[0].block_id, 3);
        assert_eq!(quads[0].face(), PackedVoxelFace::PlusY as u8);
    }

    #[test]
    fn test_compact_packed_quads_keeps_material_boundaries() {
        let mut quads = vec![
            PackedVoxelQuad::new([0, 10, 0], [32, 32], 3, PackedVoxelFace::PlusY as u8, 0, 0),
            PackedVoxelQuad::new([32, 10, 0], [32, 32], 4, PackedVoxelFace::PlusY as u8, 0, 0),
        ];

        compact_packed_quads(&mut quads);

        assert_eq!(quads.len(), 2);
    }

    #[test]
    fn test_lod_walls_use_touching_edge_height() {
        let mut chunk = ChunkData::default();
        chunk.set_block(0, 10, 0, 3);
        chunk.set_block(3, 12, 0, 3);

        let quads =
            build_lod_packed_quads_for_chunk_with_neighbors(&chunk, ChunkNeighbors::default(), 2);

        let plus_x_wall = quads.iter().find(|quad| {
            quad.face() == PackedVoxelFace::PlusX as u8
                && quad.origin == [1, 0, 0]
                && quad.size == [11, 2]
        });

        assert!(
            plus_x_wall.is_some(),
            "LOD wall should close against the neighboring cell's touching edge, not its max height"
        );
    }

    #[test]
    fn test_compact_packed_quads_merges_vertical_side_faces() {
        let mut quads = vec![
            PackedVoxelQuad::new([4, 0, 0], [16, 32], 3, PackedVoxelFace::PlusX as u8, 0, 0),
            PackedVoxelQuad::new([4, 0, 32], [16, 32], 3, PackedVoxelFace::PlusX as u8, 0, 0),
        ];

        compact_packed_quads(&mut quads);

        assert_eq!(quads.len(), 1);
        assert_eq!(quads[0].origin, [4, 0, 0]);
        assert_eq!(quads[0].size, [16, 64]);
        assert_eq!(quads[0].face(), PackedVoxelFace::PlusX as u8);
    }

    #[test]
    fn test_determinism_returns_identical_vectors() {
        let mut chunk = ChunkData::default();
        for x in &[2, 5, 8, 12, 18, 25] {
            for y in &[1, 4, 10, 16, 22, 28] {
                for z in &[3, 7, 11, 15, 20, 30] {
                    chunk.set_block(*x, *y, *z, 3);
                }
            }
        }

        let quads_first = build_packed_quads_for_chunk(&chunk);
        let quads_second = build_packed_quads_for_chunk(&chunk);

        assert_eq!(quads_first, quads_second);
    }
}
