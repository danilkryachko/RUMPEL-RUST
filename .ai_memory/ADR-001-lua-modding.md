# ADR-001: Lua scripting for gameplay mods

## Status
Accepted

## Context
RUMPELRUST needs a modding layer that is easier for creators than compiled Rust plugins while keeping the voxel engine core safe and fast. The project already keeps block data outside Rust in `assets/blocks/base.ron`, so scripting should extend that data-driven approach instead of replacing it.

## Decision
Use `mlua` with vendored Lua 5.4 for the first scripting layer. Rust remains responsible for world storage, rendering, ECS systems, and validated APIs. Lua scripts live in `assets/mods/*.lua` and initially receive a narrow API: `register_block(table)`.

## Consequences
Mods can add gameplay data without recompiling the game. The Rust side owns numeric block IDs and validates when definitions enter `BlockRegistry`. Future APIs such as block events, recipes, NPC logic, and tick callbacks should be added as explicit functions rather than exposing internal chunk storage directly.
