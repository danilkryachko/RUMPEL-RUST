use std::sync::Arc;

use bevy::{platform::collections::HashMap, prelude::*};
use bevy_voxel_world::prelude::{
    ChunkDespawnStrategy, ChunkSpawnStrategy, VoxelLookupDelegate, VoxelWorldConfig, WorldVoxel,
};
use noise::{NoiseFn, Perlin};
use rumpel_blocks::{AIR_BLOCK_ID, BlockId, BlockRegistry};
use rumpel_coords::WorldBlockPos;

pub use bevy_voxel_world::prelude::{
    Chunk as VoxelChunk, VoxelWorld, VoxelWorldCamera, VoxelWorldPlugin,
};

const DEFAULT_SEED: u32 = 1337;
const DEFAULT_SPAWNING_DISTANCE: u32 = 16;
const DEFAULT_MIN_DESPAWN_DISTANCE: u32 = 2;
const DEFAULT_MAX_SPAWN_PER_FRAME: usize = 100_000;
const DEFAULT_MAX_ACTIVE_CHUNK_THREADS: usize = 64;
const DEFAULT_SPAWNING_RAYS: usize = 4096;
const DEFAULT_SPAWNING_RAY_MARGIN: u32 = 128;
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

#[derive(Resource, Clone, Debug)]
pub struct RumpelVoxelWorld {
    block_ids: TerrainBlockIds,
    seed: u32,
    spawning_distance: u32,
    texture_mappings: Arc<std::sync::RwLock<std::collections::HashMap<BlockId, [u32; 3]>>>,
}

impl Default for RumpelVoxelWorld {
    fn default() -> Self {
        Self {
            block_ids: TerrainBlockIds::default(),
            seed: DEFAULT_SEED,
            spawning_distance: DEFAULT_SPAWNING_DISTANCE,
            texture_mappings: Arc::new(std::sync::RwLock::new(std::collections::HashMap::new())),
        }
    }
}

impl RumpelVoxelWorld {
    #[must_use]
    pub fn from_registry(registry: &BlockRegistry) -> Self {
        Self {
            block_ids: TerrainBlockIds::from_registry(registry),
            texture_mappings: registry.texture_mappings.clone(),
            ..default()
        }
    }
}

impl VoxelWorldConfig for RumpelVoxelWorld {
    type MaterialIndex = BlockId;
    type ChunkUserBundle = ();

    fn spawning_distance(&self) -> u32 {
        self.spawning_distance
    }

    fn min_despawn_distance(&self) -> u32 {
        DEFAULT_MIN_DESPAWN_DISTANCE
    }

    fn chunk_despawn_strategy(&self) -> ChunkDespawnStrategy {
        ChunkDespawnStrategy::FarAway
    }

    fn chunk_spawn_strategy(&self) -> ChunkSpawnStrategy {
        ChunkSpawnStrategy::CloseAndInView
    }

    fn max_spawn_per_frame(&self) -> usize {
        DEFAULT_MAX_SPAWN_PER_FRAME
    }

    fn max_active_chunk_threads(&self) -> usize {
        std::thread::available_parallelism()
            .map(|n| (n.get() as usize).saturating_sub(2).max(1))
            .unwrap_or(DEFAULT_MAX_ACTIVE_CHUNK_THREADS)
    }

    fn spawning_rays(&self) -> usize {
        DEFAULT_SPAWNING_RAYS
    }

    fn spawning_ray_margin(&self) -> u32 {
        DEFAULT_SPAWNING_RAY_MARGIN
    }

    fn voxel_lookup_delegate(&self) -> VoxelLookupDelegate<Self::MaterialIndex> {
        let block_ids = self.block_ids;
        let seed = self.seed;
        Box::new(move |_chunk_pos, _lod, _previous| {
            let perlin = Perlin::new(seed);
            let mut height_cache = HashMap::<(i32, i32), i32>::new();

            Box::new(move |pos: IVec3, _previous| {
                terrain_voxel_at(pos.into(), block_ids, &perlin, &mut height_cache)
            })
        })
    }

    fn voxel_texture(&self) -> Option<(String, u32)> {
        Some(("textures/blocks/voxel_texture_array.png".to_string(), 28))
    }

    fn texture_index_mapper(&self) -> Arc<dyn Fn(Self::MaterialIndex) -> [u32; 3] + Send + Sync> {
        let mappings = self.texture_mappings.clone();
        Arc::new(move |material| {
            if let Ok(map) = mappings.read() {
                if let Some(indices) = map.get(&material) {
                    return *indices;
                }
            }
            [3, 3, 3] // Default fallback to stone (3)
        })
    }
}

fn terrain_voxel_at(
    pos: WorldBlockPos,
    block_ids: TerrainBlockIds,
    perlin: &Perlin,
    height_cache: &mut HashMap<(i32, i32), i32>,
) -> WorldVoxel<BlockId> {
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

    // Sandy beach condition (at or below water/sea-level threshold)
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

    // Stateless procedural tree generation in empty air
    if base_voxel == WorldVoxel::Air {
        let check_radius = 2; // Look in a 5x5 column area
        'outer: for dx in -check_radius..=check_radius {
            for dz in -check_radius..=check_radius {
                let tx = block_pos.x + dx;
                let tz = block_pos.z + dz;

                // Deterministic stateless hash of tree coordinates
                let mut hash = tx.wrapping_mul(73856093) ^ tz.wrapping_mul(19349663);
                hash = hash.wrapping_abs();

                // 2.5% chance of tree per column - check this first to avoid height/noise computation!
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

                // Trees only spawn on grass columns (above sea level)
                if th <= 14 {
                    continue;
                }

                // Pruning check: if y coordinate is not in tree range [th + 1, th + 7], skip calculations
                if block_pos.y < th + 1 || block_pos.y > th + 7 {
                    continue;
                }

                let trunk_min = th + 1;
                let trunk_max = th + 5;

                // Spawn wood trunk
                if tx == block_pos.x && tz == block_pos.z && block_pos.y >= trunk_min && block_pos.y <= trunk_max {
                    base_voxel = WorldVoxel::Solid(block_ids.wood);
                    break 'outer;
                }

                // Spawn leaf canopy centered at top of the trunk
                let leaf_center_y = th + 5;
                let dy = block_pos.y - leaf_center_y;
                if dy >= -1 && dy <= 2 {
                    let ldx = block_pos.x - tx;
                    let ldz = block_pos.z - tz;

                    // Spherical leaf canopy
                    let dist_sq = ldx * ldx + dy * dy + ldz * ldz;
                    if dist_sq <= 5 {
                        // Don't overwrite the wood trunk
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

