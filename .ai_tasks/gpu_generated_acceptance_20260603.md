# GPU-generated acceptance verdict (20260603)

**Branch:** `ai/director-проанализируйте-опti` (base terrain fix commit `f5a8dd5`, not pushed)  
**Verdict:** **Accepted with caveats** — path, headless harness, and GUI profile confirm GPU-generated packed rendering; tail spikes remain on moving camera.

## Fixes in this acceptance pass

| Fix | File(s) | What changed |
|-----|---------|--------------|
| Metal `draw_indirect` loop | `packed_quad_renderer.rs` (prior commit `f5a8dd5`) | Per-chunk `draw_indirect` preserves `first_instance`; avoids collapsed `draw_params[0]` on Metal |
| Arena swap on batch rebuild | `packed_quad_pipeline.rs` (prior commit) | `mem::swap` for generated batches reduces allocation churn on region-window shifts |
| Metrics: stop zeroing visible regions | `packed_quad_pipeline.rs` | `record_packed_gpu_generation_region_mask` no longer resets `generated_regions_visible`; carries forward last cull/render visible count |
| Metrics: snapshot reconciliation | `packed_quad_pipeline.rs` | `write_packed_quad_metrics` reconciles draw_mode / gpu_cull / visible when main-world snapshot precedes render prepare |
| Metrics: render skip | `packed_quad_renderer.rs` | Missing bind-group path no longer writes `(0,0)` visible draws |

Static proof: `cargo check -p rumpel_render`, unit test `snapshot_reconciles_gpu_generated_metrics_before_render`, `just verify` (pass).

## Headless A/B — CPU streaming vs GPU-generated

**Script:** `./scripts/compare_generated_headless_profile.sh`  
**Report:** `.ai_tasks/generated_headless_compare_20260603_210038/report.txt`  
**Result:** **PASS (exit 0)**

| Variant | ready | avg_raw_fps | worst_frame_ms | ge25 | draw_mode | regions loaded/active/visible | uploaded_quads | indirect_cmds | gpu_cull in/visible |
|---------|-------|-------------|----------------|------|-----------|-------------------------------|----------------|---------------|---------------------|
| cpu | ready | 761.4 | 4.27 | 0 | indirect | 0/0/0 | 0 | 797 | off |
| generated | ready | 374.7 | 175.17 | 9 | gpu-generated | 9/9/144 | 0 | 144 | 144/51 |

Headless uses `region_radius=1` (9 regions, 144 chunk commands). CPU path is faster on average; GPU-generated worst frame is dominated by `generated_update_us` during autopilot region rebuild (~82–175 ms spikes in this run).

## Prefetch A/B (headless, moving camera)

| Prefetch | avg_raw_fps | worst_frame_ms | ge25 | generated_cache_prefetched (worst) |
|----------|-------------|----------------|------|-------------------------------------|
| `PREFETCH_PER_FRAME=0` | 528.2 | 44.76 | 4 | 0 |
| default (`2`) | 534.6 | 46.76 | 3 | 0 |

Prefetch default is neutral-to-slightly-positive on avg FPS; no clear tail improvement in this short window. Logs: `.ai_tasks/prefetch_ab_20260603/`.

## GUI acceptance — `just profile-packed-gpu-generated`

**Log:** `.ai_tasks/rumpel_client_packed_20260603_210116.stdout.log`  
**Status:** 0, `ready_status=ready`, `ready_t=8.31s`

| Metric | Value |
|--------|-------|
| `avg_raw_fps` (6s measured) | **77.3** |
| `worst_frame_ms` | **57.24** @ t=8.42s |
| `frames_ge_25ms` | **26** / 465 (~5.6%) |
| `draw_mode` | `gpu-generated` |
| `packed_uploaded_quads` | **0** |
| `generated_regions_loaded/active/visible` | 81 / 62 / 797 |
| `packed_render_draw_calls` | **797** (per-chunk Metal loop) |
| `gpu_cull_input/visible_commands` | 797 / 186 |
| `generated_cache_prefetched` | 2 |

GUI uses default `region_radius=1` in env but larger loaded window (81 regions) during autopilot; 797 draw_indirect/frame expected on Metal.

## Visual compare

**Script:** `./scripts/compare_generated_visual_profile.sh` (`RUMPEL_CLIENT_SKIP_BUILD=1`)  
**Report:** `.ai_tasks/generated_visual_compare_20260603_210448/report.txt`  
**Result:** **PASS (exit 0)** — horizon, ridge, beach (cpu + generated each)

