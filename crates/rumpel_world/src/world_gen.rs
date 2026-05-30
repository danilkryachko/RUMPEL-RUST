use crate::chunk::{CHUNK_SIZE, ChunkData};
use noise::{NoiseFn, Perlin};
use rumpel_blocks::{BlockId, BlockRegistry};
use rumpel_coords::ChunkPos;

const TERRAIN_SEED: u32 = 1337;
const TERRAIN_NOISE_SCALE: f64 = 0.02;
const TERRAIN_BASE_HEIGHT: f64 = 10.0;
const TERRAIN_HEIGHT_RANGE: f64 = 40.0;
const DIRT_DEPTH: usize = 3;

#[derive(Clone, Copy, Debug)]
pub struct TerrainBlockPalette {
    pub air: BlockId,
    pub dirt: BlockId,
    pub grass: BlockId,
    pub stone: BlockId,
}

impl TerrainBlockPalette {
    #[must_use]
    pub fn from_registry(registry: &BlockRegistry) -> Self {
        Self {
            air: registry.get_id("air").unwrap_or(0),
            dirt: registry.get_id("dirt").unwrap_or(0),
            grass: registry.get_id("grass").unwrap_or(0),
            stone: registry.get_id("stone").unwrap_or(0),
        }
    }
}

#[must_use]
pub fn terrain_height_at(global_x: i32, global_z: i32) -> usize {
    let perlin = Perlin::new(TERRAIN_SEED);
    terrain_height_with_noise(global_x, global_z, &perlin)
}

#[must_use]
pub fn terrain_height_with_noise(global_x: i32, global_z: i32, perlin: &Perlin) -> usize {
    let noise_val = perlin.get([
        f64::from(global_x) * TERRAIN_NOISE_SCALE,
        f64::from(global_z) * TERRAIN_NOISE_SCALE,
    ]);

    ((noise_val + 1.0) * 0.5 * TERRAIN_HEIGHT_RANGE + TERRAIN_BASE_HEIGHT) as usize
}

#[must_use]
pub fn terrain_block_at_height(
    y: usize,
    surface_height: usize,
    palette: TerrainBlockPalette,
) -> BlockId {
    if y >= surface_height {
        palette.air
    } else if y == surface_height - 1 {
        palette.grass
    } else if y > surface_height.saturating_sub(DIRT_DEPTH + 1) {
        palette.dirt
    } else {
        palette.stone
    }
}

pub fn generate_chunk(pos: ChunkPos, registry: &BlockRegistry) -> ChunkData {
    let mut chunk = ChunkData::default();
    let palette = TerrainBlockPalette::from_registry(registry);
    let perlin = Perlin::new(TERRAIN_SEED);

    for x in 0..CHUNK_SIZE {
        for z in 0..CHUNK_SIZE {
            let global_x = pos.x * CHUNK_SIZE as i32 + x as i32;
            let global_z = pos.z * CHUNK_SIZE as i32 + z as i32;
            let height = terrain_height_with_noise(global_x, global_z, &perlin);

            for y in 0..CHUNK_SIZE {
                let block_id = terrain_block_at_height(y, height, palette);
                if block_id != palette.air {
                    chunk.set_block(x, y, z, block_id);
                }
            }
        }
    }

    chunk
}
