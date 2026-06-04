# ADR-004 — Feature/Decor Layer Architecture

**Status:** Accepted  
**Date:** 2026-06-04  
**Author:** engine-architect

---

## Context

The packed terrain renderer uses three passes:

1. **Base shell** — packed CPU or GPU-generated heightmap columns (terrain blocks only,
   driven by `PackedQuadPipelinePlugin`).
2. **Feature overlay** (`packed_feature_overlay` + `terrain_feature_overlay`) — textured
   voxel meshes for Lua-placed trees and structures; computed as the *diff* between the full
   `cached_chunk` output and the terrain shell blocks (grass / dirt / stone / sand).
3. **Decor billboards** (`surface_decor`) — grass/leaf cross-quads from Lua `decor.lua` via
   `GeneratedChunk.decor`.

Prior to this ADR, the overlay used `terrain_height_with_noise` (analytical Rust noise) as its
procedural baseline, and neither overlay nor decor invalidated on `WorldBlockEdit`. This caused
stale meshes after player block edits.

---

## Decision

### 1. Keep separate render passes

Trees / Lua structures are NOT merged into the GPU heightmap. Reasons:
- The GPU heightmap is a surface-column representation; merging arbitrary voxel shapes requires
  full 3-D voxel storage and a different shader path.
- The decor billboard pass is already disjoint and performant for density-limited vegetation.
- Overlay and decor meshes are sparse compared to the terrain shell, so a separate draw call
  per chunk is acceptable.

### 2. Unified chunk source via `chunk_gen_cache::cached_chunk`

All three passes now derive terrain data from **the same source**:

```
cached_chunk(pos, &world_gen_ctx)  →  GeneratedChunk { chunk, decor }
                                    ↓
              WorldEditStore::apply_all_edits_to_chunk
                                    ↓
                     pass-specific mesh builder
```

- `terrain_feature_overlay::build_lua_feature_mesh_for_chunk` calls `cached_chunk` directly
  and uses `is_terrain_shell_block(block, palette, sand)` to identify the procedural baseline
  (same filter as `terrain_surface_cell_sample_from_world_cached`). The former analytical
  `terrain_height_with_noise` / `BiomeRegistry` call is removed.
- `surface_decor::build_chunk_decor_meshes` calls `cached_chunk`, applies edits to a mutable
  chunk copy, then passes the edited chunk to `resolve_chunk_decor`.

### 3. Edit invalidation

Both `PackedFeatureOverlayState` and `SurfaceDecorState` track `last_seen_edit_generation`.
Each frame, after `record_world_block_edits` runs, the respective invalidation systems:

1. Compare `WorldEditStore::generation()` against the stored value.
2. For every loaded or building chunk whose `chunk_revision` exceeds the old generation,
   despawn the entity and re-queue the chunk into `pending`.
3. Update the stored generation.

The procedural generation cache (`chunk_gen_cache`) does **not** need per-edit invalidation
because edits are applied on top of the cached procedural output at mesh-build time.
Contract-version invalidation (Lua script hash) is already handled by the cache.

### 4. Legacy render modes

`RumpelRenderMode` retains `Surface` and `ComputePrototype` variants for A/B testing.
`legacy_render_modes::add_legacy_render_plugins` wires up the legacy `SurfaceStreamingPlugin`
/ `VoxelComputePlugin` when those modes are explicitly requested via `RUMPEL_RENDER_MODE`.

| `RUMPEL_RENDER_MODE` value | Activated plugins |
|---|---|
| `packed` (default / unset) | `PackedQuadPipelinePlugin` + feature overlay + decor |
| `surface` | `SurfaceStreamingPlugin` + `SurfaceDecorPlugin` |
| `compute` | `VoxelComputePlugin` |

`surface_streaming` and `voxel_compute` modules are retained but not loaded by default.

---

## Consequences

- Overlay diff is now consistent with the cached Lua terrain shell; biome-variant surface
  blocks (custom grass colours, etc.) no longer create spurious feature quads.
- Player block edits trigger a rebuild of the overlay and decor chunk within the streaming
  pipeline's normal update cadence (up to `OVERLAY_CHUNKS_PER_FRAME` / `DECOR_CHUNKS_PER_FRAME`
  chunks per frame).
- Removing `terrain_height_with_noise` from the overlay hot path saves one Perlin evaluation
  per column per frame during streaming.
- Lua structures made entirely of terrain-shell block types (stone/dirt/grass/sand) will not
  appear in the feature overlay. This is an accepted limitation; Lua mods should use distinct
  block types for structural elements.

---

## Related

- ADR-001 — Lua modding architecture
- ADR-002 — GPU-driven voxel rendering roadmap (`RUMPEL_RENDER_MODE=compute`)