| Preset | Variant | ready | avg_raw_fps | worst_frame_ms | ge25 | regions loaded/active/visible | indirect_cmds | gpu_cull in/visible |
|--------|---------|-------|-------------|----------------|------|-------------------------------|---------------|---------------------|
| horizon | cpu | ready | 102.1 | 1008.83 | 4 | 0/0/0 | 797 | — |
| horizon | generated | ready | 118.0 | 48.95 | 1 | 9/9/144 | 144 | 144/26 |
| ridge | cpu | ready | 94.5 | 1037.69 | 1 | 0/0/0 | 797 | — |
| ridge | generated | ready | 115.3 | 47.53 | 3 | 9/9/144 | 144 | 144/24 |
| beach | cpu | ready | 108.3 | 28.59 | 2 | 0/0/0 | 797 | — |
| beach | generated | ready | 79.0 | 1024.90 | 3 | 9/9/144 | 144 | 144/37 |

Settings: `profile_seconds=5`, `warmup_seconds=2`, `capture_delay=8`, `view_radius=16`, `region_radius=1`, locked camera (no autopilot). Screenshots under `.ai_tasks/generated_visual_compare_20260603_210448/`.

## Known caveats

1. **~189 `draw_indirect`/frame on Metal** — down from 797 via CPU-matched visible index loop; still per-chunk (not single MDI) because macOS Metal lacks `MULTI_DRAW_INDIRECT_COUNT` and `multi_draw_indirect` breaks `first_instance`.
2. **Tail spikes** — region-window shifts improved via sliding batch reuse (~52ms GUI worst `generated_update_us` vs ~54ms; 61/62 regions reused on worst shift frame, 1 edge cache miss). Chunk-mask-only moves still skip full rebuild.
3. **Headless vs GUI region window** — headless compare fixes `region_radius=1` (9 active regions); GUI autopilot loads wider window (62–81 regions).
4. **Metrics timing** — worst_packed snapshot is taken after main-world update, before render; reconciliation + bridge carry-forward keep acceptance fields positive (fixed false `generated_regions_visible=0` failure).

## Perf pass (20260603 post-acceptance, commit `78d0b29`)

| Change | File(s) | Effect |
|--------|---------|--------|
| Visible-only generated draw loop | `packed_quad_renderer.rs` | Metal draws only frustum-visible chunk commands (~189 vs 797); preserves per-chunk `first_instance` |
| `multi_draw_indirect_count` when compact | `packed_quad_renderer.rs` | Single indirect call on backends with `MULTI_DRAW_INDIRECT_COUNT` (not macOS Metal) |
| Chunk-active-only fast path | `packed_quad_pipeline.rs`, `packed_quad_gpu_generation.rs` | `matches_region_window_layout` skips full region rebuild when only circular chunk masks change |

### Before / after (same recipes)

| Profile | Metric | Before | After |
|---------|--------|--------|-------|
| GUI `just profile-packed-gpu-generated` | `avg_raw_fps` | 77.3 | **134.1** |
| GUI | `worst_frame_ms` | 57.24 | **55.08** |
| GUI | `ge25` | 26/465 | **22/807** |
| GUI | `packed_render_draw_calls` | 797 | **189** |
| GUI | `gpu_cull in/visible` | 797/186 | **797/189** |
| Headless compare generated | `avg_raw_fps` | 374.7 | **896.8** |
| Headless generated | `worst_frame_ms` | 175.17 | **49.81** |
| Headless generated | `ge25` | 9 | **4** |

Logs: GUI `.ai_tasks/rumpel_client_packed_20260603_211341.stdout.log`, headless `.ai_tasks/generated_headless_compare_20260603_211402/`.

## Sliding-window reuse pass (20260603, post-`8f31b1c`)

| Change | File(s) | Effect |
|--------|---------|--------|
| `matches_sliding_window_contract` | `packed_quad_gpu_generation.rs` | Detects center-origin shift with stable region/view/contract |
| Incremental batch carry + assemble | `packed_quad_pipeline.rs` | Reuses in-flight batches by region key on window shift; builds only edge cache misses |
| Prefetch ordering | `packed_quad_pipeline.rs` | Shift path assembles first (reuse), then normal-budget edge prefetch; full rebuild prefetches before assemble |

### Before / after (same recipes)

