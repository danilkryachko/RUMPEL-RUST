use rumpel_blocks::BlockId;
use serde::{Deserialize, Serialize};

pub const CHUNK_SIZE: usize = 16;
pub const CHUNK_HEIGHT: usize = 256;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RleEntry {
    pub count: u32,
    pub block_id: BlockId,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RleChunk {
    pub runs: Vec<RleEntry>,
}

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

    pub fn to_rle(&self) -> RleChunk {
        let mut runs = Vec::new();
        let mut current_id = self.blocks[0][0][0];
        let mut current_count = 0u32;

        for x in 0..CHUNK_SIZE {
            for y in 0..CHUNK_HEIGHT {
                for z in 0..CHUNK_SIZE {
                    let id = self.blocks[x][y][z];
                    if id == current_id {
                        current_count += 1;
                    } else {
                        runs.push(RleEntry {
                            count: current_count,
                            block_id: current_id,
                        });
                        current_id = id;
                        current_count = 1;
                    }
                }
            }
        }

        if current_count > 0 {
            runs.push(RleEntry {
                count: current_count,
                block_id: current_id,
            });
        }

        RleChunk { runs }
    }

    pub fn from_rle(rle: &RleChunk) -> Self {
        let mut chunk = Self::new();
        let mut idx = 0usize;

        for run in &rle.runs {
            for _ in 0..run.count {
                if idx >= CHUNK_SIZE * CHUNK_HEIGHT * CHUNK_SIZE {
                    break;
                }
                let x = idx / (CHUNK_HEIGHT * CHUNK_SIZE);
                let rem = idx % (CHUNK_HEIGHT * CHUNK_SIZE);
                let y = rem / CHUNK_SIZE;
                let z = rem % CHUNK_SIZE;

                chunk.blocks[x][y][z] = run.block_id;
                idx += 1;
            }
        }

        chunk
    }
}

impl Default for Chunk {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chunk_rle_round_trip() {
        let mut original = Chunk::new();

        // Fill some blocks
        for x in 0..CHUNK_SIZE {
            for z in 0..CHUNK_SIZE {
                original.set_block(x, 10, z, 1); // Dirt
                original.set_block(x, 11, z, 2); // Grass
            }
        }

        let rle = original.to_rle();
        let decompressed = Chunk::from_rle(&rle);

        // Verify matches exactly
        for x in 0..CHUNK_SIZE {
            for y in 0..CHUNK_HEIGHT {
                for z in 0..CHUNK_SIZE {
                    assert_eq!(decompressed.get_block(x, y, z), original.get_block(x, y, z));
                }
            }
        }
    }

    #[test]
    fn test_rle_compression_ratio() {
        let mut original = Chunk::new();

        // A typical chunk with stone under grass
        for x in 0..CHUNK_SIZE {
            for z in 0..CHUNK_SIZE {
                for y in 0..10 {
                    original.set_block(x, y, z, 3); // Stone
                }
                original.set_block(x, 10, z, 2); // Grass
            }
        }

        let rle = original.to_rle();

        // Raw chunk has 16*256*16 = 65,536 voxels (65,536 * 2 bytes = 131,072 bytes)
        // Rle representation will have very few runs (around 32 runs)
        assert!(rle.runs.len() < 50);
    }
}
