//! Textured voxel meshes for Lua world-gen features (trees, structures) above the heightmap shell.

use bevy::{
    asset::RenderAssetUsages, mesh::Indices, platform::collections::HashMap, prelude::*,
    render::render_resource::PrimitiveTopology,
};
use rumpel_prelude::*;
use rumpel_world::{
    chunk::{CHUNK_SIZE, WorldEditStore},
    chunk_gen_cache::cached_chunk,
    world_gen::{WorldGenerationContext, is_terrain_shell_block},
};

use crate::voxel_material::{ATTRIBUTE_VOXEL_REPEAT_UV, ATTRIBUTE_VOXEL_TILE};

#[derive(Clone)]
pub struct FeatureOverlayContext {
    pub world: WorldGenerationContext,
    textures: FeatureTexturePalette,
    air: BlockId,
    leaves: BlockId,
    /// Sand block ID, included in the terrain-shell filter alongside stone/dirt/grass.
    sand: BlockId,
}

impl FeatureOverlayContext {
    pub fn from_registry(registry: &BlockRegistry) -> Self {
        let world = WorldGenerationContext::from_registry(registry);
        let sand = world.block_id("sand");
        Self {
            textures: FeatureTexturePalette::from_registry(registry),
            air: world.palette.air,
            leaves: registry.get_id("leaves").unwrap_or(world.palette.air),
            sand,
            world,
        }
    }

    fn face_style(&self, block: BlockId, face: FeatureFaceTexture) -> FeatureFaceStyle {
        FeatureFaceStyle {
            tile: self.textures.face_tile(block, face),
            color: [1.0, 1.0, 1.0, 1.0],
        }
    }
}

/// Builds a textured mesh for non-procedural blocks placed by Lua world gen in one chunk.
///
/// "Non-procedural" means the block is not air, not leaves (rendered via decor billboards),
/// and not a terrain-shell block (grass / dirt / stone / sand). This matches the filter used
/// by `terrain_surface_cell_sample_from_world_cached` so the baseline is derived from the
/// cached Lua chunk rather than the analytical noise function.
#[must_use]
pub fn build_lua_feature_mesh_for_chunk(
    chunk_pos: ChunkPos,
    mesh_origin_x: i32,
    mesh_origin_z: i32,
    context: &FeatureOverlayContext,
    edit_store: &WorldEditStore,
) -> Option<Mesh> {
    let generated = cached_chunk(chunk_pos, &context.world);
    let mut chunk = generated.chunk;
    edit_store.apply_all_edits_to_chunk(chunk_pos, &mut chunk);

    let base_x = chunk_pos.x * CHUNK_SIZE as i32 - mesh_origin_x;
    let base_z = chunk_pos.z * CHUNK_SIZE as i32 - mesh_origin_z;
    let palette = context.world.palette;
    let sand = context.sand;

    let mut buffers = FeatureOverlayMeshBuffers::with_block_capacity(CHUNK_SIZE * CHUNK_SIZE * 8);

    for z in 0..CHUNK_SIZE {
        for x in 0..CHUNK_SIZE {
            for y in 0..CHUNK_SIZE {
                let block = chunk.get_block(x, y, z);
                if block == context.air
                    || block == context.leaves
                    || is_terrain_shell_block(block, palette, sand)
                {
                    continue;
                }

                let world_x = (base_x + x as i32) as f32;
                let world_y = y as f32;
                let world_z = (base_z + z as i32) as f32;

                if y + 1 >= CHUNK_SIZE || chunk.get_block(x, y + 1, z) == context.air {
                    buffers.add_block_face(
                        world_x,
                        world_y,
                        world_z,
                        FeatureFace::Top,
                        context.face_style(block, FeatureFaceTexture::Top),
                    );
                }

                let side_faces = [
                    (x.checked_sub(1), Some(z), FeatureFace::West),
                    (
                        (x + 1 < CHUNK_SIZE).then_some(x + 1),
                        Some(z),
                        FeatureFace::East,
                    ),
                    (Some(x), z.checked_sub(1), FeatureFace::North),
                    (
                        Some(x),
                        (z + 1 < CHUNK_SIZE).then_some(z + 1),
                        FeatureFace::South,
                    ),
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
                            context.face_style(block, FeatureFaceTexture::Side),
                        );
                    }
                }
            }
        }
    }

    buffers.into_mesh()
}

#[derive(Clone)]
struct FeatureTexturePalette {
    blocks: HashMap<BlockId, [u32; 3]>,
    fallback: [u32; 3],
}

