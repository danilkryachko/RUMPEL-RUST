use bevy::platform::collections::HashMap;
use bevy::prelude::*;
use noise::Perlin;
use rumpel_blocks::{AIR_BLOCK_ID, BlockId, BlockRegistry};
use rumpel_coords::{ChunkPos, LocalBlockPos, WorldBlockPos};

use crate::chunk::{CHUNK_HEIGHT, CHUNK_SIZE, ChunkData, WorldBlockEdit, WorldEditStore};
use crate::chunk_gen_cache::cached_chunk;
use crate::world_gen::{
    WorldGenerationContext, terrain_block_at_surface_world, terrain_perlin,
};

struct CachedChunk {
    data: ChunkData,
    revision: u64,
}

/// Cached generated chunks for gameplay block queries and edits.
#[derive(Resource)]
pub struct WorldBlocks {
    cache: HashMap<ChunkPos, CachedChunk>,
    context: WorldGenerationContext,
    perlin: Perlin,
}

impl WorldBlocks {
    #[must_use]
    pub fn new(registry: &BlockRegistry) -> Self {
        Self {
            cache: HashMap::default(),
            context: WorldGenerationContext::from_registry(registry),
            perlin: terrain_perlin(),
        }
    }

    #[must_use]
    pub fn block_at_world(&mut self, world: IVec3, edit_store: &WorldEditStore) -> BlockId {
        if let Some((chunk_pos, local_pos)) = world_block_to_typed(world)
            && let Some(block) = edit_store.block_at(chunk_pos, local_pos)
        {
            return block;
        }

        if (0..CHUNK_HEIGHT as i32).contains(&world.y) {
            let chunk_pos = chunk_pos_from_world_xz(world.x, world.z);
            let local_x = usize::try_from(world.x.rem_euclid(CHUNK_SIZE as i32)).unwrap_or(0);
            let local_y = usize::try_from(world.y).unwrap_or(0);
            let local_z = usize::try_from(world.z.rem_euclid(CHUNK_SIZE as i32)).unwrap_or(0);
            let chunk = self.chunk_data(chunk_pos, edit_store);
            return chunk.get_block(local_x, local_y, local_z);
        }

        terrain_block_at_surface_world(
            world.x,
            usize::try_from(world.y).unwrap_or(0),
            world.z,
            self.context.palette,
            edit_store,
            &self.perlin,
        )
    }

    #[must_use]
    pub fn is_solid_at_world(
        &mut self,
        world: IVec3,
        edit_store: &WorldEditStore,
        registry: &BlockRegistry,
    ) -> bool {
        let block = self.block_at_world(world, edit_store);
        if block == AIR_BLOCK_ID {
            return false;
        }
        registry.get_block(block).is_some_and(|data| data.is_solid)
    }

    #[must_use]
    pub fn is_solid_at(
        &mut self,
        pos: WorldBlockPos,
        edit_store: &WorldEditStore,
        registry: &BlockRegistry,
    ) -> bool {
        self.is_solid_at_world(pos.position, edit_store, registry)
    }

    #[must_use]
    pub fn set_block_world(world: IVec3, block: BlockId) -> Option<WorldBlockEdit> {
        let (chunk_pos, local_pos) = world_block_to_typed(world)?;
        Some(WorldBlockEdit::new(chunk_pos, local_pos, block))
    }

    pub fn invalidate_chunk(&mut self, chunk_pos: ChunkPos) {
        crate::chunk_gen_cache::invalidate_cached_chunk(chunk_pos);
        self.cache.remove(&chunk_pos);
    }

    pub fn sync_chunk_to_single_chunk_extract(
        &mut self,
        chunk_pos: ChunkPos,
        edit_store: &WorldEditStore,
        blocks: &mut [u32; crate::chunk::CHUNK_VOLUME],
    ) {
        if chunk_pos.x != 0 || chunk_pos.z != 0 {
            return;
        }

        let chunk = self.chunk_data(chunk_pos, edit_store);
        for (index, block) in chunk.blocks.iter().enumerate() {
            blocks[index] = u32::from(*block);
        }
    }

