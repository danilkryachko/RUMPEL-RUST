use rumpel_coords::ChunkPos;
use rumpel_world::chunk::{ChunkData, PaletteRleChunk, PaletteRleRun};
use rumpel_world::surface_decor::{ChunkDecorOutput, DecorInstance};
use serde::{Deserialize, Serialize};

pub const STORED_CHUNK_FORMAT_VERSION: u32 = 2;
pub const STORED_CHUNK_FORMAT_VERSION_V1: u32 = 1;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredPaletteRun {
    pub palette_index: u16,
    pub len: u16,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredPaletteRle {
    pub palette: Vec<u16>,
    pub runs: Vec<StoredPaletteRun>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StoredDecorInstance {
    pub translation: [f32; 3],
    pub rotation_y: f32,
    pub scale: [f32; 3],
    pub custom: [f32; 4],
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct StoredChunkDecor {
    pub grass: Vec<StoredDecorInstance>,
    pub leaves: Vec<StoredDecorInstance>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StoredChunkBlob {
    pub format_version: u32,
    pub contract_version: u64,
    pub chunk_x: i32,
    pub chunk_z: i32,
    pub blocks: StoredPaletteRle,
    #[serde(default)]
    pub decor: StoredChunkDecor,
}

impl StoredChunkBlob {
    #[must_use]
    pub fn from_chunk_data(pos: ChunkPos, contract_version: u64, chunk: &ChunkData) -> Self {
        Self::from_generated(pos, contract_version, chunk, &ChunkDecorOutput::default())
    }

    #[must_use]
    pub fn from_generated(
        pos: ChunkPos,
        contract_version: u64,
        chunk: &ChunkData,
        decor: &ChunkDecorOutput,
    ) -> Self {
        let encoded = PaletteRleChunk::from_blocks(&chunk.blocks);
        Self {
            format_version: STORED_CHUNK_FORMAT_VERSION,
            contract_version,
            chunk_x: pos.x,
            chunk_z: pos.z,
            blocks: StoredPaletteRle::from_palette_rle(&encoded),
            decor: StoredChunkDecor::from_decor_output(decor),
        }
    }

    #[must_use]
    pub fn to_decor_output(&self) -> ChunkDecorOutput {
        self.decor.to_decor_output()
    }

    #[must_use]
    pub fn to_chunk_data(&self) -> ChunkData {
        let blocks = stored_palette_to_rle(&self.blocks).to_blocks();
        ChunkData { blocks }
    }
}

impl StoredChunkDecor {
    #[must_use]
    pub fn from_decor_output(decor: &ChunkDecorOutput) -> Self {
        Self {
            grass: decor.grass.iter().map(StoredDecorInstance::from).collect(),
            leaves: decor.leaves.iter().map(StoredDecorInstance::from).collect(),
        }
    }

    #[must_use]
    pub fn to_decor_output(&self) -> ChunkDecorOutput {
        ChunkDecorOutput {
            grass: self.grass.iter().map(DecorInstance::from).collect(),
            leaves: self.leaves.iter().map(DecorInstance::from).collect(),
        }
    }
}

impl From<&DecorInstance> for StoredDecorInstance {
    fn from(instance: &DecorInstance) -> Self {
        Self {
            translation: instance.translation,
            rotation_y: instance.rotation_y,
            scale: instance.scale,
            custom: instance.custom,
        }
    }
}

impl From<&StoredDecorInstance> for DecorInstance {
    fn from(instance: &StoredDecorInstance) -> Self {
        Self {
            translation: instance.translation,
            rotation_y: instance.rotation_y,
            scale: instance.scale,
            custom: instance.custom,
        }
    }
}

impl StoredPaletteRle {
    #[must_use]
    pub fn from_palette_rle(encoded: &PaletteRleChunk) -> Self {
        Self {
            palette: encoded.palette_blocks().to_vec(),
            runs: encoded
                .runs_slice()
                .iter()
                .map(|run| StoredPaletteRun {
                    palette_index: run.palette_index,
                    len: run.len,
                })
                .collect(),
        }
    }
}

fn stored_palette_to_rle(stored: &StoredPaletteRle) -> PaletteRleChunk {
    PaletteRleChunk::from_palette_runs(
        stored.palette.clone(),
        stored
            .runs
            .iter()
            .map(|run| PaletteRleRun {
                palette_index: run.palette_index,
                len: run.len,
            })
            .collect(),
    )
}

pub const CHUNK_STORAGE_KEY_PREFIX: &[u8] = b"chunk:v1:";

#[must_use]
pub fn chunk_storage_key(pos: ChunkPos) -> Vec<u8> {
    format!(
        "chunk:v1:{}:{}",
        pos.x, pos.z
    )
    .into_bytes()
}

#[must_use]
pub fn chunk_blob_format_supported(format_version: u32) -> bool {
    format_version == STORED_CHUNK_FORMAT_VERSION
        || format_version == STORED_CHUNK_FORMAT_VERSION_V1
}

#[must_use]
pub fn chunk_blob_matches_contract(blob: &StoredChunkBlob, expected_contract: u64) -> bool {
    blob.contract_version == expected_contract
}
