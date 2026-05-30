# Changelog

All notable changes to RUMPEL RUST are documented here.

## Unreleased

### Added

- Integrated `bevy_voxel_world` 0.16 as the runtime voxel streaming and meshing backend for the client.
- Added a `RumpelVoxelWorld` adapter that maps RUMPEL block registry IDs into the voxel backend terrain generator.
- Added a lightweight FPS/chunk debug HUD and made the ECS world inspector opt-in behind the `rumpel_debug/inspector` feature.
- Added custom surface-aware terrain streaming for the main client, with HUD counters for rendered, pending, and queued chunks.
- Added an environment-driven client profiling run with timed auto-exit, one-second metric logs, and optional autopilot flight.

### Changed

- Moved main-client chunk mesh generation off the frame path into Bevy's async compute task pool.
- Reused one shared terrain material for streamed chunks instead of allocating a material per chunk.
- Cached per-chunk terrain heights while meshing to reduce repeated noise sampling.

### Performance

- Limited per-frame mesh task scheduling and mesh asset insertion to reduce FPS drops while flying.
- Dropped stale pending chunk tasks when the player leaves their target radius before upload.
- Added distance-preserving terrain LOD so far chunks remain streamed at radius 32 with cheaper meshes.
- Prioritized near full-detail chunks and far coarse chunks during streaming to improve perceived draw distance.
- Disabled temporary point-light shadow maps in the client scene to avoid heavy per-frame GPU shadow work.
- Forced continuous window updates and no-vsync presentation settings for high-FPS testing.
- Switched streamed terrain material to unlit rendering and disabled MSAA on the camera.
- Raised the dev profile optimization level for local gameplay profiling.

### Infrastructure

- Added repository development workflow files for CI, local verification, dependency hygiene, pre-commit hooks, and changelog generation.
