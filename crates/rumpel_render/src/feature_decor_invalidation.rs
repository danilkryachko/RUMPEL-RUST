//! Edit-driven invalidation for feature overlay and surface decor streamed chunks.
//!
//! When the player edits blocks, both the feature overlay mesh and the decor billboard mesh
//! for the affected chunk become stale. This module provides the shared logic to detect
//! which loaded or building chunks have been dirtied by `WorldBlockEdit`s and should be
//! re-queued for rebuild.

use bevy::{platform::collections::HashMap, prelude::*};
use rumpel_prelude::ChunkPos;
use rumpel_world::chunk::WorldEditStore;

/// Collect the XZ chunk positions (as `IVec2`) from `loaded` and `building` that have a
/// recorded `chunk_revision` newer than `old_generation` in the edit store.
///
/// Results are deduplicated; a position present in both maps is returned only once.
#[must_use]
pub(crate) fn dirty_layer_chunks_since(
    edit_store: &WorldEditStore,
    old_generation: u64,
    loaded: &HashMap<IVec2, Entity>,
    building: &HashMap<IVec2, Entity>,
) -> Vec<IVec2> {
    let mut seen = HashMap::<IVec2, ()>::default();
    let mut dirty = Vec::new();

    for pos in loaded.keys().chain(building.keys()) {
        if seen.insert(*pos, ()).is_some() {
            continue;
        }
        let chunk_pos = ChunkPos::new(pos.x, pos.y);
        if edit_store.chunk_revision(chunk_pos) > old_generation {
            dirty.push(*pos);
        }
    }

    dirty
}

#[cfg(test)]
mod tests {
    use super::*;
    use rumpel_blocks::BlockId;
    use rumpel_prelude::LocalBlockPos;
    use rumpel_world::chunk::WorldBlockEdit;

    #[test]
    fn dirty_layer_chunks_since_returns_edited_chunks() {
        let mut edit_store = WorldEditStore::default();
        let chunk_a = ChunkPos::new(1, 0);
        let chunk_b = ChunkPos::new(2, 0);

        let edit = WorldBlockEdit::new(
            chunk_a,
            LocalBlockPos::new(0, 1, 0),
            BlockId::from(3u8),
        );
        assert!(edit_store.apply_edit(edit));

        let gen_after = edit_store.generation();

        let mut loaded: HashMap<IVec2, Entity> = HashMap::default();
        let mut building: HashMap<IVec2, Entity> = HashMap::default();
        loaded.insert(IVec2::new(chunk_a.x, chunk_a.z), Entity::PLACEHOLDER);
        building.insert(IVec2::new(chunk_b.x, chunk_b.z), Entity::PLACEHOLDER);

        let dirty = dirty_layer_chunks_since(&edit_store, 0, &loaded, &building);
        assert!(
            dirty.contains(&IVec2::new(chunk_a.x, chunk_a.z)),
            "chunk_a should be dirty"
        );
        assert!(
            !dirty.contains(&IVec2::new(chunk_b.x, chunk_b.z)),
            "chunk_b has no edits, should not be dirty"
        );

        let dirty_none = dirty_layer_chunks_since(&edit_store, gen_after, &loaded, &building);
        assert!(dirty_none.is_empty(), "nothing new since gen_after");
    }

    #[test]
    fn dirty_layer_chunks_since_deduplicates_loaded_and_building() {
        let mut edit_store = WorldEditStore::default();
        let chunk_a = ChunkPos::new(3, 0);
        let edit = WorldBlockEdit::new(chunk_a, LocalBlockPos::new(0, 1, 0), BlockId::from(1u8));
        assert!(edit_store.apply_edit(edit));

        let pos = IVec2::new(chunk_a.x, chunk_a.z);
        let mut loaded: HashMap<IVec2, Entity> = HashMap::default();
        let mut building: HashMap<IVec2, Entity> = HashMap::default();
        loaded.insert(pos, Entity::PLACEHOLDER);
        building.insert(pos, Entity::PLACEHOLDER);

        let dirty = dirty_layer_chunks_since(&edit_store, 0, &loaded, &building);
        assert_eq!(dirty.len(), 1, "duplicate pos should appear only once");
    }
}
