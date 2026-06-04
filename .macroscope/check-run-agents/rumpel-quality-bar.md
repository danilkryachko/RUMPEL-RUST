---
title: Rumpel Quality Bar
model: claude-opus-4-6
effort: medium
input: full_diff
tools:
  - browse_code
  - git_tools
  - modify_pr
include:
  - "crates/**"
  - "assets/**"
  - "Cargo.toml"
conclusion: failure
---
# Rumpel Rust Quality Bar Review Guidelines

Review this PR against the RUMPEL RUST project constitution and codebase conventions:

## 1. Zero TODOs & Placeholders Policy
* Flag any `TODO`, `FIXME`, or placeholder comments in modified/added code.
* Do not leave empty functions, fake implementations, or mock hooks meant for future work. Every change must be 100% complete and production-ready.

## 2. Coordinate Types & Vector Math
* For block, chunk, or world logic, you MUST use `WorldPos`, `ChunkPos`, and `LocalBlockPos` from `rumpel_coords` / `coordinates.rs`.
* **Prohibited:** Do not use raw vector types (like Bevy's `Vec3` or `IVec3`) for block/world coordinate logic. They should only be used where strictly necessary (e.g. raw mesh generation or floating-point positioning).

## 3. Data-Driven Block Registration
* Blocks must be registered data-driven. Do not hardcode block properties (like IDs, hardness, textures) in Rust code.
* Any new block must be defined in `assets/blocks/base.ron`.

## 4. Lua Mods & API Alignment
* Game mods live in `assets/mods/*.lua`.
* `assets/mods/world_gen.lua` and `assets/mods/api_stub.lua` are special files. They must NOT be loaded as standard startup mods.
* If a new mod function or Rust global is added/renamed, ensure `assets/mods/api_stub.lua` and `assets/mods/.luarc.json` are updated to match so autocomplete/type checking in IDE remains in sync.
* Worldgen Lua scripts must only run through `rumpel_world::world_gen` and are restricted to using the bounded `get_block`, `set_block`, and `get_height` APIs. They must not access the persistent gameplay `LuaRuntime`.

## 5. Rendering & Surface Mesh
* The main terrain renderer is `rumpel_render::surface_streaming` using `VoxelQuadMaterial`.
* **Prohibited:** Do not revert surface/terrain generation to standard per-chunk `StandardMaterial`, as that breaks greedy meshing, repeat UVs, and custom shader atlas features.

## 6. ECS & Architecture
* Follow strict ECS patterns: data in Components/Resources, logic in Systems. Do not mix them.
* Do not build monolithic Bevy systems ("god objects"). Keep systems modular.
* If new features are large, they should be created as separate crates inside `crates/` rather than dumping code in existing packages.

## 7. Report Format
* Structure your feedback clearly. 
* Use 🔴 for critical violations that break the rules (e.g. TODOs left, wrong coordinate types, StandardMaterial reverts) and set the check to fail.
* Use 🟡 for warnings or stylistic improvements.
* If all checks pass and code complies with the Quality Bar, output: "All quality bar checks passed successfully! 🟢" with no extra noise.
