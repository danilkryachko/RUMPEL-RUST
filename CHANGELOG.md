# Changelog

All notable changes to RUMPEL RUST are documented here.

## Unreleased

### Added

- Added GPU-driven indirect drawing path using standard WebGPU non-indexed draw indirect command buffers and structured draw parameters, reducing Bevy rendering overhead to near zero.
- Added support for hardware-accelerated Bevy/wgpu `multi_draw_indirect` (if supported by device) with automatic loop-based indirect submission using per-command `draw_indirect` calls (zero binding or pipeline state changes).
- Unified direct per-batch rendering with the indirect rendering path under a single vertex shader pipeline by transitioning translation bindings from uniforms to storage structures.
- Added active draw mode (`multi-indirect`, `indirect`, `direct`) and indirect commands count to real-time HUD and profiling logs.
- Added packed renderer texture-array atlas parity with the surface path by sampling registry texture mappings, with `RUMPEL_PACKED_FACE_DEBUG=1` preserving the face-color diagnostic mode.
- Added `RUMPEL_CAMERA_LOCK=1` / `RUMPEL_PACKED_CAMERA_LOCK=1` visual comparison mode that freezes player look, movement, interactions, and profiling autopilot for repeatable surface/packed screenshots.
- Added packed renderer memory reservation knobs with defaults of 1GiB dedicated GPU reserve and 2GiB CPU region-vector reserve target, exposed in profiling as `packed_gpu_reserved_bytes` and `packed_cpu_reserved_bytes`.
- Added lazy GPU buffer allocation inside `PackedQuadGpuArena` to resolve startup ordering panics under Bevy 0.18 when `RenderDevice` is not yet available in the Render sub-app during initialization.
- Added contiguous, unified GPU storage buffer arena (`PackedQuadGpuArena`) replacing per-chunk vertex pulling buffers to prepare for future MultiDrawIndirect (MDI) pathways.
- Added automatic, grow-only power-of-two buffer resizing when total packed quads exceed arena capacity, tracking reallocations and uploading only when generation or layout has changed.
- Added player-relative $X/Z$ streaming grid of `PackedVoxelQuad` chunks around the camera (configurable via `RUMPEL_PACKED_VIEW_RADIUS`, defaulting to 4).
- Added neighbor-aware boundary face culling procedurally on the fly for the 4 cardinal directions (+X, -X, +Z, -Z), completely eliminating chunk boundary cracks.
- Added cached `BindGroup` and translation uniform buffer (`Buffer`) directly inside `PreparedPackedQuadBatch` in the `Prepare` phase to avoid per-frame CPU-to-GPU allocations and pipeline stalls.
- Added real-time pipeline streaming metrics (`packed_chunks_loaded`, `pending_builds`, `built_this_frame`, `uploaded_this_frame`) to the profiling log samples and metrics bridge.
- Bypassed Bevy surface streaming and compute prototypes completely under `RUMPEL_RENDER_MODE=packed` to ensure clean, isolated benchmark rendering without duplicate draws.
- Expanded the local `just profile-packed` benchmark view radius to `8` for a larger, highly rigorous performance testing area.
- Added `RUMPEL_RENDER_MODE=packed` isolated experimental benchmark mode for high-performance PackedVoxelQuad vertex pulling verification without double rendering.
- Added opt-in PackedVoxelQuad custom vertex pulling renderer skeleton and Render Graph node under the `RUMPEL_PACKED_QUAD_RENDERER=1` env flag, pulling from prepared batches and drawing directly onto the 3D scene view target using standard depth attachments.
- Added opt-in debug producer, atomic metrics bridge, HUD integration, and profiling logs for the PackedVoxelQuad pipeline under the `RUMPEL_PACKED_QUAD_DEBUG=1` env flag.
- Added `packed_quad_buffer` module introducing `PackedQuadBuffer` as the GPU upload layer for Bevy/wgpu storage buffers to support high-efficiency vertex pulling / MDI rendering.
- Added `voxel_packed_quads` module introducing the GPU-friendly `PackedVoxelQuad` struct (u16 coordinates, u16 block ID, u8 face, u8 lod, u32 flags) pinned as a stable 16-byte Pod ABI (`#[repr(C)]` with `bytemuck` support for `Zeroable` and `Pod`) as the contract for vertex pulling/MDI.
- Added neighbor-aware voxel builder `build_packed_quads_for_chunk_with_neighbors` with deterministic boundary face culling across neighboring chunks and full 2D greedy meshing on all 6 direction planes (+X, -X, +Y, -Y, +Z, -Z), ready for the multi-chunk/region renderer.
- Integrated `bevy_voxel_world` 0.16 as the runtime voxel streaming and meshing backend for the client.
- Added a `RumpelVoxelWorld` adapter that maps RUMPEL block registry IDs into the voxel backend terrain generator.
- Added a lightweight FPS/chunk debug HUD and made the ECS world inspector opt-in behind the `rumpel_debug/inspector` feature.
- Added custom surface-aware terrain streaming for the main client, with HUD counters for rendered, pending, and queued chunks.
- Added an environment-driven client profiling run with timed auto-exit, one-second metric logs, and optional autopilot flight.
- Added embedded GPU compute shader loading for `rumpel_render`, so client binaries no longer depend on a package-local shader asset path.
- Added GPU mesh counter readback diagnostics for the voxel compute path, including vertex/index capacity and overflow warnings.
- Added an opt-in `RUMPEL_COMPUTE_PROTOTYPE=1` path so the prototype GPU compute renderer no longer replaces the main surface renderer by default.
- Added surface streaming frame-phase metrics to the HUD and profiling logs, covering queue depth, uploads, despawns, build time, upload time, vertices, indices, and LOD.
- Added opt-in `RUMPEL_DEPTH_PREPASS=1` and `RUMPEL_OCCLUSION_CULLING=1` camera flags for measured Bevy 0.18 depth prepass and GPU occlusion-culling experiments.
- Added a tested palette + RLE chunk codec in `rumpel_world` as the first step toward compressed chunk storage.
- Added a custom `VoxelQuadMaterial` backed by the block texture-array atlas, with per-vertex tile IDs and repeat UVs for merged voxel quads.
- Added ADR-002 with the phased GPU-driven voxel renderer roadmap for Codex and Antigravity agents.
- Added explicit `RUMPEL_RENDER_MODE=surface|compute` selection for renderer experiments.