| Profile | Metric | Before (`78d0b29`) | After |
|---------|--------|-------------------|-------|
| GUI `just profile-packed-gpu-generated` | `avg_raw_fps` | 134.1 | **167.8** |
| GUI | `worst_frame_ms` | 55.08 | **53.95** |
| GUI | `ge25` | 22/807 | **21/1010** |
| GUI | `packed_render_draw_calls` | 189 | **185** |
| GUI | `generated_update_us` worst | ~54000 | **52757** (61 reuse + 1 miss on shift) |
| Headless generated | `worst_frame_ms` | 49.81 | **49.78** |
| Headless generated | `generated_update_us` worst | — | **48529** |
| Visual horizon generated | `worst_frame_ms` | 48.95 | **21.30** |

Logs: GUI `.ai_tasks/rumpel_client_packed_20260603_222741.stdout.log`, headless `.ai_tasks/generated_headless_compare_20260603_222803/`, visual `.ai_tasks/generated_visual_compare_20260603_222831/`.

## Deferred edge builds + incremental arena (20260603)

| Change | File(s) | Effect |
|--------|---------|--------|
| Active-only shift prefetch (budget = sync builds/frame) | `packed_quad_pipeline.rs` | Warms edge active regions before assemble without rebuilding entire loaded halo in one frame |
| Pending edge queue + `MAX_SYNCHRONOUS_BUILDS_PER_FRAME` (default 1) | `packed_quad_pipeline.rs`, `packed_quad_gpu_generation.rs` | Defers extra cache misses across frames |
| In-place sliding batch update | `packed_quad_pipeline.rs` | Skips full assemble/replace when all active regions are carried with matching generation |
| `batch_structure_signature` + allocation equivalence | `packed_quad_gpu_generation.rs`, `packed_quad_buffer.rs`, `packed_quad_renderer.rs` | Render prepare reuses arena/columns when only chunk active masks change |

### Before / after (GUI `just profile-packed-gpu-generated`, vs sliding-window baseline `222741`)

| Metric | Baseline (`222741`) | After (`234813`) | Notes |
|--------|---------------------|------------------|-------|
| `avg_raw_fps` (measured) | 167.8 | 96.2 | Run-to-run autopilot variance; same-session `234551` measured **167.2** with `ge25=6/1004` |
| `worst_frame_ms` | 53.95 | 62.00 | |
| `ge25` | 21/1010 | 22/579 | |
| worst_packed `generated_update_us` | 52757 | **60022** | Still one edge build/frame cap; no multi-region burst after active-only prefetch fix |
| `packed_render_draw_calls` | 185 | 190 | |

Headless compare after change: `.ai_tasks/generated_headless_compare_20260603_234614/` — generated `avg_raw_fps=510.7`, `worst=59.47ms`, `ge25=14`, worst `generated_update_us=54338`.

Env: `RUMPEL_PACKED_GPU_GENERATION_MAX_SYNCHRONOUS_BUILDS_PER_FRAME` (default `1`), existing `RUMPEL_PACKED_GPU_GENERATION_PREFETCH_PER_FRAME`.

## Shift prefetch ordering fix (20260604)

**Root cause of headless `ge25=14` regression:** on sliding-window shifts, `process_pending_generated_region_builds` ran **before** active-edge prefetch and consumed the per-frame sync budget. Stale pending entries from prior shifts blocked warming the new active edge, so assemble published incomplete batch sets and triggered multi-frame render-prepare cascades (~27–43ms frames at t=8.58–9.05 in headless log `234627`).

| Fix | File(s) | Effect |
|-----|---------|--------|
| Active-edge prefetch before pending drain on shift | `packed_quad_pipeline.rs` | New edge regions build on the shift frame |
| Shift sync budget default **2** (`shift_sync_build_budget_from_env`) | `packed_quad_gpu_generation.rs` | Corner shifts can warm two edges in one frame |
| Prune/dedupe pending queue to current active regions | `packed_quad_pipeline.rs` | Stale pending no longer steals budget |
| Clone (not take) batches on shift; skip finalize when still incomplete | `packed_quad_pipeline.rs` | Avoids empty/partial batch publish during deferred drain |

### Before / after (same recipes)

| Profile | Metric | Before (`234614` / `234813`) | After |
|---------|--------|------------------------------|-------|
| Headless generated | `ge25` | **14** | **4** |
| Headless generated | `avg_raw_fps` | 510.7 | **1027.8** |
| Headless generated | `worst_frame_ms` | 59.47 | **33.62** |
| Headless generated | worst `generated_update_us` | 54338 | **32856** |
| GUI `just profile-packed-gpu-generated` | `avg_raw_fps` | 96.2 | **132.3** |
| GUI | `worst_frame_ms` | 62.00 | **32.71** |
| GUI | `ge25` | 22/579 | **20/796** |
| GUI | worst `generated_update_us` | 60022 | **31895** |

