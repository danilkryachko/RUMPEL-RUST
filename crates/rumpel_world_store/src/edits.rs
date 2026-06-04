use rumpel_coords::{ChunkPos, LocalBlockPos};
use rumpel_world::chunk::{WorldBlockEditKey, WorldEditStore};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredBlockEdit {
    pub chunk_x: i32,
    pub chunk_z: i32,
    pub local_x: u8,
    pub local_y: u16,
    pub local_z: u8,
    pub block: u16,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredEdits {
    pub generation: u64,
    pub edits: Vec<StoredBlockEdit>,
}

impl StoredEdits {
    #[must_use]
    pub fn from_store(store: &WorldEditStore) -> Self {
        Self {
            generation: store.generation(),
            edits: store
                .iter_edits()
                .map(|(key, block)| StoredBlockEdit {
                    chunk_x: key.chunk_pos.x,
                    chunk_z: key.chunk_pos.z,
                    local_x: key.local_pos.x,
                    local_y: key.local_pos.y,
                    local_z: key.local_pos.z,
                    block: *block,
                })
                .collect(),
        }
    }

    pub fn apply_to_store(&self, store: &mut WorldEditStore) {
        let edits = self
            .edits
            .iter()
            .filter_map(|stored| {
                let local = LocalBlockPos::new(stored.local_x, stored.local_y, stored.local_z);
                let key = WorldBlockEditKey::new(ChunkPos::new(stored.chunk_x, stored.chunk_z), local)?;
                Some((key, stored.block))
            })
            .collect();
        store.restore_edits(edits, self.generation);
    }
}