### Changed

- Changed `run_client_macos_gui.sh` to automatically prune previous run's timestamped macOS `.app` bundles and old stdout/stderr logs under `target/macos/` and `.ai_tasks/`, stopping massive disk space bloat on every run.
- Changed packed rendering to prefer the validated loop-indirect vertex-pulling path when `INDIRECT_FIRST_INSTANCE` is supported; set `RUMPEL_PACKED_INDIRECT=0` to force direct mode, while `RUMPEL_PACKED_MULTI_INDIRECT=1` remains a separate opt-in pending visual acceptance.
- Disabled the client startup chunk preload gate for now, entering gameplay immediately after Lua mod loading instead of waiting on a chunk-count warmup overlay.
- Consolidated the direct packed renderer into one render pass per view, with pipeline and view bind group set once and only per-batch storage bind groups changed before each draw.
- Corrected packed quad face winding and enabled backface culling to match the surface renderer's atlas material path.
- Corrected packed side-face repeat UVs so grass-side texture fringes stay on vertical face top edges instead of rotating onto side edges.
- Initialized Rust and Lua world time to midday so packed screen-test diagnostics start on the normal blue sky instead of briefly showing the sunrise/sunset clear color behind the debug quad.
- Changed packed view uniform preparation to derive `clip_from_world` from the extracted camera projection and transform when Bevy does not provide the precomputed matrix, avoiding an accidental identity view-projection in custom draw diagnostics.
- Ordered the packed render graph node explicitly between `EndMainPass` and `StartMainPassPostProcessing`, and fixed the debug producer key so the generated chunk is translated to its actual chunk position.
- Changed the packed renderer depth test to Bevy's reversed-Z `GreaterEqual` convention so custom vertex-pulled terrain can pass depth against the 3D view depth texture.
- Added `RUMPEL_PACKED_TOP_ONLY=1` as a packed renderer diagnostic that keeps only upward terrain faces when isolating geometry/shader issues.
- Changed the packed render pass to use Bevy's `ViewTarget`/`ViewDepthTexture` attachment helpers instead of manually binding the main texture and depth view, preserving Bevy's clear/load state across the 3D graph.
- Removed the packed screen-space quad diagnostic so stale `RUMPEL_PACKED_SCREEN_TEST=1` environment values no longer cover the world with a green rectangle.
- Made automated profiling runs flush stdout and terminate the process after `profile end`, preventing smoke-test clients from lingering after timed runs.
- Added `RUMPEL_PRESENT_MODE=auto-no-vsync|immediate|mailbox|fifo|fifo-relaxed|auto-vsync` so packed frame-pacing profiles can explicitly compare swapchain present behavior on Metal without code edits.
- Moved main-client chunk mesh generation off the frame path into Bevy's async compute task pool.
- Batched the surface terrain renderer into 4x4 chunk regions, keeping the HUD chunk count while reducing mesh entity and draw-call pressure.
- Reused one shared terrain material for streamed chunks instead of allocating a material per chunk.
- Cached per-chunk terrain heights while meshing to reduce repeated noise sampling.
- Switched streamed surface terrain from flat vertex colors to the shared block texture atlas, using registry texture mappings for grass, sand, and stone faces.
- Enabled nearest-neighbor image sampling in the client so voxel block textures render crisp instead of blurred.
- Split Lua startup mods from the bounded Lua world-generation post-pass so `world_gen.lua` no longer runs without `get_block`, `set_block`, and `get_height`.
- Added Lua Language Server metadata for mod autocomplete and linting through `assets/mods/.luarc.json` and `assets/mods/api_stub.lua`.
- Added `get_height` plus safe block query/edit intent fallbacks to the persistent Lua gameplay runtime so world tick mods can run outside bounded chunk-generation contexts.
- Routed the spawn chunk through `rumpel_world` Lua worldgen and rendered it as a block overlay while keeping the rest of the view-distance terrain on the faster heightmap path.
- Added distance-based terrain LOD for streamed regions while preserving the configured view radius.
- Limited region mesh uploads per frame to smooth streaming spikes during fast flight.
- Kept all streamed surface LOD regions on the block texture atlas so distant and nearby terrain share the same visual material path.
- Restored side walls for distant LOD surface meshes so coarse terrain no longer exposes sky-colored gaps between height steps.
- Enabled backface culling on streamed terrain materials after fixing top-face winding.
- Switched streamed surface terrain from `StandardMaterial` to the voxel quad material so greedy-merged terrain keeps repeating block textures instead of stretching them.
- Clamped creative flight above the generated heightmap surface to prevent accidentally slipping under the world.
- Disabled prepass and shadow pipelines for `VoxelQuadMaterial`, matching the current unlit forward terrain path and avoiding invalid prepass shader specialization.
- Kept the legacy `RUMPEL_COMPUTE_PROTOTYPE=1` flag as an alias for `RUMPEL_RENDER_MODE=compute`.
- Switched the GPU compute prototype's parity chunk to use `rumpel_world::generate_chunk_with_context` instead of duplicating terrain, Lua, and block mapping logic inside `rumpel_render`.
- Delayed GPU compute parity chunk setup until the loading state update so Lua startup block registration can populate the shared `BlockRegistry` first.

