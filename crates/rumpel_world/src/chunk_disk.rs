use std::collections::HashSet;
use std::sync::{Arc, Mutex, OnceLock};

use rumpel_blocks::BlockRegistry;
use rumpel_coords::ChunkPos;

use crate::chunk::WorldEditStore;
use crate::world_gen::{GeneratedChunk, WorldGenerationContext};

const MAX_PENDING_PERSIST_PER_FLUSH: usize = 64;

pub trait ChunkPersistence: Send + Sync {
    fn contract_version(&self) -> u64;
    fn load_generated_chunk(
        &self,
        pos: ChunkPos,
        context: &WorldGenerationContext,
        edit_store: &WorldEditStore,
    ) -> Option<GeneratedChunk>;
    fn materialize_and_persist_chunk(
        &self,
        pos: ChunkPos,
        context: &WorldGenerationContext,
        edit_store: &WorldEditStore,
        registry: &BlockRegistry,
    ) -> Result<(), String>;
}

struct ChunkDiskState {
    persistence: Arc<dyn ChunkPersistence>,
    pending: HashSet<ChunkPos>,
}

static CHUNK_DISK: OnceLock<Mutex<Option<ChunkDiskState>>> = OnceLock::new();

fn chunk_disk_state() -> &'static Mutex<Option<ChunkDiskState>> {
    CHUNK_DISK.get_or_init(|| Mutex::new(None))
}

pub fn install_chunk_persistence(persistence: Arc<dyn ChunkPersistence>) {
    let mut slot = chunk_disk_state()
        .lock()
        .expect("chunk disk mutex poisoned");
    *slot = Some(ChunkDiskState {
        persistence,
        pending: HashSet::default(),
    });
}

pub fn clear_chunk_persistence() {
    let mut slot = chunk_disk_state()
        .lock()
        .expect("chunk disk mutex poisoned");
    *slot = None;
}

pub fn mark_chunk_pending(pos: ChunkPos) {
    let mut slot = chunk_disk_state()
        .lock()
        .expect("chunk disk mutex poisoned");
    if let Some(state) = slot.as_mut() {
        state.pending.insert(pos);
    }
}

pub fn try_load_generated_chunk(
    pos: ChunkPos,
    context: &WorldGenerationContext,
    edit_store: &WorldEditStore,
) -> Option<GeneratedChunk> {
    let slot = chunk_disk_state().lock().ok()?;
    let state = slot.as_ref()?;
    state
        .persistence
        .load_generated_chunk(pos, context, edit_store)
}

pub fn flush_pending_chunks(
    context: &WorldGenerationContext,
    edit_store: &WorldEditStore,
    registry: &BlockRegistry,
) -> usize {
    let mut total = 0usize;
    loop {
        let flushed = flush_pending_chunk_batch(context, edit_store, registry);
        if flushed == 0 {
            break;
        }
        total += flushed;
    }
    total
}

fn flush_pending_chunk_batch(
    context: &WorldGenerationContext,
    edit_store: &WorldEditStore,
    registry: &BlockRegistry,
) -> usize {
    let mut slot = chunk_disk_state()
        .lock()
        .expect("chunk disk mutex poisoned");
    let Some(state) = slot.as_mut() else {
        return 0;
    };

    let pending: Vec<ChunkPos> = state
        .pending
        .iter()
        .copied()
        .take(MAX_PENDING_PERSIST_PER_FLUSH)
        .collect();
    if pending.is_empty() {
        return 0;
    }

    let mut persisted = 0;
    for pos in pending {
        let result = state
            .persistence
            .materialize_and_persist_chunk(pos, context, edit_store, registry);
        if result.is_ok() {
            state.pending.remove(&pos);
            persisted += 1;
        }
    }
    persisted
}

pub fn pending_chunk_count() -> usize {
    chunk_disk_state()
        .lock()
        .ok()
        .and_then(|slot| slot.as_ref().map(|state| state.pending.len()))
        .unwrap_or(0)
}

pub fn enqueue_edited_chunks(edit_store: &WorldEditStore) {
    let mut slot = chunk_disk_state()
        .lock()
        .expect("chunk disk mutex poisoned");
    let Some(state) = slot.as_mut() else {
        return;
    };
    for (key, _) in edit_store.iter_edits() {
        state.pending.insert(key.chunk_pos);
    }
}
