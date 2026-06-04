use bevy::platform::collections::HashMap;
use rumpel_coords::ChunkPos;
use std::sync::{Mutex, OnceLock};

use crate::world_gen::{
    GeneratedChunk, WorldGenerationContext, generate_chunk_uncached,
    terrain_generation_contract_version,
};

const MAX_CACHED_CHUNKS: usize = 512;

struct ChunkGenCacheState {
    contract: u64,
    entries: HashMap<ChunkPos, GeneratedChunk>,
}

impl ChunkGenCacheState {
    fn new() -> Self {
        Self {
            contract: 0,
            entries: HashMap::default(),
        }
    }

    fn sync_contract(&mut self) {
        let contract = terrain_generation_contract_version();
        if self.contract != contract {
            self.entries.clear();
            self.contract = contract;
        }
    }
}

fn cache_state() -> &'static Mutex<ChunkGenCacheState> {
    static CACHE: OnceLock<Mutex<ChunkGenCacheState>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(ChunkGenCacheState::new()))
}

#[must_use]
pub fn cached_chunk(pos: ChunkPos, context: &WorldGenerationContext) -> GeneratedChunk {
    let mut cache = cache_state()
        .lock()
        .expect("chunk generation cache mutex poisoned");
    cache.sync_contract();
    if let Some(entry) = cache.entries.get(&pos) {
        return entry.clone();
    }
    if cache.entries.len() >= MAX_CACHED_CHUNKS {
        cache.entries.clear();
    }
    let generated = generate_chunk_uncached(pos, context);
    cache.entries.insert(pos, generated.clone());
    generated
}

pub fn invalidate_cached_chunk(pos: ChunkPos) {
    let mut cache = cache_state()
        .lock()
        .expect("chunk generation cache mutex poisoned");
    cache.entries.remove(&pos);
}

pub fn reset_chunk_generation_cache() {
    let mut cache = cache_state()
        .lock()
        .expect("chunk generation cache mutex poisoned");
    cache.entries.clear();
    cache.contract = 0;
}

#[cfg(test)]
pub fn clear_chunk_generation_cache_for_tests() {
    reset_chunk_generation_cache();
}
