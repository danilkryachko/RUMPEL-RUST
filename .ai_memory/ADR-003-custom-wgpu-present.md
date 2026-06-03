# ADR-003: Custom present (Method C)

## Status
**Rejected / closed** — window baseline only (monolithic Core3d to swapchain).

## Context
macOS GUI packed profiling showed frame tails dominated by Bevy `prepare_windows` / swapchain acquire while packed terrain draw stayed small. Experiments included:

- **Method A (split display):** offscreen Core3d + Core2d sprite blit (`RUMPEL_SPLIT_DISPLAY=1`). GUI A/B often worsened `worst_render_prepare_windows_us`.
- **Method C (custom wgpu present):** offscreen Core3d + blit-only window present subgraph (`RUMPEL_CUSTOM_PRESENT=1`). Did not consistently beat window baseline on the tracked metric set.

A post-`CameraDriver` manual wgpu blit spike was removed earlier for fighting Bevy’s frame lifecycle.

## Decision (superseded)
Do **not** ship split display or custom present. The client uses the standard **window baseline**: player `Camera3d` renders Core3d (+ UI) directly to the swapchain.

Env flags `RUMPEL_SPLIT_DISPLAY` and `RUMPEL_CUSTOM_PRESENT` are removed. Compare/profile recipes (`just compare-present-methods`, `just compare-split-display`, etc.) are stubbed with a message to use `just profile-packed`.

## Consequences
- Profiling reports `render_target=window` (or `headless` when applicable); no `custom_present_blit_us` or split/custom present ready-gate.
- Historical outcomes remain in `.ai_tasks/present_methods_verdict_*.md` and `.ai_tasks/split_display_compare_*` for reference only.

## References
- [ADR-002](ADR-002-gpu-driven-voxel-roadmap.md) — GPU voxel roadmap; split/custom present notes retained as historical context