impl FeatureTexturePalette {
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

    fn face_tile(&self, block: BlockId, face: FeatureFaceTexture) -> u32 {
        let textures = self.blocks.get(&block).unwrap_or(&self.fallback);
        textures[face.index()]
    }
}

#[derive(Clone, Copy)]
struct FeatureFaceStyle {
    tile: u32,
    color: [f32; 4],
}

#[derive(Clone, Copy)]
enum FeatureFace {
    Top,
    North,
    South,
    West,
    East,
}

enum FeatureFaceTexture {
    Top,
    Side,
}

impl FeatureFaceTexture {
    fn index(self) -> usize {
        match self {
            Self::Top => 0,
            Self::Side => 1,
        }
    }
}

struct FeatureOverlayMeshBuffers {
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    colors: Vec<[f32; 4]>,
    repeat_uvs: Vec<[f32; 2]>,
    tile_ids: Vec<u32>,
    indices: Vec<u32>,
}

impl FeatureOverlayMeshBuffers {
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

    fn into_mesh(self) -> Option<Mesh> {
        if self.positions.is_empty() {
            return None;
        }

        Some(
            Mesh::new(
                PrimitiveTopology::TriangleList,
                RenderAssetUsages::RENDER_WORLD,
            )
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, self.positions)
            .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, self.normals)
            .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, self.colors)
            .with_inserted_attribute(ATTRIBUTE_VOXEL_REPEAT_UV, self.repeat_uvs)
            .with_inserted_attribute(ATTRIBUTE_VOXEL_TILE, self.tile_ids)
            .with_inserted_indices(Indices::U32(self.indices)),
        )
    }

    fn add_block_face(
        &mut self,
        x: f32,
        y: f32,
        z: f32,
        face: FeatureFace,
        style: FeatureFaceStyle,
    ) {
        let (corners, normal) = match face {
            FeatureFace::Top => (
                [
                    [x, y + 1.0, z],
                    [x, y + 1.0, z + 1.0],
                    [x + 1.0, y + 1.0, z + 1.0],
                    [x + 1.0, y + 1.0, z],
                ],
                [0.0, 1.0, 0.0],
            ),
            FeatureFace::North => (
                [
                    [x + 1.0, y, z],
                    [x, y, z],
                    [x, y + 1.0, z],
                    [x + 1.0, y + 1.0, z],
                ],
                [0.0, 0.0, -1.0],
            ),
            FeatureFace::South => (
                [
                    [x, y, z + 1.0],
                    [x + 1.0, y, z + 1.0],
                    [x + 1.0, y + 1.0, z + 1.0],
                    [x, y + 1.0, z + 1.0],
                ],
                [0.0, 0.0, 1.0],
            ),
            FeatureFace::West => (
                [
                    [x, y, z],
                    [x, y, z + 1.0],
                    [x, y + 1.0, z + 1.0],
                    [x, y + 1.0, z],
                ],
                [-1.0, 0.0, 0.0],
            ),
            FeatureFace::East => (
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
            FeatureFace::Top => quad_repeat_uvs(1.0, 1.0),
            FeatureFace::North | FeatureFace::South | FeatureFace::West | FeatureFace::East => {
                side_repeat_uvs(face, 1.0, 1.0)
            }
        };
        self.add_quad(corners, normal, repeat_uvs, style);
    }

    fn add_quad(
        &mut self,
        corners: [[f32; 3]; 4],
        normal: [f32; 3],
        repeat_uvs: [[f32; 2]; 4],
        style: FeatureFaceStyle,
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

fn side_repeat_uvs(face: FeatureFace, width: f32, height: f32) -> [[f32; 2]; 4] {
    match face {
        FeatureFace::North | FeatureFace::East => {
            [[width, height], [0.0, height], [0.0, 0.0], [width, 0.0]]
        }
        FeatureFace::South | FeatureFace::West => {
            [[0.0, height], [width, height], [width, 0.0], [0.0, 0.0]]
        }
        FeatureFace::Top => quad_repeat_uvs(width, height),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rumpel_blocks::BlockRegistry;

    #[test]
    fn empty_chunk_produces_no_feature_mesh() {
        let registry = BlockRegistry::empty();
        let context = FeatureOverlayContext::from_registry(&registry);
        let edit_store = WorldEditStore::default();
        let mesh =
            build_lua_feature_mesh_for_chunk(ChunkPos::new(0, 0), 0, 0, &context, &edit_store);
        assert!(mesh.is_none());
    }
}
