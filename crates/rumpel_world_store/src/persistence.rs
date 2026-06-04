use std::sync::Arc;

use rumpel_blocks::BlockRegistry;
use rumpel_coords::ChunkPos;
use rumpel_world::chunk::WorldEditStore;
use rumpel_world::chunk_disk::ChunkPersistence;
use rumpel_world::world_gen::{GeneratedChunk, WorldGenerationContext};

use crate::database::WorldDatabase;

pub struct DatabaseChunkPersistence {
    database: Arc<WorldDatabase>,
    contract_version: u64,
}

impl DatabaseChunkPersistence {
    #[must_use]
    pub fn new(database: Arc<WorldDatabase>, contract_version: u64) -> Self {
        Self {
            database,
            contract_version,
        }
    }

    #[must_use]
    pub fn into_arc(self) -> Arc<dyn ChunkPersistence> {
        Arc::new(self)
    }
}

impl ChunkPersistence for DatabaseChunkPersistence {
    fn contract_version(&self) -> u64 {
        self.contract_version
    }

    fn load_generated_chunk(
        &self,
        pos: ChunkPos,
        context: &WorldGenerationContext,
        edit_store: &WorldEditStore,
    ) -> Option<GeneratedChunk> {
        self.database
            .load_generated_chunk(pos, self.contract_version, context, edit_store)
    }

    fn materialize_and_persist_chunk(
        &self,
        pos: ChunkPos,
        context: &WorldGenerationContext,
        edit_store: &WorldEditStore,
        registry: &BlockRegistry,
    ) -> Result<(), String> {
        self.database
            .materialize_and_persist_chunk(
                pos,
                self.contract_version,
                context,
                edit_store,
                registry,
            )
            .map_err(|error| error.to_string())
    }
}
