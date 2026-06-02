use bevy::{platform::collections::HashMap, prelude::*};
use rumpel_blocks::{AIR_BLOCK_ID, BlockId};
use rumpel_coords::{ChunkPos, LocalBlockPos};
use std::mem::size_of;

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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PaletteRleChunk {
    palette: Vec<BlockId>,
    runs: Vec<PaletteRleRun>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PaletteRleRun {
    pub palette_index: u16,
    pub len: u16,
}

impl PaletteRleChunk {
    #[must_use]
    pub fn from_blocks(blocks: &[BlockId; CHUNK_VOLUME]) -> Self {
        let mut palette = Vec::<BlockId>::new();
        let mut block_to_palette = HashMap::<BlockId, u16>::default();
        let mut runs = Vec::<PaletteRleRun>::new();

        for &block in blocks {
            let palette_index = if let Some(&index) = block_to_palette.get(&block) {
                index
            } else {
                let index = u16::try_from(palette.len()).unwrap_or(u16::MAX);
                palette.push(block);
                block_to_palette.insert(block, index);
                index
            };

            if let Some(last_run) = runs.last_mut()
                && last_run.palette_index == palette_index
                && last_run.len < u16::MAX
            {
                last_run.len += 1;
                continue;
            }

            runs.push(PaletteRleRun {
                palette_index,
                len: 1,
            });
        }

        Self { palette, runs }
    }

    #[must_use]
    pub fn to_blocks(&self) -> Box<[BlockId; CHUNK_VOLUME]> {
        let mut blocks = Box::new([AIR_BLOCK_ID; CHUNK_VOLUME]);
        self.write_blocks(&mut blocks);
        blocks
    }

    pub fn write_blocks(&self, blocks: &mut [BlockId; CHUNK_VOLUME]) {
        let mut cursor = 0;
        for run in &self.runs {
            let block = self
                .palette
                .get(usize::from(run.palette_index))
                .copied()
                .unwrap_or(AIR_BLOCK_ID);
            let end = (cursor + usize::from(run.len)).min(CHUNK_VOLUME);
            blocks[cursor..end].fill(block);
            cursor = end;
            if cursor >= CHUNK_VOLUME {
                break;
            }
        }
        if cursor < CHUNK_VOLUME {
            blocks[cursor..].fill(AIR_BLOCK_ID);
        }
    }

    #[must_use]
    pub fn palette_len(&self) -> usize {
        self.palette.len()
    }

    #[must_use]
    pub fn run_count(&self) -> usize {
        self.runs.len()
    }

    #[must_use]
    pub fn estimated_bytes(&self) -> usize {
        self.palette.len() * size_of::<BlockId>() + self.runs.len() * size_of::<PaletteRleRun>()
    }
}

/// Tracks the loaded chunks and their entities
#[derive(Resource, Default)]
pub struct ChunkManager {
    pub loaded_chunks: HashMap<IVec3, Entity>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Message)]
pub struct WorldBlockEdit {
    pub chunk_pos: ChunkPos,
    pub local_pos: LocalBlockPos,
    pub block: BlockId,
}

impl WorldBlockEdit {
    #[must_use]
    pub fn new(chunk_pos: ChunkPos, local_pos: LocalBlockPos, block: BlockId) -> Self {
        Self {
            chunk_pos,
            local_pos,
            block,
        }
    }