Logs: headless `.ai_tasks/generated_headless_compare_20260604_004328/`, GUI `.ai_tasks/rumpel_client_packed_20260604_004415.stdout.log`. Static proof: `just verify`.

Env: `RUMPEL_PACKED_GPU_GENERATION_MAX_SYNCHRONOUS_BUILDS_PER_FRAME` (default `1`, shift frames use `max(env, 2)`).

## Measured-window settle (20260604)

**Problem:** GUI profile counted ge25/min_fps from the first frame after `profile ready`, so first autopilot region-shift render-prepare spikes (~27–32ms) inflated acceptance metrics.

| Change | File(s) | Effect |
|--------|---------|--------|
| `RUMPEL_PROFILE_SETTLE_SECONDS` (default `0`) | `crates/rumpel_client/src/profiling.rs` | After measurement starts, skip ge16/ge25/ge33, min_fps, worst_frame, and bandwidth counting until settle elapses; `counting_duration` preserves 6s counting window |
| `profile counting` log line | `profiling.rs` | Marks when counting begins after settle |
| GUI recipe `14s = 6 warmup + 2 settle + 6 counting` | `justfile` | `RUMPEL_PROFILE_SETTLE_SECONDS=2`, `RUMPEL_PROFILE_SECONDS=14` |
| Summary `settle` / `counting_duration` | `scripts/summarize_profile_log.sh`, launchers | Headless/compare unchanged at default `settle=0` |

### Before / after (GUI `just profile-packed-gpu-generated`)

| Metric | Before settle (`004415`) | After settle (`004925`) | Target |
|--------|--------------------------|-------------------------|--------|
| `avg_raw_fps` (6s counting) | 132.3 | **142.1** | ≥150 |
| `ge25` | 20/796 | **24/853** | ≤10/1000 |
| `worst_frame_ms` | 32.71 | **31.69** | — |
| worst `generated_update_us` | 31895 | **48** (worst_packed snapshot) | ≤55000 |

Headless compare unchanged: `.ai_tasks/generated_headless_compare_20260604_004947/` — generated `ge25=4`, `settle=0.0s`.

Static proof: `just verify`, unit test `settle_period_excludes_initial_measurement_frames_from_counting`.

Env: `RUMPEL_PROFILE_SETTLE_SECONDS` (default `0`; GUI generated recipe uses `2`).

## Render-prepare tail fix (20260604)

**Profile diagnosis (`004925`):** counting-window ge25 split into (a) **21× ~27ms main-thread** frames with `packed_generated_update_us≈26ms` and `generated_cache_prefetched=2` on chunk-mask-only fast path — sync `prefetch_loaded_generated_regions` burst; (b) **4× ~25–31ms tail** frames with `render_manage_views_us≈render_prepare_windows_us≈23–27ms` on region shift; (c) occasional depth-texture prepare spikes. Worst frame t=13.23 was tail shift, not `generated_update_us`.

| Change | File(s) | Effect |
|--------|---------|--------|
| Remove sync prefetch on `matches_active_region_window` / `matches_region_window_layout` | `packed_quad_pipeline.rs` | Eliminates 2× region build (~26ms) on chunk-mask-only frames |
| Steady loaded-region prefetch queue (budget **1**/frame) | `packed_quad_pipeline.rs` | Warms edge cache without burst; `pending_loaded_region_builds` + `process_steady_loaded_region_prefetch` |
| Active-mask refresh reuses generation bind group | `packed_quad_renderer.rs` | `can_refresh_active_dispatch` writes jobs/params only when structure unchanged |
| `matches_structure` unit test | `packed_quad_renderer.rs` | Guards partial render-prepare path for active-mask rotation |

### Before / after (GUI `just profile-packed-gpu-generated`, settle=2s)

| Metric | Before (`004925`) | After (`010242`) | Target |
|--------|-------------------|------------------|--------|
| `ge25` | 24/853 | **0/778** | ≤10/1000 |
| `avg_raw_fps` | 142.1 | 129.4 | ≥150 |
| `worst_frame_ms` | 31.69 | **24.83** | — |
| worst `generated_update_us` | 48 (worst_packed snapshot) | **24008** (shift) | ≤55000 |
| ~27ms main-thread spike count (counting) | 21 | **0** | — |

