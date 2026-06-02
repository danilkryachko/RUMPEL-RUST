# RUMPEL RUST Agent Instructions

This repository is the RUMPEL RUST voxel sandbox project.

Before changing code, read `AI_DEVELOPMENT_GUIDE.md`, `GDD.md`, `BOARD.md`, and relevant files in `.ai_memory/`. Treat `AI_DEVELOPMENT_GUIDE.md` as the project constitution unless a higher-priority system instruction conflicts with it.

## Multi-Agent Workflow

The project owner wants the multi-agent system to be picked up automatically for this repository.

### Cursor subagents (project config)

Custom subagents for this repo live in [`.cursor/agents/`](.cursor/agents/). Cursor loads them from disk for the Agent; the canonical check is that folder, not a Settings list.

**If they do not appear in Settings:** use **Cursor Settings** (`Cmd+Shift+J`, not VS Code `Cmd+,`), open the **Editor** Agent chat (not only the separate Agents/Glass window), type `/` and look for `engine-architect`, `code-reviewer`, etc. An empty Settings list with working `/name` commands is a known Cursor UI quirk. The always-on rule `.cursor/rules/rumpel-subagents.mdc` reminds the lead agent to delegate.

| Role | Cursor subagent | Invoke explicitly |
| --- | --- | --- |
| Engine | `engine-architect` | `/engine-architect …` |
| Gameplay | `gameplay-coder` | `/gameplay-coder …` |
| Review | `code-reviewer` | `/code-reviewer …` |
| Design docs | `game-designer` | `/game-designer …` |
| Art / assets | `art-director` | `/art-director …` |

Built-in Cursor subagents (`explore`, `bash`, `browser`) still apply for search, shell, and browser work. Prefer `explore` for read-only codebase research.

The lead agent acts as **Game Director**: delegate complex or parallelizable work to the subagents above (automatically or via `/name`).

Subagent tasks must be concrete, bounded, and assigned disjoint file ownership when editing code. Subagents are not alone in the codebase: they must not revert user changes or unrelated edits, and they must adapt to concurrent work.

### Codex mapping

In Codex environments that only expose `explorer`, `worker`, and `default`:

- `engine_architect` and `gameplay_coder` → `worker`
- `code_reviewer` → `worker` or local `cargo` checks for small tasks
- Read-only research → `explorer`

If subagent tools are unavailable, continue locally and mention the limitation briefly.

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
- Lua gameplay mods live in `assets/mods/*.lua`; `assets/mods/world_gen.lua` is a world-generation post-pass, and `assets/mods/api_stub.lua` is IDE-only metadata. Do not load either as a normal startup mod.
- Keep Lua API typings in sync with Rust globals through `assets/mods/api_stub.lua` and `assets/mods/.luarc.json` whenever adding or renaming mod functions.
- World generation Lua must run through `rumpel_world::world_gen` with the bounded `get_block`, `set_block`, and `get_height` API. Do not call the persistent gameplay `LuaRuntime` from async render/mesh tasks.
- The main terrain renderer is `rumpel_render::surface_streaming` with `VoxelQuadMaterial`, texture-array tile IDs, repeat UVs, backface culling, and greedy merged top/side quads. Do not revert it to per-chunk `StandardMaterial`.
- Do not reduce the configured surface view radius to chase FPS unless the Producer explicitly asks; optimize mesh generation, upload, culling, and material paths while preserving draw distance.
- For FPS work, use the autopilot profile command documented in `AI_DEVELOPMENT_GUIDE.md` and compare `surface_sample_vertices`, `surface_sample_indices`, build time, upload time, and FPS.
- GPU-driven voxel rendering is tracked in `.ai_memory/ADR-002-gpu-driven-voxel-roadmap.md`. Keep `RUMPEL_RENDER_MODE=surface` as the baseline and use `RUMPEL_RENDER_MODE=compute` only for measured compute prototype work until parity is proven.
- Use `WorldPos`, `ChunkPos`, and `LocalBlockPos` for block/world logic instead of raw vector types where practical.
- Keep systems ECS-oriented and modular across crates.
- Verify Rust changes with `cargo check`, `cargo test`, and `cargo clippy --all-targets -- -D warnings` when the scope justifies it.
- Do not leave placeholder code, empty future hooks, `TODO`, or `FIXME` comments.
- Do not commit or push unless the project owner explicitly asks. Before publishing changes, run `just verify` or the equivalent Cargo checks.

## Rust Toolchain Policy

- The project uses Rust version **1.96.0** as specified in [rust-toolchain.toml](file:///Users/daniil/RUMPELRUST/rust-toolchain.toml).
- All agents must compile, test, and check code using Rust 1.96.0 to utilize new ergonomic features (e.g. `IntoIterator` for new Copyable Ranges, `if let` Guards in match statements, and `cfg_select!`) and to guarantee environment consistency.

## Build Cache Policy

All agents must use the repository Cargo configuration for Rust builds. The project configures `sccache` through `.cargo/config.toml` as `rustc-wrapper`, so normal `cargo` and `just` commands automatically use the compiler cache when `sccache` is installed.

- Prefer `just check-cached` for repeated full checks where cache reuse matters.
- Prefer `just dev-cached` for cached client runs when measuring rebuild speed.
- Use `just sccache-stats` or `sccache --show-stats` when validating cache behavior.
- Do not replace the Rust compiler cache with `ccache`; `ccache` is for C/C++ and does not cache `rustc` work effectively.
- If `sccache` is missing, install it with Homebrew (`brew install sccache`) before long local build sessions when the environment allows it.
- On this macOS development machine, `.cargo/config.toml` also uses Homebrew LLVM's `/opt/homebrew/bin/ld64.lld` for faster `aarch64-apple-darwin` linking. Keep that setting active unless the local machine lacks the linker.
