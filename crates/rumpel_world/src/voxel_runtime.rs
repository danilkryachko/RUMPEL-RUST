use std::sync::Arc;
use bevy::{platform::collections::HashMap, prelude::*};
use noise::{NoiseFn, Perlin};
use rumpel_blocks::{AIR_BLOCK_ID, BlockId, BlockRegistry};
use rumpel_coords::WorldBlockPos;

const DEFAULT_SEED: u32 = 1337;
const SURFACE_BASE_HEIGHT: f64 = 10.0;
const SURFACE_HEIGHT_RANGE: f64 = 40.0;
const SURFACE_NOISE_SCALE: f64 = 0.02;

#[derive(Clone, Copy, Debug)]
pub struct TerrainBlockIds {
    pub air: BlockId,
    pub dirt: BlockId,
    pub grass: BlockId,
    pub stone: BlockId,
    pub sand: BlockId,
    pub wood: BlockId,
    pub leaves: BlockId,
}

impl Default for TerrainBlockIds {
    fn default() -> Self {
        Self {
            air: AIR_BLOCK_ID,
            dirt: 1,
            grass: 2,
            stone: 3,
            sand: 4,
            wood: 5,
            leaves: 6,
        }
    }
}

impl TerrainBlockIds {
    #[must_use]
    pub fn from_registry(registry: &BlockRegistry) -> Self {
        let defaults = Self::default();
        Self {
            air: registry.get_id("air").unwrap_or(defaults.air),
            dirt: registry.get_id("dirt").unwrap_or(defaults.dirt),
            grass: registry.get_id("grass").unwrap_or(defaults.grass),
            stone: registry.get_id("stone").unwrap_or(defaults.stone),
            sand: registry.get_id("sand").unwrap_or(defaults.sand),
            wood: registry.get_id("wood").unwrap_or(defaults.wood),
            leaves: registry.get_id("leaves").unwrap_or(defaults.leaves),
        }
    }
}

// Temporary WorldVoxel enum to replace bevy_voxel_world's enum until we write ChunkData
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum WorldVoxel {
    Air,
    Solid(BlockId),
}

pub fn terrain_voxel_at(
    pos: WorldBlockPos,
    block_ids: TerrainBlockIds,
    perlin: &Perlin,
    height_cache: &mut HashMap<(i32, i32), i32>,
) -> WorldVoxel {
    let block_pos = pos.position;
    if block_pos.y < 0 {
        return WorldVoxel::Solid(block_ids.stone);
    }

    let terrain_height = *height_cache
        .entry((block_pos.x, block_pos.z))
        .or_insert_with(|| {
            let noise_value = perlin.get([
                f64::from(block_pos.x) * SURFACE_NOISE_SCALE,
                f64::from(block_pos.z) * SURFACE_NOISE_SCALE,
            ]);
            ((noise_value + 1.0) * 0.5 * SURFACE_HEIGHT_RANGE + SURFACE_BASE_HEIGHT).floor() as i32
        });

    let is_beach = terrain_height <= 14;

    let mut base_voxel = match block_pos.y.cmp(&terrain_height) {
        std::cmp::Ordering::Greater => WorldVoxel::Air,
        std::cmp::Ordering::Equal => {
            if is_beach {
                WorldVoxel::Solid(block_ids.sand)
            } else {
                WorldVoxel::Solid(block_ids.grass)
            }
        }
        std::cmp::Ordering::Less if terrain_height - block_pos.y <= 3 => {
            if is_beach {
                WorldVoxel::Solid(block_ids.sand)
            } else {
                WorldVoxel::Solid(block_ids.dirt)
            }
        }
        std::cmp::Ordering::Less => WorldVoxel::Solid(block_ids.stone),
    };

    if base_voxel == WorldVoxel::Air {
        let check_radius = 2; 
        'outer: for dx in -check_radius..=check_radius {
            for dz in -check_radius..=check_radius {
                let tx = block_pos.x + dx;
                let tz = block_pos.z + dz;

                let mut hash = tx.wrapping_mul(73856093) ^ tz.wrapping_mul(19349663);
                hash = hash.wrapping_abs();

                if hash % 40 != 0 {
                    continue;
                }

                let th = *height_cache
                    .entry((tx, tz))
                    .or_insert_with(|| {
                        let noise_value = perlin.get([
                            f64::from(tx) * SURFACE_NOISE_SCALE,
                            f64::from(tz) * SURFACE_NOISE_SCALE,
                        ]);
                        ((noise_value + 1.0) * 0.5 * SURFACE_HEIGHT_RANGE + SURFACE_BASE_HEIGHT).floor() as i32
                    });

                if th <= 14 {
                    continue;
                }

                if block_pos.y < th + 1 || block_pos.y > th + 7 {
                    continue;
                }

                let trunk_min = th + 1;
                let trunk_max = th + 5;

                if tx == block_pos.x && tz == block_pos.z && block_pos.y >= trunk_min && block_pos.y <= trunk_max {
                    base_voxel = WorldVoxel::Solid(block_ids.wood);
                    break 'outer;
                }

                let leaf_center_y = th + 5;
                let dy = block_pos.y - leaf_center_y;
                if dy >= -1 && dy <= 2 {
                    let ldx = block_pos.x - tx;
                    let ldz = block_pos.z - tz;

                    let dist_sq = ldx * ldx + dy * dy + ldz * ldz;
                    if dist_sq <= 5 {
                        if !(ldx == 0 && ldz == 0 && block_pos.y <= trunk_max) {
                            base_voxel = WorldVoxel::Solid(block_ids.leaves);
                            break 'outer;
                        }
                    }
                }
            }
        }
    }

    base_voxel
}