    fn chunk_data(&mut self, chunk_pos: ChunkPos, edit_store: &WorldEditStore) -> &ChunkData {
        let revision = edit_store.chunk_revision(chunk_pos);
        let needs_refresh = self
            .cache
            .get(&chunk_pos)
            .is_none_or(|cached| cached.revision != revision);

        if needs_refresh {
            let mut generated = cached_chunk(chunk_pos, &self.context, edit_store);
            edit_store.apply_all_edits_to_chunk(chunk_pos, &mut generated.chunk);
            self.cache.insert(
                chunk_pos,
                CachedChunk {
                    data: generated.chunk,
                    revision,
                },
            );
        }

        &self
            .cache
            .get(&chunk_pos)
            .expect("chunk cache populated")
            .data
    }
}

impl FromWorld for WorldBlocks {
    fn from_world(world: &mut World) -> Self {
        let registry = world.resource::<BlockRegistry>();
        Self::new(registry)
    }
}

#[must_use]
pub fn chunk_pos_from_world_xz(world_x: i32, world_z: i32) -> ChunkPos {
    ChunkPos::new(
        world_x.div_euclid(CHUNK_SIZE as i32),
        world_z.div_euclid(CHUNK_SIZE as i32),
    )
}

#[must_use]
pub fn world_block_to_typed(world: IVec3) -> Option<(ChunkPos, LocalBlockPos)> {
    let chunk_pos = chunk_pos_from_world_xz(world.x, world.z);
    let local_x = u8::try_from(world.x.rem_euclid(CHUNK_SIZE as i32)).ok()?;
    let local_z = u8::try_from(world.z.rem_euclid(CHUNK_SIZE as i32)).ok()?;
    let local_y = u16::try_from(world.y).ok()?;
    Some((chunk_pos, LocalBlockPos::new(local_x, local_y, local_z)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rumpel_blocks::BlockRegistry;

    fn test_registry() -> Option<BlockRegistry> {
        let mut registry = BlockRegistry::empty();
        for (id, name) in [
            ("air", "Air"),
            ("grass", "Grass"),
            ("dirt", "Dirt"),
            ("stone", "Stone"),
        ] {
            registry.register_block(rumpel_blocks::BlockData {
                id: id.to_string(),
                name: name.to_string(),
                is_solid: id != "air",
                is_transparent: id == "air",
                color: (0.5, 0.5, 0.5, 1.0),
                gravity_affected: false,
                strength: 1.0,
                wind_animated: false,
            });
        }
        Some(registry)
    }

    #[test]
    fn world_block_to_typed_maps_positive_world_coords() {
        let world = IVec3::new(35, 4, 67);
        let (chunk_pos, local) = world_block_to_typed(world).expect("typed mapping");

        assert_eq!(chunk_pos, ChunkPos::new(1, 2));
        assert_eq!(local, LocalBlockPos::new(3, 4, 3));
    }

    #[test]
    fn world_block_to_typed_maps_negative_world_coords() {
        let world = IVec3::new(-1, 10, -33);
        let (chunk_pos, local) = world_block_to_typed(world).expect("typed mapping");

        assert_eq!(chunk_pos, ChunkPos::new(-1, -2));
        assert_eq!(local, LocalBlockPos::new(31, 10, 31));
    }

    #[test]
    fn edit_store_overrides_generated_chunk_block() {
        let Some(registry) = test_registry() else {
            return;
        };
        let mut world_blocks = WorldBlocks::new(&registry);
        let mut edit_store = WorldEditStore::default();
        let world = IVec3::new(5, 2, 7);
        let (chunk_pos, local) = world_block_to_typed(world).expect("typed mapping");

        let before = world_blocks.block_at_world(world, &edit_store);
        assert_ne!(before, AIR_BLOCK_ID);

        assert!(edit_store.apply_edit(WorldBlockEdit::new(chunk_pos, local, AIR_BLOCK_ID)));
        world_blocks.invalidate_chunk(chunk_pos);
        assert_eq!(
            world_blocks.block_at_world(world, &edit_store),
            AIR_BLOCK_ID
        );
    }

    #[test]
    fn set_block_world_returns_typed_edit() {
        let world = IVec3::new(64, 12, 96);
        let edit = WorldBlocks::set_block_world(world, 3).expect("edit");

        assert_eq!(edit.chunk_pos, ChunkPos::new(2, 3));
        assert_eq!(edit.local_pos, LocalBlockPos::new(0, 12, 0));
        assert_eq!(edit.block, 3);
    }
}