### Performance

- Moved packed chunk generation and `PackedVoxelQuad` building onto Bevy's async compute task pool, with `RUMPEL_PACKED_MAX_BUILD_TASKS` capping in-flight work while preserving the configured draw distance.
- Enabled packed distant LOD by default, with `RUMPEL_PACKED_LOD=0` available for full-resolution comparison. Far chunks build as coarse packed terrain shells while the nearer radius and LOD transition seams stay full-resolution; LOD side walls close against the minimum height of the touching edge so distant coarse cells do not expose sky-colored holes when adjacent terrain has uneven samples.
- Pre-sized the packed GPU arena from `RUMPEL_PACKED_VIEW_RADIUS` (with `RUMPEL_PACKED_ARENA_PREALLOC_QUADS` override) so radius 32 starts at the expected arena capacity instead of reallocating repeatedly during chunk streaming.
- Batched packed chunk quads into 4x4 chunk regions by default, with `RUMPEL_PACKED_REGION_SIZE` as an override, reducing direct draw submissions while keeping the non-indirect renderer path active on Metal.
- Switched packed region batches to stable arena slots and dirty-range uploads, so growing regions no longer force full arena rewrites while radius 32 streams in.
- Changed packed region assembly to append newly built chunk quads incrementally, reserving full region rebuilds for despawn/removal paths during camera movement. Cross-chunk compaction still runs immediately by default after profiling showed the deferred variant increased streaming-time command pressure.
- Added coarse per-view frustum culling for direct packed region draws, reporting `packed_visible_batches`, `packed_visible_quads`, and `packed_visible_vertex_count` in profiling.
- Applied the same per-view region culling and visible metrics to the opt-in loop-indirect packed path so indirect probes only submit visible region commands.
- Added packed render graph CPU encode metrics (`packed_render_node_us`, `packed_render_draw_calls`, and `packed_render_items_considered`) to HUD/profiling so radius 32 frame pacing can be separated from chunk streaming and upload work.
- Added packed region quad compaction across chunk boundaries and enabled adaptive face-range command culling for packed indirect draws by default, with `RUMPEL_PACKED_FACE_RANGE_CULL=0` available for full-face comparison and `RUMPEL_PACKED_FACE_RANGE_MIN_QUADS` controlling when a batch is split into face commands.
- Added adaptive packed streaming budgets for build scheduling and completed-chunk ingestion, reducing work per frame after frame-time spikes while preserving the configured view radius. `RUMPEL_PACKED_ADAPTIVE_STREAMING=0`, `RUMPEL_PACKED_TARGET_FRAME_MS`, and `RUMPEL_PACKED_MAX_COMPLETIONS_PER_FRAME` are available for profiling.
- Added opt-in deferred packed region compaction for profiling through `RUMPEL_PACKED_DEFER_COMPACTION=1`, with `RUMPEL_PACKED_MAX_COMPACTIONS_PER_FRAME` controlling the background budget.
- Limited per-frame mesh task scheduling and mesh asset insertion to reduce FPS drops while flying.
- Dropped stale pending chunk tasks when the player leaves their target radius before upload.
- Added distance-preserving terrain LOD so far chunks remain streamed at radius 32 with cheaper meshes.
- Prioritized near full-detail chunks and far coarse chunks during streaming to improve perceived draw distance.
- Disabled temporary point-light shadow maps in the client scene to avoid heavy per-frame GPU shadow work.
- Forced continuous window updates and no-vsync presentation settings for high-FPS testing.
- Switched no-vsync window setup to `AutoNoVsync` with a frame latency of 1 for safer macOS fallback behavior.
- Switched streamed terrain material to unlit rendering and disabled MSAA on the camera.
- Raised the dev profile optimization level for local gameplay profiling.
- Limited the prototype GPU compute mesh path to one chunk dispatch per frame while it uses a shared counter buffer.
- Removed per-frame HUD update logging and wired chunk HUD/profile counters to active GPU chunk mesh entities.
- Made GPU mesh counter readback opt-in through `RUMPEL_GPU_COUNTERS=1` so normal profiling avoids forced readback synchronization.
- Updated the compute mesh draw count from GPU-generated index counters, avoiding the previous fixed-capacity draw of unused indices.
- Increased the prototype compute mesh capacity to cover the current procedural terrain without counter overflow.
- Re-disabled directional shadow maps and switched the compute terrain material to unlit rendering for high-FPS profiling.
- Enabled Homebrew LLVM `ld64.lld` for Apple Silicon dev linking while keeping `sccache` as the Rust compiler cache.
- Prevented profiling runs from printing duplicate `profile end` records after the exit signal is sent.
- Added per-frame surface streaming instrumentation so FPS drops during autopilot runs can be tied to mesh build, upload, or queue pressure.
- Changed creative flight so WASD moves horizontally while Space/Shift control altitude, preventing accidental dives below the terrain while looking down.
- Prepared the chunk storage layer for palette/RLE compression without changing active dense edit semantics.
- Added 2D greedy merging for heightmap top faces and directional greedy merging for side walls, reducing the initial autopilot surface mesh from roughly 1.30M vertices to roughly 0.90M vertices at the current 16-chunk view radius.
- Reworked the surface terrain greedy pass around binary 2D face masks for top faces and side walls, with unit coverage for mask rectangle extraction.
- Added `just profile-client` for repeatable autopilot FPS/streaming profiles.
- Added `just dev-gpu-compute` and `just profile-gpu-compute` for opt-in GPU compute prototype runs with counter diagnostics.
- Made `just profile-client` force `RUMPEL_RENDER_MODE=surface` so surface baselines are not polluted by shell environment flags.

### Infrastructure

- Fixed packed renderer verification issues found during local acceptance: initialized the render-world `PackedQuadGpuArena`, used the Bevy 0.18-compatible indirect draw feature gate, and cleaned up strict clippy formatting.
- Added repository development workflow files for CI, local verification, dependency hygiene, pre-commit hooks, and changelog generation.
- Enabled local Rust compiler caching through `sccache` and added cached `just` commands for client development.
- Documented the `sccache` build-cache policy for all repository agents.
- Removed the direct `wgpu` dependency from `rumpel_render`, leaving Bevy's renderer as the single `wgpu` owner.
- Removed direct `mlua` and `rumpel_modding` dependencies from `rumpel_render`; Lua world generation stays owned by `rumpel_world`.
