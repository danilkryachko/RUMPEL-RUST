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
}

impl Default for TerrainBlockIds {
    fn default() -> Self {
        Self {
            air: AIR_BLOCK_ID,
            dirt: 1,
            grass: 2,
            stone: 3,
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
        }
    }
}

#[derive(Resource, Clone, Debug)]
pub struct RumpelVoxelWorld {
    block_ids: TerrainBlockIds,
    seed: u32,
    spawning_distance: u32,
}

impl Default for RumpelVoxelWorld {
    fn default() -> Self {
        Self {
            block_ids: TerrainBlockIds::default(),
            seed: DEFAULT_SEED,
            spawning_distance: DEFAULT_SPAWNING_DISTANCE,
        }
    }
}

impl RumpelVoxelWorld {
    #[must_use]
    pub fn from_registry(registry: &BlockRegistry) -> Self {
        Self {
            block_ids: TerrainBlockIds::from_registry(registry),
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
        ChunkDespawnStrategy::FarAwayOrOutOfView
    }

    fn chunk_spawn_strategy(&self) -> ChunkSpawnStrategy {
        ChunkSpawnStrategy::CloseAndInView
    }

    fn max_spawn_per_frame(&self) -> usize {
        DEFAULT_MAX_SPAWN_PER_FRAME
    }

    fn max_active_chunk_threads(&self) -> usize {
        DEFAULT_MAX_ACTIVE_CHUNK_THREADS
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

    fn texture_index_mapper(&self) -> Arc<dyn Fn(Self::MaterialIndex) -> [u32; 3] + Send + Sync> {
        Arc::new(|material| {
            let texture_index = u32::from(material % 4);
            [texture_index, texture_index, texture_index]
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

    match block_pos.y.cmp(&terrain_height) {
        std::cmp::Ordering::Greater => WorldVoxel::Air,
        std::cmp::Ordering::Equal => WorldVoxel::Solid(block_ids.grass),
        std::cmp::Ordering::Less if terrain_height - block_pos.y <= 3 => {
            WorldVoxel::Solid(block_ids.dirt)
        }
        std::cmp::Ordering::Less => WorldVoxel::Solid(block_ids.stone),
    }
}