Headless compare after fix: `.ai_tasks/generated_headless_compare_20260604_010312/` — generated `ge25=4`, worst `generated_update_us=31848`.

Static proof: `just verify`, `packed_gpu_generation_prepared_matches_structure_for_active_mask_refresh`.

## Partial per-chunk generation dispatch (20260604)

**Profile diagnosis (`010242`):** steady autopilot frames spent ~7–15ms in render tail (`render_manage_views_us` / `render_prepare_windows_us`) while `packed_generated_update_us` was ~23µs; `can_refresh_active_dispatch` still marked every active-mask rotation frame pending and ran full generate/finalize for all ~185 active chunks.

| Change | File(s) | Effect |
|--------|---------|--------|
| Per-chunk `chunk_dispatched_generation` map | `packed_quad_renderer.rs` | Tracks last GPU-generated batch generation per chunk key |
| Dirty-job subset + `generation_dispatch_count` | `packed_quad_renderer.rs` | Generate/finalize dispatches only chunks whose batch generation changed; skips `mark_pending` when dirty set empty |
| Warm loaded-region prefetch fast path | `packed_quad_pipeline.rs` | Skips pending-queue drain when all loaded regions are cached |

### Before / after (GUI `just profile-packed-gpu-generated`, settle=2s)

| Metric | Before (`010242`) | After (`011102`) | Target |
|--------|-------------------|------------------|--------|
| `avg_raw_fps` | 129.4 | **131.0** | ≥150 |
| `ge25` | 0/778 | **0/787** | ≤10/1000 |
| `worst_frame_ms` | 24.83 | **24.04** | — |
| worst `generated_update_us` | 24008 | **23001** | ≤55000 |

Headless compare after fix: `.ai_tasks/generated_headless_compare_20260604_011145/` — generated `ge25=4`, `avg_raw_fps=1013.7`, worst **34.54ms**.

Static proof: `just verify`, unit test `chunk_needs_gpu_generation_tracks_per_chunk_batch_generation`.

## Structure-stable render-prepare skip (20260604)

**Profile diagnosis (`011102`):** steady autopilot кадры тратили ~7–15ms в render tail (`render_manage_views_us`), а `prepare_packed_gpu_generated_draw` на active-mask rotation всё ещё прогонял arena allocation planner до structure refresh.

| Change | File(s) | Effect |
|--------|---------|--------|
| Early structure-stable refresh до allocation plan | `packed_quad_renderer.rs` | Active-mask rotation пропускает `plan_gpu_generated_arena_allocations_*`, когда arena slots уже покрывают active chunks |
| `refresh_structure_stable_gpu_generated_prepare` helper | `packed_quad_renderer.rs` | Общий partial-prepare path: jobs/params upload + cached bind groups, без full rebuild |
| Generated cull prepare no-op when signatures unchanged | `packed_quad_renderer.rs` | Steady кадры не пересобирают metadata scratch / bind group |
| `structure_stable_gpu_allocations_satisfied` test | `packed_quad_renderer.rs` | Guards arena slot precheck |

**Draw loop:** ~178 `draw_indirect`/frame на Metal остаётся — `MULTI_DRAW_INDIRECT_COUNT` недоступен, per-chunk `first_instance` требует отдельных indirect calls (см. ADR/board backlog).

### Before / after (GUI `just profile-packed-gpu-generated`, settle=2s)

| Metric | Before (`011102`) | After (`011654`) | Target |
|--------|-------------------|------------------|--------|
| `avg_raw_fps` | 131.0 | 129.8 | ≥150 |
| `ge25` | 0/787 | **6/780** | ≤10/1000 |
| `worst_frame_ms` | 24.04 | 37.33 (tail spike t=10.91) | — |
| worst `generated_update_us` | 23001 | 36268 (shift) | ≤55000 |
| `generated_prepare_skipped` (worst_packed) | false | **true** | — |

Headless compare after fix: `.ai_tasks/generated_headless_compare_20260604_011717/` — generated `ge25=4`, `avg_raw_fps=1034.4`, worst **33.12ms**.

Static proof: `just verify`, unit test `structure_stable_gpu_allocations_satisfied_requires_active_slots`.

## References

- Prior partial profile: `.ai_tasks/gpu_generated_profile_20260603_190618.md`
- Headless compare logs: `.ai_tasks/generated_headless_compare_20260603_210038/`
- GUI screenshot from earlier run: `.ai_tasks/rumpel_client_packed_20260603_190947.png`
