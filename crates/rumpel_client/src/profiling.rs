use bevy::{
    app::AppExit,
    core_pipeline::core_3d::{
        graph::{Core3d, Node3d},
        prepare_core_3d_depth_textures, prepare_core_3d_transmission_textures,
        prepare_prepass_textures,
    },
    diagnostic::{DiagnosticsStore, FrameTimeDiagnosticsPlugin},
    prelude::*,
    render::{
        Render, RenderApp, RenderSystems,
        graph::CameraDriverLabel,
        render_graph::{Node, NodeRunError, RenderGraph, RenderGraphContext, RenderLabel},
        render_resource::{
            Buffer, BufferDescriptor, BufferUsages, Extent3d, LoadOp, MapMode, Operations,
            PipelineCache, RenderPassColorAttachment, RenderPassDescriptor, StoreOp,
            TextureDescriptor, TextureDimension, TextureFormat, TextureUsages, TextureView,
            TextureViewDescriptor, WgpuFeatures,
        },
        renderer::{RenderContext, RenderDevice, RenderQueue, render_system},
        view::{prepare_view_attachments, prepare_view_uniforms, window::prepare_windows},
    },
};
use rumpel_player::{Player, PlayerPhysics};
use rumpel_render::{
    RenderedChunkCount,
    packed_quad_pipeline::{PackedQuadPipelineStats, snapshot_packed_quad_metrics},
    surface_streaming::SurfaceStreamingMetrics,
};
use std::{
    io::Write,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Instant,
};
use wgpu::{QuerySet, QuerySetDescriptor, QueryType, RenderPassTimestampWrites};

const PROFILE_SECONDS_ENV: &str = "RUMPEL_PROFILE_SECONDS";
const PROFILE_AUTOPILOT_ENV: &str = "RUMPEL_PROFILE_AUTOPILOT";
const PROFILE_LOG_INTERVAL_ENV: &str = "RUMPEL_PROFILE_LOG_INTERVAL";
const PROFILE_SLOW_FRAME_MS_ENV: &str = "RUMPEL_PROFILE_SLOW_FRAME_MS";
const PROFILE_WARMUP_SECONDS_ENV: &str = "RUMPEL_PROFILE_WARMUP_SECONDS";
const PROFILE_READY_GATE_ENV: &str = "RUMPEL_PROFILE_READY_GATE";
const PROFILE_READY_STABLE_FRAMES_ENV: &str = "RUMPEL_PROFILE_READY_STABLE_FRAMES";
const PROFILE_READY_FRAME_MS_ENV: &str = "RUMPEL_PROFILE_READY_FRAME_MS";
const PROFILE_READY_MAX_EXTRA_SECONDS_ENV: &str = "RUMPEL_PROFILE_READY_MAX_EXTRA_SECONDS";
const PROFILE_AUTOPILOT_PREROLL_SECONDS_ENV: &str = "RUMPEL_PROFILE_AUTOPILOT_PREROLL_SECONDS";
const PROFILE_SETTLE_SECONDS_ENV: &str = "RUMPEL_PROFILE_SETTLE_SECONDS";
const CAMERA_LOCK_ENV: &str = "RUMPEL_CAMERA_LOCK";
const PACKED_CAMERA_LOCK_ENV: &str = "RUMPEL_PACKED_CAMERA_LOCK";
const PACKED_FACE_RANGE_CULL_ENV: &str = "RUMPEL_PACKED_FACE_RANGE_CULL";
const PACKED_FACE_RANGE_MIN_QUADS_ENV: &str = "RUMPEL_PACKED_FACE_RANGE_MIN_QUADS";
const PRESENT_MODE_ENV: &str = "RUMPEL_PRESENT_MODE";
const FRAME_LATENCY_ENV: &str = "RUMPEL_FRAME_LATENCY";
const WINDOW_WIDTH_ENV: &str = "RUMPEL_WINDOW_WIDTH";
const WINDOW_HEIGHT_ENV: &str = "RUMPEL_WINDOW_HEIGHT";
const SHADOWS_ENV: &str = "RUMPEL_SHADOWS";
const DEBUG_HUD_ENV: &str = "RUMPEL_DEBUG_HUD";
const HEADLESS_RENDER_ENV: &str = "RUMPEL_HEADLESS_RENDER";
const HEADLESS_WAIT_MS_ENV: &str = "RUMPEL_HEADLESS_WAIT_MS";
const RENDER_GPU_FRAME_TIMESTAMPS_ENV: &str = "RUMPEL_RENDER_GPU_FRAME_TIMESTAMPS";
const DEFAULT_PRESENT_MODE: &str = "immediate";
const DEFAULT_FRAME_LATENCY: &str = "1";
const DEFAULT_HEADLESS_WAIT_MS: &str = "0";
const DEFAULT_PROFILE_LOG_INTERVAL_SECONDS: f32 = 1.0;
const DEFAULT_PROFILE_SLOW_FRAME_MS: f32 = 25.0;
const DEFAULT_PROFILE_WARMUP_SECONDS: f32 = 4.0;
const DEFAULT_PROFILE_READY_GATE: bool = false;
const DEFAULT_PROFILE_READY_STABLE_FRAMES: u32 = 30;
const DEFAULT_PROFILE_READY_FRAME_MS: f32 = 25.0;
const DEFAULT_PROFILE_READY_MAX_EXTRA_SECONDS: f32 = 8.0;
const DEFAULT_PROFILE_AUTOPILOT_PREROLL_SECONDS: f32 = 2.0;
const DEFAULT_PROFILE_SETTLE_SECONDS: f32 = 0.0;
const DEFAULT_PACKED_FACE_RANGE_CULL: bool = true;
const DEFAULT_PACKED_FACE_RANGE_MIN_QUADS: usize = 4096;
const FRAME_BUDGET_60HZ_MS: f32 = 16.666_668;
const FRAME_BUDGET_40HZ_MS: f32 = 25.0;
const FRAME_BUDGET_30HZ_MS: f32 = 33.333_336;
const AUTOPILOT_SPEED: f32 = 80.0;
const AUTOPILOT_SURFACE_CLEARANCE: f32 = 18.0;
const RENDER_GPU_FRAME_TIMESTAMP_QUERY_COUNT: u32 = 2;
const RENDER_GPU_FRAME_TIMESTAMP_BUFFER_SIZE: u64 = 16;
static RENDER_SCHEDULE_US: AtomicU64 = AtomicU64::new(0);
static RENDER_EXTRACT_COMMANDS_US: AtomicU64 = AtomicU64::new(0);
static RENDER_PREPARE_ASSETS_US: AtomicU64 = AtomicU64::new(0);
static RENDER_PREPARE_MESHES_US: AtomicU64 = AtomicU64::new(0);
static RENDER_MANAGE_VIEWS_US: AtomicU64 = AtomicU64::new(0);
static RENDER_PREPARE_WINDOWS_US: AtomicU64 = AtomicU64::new(0);
static MAX_RENDER_PREPARE_WINDOWS_US: AtomicU64 = AtomicU64::new(0);
static RENDER_QUEUE_US: AtomicU64 = AtomicU64::new(0);
static RENDER_PHASE_SORT_US: AtomicU64 = AtomicU64::new(0);
static RENDER_PREPARE_US: AtomicU64 = AtomicU64::new(0);
static RENDER_PREPARE_RESOURCES_US: AtomicU64 = AtomicU64::new(0);
static RENDER_PREPARE_VIEW_UNIFORMS_US: AtomicU64 = AtomicU64::new(0);
static RENDER_PREPARE_CORE_DEPTH_TEXTURES_US: AtomicU64 = AtomicU64::new(0);
static RENDER_PREPARE_CORE_TRANSMISSION_TEXTURES_US: AtomicU64 = AtomicU64::new(0);
static RENDER_PREPARE_PREPASS_TEXTURES_US: AtomicU64 = AtomicU64::new(0);
static RENDER_PREPARE_RESOURCES_COLLECT_US: AtomicU64 = AtomicU64::new(0);
static RENDER_PREPARE_RESOURCES_FLUSH_US: AtomicU64 = AtomicU64::new(0);
static RENDER_PREPARE_BIND_GROUPS_US: AtomicU64 = AtomicU64::new(0);
static RENDER_PREPARE_AFTER_BIND_GROUPS_US: AtomicU64 = AtomicU64::new(0);
static RENDER_RENDER_US: AtomicU64 = AtomicU64::new(0);
static RENDER_BEFORE_RENDER_SYSTEM_US: AtomicU64 = AtomicU64::new(0);
static RENDER_SYSTEM_US: AtomicU64 = AtomicU64::new(0);
static RENDER_CLEANUP_US: AtomicU64 = AtomicU64::new(0);
static RENDER_POST_CLEANUP_US: AtomicU64 = AtomicU64::new(0);
static RENDER_CAMERA_DRIVER_US: AtomicU64 = AtomicU64::new(0);
static RENDER_CORE3D_GRAPH_US: AtomicU64 = AtomicU64::new(0);
static RENDER_GPU_CAMERA_DRIVER_US: AtomicU64 = AtomicU64::new(0);
static RENDER_GPU_CAMERA_DRIVER_RAW_BEGIN: AtomicU64 = AtomicU64::new(0);
static RENDER_GPU_CAMERA_DRIVER_RAW_END: AtomicU64 = AtomicU64::new(0);
static RENDER_GPU_CAMERA_DRIVER_RAW_DELTA: AtomicU64 = AtomicU64::new(0);
static RENDER_GPU_CAMERA_DRIVER_READBACKS: AtomicU64 = AtomicU64::new(0);
static RENDER_GPU_CAMERA_DRIVER_ZERO_DELTAS: AtomicU64 = AtomicU64::new(0);
static RENDER_GPU_CAMERA_DRIVER_MAP_FAILURES: AtomicU64 = AtomicU64::new(0);
static RENDER_GPU_FRAME_TIMESTAMPS_REQUESTED: AtomicBool = AtomicBool::new(false);
static RENDER_GPU_FRAME_TIMESTAMPS_SUPPORTED: AtomicBool = AtomicBool::new(false);
static RENDER_GPU_CAMERA_DRIVER_PENDING_READBACK: AtomicBool = AtomicBool::new(false);
static RENDER_GPU_CAMERA_DRIVER_MAP_REQUESTED: AtomicBool = AtomicBool::new(false);
static RENDER_CAMERA_DRIVER_STARTED_AT: Mutex<Option<Instant>> = Mutex::new(None);
static RENDER_CORE3D_GRAPH_STARTED_AT: Mutex<Option<Instant>> = Mutex::new(None);

pub struct RumpelClientProfilingPlugin;

