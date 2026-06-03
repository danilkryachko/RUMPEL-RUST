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

1. **797 `draw_indirect`/frame on Metal** — required for correct per-chunk `draw_params`; no `multi_draw_indirect` until Metal instance_index parity is proven.
2. **Tail spikes** — moving-camera autopilot still hits 40–175 ms frames from region rebuild / `manage_views` / `prepare_windows`; not parity with CPU streaming FPS.
3. **Headless vs GUI region window** — headless compare fixes `region_radius=1` (9 active regions); GUI autopilot loads wider window (62–81 regions).
4. **Metrics timing** — worst_packed snapshot is taken after main-world update, before render; reconciliation + bridge carry-forward keep acceptance fields positive (fixed false `generated_regions_visible=0` failure).

## References

- Prior partial profile: `.ai_tasks/gpu_generated_profile_20260603_190618.md`
- Headless compare logs: `.ai_tasks/generated_headless_compare_20260603_210038/`
- GUI screenshot from earlier run: `.ai_tasks/rumpel_client_packed_20260603_190947.png`
