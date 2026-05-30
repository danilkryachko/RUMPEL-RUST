# RUMPEL RUST Agent Instructions

This repository is the RUMPEL RUST voxel sandbox project.

Before changing code, read `AI_DEVELOPMENT_GUIDE.md`, `GDD.md`, `BOARD.md`, and relevant files in `.ai_memory/`. Treat `AI_DEVELOPMENT_GUIDE.md` as the project constitution unless a higher-priority system instruction conflicts with it.

## Multi-Agent Workflow

The project owner wants the multi-agent system to be picked up automatically for this repository.

When the runtime exposes multi-agent tools, the lead agent acts as the Game Director and should delegate complex or parallelizable work to subagents according to the project's role model:

- `engine_architect`: use a `worker` subagent for voxel math, chunk storage, meshing, performance, threading, or engine architecture tasks.
- `gameplay_coder`: use a `worker` subagent for player mechanics, ECS gameplay systems, UI, inventory, interaction, or world-facing features.
- `code_reviewer`: use a `worker` subagent for independent review, `cargo check`, `cargo test`, and `cargo clippy` verification when useful.
- `game_designer`: keep design changes in `GDD.md` and task state in `BOARD.md`; use a subagent only when design work is separable from implementation.
- `art_director`: own visual assets under `assets/`; use image generation only for bitmap assets or concepts when that matches the task.
- Read-only codebase questions should use an `explorer` subagent when available.

Subagent tasks must be concrete, bounded, and assigned disjoint file ownership when editing code. Subagents are not alone in the codebase: they must not revert user changes or unrelated edits, and they must adapt to concurrent work.

If the runtime does not expose subagent tools, continue locally and mention the limitation briefly.

## Codex and Antigravity Collaboration

The project may be developed by both Codex and Antigravity agents. Treat this as a collaboration protocol, not as permission for uncontrolled parallel edits.

- The Producer owns product direction and final approval.
- Codex owns final integration and verification unless the Producer explicitly assigns that role to another agent.
- Antigravity may propose, implement, or review bounded tasks with explicit file ownership.
- Before editing, inspect the current working tree and adapt to existing changes.
- Do not revert, overwrite, or reformat changes made by another agent unless the Producer explicitly asks for it.
- For parallel work, each agent must state its task scope, editable files, forbidden files, and expected verification command.
- Shared architecture files such as `Cargo.toml`, `AGENTS.md`, `AI_DEVELOPMENT_GUIDE.md`, `GDD.md`, `BOARD.md`, and core crate boundaries require extra care: inspect diffs first and keep edits narrowly scoped.
- Commit and push require explicit Producer approval.
- Final integration must pass `just verify` or the equivalent Cargo checks before publication.

## Local Quality Bar

- Keep blocks data-driven through `assets/blocks/base.ron` and Lua registration APIs.
- Use `WorldPos`, `ChunkPos`, and `LocalBlockPos` for block/world logic instead of raw vector types where practical.
- Keep systems ECS-oriented and modular across crates.
- Verify Rust changes with `cargo check`, `cargo test`, and `cargo clippy --all-targets -- -D warnings` when the scope justifies it.
- Do not leave placeholder code, empty future hooks, `TODO`, or `FIXME` comments.
- Do not commit or push unless the project owner explicitly asks. Before publishing changes, run `just verify` or the equivalent Cargo checks.