    #[must_use]
    pub fn from_single_chunk_index(index: usize, block: BlockId) -> Option<Self> {
        if index >= CHUNK_VOLUME {
            return None;
        }

        let layer_size = CHUNK_SIZE * CHUNK_SIZE;
        let z = index / layer_size;
        let layer_index = index % layer_size;
        let y = layer_index / CHUNK_SIZE;
        let x = layer_index % CHUNK_SIZE;

        Some(Self::new(
            ChunkPos { x: 0, z: 0 },
            LocalBlockPos::new(
                u8::try_from(x).ok()?,
                u16::try_from(y).ok()?,
                u8::try_from(z).ok()?,
            ),
            block,
        ))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct WorldBlockEditKey {
    pub chunk_pos: ChunkPos,
    pub local_pos: LocalBlockPos,
}

impl WorldBlockEditKey {
    #[must_use]
    pub fn new(chunk_pos: ChunkPos, local_pos: LocalBlockPos) -> Option<Self> {
        if usize::from(local_pos.x) >= CHUNK_SIZE || usize::from(local_pos.z) >= CHUNK_SIZE {
            return None;
        }

        Some(Self {
            chunk_pos,
            local_pos,
        })
    }

    #[must_use]
    pub fn from_edit(edit: WorldBlockEdit) -> Option<Self> {
        Self::new(edit.chunk_pos, edit.local_pos)
    }
}

#[derive(Resource, Default)]
pub struct WorldEditStore {
    edits: HashMap<WorldBlockEditKey, BlockId>,
    generation: u64,
}

impl WorldEditStore {
    pub fn apply_edit(&mut self, edit: WorldBlockEdit) -> bool {
        let Some(key) = WorldBlockEditKey::from_edit(edit) else {
            return false;
        };

        if self
            .edits
            .get(&key)
            .is_some_and(|block| *block == edit.block)
        {
            return false;
        }

        self.edits.insert(key, edit.block);
        self.generation = self.generation.wrapping_add(1);
        true
    }

    #[must_use]
    pub fn block_at(&self, chunk_pos: ChunkPos, local_pos: LocalBlockPos) -> Option<BlockId> {
        let key = WorldBlockEditKey::new(chunk_pos, local_pos)?;
        self.edits.get(&key).copied()
    }

    #[must_use]
    pub fn generation(&self) -> u64 {
        self.generation
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.edits.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.edits.is_empty()
    }

    pub fn apply_to_chunk_layer(
        &self,
        chunk_pos: ChunkPos,
        y_base: i32,
        chunk: &mut ChunkData,
    ) -> usize {
        let y_end = y_base + CHUNK_SIZE as i32;
        let mut applied = 0;

        for (&key, &block) in &self.edits {
            if key.chunk_pos != chunk_pos {
                continue;
            }

            let world_y = i32::from(key.local_pos.y);
            if world_y < y_base || world_y >= y_end {
                continue;
            }

            let Ok(y) = usize::try_from(world_y - y_base) else {
                continue;
            };
            let x = usize::from(key.local_pos.x);
            let z = usize::from(key.local_pos.z);
            chunk.set_block(x, y, z, block);
            applied += 1;
        }

        applied
    }
}

pub fn record_world_block_edits(
    mut edits: MessageReader<WorldBlockEdit>,
    mut store: ResMut<WorldEditStore>,
) {
    let mut stored_edits = 0;
    let mut ignored_edits = 0;

    for edit in edits.read().copied() {
        if store.apply_edit(edit) {
            stored_edits += 1;
        } else {
            ignored_edits += 1;
        }
    }

    if stored_edits > 0 || ignored_edits > 0 {
        info!(
            stored_edits,
            ignored_edits,
            store_generation = store.generation(),
            store_edits = store.len(),
            "world block edits stored"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn palette_rle_roundtrips_dense_chunk() {
        let mut blocks = [AIR_BLOCK_ID; CHUNK_VOLUME];
        for z in 0..CHUNK_SIZE {
            for x in 0..CHUNK_SIZE {
                let height = 4 + (x % 3) + (z % 5);
                for y in 0..height {
                    blocks[ChunkData::get_index(x, y, z)] = if y + 1 == height { 2 } else { 1 };
                }
            }
        }

        let encoded = PaletteRleChunk::from_blocks(&blocks);

        assert_eq!(&*encoded.to_blocks(), &blocks);
        assert_eq!(encoded.palette_len(), 3);
    }

    #[test]
    fn palette_rle_compresses_uniform_air() {
        let blocks = [AIR_BLOCK_ID; CHUNK_VOLUME];
        let encoded = PaletteRleChunk::from_blocks(&blocks);

        assert_eq!(encoded.palette_len(), 1);
        assert_eq!(encoded.run_count(), 1);
        assert!(encoded.estimated_bytes() < CHUNK_VOLUME * size_of::<BlockId>());
    }

    #[test]
    fn world_block_edit_maps_single_chunk_index_to_typed_positions() {
        let index = ChunkData::get_index(3, 4, 5);
        let edit = WorldBlockEdit::from_single_chunk_index(index, 7).expect("valid index");

        assert_eq!(edit.chunk_pos, ChunkPos { x: 0, z: 0 });
        assert_eq!(edit.local_pos, LocalBlockPos::new(3, 4, 5));
        assert_eq!(edit.block, 7);
    }

    #[test]
    fn world_block_edit_rejects_out_of_range_single_chunk_index() {
        assert!(WorldBlockEdit::from_single_chunk_index(CHUNK_VOLUME, 7).is_none());
    }

    #[test]
    fn world_edit_store_records_latest_block_and_generation() {
        let pos = LocalBlockPos::new(3, 4, 5);
        let mut store = WorldEditStore::default();
        let first = WorldBlockEdit::new(ChunkPos { x: 0, z: 0 }, pos, 3);
        let second = WorldBlockEdit::new(ChunkPos { x: 0, z: 0 }, pos, 7);

        assert!(store.apply_edit(first));
        assert_eq!(store.generation(), 1);
        assert_eq!(store.block_at(first.chunk_pos, pos), Some(3));
        assert!(!store.apply_edit(first));
        assert_eq!(store.generation(), 1);

        assert!(store.apply_edit(second));
        assert_eq!(store.generation(), 2);
        assert_eq!(store.len(), 1);
        assert_eq!(store.block_at(second.chunk_pos, pos), Some(7));
    }

    #[test]
    fn world_edit_store_applies_only_matching_chunk_layer_edits() {
        let mut store = WorldEditStore::default();
        assert!(store.apply_edit(WorldBlockEdit::new(
            ChunkPos { x: 0, z: 0 },
            LocalBlockPos::new(1, 2, 3),
            4,
        )));
        assert!(store.apply_edit(WorldBlockEdit::new(
            ChunkPos { x: 0, z: 0 },
            LocalBlockPos::new(5, 40, 6),
            8,
        )));
        assert!(store.apply_edit(WorldBlockEdit::new(
            ChunkPos { x: 1, z: 0 },
            LocalBlockPos::new(1, 2, 3),
            9,
        )));
        let mut chunk = ChunkData::default();

        let applied = store.apply_to_chunk_layer(ChunkPos { x: 0, z: 0 }, 32, &mut chunk);

        assert_eq!(applied, 1);
        assert_eq!(chunk.get_block(5, 8, 6), 8);
        assert_eq!(chunk.get_block(1, 2, 3), AIR_BLOCK_ID);
    }

    #[test]
    fn record_world_block_edits_persists_messages_into_store() {
        let mut app = App::new();
        app.add_message::<WorldBlockEdit>();
        app.init_resource::<WorldEditStore>();
        app.add_systems(Update, record_world_block_edits);
        let edit = WorldBlockEdit::new(ChunkPos { x: -1, z: 2 }, LocalBlockPos::new(7, 8, 9), 11);

        app.world_mut()
            .resource_mut::<bevy::ecs::message::Messages<WorldBlockEdit>>()
            .write(edit);
        app.update();

        let store = app.world().resource::<WorldEditStore>();
        assert_eq!(store.generation(), 1);
        assert_eq!(store.block_at(edit.chunk_pos, edit.local_pos), Some(11));
    }
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

#[derive(Resource, Clone, bevy::render::extract_resource::ExtractResource)]
pub struct SingleChunkExtract {
    pub blocks: Box<[u32; 32768]>, // WGSL expects array<u32>
    pub has_changes: bool,
}

impl Default for SingleChunkExtract {
    fn default() -> Self {
        Self {
            blocks: Box::new([0; 32768]),
            has_changes: false,
        }
    }
}
