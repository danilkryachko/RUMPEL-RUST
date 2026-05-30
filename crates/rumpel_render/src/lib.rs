use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use noise::Perlin;

use rumpel_prelude::*;

// Вектора нормалей и позиций вершин для 6 граней куба
const VOXEL_POSITIONS: [[f32; 3]; 8] = [
    [0.0, 0.0, 0.0],
    [1.0, 0.0, 0.0],
    [1.0, 1.0, 0.0],
    [0.0, 1.0, 0.0], // Front
    [0.0, 0.0, 1.0],
    [1.0, 0.0, 1.0],
    [1.0, 1.0, 1.0],
    [0.0, 1.0, 1.0], // Back
];

const FACES: [[usize; 4]; 6] = [
    [2, 3, 0, 1], // Front (Z-)
    [6, 5, 4, 7], // Back (Z+)
    [3, 7, 4, 0], // Left (X-)
    [6, 2, 1, 5], // Right (X+)
    [6, 7, 3, 2], // Top (Y+)
    [1, 0, 4, 5], // Bottom (Y-)
];

const NORMALS: [[f32; 3]; 6] = [
    [0.0, 0.0, -1.0], // Front
    [0.0, 0.0, 1.0],  // Back
    [-1.0, 0.0, 0.0], // Left
    [1.0, 0.0, 0.0],  // Right
    [0.0, 1.0, 0.0],  // Top
    [0.0, -1.0, 0.0], // Bottom
];

pub fn mesh_chunk(chunk: &Chunk, registry: &BlockRegistry) -> Mesh {
    let mut positions = Vec::new();
    let mut normals = Vec::new();
    let mut colors = Vec::new();
    let mut indices = Vec::new();

    let mut vertex_count = 0;

    for x in 0..CHUNK_SIZE {
        for y in 0..CHUNK_HEIGHT {
            for z in 0..CHUNK_SIZE {
                let block_id = chunk.get_block(x, y, z);
                if block_id == 0 {
                    continue;
                } // Air

                let block_data = registry.get_block(block_id).unwrap();
                let color = [
                    block_data.color.0,
                    block_data.color.1,
                    block_data.color.2,
                    block_data.color.3,
                ];

                // Check 6 neighbors
                let neighbors = [
                    (x as i32, y as i32, z as i32 - 1), // Front
                    (x as i32, y as i32, z as i32 + 1), // Back
                    (x as i32 - 1, y as i32, z as i32), // Left
                    (x as i32 + 1, y as i32, z as i32), // Right
                    (x as i32, y as i32 + 1, z as i32), // Top
                    (x as i32, y as i32 - 1, z as i32), // Bottom
                ];

                for (face_idx, &(nx, ny, nz)) in neighbors.iter().enumerate() {
                    let mut draw_face = false;

                    if nx < 0
                        || nx >= CHUNK_SIZE as i32
                        || ny < 0
                        || ny >= CHUNK_HEIGHT as i32
                        || nz < 0
                        || nz >= CHUNK_SIZE as i32
                    {
                        // At chunk boundary, draw face (for now, until we have neighbor chunks loaded)
                        draw_face = true;
                    } else {
                        let neighbor_id = chunk.get_block(nx as usize, ny as usize, nz as usize);
                        if neighbor_id == 0 {
                            draw_face = true; // Neighbor is air
                        } else if let Some(n_data) = registry.get_block(neighbor_id)
                            && n_data.is_transparent
                            && !block_data.is_transparent
                        {
                            draw_face = true;
                        }
                    }

                    if draw_face {
                        for &v_idx in &FACES[face_idx] {
                            let vx = VOXEL_POSITIONS[v_idx][0] + x as f32;
                            let vy = VOXEL_POSITIONS[v_idx][1] + y as f32;
                            let vz = VOXEL_POSITIONS[v_idx][2] + z as f32;

                            positions.push([vx, vy, vz]);
                            normals.push(NORMALS[face_idx]);
                            colors.push(color);
                        }

                        indices.push(vertex_count);
                        indices.push(vertex_count + 1);
                        indices.push(vertex_count + 2);
                        indices.push(vertex_count + 2);
                        indices.push(vertex_count + 3);
                        indices.push(vertex_count);

                        vertex_count += 4;
                    }
                }
            }
        }
    }

    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
    mesh.insert_indices(Indices::U32(indices));

    mesh
}