impl Plugin for RumpelClientProfilingPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ProfilingRun>()
            .init_resource::<MainFramePhaseProbe>()
            .add_systems(Startup, announce_profiling_run)
            .add_systems(First, mark_main_frame_start)
            .add_systems(Update, (profile_autopilot, log_profile_metrics).chain());

        app.add_systems(Last, mark_main_frame_end);

        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        render_app
            .init_resource::<RenderGpuFrameTimestampProfiler>()
            .init_resource::<RenderFramePhaseProbe>()
            .add_systems(
                Render,
                (
                    initialize_render_gpu_frame_timestamp_profiler.before(RenderSystems::Render),
                    mark_render_schedule_start.before(RenderSystems::ExtractCommands),
                    mark_render_after_extract_commands
                        .after(RenderSystems::ExtractCommands)
                        .before(RenderSystems::PrepareAssets),
                    mark_render_after_prepare_assets
                        .after(RenderSystems::PrepareAssets)
                        .before(RenderSystems::PrepareMeshes),
                    mark_render_after_prepare_meshes
                        .after(RenderSystems::PrepareMeshes)
                        .before(RenderSystems::ManageViews),
                    mark_render_prepare_windows_start
                        .in_set(RenderSystems::ManageViews)
                        .before(prepare_windows),
                    mark_render_prepare_windows_end
                        .in_set(RenderSystems::ManageViews)
                        .after(prepare_windows)
                        .before(prepare_view_attachments),
                    mark_render_after_manage_views
                        .after(RenderSystems::ManageViews)
                        .before(RenderSystems::Queue),
                    mark_render_after_queue
                        .after(RenderSystems::Queue)
                        .before(RenderSystems::PhaseSort),
                    mark_render_after_phase_sort
                        .after(RenderSystems::PhaseSort)
                        .before(RenderSystems::Prepare),
                    mark_render_after_prepare_resources
                        .after(RenderSystems::PrepareResources)
                        .before(RenderSystems::PrepareResourcesCollectPhaseBuffers),
                    mark_render_after_prepare_resources_collect
                        .after(RenderSystems::PrepareResourcesCollectPhaseBuffers)
                        .before(RenderSystems::PrepareResourcesFlush),
                    mark_render_after_prepare_resources_flush
                        .after(RenderSystems::PrepareResourcesFlush)
                        .before(RenderSystems::PrepareBindGroups),
                    mark_render_after_prepare_bind_groups
                        .in_set(RenderSystems::Prepare)
                        .after(RenderSystems::PrepareBindGroups),
                    mark_render_after_prepare
                        .after(RenderSystems::Prepare)
                        .before(RenderSystems::Render),
                    mark_render_system_start
                        .in_set(RenderSystems::Render)
                        .before(render_system),
                    mark_render_system_end
                        .in_set(RenderSystems::Render)
                        .after(render_system),
                    mark_render_after_render
                        .after(RenderSystems::Render)
                        .before(RenderSystems::Cleanup),
                    mark_render_after_cleanup
                        .after(RenderSystems::Cleanup)
                        .before(RenderSystems::PostCleanup),
                    mark_render_schedule_end.after(RenderSystems::PostCleanup),
                ),
            );
        render_app.add_systems(
            Render,
            (
                mark_render_prepare_view_uniforms_start
                    .in_set(RenderSystems::PrepareResources)
                    .before(prepare_view_uniforms),
                mark_render_prepare_view_uniforms_end
                    .in_set(RenderSystems::PrepareResources)
                    .after(prepare_view_uniforms),
                mark_render_prepare_core_depth_textures_start
                    .in_set(RenderSystems::PrepareResources)
                    .before(prepare_core_3d_depth_textures),
                mark_render_prepare_core_depth_textures_end
                    .in_set(RenderSystems::PrepareResources)
                    .after(prepare_core_3d_depth_textures),
                mark_render_prepare_core_transmission_textures_start
                    .in_set(RenderSystems::PrepareResources)
                    .before(prepare_core_3d_transmission_textures),
                mark_render_prepare_core_transmission_textures_end
                    .in_set(RenderSystems::PrepareResources)
                    .after(prepare_core_3d_transmission_textures),
                mark_render_prepare_prepass_textures_start
                    .in_set(RenderSystems::PrepareResources)
                    .before(prepare_prepass_textures),
                mark_render_prepare_prepass_textures_end
                    .in_set(RenderSystems::PrepareResources)
                    .after(prepare_prepass_textures),
            ),
        );

        let mut render_graph = render_app.world_mut().resource_mut::<RenderGraph>();
        render_graph.add_node(CameraDriverProfileStartLabel, CameraDriverProfileStartNode);
        render_graph.add_node(CameraDriverProfileEndLabel, CameraDriverProfileEndNode);
        render_graph.add_node_edge(CameraDriverProfileStartLabel, CameraDriverLabel);
        render_graph.add_node_edge(CameraDriverLabel, CameraDriverProfileEndLabel);

        if let Some(core_3d_graph) = render_graph.get_sub_graph_mut(Core3d) {
            core_3d_graph.add_node(Core3dProfileStartLabel, Core3dProfileStartNode);
            core_3d_graph.add_node(Core3dProfileEndLabel, Core3dProfileEndNode);
            core_3d_graph.add_node_edge(Core3dProfileStartLabel, Node3d::EarlyPrepass);
            core_3d_graph.add_node_edge(Node3d::Upscaling, Core3dProfileEndLabel);
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct ProfilePhaseStats {
    frame_wall_us: u64,
    main_schedule_us: u64,
    main_tail_us: u64,
    render_schedule_us: u64,
    render_extract_commands_us: u64,
    render_prepare_assets_us: u64,
    render_prepare_meshes_us: u64,
    render_manage_views_us: u64,
    render_prepare_windows_us: u64,
    render_queue_us: u64,
    render_phase_sort_us: u64,
    render_prepare_us: u64,
    render_prepare_resources_us: u64,
    render_prepare_view_uniforms_us: u64,
    render_prepare_core_depth_textures_us: u64,
    render_prepare_core_transmission_textures_us: u64,
    render_prepare_prepass_textures_us: u64,
    render_prepare_resources_collect_us: u64,
    render_prepare_resources_flush_us: u64,
    render_prepare_bind_groups_us: u64,
    render_prepare_after_bind_groups_us: u64,
    render_render_us: u64,
    render_before_render_system_us: u64,
    render_system_us: u64,
    render_cleanup_us: u64,
    render_post_cleanup_us: u64,
    render_camera_driver_us: u64,
    render_gpu_camera_driver_us: u64,
    render_gpu_camera_driver_raw_begin: u64,
    render_gpu_camera_driver_raw_end: u64,
    render_gpu_camera_driver_raw_delta: u64,
    render_gpu_camera_driver_readbacks: u64,
    render_gpu_camera_driver_zero_deltas: u64,
    render_gpu_camera_driver_map_failures: u64,
    render_gpu_frame_timestamps_requested: bool,
    render_gpu_frame_timestamps_supported: bool,
    render_gpu_camera_driver_pending_readback: bool,
    render_gpu_camera_driver_map_requested: bool,
    render_core3d_us: u64,
}

#[derive(Resource)]
struct RenderGpuFrameTimestampProfiler {
    initialized: bool,
    enabled: bool,
    supported: bool,
    query_set: Option<QuerySet>,
    resolve_buffer: Option<Buffer>,
    readback_buffer: Option<Buffer>,
    marker_texture_view: Option<TextureView>,
    timestamp_period_ns: f32,
    pending_readback: Arc<AtomicBool>,
    map_requested: Arc<AtomicBool>,
    mapped_readback: Arc<AtomicBool>,
    active_query: AtomicBool,
}

impl Default for RenderGpuFrameTimestampProfiler {
    fn default() -> Self {
        Self::uninitialized()
    }
}

fn reset_render_gpu_frame_timestamp_stats() {
    RENDER_GPU_CAMERA_DRIVER_US.store(0, Ordering::Relaxed);
    RENDER_GPU_CAMERA_DRIVER_RAW_BEGIN.store(0, Ordering::Relaxed);
    RENDER_GPU_CAMERA_DRIVER_RAW_END.store(0, Ordering::Relaxed);
    RENDER_GPU_CAMERA_DRIVER_RAW_DELTA.store(0, Ordering::Relaxed);
    RENDER_GPU_CAMERA_DRIVER_READBACKS.store(0, Ordering::Relaxed);
    RENDER_GPU_CAMERA_DRIVER_ZERO_DELTAS.store(0, Ordering::Relaxed);
    RENDER_GPU_CAMERA_DRIVER_MAP_FAILURES.store(0, Ordering::Relaxed);
    RENDER_GPU_CAMERA_DRIVER_PENDING_READBACK.store(false, Ordering::Relaxed);
    RENDER_GPU_CAMERA_DRIVER_MAP_REQUESTED.store(false, Ordering::Relaxed);
}

impl RenderGpuFrameTimestampProfiler {
    fn uninitialized() -> Self {
        Self {
            initialized: false,
            enabled: false,
            supported: false,
            query_set: None,
            resolve_buffer: None,
            readback_buffer: None,
            marker_texture_view: None,
            timestamp_period_ns: 0.0,
            pending_readback: Arc::new(AtomicBool::new(false)),
            map_requested: Arc::new(AtomicBool::new(false)),
            mapped_readback: Arc::new(AtomicBool::new(false)),
            active_query: AtomicBool::new(false),
        }
    }

    fn new(render_device: &RenderDevice, render_queue: &RenderQueue) -> Self {
        let enabled = env_flag(RENDER_GPU_FRAME_TIMESTAMPS_ENV);
        let features = render_device.features();
        let supported = enabled && features.contains(WgpuFeatures::TIMESTAMP_QUERY);
        RENDER_GPU_FRAME_TIMESTAMPS_REQUESTED.store(enabled, Ordering::Relaxed);
        RENDER_GPU_FRAME_TIMESTAMPS_SUPPORTED.store(supported, Ordering::Relaxed);
        reset_render_gpu_frame_timestamp_stats();

        if !enabled {
            return Self::disabled(false);
        }

        if !supported {
            info!(
                "CLIENT PROFILE: RUMPEL_RENDER_GPU_FRAME_TIMESTAMPS requested, but TIMESTAMP_QUERY is not enabled on this device"
            );
            return Self::disabled(true);
        }

        let query_set = render_device
            .wgpu_device()
            .create_query_set(&QuerySetDescriptor {
                label: Some("rumpel_render_gpu_frame_timestamp_query_set"),
                ty: QueryType::Timestamp,
                count: RENDER_GPU_FRAME_TIMESTAMP_QUERY_COUNT,
            });
        let resolve_buffer = render_device.create_buffer(&BufferDescriptor {
            label: Some("rumpel_render_gpu_frame_timestamp_resolve_buffer"),
            size: RENDER_GPU_FRAME_TIMESTAMP_BUFFER_SIZE,
            usage: BufferUsages::QUERY_RESOLVE | BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let readback_buffer = render_device.create_buffer(&BufferDescriptor {
            label: Some("rumpel_render_gpu_frame_timestamp_readback_buffer"),
            size: RENDER_GPU_FRAME_TIMESTAMP_BUFFER_SIZE,
            usage: BufferUsages::MAP_READ | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let marker_texture = render_device.create_texture(&TextureDescriptor {
            label: Some("rumpel_render_gpu_frame_timestamp_marker_texture"),
            size: Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: TextureFormat::Rgba8Unorm,
            usage: TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let marker_texture_view = marker_texture.create_view(&TextureViewDescriptor::default());

        info!(
            timestamp_period_ns = render_queue.get_timestamp_period(),
            "CLIENT PROFILE: GPU CameraDriver timestamp profiling enabled with render pass markers"
        );

        Self {
            initialized: true,
            enabled: true,
            supported: true,
            query_set: Some(query_set),
            resolve_buffer: Some(resolve_buffer),
            readback_buffer: Some(readback_buffer),
            marker_texture_view: Some(marker_texture_view),
            timestamp_period_ns: render_queue.get_timestamp_period(),
            pending_readback: Arc::new(AtomicBool::new(false)),
            map_requested: Arc::new(AtomicBool::new(false)),
            mapped_readback: Arc::new(AtomicBool::new(false)),
            active_query: AtomicBool::new(false),
        }
    }

    fn disabled(requested: bool) -> Self {
        reset_render_gpu_frame_timestamp_stats();
        Self {
            initialized: true,
            enabled: requested,
            supported: false,
            query_set: None,
            resolve_buffer: None,
            readback_buffer: None,
            marker_texture_view: None,
            timestamp_period_ns: 0.0,
            pending_readback: Arc::new(AtomicBool::new(false)),
            map_requested: Arc::new(AtomicBool::new(false)),
            mapped_readback: Arc::new(AtomicBool::new(false)),
            active_query: AtomicBool::new(false),
        }
    }

    fn initialize_if_needed(&mut self, render_device: &RenderDevice, render_queue: &RenderQueue) {
        if self.initialized {
            return;
        }
        *self = Self::new(render_device, render_queue);
    }

    fn collect_mapped_result(&self) {
        if !self.enabled || !self.supported {
            return;
        }

        if !self.mapped_readback.swap(false, Ordering::AcqRel) {
            self.request_timestamp_map();
            return;
        }

        let Some(readback_buffer) = &self.readback_buffer else {
            self.pending_readback.store(false, Ordering::Release);
            self.map_requested.store(false, Ordering::Release);
            RENDER_GPU_CAMERA_DRIVER_PENDING_READBACK.store(false, Ordering::Release);
            RENDER_GPU_CAMERA_DRIVER_MAP_REQUESTED.store(false, Ordering::Release);
            return;
        };

        let data = readback_buffer.slice(..).get_mapped_range();
        if data.len() >= RENDER_GPU_FRAME_TIMESTAMP_BUFFER_SIZE as usize {
            let begin = u64::from_le_bytes(data[0..8].try_into().expect("timestamp begin bytes"));
            let end = u64::from_le_bytes(data[8..16].try_into().expect("timestamp end bytes"));
            if end >= begin {
                let delta = end - begin;
                let elapsed_us = (delta as f64 * f64::from(self.timestamp_period_ns) / 1_000.0)
                    .round()
                    .clamp(0.0, u64::MAX as f64) as u64;
                RENDER_GPU_CAMERA_DRIVER_RAW_BEGIN.store(begin, Ordering::Relaxed);
                RENDER_GPU_CAMERA_DRIVER_RAW_END.store(end, Ordering::Relaxed);
                RENDER_GPU_CAMERA_DRIVER_RAW_DELTA.store(delta, Ordering::Relaxed);
                RENDER_GPU_CAMERA_DRIVER_READBACKS.fetch_add(1, Ordering::Relaxed);
                if delta == 0 {
                    RENDER_GPU_CAMERA_DRIVER_ZERO_DELTAS.fetch_add(1, Ordering::Relaxed);
                }
                RENDER_GPU_CAMERA_DRIVER_US.store(elapsed_us, Ordering::Relaxed);
            }
        }
        drop(data);
        readback_buffer.unmap();
        self.map_requested.store(false, Ordering::Release);
        self.pending_readback.store(false, Ordering::Release);
        RENDER_GPU_CAMERA_DRIVER_MAP_REQUESTED.store(false, Ordering::Release);
        RENDER_GPU_CAMERA_DRIVER_PENDING_READBACK.store(false, Ordering::Release);
    }

    fn request_timestamp_map(&self) {
        if !self.pending_readback.load(Ordering::Acquire)
            || self
                .map_requested
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                .is_err()
        {
            return;
        }

        let Some(readback_buffer) = &self.readback_buffer else {
            self.map_requested.store(false, Ordering::Release);
            self.pending_readback.store(false, Ordering::Release);
            RENDER_GPU_CAMERA_DRIVER_MAP_REQUESTED.store(false, Ordering::Release);
            RENDER_GPU_CAMERA_DRIVER_PENDING_READBACK.store(false, Ordering::Release);
            return;
        };
        RENDER_GPU_CAMERA_DRIVER_MAP_REQUESTED.store(true, Ordering::Release);

        let mapped_readback = Arc::clone(&self.mapped_readback);
        let map_requested = Arc::clone(&self.map_requested);
        let pending_readback = Arc::clone(&self.pending_readback);
        readback_buffer
            .slice(..)
            .map_async(MapMode::Read, move |result| {
                if let Err(error) = result {
                    warn!("CLIENT PROFILE: GPU frame timestamp readback failed: {error}");
                    RENDER_GPU_CAMERA_DRIVER_MAP_FAILURES.fetch_add(1, Ordering::Relaxed);
                    map_requested.store(false, Ordering::Release);
                    pending_readback.store(false, Ordering::Release);
                    RENDER_GPU_CAMERA_DRIVER_MAP_REQUESTED.store(false, Ordering::Release);
                    RENDER_GPU_CAMERA_DRIVER_PENDING_READBACK.store(false, Ordering::Release);
                    return;
                }
                mapped_readback.store(true, Ordering::Release);
            });
    }

    fn begin_camera_driver_query(&self, render_context: &mut RenderContext) {
        self.active_query.store(false, Ordering::Release);
        self.collect_mapped_result();
        if !self.enabled
            || !self.supported
            || self
                .pending_readback
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                .is_err()
        {
            return;
        }

        let (Some(query_set), Some(marker_texture_view)) =
            (&self.query_set, &self.marker_texture_view)
        else {
            self.pending_readback.store(false, Ordering::Release);
            RENDER_GPU_CAMERA_DRIVER_PENDING_READBACK.store(false, Ordering::Release);
            return;
        };
        {
            let _marker_pass =
                render_context
                    .command_encoder()
                    .begin_render_pass(&RenderPassDescriptor {
                        label: Some("rumpel_gpu_camera_driver_begin_marker"),
                        color_attachments: &[Some(RenderPassColorAttachment {
                            view: marker_texture_view,
                            depth_slice: None,
                            resolve_target: None,
                            ops: Operations {
                                load: LoadOp::Clear(wgpu::Color::BLACK),
                                store: StoreOp::Store,
                            },
                        })],
                        depth_stencil_attachment: None,
                        timestamp_writes: Some(RenderPassTimestampWrites {
                            query_set,
                            beginning_of_pass_write_index: Some(0),
                            end_of_pass_write_index: None,
                        }),
                        occlusion_query_set: None,
                    });
        }
        RENDER_GPU_CAMERA_DRIVER_PENDING_READBACK.store(true, Ordering::Release);
        self.active_query.store(true, Ordering::Release);
    }

    fn finish_camera_driver_query(&self, render_context: &mut RenderContext) {
        if !self.active_query.swap(false, Ordering::AcqRel) {
            return;
        }
        let (
            Some(query_set),
            Some(resolve_buffer),
            Some(readback_buffer),
            Some(marker_texture_view),
        ) = (
            &self.query_set,
            &self.resolve_buffer,
            &self.readback_buffer,
            &self.marker_texture_view,
        )
        else {
            self.pending_readback.store(false, Ordering::Release);
            RENDER_GPU_CAMERA_DRIVER_PENDING_READBACK.store(false, Ordering::Release);
            return;
        };

        let encoder = render_context.command_encoder();
        {
            let _marker_pass = encoder.begin_render_pass(&RenderPassDescriptor {
                label: Some("rumpel_gpu_camera_driver_end_marker"),
                color_attachments: &[Some(RenderPassColorAttachment {
                    view: marker_texture_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: Operations {
                        load: LoadOp::Load,
                        store: StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: Some(RenderPassTimestampWrites {
                    query_set,
                    beginning_of_pass_write_index: None,
                    end_of_pass_write_index: Some(1),
                }),
                occlusion_query_set: None,
            });
        }
        encoder.resolve_query_set(
            query_set,
            0..RENDER_GPU_FRAME_TIMESTAMP_QUERY_COUNT,
            resolve_buffer,
            0,
        );
        encoder.copy_buffer_to_buffer(
            resolve_buffer,
            0,
            readback_buffer,
            0,
            RENDER_GPU_FRAME_TIMESTAMP_BUFFER_SIZE,
        );
    }
}

#[derive(Clone, Eq, PartialEq, Hash, Debug, RenderLabel)]
struct CameraDriverProfileStartLabel;

#[derive(Clone, Eq, PartialEq, Hash, Debug, RenderLabel)]
pub struct CameraDriverProfileEndLabel;

#[derive(Clone, Eq, PartialEq, Hash, Debug, RenderLabel)]
struct Core3dProfileStartLabel;

#[derive(Clone, Eq, PartialEq, Hash, Debug, RenderLabel)]
struct Core3dProfileEndLabel;

struct CameraDriverProfileStartNode;

impl Node for CameraDriverProfileStartNode {
    fn run(
        &self,
        _graph: &mut RenderGraphContext,
        render_context: &mut RenderContext,
        world: &World,
    ) -> Result<(), NodeRunError> {
        *RENDER_CAMERA_DRIVER_STARTED_AT
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(Instant::now());
        if let Some(profiler) = world.get_resource::<RenderGpuFrameTimestampProfiler>() {
            profiler.begin_camera_driver_query(render_context);
        }
        Ok(())
    }
}

struct CameraDriverProfileEndNode;

impl Node for CameraDriverProfileEndNode {
    fn run(
        &self,
        _graph: &mut RenderGraphContext,
        render_context: &mut RenderContext,
        world: &World,
    ) -> Result<(), NodeRunError> {
        let started_at = RENDER_CAMERA_DRIVER_STARTED_AT
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take();
        store_render_phase_metric(&RENDER_CAMERA_DRIVER_US, started_at);
        if let Some(profiler) = world.get_resource::<RenderGpuFrameTimestampProfiler>() {
            profiler.finish_camera_driver_query(render_context);
        }
        Ok(())
    }
}

struct Core3dProfileStartNode;

impl Node for Core3dProfileStartNode {
    fn run(
        &self,
        _graph: &mut RenderGraphContext,
        _render_context: &mut RenderContext,
        _world: &World,
    ) -> Result<(), NodeRunError> {
        *RENDER_CORE3D_GRAPH_STARTED_AT
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(Instant::now());
        Ok(())
    }
}

struct Core3dProfileEndNode;

impl Node for Core3dProfileEndNode {
    fn run(
        &self,
        _graph: &mut RenderGraphContext,
        _render_context: &mut RenderContext,
        _world: &World,
    ) -> Result<(), NodeRunError> {
        let started_at = RENDER_CORE3D_GRAPH_STARTED_AT
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take();
        store_render_phase_metric(&RENDER_CORE3D_GRAPH_US, started_at);
        Ok(())
    }
}

#[derive(Resource, Default)]
struct MainFramePhaseProbe {
    current_frame_started_at: Option<Instant>,
    previous_main_finished_at: Option<Instant>,
    previous_main_schedule_us: u64,
    completed: ProfilePhaseStats,
}

#[derive(Resource, Default)]
struct RenderFramePhaseProbe {
    schedule_started_at: Option<Instant>,
    phase_started_at: Option<Instant>,
    prepare_segment_started_at: Option<Instant>,
    prepare_view_uniforms_started_at: Option<Instant>,
    prepare_core_depth_textures_started_at: Option<Instant>,
    prepare_core_transmission_textures_started_at: Option<Instant>,
    prepare_prepass_textures_started_at: Option<Instant>,
    prepare_windows_started_at: Option<Instant>,
    render_system_started_at: Option<Instant>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ProfileReadyStatus {
    Waiting,
    Ready,
    TimedOut,
}

impl ProfileReadyStatus {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Waiting => "waiting",
            Self::Ready => "ready",
            Self::TimedOut => "timeout",
        }
    }
}

#[derive(Resource)]
struct ProfilingRun {
    enabled: bool,
    autopilot: bool,
    duration_seconds: f32,
    log_interval_seconds: f32,
    slow_frame_threshold_ms: f32,
    warmup_seconds: f32,
    ready_gate: bool,
    ready_stable_frames_required: u32,
    ready_frame_ms: f32,
    ready_max_extra_seconds: f32,
    ready_stable_frames: u32,
    ready_status: ProfileReadyStatus,
    ready_seconds: f32,
    autopilot_preroll_started: bool,
    autopilot_preroll_start_seconds: f32,
    autopilot_preroll_seconds: f32,
    settle_seconds: f32,
    counting_started_logged: bool,
    measurement_started: bool,
    measurement_start_seconds: f32,
    measurement_target_seconds: f32,
    elapsed_seconds: f32,
    next_log_seconds: f32,
    sample_count: u32,
    min_fps: f64,
    min_raw_fps: f32,
    measured_frame_count: u64,
    measured_frame_ms_sum: f64,
    frames_ge_16ms: u64,
    frames_ge_25ms: u64,
    frames_ge_33ms: u64,
    interval_frames_ge_16ms: u32,
    interval_frames_ge_25ms: u32,
    interval_frames_ge_33ms: u32,
    interval_worst_frame_ms: f32,
    interval_worst_frame_t: f32,
    interval_worst_phase_stats: ProfilePhaseStats,
    interval_worst_packed_stats: Option<PackedQuadPipelineStats>,
    worst_frame_ms: f32,
    worst_frame_t: f32,
    worst_phase_stats: ProfilePhaseStats,
    worst_packed_stats: Option<PackedQuadPipelineStats>,
    finished: bool,
    last_surface_totals: ProfileSurfaceTotals,
    total_surface_uploaded_bytes: u64,
    last_total_surface_uploaded_bytes: u64,
    total_packed_uploaded_bytes: u64,
    last_total_packed_uploaded_bytes: u64,
    last_log_seconds: f32,
}

impl Default for ProfilingRun {
    fn default() -> Self {
        let duration_seconds = std::env::var(PROFILE_SECONDS_ENV)
            .ok()
            .and_then(|value| value.parse::<f32>().ok())
            .unwrap_or(0.0)
            .max(0.0);
        let log_interval_seconds = std::env::var(PROFILE_LOG_INTERVAL_ENV)
            .ok()
            .and_then(|value| value.parse::<f32>().ok())
            .unwrap_or(DEFAULT_PROFILE_LOG_INTERVAL_SECONDS)
            .max(0.1);
        let slow_frame_threshold_ms = std::env::var(PROFILE_SLOW_FRAME_MS_ENV)
            .ok()
            .and_then(|value| value.parse::<f32>().ok())
            .unwrap_or(DEFAULT_PROFILE_SLOW_FRAME_MS)
            .max(0.0);
        let warmup_seconds = std::env::var(PROFILE_WARMUP_SECONDS_ENV)
            .ok()
            .and_then(|value| value.parse::<f32>().ok())
            .unwrap_or(DEFAULT_PROFILE_WARMUP_SECONDS)
            .max(0.0);
        let ready_gate = env_flag_default(PROFILE_READY_GATE_ENV, DEFAULT_PROFILE_READY_GATE);
        let ready_stable_frames_required = std::env::var(PROFILE_READY_STABLE_FRAMES_ENV)
            .ok()
            .and_then(|value| value.parse::<u32>().ok())
            .unwrap_or(DEFAULT_PROFILE_READY_STABLE_FRAMES)
            .max(1);
        let ready_frame_ms = std::env::var(PROFILE_READY_FRAME_MS_ENV)
            .ok()
            .and_then(|value| value.parse::<f32>().ok())
            .unwrap_or(DEFAULT_PROFILE_READY_FRAME_MS)
            .max(0.0);
        let ready_max_extra_seconds = std::env::var(PROFILE_READY_MAX_EXTRA_SECONDS_ENV)
            .ok()
            .and_then(|value| value.parse::<f32>().ok())
            .unwrap_or(DEFAULT_PROFILE_READY_MAX_EXTRA_SECONDS)
            .max(0.0);
        let autopilot_preroll_seconds = std::env::var(PROFILE_AUTOPILOT_PREROLL_SECONDS_ENV)
            .ok()
            .and_then(|value| value.parse::<f32>().ok())
            .unwrap_or(DEFAULT_PROFILE_AUTOPILOT_PREROLL_SECONDS)
            .max(0.0);
        let settle_seconds = std::env::var(PROFILE_SETTLE_SECONDS_ENV)
            .ok()
            .and_then(|value| value.parse::<f32>().ok())
            .unwrap_or(DEFAULT_PROFILE_SETTLE_SECONDS)
            .max(0.0);
        let measurement_target_seconds =
            (duration_seconds - warmup_seconds - settle_seconds).max(0.0);
        let camera_lock = camera_lock_enabled();

        Self {
            enabled: duration_seconds > 0.0,
            autopilot: env_flag(PROFILE_AUTOPILOT_ENV) && !camera_lock,
            duration_seconds,
            log_interval_seconds,
            slow_frame_threshold_ms,
            warmup_seconds,
            ready_gate,
            ready_stable_frames_required,
            ready_frame_ms,
            ready_max_extra_seconds,
            ready_stable_frames: 0,
            ready_status: ProfileReadyStatus::Waiting,
            ready_seconds: 0.0,
            autopilot_preroll_started: false,
            autopilot_preroll_start_seconds: 0.0,
            autopilot_preroll_seconds,
            settle_seconds,
            counting_started_logged: false,
            measurement_started: false,
            measurement_start_seconds: 0.0,
            measurement_target_seconds,
            elapsed_seconds: 0.0,
            next_log_seconds: 0.0,
            sample_count: 0,
            min_fps: f64::MAX,
            min_raw_fps: f32::MAX,
            measured_frame_count: 0,
            measured_frame_ms_sum: 0.0,
            frames_ge_16ms: 0,
            frames_ge_25ms: 0,
            frames_ge_33ms: 0,
            interval_frames_ge_16ms: 0,
            interval_frames_ge_25ms: 0,
            interval_frames_ge_33ms: 0,
            interval_worst_frame_ms: 0.0,
            interval_worst_frame_t: 0.0,
            interval_worst_phase_stats: ProfilePhaseStats::default(),
            interval_worst_packed_stats: None,
            worst_frame_ms: 0.0,
            worst_frame_t: 0.0,
            worst_phase_stats: ProfilePhaseStats::default(),
            worst_packed_stats: None,
            finished: false,
            last_surface_totals: ProfileSurfaceTotals::default(),
            total_surface_uploaded_bytes: 0,
            last_total_surface_uploaded_bytes: 0,
            total_packed_uploaded_bytes: 0,
            last_total_packed_uploaded_bytes: 0,
            last_log_seconds: 0.0,
        }
    }
}

impl ProfilingRun {
    fn measurement_active(&self) -> bool {
        if self.ready_gate {
            self.measurement_started
        } else {
            self.elapsed_seconds >= self.warmup_seconds
        }
    }

    fn autopilot_preroll_active(&self) -> bool {
        self.ready_gate
            && self.autopilot
            && self.autopilot_preroll_started
            && !self.measurement_started
    }

    fn measurement_elapsed_seconds(&self) -> f32 {
        if self.measurement_started {
            (self.elapsed_seconds - self.measurement_start_seconds).max(0.0)
        } else {
            0.0
        }
    }

    fn counting_elapsed_seconds(&self) -> f32 {
        if self.measurement_started {
            (self.measurement_elapsed_seconds() - self.settle_seconds).max(0.0)
        } else {
            0.0
        }
    }

    fn counting_active(&self) -> bool {
        self.measurement_active() && self.measurement_elapsed_seconds() >= self.settle_seconds
    }

    fn should_finish(&self) -> bool {
        if self.ready_gate {
            self.measurement_started
                && self.measurement_elapsed_seconds()
                    >= self.settle_seconds + self.measurement_target_seconds
        } else {
            self.elapsed_seconds >= self.duration_seconds
        }
    }

    fn update_measurement_gate(
        &mut self,
        frame_ms: f32,
        packed_stats: Option<PackedQuadPipelineStats>,
    ) {
        if !self.ready_gate {
            if !self.measurement_started && self.elapsed_seconds >= self.warmup_seconds {
                self.measurement_started = true;
                self.measurement_start_seconds = self.warmup_seconds;
                self.ready_seconds = self.warmup_seconds;
                self.ready_status = ProfileReadyStatus::Ready;
                self.total_surface_uploaded_bytes = 0;
                self.last_total_surface_uploaded_bytes = 0;
                self.total_packed_uploaded_bytes = 0;
                self.last_total_packed_uploaded_bytes = 0;
                self.last_log_seconds = self.warmup_seconds;
            }
            return;
        }

        if self.measurement_started || self.elapsed_seconds < self.warmup_seconds {
            return;
        }

        let readiness_packed_stats = if self.autopilot_preroll_started {
            None
        } else {
            packed_stats
        };
        if profile_frame_ready(frame_ms, self.ready_frame_ms, readiness_packed_stats) {
            self.ready_stable_frames = self.ready_stable_frames.saturating_add(1);
        } else {
            self.ready_stable_frames = 0;
        }

        let stable_frames_ready = self.ready_stable_frames >= self.ready_stable_frames_required;
        let preroll_time_ready = !self.autopilot_preroll_started
            || self.elapsed_seconds
                >= self.autopilot_preroll_start_seconds + self.autopilot_preroll_seconds;
        let ready = stable_frames_ready && preroll_time_ready;
        let ready_gate_start_seconds = if self.autopilot_preroll_started {
            self.autopilot_preroll_start_seconds
        } else {
            self.warmup_seconds
        };
        let timed_out =
            self.elapsed_seconds >= ready_gate_start_seconds + self.ready_max_extra_seconds;

        if self.autopilot && !self.autopilot_preroll_started {
            if ready {
                self.start_autopilot_preroll(ProfileReadyStatus::Ready, frame_ms, packed_stats);
            } else if timed_out {
                self.start_autopilot_preroll(ProfileReadyStatus::TimedOut, frame_ms, packed_stats);
            }
            return;
        }

        if ready {
            self.start_measurement(ProfileReadyStatus::Ready, frame_ms, packed_stats);
        } else if timed_out {
            self.start_measurement(ProfileReadyStatus::TimedOut, frame_ms, packed_stats);
        }
    }

    fn start_autopilot_preroll(
        &mut self,
        status: ProfileReadyStatus,
        frame_ms: f32,
        packed_stats: Option<PackedQuadPipelineStats>,
    ) {
        self.autopilot_preroll_started = true;
        self.autopilot_preroll_start_seconds = self.elapsed_seconds;
        self.ready_stable_frames = 0;

        let stats = packed_stats.unwrap_or_default();
        println!(
            "profile preroll t={:.2}s status={} frame_ms={:.2} ready_frame_ms={:.2} pending_builds={} pending_region_rebuilds={} stream_spawned_builds={} stream_rebuild_regions={} built_this_frame={} uploaded_this_frame={} compacted_regions={}",
            self.elapsed_seconds,
            status.as_str(),
            frame_ms,
            self.ready_frame_ms,
            stats.pending_builds,
            stats.pending_region_rebuilds,
            stats.stream_spawned_builds,
            stats.stream_rebuild_regions,
            stats.built_this_frame,
            stats.uploaded_this_frame,
            stats.compacted_regions_this_frame
        );
    }

    fn start_measurement(
        &mut self,
        status: ProfileReadyStatus,
        frame_ms: f32,
        packed_stats: Option<PackedQuadPipelineStats>,
    ) {
        self.measurement_started = true;
        self.measurement_start_seconds = self.elapsed_seconds;
        self.ready_seconds = self.elapsed_seconds;
        self.ready_status = status;
        self.total_surface_uploaded_bytes = 0;
        self.last_total_surface_uploaded_bytes = 0;
        self.total_packed_uploaded_bytes = 0;
        self.last_total_packed_uploaded_bytes = 0;
        self.last_log_seconds = self.elapsed_seconds;

        let stats = packed_stats.unwrap_or_default();
        println!(
            "profile ready t={:.2}s status={} stable_frames={} required_stable_frames={} frame_ms={:.2} ready_frame_ms={:.2} measured_target={:.1}s autopilot_preroll={} pending_builds={} pending_region_rebuilds={} stream_spawned_builds={} stream_rebuild_regions={} built_this_frame={} uploaded_this_frame={} compacted_regions={}",
            self.elapsed_seconds,
            status.as_str(),
            self.ready_stable_frames,
            self.ready_stable_frames_required,
            frame_ms,
            self.ready_frame_ms,
            self.measurement_target_seconds,
            self.autopilot_preroll_started,
            stats.pending_builds,
            stats.pending_region_rebuilds,
            stats.stream_spawned_builds,
            stats.stream_rebuild_regions,
            stats.built_this_frame,
            stats.uploaded_this_frame,
            stats.compacted_regions_this_frame
        );
    }
}

#[derive(Clone, Copy, Default)]
struct ProfileSurfaceTotals {
    spawned_regions: u64,
    uploaded_regions: u64,
    discarded_finished_regions: u64,
    despawned_loaded_regions: u64,
    despawned_building_regions: u64,
    stream_system_us: u64,
    upload_system_us: u64,
    completed_build_us: u64,
    completed_vertices: u64,
    completed_indices: u64,
}

impl ProfileSurfaceTotals {
    fn from_metrics(metrics: SurfaceStreamingMetrics) -> Self {
        Self {
            spawned_regions: metrics.total_spawned_regions,
            uploaded_regions: metrics.total_uploaded_regions,
            discarded_finished_regions: metrics.total_discarded_finished_regions,
            despawned_loaded_regions: metrics.total_despawned_loaded_regions,
            despawned_building_regions: metrics.total_despawned_building_regions,
            stream_system_us: metrics.total_stream_system_us,
            upload_system_us: metrics.total_upload_system_us,
            completed_build_us: metrics.total_completed_build_us,
            completed_vertices: metrics.total_completed_vertices,
            completed_indices: metrics.total_completed_indices,
        }
    }

    fn delta_from(self, previous: Self) -> Self {
        Self {
            spawned_regions: self
                .spawned_regions
                .saturating_sub(previous.spawned_regions),
            uploaded_regions: self
                .uploaded_regions
                .saturating_sub(previous.uploaded_regions),
            discarded_finished_regions: self
                .discarded_finished_regions
                .saturating_sub(previous.discarded_finished_regions),
            despawned_loaded_regions: self
                .despawned_loaded_regions
                .saturating_sub(previous.despawned_loaded_regions),
            despawned_building_regions: self
                .despawned_building_regions
                .saturating_sub(previous.despawned_building_regions),
            stream_system_us: self
                .stream_system_us
                .saturating_sub(previous.stream_system_us),
            upload_system_us: self
                .upload_system_us
                .saturating_sub(previous.upload_system_us),
            completed_build_us: self
                .completed_build_us
                .saturating_sub(previous.completed_build_us),
            completed_vertices: self
                .completed_vertices
                .saturating_sub(previous.completed_vertices),
            completed_indices: self
                .completed_indices
                .saturating_sub(previous.completed_indices),
        }
    }
}

fn elapsed_us_between(start: Instant, end: Instant) -> u64 {
    end.saturating_duration_since(start)
        .as_micros()
        .min(u128::from(u64::MAX)) as u64
}

fn elapsed_us_since(start: Instant) -> u64 {
    start.elapsed().as_micros().min(u128::from(u64::MAX)) as u64
}

fn store_render_phase_metric(metric: &AtomicU64, started_at: Option<Instant>) {
    if let Some(started_at) = started_at {
        metric.store(elapsed_us_since(started_at), Ordering::Relaxed);
    }
}

fn update_atomic_max(metric: &AtomicU64, value: u64) {
    let mut current = metric.load(Ordering::Relaxed);
    while value > current {
        match metric.compare_exchange_weak(current, value, Ordering::Relaxed, Ordering::Relaxed) {
            Ok(_) => break,
            Err(actual) => current = actual,
        }
    }
}

pub fn max_render_prepare_windows_us() -> u64 {
    MAX_RENDER_PREPARE_WINDOWS_US.load(Ordering::Relaxed)
}

pub fn reset_max_render_prepare_windows_us() {
    MAX_RENDER_PREPARE_WINDOWS_US.store(0, Ordering::Relaxed);
}

fn snapshot_render_phase_stats() -> ProfilePhaseStats {
    ProfilePhaseStats {
        render_schedule_us: RENDER_SCHEDULE_US.load(Ordering::Relaxed),
        render_extract_commands_us: RENDER_EXTRACT_COMMANDS_US.load(Ordering::Relaxed),
        render_prepare_assets_us: RENDER_PREPARE_ASSETS_US.load(Ordering::Relaxed),
        render_prepare_meshes_us: RENDER_PREPARE_MESHES_US.load(Ordering::Relaxed),
        render_manage_views_us: RENDER_MANAGE_VIEWS_US.load(Ordering::Relaxed),
        render_prepare_windows_us: RENDER_PREPARE_WINDOWS_US.load(Ordering::Relaxed),
        render_queue_us: RENDER_QUEUE_US.load(Ordering::Relaxed),
        render_phase_sort_us: RENDER_PHASE_SORT_US.load(Ordering::Relaxed),
        render_prepare_us: RENDER_PREPARE_US.load(Ordering::Relaxed),
        render_prepare_resources_us: RENDER_PREPARE_RESOURCES_US.load(Ordering::Relaxed),
        render_prepare_view_uniforms_us: RENDER_PREPARE_VIEW_UNIFORMS_US.load(Ordering::Relaxed),
        render_prepare_core_depth_textures_us: RENDER_PREPARE_CORE_DEPTH_TEXTURES_US
            .load(Ordering::Relaxed),
        render_prepare_core_transmission_textures_us: RENDER_PREPARE_CORE_TRANSMISSION_TEXTURES_US
            .load(Ordering::Relaxed),
        render_prepare_prepass_textures_us: RENDER_PREPARE_PREPASS_TEXTURES_US
            .load(Ordering::Relaxed),
        render_prepare_resources_collect_us: RENDER_PREPARE_RESOURCES_COLLECT_US
            .load(Ordering::Relaxed),
        render_prepare_resources_flush_us: RENDER_PREPARE_RESOURCES_FLUSH_US
            .load(Ordering::Relaxed),
        render_prepare_bind_groups_us: RENDER_PREPARE_BIND_GROUPS_US.load(Ordering::Relaxed),
        render_prepare_after_bind_groups_us: RENDER_PREPARE_AFTER_BIND_GROUPS_US
            .load(Ordering::Relaxed),
        render_render_us: RENDER_RENDER_US.load(Ordering::Relaxed),
        render_before_render_system_us: RENDER_BEFORE_RENDER_SYSTEM_US.load(Ordering::Relaxed),
        render_system_us: RENDER_SYSTEM_US.load(Ordering::Relaxed),
        render_cleanup_us: RENDER_CLEANUP_US.load(Ordering::Relaxed),
        render_post_cleanup_us: RENDER_POST_CLEANUP_US.load(Ordering::Relaxed),
        render_camera_driver_us: RENDER_CAMERA_DRIVER_US.load(Ordering::Relaxed),
        render_gpu_camera_driver_us: RENDER_GPU_CAMERA_DRIVER_US.load(Ordering::Relaxed),
        render_gpu_camera_driver_raw_begin: RENDER_GPU_CAMERA_DRIVER_RAW_BEGIN
            .load(Ordering::Relaxed),
        render_gpu_camera_driver_raw_end: RENDER_GPU_CAMERA_DRIVER_RAW_END.load(Ordering::Relaxed),
        render_gpu_camera_driver_raw_delta: RENDER_GPU_CAMERA_DRIVER_RAW_DELTA
            .load(Ordering::Relaxed),
        render_gpu_camera_driver_readbacks: RENDER_GPU_CAMERA_DRIVER_READBACKS
            .load(Ordering::Relaxed),
        render_gpu_camera_driver_zero_deltas: RENDER_GPU_CAMERA_DRIVER_ZERO_DELTAS
            .load(Ordering::Relaxed),
        render_gpu_camera_driver_map_failures: RENDER_GPU_CAMERA_DRIVER_MAP_FAILURES
            .load(Ordering::Relaxed),
        render_gpu_frame_timestamps_requested: RENDER_GPU_FRAME_TIMESTAMPS_REQUESTED
            .load(Ordering::Relaxed),
        render_gpu_frame_timestamps_supported: RENDER_GPU_FRAME_TIMESTAMPS_SUPPORTED
            .load(Ordering::Relaxed),
        render_gpu_camera_driver_pending_readback: RENDER_GPU_CAMERA_DRIVER_PENDING_READBACK
            .load(Ordering::Relaxed),
        render_gpu_camera_driver_map_requested: RENDER_GPU_CAMERA_DRIVER_MAP_REQUESTED
            .load(Ordering::Relaxed),
        render_core3d_us: RENDER_CORE3D_GRAPH_US.load(Ordering::Relaxed),
        ..default()
    }
}

fn initialize_render_gpu_frame_timestamp_profiler(
    mut profiler: ResMut<RenderGpuFrameTimestampProfiler>,
    render_device: Res<RenderDevice>,
    render_queue: Res<RenderQueue>,
) {
    profiler.initialize_if_needed(&render_device, &render_queue);
}

fn mark_main_frame_start(profiling: Res<ProfilingRun>, mut probe: ResMut<MainFramePhaseProbe>) {
    if !profiling.enabled {
        return;
    }

    let now = Instant::now();
    let render_stats = snapshot_render_phase_stats();
    probe.completed = ProfilePhaseStats {
        frame_wall_us: probe
            .current_frame_started_at
            .map_or(0, |started_at| elapsed_us_between(started_at, now)),
        main_schedule_us: probe.previous_main_schedule_us,
        main_tail_us: probe
            .previous_main_finished_at
            .map_or(0, |finished_at| elapsed_us_between(finished_at, now)),
        render_schedule_us: render_stats.render_schedule_us,
        render_camera_driver_us: render_stats.render_camera_driver_us,
        render_gpu_camera_driver_us: render_stats.render_gpu_camera_driver_us,
        render_gpu_frame_timestamps_requested: render_stats.render_gpu_frame_timestamps_requested,
        render_gpu_frame_timestamps_supported: render_stats.render_gpu_frame_timestamps_supported,
        render_core3d_us: render_stats.render_core3d_us,
        ..render_stats
    };
    probe.current_frame_started_at = Some(now);
}

fn mark_main_frame_end(profiling: Res<ProfilingRun>, mut probe: ResMut<MainFramePhaseProbe>) {
    if !profiling.enabled {
        return;
    }

    let now = Instant::now();
    if let Some(started_at) = probe.current_frame_started_at {
        probe.previous_main_schedule_us = elapsed_us_between(started_at, now);
    }
    probe.previous_main_finished_at = Some(now);
}

fn mark_render_schedule_start(mut probe: ResMut<RenderFramePhaseProbe>) {
    let now = Instant::now();
    probe.schedule_started_at = Some(now);
    probe.phase_started_at = Some(now);
}

fn finish_render_phase(probe: &mut RenderFramePhaseProbe, metric: &AtomicU64) {
    let now = Instant::now();
    if let Some(started_at) = probe.phase_started_at.replace(now) {
        metric.store(elapsed_us_between(started_at, now), Ordering::Relaxed);
    }
}

fn mark_render_after_extract_commands(mut probe: ResMut<RenderFramePhaseProbe>) {
    finish_render_phase(&mut probe, &RENDER_EXTRACT_COMMANDS_US);
}

fn mark_render_after_prepare_assets(mut probe: ResMut<RenderFramePhaseProbe>) {
    finish_render_phase(&mut probe, &RENDER_PREPARE_ASSETS_US);
}

fn mark_render_after_prepare_meshes(mut probe: ResMut<RenderFramePhaseProbe>) {
    finish_render_phase(&mut probe, &RENDER_PREPARE_MESHES_US);
}

fn mark_render_prepare_windows_start(mut probe: ResMut<RenderFramePhaseProbe>) {
    probe.prepare_windows_started_at = Some(Instant::now());
}

fn mark_render_prepare_windows_end(mut probe: ResMut<RenderFramePhaseProbe>) {
    let started_at = probe.prepare_windows_started_at.take();
    store_render_phase_metric(&RENDER_PREPARE_WINDOWS_US, started_at);
    update_atomic_max(
        &MAX_RENDER_PREPARE_WINDOWS_US,
        RENDER_PREPARE_WINDOWS_US.load(Ordering::Relaxed),
    );
}

fn mark_render_after_manage_views(mut probe: ResMut<RenderFramePhaseProbe>) {
    finish_render_phase(&mut probe, &RENDER_MANAGE_VIEWS_US);
}

fn mark_render_after_queue(mut probe: ResMut<RenderFramePhaseProbe>) {
    finish_render_phase(&mut probe, &RENDER_QUEUE_US);
}

fn mark_render_after_phase_sort(mut probe: ResMut<RenderFramePhaseProbe>) {
    finish_render_phase(&mut probe, &RENDER_PHASE_SORT_US);
    probe.prepare_segment_started_at = probe.phase_started_at;
}

fn finish_render_prepare_segment(probe: &mut RenderFramePhaseProbe, metric: &AtomicU64) {
    let now = Instant::now();
    if let Some(started_at) = probe.prepare_segment_started_at.replace(now) {
        metric.store(elapsed_us_between(started_at, now), Ordering::Relaxed);
    }
}

fn mark_render_prepare_view_uniforms_start(mut probe: ResMut<RenderFramePhaseProbe>) {
    probe.prepare_view_uniforms_started_at = Some(Instant::now());
}

fn mark_render_prepare_view_uniforms_end(mut probe: ResMut<RenderFramePhaseProbe>) {
    let started_at = probe.prepare_view_uniforms_started_at.take();
    store_render_phase_metric(&RENDER_PREPARE_VIEW_UNIFORMS_US, started_at);
}

fn mark_render_prepare_core_depth_textures_start(mut probe: ResMut<RenderFramePhaseProbe>) {
    probe.prepare_core_depth_textures_started_at = Some(Instant::now());
}

fn mark_render_prepare_core_depth_textures_end(mut probe: ResMut<RenderFramePhaseProbe>) {
    let started_at = probe.prepare_core_depth_textures_started_at.take();
    store_render_phase_metric(&RENDER_PREPARE_CORE_DEPTH_TEXTURES_US, started_at);
}

fn mark_render_prepare_core_transmission_textures_start(mut probe: ResMut<RenderFramePhaseProbe>) {
    probe.prepare_core_transmission_textures_started_at = Some(Instant::now());
}

fn mark_render_prepare_core_transmission_textures_end(mut probe: ResMut<RenderFramePhaseProbe>) {
    let started_at = probe.prepare_core_transmission_textures_started_at.take();
    store_render_phase_metric(&RENDER_PREPARE_CORE_TRANSMISSION_TEXTURES_US, started_at);
}

fn mark_render_prepare_prepass_textures_start(mut probe: ResMut<RenderFramePhaseProbe>) {
    probe.prepare_prepass_textures_started_at = Some(Instant::now());
}

fn mark_render_prepare_prepass_textures_end(mut probe: ResMut<RenderFramePhaseProbe>) {
    let started_at = probe.prepare_prepass_textures_started_at.take();
    store_render_phase_metric(&RENDER_PREPARE_PREPASS_TEXTURES_US, started_at);
}

fn mark_render_after_prepare_resources(mut probe: ResMut<RenderFramePhaseProbe>) {
    finish_render_prepare_segment(&mut probe, &RENDER_PREPARE_RESOURCES_US);
}

fn mark_render_after_prepare_resources_collect(mut probe: ResMut<RenderFramePhaseProbe>) {
    finish_render_prepare_segment(&mut probe, &RENDER_PREPARE_RESOURCES_COLLECT_US);
}

fn mark_render_after_prepare_resources_flush(mut probe: ResMut<RenderFramePhaseProbe>) {
    finish_render_prepare_segment(&mut probe, &RENDER_PREPARE_RESOURCES_FLUSH_US);
}

fn mark_render_after_prepare_bind_groups(mut probe: ResMut<RenderFramePhaseProbe>) {
    finish_render_prepare_segment(&mut probe, &RENDER_PREPARE_BIND_GROUPS_US);
}

fn mark_render_after_prepare(mut probe: ResMut<RenderFramePhaseProbe>) {
    finish_render_prepare_segment(&mut probe, &RENDER_PREPARE_AFTER_BIND_GROUPS_US);
    probe.prepare_segment_started_at = None;
    finish_render_phase(&mut probe, &RENDER_PREPARE_US);
}

fn mark_render_system_start(
    mut probe: ResMut<RenderFramePhaseProbe>,
    _pipeline_cache: Res<PipelineCache>,
) {
    let now = Instant::now();
    if let Some(started_at) = probe.phase_started_at {
        RENDER_BEFORE_RENDER_SYSTEM_US
            .store(elapsed_us_between(started_at, now), Ordering::Relaxed);
    }
    probe.render_system_started_at = Some(now);
}

fn mark_render_system_end(mut probe: ResMut<RenderFramePhaseProbe>) {
    let started_at = probe.render_system_started_at.take();
    store_render_phase_metric(&RENDER_SYSTEM_US, started_at);
}

fn mark_render_after_render(mut probe: ResMut<RenderFramePhaseProbe>) {
    finish_render_phase(&mut probe, &RENDER_RENDER_US);
}

fn mark_render_after_cleanup(mut probe: ResMut<RenderFramePhaseProbe>) {
    finish_render_phase(&mut probe, &RENDER_CLEANUP_US);
}

fn mark_render_schedule_end(mut probe: ResMut<RenderFramePhaseProbe>) {
    finish_render_phase(&mut probe, &RENDER_POST_CLEANUP_US);
    let started_at = probe.schedule_started_at.take();
    store_render_phase_metric(&RENDER_SCHEDULE_US, started_at);
}

fn announce_profiling_run(
    profiling: Res<ProfilingRun>,
    render_mode: Option<Res<rumpel_render::RumpelRenderMode>>,
) {
    if !profiling.enabled {
        return;
    }

    let render_mode_value = render_mode.as_ref().map(|mode| **mode);
    let mode_str = match render_mode_value.unwrap_or(rumpel_render::RumpelRenderMode::Surface) {
        rumpel_render::RumpelRenderMode::Surface => "surface",
        rumpel_render::RumpelRenderMode::ComputePrototype => "compute",
        rumpel_render::RumpelRenderMode::PackedPrototype => "packed",
        rumpel_render::RumpelRenderMode::PackedMaterial => "packed_material",
    };

    println!(
        "profile start duration={:.1}s warmup={:.1}s settle={:.1}s measured_target={:.1}s ready_gate={} ready_stable_frames={} ready_frame_ms={:.1} ready_max_extra={:.1}s autopilot={} autopilot_preroll={:.1}s interval={:.1}s slow_frame_ms={:.1} render_mode={} render_target={} headless_wait_ms={} gpu_frame_timestamps={} present_mode={} frame_latency={} window_size={} shadows={} debug_hud={}",
        profiling.duration_seconds,
        profiling.warmup_seconds,
        profiling.settle_seconds,
        profiling.measurement_target_seconds,
        profiling.ready_gate,
        profiling.ready_stable_frames_required,
        profiling.ready_frame_ms,
        profiling.ready_max_extra_seconds,
        profiling.autopilot,
        profiling.autopilot_preroll_seconds,
        profiling.log_interval_seconds,
        profiling.slow_frame_threshold_ms,
        mode_str,
        render_target_label(),
        headless_wait_ms_label(),
        render_gpu_frame_timestamps_label(),
        present_mode_label(),
        frame_latency_label(),
        window_size_label(),
        shadows_label(render_mode_value),
        debug_hud_label()
    );
}

fn profile_autopilot(
    time: Res<Time>,
    profiling: Res<ProfilingRun>,
    mut player_query: Query<(&mut Transform, Option<&mut PlayerPhysics>), With<Player>>,
) {
    if !profiling.enabled || !profiling.autopilot {
        return;
    }
    if camera_lock_enabled() {
        return;
    }
    if profiling.ready_gate
        && !profiling.measurement_active()
        && !profiling.autopilot_preroll_active()
    {
        return;
    }

    let Ok((mut player_transform, player_physics)) = player_query.single_mut() else {
        return;
    };
    if let Some(mut physics) = player_physics {
        physics.is_flying = true;
        physics.velocity = Vec3::ZERO;
        physics.is_grounded = false;
    }

    let elapsed = profiling.elapsed_seconds;
    let direction = Vec3::new((elapsed * 0.35).cos(), 0.0, (elapsed * 0.35).sin()).normalize();
    player_transform.translation += direction * AUTOPILOT_SPEED * time.delta_secs();
    let surface_y = rumpel_prelude::terrain_height_at(
        player_transform.translation.x.floor() as i32,
        player_transform.translation.z.floor() as i32,
    ) as f32;
    player_transform.translation.y = surface_y + AUTOPILOT_SURFACE_CLEARANCE;
}

#[allow(clippy::too_many_arguments)]
fn log_profile_metrics(
    time: Res<Time>,
    diagnostics: Res<DiagnosticsStore>,
    player_query: Query<&Transform, With<Player>>,
    chunk_meshes: Query<&RenderedChunkCount>,
    surface_metrics: Option<Res<SurfaceStreamingMetrics>>,
    packed_stats: Option<Res<PackedQuadPipelineStats>>,
    phase_probe: Option<Res<MainFramePhaseProbe>>,
    render_mode: Option<Res<rumpel_render::RumpelRenderMode>>,
    mut profiling: ResMut<ProfilingRun>,
    mut app_exit: MessageWriter<AppExit>,
) {
    if !profiling.enabled || profiling.finished {
        return;
    }

    profiling.elapsed_seconds += time.delta_secs();
    let bevy_delta_ms = time.delta_secs() * 1000.0;
    let fps = diagnostics
        .get(&FrameTimeDiagnosticsPlugin::FPS)
        .and_then(|diagnostic| diagnostic.smoothed())
        .unwrap_or(0.0);
    let rendered_chunk_count = chunk_meshes.iter().map(|count| count.0).sum::<usize>();
    let surface = surface_metrics.as_deref().copied().unwrap_or_default();
    let packed_stats_snapshot = packed_stats
        .as_deref()
        .map(|_| snapshot_packed_quad_metrics());
    let phase_stats = phase_probe
        .as_deref()
        .map(|probe| probe.completed)
        .unwrap_or_default();
    let frame_ms = if phase_stats.frame_wall_us > 0 {
        phase_stats.frame_wall_us as f32 / 1_000.0
    } else {
        bevy_delta_ms
    };
    let raw_fps = if frame_ms > 0.0 {
        1_000.0 / frame_ms
    } else {
        0.0
    };

    profiling.update_measurement_gate(frame_ms, packed_stats_snapshot);
    let measure_frame = profiling.measurement_active();
    let count_frame = profiling.counting_active();

    if measure_frame && !profiling.counting_started_logged && count_frame {
        profiling.counting_started_logged = true;
        println!(
            "profile counting t={:.2}s settle={:.1}s measured_target={:.1}s",
            profiling.elapsed_seconds,
            profiling.settle_seconds,
            profiling.measurement_target_seconds
        );
    }

    if count_frame {
        let frame_surface_bytes = (surface.completed_vertices_last_frame * 52
            + surface.completed_indices_last_frame * 4) as u64;
        profiling.total_surface_uploaded_bytes += frame_surface_bytes;

        if let Some(stats) = packed_stats_snapshot {
            profiling.total_packed_uploaded_bytes += stats.uploaded_bytes;
        }
    }

    if fps > 0.0 && count_frame {
        profiling.min_fps = profiling.min_fps.min(fps);
    }
    if raw_fps > 0.0 && count_frame {
        profiling.min_raw_fps = profiling.min_raw_fps.min(raw_fps);
    }

    if frame_ms > 0.0 && count_frame {
        profiling.measured_frame_count += 1;
        profiling.measured_frame_ms_sum += f64::from(frame_ms);

        if frame_ms >= FRAME_BUDGET_60HZ_MS {
            profiling.frames_ge_16ms += 1;
            profiling.interval_frames_ge_16ms += 1;
        }
        if frame_ms >= FRAME_BUDGET_40HZ_MS {
            profiling.frames_ge_25ms += 1;
            profiling.interval_frames_ge_25ms += 1;
        }
        if frame_ms >= FRAME_BUDGET_30HZ_MS {
            profiling.frames_ge_33ms += 1;
            profiling.interval_frames_ge_33ms += 1;
        }

        if profiling.slow_frame_threshold_ms > 0.0 && frame_ms >= profiling.slow_frame_threshold_ms
        {
            log_slow_frame(
                profiling.elapsed_seconds,
                frame_ms,
                bevy_delta_ms,
                raw_fps,
                rendered_chunk_count,
                phase_stats,
                packed_stats_snapshot,
            );
        }

        if frame_ms > profiling.interval_worst_frame_ms {
            profiling.interval_worst_frame_ms = frame_ms;
            profiling.interval_worst_frame_t = profiling.elapsed_seconds;
            profiling.interval_worst_phase_stats = phase_stats;
            profiling.interval_worst_packed_stats = packed_stats_snapshot;
        }

        if frame_ms > profiling.worst_frame_ms {
            profiling.worst_frame_ms = frame_ms;
            profiling.worst_frame_t = profiling.elapsed_seconds;
            profiling.worst_phase_stats = phase_stats;
            profiling.worst_packed_stats = packed_stats_snapshot;
        }
    }

    if profiling.elapsed_seconds >= profiling.next_log_seconds {
        profiling.sample_count += 1;
        profiling.next_log_seconds = profiling.elapsed_seconds + profiling.log_interval_seconds;

        let player_position = player_query
            .single()
            .map(|transform| transform.translation)
            .unwrap_or(Vec3::ZERO);
        let surface_totals = ProfileSurfaceTotals::from_metrics(surface);
        let surface_delta = surface_totals.delta_from(profiling.last_surface_totals);
        profiling.last_surface_totals = surface_totals;

        let sample_elapsed = (profiling.elapsed_seconds - profiling.last_log_seconds).max(0.001);

        let surface_sample_bytes = profiling
            .total_surface_uploaded_bytes
            .saturating_sub(profiling.last_total_surface_uploaded_bytes);
        let surface_upload_mb_s =
            (surface_sample_bytes as f64) / (sample_elapsed as f64) / (1024.0 * 1024.0);
        let surface_total_upload_mb =
            (profiling.total_surface_uploaded_bytes as f64) / (1024.0 * 1024.0);

        let packed_sample_bytes = profiling
            .total_packed_uploaded_bytes
            .saturating_sub(profiling.last_total_packed_uploaded_bytes);
        let packed_upload_mb_s =
            (packed_sample_bytes as f64) / (sample_elapsed as f64) / (1024.0 * 1024.0);
        let packed_total_upload_mb =
            (profiling.total_packed_uploaded_bytes as f64) / (1024.0 * 1024.0);

        profiling.last_total_surface_uploaded_bytes = profiling.total_surface_uploaded_bytes;
        profiling.last_total_packed_uploaded_bytes = profiling.total_packed_uploaded_bytes;
        profiling.last_log_seconds = profiling.elapsed_seconds;

        let mode_str = render_mode
            .map(|m| match *m {
                rumpel_render::RumpelRenderMode::Surface => "surface",
                rumpel_render::RumpelRenderMode::ComputePrototype => "compute",
                rumpel_render::RumpelRenderMode::PackedPrototype => "packed",
                rumpel_render::RumpelRenderMode::PackedMaterial => "packed_material",
            })
            .unwrap_or("surface");

        let mut log_str = format!(
            "profile sample={} t={:.1}s render_mode={} fps={:.1} raw_fps={:.1} frame_ms={:.2} bevy_delta_ms={:.2} frame_wall_us={} frame_main_us={} frame_tail_us={} render_schedule_us={} render_camera_driver_us={} render_gpu_camera_driver_us={} render_graph_tail_us={} render_core3d_us={} interval_worst_frame_ms={:.2} interval_worst_frame_fps={:.1} interval_worst_frame_t={:.2} interval_frames_ge_16ms={} interval_frames_ge_25ms={} interval_frames_ge_33ms={} chunks={} player=({:.1},{:.1},{:.1}) surface_loaded={} surface_building={} surface_pending={} surface_spawned={} surface_uploaded={} surface_discarded={} surface_despawn_loaded={} surface_despawn_building={} surface_stream_us={} surface_upload_us={} surface_build_us_sum={} surface_build_us_max={} surface_vertices={} surface_indices={} surface_lod_max={} surface_textured={} surface_sample_spawned={} surface_sample_uploaded={} surface_sample_discarded={} surface_sample_despawn_loaded={} surface_sample_despawn_building={} surface_sample_stream_us={} surface_sample_upload_us={} surface_sample_build_us={} surface_sample_vertices={} surface_sample_indices={}",
            profiling.sample_count,
            profiling.elapsed_seconds,
            mode_str,
            fps,
            raw_fps,
            frame_ms,
            bevy_delta_ms,
            phase_stats.frame_wall_us,
            phase_stats.main_schedule_us,
            phase_stats.main_tail_us,
            phase_stats.render_schedule_us,
            phase_stats.render_camera_driver_us,
            phase_stats.render_gpu_camera_driver_us,
            render_graph_tail_us(phase_stats),
            phase_stats.render_core3d_us,
            profiling.interval_worst_frame_ms,
            fps_from_frame_ms(profiling.interval_worst_frame_ms),
            profiling.interval_worst_frame_t,
            profiling.interval_frames_ge_16ms,
            profiling.interval_frames_ge_25ms,
            profiling.interval_frames_ge_33ms,
            rendered_chunk_count,
            player_position.x,
            player_position.y,
            player_position.z,
            surface.loaded_regions,
            surface.building_regions,
            surface.pending_regions,
            surface.spawned_regions_last_frame,
            surface.uploaded_regions_last_frame,
            surface.discarded_finished_regions_last_frame,
            surface.despawned_loaded_last_frame,
            surface.despawned_building_last_frame,
            surface.stream_system_us_last_frame,
            surface.upload_system_us_last_frame,
            surface.completed_build_us_last_frame_sum,
            surface.completed_build_us_last_frame_max,
            surface.completed_vertices_last_frame,
            surface.completed_indices_last_frame,
            surface.completed_lod_step_max,
            surface.completed_textured_last_frame,
            surface_delta.spawned_regions,
            surface_delta.uploaded_regions,
            surface_delta.discarded_finished_regions,
            surface_delta.despawned_loaded_regions,
            surface_delta.despawned_building_regions,
            surface_delta.stream_system_us,
            surface_delta.upload_system_us,
            surface_delta.completed_build_us,
            surface_delta.completed_vertices,
            surface_delta.completed_indices
        );
        log_str.push_str(&render_phase_detail_fields("", phase_stats));
        log_str.push_str(&format!(
            " surface_upload_mb_s={:.2} surface_total_upload_mb={:.2} packed_upload_mb_s={:.2} packed_total_upload_mb={:.2}",
            surface_upload_mb_s,
            surface_total_upload_mb,
            packed_upload_mb_s,
            packed_total_upload_mb,
        ));

        if let Some(stats) = packed_stats_snapshot {
            let vertex_count =
                rumpel_render::packed_quad_renderer::vertex_count_for_quads(stats.arena_used_quads);
            let mode_str = packed_draw_mode_label(stats.draw_mode);
            let face_range_cull =
                env_flag_default(PACKED_FACE_RANGE_CULL_ENV, DEFAULT_PACKED_FACE_RANGE_CULL);
            let face_range_min_quads = env_usize(
                PACKED_FACE_RANGE_MIN_QUADS_ENV,
                DEFAULT_PACKED_FACE_RANGE_MIN_QUADS,
            );
            log_str.push_str(&format!(
                " packed_batches={} packed_visible_batches={} packed_quads={} packed_visible_quads={} packed_uploaded_quads={} packed_dropped_quads={} packed_uploaded_bytes={} packed_buffer_capacity={} packed_vertex_count={} packed_visible_vertex_count={} packed_chunks_loaded={} packed_chunks_active={} packed_chunk_ranges={} packed_resident_ranges={} packed_tombstone_ranges={} packed_resident_capacity_quads={} packed_tombstone_capacity_quads={} packed_dirty_ranges={} packed_dirty_range_quads={} pending_builds={} pending_region_rebuilds={} packed_prepare_us={} packed_view_prepare_us={} packed_stream_us={} packed_stream_spawned_builds={} packed_stream_rebuild_regions={} packed_build_task_us={} built_this_frame={} packed_compaction_us={} packed_compacted_regions={} uploaded_this_frame={} arena_capacity_quads={} arena_used_quads={} arena_slot_quads={} arena_uploaded_bytes={} arena_reallocations={} arena_compactions={} packed_cpu_reserved_bytes={} packed_min_ram_bytes={} packed_gpu_reserved_bytes={} packed_min_vram_bytes={} packed_draw_mode={} packed_generated_regions_loaded={} packed_generated_regions_active={} packed_generated_regions_visible={} packed_generated_update_us={} packed_generated_update_skipped={} packed_generated_cache_hits={} packed_generated_cache_misses={} packed_generated_cache_invalidated={} packed_generated_cache_evicted={} packed_generated_cache_prefetched={} packed_generated_prepare_skipped={} packed_generated_cull_metadata_uploaded={} packed_generated_cull_config_uploaded={} packed_generated_cull_dispatch_skipped={} packed_face_range_cull={} packed_face_range_min_quads={} packed_indirect_draw_commands={} packed_render_node_us={} packed_render_draw_calls={} packed_render_items_considered={} packed_render_gpu_pass_us={} packed_gpu_timestamps_requested={} packed_gpu_timestamps_supported={} packed_gpu_cull_enabled={} packed_gpu_cull_input_commands={} packed_gpu_cull_est_visible_commands={} packed_gpu_cull_est_visible_quads={} packed_gpu_cull_node_us={} packed_gpu_cull_count_supported={} packed_gpu_cull_compact_enabled={} packed_cpu_visible_compact_enabled={} packed_cpu_visible_commands={} packed_material_entities={} packed_material_sync_us={}",
                stats.batches,
                stats.visible_batches,
                stats.quads,
                stats.visible_quads,
                stats.uploaded_quads,
                stats.dropped_quads,
                stats.uploaded_bytes,
                stats.buffer_capacity_quads,
                vertex_count,
                rumpel_render::packed_quad_renderer::vertex_count_for_quads(stats.visible_quads),
                stats.chunks_loaded,
                stats.chunks_active,
                stats.chunk_ranges,
                stats.resident_chunk_ranges,
                stats.tombstone_chunk_ranges,
                stats.resident_range_capacity_quads,
                stats.tombstone_capacity_quads,
                stats.dirty_ranges,
                stats.dirty_range_quads,
                stats.pending_builds,
                stats.pending_region_rebuilds,
                stats.prepare_system_us,
                stats.view_prepare_system_us,
                stats.stream_system_us,
                stats.stream_spawned_builds,
                stats.stream_rebuild_regions,
                stats.build_task_system_us,
                stats.built_this_frame,
                stats.compaction_system_us,
                stats.compacted_regions_this_frame,
                stats.uploaded_this_frame,
                stats.arena_capacity_quads,
                stats.arena_used_quads,
                stats.arena_slot_quads,
                stats.arena_uploaded_bytes,
                stats.arena_reallocations,
                stats.arena_compactions,
                stats.cpu_reserved_bytes,
                stats.min_ram_bytes,
                stats.gpu_reserved_bytes,
                stats.min_vram_bytes,
                mode_str,
                stats.generated_regions_loaded,
                stats.generated_regions_active,
                stats.generated_regions_visible,
                stats.generated_update_us,
                stats.generated_update_skipped,
                stats.generated_cache_hits,
                stats.generated_cache_misses,
                stats.generated_cache_invalidated,
                stats.generated_cache_evicted,
                stats.generated_cache_prefetched,
                stats.generated_prepare_skipped,
                stats.generated_cull_metadata_uploaded,
                stats.generated_cull_config_uploaded,
                stats.generated_cull_dispatch_skipped,
                face_range_cull,
                face_range_min_quads,
                stats.indirect_draw_commands,
                stats.render_node_us,
                stats.render_draw_calls,
                stats.render_items_considered,
                stats.render_gpu_pass_us,
                stats.gpu_timestamps_requested,
                stats.gpu_timestamps_supported,
                stats.gpu_cull_enabled,
                stats.gpu_cull_input_commands,
                stats.gpu_cull_est_visible_commands,
                stats.gpu_cull_est_visible_quads,
                stats.gpu_cull_node_us,
                stats.gpu_cull_count_supported,
                stats.gpu_cull_compact_enabled,
                stats.cpu_visible_compact_enabled,
                stats.cpu_visible_commands,
                stats.material_entities,
                stats.material_sync_us
            ));
        }

        if profiling.interval_worst_frame_ms > 0.0 {
            let phase = profiling.interval_worst_phase_stats;
            log_str.push_str(&format!(
                " interval_worst_frame_wall_us={} interval_worst_frame_main_us={} interval_worst_frame_tail_us={} interval_worst_render_schedule_us={} interval_worst_render_camera_driver_us={} interval_worst_render_gpu_camera_driver_us={} interval_worst_render_graph_tail_us={} interval_worst_render_core3d_us={}",
                phase.frame_wall_us,
                phase.main_schedule_us,
                phase.main_tail_us,
                phase.render_schedule_us,
                phase.render_camera_driver_us,
                phase.render_gpu_camera_driver_us,
                render_graph_tail_us(phase),
                phase.render_core3d_us,
            ));
            log_str.push_str(&render_phase_detail_fields("interval_worst_", phase));
        }

        if let Some(stats) = profiling.interval_worst_packed_stats {
            log_str.push_str(&format!(
                " interval_worst_packed_visible_quads={} interval_worst_packed_uploaded_quads={} interval_worst_pending_builds={} interval_worst_pending_region_rebuilds={} interval_worst_prepare_us={} interval_worst_view_prepare_us={} interval_worst_stream_us={} interval_worst_stream_spawned_builds={} interval_worst_stream_rebuild_regions={} interval_worst_build_task_us={} interval_worst_built_this_frame={} interval_worst_compaction_us={} interval_worst_compacted_regions={} interval_worst_uploaded_this_frame={} interval_worst_arena_compactions={} interval_worst_render_node_us={} interval_worst_packed_render_gpu_pass_us={} interval_worst_gpu_cull_node_us={} interval_worst_cpu_visible_commands={} interval_worst_material_entities={} interval_worst_material_sync_us={}",
                stats.visible_quads,
                stats.uploaded_quads,
                stats.pending_builds,
                stats.pending_region_rebuilds,
                stats.prepare_system_us,
                stats.view_prepare_system_us,
                stats.stream_system_us,
                stats.stream_spawned_builds,
                stats.stream_rebuild_regions,
                stats.build_task_system_us,
                stats.built_this_frame,
                stats.compaction_system_us,
                stats.compacted_regions_this_frame,
                stats.uploaded_this_frame,
                stats.arena_compactions,
                stats.render_node_us,
                stats.render_gpu_pass_us,
                stats.gpu_cull_node_us,
                stats.cpu_visible_commands,
                stats.material_entities,
                stats.material_sync_us
            ));
        }

        println!("{}", log_str);
        profiling.interval_worst_frame_ms = 0.0;
        profiling.interval_worst_frame_t = 0.0;
        profiling.interval_frames_ge_16ms = 0;
        profiling.interval_frames_ge_25ms = 0;
        profiling.interval_frames_ge_33ms = 0;
        profiling.interval_worst_phase_stats = ProfilePhaseStats::default();
        profiling.interval_worst_packed_stats = None;
    }

    if profiling.should_finish() {
        profiling.finished = true;
        let min_fps = if profiling.min_fps == f64::MAX {
            0.0
        } else {
            profiling.min_fps
        };
        let min_raw_fps = if profiling.min_raw_fps == f32::MAX {
            0.0
        } else {
            profiling.min_raw_fps
        };
        let average_frame_ms = if profiling.measured_frame_count > 0 {
            profiling.measured_frame_ms_sum / profiling.measured_frame_count as f64
        } else {
            0.0
        };
        let average_raw_fps = if average_frame_ms > 0.0 {
            1000.0 / average_frame_ms
        } else {
            0.0
        };

        let measured_duration = profiling.counting_elapsed_seconds();
        let measured_duration_s = if measured_duration > 0.0 {
            measured_duration
        } else {
            1.0
        };

        let measured_surface_upload_mb =
            (profiling.total_surface_uploaded_bytes as f64) / (1024.0 * 1024.0);
        let measured_surface_bandwidth_mb_s =
            measured_surface_upload_mb / (measured_duration_s as f64);

        let measured_packed_upload_mb =
            (profiling.total_packed_uploaded_bytes as f64) / (1024.0 * 1024.0);
        let measured_packed_bandwidth_mb_s =
            measured_packed_upload_mb / (measured_duration_s as f64);

        let max_render_prepare_windows_us = max_render_prepare_windows_us();

        println!(
            "profile end samples={} duration={:.1}s measured_duration={:.1}s counting_duration={:.1}s settle={:.1}s ready_status={} ready_t={:.2} measured_frames={} frames_ge_16ms={} frames_ge_25ms={} frames_ge_33ms={} min_fps={:.1} min_raw_fps={:.1} avg_raw_fps={:.1} worst_frame_ms={:.2} worst_frame_fps={:.1} worst_frame_t={:.2} worst_frame_wall_us={} worst_frame_main_us={} worst_frame_tail_us={} worst_render_schedule_us={} worst_render_camera_driver_us={} worst_render_gpu_camera_driver_us={} worst_render_graph_tail_us={} worst_render_core3d_us={}{} max_render_prepare_windows_us={} measured_surface_upload_mb={:.2} measured_packed_upload_mb={:.2} measured_surface_bandwidth_mb_s={:.2} measured_packed_bandwidth_mb_s={:.2}",
            profiling.sample_count,
            profiling.elapsed_seconds,
            profiling.measurement_elapsed_seconds(),
            profiling.counting_elapsed_seconds(),
            profiling.settle_seconds,
            profiling.ready_status.as_str(),
            profiling.ready_seconds,
            profiling.measured_frame_count,
            profiling.frames_ge_16ms,
            profiling.frames_ge_25ms,
            profiling.frames_ge_33ms,
            min_fps,
            min_raw_fps,
            average_raw_fps,
            profiling.worst_frame_ms,
            fps_from_frame_ms(profiling.worst_frame_ms),
            profiling.worst_frame_t,
            profiling.worst_phase_stats.frame_wall_us,
            profiling.worst_phase_stats.main_schedule_us,
            profiling.worst_phase_stats.main_tail_us,
            profiling.worst_phase_stats.render_schedule_us,
            profiling.worst_phase_stats.render_camera_driver_us,
            profiling.worst_phase_stats.render_gpu_camera_driver_us,
            render_graph_tail_us(profiling.worst_phase_stats),
            profiling.worst_phase_stats.render_core3d_us,
            render_phase_detail_fields("worst_", profiling.worst_phase_stats),
            max_render_prepare_windows_us,
            measured_surface_upload_mb,
            measured_packed_upload_mb,
            measured_surface_bandwidth_mb_s,
            measured_packed_bandwidth_mb_s
        );
        reset_max_render_prepare_windows_us();
        if let Some(stats) = profiling.worst_packed_stats {
            println!(
                "profile worst_packed draw_mode={} generated_regions_loaded={} generated_regions_active={} generated_regions_visible={} generated_update_us={} generated_update_skipped={} generated_cache_hits={} generated_cache_misses={} generated_cache_invalidated={} generated_cache_evicted={} generated_cache_prefetched={} generated_prepare_skipped={} generated_cull_metadata_uploaded={} generated_cull_config_uploaded={} generated_cull_dispatch_skipped={} visible_quads={} uploaded_quads={} indirect_draw_commands={} pending_builds={} pending_region_rebuilds={} prepare_us={} view_prepare_us={} stream_us={} stream_spawned_builds={} stream_rebuild_regions={} build_task_us={} built_this_frame={} compaction_us={} compacted_regions={} uploaded_this_frame={} arena_compactions={} render_node_us={} packed_render_draw_calls={} packed_render_items_considered={} packed_render_gpu_pass_us={} gpu_cull_enabled={} gpu_cull_input_commands={} gpu_cull_est_visible_commands={} gpu_cull_est_visible_quads={} gpu_cull_node_us={} gpu_cull_count_supported={} gpu_cull_compact_enabled={} cpu_visible_compact_enabled={} cpu_visible_commands={} material_entities={} material_sync_us={}",
                packed_draw_mode_label(stats.draw_mode),
                stats.generated_regions_loaded,
                stats.generated_regions_active,
                stats.generated_regions_visible,
                stats.generated_update_us,
                stats.generated_update_skipped,
                stats.generated_cache_hits,
                stats.generated_cache_misses,
                stats.generated_cache_invalidated,
                stats.generated_cache_evicted,
                stats.generated_cache_prefetched,
                stats.generated_prepare_skipped,
                stats.generated_cull_metadata_uploaded,
                stats.generated_cull_config_uploaded,
                stats.generated_cull_dispatch_skipped,
                stats.visible_quads,
                stats.uploaded_quads,
                stats.indirect_draw_commands,
                stats.pending_builds,
                stats.pending_region_rebuilds,
                stats.prepare_system_us,
                stats.view_prepare_system_us,
                stats.stream_system_us,
                stats.stream_spawned_builds,
                stats.stream_rebuild_regions,
                stats.build_task_system_us,
                stats.built_this_frame,
                stats.compaction_system_us,
                stats.compacted_regions_this_frame,
                stats.uploaded_this_frame,
                stats.arena_compactions,
                stats.render_node_us,
                stats.render_draw_calls,
                stats.render_items_considered,
                stats.render_gpu_pass_us,
                stats.gpu_cull_enabled,
                stats.gpu_cull_input_commands,
                stats.gpu_cull_est_visible_commands,
                stats.gpu_cull_est_visible_quads,
                stats.gpu_cull_node_us,
                stats.gpu_cull_count_supported,
                stats.gpu_cull_compact_enabled,
                stats.cpu_visible_compact_enabled,
                stats.cpu_visible_commands,
                stats.material_entities,
                stats.material_sync_us
            );
        }
        app_exit.write(AppExit::Success);
        let _ = std::io::stdout().flush();
        std::process::exit(0);
    }
}

fn packed_draw_mode_label(draw_mode: usize) -> &'static str {
    match draw_mode {
        rumpel_render::packed_quad_pipeline::PACKED_DRAW_MODE_GPU_GENERATED => "gpu-generated",
        rumpel_render::packed_quad_pipeline::PACKED_DRAW_MODE_MATERIAL => "material",
        rumpel_render::packed_quad_pipeline::PACKED_DRAW_MODE_MULTI_INDIRECT => "multi-indirect",
        rumpel_render::packed_quad_pipeline::PACKED_DRAW_MODE_INDIRECT => "indirect",
        _ => "direct",
    }
}

fn env_flag(name: &str) -> bool {
    std::env::var(name).is_ok_and(|value| {
        matches!(
            value.to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        )
    })
}

fn env_flag_default(name: &str, default: bool) -> bool {
    std::env::var(name).map_or(default, |value| match value.to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => true,
        "0" | "false" | "no" | "off" => false,
        _ => default,
    })
}

fn env_usize(name: &str, default: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(default)
}

fn fps_from_frame_ms(frame_ms: f32) -> f32 {
    if frame_ms > 0.0 {
        1000.0 / frame_ms
    } else {
        0.0
    }
}

fn profile_frame_ready(
    frame_ms: f32,
    ready_frame_ms: f32,
    packed_stats: Option<PackedQuadPipelineStats>,
) -> bool {
    let frame_ready = frame_ms > 0.0 && (ready_frame_ms <= 0.0 || frame_ms <= ready_frame_ms);
    frame_ready && packed_profile_frame_ready(packed_stats)
}

fn packed_profile_frame_ready(packed_stats: Option<PackedQuadPipelineStats>) -> bool {
    packed_stats.is_none_or(|stats| {
        stats.pending_builds == 0
            && stats.pending_region_rebuilds == 0
            && stats.stream_spawned_builds == 0
            && stats.stream_rebuild_regions == 0
            && stats.built_this_frame == 0
            && stats.uploaded_this_frame == 0
            && stats.compacted_regions_this_frame == 0
    })
}

fn render_graph_tail_us(phase_stats: ProfilePhaseStats) -> u64 {
    phase_stats
        .render_render_us
        .saturating_sub(phase_stats.render_camera_driver_us)
}

fn render_system_tail_us(phase_stats: ProfilePhaseStats) -> u64 {
    phase_stats
        .render_system_us
        .saturating_sub(phase_stats.render_camera_driver_us)
}

fn render_prepare_resources_other_us(phase_stats: ProfilePhaseStats) -> u64 {
    let known_prepare_resources_us = phase_stats
        .render_prepare_view_uniforms_us
        .saturating_add(phase_stats.render_prepare_core_depth_textures_us)
        .saturating_add(phase_stats.render_prepare_core_transmission_textures_us)
        .saturating_add(phase_stats.render_prepare_prepass_textures_us);
    phase_stats
        .render_prepare_resources_us
        .saturating_sub(known_prepare_resources_us)
}

fn render_phase_detail_fields(prefix: &str, phase_stats: ProfilePhaseStats) -> String {
    format!(
        " {prefix}render_extract_commands_us={} {prefix}render_prepare_assets_us={} {prefix}render_prepare_meshes_us={} {prefix}render_manage_views_us={} {prefix}render_prepare_windows_us={} {prefix}render_queue_us={} {prefix}render_phase_sort_us={} {prefix}render_prepare_us={} {prefix}render_prepare_resources_us={} {prefix}render_prepare_view_uniforms_us={} {prefix}render_prepare_core_depth_textures_us={} {prefix}render_prepare_core_transmission_textures_us={} {prefix}render_prepare_prepass_textures_us={} {prefix}render_prepare_resources_other_us={} {prefix}render_prepare_resources_collect_us={} {prefix}render_prepare_resources_flush_us={} {prefix}render_prepare_bind_groups_us={} {prefix}render_prepare_after_bind_groups_us={} {prefix}render_render_us={} {prefix}render_before_render_system_us={} {prefix}render_system_us={} {prefix}render_system_tail_us={} {prefix}render_camera_driver_us={} {prefix}render_gpu_camera_driver_us={} {prefix}render_gpu_camera_driver_raw_begin={} {prefix}render_gpu_camera_driver_raw_end={} {prefix}render_gpu_camera_driver_raw_delta={} {prefix}render_gpu_camera_driver_readbacks={} {prefix}render_gpu_camera_driver_zero_deltas={} {prefix}render_gpu_camera_driver_map_failures={} {prefix}render_gpu_camera_driver_pending_readback={} {prefix}render_gpu_camera_driver_map_requested={} {prefix}render_graph_tail_us={} {prefix}render_cleanup_us={} {prefix}render_post_cleanup_us={} {prefix}render_gpu_frame_timestamps_requested={} {prefix}render_gpu_frame_timestamps_supported={}",
        phase_stats.render_extract_commands_us,
        phase_stats.render_prepare_assets_us,
        phase_stats.render_prepare_meshes_us,
        phase_stats.render_manage_views_us,
        phase_stats.render_prepare_windows_us,
        phase_stats.render_queue_us,
        phase_stats.render_phase_sort_us,
        phase_stats.render_prepare_us,
        phase_stats.render_prepare_resources_us,
        phase_stats.render_prepare_view_uniforms_us,
        phase_stats.render_prepare_core_depth_textures_us,
        phase_stats.render_prepare_core_transmission_textures_us,
        phase_stats.render_prepare_prepass_textures_us,
        render_prepare_resources_other_us(phase_stats),
        phase_stats.render_prepare_resources_collect_us,
        phase_stats.render_prepare_resources_flush_us,
        phase_stats.render_prepare_bind_groups_us,
        phase_stats.render_prepare_after_bind_groups_us,
        phase_stats.render_render_us,
        phase_stats.render_before_render_system_us,
        phase_stats.render_system_us,
        render_system_tail_us(phase_stats),
        phase_stats.render_camera_driver_us,
        phase_stats.render_gpu_camera_driver_us,
        phase_stats.render_gpu_camera_driver_raw_begin,
        phase_stats.render_gpu_camera_driver_raw_end,
        phase_stats.render_gpu_camera_driver_raw_delta,
        phase_stats.render_gpu_camera_driver_readbacks,
        phase_stats.render_gpu_camera_driver_zero_deltas,
        phase_stats.render_gpu_camera_driver_map_failures,
        phase_stats.render_gpu_camera_driver_pending_readback,
        phase_stats.render_gpu_camera_driver_map_requested,
        render_graph_tail_us(phase_stats),
        phase_stats.render_cleanup_us,
        phase_stats.render_post_cleanup_us,
        phase_stats.render_gpu_frame_timestamps_requested,
        phase_stats.render_gpu_frame_timestamps_supported,
    )
}

fn log_slow_frame(
    elapsed_seconds: f32,
    frame_ms: f32,
    bevy_delta_ms: f32,
    raw_fps: f32,
    rendered_chunk_count: usize,
    phase_stats: ProfilePhaseStats,
    packed_stats: Option<PackedQuadPipelineStats>,
) {
    let render_phase_fields = render_phase_detail_fields("", phase_stats);
    if let Some(stats) = packed_stats {
        println!(
            "profile slow_frame t={:.2}s frame_ms={:.2} bevy_delta_ms={:.2} raw_fps={:.1} chunks={} frame_wall_us={} frame_main_us={} frame_tail_us={} render_schedule_us={} render_camera_driver_us={} render_gpu_camera_driver_us={} render_graph_tail_us={} render_core3d_us={}{} packed_visible_quads={} packed_uploaded_quads={} pending_builds={} pending_region_rebuilds={} prepare_us={} view_prepare_us={} stream_us={} build_task_us={} built_this_frame={} compaction_us={} uploaded_this_frame={} render_node_us={} packed_render_gpu_pass_us={} gpu_cull_node_us={} cpu_visible_commands={} material_entities={} material_sync_us={}",
            elapsed_seconds,
            frame_ms,
            bevy_delta_ms,
            raw_fps,
            rendered_chunk_count,
            phase_stats.frame_wall_us,
            phase_stats.main_schedule_us,
            phase_stats.main_tail_us,
            phase_stats.render_schedule_us,
            phase_stats.render_camera_driver_us,
            phase_stats.render_gpu_camera_driver_us,
            render_graph_tail_us(phase_stats),
            phase_stats.render_core3d_us,
            render_phase_fields,
            stats.visible_quads,
            stats.uploaded_quads,
            stats.pending_builds,
            stats.pending_region_rebuilds,
            stats.prepare_system_us,
            stats.view_prepare_system_us,
            stats.stream_system_us,
            stats.build_task_system_us,
            stats.built_this_frame,
            stats.compaction_system_us,
            stats.uploaded_this_frame,
            stats.render_node_us,
            stats.render_gpu_pass_us,
            stats.gpu_cull_node_us,
            stats.cpu_visible_commands,
            stats.material_entities,
            stats.material_sync_us
        );
    } else {
        println!(
            "profile slow_frame t={:.2}s frame_ms={:.2} bevy_delta_ms={:.2} raw_fps={:.1} chunks={} frame_wall_us={} frame_main_us={} frame_tail_us={} render_schedule_us={} render_camera_driver_us={} render_gpu_camera_driver_us={} render_graph_tail_us={} render_core3d_us={}{}",
            elapsed_seconds,
            frame_ms,
            bevy_delta_ms,
            raw_fps,
            rendered_chunk_count,
            phase_stats.frame_wall_us,
            phase_stats.main_schedule_us,
            phase_stats.main_tail_us,
            phase_stats.render_schedule_us,
            phase_stats.render_camera_driver_us,
            phase_stats.render_gpu_camera_driver_us,
            render_graph_tail_us(phase_stats),
            phase_stats.render_core3d_us,
            render_phase_fields
        );
    }
}

fn camera_lock_enabled() -> bool {
    env_flag(CAMERA_LOCK_ENV) || env_flag(PACKED_CAMERA_LOCK_ENV)
}

fn present_mode_label() -> String {
    std::env::var(PRESENT_MODE_ENV).unwrap_or_else(|_| DEFAULT_PRESENT_MODE.to_string())
}

fn frame_latency_label() -> String {
    std::env::var(FRAME_LATENCY_ENV).unwrap_or_else(|_| DEFAULT_FRAME_LATENCY.to_string())
}

fn window_size_label() -> String {
    let width = non_empty_env(WINDOW_WIDTH_ENV);
    let height = non_empty_env(WINDOW_HEIGHT_ENV);
    match (width, height) {
        (None, None) => "default".to_string(),
        (Some(width), Some(height)) => format!("{width}x{height}"),
        _ => "invalid".to_string(),
    }
}

fn shadows_label(render_mode: Option<rumpel_render::RumpelRenderMode>) -> String {
    non_empty_env(SHADOWS_ENV).unwrap_or_else(|| {
        if matches!(
            render_mode,
            Some(rumpel_render::RumpelRenderMode::PackedPrototype)
        ) {
            "0".to_string()
        } else {
            "1".to_string()
        }
    })
}

fn debug_hud_label() -> String {
    non_empty_env(DEBUG_HUD_ENV).unwrap_or_else(|| "1".to_string())
}

fn non_empty_env(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn render_target_label() -> &'static str {
    if env_flag(HEADLESS_RENDER_ENV) {
        "headless"
    } else {
        "window"
    }
}

fn headless_wait_ms_label() -> String {
    std::env::var(HEADLESS_WAIT_MS_ENV).unwrap_or_else(|_| DEFAULT_HEADLESS_WAIT_MS.to_string())
}

fn render_gpu_frame_timestamps_label() -> String {
    std::env::var(RENDER_GPU_FRAME_TIMESTAMPS_ENV).unwrap_or_else(|_| "0".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_frame_ready_accepts_stable_non_packed_frame() {
        assert!(profile_frame_ready(12.0, 25.0, None));
    }

    #[test]
    fn profile_frame_ready_rejects_slow_frame() {
        assert!(!profile_frame_ready(31.0, 25.0, None));
    }

    #[test]
    fn profile_frame_ready_rejects_packed_streaming_activity() {
        let stats = PackedQuadPipelineStats {
            pending_builds: 1,
            ..PackedQuadPipelineStats::default()
        };
        assert!(!profile_frame_ready(12.0, 25.0, Some(stats)));

        let stats = PackedQuadPipelineStats {
            uploaded_this_frame: 1,
            ..PackedQuadPipelineStats::default()
        };
        assert!(!profile_frame_ready(12.0, 25.0, Some(stats)));
    }

    #[test]
    fn profile_frame_ready_accepts_settled_packed_frame() {
        assert!(profile_frame_ready(
            12.0,
            25.0,
            Some(PackedQuadPipelineStats::default())
        ));
    }

    #[test]
    fn ready_gate_prerolls_autopilot_before_measurement() {
        let mut profiling = ProfilingRun {
            enabled: true,
            autopilot: true,
            ready_gate: true,
            warmup_seconds: 0.0,
            ready_stable_frames_required: 2,
            ready_frame_ms: 25.0,
            ready_max_extra_seconds: 4.0,
            autopilot_preroll_seconds: 0.0,
            measurement_target_seconds: 6.0,
            ..ProfilingRun::default()
        };

        profiling.elapsed_seconds = 1.0;
        profiling.update_measurement_gate(10.0, Some(PackedQuadPipelineStats::default()));
        assert!(!profiling.autopilot_preroll_started);
        assert!(!profiling.measurement_started);

        profiling.elapsed_seconds = 1.1;
        profiling.update_measurement_gate(10.0, Some(PackedQuadPipelineStats::default()));
        assert!(profiling.autopilot_preroll_started);
        assert!(profiling.autopilot_preroll_active());
        assert!(!profiling.measurement_started);

        profiling.elapsed_seconds = 1.2;
        profiling.update_measurement_gate(10.0, Some(PackedQuadPipelineStats::default()));
        assert!(!profiling.measurement_started);

        profiling.elapsed_seconds = 1.3;
        profiling.update_measurement_gate(10.0, Some(PackedQuadPipelineStats::default()));
        assert!(profiling.measurement_started);
        assert!(!profiling.autopilot_preroll_active());
    }

    #[test]
    fn autopilot_preroll_allows_expected_streaming_activity() {
        let mut profiling = ProfilingRun {
            enabled: true,
            autopilot: true,
            ready_gate: true,
            warmup_seconds: 0.0,
            ready_stable_frames_required: 1,
            ready_frame_ms: 25.0,
            ready_max_extra_seconds: 4.0,
            autopilot_preroll_seconds: 0.0,
            measurement_target_seconds: 6.0,
            ..ProfilingRun::default()
        };

        profiling.elapsed_seconds = 1.0;
        profiling.update_measurement_gate(10.0, Some(PackedQuadPipelineStats::default()));
        assert!(profiling.autopilot_preroll_started);
        assert!(!profiling.measurement_started);

        let streaming_stats = PackedQuadPipelineStats {
            pending_builds: 4,
            uploaded_this_frame: 1,
            ..PackedQuadPipelineStats::default()
        };
        profiling.elapsed_seconds = 1.1;
        profiling.update_measurement_gate(10.0, Some(streaming_stats));
        assert!(profiling.measurement_started);
    }

    #[test]
    fn autopilot_preroll_respects_minimum_seconds() {
        let mut profiling = ProfilingRun {
            enabled: true,
            autopilot: true,
            ready_gate: true,
            warmup_seconds: 0.0,
            ready_stable_frames_required: 1,
            ready_frame_ms: 25.0,
            ready_max_extra_seconds: 4.0,
            autopilot_preroll_seconds: 2.0,
            measurement_target_seconds: 6.0,
            ..ProfilingRun::default()
        };

        profiling.elapsed_seconds = 1.0;
        profiling.update_measurement_gate(10.0, Some(PackedQuadPipelineStats::default()));
        assert!(profiling.autopilot_preroll_started);

        profiling.elapsed_seconds = 1.1;
        profiling.update_measurement_gate(10.0, Some(PackedQuadPipelineStats::default()));
        assert!(!profiling.measurement_started);

        profiling.elapsed_seconds = 3.0;
        profiling.update_measurement_gate(10.0, Some(PackedQuadPipelineStats::default()));
        assert!(profiling.measurement_started);
    }

    #[test]
    fn settle_period_excludes_initial_measurement_frames_from_counting() {
        let mut profiling = ProfilingRun {
            enabled: true,
            ready_gate: false,
            warmup_seconds: 0.0,
            settle_seconds: 1.0,
            measurement_target_seconds: 2.0,
            duration_seconds: 3.0,
            ..ProfilingRun::default()
        };

        profiling.elapsed_seconds = 0.0;
        profiling.update_measurement_gate(30.0, None);
        assert!(profiling.measurement_started);
        assert!(!profiling.counting_active());

        profiling.elapsed_seconds = 0.5;
        assert!(!profiling.counting_active());

        profiling.elapsed_seconds = 1.0;
        assert!(profiling.counting_active());
        assert!((profiling.counting_elapsed_seconds() - 0.0).abs() < f32::EPSILON);

        profiling.elapsed_seconds = 2.0;
        assert!((profiling.counting_elapsed_seconds() - 1.0).abs() < f32::EPSILON);
        assert!(!profiling.should_finish());

        profiling.elapsed_seconds = 3.0;
        assert!(profiling.should_finish());
    }
}
