use bevy::{platform::collections::HashMap, prelude::*};
use rumpel_blocks::{BlockId, AIR_BLOCK_ID};

pub const CHUNK_SIZE: usize = 32;
pub const CHUNK_VOLUME: usize = CHUNK_SIZE * CHUNK_SIZE * CHUNK_SIZE;

/// A flat array of block IDs optimized for GPU StorageBuffers
#[derive(Clone, Component)]
pub struct ChunkData {
    pub blocks: Box<[BlockId; CHUNK_VOLUME]>,
}

impl Default for ChunkData {
    fn default() -> Self {
        Self {
            blocks: Box::new([AIR_BLOCK_ID; CHUNK_VOLUME]),
        }
    }
}

impl ChunkData {
    #[inline]
    pub fn get_index(x: usize, y: usize, z: usize) -> usize {
        x + y * CHUNK_SIZE + z * CHUNK_SIZE * CHUNK_SIZE
    }

    #[inline]
    pub fn get_block(&self, x: usize, y: usize, z: usize) -> BlockId {
        if x < CHUNK_SIZE && y < CHUNK_SIZE && z < CHUNK_SIZE {
            self.blocks[Self::get_index(x, y, z)]
        } else {
            AIR_BLOCK_ID
        }
    }

    #[inline]
    pub fn set_block(&mut self, x: usize, y: usize, z: usize, id: BlockId) {
        if x < CHUNK_SIZE && y < CHUNK_SIZE && z < CHUNK_SIZE {
            let index = Self::get_index(x, y, z);
            self.blocks[index] = id;
        }
    }
}

/// Tracks the loaded chunks and their entities
#[derive(Resource, Default)]
pub struct ChunkManager {
    pub loaded_chunks: HashMap<IVec3, Entity>,
}

impl ChunkManager {
    pub fn world_to_chunk_pos(world_pos: Vec3) -> IVec3 {
        IVec3::new(
            (world_pos.x / CHUNK_SIZE as f32).floor() as i32,
            (world_pos.y / CHUNK_SIZE as f32).floor() as i32,
            (world_pos.z / CHUNK_SIZE as f32).floor() as i32,
        )
    }

    pub fn chunk_to_world_pos(chunk_pos: IVec3) -> Vec3 {
        Vec3::new(
            (chunk_pos.x * CHUNK_SIZE as i32) as f32,
            (chunk_pos.y * CHUNK_SIZE as i32) as f32,
            (chunk_pos.z * CHUNK_SIZE as i32) as f32,
        )
    }
}