pub fn mesh_terrain_chunk(pos: ChunkPos, registry: &BlockRegistry) -> Mesh {
    mesh_terrain_chunk_with_palette(pos, TerrainMeshPalette::from_registry(registry))
}

pub fn mesh_terrain_chunk_with_palette(pos: ChunkPos, palette: TerrainMeshPalette) -> Mesh {
    mesh_terrain_chunk_with_detail(pos, palette, TerrainMeshDetail::full_resolution())
}

pub fn mesh_terrain_chunk_with_detail(
    pos: ChunkPos,
    palette: TerrainMeshPalette,
    detail: TerrainMeshDetail,
) -> Mesh {
    let height_grid = TerrainHeightGrid::new(pos, detail.sample_step);
    let mut positions = Vec::new();
    let mut normals = Vec::new();
    let mut colors = Vec::new();
    let mut indices = Vec::new();
    let cell_budget = (CHUNK_SIZE / detail.sample_step).max(1);
    positions.reserve(cell_budget * cell_budget * 20);
    normals.reserve(cell_budget * cell_budget * 20);
    colors.reserve(cell_budget * cell_budget * 20);
    indices.reserve(cell_budget * cell_budget * 30);
    let mut vertex_count = 0;

    for x in (0..CHUNK_SIZE).step_by(detail.sample_step) {
        for z in (0..CHUNK_SIZE).step_by(detail.sample_step) {
            let height = height_grid.get(x as i32, z as i32);
            if height == 0 {
                continue;
            }

            let top_color = palette.grass_color;
            let side_color = palette.dirt_color;
            let cell_x = detail.sample_step.min(CHUNK_SIZE - x) as f32;
            let cell_z = detail.sample_step.min(CHUNK_SIZE - z) as f32;
            let lx = x as f32;
            let lz = z as f32;
            let top_y = height as f32;

            push_quad(
                &mut positions,
                &mut normals,
                &mut colors,
                &mut indices,
                &mut vertex_count,
                [
                    [lx, top_y, lz + cell_z],
                    [lx + cell_x, top_y, lz + cell_z],
                    [lx + cell_x, top_y, lz],
                    [lx, top_y, lz],
                ],
                [0.0, 1.0, 0.0],
                top_color,
            );

            push_height_delta_face(
                palette,
                &height_grid,
                &mut positions,
                &mut normals,
                &mut colors,
                &mut indices,
                &mut vertex_count,
                x as i32,
                z as i32 - detail.sample_step as i32,
                height,
                [
                    [lx + cell_x, 0.0, lz],
                    [lx, 0.0, lz],
                    [lx, 0.0, lz],
                    [lx + cell_x, 0.0, lz],
                ],
                [0.0, 0.0, -1.0],
                side_color,
            );
            push_height_delta_face(
                palette,
                &height_grid,
                &mut positions,
                &mut normals,
                &mut colors,
                &mut indices,
                &mut vertex_count,
                x as i32,
                z as i32 + detail.sample_step as i32,
                height,
                [
                    [lx, 0.0, lz + cell_z],
                    [lx + cell_x, 0.0, lz + cell_z],
                    [lx + cell_x, 0.0, lz + cell_z],
                    [lx, 0.0, lz + cell_z],
                ],
                [0.0, 0.0, 1.0],
                side_color,
            );
            push_height_delta_face(
                palette,
                &height_grid,
                &mut positions,
                &mut normals,
                &mut colors,
                &mut indices,
                &mut vertex_count,
                x as i32 - detail.sample_step as i32,
                z as i32,
                height,
                [
                    [lx, 0.0, lz],
                    [lx, 0.0, lz + cell_z],
                    [lx, 0.0, lz + cell_z],
                    [lx, 0.0, lz],
                ],
                [-1.0, 0.0, 0.0],
                side_color,
            );
            push_height_delta_face(
                palette,
                &height_grid,
                &mut positions,
                &mut normals,
                &mut colors,
                &mut indices,
                &mut vertex_count,
                x as i32 + detail.sample_step as i32,
                z as i32,
                height,
                [
                    [lx + cell_x, 0.0, lz + cell_z],
                    [lx + cell_x, 0.0, lz],
                    [lx + cell_x, 0.0, lz],
                    [lx + cell_x, 0.0, lz + cell_z],
                ],
                [1.0, 0.0, 0.0],
                side_color,
            );
        }
    }

    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD,
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
    mesh.insert_indices(Indices::U32(indices));

    mesh
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TerrainMeshDetail {
    pub sample_step: usize,
}

impl TerrainMeshDetail {
    #[must_use]
    pub const fn full_resolution() -> Self {
        Self { sample_step: 1 }
    }

    #[must_use]
    pub const fn new(sample_step: usize) -> Self {
        Self { sample_step }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct TerrainMeshPalette {
    block_palette: TerrainBlockPalette,
    grass_color: [f32; 4],
    dirt_color: [f32; 4],
    stone_color: [f32; 4],
}

impl TerrainMeshPalette {
    #[must_use]
    pub fn from_registry(registry: &BlockRegistry) -> Self {
        let block_palette = TerrainBlockPalette::from_registry(registry);

        Self {
            block_palette,
            grass_color: block_color(registry, block_palette.grass),
            dirt_color: block_color(registry, block_palette.dirt),
            stone_color: block_color(registry, block_palette.stone),
        }
    }

    fn color_for_block(self, block_id: BlockId) -> [f32; 4] {
        if block_id == self.block_palette.grass {
            self.grass_color
        } else if block_id == self.block_palette.dirt {
            self.dirt_color
        } else if block_id == self.block_palette.stone {
            self.stone_color
        } else {
            self.dirt_color
        }
    }
}

struct TerrainHeightGrid {
    heights: Vec<usize>,
    margin: i32,
    size: usize,
}

impl TerrainHeightGrid {
    fn new(pos: ChunkPos, sample_step: usize) -> Self {
        let perlin = Perlin::new(1337);
        let margin = sample_step as i32;
        let size = CHUNK_SIZE + sample_step * 2;
        let mut heights = vec![0; size * size];

        for local_x in -margin..CHUNK_SIZE as i32 + margin {
            for local_z in -margin..CHUNK_SIZE as i32 + margin {
                let global_x = pos.x * CHUNK_SIZE as i32 + local_x;
                let global_z = pos.z * CHUNK_SIZE as i32 + local_z;
                let index = Self::index(local_x, local_z, margin, size);
                heights[index] = terrain_height_with_noise(global_x, global_z, &perlin);
            }
        }

        Self {
            heights,
            margin,
            size,
        }
    }

    fn get(&self, local_x: i32, local_z: i32) -> usize {
        self.heights[Self::index(local_x, local_z, self.margin, self.size)]
    }

    fn index(local_x: i32, local_z: i32, margin: i32, size: usize) -> usize {
        let x = (local_x + margin) as usize;
        let z = (local_z + margin) as usize;
        x * size + z
    }
}

#[allow(clippy::too_many_arguments)]
fn push_height_delta_face(
    palette: TerrainMeshPalette,
    height_grid: &TerrainHeightGrid,
    positions: &mut Vec<[f32; 3]>,
    normals: &mut Vec<[f32; 3]>,
    colors: &mut Vec<[f32; 4]>,
    indices: &mut Vec<u32>,
    vertex_count: &mut u32,
    neighbor_global_x: i32,
    neighbor_global_z: i32,
    height: usize,
    mut corners: [[f32; 3]; 4],
    normal: [f32; 3],
    fallback_color: [f32; 4],
) {
    let neighbor_height = height_grid.get(neighbor_global_x, neighbor_global_z);
    if neighbor_height >= height {
        return;
    }

    let low = neighbor_height as f32;
    let high = height as f32;
    corners[0][1] = high;
    corners[1][1] = high;
    corners[2][1] = low;
    corners[3][1] = low;

    let side_block = terrain_block_at_height(neighbor_height, height, palette.block_palette);
    let color = if side_block == palette.block_palette.air {
        fallback_color
    } else {
        palette.color_for_block(side_block)
    };

    push_quad(
        positions,
        normals,
        colors,
        indices,
        vertex_count,
        corners,
        normal,
        color,
    );
}

#[allow(clippy::too_many_arguments)]
fn push_quad(
    positions: &mut Vec<[f32; 3]>,
    normals: &mut Vec<[f32; 3]>,
    colors: &mut Vec<[f32; 4]>,
    indices: &mut Vec<u32>,
    vertex_count: &mut u32,
    corners: [[f32; 3]; 4],
    normal: [f32; 3],
    color: [f32; 4],
) {
    positions.extend(corners);
    normals.extend([normal; 4]);
    colors.extend([color; 4]);

    indices.push(*vertex_count);
    indices.push(*vertex_count + 1);
    indices.push(*vertex_count + 2);
    indices.push(*vertex_count + 2);
    indices.push(*vertex_count + 3);
    indices.push(*vertex_count);

    *vertex_count += 4;
}

fn block_color(registry: &BlockRegistry, block_id: BlockId) -> [f32; 4] {
    registry.get_block(block_id).map_or([1.0; 4], |block| {
        [block.color.0, block.color.1, block.color.2, block.color.3]
    })
}

pub fn mesh_terrain_macro_chunk_with_detail(
    chunks: &[ChunkPos],
    palette: TerrainMeshPalette,
    detail: TerrainMeshDetail,
) -> Mesh {
    let mut combined_positions = Vec::new();
    let mut combined_normals = Vec::new();
    let mut combined_colors = Vec::new();
    let mut combined_indices = Vec::new();
    let mut vertex_offset = 0u32;

    for &pos in chunks {
        let mesh = mesh_terrain_chunk_with_detail(pos, palette, detail);
        
        if let Some(bevy::mesh::VertexAttributeValues::Float32x3(positions)) = mesh.attribute(Mesh::ATTRIBUTE_POSITION) {
            if let Some(bevy::mesh::VertexAttributeValues::Float32x3(normals)) = mesh.attribute(Mesh::ATTRIBUTE_NORMAL) {
                if let Some(bevy::mesh::VertexAttributeValues::Float32x4(colors)) = mesh.attribute(Mesh::ATTRIBUTE_COLOR) {
                    let chunk_offset_x = (pos.x * CHUNK_SIZE as i32) as f32;
                    let chunk_offset_z = (pos.z * CHUNK_SIZE as i32) as f32;

                    combined_positions.extend(positions.iter().map(|p| {
                        [p[0] + chunk_offset_x, p[1], p[2] + chunk_offset_z]
                    }));
                    combined_normals.extend(normals.iter().copied());
                    combined_colors.extend(colors.iter().copied());

                    if let Some(indices) = mesh.indices() {
                        combined_indices.extend(indices.iter().map(|i| (i as u32) + vertex_offset));
                    }
                    
                    vertex_offset += positions.len() as u32;
                }
            }
        }
    }

    let mut combined_mesh = Mesh::new(
        bevy::mesh::PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD,
    );
    combined_mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, combined_positions);
    combined_mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, combined_normals);
    combined_mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, combined_colors);
    combined_mesh.insert_indices(bevy::mesh::Indices::U32(combined_indices));

    combined_mesh
}
