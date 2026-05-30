use bevy::prelude::*;
use bevy::render::mesh::{Indices, PrimitiveTopology};
use bevy::render::render_asset::RenderAssetUsages;

use rumpel_world::chunk::{Chunk, CHUNK_SIZE, CHUNK_HEIGHT};
use rumpel_blocks::BlockRegistry;

// Вектора нормалей и позиций вершин для 6 граней куба
const VOXEL_POSITIONS: [[f32; 3]; 8] = [
    [0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [1.0, 1.0, 0.0], [0.0, 1.0, 0.0], // Front
    [0.0, 0.0, 1.0], [1.0, 0.0, 1.0], [1.0, 1.0, 1.0], [0.0, 1.0, 1.0], // Back
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
                if block_id == 0 { continue; } // Air

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
                    
                    if nx < 0 || nx >= CHUNK_SIZE as i32 || 
                       ny < 0 || ny >= CHUNK_HEIGHT as i32 || 
                       nz < 0 || nz >= CHUNK_SIZE as i32 {
                        // At chunk boundary, draw face (for now, until we have neighbor chunks loaded)
                        draw_face = true;
                    } else {
                        let neighbor_id = chunk.get_block(nx as usize, ny as usize, nz as usize);
                        if neighbor_id == 0 {
                            draw_face = true; // Neighbor is air
                        } else {
                            if let Some(n_data) = registry.get_block(neighbor_id) {
                                if n_data.is_transparent && !block_data.is_transparent {
                                    draw_face = true;
                                }
                            }
                        }
                    }

                    if draw_face {
                        for i in 0..4 {
                            let v_idx = FACES[face_idx][i];
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

    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
    mesh.insert_indices(Indices::U32(indices));

    mesh
}
