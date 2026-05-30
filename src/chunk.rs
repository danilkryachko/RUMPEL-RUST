use crate::blocks::BlockId;

pub const CHUNK_SIZE: usize = 16;
pub const CHUNK_HEIGHT: usize = 256;

pub struct Chunk {
    // Array format: [x][y][z]
    // Box is used to avoid stack overflow since the chunk is large (16 * 256 * 16 * 2 bytes = 128 KB)
    pub blocks: Box<[[[BlockId; CHUNK_SIZE]; CHUNK_HEIGHT]; CHUNK_SIZE]>,
}

impl Chunk {
    pub fn new() -> Self {
        Self {
            blocks: Box::new([[[0; CHUNK_SIZE]; CHUNK_HEIGHT]; CHUNK_SIZE]),
        }
    }
    
    pub fn get_block(&self, x: usize, y: usize, z: usize) -> BlockId {
        if x >= CHUNK_SIZE || y >= CHUNK_HEIGHT || z >= CHUNK_SIZE {
            return 0; // Air outside bounds
        }
        self.blocks[x][y][z]
    }
    
    pub fn set_block(&mut self, x: usize, y: usize, z: usize, id: BlockId) {
        if x < CHUNK_SIZE && y < CHUNK_HEIGHT && z < CHUNK_SIZE {
            self.blocks[x][y][z] = id;
        }
    }
}
