# Development Workflow

## Daily Commands

- `just dev`: run the local development build with Bevy dynamic linking.
- `just dev-cached`: run the client with `CARGO_INCREMENTAL=0` so `sccache` can reuse more Rust compiler work.
- `just dev-gpu-compute`: run the opt-in GPU compute voxel prototype with counter diagnostics.
- `just release`: run the release build without Bevy dynamic linking.
- `just verify`: run formatting, check, Clippy, and tests.
- `just check-cached`: run workspace checks with `CARGO_INCREMENTAL=0` for better `sccache` reuse.
- `just sccache-stats`: inspect local compiler cache hit/miss statistics.
- `just profile-client`: run the surface renderer autopilot profile with `RUMPEL_RENDER_MODE=surface`.
- `just profile-gpu-compute`: run the GPU compute prototype parity profile with counter readback on the center slice.
- `just profile-gpu-compute-stress`: run the GPU compute queue stress profile without diagnostic readback stalls by default.
- `just changelog`: regenerate `CHANGELOG.md` from Conventional Commit history.

The project targets Bevy `0.18.x`. Keep Bevy ecosystem crates on versions compatible with that line.

## Render Modes

The production terrain path is selected with:

```bash
RUMPEL_RENDER_MODE=surface cargo run -p rumpel_client
```

GPU compute experiments are selected with:

```bash
RUMPEL_RENDER_MODE=compute RUMPEL_GPU_COUNTERS=1 cargo run -p rumpel_client
```

Use `just profile-gpu-compute` for strict parity proof. It pins `RUMPEL_GPU_COMPUTE_QUEUE_RADIUS=0`, `RUMPEL_GPU_COMPUTE_MAX_JOBS_PER_FRAME=1`, and `RUMPEL_GPU_COUNTERS=1` so the diagnostic readback can validate every prepared compute chunk.

Use `just profile-gpu-compute-stress` for production-like queue pressure. It defaults to `RUMPEL_GPU_COMPUTE_QUEUE_RADIUS=2`, `RUMPEL_GPU_COMPUTE_MAX_JOBS_PER_FRAME=8`, `RUMPEL_GPU_COUNTERS=0`, and an 8s warmup before a 6s measured window; override those env vars for larger experiments. Keep counter readbacks off for stress unless you are deliberately measuring diagnostic overhead.

Compute stress summaries include `compute_lifecycle=...`, which reports lifecycle samples, pending/building/loaded totals, queued/submitted chunk counts, dirty-generation invalidation totals, rebuild totals, evicted lifecycle/buffer totals, cancelled readback totals, owned compute output buffer allocation totals (`owned_output_buffers_sum`, `owned_output_bytes_sum`), and `max_building`. A clean radius-2 stress run should end with all expected chunks loaded, owned output buffers allocated for those chunks, and no lingering building work.

World edit summaries include `world_edits=...`, which reports stored and ignored neutral `WorldBlockEdit` messages plus max store generation and edit count from `world block edits stored` telemetry. Gameplay/world edit producers should send `WorldBlockEdit` with `ChunkPos`, `LocalBlockPos`, and `BlockId`; `WorldEditStore` preserves the latest runtime override per typed block key and applies it as an overlay when compute terrain layers are generated.

Compute edit summaries include `compute_edits=...`, which reports applied and ignored edit message totals plus the maximum number of touched compute chunks from `voxel compute block edits applied` telemetry. The compute renderer bridges neutral `WorldBlockEdit` messages to `VoxelComputeBlockEdit`, then updates compute source generation, contracts, and neighbor boundaries before render extraction.

The legacy `RUMPEL_COMPUTE_PROTOTYPE=1` flag still enables the compute prototype, but new workflows should prefer `RUMPEL_RENDER_MODE`. The phased migration plan lives in `.ai_memory/ADR-002-gpu-driven-voxel-roadmap.md`.

## Build Cache

Rust builds use `sccache` through `.cargo/config.toml`:

```toml
[build]
rustc-wrapper = "/opt/homebrew/bin/sccache"
```

All agents should keep this wrapper enabled. `ccache` is not a substitute for Rust builds; use `sccache` for `rustc` caching. When validating cache behavior, prefer `CARGO_INCREMENTAL=0` through `just check-cached` or `just dev-cached`, then inspect hits with `just sccache-stats`.

On Apple Silicon macOS, `.cargo/config.toml` also points Rust linking at Homebrew LLVM's `ld64.lld`:

```toml
[target.aarch64-apple-darwin]
rustflags = [
  "-C", "target-cpu=native",
  "-C", "link-arg=-fuse-ld=/opt/homebrew/bin/ld64.lld",
]
```

Keep `/opt/homebrew/bin/ld64.lld` installed through `brew install llvm` on this machine. If a different macOS host lacks that path, install LLVM first rather than silently removing the linker optimization.

## Lua Mods

Lua Language Server metadata lives in `assets/mods/.luarc.json` and `assets/mods/api_stub.lua`. The stub file is for IDE autocomplete and linting only; the runtime mod loader skips it.

`assets/mods/world_gen.lua` is also skipped by the startup mod loader. It runs as a bounded chunk-generation post-pass through `rumpel_world::world_gen`, where Rust exposes local `get_block`, `set_block`, `get_height`, and `Chunk` APIs.

## Hooks

Install local hooks with:

```bash
pre-commit install
pre-commit install --hook-type pre-push
```

Pre-commit runs lightweight file hygiene and formatting checks. Pre-push runs the full Clippy gate.

## Dependency Hygiene

- `just deny`: run `cargo-deny` for advisories, licenses, bans, and source checks.
- `just machete`: run `cargo-machete` for unused dependency detection.

Install optional tools when needed:

```bash
cargo install cargo-deny cargo-machete git-cliff
```
