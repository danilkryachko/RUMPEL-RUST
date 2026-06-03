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

## References

- Prior partial profile: `.ai_tasks/gpu_generated_profile_20260603_190618.md`
- Headless compare logs: `.ai_tasks/generated_headless_compare_20260603_210038/`
- GUI screenshot from earlier run: `.ai_tasks/rumpel_client_packed_20260603_190947.png`
