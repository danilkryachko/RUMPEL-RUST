use bevy::prelude::*;
use bevy::render::extract_resource::{ExtractResource, ExtractResourcePlugin};
use bevy::render::render_asset::RenderAssets;
use bevy::render::render_resource::{
    BindGroup, BindGroupEntries, BindingResource, Buffer, BufferDescriptor, BufferUsages,
};
use bevy::render::renderer::{RenderDevice, RenderQueue};
use bevy::render::texture::GpuImage;
use bevy::render::{Render, RenderApp, RenderSystems};
use bevy::tasks::{AsyncComputeTaskPool, Task, futures::check_ready};
use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::sync::{
    Arc, LazyLock, Mutex,
    atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
};
use std::time::Instant;

use rumpel_blocks::{AIR_BLOCK_ID, BlockId, BlockRegistry};
use rumpel_prelude::ChunkPos;
use rumpel_world::chunk::WorldEditStore;
use rumpel_world::world_gen::{WorldGenerationContext, terrain_surface_contract_version};

use crate::packed_quad_gpu_generation::{
    PACKED_GPU_GENERATION_MAX_QUADS_PER_COLUMN, PackedGpuGenerationBatch,
    PackedGpuGenerationBatches, PackedGpuGenerationCacheContract, PackedGpuGenerationParams,
    PackedGpuGenerationTarget, packed_gpu_generation_columns_per_chunk,
    packed_gpu_generation_lod_for_cell_size,
};
use crate::voxel_material::load_block_atlas;
use crate::voxel_packed_quads::{PackedVoxelFace, PackedVoxelQuad};
use crate::{RenderedChunk, RenderedChunkCount};

const PACKED_INDIRECT_ENV: &str = "RUMPEL_PACKED_INDIRECT";
const PACKED_MULTI_INDIRECT_ENV: &str = "RUMPEL_PACKED_MULTI_INDIRECT";
const PACKED_TOP_ONLY_ENV: &str = "RUMPEL_PACKED_TOP_ONLY";
const PACKED_VIEW_RADIUS_ENV: &str = "RUMPEL_PACKED_VIEW_RADIUS";
const PACKED_REGION_SIZE_ENV: &str = "RUMPEL_PACKED_REGION_SIZE";
const PACKED_GPU_GENERATION_REGION_RADIUS_ENV: &str = "RUMPEL_PACKED_GPU_GENERATION_REGION_RADIUS";
const PACKED_ARENA_PREALLOC_QUADS_ENV: &str = "RUMPEL_PACKED_ARENA_PREALLOC_QUADS";
const PACKED_MIN_VRAM_BYTES_ENV: &str = "RUMPEL_PACKED_MIN_VRAM_BYTES";
const PACKED_MIN_RAM_BYTES_ENV: &str = "RUMPEL_PACKED_MIN_RAM_BYTES";
const PACKED_MAX_BUILDS_PER_FRAME_ENV: &str = "RUMPEL_PACKED_MAX_BUILDS_PER_FRAME";
const PACKED_MAX_COMPLETIONS_PER_FRAME_ENV: &str = "RUMPEL_PACKED_MAX_COMPLETIONS_PER_FRAME";
const PACKED_MAX_REBUILDS_PER_FRAME_ENV: &str = "RUMPEL_PACKED_MAX_REBUILDS_PER_FRAME";
const PACKED_MAX_COMPACTIONS_PER_FRAME_ENV: &str = "RUMPEL_PACKED_MAX_COMPACTIONS_PER_FRAME";
const PACKED_MAX_BUILD_TASKS_ENV: &str = "RUMPEL_PACKED_MAX_BUILD_TASKS";
const PACKED_LOD_ENV: &str = "RUMPEL_PACKED_LOD";
const PACKED_MIN_CELL_SIZE_ENV: &str = "RUMPEL_PACKED_MIN_CELL_SIZE";
const PACKED_FACE_RANGE_CULL_ENV: &str = "RUMPEL_PACKED_FACE_RANGE_CULL";
const PACKED_FACE_RANGE_MIN_QUADS_ENV: &str = "RUMPEL_PACKED_FACE_RANGE_MIN_QUADS";
const PACKED_GPU_CULL_ENV: &str = "RUMPEL_PACKED_GPU_CULL";
const PACKED_CPU_VISIBLE_COMPACT_ENV: &str = "RUMPEL_PACKED_CPU_VISIBLE_COMPACT";
const PACKED_ADAPTIVE_STREAMING_ENV: &str = "RUMPEL_PACKED_ADAPTIVE_STREAMING";
const PACKED_DEFER_COMPACTION_ENV: &str = "RUMPEL_PACKED_DEFER_COMPACTION";
const PACKED_ARENA_HEADROOM_ENV: &str = "RUMPEL_PACKED_ARENA_HEADROOM";
const PACKED_TARGET_FRAME_MS_ENV: &str = "RUMPEL_PACKED_TARGET_FRAME_MS";
const PACKED_FOG_START_ENV: &str = "RUMPEL_PACKED_FOG_START";
const PACKED_FOG_END_ENV: &str = "RUMPEL_PACKED_FOG_END";
const DEFAULT_PACKED_FACE_RANGE_CULL: bool = true;
const PACKED_BLOCK_PALETTE_LEN: usize = 256;
const DEFAULT_PACKED_VIEW_RADIUS: i32 = 16;
const DEFAULT_PACKED_REGION_SIZE: i32 = 4;
const PACKED_ARENA_ESTIMATED_QUADS_PER_CHUNK: usize = 640;
const DEFAULT_PACKED_MIN_VRAM_BYTES: usize = 0;
const DEFAULT_PACKED_MIN_RAM_BYTES: usize = 0;
const DEFAULT_PACKED_MAX_BUILDS_PER_FRAME: usize = 4;
const DEFAULT_PACKED_MAX_COMPLETIONS_PER_FRAME: usize = 2;
const DEFAULT_PACKED_MAX_REBUILDS_PER_FRAME: usize = 2;
const DEFAULT_PACKED_MAX_COMPACTIONS_PER_FRAME: usize = 1;
const DEFAULT_PACKED_MAX_BUILD_TASKS: usize = 64;
const DEFAULT_PACKED_MIN_CELL_SIZE: usize = 2;
const DEFAULT_PACKED_FACE_RANGE_MIN_QUADS: usize = 4096;
const DEFAULT_PACKED_ARENA_HEADROOM: usize = 2;
const DEFAULT_PACKED_DEFER_COMPACTION: bool = true;
const DEFAULT_PACKED_TARGET_FRAME_MS: f32 = 16.7;
const DEFAULT_PACKED_FOG_START: f32 = 160.0;
const DEFAULT_PACKED_FOG_END: f32 = 360.0;
const PACKED_REGION_BOUNDS_MAX_Y: f32 = 96.0;
const PACKED_MID_LOD_DISTANCE_CHUNKS: i32 = 12;
const PACKED_LOW_LOD_DISTANCE_CHUNKS: i32 = 20;
const PACKED_FAR_LOD_DISTANCE_CHUNKS: i32 = 28;

pub const PACKED_DRAW_MODE_DIRECT: usize = 0;
pub const PACKED_DRAW_MODE_INDIRECT: usize = 1;
pub const PACKED_DRAW_MODE_MULTI_INDIRECT: usize = 2;
pub const PACKED_DRAW_MODE_MATERIAL: usize = 3;
pub const PACKED_DRAW_MODE_GPU_GENERATED: usize = 4;

/// Represents a single batch of packed voxel quads on the CPU side (Main World).
#[derive(Debug, Clone)]
pub struct PackedQuadBatch {
    /// Unique identifier for this batch.
    pub key: u64,
    /// Vector of packed voxel quads.
    pub quads: Arc<Vec<PackedVoxelQuad>>,
    /// Per-chunk quad ranges inside `quads`, used to draw only the current active set.
    pub chunk_ranges: Arc<Vec<PackedQuadChunkRange>>,
    /// Subranges that changed without changing the total region quad count.
    pub dirty_ranges: Arc<Vec<PackedQuadDirtyRange>>,
    /// Incremental generation counter to identify updates.
    pub generation: u64,
    /// True while cross-chunk region compaction is pending.
    pub needs_compaction: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PackedQuadChunkRange {
    pub chunk_key: u64,
    pub start_quads: usize,
    pub len_quads: usize,
    pub capacity_quads: usize,
    pub active: bool,
    pub resident: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PackedQuadDirtyRange {
    pub start_quads: usize,
    pub len_quads: usize,
    pub generation: u64,
}

/// Resource in the Main World storing all active packed quad batches.
#[derive(Resource, Default, Clone)]
pub struct PackedQuadBatches {
    pub batches: Vec<PackedQuadBatch>,
}

// Manual compile-safe implementation of ExtractResource for Bevy 0.18 compatibility.
impl ExtractResource for PackedQuadBatches {
    type Source = Self;

    fn extract_resource(source: &Self::Source) -> Self {
        source.clone()
    }
}

#[derive(Resource, Default)]
pub struct PackedGpuGenerationRegionScratch {
    loaded_region_keys: Vec<u64>,
    active_regions: Vec<(i32, i32, u64)>,
    target_keys: HashSet<u64>,
    generated_batches: Vec<PackedGpuGenerationBatch>,
}

/// Represents a prepared batch of packed voxel quads in the Render World.
pub struct PreparedPackedQuadBatch {
    /// Unique identifier matching the CPU-side batch.
    pub key: u64,
    /// Source generation of the CPU-side batch used to avoid redundant quad scans.
    pub generation: u64,
    /// Starting offset of this batch inside the GPU storage buffer (in quads).
    pub offset_quads: usize,
    /// Number of quads currently loaded on the GPU.
    pub len_quads: usize,
    /// The translation uniform buffer.
    pub translation_buffer: Buffer,
    /// The cached bind group. Optional because it is not needed in PackedMaterial mode.
    pub bind_group: Option<BindGroup>,
    /// Conservative world-space region minimum for CPU-side frustum culling.
    pub bounds_min: Vec3,
    /// Conservative world-space region maximum for CPU-side frustum culling.
    pub bounds_max: Vec3,
    /// Contiguous face ranges inside this batch for direct CPU-side face culling.
    pub face_ranges: Vec<PackedQuadFaceRange>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PackedQuadFaceRange {
    pub face: u8,
    pub start_quads: usize,
    pub len_quads: usize,
}

/// Resource in the Render World storing prepared packed quad batches.
#[derive(Resource, Default)]
pub struct PreparedPackedQuadBatches {
    pub batches: HashMap<u64, PreparedPackedQuadBatch>,
}

/// CPU-side block texture tile palette used by the packed renderer.
#[derive(Resource, Clone)]
pub struct PackedQuadBlockTexturePalette {
    pub tiles: Vec<[u32; 4]>,
}

impl Default for PackedQuadBlockTexturePalette {
    fn default() -> Self {
        Self {
            tiles: vec![[3, 3, 3, 0]; PACKED_BLOCK_PALETTE_LEN],
        }
    }
}

impl ExtractResource for PackedQuadBlockTexturePalette {
    type Source = Self;

    fn extract_resource(source: &Self::Source) -> Self {
        source.clone()
    }
}

/// Main/render-world handle for the texture-array atlas shared with surface rendering.
#[derive(Resource, Clone)]
pub struct PackedQuadBlockAtlas {
    pub handle: Handle<Image>,
}

impl ExtractResource for PackedQuadBlockAtlas {
    type Source = Self;

    fn extract_resource(source: &Self::Source) -> Self {
        source.clone()
    }
}

/// Render-world GPU buffer for the packed block texture tile palette.
#[derive(Resource, Default)]
pub struct PreparedPackedQuadBlockTexturePalette {
    pub buffer: Option<Buffer>,
    pub tiles: Vec<[u32; 4]>,
}

/// Resource in the Render World storing the shared GPU indirect draw command buffer.
#[derive(Resource)]
pub struct PackedQuadIndirectBuffer {
    pub buffer: Option<Buffer>,
    pub capacity_commands: usize,
}

impl FromWorld for PackedQuadIndirectBuffer {
    fn from_world(_world: &mut World) -> Self {
        Self {
            buffer: None,
            capacity_commands: 0,
        }
    }
}

/// Resource in the Render World storing the structured draw parameters buffer.
#[derive(Resource)]
pub struct PackedQuadParamsBuffer {
    pub buffer: Option<Buffer>,
    pub capacity_params: usize,
}

impl FromWorld for PackedQuadParamsBuffer {
    fn from_world(_world: &mut World) -> Self {
        Self {
            buffer: None,
            capacity_params: 0,
        }
    }
}

/// Combined metadata and resources for unified indirect rendering.
#[derive(Resource, Default)]
pub struct PreparedPackedQuadIndirectDraw {
    /// Cached single global bind group.
    pub bind_group: Option<BindGroup>,
    /// Buffer holding indirect draw commands.
    pub indirect_buffer: Option<Buffer>,
    /// Number of active draw commands in the buffer.
    pub command_count: usize,
    /// CPU copy of the tightly packed draw commands for per-view visible compaction.
    pub commands: Vec<crate::packed_quad_buffer::PackedQuadDrawCommand>,
    /// CPU-side metadata for each indirect command.
    pub command_metadata: Vec<PackedQuadIndirectCommandMetadata>,
    /// Mode of drawing used (`direct`, `indirect`, `multi-indirect`, or `material`).
    pub draw_mode: String,
    /// Indication if indirect draw is supported and ready.
    pub is_indirect_enabled: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct PackedQuadIndirectCommandMetadata {
    pub batch_key: u64,
    pub face: Option<u8>,
    pub len_quads: usize,
    pub bounds_min: Vec3,
    pub bounds_max: Vec3,
}

/// Render-world buffer used to upload CPU-visible indirect commands for one-view rendering.
#[derive(Resource)]
pub struct PackedQuadCpuVisibleIndirectBuffer {
    pub buffer: Option<Buffer>,
    pub capacity_commands: usize,
}

impl FromWorld for PackedQuadCpuVisibleIndirectBuffer {
    fn from_world(_world: &mut World) -> Self {
        Self {
            buffer: None,
            capacity_commands: 0,
        }
    }
}

/// Render-world resources prepared for opt-in GPU culling of indirect commands.
#[derive(Resource)]
pub struct PreparedPackedQuadGpuCull {
    pub enabled: bool,
    pub compact_enabled: bool,
    pub count_supported: bool,
    pub command_count: usize,
    pub capacity_commands: usize,
    pub metadata_buffer: Option<Buffer>,
    pub config_buffer: Option<Buffer>,
    pub output_indirect_buffer: Option<Buffer>,
    pub count_buffer: Option<Buffer>,
    pub bind_group: Option<BindGroup>,
    pub source_signature: u64,
    pub metadata_signature: u64,
    pub config_signature: u64,
    dispatched: AtomicBool,
    dispatched_signature: AtomicU64,
    last_visible_commands: AtomicUsize,
    last_visible_quads: AtomicUsize,
}

impl PreparedPackedQuadGpuCull {
    pub fn reset_dispatched(&self) {
        self.dispatched.store(false, Ordering::Release);
        self.dispatched_signature.store(0, Ordering::Release);
    }

    pub fn mark_dispatched(
        &self,
        dispatch_signature: u64,
        visible_commands: usize,
        visible_quads: usize,
    ) {
        self.last_visible_commands
            .store(visible_commands, Ordering::Release);
        self.last_visible_quads
            .store(visible_quads, Ordering::Release);
        self.dispatched_signature
            .store(dispatch_signature, Ordering::Release);
        self.dispatched.store(true, Ordering::Release);
    }

    pub fn was_dispatched(&self) -> bool {
        self.dispatched.load(Ordering::Acquire)
    }

    pub fn was_dispatched_for(&self, dispatch_signature: u64) -> bool {
        dispatch_signature != 0
            && self.was_dispatched()
            && self.dispatched_signature.load(Ordering::Acquire) == dispatch_signature
    }

    pub fn last_visible_commands(&self) -> usize {
        self.last_visible_commands.load(Ordering::Acquire)
    }

    pub fn last_visible_quads(&self) -> usize {
        self.last_visible_quads.load(Ordering::Acquire)
    }

    pub fn disable(&mut self) {
        self.enabled = false;
        self.compact_enabled = false;
        self.count_supported = false;
        self.command_count = 0;
        self.bind_group = None;
        self.source_signature = 0;
        self.metadata_signature = 0;
        self.config_signature = 0;
        self.reset_dispatched();
    }
}

impl FromWorld for PreparedPackedQuadGpuCull {
    fn from_world(_world: &mut World) -> Self {
        Self {
            enabled: false,
            compact_enabled: false,
            count_supported: false,
            command_count: 0,
            capacity_commands: 0,
            metadata_buffer: None,
            config_buffer: None,
            output_indirect_buffer: None,
            count_buffer: None,
            bind_group: None,
            source_signature: 0,
            metadata_signature: 0,
            config_signature: 0,
            dispatched: AtomicBool::new(false),
            dispatched_signature: AtomicU64::new(0),
            last_visible_commands: AtomicUsize::new(0),
            last_visible_quads: AtomicUsize::new(0),
        }
    }
}

/// Resource in the Render World storing the shared GPU storage buffer and allocations.
#[derive(Resource)]
pub struct PackedQuadGpuArena {
    /// The single large GPU storage buffer housing all quads.
    pub buffer: Option<Buffer>,
    /// Maximum capacity of quads the current buffer can hold.
    pub capacity_quads: usize,
    /// Next never-reused free quad offset for stable region slots.
    pub next_free_quads: usize,
    /// Map of active allocations by chunk key.
    pub allocations: HashMap<u64, crate::packed_quad_buffer::PackedQuadArenaAllocation>,
    /// Accumulated arena stats.
    pub stats: crate::packed_quad_buffer::PackedQuadArenaStats,
    /// Generation counter incremented whenever the GPU buffer is reallocated.
    pub generation: u64,
}

impl FromWorld for PackedQuadGpuArena {
    fn from_world(_world: &mut World) -> Self {
        Self {
            buffer: None,
            capacity_quads: 0,
            next_free_quads: 0,
            allocations: HashMap::default(),
            stats: crate::packed_quad_buffer::PackedQuadArenaStats {
                total_capacity_quads: 0,
                ..Default::default()
            },
            generation: 1,
        }
    }
}

/// Dedicated GPU memory reservation kept separate from the bindable quad arena.
#[derive(Resource, Default)]
pub struct PackedQuadGpuMemoryReserve {
    pub buffers: Vec<Buffer>,
    pub total_bytes: u64,
}

/// Metric stats for the packed quad pipeline in the Main World.
#[derive(Resource, Default, Debug, Clone, Copy)]
pub struct PackedQuadPipelineStats {
    /// Number of active batches.
    pub batches: usize,
    /// Number of active chunk columns feeding those batches.
    pub chunks_loaded: usize,
    /// Number of loaded chunk columns inside the strict render radius.
    pub chunks_active: usize,
    /// Total per-chunk ranges tracked inside packed region batches.
    pub chunk_ranges: usize,
    /// Resident per-chunk ranges that can participate in drawing.
    pub resident_chunk_ranges: usize,
    /// Non-resident tombstone ranges retained for slot reuse.
    pub tombstone_chunk_ranges: usize,
    /// Reserved quad capacity held by resident ranges.
    pub resident_range_capacity_quads: usize,
    /// Reserved quad capacity held by non-resident tombstone ranges.
    pub tombstone_capacity_quads: usize,
    /// Dirty subranges waiting to be considered by render-world upload planning.
    pub dirty_ranges: usize,
    /// Total quad span covered by dirty subranges.
    pub dirty_range_quads: usize,
    /// Total number of quads on CPU.
    pub quads: usize,
    /// Total number of uploaded quads on GPU.
    pub uploaded_quads: usize,
    /// Total number of dropped quads due to capacity limits.
    pub dropped_quads: usize,
    /// Total number of bytes written to the GPU.
    pub uploaded_bytes: u64,
    /// Allocated GPU buffer capacity in quads.
    pub buffer_capacity_quads: usize,
    /// Number of pending builds in streaming queue.
    pub pending_builds: usize,
    /// Number of pending active-region rebuilds waiting for throttled processing.
    pub pending_region_rebuilds: usize,
    /// CPU time spent in the render-world packed prepare system in microseconds.
    pub prepare_system_us: u64,
    /// CPU time spent preparing per-view packed camera resources in microseconds.
    pub view_prepare_system_us: u64,
    /// CPU time spent in the main-world packed streaming system in microseconds.
    pub stream_system_us: u64,
    /// Number of async build tasks spawned by packed streaming in the current frame.
    pub stream_spawned_builds: usize,
    /// Number of packed region batches rebuilt by streaming in the current frame.
    pub stream_rebuild_regions: usize,
    /// CPU time spent polling/completing packed build tasks in microseconds.
    pub build_task_system_us: u64,
    /// Chunks built in the current frame.
    pub built_this_frame: usize,
    /// CPU time spent compacting deferred packed regions in microseconds.
    pub compaction_system_us: u64,
    /// Deferred packed regions compacted in the current frame.
    pub compacted_regions_this_frame: usize,
    /// Batches uploaded in the current frame.
    pub uploaded_this_frame: usize,
    /// Number of region batches drawn by the direct packed renderer after view culling.
    pub visible_batches: usize,
    /// Number of quads drawn by the direct packed renderer after view culling.
    pub visible_quads: usize,
    /// CPU-side reserved capacity for packed region quad vectors.
    pub cpu_reserved_quads: usize,
    /// CPU-side reserved capacity in bytes for packed region quad vectors.
    pub cpu_reserved_bytes: u64,
    /// Runtime minimum packed GPU arena reservation in bytes.
    pub min_vram_bytes: u64,
    /// Runtime minimum packed CPU reservation target in bytes.
    pub min_ram_bytes: u64,
    /// Dedicated GPU memory reserved for packed rendering in bytes.
    pub gpu_reserved_bytes: u64,

    // --- Arena metrics ---
    /// Total capacity of the GPU arena storage buffer.
    pub arena_capacity_quads: usize,
    /// Total quads actively used in the arena.
    pub arena_used_quads: usize,
    /// Total quads occupied by stable arena slots before the free tail.
    pub arena_slot_quads: usize,
    /// Total bytes uploaded to the arena.
    pub arena_uploaded_bytes: u64,
    /// Cumulative count of buffer reallocations.
    pub arena_reallocations: usize,
    /// Cumulative count of in-place stable allocation compactions.
    pub arena_compactions: usize,

    // --- Indirect draw metrics ---
    /// Number of active indirect draw commands.
    pub indirect_draw_commands: usize,
    /// Draw mode (0: direct, 1: indirect, 2: multi-indirect, 3: material)
    pub draw_mode: usize,
    /// Number of Bevy material entities used by the experimental material path.
    pub material_entities: usize,
    /// CPU time spent synchronizing experimental material entities in microseconds.
    pub material_sync_us: u64,
    /// CPU time spent encoding the packed render graph node in microseconds.
    pub render_node_us: u64,
    /// Actual draw calls encoded by the packed render graph node after view culling.
    pub render_draw_calls: usize,
    /// Candidate batches or indirect commands considered by the render graph node.
    pub render_items_considered: usize,
    /// Latest measured GPU time for the packed render pass in microseconds.
    pub render_gpu_pass_us: u64,
    /// Whether packed GPU timestamp profiling was requested.
    pub gpu_timestamps_requested: bool,
    /// Whether the current renderer device supports packed GPU timestamp profiling.
    pub gpu_timestamps_supported: bool,
    /// Whether opt-in packed GPU command culling is prepared for the current frame.
    pub gpu_cull_enabled: bool,
    /// Indirect command count fed into the GPU cull compute pass.
    pub gpu_cull_input_commands: usize,
    /// CPU-side estimate of commands that the GPU cull pass should leave visible.
    pub gpu_cull_est_visible_commands: usize,
    /// CPU-side estimate of quads that the GPU cull pass should leave visible.
    pub gpu_cull_est_visible_quads: usize,
    /// CPU time spent encoding the packed GPU cull graph node in microseconds.
    pub gpu_cull_node_us: u64,
    /// Whether the current device can use indirect-count drawing for compacted GPU cull output.
    pub gpu_cull_count_supported: bool,
    /// Whether the current frame uses compacted GPU cull output plus indirect-count drawing.
    pub gpu_cull_compact_enabled: bool,
    /// Whether the render node used CPU-visible compact indirect submission this frame.
    pub cpu_visible_compact_enabled: bool,
    /// Number of per-view visible indirect commands uploaded by the CPU compact fallback.
    pub cpu_visible_commands: usize,
    /// Generated-region window size around the camera target (loaded cache).
    pub generated_regions_loaded: usize,
    /// Generated regions intersecting the strict chunk view radius.
    pub generated_regions_active: usize,
    /// Generated regions drawn after per-view frustum culling.
    pub generated_regions_visible: usize,
    /// CPU time spent in the main-world GPU-generated region update system.
    pub generated_update_us: u64,
    /// Whether the main-world GPU-generated region update skipped stable target planning.
    pub generated_update_skipped: bool,
    /// Generated regions reused from the CPU-side source cache this frame.
    pub generated_cache_hits: usize,
    /// Generated regions built because no reusable cache entry was available this frame.
    pub generated_cache_misses: usize,
    /// Generated cache entries invalidated by source-contract or edit-store changes this frame.
    pub generated_cache_invalidated: usize,
    /// Generated cache entries evicted because they left the loaded region window this frame.
    pub generated_cache_evicted: usize,
    /// Whether GPU-generated render prepare reused already prepared region resources this frame.
    pub generated_prepare_skipped: bool,
    /// Whether GPU-generated cull metadata was uploaded this frame.
    pub generated_cull_metadata_uploaded: bool,
    /// Whether GPU-generated cull config was uploaded this frame.
    pub generated_cull_config_uploaded: bool,
    /// Whether GPU-generated cull dispatch reused the previous culled output this frame.
    pub generated_cull_dispatch_skipped: bool,
}

struct PackedQuadMetricsBridge {
    batches: AtomicUsize,
    chunks_loaded: AtomicUsize,
    chunks_active: AtomicUsize,
    chunk_ranges: AtomicUsize,
    resident_chunk_ranges: AtomicUsize,
    tombstone_chunk_ranges: AtomicUsize,
    resident_range_capacity_quads: AtomicUsize,
    tombstone_capacity_quads: AtomicUsize,
    dirty_ranges: AtomicUsize,
    dirty_range_quads: AtomicUsize,
    quads: AtomicUsize,
    uploaded_quads: AtomicUsize,
    dropped_quads: AtomicUsize,
    uploaded_bytes: AtomicU64,
    buffer_capacity_quads: AtomicUsize,
    pending_builds: AtomicUsize,
    pending_region_rebuilds: AtomicUsize,
    prepare_system_us: AtomicU64,
    view_prepare_system_us: AtomicU64,
    stream_system_us: AtomicU64,
    stream_spawned_builds: AtomicUsize,
    stream_rebuild_regions: AtomicUsize,
    build_task_system_us: AtomicU64,
    built_this_frame: AtomicUsize,
    compaction_system_us: AtomicU64,
    compacted_regions_this_frame: AtomicUsize,
    uploaded_this_frame: AtomicUsize,
    visible_batches: AtomicUsize,
    visible_quads: AtomicUsize,
    cpu_reserved_quads: AtomicUsize,
    cpu_reserved_bytes: AtomicU64,
    gpu_reserved_bytes: AtomicU64,

    // --- Arena metrics ---
    arena_capacity_quads: AtomicUsize,
    arena_used_quads: AtomicUsize,
    arena_slot_quads: AtomicUsize,
    arena_uploaded_bytes: AtomicU64,
    arena_reallocations: AtomicUsize,
    arena_compactions: AtomicUsize,

    // --- Indirect draw metrics ---
    indirect_draw_commands: AtomicUsize,
    draw_mode: AtomicUsize,
    material_entities: AtomicUsize,
    material_sync_us: AtomicU64,
    render_node_us: AtomicU64,
    render_draw_calls: AtomicUsize,
    render_items_considered: AtomicUsize,
    render_gpu_pass_us: AtomicU64,
    gpu_timestamps_requested: AtomicUsize,
    gpu_timestamps_supported: AtomicUsize,
    gpu_cull_enabled: AtomicUsize,
    gpu_cull_input_commands: AtomicUsize,
    gpu_cull_est_visible_commands: AtomicUsize,
    gpu_cull_est_visible_quads: AtomicUsize,
    gpu_cull_node_us: AtomicU64,
    gpu_cull_count_supported: AtomicUsize,
    gpu_cull_compact_enabled: AtomicUsize,
    cpu_visible_compact_enabled: AtomicUsize,
    cpu_visible_commands: AtomicUsize,
    generated_regions_loaded: AtomicUsize,
    generated_regions_active: AtomicUsize,
    generated_regions_visible: AtomicUsize,
    generated_update_us: AtomicU64,
    generated_update_skipped: AtomicUsize,
    generated_cache_hits: AtomicUsize,
    generated_cache_misses: AtomicUsize,
    generated_cache_invalidated: AtomicUsize,
    generated_cache_evicted: AtomicUsize,
    generated_prepare_skipped: AtomicUsize,
    generated_cull_metadata_uploaded: AtomicUsize,
    generated_cull_config_uploaded: AtomicUsize,
    generated_cull_dispatch_skipped: AtomicUsize,
}

static METRICS_BRIDGE: PackedQuadMetricsBridge = PackedQuadMetricsBridge {
    batches: AtomicUsize::new(0),
    chunks_loaded: AtomicUsize::new(0),
    chunks_active: AtomicUsize::new(0),
    chunk_ranges: AtomicUsize::new(0),
    resident_chunk_ranges: AtomicUsize::new(0),
    tombstone_chunk_ranges: AtomicUsize::new(0),
    resident_range_capacity_quads: AtomicUsize::new(0),
    tombstone_capacity_quads: AtomicUsize::new(0),
    dirty_ranges: AtomicUsize::new(0),
    dirty_range_quads: AtomicUsize::new(0),
    quads: AtomicUsize::new(0),
    uploaded_quads: AtomicUsize::new(0),
    dropped_quads: AtomicUsize::new(0),
    uploaded_bytes: AtomicU64::new(0),
    buffer_capacity_quads: AtomicUsize::new(0),
    pending_builds: AtomicUsize::new(0),
    pending_region_rebuilds: AtomicUsize::new(0),
    prepare_system_us: AtomicU64::new(0),
    view_prepare_system_us: AtomicU64::new(0),
    stream_system_us: AtomicU64::new(0),
    stream_spawned_builds: AtomicUsize::new(0),
    stream_rebuild_regions: AtomicUsize::new(0),
    build_task_system_us: AtomicU64::new(0),
    built_this_frame: AtomicUsize::new(0),
    compaction_system_us: AtomicU64::new(0),
    compacted_regions_this_frame: AtomicUsize::new(0),
    uploaded_this_frame: AtomicUsize::new(0),
    visible_batches: AtomicUsize::new(0),
    visible_quads: AtomicUsize::new(0),
    cpu_reserved_quads: AtomicUsize::new(0),
    cpu_reserved_bytes: AtomicU64::new(0),
    gpu_reserved_bytes: AtomicU64::new(0),

    // --- Arena metrics ---
    arena_capacity_quads: AtomicUsize::new(0),
    arena_used_quads: AtomicUsize::new(0),
    arena_slot_quads: AtomicUsize::new(0),
    arena_uploaded_bytes: AtomicU64::new(0),
    arena_reallocations: AtomicUsize::new(0),
    arena_compactions: AtomicUsize::new(0),

    // --- Indirect draw metrics ---
    indirect_draw_commands: AtomicUsize::new(0),
    draw_mode: AtomicUsize::new(0),
    material_entities: AtomicUsize::new(0),
    material_sync_us: AtomicU64::new(0),
    render_node_us: AtomicU64::new(0),
    render_draw_calls: AtomicUsize::new(0),
    render_items_considered: AtomicUsize::new(0),
    render_gpu_pass_us: AtomicU64::new(0),
    gpu_timestamps_requested: AtomicUsize::new(0),
    gpu_timestamps_supported: AtomicUsize::new(0),
    gpu_cull_enabled: AtomicUsize::new(0),
    gpu_cull_input_commands: AtomicUsize::new(0),
    gpu_cull_est_visible_commands: AtomicUsize::new(0),
    gpu_cull_est_visible_quads: AtomicUsize::new(0),
    gpu_cull_node_us: AtomicU64::new(0),
    gpu_cull_count_supported: AtomicUsize::new(0),
    gpu_cull_compact_enabled: AtomicUsize::new(0),
    cpu_visible_compact_enabled: AtomicUsize::new(0),
    cpu_visible_commands: AtomicUsize::new(0),
    generated_regions_loaded: AtomicUsize::new(0),
    generated_regions_active: AtomicUsize::new(0),
    generated_regions_visible: AtomicUsize::new(0),
    generated_update_us: AtomicU64::new(0),
    generated_update_skipped: AtomicUsize::new(0),
    generated_cache_hits: AtomicUsize::new(0),
    generated_cache_misses: AtomicUsize::new(0),
    generated_cache_invalidated: AtomicUsize::new(0),
    generated_cache_evicted: AtomicUsize::new(0),
    generated_prepare_skipped: AtomicUsize::new(0),
    generated_cull_metadata_uploaded: AtomicUsize::new(0),
    generated_cull_config_uploaded: AtomicUsize::new(0),
    generated_cull_dispatch_skipped: AtomicUsize::new(0),
};

static CONFIRMED_PACKED_BATCH_GENERATIONS: LazyLock<Mutex<HashMap<u64, u64>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Pure helper function to determine the next buffer capacity when growing is required.
pub fn next_packed_quad_capacity(current_capacity: usize, requested_capacity: usize) -> usize {
    if current_capacity >= requested_capacity {
        return current_capacity;
    }
    if requested_capacity == 0 {
        return 0;
    }

    let mut capacity = requested_capacity.next_power_of_two();
    if capacity < 16 {
        capacity = 16;
    }
    capacity
}

pub fn packed_quad_slot_capacity(requested_capacity: usize) -> usize {
    if requested_capacity == 0 {
        return 0;
    }

    requested_capacity.max(16)
}

pub fn packed_chunk_count_for_radius(view_radius: i32) -> usize {
    let radius = view_radius.max(0);
    let radius_sq = radius * radius;
    let mut count = 0;
    for dz in -radius..=radius {
        for dx in -radius..=radius {
            if dx * dx + dz * dz <= radius_sq {
                count += 1;
            }
        }
    }
    count
}

pub fn packed_region_count_for_radius(view_radius: i32, region_size: i32) -> usize {
    let radius = view_radius.max(0);
    let size = region_size.max(1);
    let radius_sq = radius * radius;
    let mut regions = HashSet::new();
    for dz in -radius..=radius {
        for dx in -radius..=radius {
            if dx * dx + dz * dz <= radius_sq {
                regions.insert(packed_region_origin_for_chunk(dx, dz, size));
            }
        }
    }
    regions.len()
}

pub fn estimated_packed_region_capacity_quads(region_size: i32) -> usize {
    let size = region_size.max(1) as usize;
    size * size * PACKED_ARENA_ESTIMATED_QUADS_PER_CHUNK
}

pub fn estimated_packed_arena_quads_for_radius(view_radius: i32) -> usize {
    estimated_packed_arena_quads_for_radius_and_region_size(
        view_radius,
        packed_region_size_from_env(),
    )
}

pub fn estimated_packed_arena_quads_for_radius_and_region_size(
    view_radius: i32,
    region_size: i32,
) -> usize {
    packed_region_count_for_radius(view_radius, region_size)
        * estimated_packed_region_capacity_quads(region_size)
}

pub fn packed_region_origin_for_chunk(chunk_x: i32, chunk_z: i32, region_size: i32) -> (i32, i32) {
    let size = region_size.max(1);
    (
        chunk_x.div_euclid(size) * size,
        chunk_z.div_euclid(size) * size,
    )
}

pub fn offset_quads_to_region(
    quads: &[PackedVoxelQuad],
    chunk_x: i32,
    chunk_z: i32,
    region_origin_x: i32,
    region_origin_z: i32,
) -> Vec<PackedVoxelQuad> {
    let offset_x = (chunk_x - region_origin_x) * 32;
    let offset_z = (chunk_z - region_origin_z) * 32;
    debug_assert!(offset_x >= 0);
    debug_assert!(offset_z >= 0);

    quads
        .iter()
        .copied()
        .map(|mut quad| {
            quad.origin[0] = quad.origin[0].saturating_add(offset_x as u16);
            quad.origin[2] = quad.origin[2].saturating_add(offset_z as u16);
            quad
        })
        .collect()
}

pub fn packed_region_world_bounds(region_key: u64, region_size: i32) -> (Vec3, Vec3) {
    let (region_x, region_z) = unpack_chunk_key(region_key);
    let size_blocks = region_size.max(1) * 32;
    let min = Vec3::new((region_x * 32) as f32, 0.0, (region_z * 32) as f32);
    let max = Vec3::new(
        (region_x * 32 + size_blocks) as f32,
        PACKED_REGION_BOUNDS_MAX_Y,
        (region_z * 32 + size_blocks) as f32,
    );
    (min, max)
}

/// Returns true when a generated region should be drawn for the current view.
#[must_use]
pub fn generated_region_bounds_visible(
    view_position: Vec3,
    clip_from_world: Mat4,
    bounds_min: Vec3,
    bounds_max: Vec3,
) -> bool {
    view_position.x >= bounds_min.x
        && view_position.x <= bounds_max.x
        && view_position.y >= bounds_min.y
        && view_position.y <= bounds_max.y
        && view_position.z >= bounds_min.z
        && view_position.z <= bounds_max.z
        || aabb_intersects_clip_frustum(clip_from_world, bounds_min, bounds_max)
}

#[must_use]
pub fn aabb_intersects_clip_frustum(
    clip_from_world: Mat4,
    bounds_min: Vec3,
    bounds_max: Vec3,
) -> bool {
    let corners = [
        Vec3::new(bounds_min.x, bounds_min.y, bounds_min.z),
        Vec3::new(bounds_max.x, bounds_min.y, bounds_min.z),
        Vec3::new(bounds_min.x, bounds_max.y, bounds_min.z),
        Vec3::new(bounds_max.x, bounds_max.y, bounds_min.z),
        Vec3::new(bounds_min.x, bounds_min.y, bounds_max.z),
        Vec3::new(bounds_max.x, bounds_min.y, bounds_max.z),
        Vec3::new(bounds_min.x, bounds_max.y, bounds_max.z),
        Vec3::new(bounds_max.x, bounds_max.y, bounds_max.z),
    ];

    let mut outside_left = true;
    let mut outside_right = true;
    let mut outside_bottom = true;
    let mut outside_top = true;
    let mut outside_near = true;
    let mut outside_far = true;

    for corner in corners {
        let clip = clip_from_world * corner.extend(1.0);
        outside_left &= clip.x < -clip.w;
        outside_right &= clip.x > clip.w;
        outside_bottom &= clip.y < -clip.w;
        outside_top &= clip.y > clip.w;
        outside_near &= clip.z < 0.0;
        outside_far &= clip.z > clip.w;
    }

    !(outside_left || outside_right || outside_bottom || outside_top || outside_near || outside_far)
}

pub fn packed_chunk_world_bounds(chunk_key: u64) -> (Vec3, Vec3) {
    let (chunk_x, chunk_z) = unpack_chunk_key(chunk_key);
    let min = Vec3::new((chunk_x * 32) as f32, 0.0, (chunk_z * 32) as f32);
    let max = Vec3::new(
        (chunk_x * 32 + 32) as f32,
        PACKED_REGION_BOUNDS_MAX_Y,
        (chunk_z * 32 + 32) as f32,
    );
    (min, max)
}

pub fn packed_quad_face_ranges(quads: &[PackedVoxelQuad]) -> Vec<PackedQuadFaceRange> {
    let mut ranges = Vec::new();
    let Some(first) = quads.first() else {
        return ranges;
    };

    let mut current_face = first.face();
    let mut start_quads = 0;
    let mut len_quads = 0;
    for (index, quad) in quads.iter().enumerate() {
        let face = quad.face();
        if face != current_face {
            ranges.push(PackedQuadFaceRange {
                face: current_face,
                start_quads,
                len_quads,
            });
            current_face = face;
            start_quads = index;
            len_quads = 0;
        }
        len_quads += 1;
    }

    ranges.push(PackedQuadFaceRange {
        face: current_face,
        start_quads,
        len_quads,
    });
    ranges
}

struct PackedIndirectCommandInput {
    translation: Vec4,
    batch_key: u64,
    bounds_min: Vec3,
    bounds_max: Vec3,
    start_quads: usize,
    len_quads: usize,
    face: Option<u8>,
}

struct PackedChunkRangeIndirectInput<'a> {
    batch: &'a PackedQuadBatch,
    range: PackedQuadChunkRange,
    allocation_len_quads: usize,
    translation: Vec4,
    face_range_cull_enabled: bool,
    face_range_min_quads: usize,
}

fn push_packed_indirect_command(
    commands_staging: &mut Vec<crate::packed_quad_buffer::PackedQuadDrawCommand>,
    params_staging: &mut Vec<crate::packed_quad_buffer::PackedQuadDrawParams>,
    command_metadata: &mut Vec<PackedQuadIndirectCommandMetadata>,
    input: PackedIndirectCommandInput,
) {
    if input.len_quads == 0 {
        return;
    }

    let command_index = params_staging.len();
    params_staging.push(crate::packed_quad_buffer::PackedQuadDrawParams {
        chunk_offset: input.translation.to_array(),
    });
    commands_staging.push(crate::packed_quad_buffer::PackedQuadDrawCommand {
        vertex_count: input.len_quads.saturating_mul(6).min(u32::MAX as usize) as u32,
        instance_count: 1,
        first_vertex: input.start_quads.saturating_mul(6).min(u32::MAX as usize) as u32,
        first_instance: command_index.min(u32::MAX as usize) as u32,
    });
    command_metadata.push(PackedQuadIndirectCommandMetadata {
        batch_key: input.batch_key,
        face: input.face,
        len_quads: input.len_quads,
        bounds_min: input.bounds_min,
        bounds_max: input.bounds_max,
    });
}

pub fn packed_gpu_cull_metadata_from_command(
    command: PackedQuadIndirectCommandMetadata,
) -> crate::packed_quad_buffer::PackedQuadCullCommandMetadata {
    crate::packed_quad_buffer::PackedQuadCullCommandMetadata {
        bounds_min: command.bounds_min.extend(0.0).to_array(),
        bounds_max: command.bounds_max.extend(0.0).to_array(),
        meta: [
            command.len_quads.min(u32::MAX as usize) as u32,
            command.face.map_or(0, |face| u32::from(face) + 1),
            0,
            0,
        ],
    }
}

fn should_split_packed_face_ranges(
    face_range_cull_enabled: bool,
    batch_quads: usize,
    face_ranges: &[PackedQuadFaceRange],
    min_split_quads: usize,
) -> bool {
    face_range_cull_enabled && face_ranges.len() > 1 && batch_quads >= min_split_quads
}

fn push_packed_chunk_range_indirect_commands(
    commands_staging: &mut Vec<crate::packed_quad_buffer::PackedQuadDrawCommand>,
    params_staging: &mut Vec<crate::packed_quad_buffer::PackedQuadDrawParams>,
    command_metadata: &mut Vec<PackedQuadIndirectCommandMetadata>,
    input: PackedChunkRangeIndirectInput<'_>,
) {
    let batch = input.batch;
    let range = input.range;
    if !range.resident || !range.active || range.len_quads == 0 {
        return;
    }

    let start_quads = range.start_quads.min(input.allocation_len_quads);
    let len_quads = range
        .len_quads
        .min(input.allocation_len_quads.saturating_sub(start_quads));
    if len_quads == 0 {
        return;
    }

    let (bounds_min, bounds_max) = packed_chunk_world_bounds(range.chunk_key);
    let end_quads = start_quads.saturating_add(len_quads).min(batch.quads.len());
    let len_quads = end_quads.saturating_sub(start_quads);
    if len_quads == 0 {
        return;
    }

    if input.face_range_cull_enabled && !batch.needs_compaction {
        let face_ranges = packed_quad_face_ranges(&batch.quads[start_quads..end_quads]);
        if should_split_packed_face_ranges(
            true,
            len_quads,
            &face_ranges,
            input.face_range_min_quads,
        ) {
            for face_range in face_ranges {
                push_packed_indirect_command(
                    commands_staging,
                    params_staging,
                    command_metadata,
                    PackedIndirectCommandInput {
                        translation: input.translation,
                        batch_key: batch.key,
                        bounds_min,
                        bounds_max,
                        start_quads: start_quads + face_range.start_quads,
                        len_quads: face_range.len_quads,
                        face: Some(face_range.face),
                    },
                );
            }
            return;
        }
    }

    push_packed_indirect_command(
        commands_staging,
        params_staging,
        command_metadata,
        PackedIndirectCommandInput {
            translation: input.translation,
            batch_key: batch.key,
            bounds_min,
            bounds_max,
            start_quads,
            len_quads,
            face: None,
        },
    );
}

fn packed_view_radius_from_env() -> i32 {
    std::env::var(PACKED_VIEW_RADIUS_ENV)
        .ok()
        .and_then(|val| val.parse::<i32>().ok())
        .unwrap_or(DEFAULT_PACKED_VIEW_RADIUS)
        .max(0)
}

fn packed_face_range_min_quads_from_env() -> usize {
    std::env::var(PACKED_FACE_RANGE_MIN_QUADS_ENV)
        .ok()
        .and_then(|val| val.parse::<usize>().ok())
        .unwrap_or(DEFAULT_PACKED_FACE_RANGE_MIN_QUADS)
}

fn packed_region_size_from_env() -> i32 {
    std::env::var(PACKED_REGION_SIZE_ENV)
        .ok()
        .and_then(|val| val.parse::<i32>().ok())
        .unwrap_or(DEFAULT_PACKED_REGION_SIZE)
        .max(1)
}

fn is_packed_material_mode() -> bool {
    std::env::var("RUMPEL_RENDER_MODE").is_ok_and(|mode| {
        matches!(
            mode.to_ascii_lowercase().as_str(),
            "packed_material" | "packed-material" | "material" | "material-packed"
        )
    })
}

fn packed_arena_initial_capacity_quads() -> usize {
    let requested = std::env::var(PACKED_ARENA_PREALLOC_QUADS_ENV)
        .ok()
        .and_then(|val| val.parse::<usize>().ok())
        .unwrap_or_else(|| {
            estimated_packed_arena_quads_for_radius(packed_view_radius_from_env())
                .saturating_mul(packed_arena_headroom_from_env())
        });

    next_packed_quad_capacity(0, requested)
}

fn packed_arena_headroom_from_env() -> usize {
    std::env::var(PACKED_ARENA_HEADROOM_ENV)
        .ok()
        .and_then(|val| val.parse::<usize>().ok())
        .unwrap_or(DEFAULT_PACKED_ARENA_HEADROOM)
        .max(1)
}

fn packed_min_vram_bytes_from_env() -> usize {
    std::env::var(PACKED_MIN_VRAM_BYTES_ENV)
        .ok()
        .and_then(|val| val.parse::<usize>().ok())
        .unwrap_or(DEFAULT_PACKED_MIN_VRAM_BYTES)
}

fn packed_min_ram_bytes_from_env() -> usize {
    std::env::var(PACKED_MIN_RAM_BYTES_ENV)
        .ok()
        .and_then(|val| val.parse::<usize>().ok())
        .unwrap_or(DEFAULT_PACKED_MIN_RAM_BYTES)
}

fn packed_max_builds_per_frame_from_env() -> usize {
    std::env::var(PACKED_MAX_BUILDS_PER_FRAME_ENV)
        .ok()
        .and_then(|val| val.parse::<usize>().ok())
        .unwrap_or(DEFAULT_PACKED_MAX_BUILDS_PER_FRAME)
}

fn packed_max_completions_per_frame_from_env() -> usize {
    std::env::var(PACKED_MAX_COMPLETIONS_PER_FRAME_ENV)
        .ok()
        .and_then(|val| val.parse::<usize>().ok())
        .unwrap_or(DEFAULT_PACKED_MAX_COMPLETIONS_PER_FRAME)
}

fn packed_max_rebuilds_per_frame_from_env() -> usize {
    std::env::var(PACKED_MAX_REBUILDS_PER_FRAME_ENV)
        .ok()
        .and_then(|val| val.parse::<usize>().ok())
        .unwrap_or(DEFAULT_PACKED_MAX_REBUILDS_PER_FRAME)
}

fn packed_max_compactions_per_frame_from_env() -> usize {
    std::env::var(PACKED_MAX_COMPACTIONS_PER_FRAME_ENV)
        .ok()
        .and_then(|val| val.parse::<usize>().ok())
        .unwrap_or(DEFAULT_PACKED_MAX_COMPACTIONS_PER_FRAME)
}

fn packed_defer_compaction_from_env() -> bool {
    env_flag_default(PACKED_DEFER_COMPACTION_ENV, DEFAULT_PACKED_DEFER_COMPACTION)
}

fn packed_max_build_tasks_from_env() -> usize {
    std::env::var(PACKED_MAX_BUILD_TASKS_ENV)
        .ok()
        .and_then(|val| val.parse::<usize>().ok())
        .unwrap_or(DEFAULT_PACKED_MAX_BUILD_TASKS)
}

fn packed_target_frame_secs_from_env() -> f32 {
    let target_ms = std::env::var(PACKED_TARGET_FRAME_MS_ENV)
        .ok()
        .and_then(|val| val.parse::<f32>().ok())
        .unwrap_or(DEFAULT_PACKED_TARGET_FRAME_MS)
        .max(1.0);
    target_ms / 1000.0
}

fn adaptive_packed_streaming_budget(
    requested_budget: usize,
    delta_secs: f32,
    target_frame_secs: f32,
    adaptive_enabled: bool,
) -> usize {
    if !adaptive_enabled || requested_budget <= 1 || delta_secs <= 0.0 {
        return requested_budget;
    }

    if delta_secs > target_frame_secs * 2.0 {
        1
    } else if delta_secs > target_frame_secs * 1.25 {
        requested_budget.div_ceil(2).max(1)
    } else {
        requested_budget
    }
}

fn adaptive_packed_background_budget(
    requested_budget: usize,
    delta_secs: f32,
    target_frame_secs: f32,
    adaptive_enabled: bool,
) -> usize {
    if !adaptive_enabled || requested_budget == 0 || delta_secs <= target_frame_secs * 1.25 {
        requested_budget
    } else {
        0
    }
}

fn packed_min_cell_size_from_env() -> usize {
    std::env::var(PACKED_MIN_CELL_SIZE_ENV)
        .ok()
        .and_then(|val| val.parse::<usize>().ok())
        .unwrap_or(DEFAULT_PACKED_MIN_CELL_SIZE)
        .clamp(1, rumpel_world::chunk::CHUNK_SIZE)
}

fn packed_lod_step_for_distance_sq(distance_sq: i32, min_cell_size: usize) -> usize {
    let step = if distance_sq >= PACKED_FAR_LOD_DISTANCE_CHUNKS * PACKED_FAR_LOD_DISTANCE_CHUNKS {
        8
    } else if distance_sq >= PACKED_LOW_LOD_DISTANCE_CHUNKS * PACKED_LOW_LOD_DISTANCE_CHUNKS {
        4
    } else if distance_sq >= PACKED_MID_LOD_DISTANCE_CHUNKS * PACKED_MID_LOD_DISTANCE_CHUNKS {
        2
    } else {
        1
    };

    step.max(min_cell_size)
}

fn packed_cpu_region_prealloc_quads_from_env() -> usize {
    let view_radius = packed_view_radius_from_env();
    let region_size = packed_region_size_from_env();
    let region_count = packed_region_count_for_radius(view_radius, region_size).max(1);
    let min_ram_quads =
        packed_min_ram_bytes_from_env().div_ceil(std::mem::size_of::<PackedVoxelQuad>());
    let min_region_quads = min_ram_quads.div_ceil(region_count);
    estimated_packed_region_capacity_quads(region_size).max(min_region_quads)
}

fn ensure_packed_gpu_memory_reserve(
    render_device: &RenderDevice,
    reserve: &mut PackedQuadGpuMemoryReserve,
) {
    let target_bytes = packed_min_vram_bytes_from_env() as u64;
    if reserve.total_bytes >= target_bytes {
        METRICS_BRIDGE
            .gpu_reserved_bytes
            .store(reserve.total_bytes, Ordering::Relaxed);
        return;
    }

    let limits = render_device.limits();
    let max_buffer_size = limits.max_buffer_size.max(1);
    let reserve_chunk_bytes = max_buffer_size.min(256 * 1024 * 1024);

    while reserve.total_bytes < target_bytes {
        let remaining = target_bytes - reserve.total_bytes;
        let size = remaining.min(reserve_chunk_bytes).max(1);
        let label = format!("packed_quad_gpu_memory_reserve_{}", reserve.buffers.len());
        let buffer = render_device.create_buffer(&BufferDescriptor {
            label: Some(label.as_str()),
            size,
            usage: BufferUsages::COPY_SRC | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        reserve.buffers.push(buffer);
        reserve.total_bytes += size;
    }

    METRICS_BRIDGE
        .gpu_reserved_bytes
        .store(reserve.total_bytes, Ordering::Relaxed);
}

/// Calculates the estimated bytes required for a given number of quads.
#[inline]
pub fn estimate_quads_bytes(quads_count: usize) -> u64 {
    (quads_count as u64) * 16
}

/// Helper to update prepared quad batch metadata.
#[inline]
pub fn update_prepared_metadata(
    generation: &mut u64,
    len_quads: &mut usize,
    new_generation: u64,
    new_len: usize,
) {
    *generation = new_generation;
    *len_quads = new_len;
}

/// Helper to pack a slice of extracted batches sorted by key contiguously.
/// Returns the CPU staging buffer of quads, the allocations map, and the total required quads.
pub fn pack_arena_allocations(
    batches: &[PackedQuadBatch],
) -> (
    Vec<PackedVoxelQuad>,
    HashMap<u64, crate::packed_quad_buffer::PackedQuadArenaAllocation>,
    usize,
) {
    let mut sorted_batches = batches.iter().collect::<Vec<_>>();
    sorted_batches.sort_by_key(|batch| batch.key);

    let total_quads: usize = sorted_batches.iter().map(|batch| batch.quads.len()).sum();
    let mut staging = Vec::with_capacity(total_quads);
    let mut allocations = HashMap::new();
    let mut current_offset = 0;

    for batch in sorted_batches {
        let len = batch.quads.len();
        staging.extend_from_slice(batch.quads.as_slice());
        allocations.insert(
            batch.key,
            crate::packed_quad_buffer::PackedQuadArenaAllocation {
                key: batch.key,
                offset_quads: current_offset,
                len_quads: len,
                capacity_quads: len,
                generation: batch.generation,
            },
        );
        current_offset += len;
    }

    (staging, allocations, total_quads)
}

fn batch_has_dirty_ranges_since(batch: &PackedQuadBatch, generation: u64) -> bool {
    batch
        .dirty_ranges
        .iter()
        .any(|range| range.generation > generation && range.len_quads > 0)
}

pub fn plan_stable_arena_allocations(
    existing_allocations: &HashMap<u64, crate::packed_quad_buffer::PackedQuadArenaAllocation>,
    batches: &[PackedQuadBatch],
    next_free_quads: usize,
    default_batch_capacity_quads: usize,
) -> (
    HashMap<u64, crate::packed_quad_buffer::PackedQuadArenaAllocation>,
    Vec<u64>,
    usize,
    usize,
) {
    let mut sorted_batches = batches.iter().collect::<Vec<_>>();
    sorted_batches.sort_by_key(|batch| batch.key);

    let mut new_allocations = HashMap::with_capacity(sorted_batches.len());
    let mut dirty_keys = Vec::new();
    let mut next_free = next_free_quads.max(
        existing_allocations
            .values()
            .map(|allocation| allocation.offset_quads + allocation.capacity_quads)
            .max()
            .unwrap_or(0),
    );
    let mut total_required_quads = 0;

    for batch in sorted_batches {
        let len = batch.quads.len();
        total_required_quads += len;

        let allocation = if let Some(existing) = existing_allocations.get(&batch.key)
            && len <= existing.capacity_quads
        {
            let can_upload_dirty_ranges = existing.len_quads == len
                && existing.generation != batch.generation
                && batch_has_dirty_ranges_since(batch, existing.generation);
            if (existing.generation != batch.generation || existing.len_quads != len)
                && !can_upload_dirty_ranges
            {
                dirty_keys.push(batch.key);
            }
            crate::packed_quad_buffer::PackedQuadArenaAllocation {
                key: batch.key,
                offset_quads: existing.offset_quads,
                len_quads: len,
                capacity_quads: existing.capacity_quads,
                generation: batch.generation,
            }
        } else {
            let requested_capacity = len.max(default_batch_capacity_quads);
            let capacity = packed_quad_slot_capacity(requested_capacity);
            let allocation = crate::packed_quad_buffer::PackedQuadArenaAllocation {
                key: batch.key,
                offset_quads: next_free,
                len_quads: len,
                capacity_quads: capacity,
                generation: batch.generation,
            };
            next_free += capacity;
            dirty_keys.push(batch.key);
            allocation
        };

        new_allocations.insert(batch.key, allocation);
    }

    (new_allocations, dirty_keys, total_required_quads, next_free)
}

fn should_compact_packed_arena_slots(
    current_capacity_quads: usize,
    planned_next_free: usize,
) -> bool {
    current_capacity_quads > 0 && planned_next_free > current_capacity_quads
}

fn compacted_packed_arena_allocation_plan(
    batches: &[PackedQuadBatch],
    default_batch_capacity_quads: usize,
) -> (
    HashMap<u64, crate::packed_quad_buffer::PackedQuadArenaAllocation>,
    Vec<u64>,
    usize,
    usize,
) {
    plan_stable_arena_allocations(&HashMap::new(), batches, 0, default_batch_capacity_quads)
}

/// Bevy Render stage system that extracts, allocates, reuses, and uploads
/// packed quad buffers during Bevy's `Prepare` stage.
#[allow(clippy::too_many_arguments)]
pub fn prepare_packed_quad_buffers(
    render_device: Res<RenderDevice>,
    render_queue: Res<RenderQueue>,
    pipeline: Option<Res<crate::packed_quad_renderer::PackedQuadPipeline>>,
    atlas: Res<PackedQuadBlockAtlas>,
    gpu_images: Res<RenderAssets<GpuImage>>,
    extracted_batches: Res<PackedQuadBatches>,
    extracted_palette: Res<PackedQuadBlockTexturePalette>,
    mut prepared_batches: ResMut<PreparedPackedQuadBatches>,
    mut prepared_palette: ResMut<PreparedPackedQuadBlockTexturePalette>,
    mut arena: ResMut<PackedQuadGpuArena>,
    mut gpu_reserve: ResMut<PackedQuadGpuMemoryReserve>,
    mut indirect_buf: ResMut<PackedQuadIndirectBuffer>,
    mut params_buf: ResMut<PackedQuadParamsBuffer>,
    mut cpu_visible_indirect_buf: ResMut<PackedQuadCpuVisibleIndirectBuffer>,
    mut indirect_draw: ResMut<PreparedPackedQuadIndirectDraw>,
    mut gpu_cull: ResMut<PreparedPackedQuadGpuCull>,
) {
    let system_started_at = Instant::now();
    let pipeline_res = pipeline.as_deref();

    let Some(gpu_atlas) = gpu_images.get(&atlas.handle) else {
        METRICS_BRIDGE
            .prepare_system_us
            .store(elapsed_us(system_started_at), Ordering::Relaxed);
        return;
    };

    if prepared_palette.buffer.is_none() || prepared_palette.tiles != extracted_palette.tiles {
        let size_bytes = (extracted_palette.tiles.len() * std::mem::size_of::<[u32; 4]>()) as u64;
        let buffer = render_device.create_buffer(&BufferDescriptor {
            label: Some("packed_quad_block_texture_palette_buffer"),
            size: size_bytes,
            usage: BufferUsages::STORAGE | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        render_queue.write_buffer(&buffer, 0, bytemuck::cast_slice(&extracted_palette.tiles));
        prepared_palette.buffer = Some(buffer);
        prepared_palette.tiles = extracted_palette.tiles.clone();
    }

    let Some(texture_palette_buffer) = prepared_palette.buffer.as_ref() else {
        return;
    };
    ensure_packed_gpu_memory_reserve(&render_device, &mut gpu_reserve);

    // 1. Evict prepared batches that are no longer present in CPU extracted batches
    prepared_batches
        .batches
        .retain(|key, _| extracted_batches.batches.iter().any(|b| b.key == *key));

    // 2. Compute stable per-region allocations. Offsets do not shift when a
    // region grows, so unchanged regions keep their GPU data and bind group.
    let default_batch_capacity_quads =
        estimated_packed_region_capacity_quads(packed_region_size_from_env());
    let previous_allocations = arena.allocations.clone();
    let (mut new_allocations, mut dirty_batch_keys, mut total_required_quads, mut next_free_quads) =
        plan_stable_arena_allocations(
            &arena.allocations,
            &extracted_batches.batches,
            arena.next_free_quads,
            default_batch_capacity_quads,
        );
    if should_compact_packed_arena_slots(arena.capacity_quads, next_free_quads) {
        let (
            compacted_allocations,
            compacted_dirty_batch_keys,
            compacted_total_required_quads,
            compacted_next_free_quads,
        ) = compacted_packed_arena_allocation_plan(
            &extracted_batches.batches,
            default_batch_capacity_quads,
        );
        if compacted_next_free_quads <= arena.capacity_quads {
            new_allocations = compacted_allocations;
            dirty_batch_keys = compacted_dirty_batch_keys;
            total_required_quads = compacted_total_required_quads;
            next_free_quads = compacted_next_free_quads;
            arena.stats.compactions += 1;
        }
    }

    let mut arena_reallocated = false;
    let mut uploaded_bytes = 0;
    let mut uploaded_quads = 0;

    // 3. Grow/reallocate arena buffer if stable slots exceed capacity.
    if arena.buffer.is_none() || next_free_quads > arena.capacity_quads {
        let next_capacity = if arena.buffer.is_none() {
            next_packed_quad_capacity(packed_arena_initial_capacity_quads(), next_free_quads)
        } else {
            next_packed_quad_capacity(arena.capacity_quads, next_free_quads)
        };
        let size_bytes = next_capacity as u64 * 16;

        let label = "packed_quad_arena_buffer";
        let new_buffer = render_device.create_buffer(&BufferDescriptor {
            label: Some(label),
            size: size_bytes,
            usage: BufferUsages::STORAGE | BufferUsages::COPY_DST | BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });

        let is_lazy_init = arena.capacity_quads == 0;
        arena.buffer = Some(new_buffer);
        arena.capacity_quads = next_capacity;
        arena.stats.total_capacity_quads = next_capacity;
        if !is_lazy_init {
            arena.stats.reallocations += 1;
        }
        arena.generation += 1;
        arena_reallocated = true;
    }

    if arena_reallocated {
        dirty_batch_keys = extracted_batches
            .batches
            .iter()
            .map(|batch| batch.key)
            .collect();
    }

    let dirty_batch_keys = dirty_batch_keys.into_iter().collect::<HashSet<_>>();
    let mut uploaded_batches = 0;

    // 4. Upload only changed stable ranges. A buffer reallocation marks all
    // active ranges dirty because the new GPU buffer starts empty.
    if let Some(arena_buffer) = arena.buffer.as_ref() {
        for batch in &extracted_batches.batches {
            let is_full_dirty = dirty_batch_keys.contains(&batch.key);
            if batch.quads.is_empty() {
                continue;
            }
            let Some(allocation) = new_allocations.get(&batch.key) else {
                continue;
            };
            if is_full_dirty {
                render_queue.write_buffer(
                    arena_buffer,
                    estimate_quads_bytes(allocation.offset_quads),
                    bytemuck::cast_slice(batch.quads.as_slice()),
                );
                uploaded_bytes += estimate_quads_bytes(batch.quads.len());
                uploaded_quads += batch.quads.len();
                uploaded_batches += 1;
                continue;
            }

            let Some(previous_allocation) = previous_allocations.get(&batch.key) else {
                continue;
            };
            if previous_allocation.generation == batch.generation
                || previous_allocation.len_quads != allocation.len_quads
                || previous_allocation.offset_quads != allocation.offset_quads
            {
                continue;
            }

            let mut uploaded_subranges_for_batch = false;
            for range in batch
                .dirty_ranges
                .iter()
                .filter(|range| range.generation > previous_allocation.generation)
            {
                let start_quads = range.start_quads.min(batch.quads.len());
                let end_quads = start_quads
                    .saturating_add(range.len_quads)
                    .min(batch.quads.len());
                if start_quads == end_quads {
                    continue;
                }
                render_queue.write_buffer(
                    arena_buffer,
                    estimate_quads_bytes(allocation.offset_quads + start_quads),
                    bytemuck::cast_slice(&batch.quads[start_quads..end_quads]),
                );
                let len_quads = end_quads - start_quads;
                uploaded_bytes += estimate_quads_bytes(len_quads);
                uploaded_quads += len_quads;
                uploaded_subranges_for_batch = true;
            }
            uploaded_batches += usize::from(uploaded_subranges_for_batch);
        }
    }
    arena.allocations = new_allocations.clone();
    record_confirmed_packed_batch_generations(&new_allocations);
    arena.next_free_quads = next_free_quads;
    arena.stats.used_quads = total_required_quads;
    arena.stats.allocated_slot_quads = next_free_quads;
    arena.stats.free_quads = arena.capacity_quads.saturating_sub(total_required_quads);
    arena.stats.uploaded_bytes = uploaded_bytes;

    // 6. Sort extracted batches by key to match allocations order deterministically
    let mut sorted_batches = extracted_batches.batches.iter().collect::<Vec<_>>();
    sorted_batches.sort_by_key(|batch| batch.key);

    let face_range_cull_enabled =
        env_flag_default(PACKED_FACE_RANGE_CULL_ENV, DEFAULT_PACKED_FACE_RANGE_CULL);
    let face_range_min_quads = packed_face_range_min_quads_from_env();
    let estimated_command_capacity = sorted_batches
        .iter()
        .map(|batch| {
            batch
                .chunk_ranges
                .iter()
                .filter(|range| range.resident && range.active && range.len_quads > 0)
                .count()
                .max(usize::from(batch.chunk_ranges.is_empty()))
        })
        .sum::<usize>()
        .max(sorted_batches.len());
    let mut commands_staging = Vec::with_capacity(estimated_command_capacity);
    let mut params_staging = Vec::with_capacity(estimated_command_capacity);
    let mut command_metadata = Vec::with_capacity(estimated_command_capacity);
    let region_size = packed_region_size_from_env();

    // 7. Update direct prepared batches and build indirect command buffers.
    for batch in sorted_batches {
        let allocation = new_allocations.get(&batch.key).copied().unwrap_or(
            crate::packed_quad_buffer::PackedQuadArenaAllocation {
                key: batch.key,
                offset_quads: 0,
                len_quads: 0,
                capacity_quads: 0,
                generation: batch.generation,
            },
        );

        let (tx, tz) = unpack_chunk_key(batch.key);
        let (bounds_min, bounds_max) = packed_region_world_bounds(batch.key, region_size);
        let existing_generation = prepared_batches
            .batches
            .get(&batch.key)
            .map(|prepared| prepared.generation);
        let face_ranges = if existing_generation == Some(batch.generation) {
            prepared_batches
                .batches
                .get(&batch.key)
                .map(|prepared| Cow::Borrowed(prepared.face_ranges.as_slice()))
                .unwrap_or_else(|| Cow::Owned(packed_quad_face_ranges(batch.quads.as_slice())))
        } else {
            Cow::Owned(packed_quad_face_ranges(batch.quads.as_slice()))
        };
        // Translation Vec4: x, y, z are world translations, w is the base quad offset as float!
        let translation = Vec4::new(
            (tx * 32) as f32,
            0.0,
            (tz * 32) as f32,
            allocation.offset_quads as f32,
        );
        let translation_uniform = packed_material_uniform(translation);

        if batch.chunk_ranges.is_empty() {
            if should_split_packed_face_ranges(
                face_range_cull_enabled && !batch.needs_compaction,
                allocation.len_quads,
                face_ranges.as_ref(),
                face_range_min_quads,
            ) {
                for range in face_ranges.as_ref() {
                    push_packed_indirect_command(
                        &mut commands_staging,
                        &mut params_staging,
                        &mut command_metadata,
                        PackedIndirectCommandInput {
                            translation,
                            batch_key: batch.key,
                            bounds_min,
                            bounds_max,
                            start_quads: range.start_quads,
                            len_quads: range.len_quads,
                            face: Some(range.face),
                        },
                    );
                }
            } else {
                push_packed_indirect_command(
                    &mut commands_staging,
                    &mut params_staging,
                    &mut command_metadata,
                    PackedIndirectCommandInput {
                        translation,
                        batch_key: batch.key,
                        bounds_min,
                        bounds_max,
                        start_quads: 0,
                        len_quads: allocation.len_quads,
                        face: None,
                    },
                );
            }
        } else {
            for range in batch.chunk_ranges.iter().copied() {
                push_packed_chunk_range_indirect_commands(
                    &mut commands_staging,
                    &mut params_staging,
                    &mut command_metadata,
                    PackedChunkRangeIndirectInput {
                        batch,
                        range,
                        allocation_len_quads: allocation.len_quads,
                        translation,
                        face_range_cull_enabled,
                        face_range_min_quads,
                    },
                );
            }
        }

        let updated_face_ranges = match face_ranges {
            Cow::Owned(face_ranges) => Some(face_ranges),
            Cow::Borrowed(_) => None,
        };

        // Keep prepared batches for the direct packed render path.
        if let Some(prepared) = prepared_batches.batches.get_mut(&batch.key) {
            let layout_or_offset_changed = prepared.offset_quads != allocation.offset_quads
                || prepared.len_quads != allocation.len_quads
                || arena_reallocated;
            prepared.generation = batch.generation;
            prepared.bounds_min = bounds_min;
            prepared.bounds_max = bounds_max;
            if let Some(face_ranges) = updated_face_ranges {
                prepared.face_ranges = face_ranges;
            }

            if layout_or_offset_changed {
                render_queue.write_buffer(
                    &prepared.translation_buffer,
                    0,
                    bytemuck::bytes_of(&translation_uniform),
                );

                let bind_group = pipeline_res.map(|pipeline| {
                    let bind_group_label = format!("packed_quad_batch_bind_group_{}", batch.key);
                    render_device.create_bind_group(
                        Some(bind_group_label.as_str()),
                        &pipeline.quad_bind_group_layout,
                        &BindGroupEntries::sequential((
                            arena.buffer.as_ref().unwrap().as_entire_buffer_binding(),
                            prepared.translation_buffer.as_entire_buffer_binding(),
                            texture_palette_buffer.as_entire_buffer_binding(),
                            BindingResource::TextureView(&gpu_atlas.texture_view),
                            BindingResource::Sampler(&gpu_atlas.sampler),
                        )),
                    )
                });

                prepared.bind_group = bind_group;
                prepared.offset_quads = allocation.offset_quads;
                prepared.len_quads = allocation.len_quads;
            }
        } else {
            // Fresh prepared batch: allocate translation uniform and cached bind group
            let trans_label = format!("packed_quad_translation_{}", batch.key);
            let translation_buffer = render_device.create_buffer(&BufferDescriptor {
                label: Some(trans_label.as_str()),
                size: crate::packed_quad_material::PackedVoxelUniform::SIZE,
                usage: BufferUsages::STORAGE | BufferUsages::COPY_DST | BufferUsages::UNIFORM,
                mapped_at_creation: false,
            });
            render_queue.write_buffer(
                &translation_buffer,
                0,
                bytemuck::bytes_of(&translation_uniform),
            );

            let bind_group = pipeline_res.map(|pipeline| {
                let bind_group_label = format!("packed_quad_batch_bind_group_{}", batch.key);
                render_device.create_bind_group(
                    Some(bind_group_label.as_str()),
                    &pipeline.quad_bind_group_layout,
                    &BindGroupEntries::sequential((
                        arena.buffer.as_ref().unwrap().as_entire_buffer_binding(),
                        translation_buffer.as_entire_buffer_binding(),
                        texture_palette_buffer.as_entire_buffer_binding(),
                        BindingResource::TextureView(&gpu_atlas.texture_view),
                        BindingResource::Sampler(&gpu_atlas.sampler),
                    )),
                )
            });

            prepared_batches.batches.insert(
                batch.key,
                PreparedPackedQuadBatch {
                    key: batch.key,
                    generation: batch.generation,
                    offset_quads: allocation.offset_quads,
                    len_quads: allocation.len_quads,
                    translation_buffer,
                    bind_group,
                    bounds_min,
                    bounds_max,
                    face_ranges: updated_face_ranges
                        .unwrap_or_else(|| packed_quad_face_ranges(batch.quads.as_slice())),
                },
            );
        }
    }

    let command_count = commands_staging.len();

    // 8. Reallocate & upload to the unified GPU indirect buffer
    if command_count > 0 {
        // Resize indirect buffer if needed
        if indirect_buf.buffer.is_none() || command_count > indirect_buf.capacity_commands {
            let next_capacity = if indirect_buf.buffer.is_none() {
                command_count.max(256).next_power_of_two()
            } else {
                command_count
                    .next_power_of_two()
                    .max(indirect_buf.capacity_commands * 2)
            };
            let size_bytes = next_capacity as u64
                * std::mem::size_of::<crate::packed_quad_buffer::PackedQuadDrawCommand>() as u64;

            let label = "packed_quad_indirect_draw_buffer";
            let new_buffer = render_device.create_buffer(&BufferDescriptor {
                label: Some(label),
                size: size_bytes,
                usage: BufferUsages::INDIRECT | BufferUsages::STORAGE | BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });

            indirect_buf.buffer = Some(new_buffer);
            indirect_buf.capacity_commands = next_capacity;
        }

        // Resize draw params buffer if needed
        if params_buf.buffer.is_none() || command_count > params_buf.capacity_params {
            let next_capacity = if params_buf.buffer.is_none() {
                command_count.max(256).next_power_of_two()
            } else {
                command_count
                    .next_power_of_two()
                    .max(params_buf.capacity_params * 2)
            };
            let size_bytes = next_capacity as u64
                * std::mem::size_of::<crate::packed_quad_buffer::PackedQuadDrawParams>() as u64;

            let label = "packed_quad_draw_params_buffer";
            let new_buffer = render_device.create_buffer(&BufferDescriptor {
                label: Some(label),
                size: size_bytes,
                usage: BufferUsages::STORAGE | BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });

            params_buf.buffer = Some(new_buffer);
            params_buf.capacity_params = next_capacity;
        }

        // Upload to command buffers
        if let Some(buf) = &indirect_buf.buffer {
            render_queue.write_buffer(buf, 0, bytemuck::cast_slice(&commands_staging));
        }
        if let Some(buf) = &params_buf.buffer {
            render_queue.write_buffer(buf, 0, bytemuck::cast_slice(&params_staging));
        }
    }

    // 9. Build the single unified global BindGroup for the drawing pass
    let mut global_bind_group = None;
    if let Some(pipeline) = pipeline_res
        && command_count > 0
        && let (Some(arena_buffer), Some(params_buffer)) = (&arena.buffer, &params_buf.buffer)
    {
        let bind_group_label = "packed_quad_indirect_global_bind_group";
        let bg = render_device.create_bind_group(
            Some(bind_group_label),
            &pipeline.quad_bind_group_layout,
            &BindGroupEntries::sequential((
                arena_buffer.as_entire_buffer_binding(),
                params_buffer.as_entire_buffer_binding(),
                texture_palette_buffer.as_entire_buffer_binding(),
                BindingResource::TextureView(&gpu_atlas.texture_view),
                BindingResource::Sampler(&gpu_atlas.sampler),
            )),
        );
        global_bind_group = Some(bg);
    }

    // Prefer the validated loop-indirect packed path when first_instance is
    // supported. Keep direct available through RUMPEL_PACKED_INDIRECT=0, and
    // keep multi-indirect as a separate opt-in until it has visual acceptance.
    let indirect_requested = env_flag_default(PACKED_INDIRECT_ENV, true);
    let multi_indirect_requested = env_flag(PACKED_MULTI_INDIRECT_ENV);
    let has_first_instance = render_device
        .features()
        .contains(bevy::render::render_resource::WgpuFeatures::INDIRECT_FIRST_INSTANCE);
    let has_indirect_count = render_device
        .features()
        .contains(bevy::render::render_resource::WgpuFeatures::MULTI_DRAW_INDIRECT_COUNT);
    let use_indirect = indirect_requested && has_first_instance;
    let material_mode = is_packed_material_mode();
    let draw_mode = if material_mode {
        "material".to_string()
    } else if use_indirect && multi_indirect_requested {
        "multi-indirect".to_string()
    } else if use_indirect {
        "indirect".to_string()
    } else {
        "direct".to_string()
    };
    let mode_code = match draw_mode.as_str() {
        "material" => PACKED_DRAW_MODE_MATERIAL,
        "multi-indirect" => PACKED_DRAW_MODE_MULTI_INDIRECT,
        "indirect" => PACKED_DRAW_MODE_INDIRECT,
        _ => PACKED_DRAW_MODE_DIRECT,
    };

    let gpu_cull_requested = env_flag(PACKED_GPU_CULL_ENV);
    let gpu_cull_enabled =
        !material_mode && gpu_cull_requested && use_indirect && command_count > 0;
    let gpu_cull_compact_enabled = gpu_cull_enabled && has_indirect_count;
    let cpu_visible_compact_requested = env_flag_default(PACKED_CPU_VISIBLE_COMPACT_ENV, true);
    if use_indirect
        && cpu_visible_compact_requested
        && command_count > 0
        && (cpu_visible_indirect_buf.buffer.is_none()
            || command_count > cpu_visible_indirect_buf.capacity_commands)
    {
        let next_capacity = if cpu_visible_indirect_buf.capacity_commands == 0 {
            command_count.max(256).next_power_of_two()
        } else {
            command_count
                .next_power_of_two()
                .max(cpu_visible_indirect_buf.capacity_commands * 2)
        };
        let size_bytes = next_capacity as u64
            * std::mem::size_of::<crate::packed_quad_buffer::PackedQuadDrawCommand>() as u64;
        cpu_visible_indirect_buf.buffer = Some(render_device.create_buffer(&BufferDescriptor {
            label: Some("packed_quad_cpu_visible_indirect_buffer"),
            size: size_bytes,
            usage: BufferUsages::INDIRECT | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        }));
        cpu_visible_indirect_buf.capacity_commands = next_capacity;
    }

    if gpu_cull_enabled {
        let next_capacity = if gpu_cull.metadata_buffer.is_none()
            || gpu_cull.output_indirect_buffer.is_none()
            || command_count > gpu_cull.capacity_commands
        {
            if gpu_cull.capacity_commands == 0 {
                command_count.max(256).next_power_of_two()
            } else {
                command_count
                    .next_power_of_two()
                    .max(gpu_cull.capacity_commands * 2)
            }
        } else {
            gpu_cull.capacity_commands
        };

        if next_capacity != gpu_cull.capacity_commands
            || gpu_cull.metadata_buffer.is_none()
            || gpu_cull.output_indirect_buffer.is_none()
        {
            let metadata_size_bytes = next_capacity as u64
                * std::mem::size_of::<crate::packed_quad_buffer::PackedQuadCullCommandMetadata>()
                    as u64;
            gpu_cull.metadata_buffer = Some(render_device.create_buffer(&BufferDescriptor {
                label: Some("packed_quad_gpu_cull_metadata_buffer"),
                size: metadata_size_bytes,
                usage: BufferUsages::STORAGE | BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));

            let output_size_bytes = next_capacity as u64
                * std::mem::size_of::<crate::packed_quad_buffer::PackedQuadDrawCommand>() as u64;
            gpu_cull.output_indirect_buffer =
                Some(render_device.create_buffer(&BufferDescriptor {
                    label: Some("packed_quad_gpu_cull_output_indirect_buffer"),
                    size: output_size_bytes,
                    usage: BufferUsages::INDIRECT | BufferUsages::STORAGE | BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                }));
            gpu_cull.capacity_commands = next_capacity;
        }

        if gpu_cull.config_buffer.is_none() {
            gpu_cull.config_buffer = Some(render_device.create_buffer(&BufferDescriptor {
                label: Some("packed_quad_gpu_cull_config_buffer"),
                size: std::mem::size_of::<crate::packed_quad_buffer::PackedQuadCullConfig>() as u64,
                usage: BufferUsages::STORAGE | BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
        }
        if gpu_cull.count_buffer.is_none() {
            gpu_cull.count_buffer = Some(render_device.create_buffer(&BufferDescriptor {
                label: Some("packed_quad_gpu_cull_count_buffer"),
                size: std::mem::size_of::<u32>() as u64,
                usage: BufferUsages::INDIRECT | BufferUsages::STORAGE | BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
        }

        let cull_metadata = command_metadata
            .iter()
            .copied()
            .map(packed_gpu_cull_metadata_from_command)
            .collect::<Vec<_>>();
        let cull_config = crate::packed_quad_buffer::PackedQuadCullConfig {
            command_count: command_count.min(u32::MAX as usize) as u32,
            face_range_cull: u32::from(face_range_cull_enabled),
            compact_output: u32::from(gpu_cull_compact_enabled),
            _padding: 0,
        };

        if let Some(metadata_buffer) = &gpu_cull.metadata_buffer {
            render_queue.write_buffer(metadata_buffer, 0, bytemuck::cast_slice(&cull_metadata));
        }
        if let Some(config_buffer) = &gpu_cull.config_buffer {
            render_queue.write_buffer(config_buffer, 0, bytemuck::bytes_of(&cull_config));
        }

        gpu_cull.enabled = true;
        gpu_cull.compact_enabled = gpu_cull_compact_enabled;
        gpu_cull.count_supported = has_indirect_count;
        gpu_cull.command_count = command_count;
        gpu_cull.bind_group = None;
        gpu_cull.source_signature = 0;
        gpu_cull.metadata_signature = 0;
        gpu_cull.config_signature = 0;
        gpu_cull.reset_dispatched();
    } else {
        gpu_cull.disable();
    }
    record_packed_quad_gpu_cull_prepare(
        gpu_cull.enabled,
        gpu_cull.command_count,
        gpu_cull.count_supported,
        gpu_cull.compact_enabled,
    );
    record_packed_quad_cpu_visible_indirect(false, 0);

    *indirect_draw = PreparedPackedQuadIndirectDraw {
        bind_group: global_bind_group,
        indirect_buffer: indirect_buf.buffer.clone(),
        command_count,
        commands: commands_staging,
        command_metadata,
        draw_mode,
        is_indirect_enabled: use_indirect && command_count > 0,
    };

    let mut chunk_ranges = 0;
    let mut resident_chunk_ranges = 0;
    let mut tombstone_chunk_ranges = 0;
    let mut resident_range_capacity_quads = 0;
    let mut tombstone_capacity_quads = 0;
    let mut dirty_ranges = 0;
    let mut dirty_range_quads = 0;
    for batch in &extracted_batches.batches {
        chunk_ranges += batch.chunk_ranges.len();
        dirty_ranges += batch.dirty_ranges.len();
        dirty_range_quads += batch
            .dirty_ranges
            .iter()
            .map(|range| range.len_quads)
            .sum::<usize>();
        for range in batch.chunk_ranges.iter() {
            if range.resident {
                resident_chunk_ranges += 1;
                resident_range_capacity_quads += range.capacity_quads;
            } else {
                tombstone_chunk_ranges += 1;
                tombstone_capacity_quads += range.capacity_quads;
            }
        }
    }

    // 7. Store metrics in the atomic bridge for Main World access
    METRICS_BRIDGE
        .batches
        .store(extracted_batches.batches.len(), Ordering::Relaxed);
    METRICS_BRIDGE
        .chunk_ranges
        .store(chunk_ranges, Ordering::Relaxed);
    METRICS_BRIDGE
        .resident_chunk_ranges
        .store(resident_chunk_ranges, Ordering::Relaxed);
    METRICS_BRIDGE
        .tombstone_chunk_ranges
        .store(tombstone_chunk_ranges, Ordering::Relaxed);
    METRICS_BRIDGE
        .resident_range_capacity_quads
        .store(resident_range_capacity_quads, Ordering::Relaxed);
    METRICS_BRIDGE
        .tombstone_capacity_quads
        .store(tombstone_capacity_quads, Ordering::Relaxed);
    METRICS_BRIDGE
        .dirty_ranges
        .store(dirty_ranges, Ordering::Relaxed);
    METRICS_BRIDGE
        .dirty_range_quads
        .store(dirty_range_quads, Ordering::Relaxed);
    METRICS_BRIDGE.quads.store(
        extracted_batches
            .batches
            .iter()
            .map(|b| b.quads.len())
            .sum(),
        Ordering::Relaxed,
    );
    METRICS_BRIDGE
        .uploaded_quads
        .store(uploaded_quads, Ordering::Relaxed);
    METRICS_BRIDGE
        .uploaded_bytes
        .store(uploaded_bytes, Ordering::Relaxed);
    METRICS_BRIDGE
        .uploaded_this_frame
        .store(uploaded_batches, Ordering::Relaxed);

    METRICS_BRIDGE
        .arena_capacity_quads
        .store(arena.capacity_quads, Ordering::Relaxed);
    METRICS_BRIDGE
        .arena_used_quads
        .store(total_required_quads, Ordering::Relaxed);
    METRICS_BRIDGE
        .arena_slot_quads
        .store(next_free_quads, Ordering::Relaxed);
    METRICS_BRIDGE
        .arena_uploaded_bytes
        .store(uploaded_bytes, Ordering::Relaxed);
    METRICS_BRIDGE
        .arena_reallocations
        .store(arena.stats.reallocations, Ordering::Relaxed);
    METRICS_BRIDGE
        .arena_compactions
        .store(arena.stats.compactions, Ordering::Relaxed);

    // --- Indirect draw metrics ---
    let reported_indirect_commands = if !material_mode && use_indirect {
        command_count
    } else {
        0
    };
    METRICS_BRIDGE
        .indirect_draw_commands
        .store(reported_indirect_commands, Ordering::Relaxed);
    METRICS_BRIDGE.draw_mode.store(mode_code, Ordering::Relaxed);
    if !material_mode {
        METRICS_BRIDGE.material_entities.store(0, Ordering::Relaxed);
        METRICS_BRIDGE.material_sync_us.store(0, Ordering::Relaxed);
    }
    METRICS_BRIDGE
        .prepare_system_us
        .store(elapsed_us(system_started_at), Ordering::Relaxed);
}

/// Records the actual direct-renderer work after per-view visibility culling.
pub fn record_packed_quad_visible_draws(visible_batches: usize, visible_quads: usize) {
    METRICS_BRIDGE
        .visible_batches
        .store(visible_batches, Ordering::Relaxed);
    METRICS_BRIDGE
        .visible_quads
        .store(visible_quads, Ordering::Relaxed);
}

/// Records CPU-side work done while encoding the packed render graph node.
pub fn record_packed_quad_render_node(
    render_node_us: u64,
    draw_calls: usize,
    items_considered: usize,
) {
    METRICS_BRIDGE
        .render_node_us
        .store(render_node_us, Ordering::Relaxed);
    METRICS_BRIDGE
        .render_draw_calls
        .store(draw_calls, Ordering::Relaxed);
    METRICS_BRIDGE
        .render_items_considered
        .store(items_considered, Ordering::Relaxed);
}

/// Records the experimental Bevy material path estimates for HUD and profiling.
pub fn record_packed_material_draw_estimate(
    material_entities: usize,
    visible_quads_estimate: usize,
    material_sync_us: u64,
) {
    METRICS_BRIDGE
        .visible_batches
        .store(material_entities, Ordering::Relaxed);
    METRICS_BRIDGE
        .visible_quads
        .store(visible_quads_estimate, Ordering::Relaxed);
    METRICS_BRIDGE
        .indirect_draw_commands
        .store(0, Ordering::Relaxed);
    METRICS_BRIDGE
        .draw_mode
        .store(PACKED_DRAW_MODE_MATERIAL, Ordering::Relaxed);
    METRICS_BRIDGE
        .material_entities
        .store(material_entities, Ordering::Relaxed);
    METRICS_BRIDGE
        .material_sync_us
        .store(material_sync_us, Ordering::Relaxed);
    METRICS_BRIDGE.render_node_us.store(0, Ordering::Relaxed);
    METRICS_BRIDGE
        .render_draw_calls
        .store(material_entities, Ordering::Relaxed);
    METRICS_BRIDGE
        .render_items_considered
        .store(material_entities, Ordering::Relaxed);
    METRICS_BRIDGE
        .render_gpu_pass_us
        .store(0, Ordering::Relaxed);
    METRICS_BRIDGE.gpu_cull_enabled.store(0, Ordering::Relaxed);
    METRICS_BRIDGE
        .gpu_cull_input_commands
        .store(0, Ordering::Relaxed);
    METRICS_BRIDGE
        .gpu_cull_est_visible_commands
        .store(0, Ordering::Relaxed);
    METRICS_BRIDGE
        .gpu_cull_est_visible_quads
        .store(0, Ordering::Relaxed);
    METRICS_BRIDGE.gpu_cull_node_us.store(0, Ordering::Relaxed);
    METRICS_BRIDGE
        .gpu_cull_compact_enabled
        .store(0, Ordering::Relaxed);
    METRICS_BRIDGE
        .cpu_visible_compact_enabled
        .store(0, Ordering::Relaxed);
    METRICS_BRIDGE
        .cpu_visible_commands
        .store(material_entities, Ordering::Relaxed);
}

pub fn record_packed_gpu_generation_region_mask(loaded_regions: usize, active_regions: usize) {
    METRICS_BRIDGE
        .generated_regions_loaded
        .store(loaded_regions, Ordering::Relaxed);
    METRICS_BRIDGE
        .generated_regions_active
        .store(active_regions, Ordering::Relaxed);
    METRICS_BRIDGE
        .generated_regions_visible
        .store(0, Ordering::Relaxed);
}

pub fn record_packed_gpu_generation_visible_draws(visible_regions: usize, visible_quads: usize) {
    METRICS_BRIDGE
        .generated_regions_visible
        .store(visible_regions, Ordering::Relaxed);
    METRICS_BRIDGE
        .visible_batches
        .store(visible_regions, Ordering::Relaxed);
    METRICS_BRIDGE
        .visible_quads
        .store(visible_quads, Ordering::Relaxed);
    METRICS_BRIDGE
        .cpu_visible_commands
        .store(visible_regions, Ordering::Relaxed);
}

pub fn record_packed_gpu_generation_update(update_us: u64, skipped: bool) {
    METRICS_BRIDGE
        .generated_update_us
        .store(update_us, Ordering::Relaxed);
    METRICS_BRIDGE
        .generated_update_skipped
        .store(usize::from(skipped), Ordering::Relaxed);
}

pub fn record_packed_gpu_generation_cache_lifecycle(
    hits: usize,
    misses: usize,
    invalidated: usize,
    evicted: usize,
) {
    METRICS_BRIDGE
        .generated_cache_hits
        .store(hits, Ordering::Relaxed);
    METRICS_BRIDGE
        .generated_cache_misses
        .store(misses, Ordering::Relaxed);
    METRICS_BRIDGE
        .generated_cache_invalidated
        .store(invalidated, Ordering::Relaxed);
    METRICS_BRIDGE
        .generated_cache_evicted
        .store(evicted, Ordering::Relaxed);
}

pub fn record_packed_gpu_generation_prepare_reuse(skipped: bool) {
    METRICS_BRIDGE
        .generated_prepare_skipped
        .store(usize::from(skipped), Ordering::Relaxed);
}

pub fn record_packed_gpu_generation_cull_uploads(metadata_uploaded: bool, config_uploaded: bool) {
    METRICS_BRIDGE
        .generated_cull_metadata_uploaded
        .store(usize::from(metadata_uploaded), Ordering::Relaxed);
    METRICS_BRIDGE
        .generated_cull_config_uploaded
        .store(usize::from(config_uploaded), Ordering::Relaxed);
}

pub fn record_packed_gpu_generation_cull_dispatch_reuse(skipped: bool) {
    METRICS_BRIDGE
        .generated_cull_dispatch_skipped
        .store(usize::from(skipped), Ordering::Relaxed);
}

pub fn record_packed_gpu_generation_prepare(
    capacity_quads: usize,
    slot_quads: usize,
    max_output_quads: usize,
    column_count: usize,
    batch_count: usize,
    source_chunk_count: usize,
) {
    METRICS_BRIDGE.batches.store(batch_count, Ordering::Relaxed);
    METRICS_BRIDGE
        .chunks_loaded
        .store(source_chunk_count, Ordering::Relaxed);
    METRICS_BRIDGE
        .chunks_active
        .store(source_chunk_count, Ordering::Relaxed);
    METRICS_BRIDGE.chunk_ranges.store(0, Ordering::Relaxed);
    METRICS_BRIDGE
        .resident_chunk_ranges
        .store(0, Ordering::Relaxed);
    METRICS_BRIDGE
        .tombstone_chunk_ranges
        .store(0, Ordering::Relaxed);
    METRICS_BRIDGE
        .resident_range_capacity_quads
        .store(0, Ordering::Relaxed);
    METRICS_BRIDGE
        .tombstone_capacity_quads
        .store(0, Ordering::Relaxed);
    METRICS_BRIDGE.dirty_ranges.store(0, Ordering::Relaxed);
    METRICS_BRIDGE.dirty_range_quads.store(0, Ordering::Relaxed);
    METRICS_BRIDGE
        .quads
        .store(max_output_quads, Ordering::Relaxed);
    METRICS_BRIDGE.uploaded_quads.store(0, Ordering::Relaxed);
    METRICS_BRIDGE.uploaded_bytes.store(0, Ordering::Relaxed);
    METRICS_BRIDGE
        .uploaded_this_frame
        .store(0, Ordering::Relaxed);
    METRICS_BRIDGE
        .arena_capacity_quads
        .store(capacity_quads, Ordering::Relaxed);
    METRICS_BRIDGE
        .arena_used_quads
        .store(max_output_quads, Ordering::Relaxed);
    METRICS_BRIDGE
        .arena_slot_quads
        .store(slot_quads, Ordering::Relaxed);
    METRICS_BRIDGE
        .arena_uploaded_bytes
        .store(0, Ordering::Relaxed);
    METRICS_BRIDGE.indirect_draw_commands.store(
        usize::from(column_count > 0) * batch_count,
        Ordering::Relaxed,
    );
    METRICS_BRIDGE
        .draw_mode
        .store(PACKED_DRAW_MODE_GPU_GENERATED, Ordering::Relaxed);
    METRICS_BRIDGE.material_entities.store(0, Ordering::Relaxed);
    METRICS_BRIDGE.material_sync_us.store(0, Ordering::Relaxed);
    METRICS_BRIDGE.gpu_cull_enabled.store(0, Ordering::Relaxed);
    METRICS_BRIDGE
        .gpu_cull_input_commands
        .store(0, Ordering::Relaxed);
    METRICS_BRIDGE
        .gpu_cull_est_visible_commands
        .store(0, Ordering::Relaxed);
    METRICS_BRIDGE
        .gpu_cull_est_visible_quads
        .store(0, Ordering::Relaxed);
    METRICS_BRIDGE.gpu_cull_node_us.store(0, Ordering::Relaxed);
    METRICS_BRIDGE
        .gpu_cull_compact_enabled
        .store(0, Ordering::Relaxed);
    METRICS_BRIDGE
        .cpu_visible_compact_enabled
        .store(0, Ordering::Relaxed);
    METRICS_BRIDGE
        .cpu_visible_commands
        .store(0, Ordering::Relaxed);
}

/// Records CPU-side work done while preparing packed per-view resources.
pub fn record_packed_quad_view_prepare(view_prepare_system_us: u64) {
    METRICS_BRIDGE
        .view_prepare_system_us
        .store(view_prepare_system_us, Ordering::Relaxed);
}

/// Records GPU timestamp profiler availability for the packed render pass.
pub fn record_packed_quad_gpu_timestamp_status(requested: bool, supported: bool) {
    METRICS_BRIDGE
        .gpu_timestamps_requested
        .store(usize::from(requested), Ordering::Relaxed);
    METRICS_BRIDGE
        .gpu_timestamps_supported
        .store(usize::from(supported), Ordering::Relaxed);
    if !requested || !supported {
        METRICS_BRIDGE
            .render_gpu_pass_us
            .store(0, Ordering::Relaxed);
    }
}

/// Records the latest measured packed render pass GPU time.
pub fn record_packed_quad_gpu_pass_time(render_gpu_pass_us: u64) {
    METRICS_BRIDGE
        .render_gpu_pass_us
        .store(render_gpu_pass_us, Ordering::Relaxed);
}

/// Records prepared GPU cull resources for packed indirect rendering.
pub fn record_packed_quad_gpu_cull_prepare(
    enabled: bool,
    input_commands: usize,
    count_supported: bool,
    compact_enabled: bool,
) {
    METRICS_BRIDGE
        .gpu_cull_enabled
        .store(usize::from(enabled), Ordering::Relaxed);
    METRICS_BRIDGE
        .gpu_cull_input_commands
        .store(input_commands, Ordering::Relaxed);
    METRICS_BRIDGE
        .gpu_cull_count_supported
        .store(usize::from(count_supported), Ordering::Relaxed);
    METRICS_BRIDGE
        .gpu_cull_compact_enabled
        .store(usize::from(compact_enabled), Ordering::Relaxed);
    METRICS_BRIDGE
        .gpu_cull_est_visible_commands
        .store(0, Ordering::Relaxed);
    METRICS_BRIDGE
        .gpu_cull_est_visible_quads
        .store(0, Ordering::Relaxed);
    METRICS_BRIDGE.gpu_cull_node_us.store(0, Ordering::Relaxed);
}

/// Records CPU-side estimates and encode time for the packed GPU cull node.
pub fn record_packed_quad_gpu_cull_node(
    node_us: u64,
    est_visible_commands: usize,
    est_visible_quads: usize,
) {
    METRICS_BRIDGE
        .gpu_cull_node_us
        .store(node_us, Ordering::Relaxed);
    METRICS_BRIDGE
        .gpu_cull_est_visible_commands
        .store(est_visible_commands, Ordering::Relaxed);
    METRICS_BRIDGE
        .gpu_cull_est_visible_quads
        .store(est_visible_quads, Ordering::Relaxed);
}

/// Records the per-view CPU compact fallback for indirect submission.
pub fn record_packed_quad_cpu_visible_indirect(compact_enabled: bool, visible_commands: usize) {
    METRICS_BRIDGE
        .cpu_visible_compact_enabled
        .store(usize::from(compact_enabled), Ordering::Relaxed);
    METRICS_BRIDGE
        .cpu_visible_commands
        .store(visible_commands, Ordering::Relaxed);
}

fn record_confirmed_packed_batch_generations(
    allocations: &HashMap<u64, crate::packed_quad_buffer::PackedQuadArenaAllocation>,
) {
    if let Ok(mut confirmed_generations) = CONFIRMED_PACKED_BATCH_GENERATIONS.lock() {
        confirmed_generations.clear();
        confirmed_generations.extend(
            allocations
                .iter()
                .map(|(&key, allocation)| (key, allocation.generation)),
        );
    }
}

fn prune_dirty_ranges_confirmed_through(
    batch: &mut PackedQuadBatch,
    confirmed_generation: u64,
) -> usize {
    let dirty_ranges = Arc::make_mut(&mut batch.dirty_ranges);
    let len_before = dirty_ranges.len();
    dirty_ranges.retain(|range| range.generation > confirmed_generation);
    len_before - dirty_ranges.len()
}

/// Main World system that removes dirty subrange metadata after render-world upload planning
/// has confirmed the corresponding batch generation.
pub fn prune_confirmed_packed_dirty_ranges(mut batches: ResMut<PackedQuadBatches>) {
    let confirmed_generations = CONFIRMED_PACKED_BATCH_GENERATIONS
        .lock()
        .map_or_else(|_| HashMap::new(), |generations| generations.clone());
    if confirmed_generations.is_empty() {
        return;
    }

    for batch in &mut batches.batches {
        if let Some(&confirmed_generation) = confirmed_generations.get(&batch.key) {
            prune_dirty_ranges_confirmed_through(batch, confirmed_generation);
        }
    }
}

pub fn snapshot_packed_quad_metrics() -> PackedQuadPipelineStats {
    let mut stats = PackedQuadPipelineStats::default();
    write_packed_quad_metrics(&mut stats);
    stats
}

/// Main World system that synchronizes render-world metrics back to the CPU stats resource.
pub fn sync_packed_quad_metrics(mut stats: ResMut<PackedQuadPipelineStats>) {
    write_packed_quad_metrics(&mut stats);
}

fn write_packed_quad_metrics(stats: &mut PackedQuadPipelineStats) {
    stats.batches = METRICS_BRIDGE.batches.load(Ordering::Relaxed);
    stats.chunks_loaded = METRICS_BRIDGE.chunks_loaded.load(Ordering::Relaxed);
    stats.chunks_active = METRICS_BRIDGE.chunks_active.load(Ordering::Relaxed);
    stats.chunk_ranges = METRICS_BRIDGE.chunk_ranges.load(Ordering::Relaxed);
    stats.resident_chunk_ranges = METRICS_BRIDGE.resident_chunk_ranges.load(Ordering::Relaxed);
    stats.tombstone_chunk_ranges = METRICS_BRIDGE
        .tombstone_chunk_ranges
        .load(Ordering::Relaxed);
    stats.resident_range_capacity_quads = METRICS_BRIDGE
        .resident_range_capacity_quads
        .load(Ordering::Relaxed);
    stats.tombstone_capacity_quads = METRICS_BRIDGE
        .tombstone_capacity_quads
        .load(Ordering::Relaxed);
    stats.dirty_ranges = METRICS_BRIDGE.dirty_ranges.load(Ordering::Relaxed);
    stats.dirty_range_quads = METRICS_BRIDGE.dirty_range_quads.load(Ordering::Relaxed);
    stats.quads = METRICS_BRIDGE.quads.load(Ordering::Relaxed);
    stats.uploaded_quads = METRICS_BRIDGE.uploaded_quads.load(Ordering::Relaxed);
    stats.dropped_quads = METRICS_BRIDGE.dropped_quads.load(Ordering::Relaxed);
    stats.uploaded_bytes = METRICS_BRIDGE.uploaded_bytes.load(Ordering::Relaxed);
    stats.buffer_capacity_quads = METRICS_BRIDGE.buffer_capacity_quads.load(Ordering::Relaxed);
    stats.pending_builds = METRICS_BRIDGE.pending_builds.load(Ordering::Relaxed);
    stats.pending_region_rebuilds = METRICS_BRIDGE
        .pending_region_rebuilds
        .load(Ordering::Relaxed);
    stats.prepare_system_us = METRICS_BRIDGE.prepare_system_us.load(Ordering::Relaxed);
    stats.view_prepare_system_us = METRICS_BRIDGE
        .view_prepare_system_us
        .load(Ordering::Relaxed);
    stats.stream_system_us = METRICS_BRIDGE.stream_system_us.load(Ordering::Relaxed);
    stats.stream_spawned_builds = METRICS_BRIDGE.stream_spawned_builds.load(Ordering::Relaxed);
    stats.stream_rebuild_regions = METRICS_BRIDGE
        .stream_rebuild_regions
        .load(Ordering::Relaxed);
    stats.build_task_system_us = METRICS_BRIDGE.build_task_system_us.load(Ordering::Relaxed);
    stats.built_this_frame = METRICS_BRIDGE.built_this_frame.load(Ordering::Relaxed);
    stats.compaction_system_us = METRICS_BRIDGE.compaction_system_us.load(Ordering::Relaxed);
    stats.compacted_regions_this_frame = METRICS_BRIDGE
        .compacted_regions_this_frame
        .load(Ordering::Relaxed);
    stats.uploaded_this_frame = METRICS_BRIDGE.uploaded_this_frame.load(Ordering::Relaxed);
    stats.visible_batches = METRICS_BRIDGE.visible_batches.load(Ordering::Relaxed);
    stats.visible_quads = METRICS_BRIDGE.visible_quads.load(Ordering::Relaxed);
    stats.cpu_reserved_quads = METRICS_BRIDGE.cpu_reserved_quads.load(Ordering::Relaxed);
    stats.cpu_reserved_bytes = METRICS_BRIDGE.cpu_reserved_bytes.load(Ordering::Relaxed);
    stats.min_vram_bytes = packed_min_vram_bytes_from_env() as u64;
    stats.min_ram_bytes = packed_min_ram_bytes_from_env() as u64;
    stats.gpu_reserved_bytes = METRICS_BRIDGE.gpu_reserved_bytes.load(Ordering::Relaxed);

    stats.arena_capacity_quads = METRICS_BRIDGE.arena_capacity_quads.load(Ordering::Relaxed);
    stats.arena_used_quads = METRICS_BRIDGE.arena_used_quads.load(Ordering::Relaxed);
    stats.arena_slot_quads = METRICS_BRIDGE.arena_slot_quads.load(Ordering::Relaxed);
    stats.arena_uploaded_bytes = METRICS_BRIDGE.arena_uploaded_bytes.load(Ordering::Relaxed);
    stats.arena_reallocations = METRICS_BRIDGE.arena_reallocations.load(Ordering::Relaxed);
    stats.arena_compactions = METRICS_BRIDGE.arena_compactions.load(Ordering::Relaxed);

    stats.indirect_draw_commands = METRICS_BRIDGE
        .indirect_draw_commands
        .load(Ordering::Relaxed);
    stats.draw_mode = METRICS_BRIDGE.draw_mode.load(Ordering::Relaxed);
    stats.material_entities = METRICS_BRIDGE.material_entities.load(Ordering::Relaxed);
    stats.material_sync_us = METRICS_BRIDGE.material_sync_us.load(Ordering::Relaxed);
    stats.render_node_us = METRICS_BRIDGE.render_node_us.load(Ordering::Relaxed);
    stats.render_draw_calls = METRICS_BRIDGE.render_draw_calls.load(Ordering::Relaxed);
    stats.render_items_considered = METRICS_BRIDGE
        .render_items_considered
        .load(Ordering::Relaxed);
    stats.render_gpu_pass_us = METRICS_BRIDGE.render_gpu_pass_us.load(Ordering::Relaxed);
    stats.gpu_timestamps_requested = METRICS_BRIDGE
        .gpu_timestamps_requested
        .load(Ordering::Relaxed)
        != 0;
    stats.gpu_timestamps_supported = METRICS_BRIDGE
        .gpu_timestamps_supported
        .load(Ordering::Relaxed)
        != 0;
    stats.gpu_cull_enabled = METRICS_BRIDGE.gpu_cull_enabled.load(Ordering::Relaxed) != 0;
    stats.gpu_cull_input_commands = METRICS_BRIDGE
        .gpu_cull_input_commands
        .load(Ordering::Relaxed);
    stats.gpu_cull_est_visible_commands = METRICS_BRIDGE
        .gpu_cull_est_visible_commands
        .load(Ordering::Relaxed);
    stats.gpu_cull_est_visible_quads = METRICS_BRIDGE
        .gpu_cull_est_visible_quads
        .load(Ordering::Relaxed);
    stats.gpu_cull_node_us = METRICS_BRIDGE.gpu_cull_node_us.load(Ordering::Relaxed);
    stats.gpu_cull_count_supported = METRICS_BRIDGE
        .gpu_cull_count_supported
        .load(Ordering::Relaxed)
        != 0;
    stats.gpu_cull_compact_enabled = METRICS_BRIDGE
        .gpu_cull_compact_enabled
        .load(Ordering::Relaxed)
        != 0;
    stats.cpu_visible_compact_enabled = METRICS_BRIDGE
        .cpu_visible_compact_enabled
        .load(Ordering::Relaxed)
        != 0;
    stats.cpu_visible_commands = METRICS_BRIDGE.cpu_visible_commands.load(Ordering::Relaxed);
    stats.generated_regions_loaded = METRICS_BRIDGE
        .generated_regions_loaded
        .load(Ordering::Relaxed);
    stats.generated_regions_active = METRICS_BRIDGE
        .generated_regions_active
        .load(Ordering::Relaxed);
    stats.generated_regions_visible = METRICS_BRIDGE
        .generated_regions_visible
        .load(Ordering::Relaxed);
    stats.generated_update_us = METRICS_BRIDGE.generated_update_us.load(Ordering::Relaxed);
    stats.generated_update_skipped = METRICS_BRIDGE
        .generated_update_skipped
        .load(Ordering::Relaxed)
        != 0;
    stats.generated_cache_hits = METRICS_BRIDGE.generated_cache_hits.load(Ordering::Relaxed);
    stats.generated_cache_misses = METRICS_BRIDGE
        .generated_cache_misses
        .load(Ordering::Relaxed);
    stats.generated_cache_invalidated = METRICS_BRIDGE
        .generated_cache_invalidated
        .load(Ordering::Relaxed);
    stats.generated_cache_evicted = METRICS_BRIDGE
        .generated_cache_evicted
        .load(Ordering::Relaxed);
    stats.generated_prepare_skipped = METRICS_BRIDGE
        .generated_prepare_skipped
        .load(Ordering::Relaxed)
        != 0;
    stats.generated_cull_metadata_uploaded = METRICS_BRIDGE
        .generated_cull_metadata_uploaded
        .load(Ordering::Relaxed)
        != 0;
    stats.generated_cull_config_uploaded = METRICS_BRIDGE
        .generated_cull_config_uploaded
        .load(Ordering::Relaxed)
        != 0;
    stats.generated_cull_dispatch_skipped = METRICS_BRIDGE
        .generated_cull_dispatch_skipped
        .load(Ordering::Relaxed)
        != 0;

    if stats.draw_mode == PACKED_DRAW_MODE_MATERIAL {
        stats.batches = stats.material_entities;
        stats.visible_batches = stats.material_entities;
        stats.quads = stats.visible_quads;
    }
}

/// Sync data-driven block texture mappings into the packed renderer palette.
pub fn sync_packed_quad_block_texture_palette(
    registry: Res<BlockRegistry>,
    mut palette: ResMut<PackedQuadBlockTexturePalette>,
) {
    let mut tiles = vec![[3, 3, 3, 0]; PACKED_BLOCK_PALETTE_LEN];
    tiles[AIR_BLOCK_ID as usize] = [0, 0, 0, 0];

    if let Ok(mappings) = registry.texture_mappings.read() {
        for (&block_id, &mapping) in mappings.iter() {
            let index = usize::from(block_id);
            if index < tiles.len() {
                tiles[index] = [mapping[0], mapping[1], mapping[2], 0];
            }
        }
    }

    if palette.tiles != tiles {
        palette.tiles = tiles;
    }
}

/// Deterministic one-shot debug producer system.
///
/// Builds a chunk using game world context and block registry, and publishes
/// it exactly once as a packed quad batch in the Main World.
pub fn setup_packed_quad_debug_producer(
    registry: Res<BlockRegistry>,
    mut batches: ResMut<PackedQuadBatches>,
    mut spawned: Local<bool>,
) {
    if *spawned {
        return;
    }

    let context = WorldGenerationContext::from_registry(&registry);
    let chunk_pos = ChunkPos::new(0, 0);
    let sand_block = registry.get_id("sand").unwrap_or(context.palette.dirt);
    let cell_size = packed_min_cell_size_from_env();

    // Build the packed quads
    let mut quads = crate::voxel_packed_quads::build_surface_packed_quads_for_chunk(
        chunk_pos, &context, cell_size, sand_block,
    );
    filter_debug_packed_quads(&mut quads);
    let estimated_bytes = estimate_quads_bytes(quads.len());

    info!(
        "PACKED QUAD DEBUG PRODUCER: Generated debug batch! \
         ChunkPos: {:?}, Packed quads: {}, Estimated bytes: {}",
        chunk_pos,
        quads.len(),
        estimated_bytes
    );

    // Publish batch with unique id 1 and generation 1
    batches.batches.push(PackedQuadBatch {
        key: pack_chunk_key(chunk_pos.x, chunk_pos.z),
        chunk_ranges: Arc::new(vec![PackedQuadChunkRange {
            chunk_key: pack_chunk_key(chunk_pos.x, chunk_pos.z),
            start_quads: 0,
            len_quads: quads.len(),
            capacity_quads: quads.len(),
            active: true,
            resident: true,
        }]),
        dirty_ranges: Arc::new(Vec::new()),
        quads: Arc::new(quads),
        generation: 1,
        needs_compaction: false,
    });

    *spawned = true;
}

fn build_packed_gpu_generation_cache_contract(
    registry: &BlockRegistry,
    context: &WorldGenerationContext,
    region_size: i32,
    cell_size: usize,
    surface_top_material: BlockId,
) -> PackedGpuGenerationCacheContract {
    let terrain_palette = [
        context.palette.air,
        context.palette.dirt,
        context.palette.grass,
        context.palette.stone,
    ];
    let material_contract_version = registry.material_contract_version_for_blocks(&[
        terrain_palette[0],
        terrain_palette[1],
        terrain_palette[2],
        terrain_palette[3],
        surface_top_material,
    ]);

    PackedGpuGenerationCacheContract::new(
        region_size,
        cell_size,
        terrain_palette,
        surface_top_material,
        terrain_surface_contract_version(),
        material_contract_version,
    )
}

#[allow(clippy::too_many_arguments)]
fn reuse_generated_region_cache_entry(
    region_cache: &mut crate::packed_quad_gpu_generation::GeneratedRegionCache,
    region_key: u64,
    region_origin_x: i32,
    region_origin_z: i32,
    region_size: i32,
    contract: PackedGpuGenerationCacheContract,
    edit_store: &WorldEditStore,
    cache_frame: u64,
) -> Option<PackedGpuGenerationBatch> {
    let existing = region_cache.entries.get_mut(&region_key)?;
    if existing.contract != contract {
        return None;
    }
    if edit_store.region_has_edits_since(
        region_origin_x,
        region_origin_z,
        region_size,
        existing.edit_store_generation,
    ) {
        return None;
    }

    existing.last_seen_frame = cache_frame;
    Some(existing.to_batch())
}

pub fn update_packed_gpu_generation_regions(
    registry: Res<BlockRegistry>,
    edit_store: Res<WorldEditStore>,
    mut cpu_batches: ResMut<PackedQuadBatches>,
    mut gpu_batches: ResMut<PackedGpuGenerationBatches>,
    mut region_cache: ResMut<crate::packed_quad_gpu_generation::GeneratedRegionCache>,
    mut region_scratch: ResMut<PackedGpuGenerationRegionScratch>,
    camera_query: Query<&GlobalTransform, With<Camera3d>>,
) {
    let update_started = Instant::now();
    let context = WorldGenerationContext::from_registry(&registry);
    let region_size = packed_region_size_from_env();
    let region_radius = packed_gpu_generation_region_radius_from_env();

    let Some(camera_translation) = camera_query.iter().next().map(GlobalTransform::translation)
    else {
        return;
    };
    let chunk_size = rumpel_world::chunk::CHUNK_SIZE as f32;
    let camera_chunk_x = (camera_translation.x / chunk_size).floor() as i32;
    let camera_chunk_z = (camera_translation.z / chunk_size).floor() as i32;
    let (center_origin_x, center_origin_z) =
        packed_region_origin_for_chunk(camera_chunk_x, camera_chunk_z, region_size);

    let cell_size = packed_min_cell_size_from_env();
    let sand_block = registry.get_id("sand").unwrap_or(context.palette.dirt);
    let contract = build_packed_gpu_generation_cache_contract(
        &registry,
        &context,
        region_size,
        cell_size,
        sand_block,
    );
    let contract_generation = contract.generation();
    let cache_frame = region_cache.next_frame();
    let view_center = IVec2::new(camera_chunk_x, camera_chunk_z);
    let view_radius = packed_view_radius_from_env();
    let generated_region_side = region_radius.saturating_mul(2).saturating_add(1).max(1) as usize;
    let loaded_region_capacity = generated_region_side.saturating_mul(generated_region_side);
    let scratch = &mut *region_scratch;
    scratch.loaded_region_keys.clear();
    if scratch.loaded_region_keys.capacity() < loaded_region_capacity {
        scratch
            .loaded_region_keys
            .reserve(loaded_region_capacity - scratch.loaded_region_keys.capacity());
    }
    scratch.active_regions.clear();
    if scratch.active_regions.capacity() < loaded_region_capacity {
        scratch
            .active_regions
            .reserve(loaded_region_capacity - scratch.active_regions.capacity());
    }

    for region_z in -region_radius..=region_radius {
        for region_x in -region_radius..=region_radius {
            let region_origin_x = center_origin_x + region_x * region_size;
            let region_origin_z = center_origin_z + region_z * region_size;
            let region_key = pack_chunk_key(region_origin_x, region_origin_z);
            scratch.loaded_region_keys.push(region_key);

            if crate::packed_quad_gpu_generation::region_has_active_chunks(
                region_origin_x,
                region_origin_z,
                region_size,
                view_center,
                view_radius,
            ) {
                scratch
                    .active_regions
                    .push((region_origin_x, region_origin_z, region_key));
            }
        }
    }
    let loaded_regions = scratch.loaded_region_keys.len();
    let (active_region_count, active_region_hash) =
        PackedGpuGenerationTarget::active_region_signature(
            scratch
                .active_regions
                .iter()
                .map(|(_, _, region_key)| *region_key),
        );
    let target = PackedGpuGenerationTarget::new(
        camera_chunk_x,
        camera_chunk_z,
        center_origin_x,
        center_origin_z,
        region_size,
        region_radius,
        view_radius,
        contract_generation,
        edit_store.generation(),
        active_region_count,
        active_region_hash,
    );

    if gpu_batches
        .target
        .is_some_and(|previous| previous.matches_active_region_window(target))
        && !gpu_batches.batches.is_empty()
    {
        record_packed_gpu_generation_region_mask(loaded_regions, active_region_count);
        record_packed_gpu_generation_cache_lifecycle(0, 0, 0, 0);
        record_packed_gpu_generation_update(elapsed_us(update_started), true);
        gpu_batches.target = Some(target);
        return;
    }

    scratch.generated_batches.clear();
    let mut cache_hits = 0usize;
    let mut cache_misses = 0usize;
    let mut cache_invalidated = 0usize;
    scratch.target_keys.clear();
    if scratch.target_keys.capacity() < loaded_regions {
        scratch
            .target_keys
            .reserve(loaded_regions - scratch.target_keys.capacity());
    }
    for region_key in &scratch.loaded_region_keys {
        scratch.target_keys.insert(*region_key);
    }

    for (region_origin_x, region_origin_z, region_key) in scratch.active_regions.iter().copied() {
        if let Some(batch) = reuse_generated_region_cache_entry(
            &mut region_cache,
            region_key,
            region_origin_x,
            region_origin_z,
            region_size,
            contract,
            &edit_store,
            cache_frame,
        ) {
            cache_hits = cache_hits.saturating_add(1);
            scratch.generated_batches.push(batch);
            continue;
        }

        if let Some(stale) = region_cache.entries.remove(&region_key) {
            cache_invalidated = cache_invalidated.saturating_add(1);
            let invalidated_by_edits = edit_store.region_has_edits_since(
                region_origin_x,
                region_origin_z,
                region_size,
                stale.edit_store_generation,
            );
            info!(
                region_key,
                old_generation = stale.generation,
                new_generation = contract_generation,
                invalidated_by_edits,
                old_edit_store_generation = stale.edit_store_generation,
                edit_store_generation = edit_store.generation(),
                "PACKED GPU GENERATION: invalidated stale generated region cache entry"
            );
        } else {
            cache_misses = cache_misses.saturating_add(1);
        }

        let source_chunk_count = {
            let side = region_size.max(0) as usize;
            side.saturating_mul(side)
        };
        if source_chunk_count == 0 {
            continue;
        }

        let expected_column_count =
            source_chunk_count.saturating_mul(packed_gpu_generation_columns_per_chunk(cell_size));
        let mut columns = Vec::with_capacity(expected_column_count);

        for chunk_z in region_origin_z..region_origin_z + region_size {
            for chunk_x in region_origin_x..region_origin_x + region_size {
                let appended_start = columns.len();
                crate::voxel_packed_quads::append_surface_gpu_generation_columns_for_chunk(
                    &mut columns,
                    ChunkPos::new(chunk_x, chunk_z),
                    &context,
                    cell_size,
                    sand_block,
                    &edit_store,
                );
                let offset_x =
                    (chunk_x - region_origin_x) as u32 * rumpel_world::chunk::CHUNK_SIZE as u32;
                let offset_z =
                    (chunk_z - region_origin_z) as u32 * rumpel_world::chunk::CHUNK_SIZE as u32;
                for column in &mut columns[appended_start..] {
                    column.local[0] = column.local[0].saturating_add(offset_x);
                    column.local[1] = column.local[1].saturating_add(offset_z);
                }
            }
        }

        let max_output_quads = columns
            .len()
            .saturating_mul(PACKED_GPU_GENERATION_MAX_QUADS_PER_COLUMN);
        let params = PackedGpuGenerationParams::new(
            columns.len(),
            max_output_quads,
            packed_gpu_generation_lod_for_cell_size(cell_size),
            context.palette.air,
            context.palette.dirt,
            context.palette.grass,
            context.palette.stone,
        );
        let (bounds_min, bounds_max) = packed_region_world_bounds(region_key, region_size);

        let entry = crate::packed_quad_gpu_generation::GeneratedRegionCacheEntry {
            key: region_key,
            columns: Arc::new(columns),
            params,
            source_chunk_count,
            max_output_quads,
            translation: Vec4::new(
                (region_origin_x * rumpel_world::chunk::CHUNK_SIZE as i32) as f32,
                0.0,
                (region_origin_z * rumpel_world::chunk::CHUNK_SIZE as i32) as f32,
                0.0,
            ),
            bounds_min,
            bounds_max,
            generation: contract_generation,
            contract,
            edit_store_generation: edit_store.generation(),
            last_seen_frame: cache_frame,
        };

        let batch = entry.to_batch();
        region_cache.entries.insert(region_key, entry);
        scratch.generated_batches.push(batch);
    }
    scratch.generated_batches.sort_by_key(|batch| batch.key);
    let generated_batch_count = scratch.generated_batches.len();

    let cache_entries_before_retain = region_cache.entries.len();
    region_cache
        .entries
        .retain(|k, _| scratch.target_keys.contains(k));
    let cache_evicted = cache_entries_before_retain.saturating_sub(region_cache.entries.len());

    cpu_batches.batches.clear();
    record_packed_gpu_generation_region_mask(loaded_regions, generated_batch_count);
    record_packed_gpu_generation_cache_lifecycle(
        cache_hits,
        cache_misses,
        cache_invalidated,
        cache_evicted,
    );

    let batch_signature =
        PackedGpuGenerationBatches::calculate_batch_signature(&scratch.generated_batches);
    let batch_summary = PackedGpuGenerationBatches::summarize_batches(&scratch.generated_batches);
    let changed = gpu_batches.batch_signature != batch_signature;

    gpu_batches.target = Some(target);

    if changed {
        gpu_batches.batch_signature = batch_signature;
        gpu_batches.summary = batch_summary;
        std::mem::swap(&mut gpu_batches.batches, &mut scratch.generated_batches);

        info!(
            region_size,
            region_radius,
            generation = contract_generation,
            loaded_regions,
            active_regions = gpu_batches.batches.len(),
            batches = gpu_batches.batches.len(),
            columns = gpu_batches.summary.total_column_count,
            max_output_quads = gpu_batches.summary.total_max_output_quads,
            "PACKED GPU GENERATION: updated compact column source batches for new camera target"
        );
    }
    scratch.generated_batches.clear();

    record_packed_gpu_generation_update(elapsed_us(update_started), false);
}

/// Region-grid half-width in region tiles needed to cover a circular chunk view radius.
#[must_use]
pub fn packed_gpu_generation_region_radius_for_view(view_radius: i32, region_size: i32) -> i32 {
    let region_size = region_size.max(1);
    ((view_radius.max(0) + region_size - 1) / region_size).max(1)
}

#[must_use]
pub fn default_packed_gpu_generation_region_radius() -> i32 {
    packed_gpu_generation_region_radius_for_view(
        packed_view_radius_from_env(),
        packed_region_size_from_env(),
    )
}

fn packed_gpu_generation_region_radius_from_env() -> i32 {
    std::env::var(PACKED_GPU_GENERATION_REGION_RADIUS_ENV)
        .ok()
        .and_then(|value| value.parse::<i32>().ok())
        .filter(|value| *value >= 0)
        .unwrap_or_else(default_packed_gpu_generation_region_radius)
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

fn env_f32_default(name: &str, default: f32) -> f32 {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse::<f32>().ok())
        .filter(|value| value.is_finite())
        .unwrap_or(default)
}

fn filter_debug_packed_quads(quads: &mut Vec<PackedVoxelQuad>) {
    if env_flag(PACKED_TOP_ONLY_ENV) {
        quads.retain(|quad| quad.face() == PackedVoxelFace::PlusY as u8);
    }
}

/// Bevy plugin to register resources and stages for the Packed Voxel Quad Pipeline.
pub struct PackedQuadPipelinePlugin;

impl Plugin for PackedQuadPipelinePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PackedQuadBatches>();
        app.init_resource::<PackedGpuGenerationBatches>();
        app.init_resource::<PackedGpuGenerationRegionScratch>();
        app.init_resource::<PackedQuadBlockTexturePalette>();
        app.init_resource::<PackedQuadPipelineStats>();
        let atlas = {
            let asset_server = app.world().resource::<AssetServer>();
            load_block_atlas(asset_server)
        };
        app.insert_resource(PackedQuadBlockAtlas { handle: atlas });
        app.add_plugins(ExtractResourcePlugin::<PackedQuadBatches>::default());
        app.add_plugins(ExtractResourcePlugin::<PackedGpuGenerationBatches>::default());
        app.add_plugins(ExtractResourcePlugin::<PackedQuadBlockTexturePalette>::default());
        app.add_plugins(ExtractResourcePlugin::<PackedQuadBlockAtlas>::default());

        app.add_systems(
            Update,
            (
                sync_packed_quad_metrics,
                prune_confirmed_packed_dirty_ranges,
                sync_packed_quad_block_texture_palette,
            ),
        );

        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };

        render_app.init_resource::<PreparedPackedQuadBatches>();
        render_app.init_resource::<PreparedPackedQuadBlockTexturePalette>();
        render_app.init_resource::<PackedQuadGpuArena>();
        render_app.init_resource::<PackedQuadGpuMemoryReserve>();
        render_app.init_resource::<PackedQuadIndirectBuffer>();
        render_app.init_resource::<PackedQuadParamsBuffer>();
        render_app.init_resource::<PackedQuadCpuVisibleIndirectBuffer>();
        render_app.init_resource::<PreparedPackedQuadIndirectDraw>();
        render_app.init_resource::<PreparedPackedQuadGpuCull>();
        render_app.add_systems(
            Render,
            prepare_packed_quad_buffers.in_set(RenderSystems::Prepare),
        );
    }
}

/// Helper to pack an (x, z) chunk column coordinate into a single u64 key.
/// Signed coordinates are cast to u32 first to handle negative values safely.
#[inline]
pub fn pack_chunk_key(x: i32, z: i32) -> u64 {
    let ux = x as u32 as u64;
    let uz = z as u32 as u64;
    (ux << 32) | uz
}

/// Helper to unpack a single u64 key back into signed (x, z) chunk coordinates.
#[inline]
pub fn unpack_chunk_key(key: u64) -> (i32, i32) {
    let x = (key >> 32) as u32 as i32;
    let z = (key & 0xFFFFFFFF) as u32 as i32;
    (x, z)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PendingChunk {
    pub x: i32,
    pub z: i32,
    pub offset_x: i32,
    pub offset_z: i32,
    pub distance_sq: i32,
}

#[derive(Resource, Default)]
pub struct PackedQuadStreamingState {
    pub loaded: HashMap<u64, LoadedPackedQuadChunk>,
    pub building: HashMap<u64, Entity>,
    pub pending: Vec<PendingChunk>,
    pub active_render_chunks: HashSet<u64>,
    pub last_center: Option<IVec2>,
    pub region_generations: HashMap<u64, u64>,
    pub pending_rebuild_regions: Vec<u64>,
    pub pending_compaction_regions: Vec<u64>,
}

pub struct LoadedPackedQuadChunk {
    pub entity: Entity,
    pub region_key: u64,
    pub quads: Vec<PackedVoxelQuad>,
}

#[derive(Component)]
pub struct PackedQuadChunkBuildTask(Task<BuiltPackedQuadChunk>);

pub struct BuiltPackedQuadChunk {
    pub key: u64,
    pub region_key: u64,
    pub quads: Vec<PackedVoxelQuad>,
}

fn build_packed_quad_chunk(
    pending: PendingChunk,
    context: WorldGenerationContext,
    region_size: i32,
    lod_enabled: bool,
    min_cell_size: usize,
    sand_block: u16,
) -> BuiltPackedQuadChunk {
    let chunk_pos = ChunkPos::new(pending.x, pending.z);
    let lod_step = if lod_enabled {
        packed_seam_safe_lod_step(pending, min_cell_size)
    } else {
        min_cell_size
    };
    let mut quads = crate::voxel_packed_quads::build_surface_packed_quads_for_chunk(
        chunk_pos, &context, lod_step, sand_block,
    );
    filter_debug_packed_quads(&mut quads);

    let (region_origin_x, region_origin_z) =
        packed_region_origin_for_chunk(pending.x, pending.z, region_size);
    let region_key = pack_chunk_key(region_origin_x, region_origin_z);
    let quads = offset_quads_to_region(
        &quads,
        pending.x,
        pending.z,
        region_origin_x,
        region_origin_z,
    );

    BuiltPackedQuadChunk {
        key: pack_chunk_key(pending.x, pending.z),
        region_key,
        quads,
    }
}

fn packed_seam_safe_lod_step(pending: PendingChunk, min_cell_size: usize) -> usize {
    let own_step = packed_lod_step_for_distance_sq(pending.distance_sq, min_cell_size);
    if own_step <= min_cell_size {
        return own_step;
    }

    let neighbor_offsets = [
        (pending.offset_x + 1, pending.offset_z),
        (pending.offset_x - 1, pending.offset_z),
        (pending.offset_x, pending.offset_z + 1),
        (pending.offset_x, pending.offset_z - 1),
    ];

    for (offset_x, offset_z) in neighbor_offsets {
        let neighbor_distance_sq = offset_x * offset_x + offset_z * offset_z;
        let neighbor_step = packed_lod_step_for_distance_sq(neighbor_distance_sq, min_cell_size);
        if neighbor_step < own_step {
            return neighbor_step;
        }
    }

    own_step
}

fn packed_render_chunks_for_center(center: IVec2, view_radius: i32) -> Vec<PendingChunk> {
    let radius = view_radius.max(0);
    let radius_sq = radius * radius;
    let mut chunks = Vec::with_capacity(packed_chunk_count_for_radius(radius));

    for dz in -radius..=radius {
        for dx in -radius..=radius {
            let distance_sq = dx * dx + dz * dz;
            if distance_sq > radius_sq {
                continue;
            }
            chunks.push(PendingChunk {
                x: center.x + dx,
                z: center.y + dz,
                offset_x: dx,
                offset_z: dz,
                distance_sq,
            });
        }
    }

    chunks
}

fn active_loaded_chunk_count(state: &PackedQuadStreamingState) -> usize {
    state
        .loaded
        .keys()
        .filter(|key| state.active_render_chunks.contains(key))
        .count()
}

fn sync_region_batch_active_flags(
    batches: &mut PackedQuadBatches,
    state: &PackedQuadStreamingState,
    region_key: u64,
) {
    let Some(batch) = batches
        .batches
        .iter_mut()
        .find(|batch| batch.key == region_key)
    else {
        return;
    };

    let ranges = Arc::make_mut(&mut batch.chunk_ranges);
    for range in ranges {
        range.active = range.resident && state.active_render_chunks.contains(&range.chunk_key);
    }
}

fn evict_chunk_from_region_batch(
    batches: &mut PackedQuadBatches,
    state: &mut PackedQuadStreamingState,
    chunk_key: u64,
    region_key: u64,
) -> bool {
    let Some(batch_index) = batches
        .batches
        .iter()
        .position(|batch| batch.key == region_key)
    else {
        return false;
    };

    let has_resident_ranges = {
        let batch = &mut batches.batches[batch_index];
        let ranges = Arc::make_mut(&mut batch.chunk_ranges);
        let Some(range) = ranges
            .iter_mut()
            .find(|range| range.chunk_key == chunk_key && range.resident)
        else {
            return false;
        };

        range.active = false;
        range.resident = false;
        range.capacity_quads = range.capacity_quads.max(range.len_quads);
        range.len_quads = 0;
        batch.needs_compaction = false;
        ranges.iter().any(|range| range.resident)
    };

    state
        .pending_compaction_regions
        .retain(|pending_key| *pending_key != region_key);
    state
        .pending_rebuild_regions
        .retain(|pending_key| *pending_key != region_key);

    if has_resident_ranges {
        return true;
    }

    batches.batches.remove(batch_index);
    state.region_generations.remove(&region_key);
    true
}

fn record_packed_streaming_counts(state: &PackedQuadStreamingState) {
    METRICS_BRIDGE
        .chunks_loaded
        .store(state.loaded.len(), Ordering::Relaxed);
    METRICS_BRIDGE
        .chunks_active
        .store(active_loaded_chunk_count(state), Ordering::Relaxed);
    METRICS_BRIDGE.pending_builds.store(
        state.pending.len() + state.building.len(),
        Ordering::Relaxed,
    );
    METRICS_BRIDGE
        .pending_region_rebuilds
        .store(state.pending_rebuild_regions.len(), Ordering::Relaxed);
}

fn elapsed_us(started_at: Instant) -> u64 {
    started_at.elapsed().as_micros().min(u128::from(u64::MAX)) as u64
}

fn packed_material_uniform(
    chunk_translation_and_offset: Vec4,
) -> crate::packed_quad_material::PackedVoxelUniform {
    let fog_start = env_f32_default(PACKED_FOG_START_ENV, DEFAULT_PACKED_FOG_START);
    let fog_end = env_f32_default(PACKED_FOG_END_ENV, DEFAULT_PACKED_FOG_END).max(fog_start + 1.0);
    crate::packed_quad_material::PackedVoxelUniform {
        chunk_translation_and_offset,
        fog_color_and_start: Vec4::new(0.5, 0.6, 0.8, fog_start),
        fog_end_and_padding: Vec4::new(fog_end, 0.0, 0.0, 0.0),
    }
}

/// Main World system that tracks player camera and streams packed voxel quad chunks.
pub fn stream_packed_quad_chunks(
    mut commands: Commands,
    time: Res<Time>,
    registry: Res<BlockRegistry>,
    camera_query: Query<&GlobalTransform, With<Camera3d>>,
    mut batches: ResMut<PackedQuadBatches>,
    mut state: ResMut<PackedQuadStreamingState>,
) {
    let system_started_at = Instant::now();
    let Some(camera_transform) = camera_query.iter().next() else {
        record_packed_streaming_counts(&state);
        METRICS_BRIDGE
            .stream_spawned_builds
            .store(0, Ordering::Relaxed);
        METRICS_BRIDGE
            .stream_rebuild_regions
            .store(0, Ordering::Relaxed);
        METRICS_BRIDGE
            .stream_system_us
            .store(elapsed_us(system_started_at), Ordering::Relaxed);
        return;
    };
    let camera_pos = camera_transform.translation();
    let center = IVec2::new(
        (camera_pos.x / 32.0).floor() as i32,
        (camera_pos.z / 32.0).floor() as i32,
    );

    let view_radius = packed_view_radius_from_env();
    let region_size = packed_region_size_from_env();
    let despawn_radius = view_radius + 2;
    let mut regions_to_sync = HashSet::new();

    if state.last_center != Some(center) {
        let render_chunks = packed_render_chunks_for_center(center, view_radius);
        let next_active_render_chunks = render_chunks
            .iter()
            .map(|chunk| pack_chunk_key(chunk.x, chunk.z))
            .collect::<HashSet<_>>();
        let previous_active_render_chunks =
            std::mem::replace(&mut state.active_render_chunks, next_active_render_chunks);
        let despawn_radius_sq = despawn_radius * despawn_radius;
        let mut to_despawn = Vec::new();
        for &key in state.loaded.keys() {
            let (cx, cz) = unpack_chunk_key(key);
            let dx = cx - center.x;
            let dz = cz - center.y;
            if dx * dx + dz * dz > despawn_radius_sq {
                to_despawn.push(key);
            }
        }
        for key in to_despawn {
            if let Some(loaded) = state.loaded.remove(&key) {
                commands.entity(loaded.entity).despawn();
                evict_chunk_from_region_batch(&mut batches, &mut state, key, loaded.region_key);
            }
        }

        let mut building_to_despawn = Vec::new();
        for (&key, &entity) in &state.building {
            let (cx, cz) = unpack_chunk_key(key);
            let dx = cx - center.x;
            let dz = cz - center.y;
            if dx * dx + dz * dz > despawn_radius_sq {
                building_to_despawn.push((key, entity));
            }
        }
        for (key, entity) in building_to_despawn {
            state.building.remove(&key);
            commands.entity(entity).despawn();
        }

        let membership_changes = previous_active_render_chunks
            .symmetric_difference(&state.active_render_chunks)
            .copied()
            .collect::<Vec<_>>();
        for key in membership_changes {
            let Some(loaded) = state.loaded.get(&key) else {
                continue;
            };
            regions_to_sync.insert(loaded.region_key);
            if state.active_render_chunks.contains(&key) {
                commands
                    .entity(loaded.entity)
                    .insert((RenderedChunk, RenderedChunkCount(1)));
            } else {
                commands
                    .entity(loaded.entity)
                    .remove::<RenderedChunk>()
                    .remove::<RenderedChunkCount>();
            }
        }

        let mut pending = Vec::new();
        for chunk in render_chunks {
            let key = pack_chunk_key(chunk.x, chunk.z);
            if !state.loaded.contains_key(&key) && !state.building.contains_key(&key) {
                pending.push(chunk);
            }
        }

        // Sort pending chunks: prioritize closest chunks first (closer distance_sq means pop last)
        pending.sort_by_key(|chunk| std::cmp::Reverse(chunk.distance_sq));
        state.pending = pending;
        state.last_center = Some(center);
    }

    for region_key in regions_to_sync {
        sync_region_batch_active_flags(&mut batches, &state, region_key);
    }

    let max_rebuilds = adaptive_packed_streaming_budget(
        packed_max_rebuilds_per_frame_from_env().max(1),
        time.delta_secs(),
        packed_target_frame_secs_from_env(),
        env_flag_default(PACKED_ADAPTIVE_STREAMING_ENV, true),
    );
    let rebuilt_region_count =
        rebuild_queued_region_batches(&mut batches, &mut state, max_rebuilds);

    let max_builds = adaptive_packed_streaming_budget(
        packed_max_builds_per_frame_from_env(),
        time.delta_secs(),
        packed_target_frame_secs_from_env(),
        env_flag_default(PACKED_ADAPTIVE_STREAMING_ENV, true),
    );
    let max_build_tasks = packed_max_build_tasks_from_env();
    let lod_enabled = env_flag_default(PACKED_LOD_ENV, true);
    let min_cell_size = packed_min_cell_size_from_env();
    let context = WorldGenerationContext::from_registry(&registry);
    let sand_block = registry.get_id("sand").unwrap_or(context.palette.dirt);
    let thread_pool = AsyncComputeTaskPool::get();
    let mut spawned_this_frame = 0;

    while spawned_this_frame < max_builds && state.building.len() < max_build_tasks {
        let Some(pending) = state.pending.pop() else {
            break;
        };

        let key = pack_chunk_key(pending.x, pending.z);
        if state.loaded.contains_key(&key) || state.building.contains_key(&key) {
            continue;
        }

        let context = context.clone();
        let task = thread_pool.spawn(async move {
            build_packed_quad_chunk(
                pending,
                context,
                region_size,
                lod_enabled,
                min_cell_size,
                sand_block,
            )
        });
        let entity = commands.spawn(PackedQuadChunkBuildTask(task)).id();
        state.building.insert(key, entity);
        spawned_this_frame += 1;
    }

    record_packed_streaming_counts(&state);
    METRICS_BRIDGE
        .stream_spawned_builds
        .store(spawned_this_frame, Ordering::Relaxed);
    METRICS_BRIDGE
        .stream_rebuild_regions
        .store(rebuilt_region_count, Ordering::Relaxed);
    METRICS_BRIDGE
        .stream_system_us
        .store(elapsed_us(system_started_at), Ordering::Relaxed);
}

pub fn handle_packed_quad_build_tasks(
    mut commands: Commands,
    time: Res<Time>,
    mut batches: ResMut<PackedQuadBatches>,
    mut state: ResMut<PackedQuadStreamingState>,
    mut tasks: Query<(Entity, &mut PackedQuadChunkBuildTask)>,
) {
    let system_started_at = Instant::now();
    let mut built_this_frame = 0;
    let max_completions = adaptive_packed_streaming_budget(
        packed_max_completions_per_frame_from_env().max(1),
        time.delta_secs(),
        packed_target_frame_secs_from_env(),
        env_flag_default(PACKED_ADAPTIVE_STREAMING_ENV, true),
    );

    for (entity, mut task) in &mut tasks {
        if built_this_frame >= max_completions {
            break;
        }

        let Some(built) = check_ready(&mut task.0) else {
            continue;
        };

        if state.loaded.contains_key(&built.key) {
            state.building.remove(&built.key);
            commands.entity(entity).despawn();
            continue;
        }

        state.building.remove(&built.key);
        let is_active_render_chunk = state.active_render_chunks.contains(&built.key);
        commands.entity(entity).remove::<PackedQuadChunkBuildTask>();
        append_chunk_to_region_batch(
            &mut batches,
            &mut state,
            built.key,
            built.region_key,
            &built.quads,
        );

        if is_active_render_chunk {
            commands
                .entity(entity)
                .insert((RenderedChunk, RenderedChunkCount(1)));
        }
        state.loaded.insert(
            built.key,
            LoadedPackedQuadChunk {
                entity,
                region_key: built.region_key,
                quads: built.quads,
            },
        );
        built_this_frame += 1;
    }

    METRICS_BRIDGE
        .built_this_frame
        .store(built_this_frame, Ordering::Relaxed);
    METRICS_BRIDGE
        .build_task_system_us
        .store(elapsed_us(system_started_at), Ordering::Relaxed);
    record_packed_streaming_counts(&state);
    record_packed_cpu_reserved_metrics(&batches);
}

fn next_region_generation(state: &mut PackedQuadStreamingState, region_key: u64) -> u64 {
    let generation = state
        .region_generations
        .entry(region_key)
        .and_modify(|generation| *generation += 1)
        .or_insert(1);
    *generation
}

fn mark_region_for_deferred_compaction(state: &mut PackedQuadStreamingState, region_key: u64) {
    if !state.pending_compaction_regions.contains(&region_key) {
        state.pending_compaction_regions.push(region_key);
    }
}

#[cfg(test)]
fn queue_region_rebuild(state: &mut PackedQuadStreamingState, region_key: u64) {
    if !state.pending_rebuild_regions.contains(&region_key) {
        state.pending_rebuild_regions.push(region_key);
    }
}

fn rebuild_queued_region_batches(
    batches: &mut PackedQuadBatches,
    state: &mut PackedQuadStreamingState,
    max_rebuilds: usize,
) -> usize {
    let mut rebuilt_this_frame = 0;
    while rebuilt_this_frame < max_rebuilds {
        let Some(region_key) = state.pending_rebuild_regions.pop() else {
            break;
        };
        rebuild_region_batch(batches, state, region_key);
        rebuilt_this_frame += 1;
    }
    rebuilt_this_frame
}

fn append_chunk_to_region_batch(
    batches: &mut PackedQuadBatches,
    state: &mut PackedQuadStreamingState,
    chunk_key: u64,
    region_key: u64,
    quads: &[PackedVoxelQuad],
) {
    append_chunk_to_region_batch_with_capacity(
        batches,
        state,
        chunk_key,
        region_key,
        quads,
        packed_cpu_region_prealloc_quads_from_env(),
    );
}

fn append_chunk_to_region_batch_with_capacity(
    batches: &mut PackedQuadBatches,
    state: &mut PackedQuadStreamingState,
    chunk_key: u64,
    region_key: u64,
    quads: &[PackedVoxelQuad],
    reserved_capacity_quads: usize,
) {
    append_chunk_to_region_batch_with_capacity_and_mode(
        batches,
        state,
        chunk_key,
        region_key,
        quads,
        reserved_capacity_quads,
        packed_defer_compaction_from_env(),
    );
}

fn append_chunk_to_region_batch_with_capacity_and_mode(
    batches: &mut PackedQuadBatches,
    state: &mut PackedQuadStreamingState,
    chunk_key: u64,
    region_key: u64,
    quads: &[PackedVoxelQuad],
    reserved_capacity_quads: usize,
    defer_compaction: bool,
) {
    let generation = next_region_generation(state, region_key);
    let active = state.active_render_chunks.contains(&chunk_key);

    if let Some(batch) = batches
        .batches
        .iter_mut()
        .find(|batch| batch.key == region_key)
    {
        let reusable_range_index = batch.chunk_ranges.iter().position(|range| {
            !range.resident
                && range.capacity_quads >= quads.len()
                && range.start_quads.saturating_add(range.capacity_quads) <= batch.quads.len()
        });
        if let Some(range_index) = reusable_range_index {
            let (start_quads, capacity_quads) = {
                let range = batch.chunk_ranges[range_index];
                (range.start_quads, range.capacity_quads)
            };
            if !quads.is_empty() {
                let batch_quads = Arc::make_mut(&mut batch.quads);
                batch_quads[start_quads..start_quads + quads.len()].copy_from_slice(quads);
            }
            let ranges = Arc::make_mut(&mut batch.chunk_ranges);
            ranges[range_index] = PackedQuadChunkRange {
                chunk_key,
                start_quads,
                len_quads: quads.len(),
                capacity_quads,
                active,
                resident: true,
            };
            if !quads.is_empty() {
                Arc::make_mut(&mut batch.dirty_ranges).push(PackedQuadDirtyRange {
                    start_quads,
                    len_quads: quads.len(),
                    generation,
                });
            }
            batch.generation = generation;
            batch.needs_compaction = false;
            state
                .pending_compaction_regions
                .retain(|pending_key| *pending_key != region_key);
            return;
        }

        let start_quads = {
            let batch_quads = Arc::make_mut(&mut batch.quads);
            let start_quads = batch_quads.len();
            batch_quads.extend_from_slice(quads);
            start_quads
        };
        Arc::make_mut(&mut batch.chunk_ranges).push(PackedQuadChunkRange {
            chunk_key,
            start_quads,
            len_quads: quads.len(),
            capacity_quads: quads.len(),
            active,
            resident: true,
        });
        batch.generation = generation;
        if defer_compaction {
            batch.needs_compaction = true;
            mark_region_for_deferred_compaction(state, region_key);
        } else {
            compact_region_batch_preserving_chunk_ranges(batch);
            batch.needs_compaction = false;
        }
    } else {
        let mut region_quads = Vec::with_capacity(reserved_capacity_quads.max(quads.len()));
        region_quads.extend_from_slice(quads);
        batches.batches.push(PackedQuadBatch {
            key: region_key,
            quads: Arc::new(region_quads),
            chunk_ranges: Arc::new(vec![PackedQuadChunkRange {
                chunk_key,
                start_quads: 0,
                len_quads: quads.len(),
                capacity_quads: quads.len(),
                active,
                resident: true,
            }]),
            dirty_ranges: Arc::new(Vec::new()),
            generation,
            needs_compaction: false,
        });
    }
}

fn compact_region_batch_preserving_chunk_ranges(batch: &mut PackedQuadBatch) -> bool {
    if batch.quads.len() < 2 || batch.chunk_ranges.is_empty() {
        return false;
    }

    let old_quads = batch.quads.as_slice();
    let old_ranges = batch.chunk_ranges.as_slice();
    let mut compacted_quads = Vec::with_capacity(batch.quads.capacity());
    let mut compacted_ranges = Vec::with_capacity(old_ranges.len());

    for range in old_ranges {
        let start_quads = range.start_quads.min(old_quads.len());
        let slot_len_quads = if range.resident {
            range.len_quads
        } else {
            range.capacity_quads
        };
        let end_quads = start_quads
            .saturating_add(slot_len_quads)
            .min(old_quads.len());
        if !range.resident {
            let compacted_start = compacted_quads.len();
            compacted_quads.extend_from_slice(&old_quads[start_quads..end_quads]);
            compacted_ranges.push(PackedQuadChunkRange {
                start_quads: compacted_start,
                len_quads: 0,
                capacity_quads: end_quads - start_quads,
                active: false,
                resident: false,
                ..*range
            });
            continue;
        }

        let mut chunk_quads = old_quads[start_quads..end_quads].to_vec();
        crate::voxel_packed_quads::compact_packed_quads(&mut chunk_quads);

        let compacted_start = compacted_quads.len();
        compacted_quads.extend_from_slice(&chunk_quads);
        compacted_ranges.push(PackedQuadChunkRange {
            start_quads: compacted_start,
            len_quads: chunk_quads.len(),
            capacity_quads: chunk_quads.len(),
            ..*range
        });
    }

    let changed = compacted_quads.as_slice() != old_quads || compacted_ranges != old_ranges;
    if changed {
        batch.quads = Arc::new(compacted_quads);
        batch.chunk_ranges = Arc::new(compacted_ranges);
        batch.dirty_ranges = Arc::new(Vec::new());
    }
    changed
}

fn rebuild_region_batch(
    batches: &mut PackedQuadBatches,
    state: &mut PackedQuadStreamingState,
    region_key: u64,
) {
    let mut chunk_keys = state
        .loaded
        .iter()
        .filter_map(|(&chunk_key, loaded)| (loaded.region_key == region_key).then_some(chunk_key))
        .collect::<Vec<_>>();
    chunk_keys.sort_unstable();

    batches.batches.retain(|batch| batch.key != region_key);

    if chunk_keys.is_empty() {
        state.region_generations.remove(&region_key);
        state
            .pending_compaction_regions
            .retain(|pending_key| *pending_key != region_key);
        return;
    }

    let total_quads = chunk_keys
        .iter()
        .map(|chunk_key| state.loaded[chunk_key].quads.len())
        .sum();
    let mut region_quads =
        Vec::with_capacity(packed_cpu_region_prealloc_quads_from_env().max(total_quads));
    let mut chunk_ranges = Vec::with_capacity(chunk_keys.len());
    for chunk_key in chunk_keys {
        let start_quads = region_quads.len();
        let loaded_quads = &state.loaded[&chunk_key].quads;
        region_quads.extend_from_slice(loaded_quads);
        chunk_ranges.push(PackedQuadChunkRange {
            chunk_key,
            start_quads,
            len_quads: loaded_quads.len(),
            capacity_quads: loaded_quads.len(),
            active: state.active_render_chunks.contains(&chunk_key),
            resident: true,
        });
    }
    let defer_compaction = packed_defer_compaction_from_env();

    let generation = next_region_generation(state, region_key);

    let mut batch = PackedQuadBatch {
        key: region_key,
        quads: Arc::new(region_quads),
        chunk_ranges: Arc::new(chunk_ranges),
        dirty_ranges: Arc::new(Vec::new()),
        generation,
        needs_compaction: defer_compaction,
    };
    if !defer_compaction {
        compact_region_batch_preserving_chunk_ranges(&mut batch);
    }
    batches.batches.push(batch);
    if defer_compaction {
        mark_region_for_deferred_compaction(state, region_key);
    }
}

fn compact_deferred_packed_region_batch(
    batches: &mut PackedQuadBatches,
    state: &mut PackedQuadStreamingState,
    region_key: u64,
) -> bool {
    let Some(batch_index) = batches
        .batches
        .iter()
        .position(|batch| batch.key == region_key)
    else {
        return false;
    };

    let changed = compact_region_batch_preserving_chunk_ranges(&mut batches.batches[batch_index]);
    if changed {
        batches.batches[batch_index].generation = next_region_generation(state, region_key);
    }
    batches.batches[batch_index].needs_compaction = false;
    true
}

pub fn compact_pending_packed_regions(
    time: Res<Time>,
    mut batches: ResMut<PackedQuadBatches>,
    mut state: ResMut<PackedQuadStreamingState>,
) {
    let system_started_at = Instant::now();
    if !packed_defer_compaction_from_env() {
        METRICS_BRIDGE
            .compacted_regions_this_frame
            .store(0, Ordering::Relaxed);
        METRICS_BRIDGE
            .compaction_system_us
            .store(elapsed_us(system_started_at), Ordering::Relaxed);
        return;
    }

    let max_compactions = adaptive_packed_background_budget(
        packed_max_compactions_per_frame_from_env(),
        time.delta_secs(),
        packed_target_frame_secs_from_env(),
        env_flag_default(PACKED_ADAPTIVE_STREAMING_ENV, true),
    );
    let mut compacted_this_frame = 0;

    while compacted_this_frame < max_compactions {
        let Some(region_key) = state.pending_compaction_regions.pop() else {
            break;
        };
        if compact_deferred_packed_region_batch(&mut batches, &mut state, region_key) {
            compacted_this_frame += 1;
        }
    }

    if compacted_this_frame > 0 {
        record_packed_cpu_reserved_metrics(&batches);
    }
    METRICS_BRIDGE
        .compacted_regions_this_frame
        .store(compacted_this_frame, Ordering::Relaxed);
    METRICS_BRIDGE
        .compaction_system_us
        .store(elapsed_us(system_started_at), Ordering::Relaxed);
}

fn record_packed_cpu_reserved_metrics(batches: &PackedQuadBatches) {
    let reserved_quads = batches
        .batches
        .iter()
        .map(|batch| batch.quads.capacity())
        .sum::<usize>();
    METRICS_BRIDGE
        .cpu_reserved_quads
        .store(reserved_quads, Ordering::Relaxed);
    METRICS_BRIDGE.cpu_reserved_bytes.store(
        (reserved_quads * std::mem::size_of::<PackedVoxelQuad>()) as u64,
        Ordering::Relaxed,
    );
}

#[derive(Resource, Default)]
pub struct PackedMaterialEntities {
    pub entities: HashMap<u64, (Entity, u64)>,
}

pub fn sync_packed_material_entities(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<crate::packed_quad_material::PackedVoxelMaterial>>,
    batches: Res<PackedQuadBatches>,
    mut entity_map: ResMut<PackedMaterialEntities>,
    atlas: Res<PackedQuadBlockAtlas>,
) {
    let system_started_at = Instant::now();
    let active_keys: std::collections::HashSet<u64> =
        batches.batches.iter().map(|b| b.key).collect();

    entity_map.entities.retain(|key, (entity, _)| {
        if !active_keys.contains(key) {
            if let Ok(mut entity_commands) = commands.get_entity(*entity) {
                entity_commands.despawn();
            }
            false
        } else {
            true
        }
    });

    for batch in &batches.batches {
        let current_gen = batch.generation;
        let needs_update = match entity_map.entities.get(&batch.key) {
            Some((_, g)) => *g != current_gen,
            None => true,
        };

        if needs_update {
            let vertex_count = batch.quads.len() * 6;

            let mut mesh = Mesh::new(
                bevy::render::render_resource::PrimitiveTopology::TriangleList,
                bevy::asset::RenderAssetUsages::RENDER_WORLD,
            );
            if vertex_count > 0 {
                let region_size = packed_region_size_from_env();
                let (bounds_min, bounds_max) = packed_region_world_bounds(batch.key, region_size);
                let x_min = bounds_min.x;
                let z_min = bounds_min.z;
                let x_max = bounds_max.x;
                let z_max = bounds_max.z;

                let mut positions = vec![Vec3::new(x_min, 0.0, z_min); vertex_count];
                if vertex_count >= 2 {
                    positions[1] = Vec3::new(x_max, bounds_max.y, z_max);
                }
                let local_vertex_ids = (0..vertex_count)
                    .map(|vertex_index| [vertex_index as f32, 0.0])
                    .collect::<Vec<_>>();
                mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
                mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, local_vertex_ids);
            }
            let mesh_handle = meshes.add(mesh);

            if let Some(&(entity, _)) = entity_map.entities.get(&batch.key) {
                if let Ok(mut entity_commands) = commands.get_entity(entity) {
                    entity_commands.insert(Mesh3d(mesh_handle));
                }
                entity_map.entities.insert(batch.key, (entity, current_gen));
            } else {
                let material = crate::packed_quad_material::PackedVoxelMaterial {
                    atlas: atlas.handle.clone(),
                    batch_key: batch.key,
                };
                let entity = commands
                    .spawn((
                        Mesh3d(mesh_handle),
                        MeshMaterial3d(materials.add(material)),
                        Transform::default(),
                        Visibility::Visible,
                    ))
                    .id();
                entity_map.entities.insert(batch.key, (entity, current_gen));
            }
        }
    }

    let active_quads = batches
        .batches
        .iter()
        .map(|batch| batch.quads.len())
        .sum::<usize>();
    record_packed_material_draw_estimate(
        entity_map.entities.len(),
        active_quads,
        elapsed_us(system_started_at),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use rumpel_blocks::BlockData;

    fn test_quads(quads: Vec<PackedVoxelQuad>) -> Arc<Vec<PackedVoxelQuad>> {
        Arc::new(quads)
    }

    fn test_block(id: &str, is_solid: bool) -> BlockData {
        BlockData {
            id: id.to_string(),
            name: id.to_string(),
            is_solid,
            is_transparent: !is_solid,
            color: (1.0, 1.0, 1.0, 1.0),
            gravity_affected: false,
            strength: 1.0,
        }
    }

    fn test_terrain_registry() -> BlockRegistry {
        let mut registry = BlockRegistry::empty();
        registry.register_block(test_block("air", false));
        registry.register_block(test_block("dirt", true));
        registry.register_block(test_block("grass", true));
        registry.register_block(test_block("stone", true));
        registry.register_block(test_block("sand", true));
        registry
    }

    #[test]
    fn test_next_packed_quad_capacity() {
        assert_eq!(next_packed_quad_capacity(32, 16), 32);
        assert_eq!(next_packed_quad_capacity(32, 32), 32);
        assert_eq!(next_packed_quad_capacity(0, 0), 0);
        assert_eq!(next_packed_quad_capacity(16, 0), 16);
        assert_eq!(next_packed_quad_capacity(0, 5), 16);
        assert_eq!(next_packed_quad_capacity(0, 16), 16);
        assert_eq!(next_packed_quad_capacity(0, 17), 32);
        assert_eq!(next_packed_quad_capacity(16, 17), 32);
        assert_eq!(next_packed_quad_capacity(32, 45), 64);
    }

    #[test]
    fn test_packed_quad_slot_capacity_does_not_power_of_two_region_slots() {
        assert_eq!(packed_quad_slot_capacity(0), 0);
        assert_eq!(packed_quad_slot_capacity(5), 16);
        assert_eq!(packed_quad_slot_capacity(16), 16);
        assert_eq!(packed_quad_slot_capacity(17), 17);
        assert_eq!(packed_quad_slot_capacity(10_240), 10_240);
    }

    #[test]
    fn test_estimate_quads_bytes() {
        assert_eq!(estimate_quads_bytes(0), 0);
        assert_eq!(estimate_quads_bytes(1), 16);
        assert_eq!(estimate_quads_bytes(10), 160);
        assert_eq!(estimate_quads_bytes(1024), 16384);
    }

    #[test]
    fn test_gpu_generation_contract_tracks_source_versions() {
        let mut registry = test_terrain_registry();
        let context = WorldGenerationContext::from_registry(&registry);
        let sand = registry.get_id("sand").expect("sand id");
        let base = build_packed_gpu_generation_cache_contract(&registry, &context, 4, 2, sand);

        registry.register_block(test_block("cobblestone", true));
        assert_eq!(
            base,
            build_packed_gpu_generation_cache_contract(&registry, &context, 4, 2, sand),
            "unrelated block material changes should not invalidate generated terrain"
        );

        registry
            .texture_mappings
            .write()
            .expect("texture mapping lock")
            .insert(sand, [9, 10, 11]);
        assert_ne!(
            base.generation(),
            build_packed_gpu_generation_cache_contract(&registry, &context, 4, 2, sand)
                .generation()
        );
    }

    #[test]
    fn test_generated_region_cache_reuse_requires_matching_contract() {
        let region_key = pack_chunk_key(0, 0);
        let contract = PackedGpuGenerationCacheContract::new(4, 2, [0, 1, 2, 3], 4, 10, 20);
        let stale_contract = PackedGpuGenerationCacheContract::new(4, 2, [0, 1, 2, 3], 4, 10, 21);
        let mut cache = crate::packed_quad_gpu_generation::GeneratedRegionCache::default();

        cache.entries.insert(
            region_key,
            crate::packed_quad_gpu_generation::GeneratedRegionCacheEntry {
                key: region_key,
                columns: Arc::new(Vec::<
                    crate::packed_quad_gpu_generation::PackedGpuSurfaceColumn,
                >::new()),
                params: PackedGpuGenerationParams::new(0, 0, 0, 0, 1, 2, 3),
                source_chunk_count: 0,
                max_output_quads: 0,
                translation: Vec4::ZERO,
                bounds_min: Vec3::ZERO,
                bounds_max: Vec3::ZERO,
                generation: contract.generation(),
                contract,
                edit_store_generation: 0,
                last_seen_frame: 1,
            },
        );

        let edit_store = WorldEditStore::default();
        let reused = reuse_generated_region_cache_entry(
            &mut cache,
            region_key,
            0,
            0,
            4,
            contract,
            &edit_store,
            7,
        )
        .expect("reuse");
        assert_eq!(reused.generation, contract.generation());
        assert_eq!(cache.entries[&region_key].last_seen_frame, 7);

        assert!(
            reuse_generated_region_cache_entry(
                &mut cache,
                region_key,
                0,
                0,
                4,
                stale_contract,
                &edit_store,
                8,
            )
            .is_none()
        );
        let stale_entry = cache
            .entries
            .remove(&region_key)
            .expect("stale entry remains available for rebuild logging");
        assert_eq!(stale_entry.contract, contract);
        assert_eq!(stale_entry.generation, contract.generation());
    }

    #[test]
    fn test_generated_region_cache_rejects_edits_after_snapshot() {
        let region_key = pack_chunk_key(0, 0);
        let contract = PackedGpuGenerationCacheContract::new(4, 2, [0, 1, 2, 3], 4, 10, 20);
        let mut cache = crate::packed_quad_gpu_generation::GeneratedRegionCache::default();
        let mut edit_store = WorldEditStore::default();

        cache.entries.insert(
            region_key,
            crate::packed_quad_gpu_generation::GeneratedRegionCacheEntry {
                key: region_key,
                columns: Arc::new(Vec::new()),
                params: PackedGpuGenerationParams::new(0, 0, 0, 0, 1, 2, 3),
                source_chunk_count: 0,
                max_output_quads: 0,
                translation: Vec4::ZERO,
                bounds_min: Vec3::ZERO,
                bounds_max: Vec3::ZERO,
                generation: contract.generation(),
                contract,
                edit_store_generation: edit_store.generation(),
                last_seen_frame: 1,
            },
        );

        assert!(
            reuse_generated_region_cache_entry(
                &mut cache,
                region_key,
                0,
                0,
                4,
                contract,
                &edit_store,
                2,
            )
            .is_some()
        );

        let edit = rumpel_world::chunk::WorldBlockEdit::from_single_chunk_index(
            rumpel_world::chunk::ChunkData::get_index(1, 10, 1),
            7,
        )
        .expect("valid edit index");
        assert!(edit_store.apply_edit(edit));

        assert!(
            reuse_generated_region_cache_entry(
                &mut cache,
                region_key,
                0,
                0,
                4,
                contract,
                &edit_store,
                3,
            )
            .is_none()
        );
    }

    #[test]
    fn test_update_prepared_metadata() {
        let mut generation = 1;
        let mut len = 10;
        update_prepared_metadata(&mut generation, &mut len, 5, 20);
        assert_eq!(generation, 5);
        assert_eq!(len, 20);
    }

    #[test]
    fn test_pack_unpack_chunk_key() {
        let cases = vec![(0, 0), (1, 2), (-1, -2), (1000, -500), (-32768, 32767)];
        for (x, z) in cases {
            let key = pack_chunk_key(x, z);
            let (ux, uz) = unpack_chunk_key(key);
            assert_eq!(x, ux);
            assert_eq!(z, uz);
        }
    }

    #[test]
    fn test_pending_chunks_sorting() {
        let mut pending = vec![
            PendingChunk {
                x: 0,
                z: 0,
                offset_x: 0,
                offset_z: 0,
                distance_sq: 0,
            },
            PendingChunk {
                x: 5,
                z: 5,
                offset_x: 5,
                offset_z: 5,
                distance_sq: 50,
            },
            PendingChunk {
                x: 2,
                z: 2,
                offset_x: 2,
                offset_z: 2,
                distance_sq: 8,
            },
        ];
        // Sort closest first (distance_sq smallest popped last, meaning distance_sq largest popped first)
        pending.sort_by_key(|chunk| std::cmp::Reverse(chunk.distance_sq));

        // Let's assert: pop gets the closest one (distance_sq = 0)
        assert_eq!(pending.pop().unwrap().distance_sq, 0);
        assert_eq!(pending.pop().unwrap().distance_sq, 8);
        assert_eq!(pending.pop().unwrap().distance_sq, 50);
    }

    #[test]
    fn test_pack_arena_allocations_sorting() {
        let quad1 = PackedVoxelQuad::new([1, 1, 1], [1, 1], 1, 0, 0, 0);
        let quad2 = PackedVoxelQuad::new([2, 2, 2], [2, 2], 2, 1, 0, 0);
        let quad3 = PackedVoxelQuad::new([3, 3, 3], [3, 3], 3, 2, 0, 0);

        let batches = vec![
            PackedQuadBatch {
                key: 100,
                quads: test_quads(vec![quad1, quad2]),
                chunk_ranges: Arc::new(Vec::new()),
                dirty_ranges: Arc::new(Vec::new()),
                generation: 1,
                needs_compaction: false,
            },
            PackedQuadBatch {
                key: 50,
                quads: test_quads(vec![quad3]),
                chunk_ranges: Arc::new(Vec::new()),
                dirty_ranges: Arc::new(Vec::new()),
                generation: 2,
                needs_compaction: false,
            },
        ];

        let (staging, allocations, total_quads) = pack_arena_allocations(&batches);

        assert_eq!(total_quads, 3);
        assert_eq!(staging.len(), 3);
        // Sorted by key: 50 first, then 100
        assert_eq!(staging[0], quad3);
        assert_eq!(staging[1], quad1);
        assert_eq!(staging[2], quad2);

        let alloc50 = allocations.get(&50).unwrap();
        assert_eq!(alloc50.offset_quads, 0);
        assert_eq!(alloc50.len_quads, 1);

        let alloc100 = allocations.get(&100).unwrap();
        assert_eq!(alloc100.offset_quads, 1);
        assert_eq!(alloc100.len_quads, 2);
    }

    #[test]
    fn test_arena_growth_from_zero() {
        assert_eq!(next_packed_quad_capacity(0, 0), 0);
        assert_eq!(next_packed_quad_capacity(0, 1), 16);
        assert_eq!(next_packed_quad_capacity(0, 20), 32);
    }

    #[test]
    fn test_stable_offsets_for_sorted_input() {
        let quad = PackedVoxelQuad::new([1, 1, 1], [1, 1], 1, 0, 0, 0);
        let batches = vec![
            PackedQuadBatch {
                key: 2,
                quads: test_quads(vec![quad]),
                chunk_ranges: Arc::new(Vec::new()),
                dirty_ranges: Arc::new(Vec::new()),
                generation: 1,
                needs_compaction: false,
            },
            PackedQuadBatch {
                key: 1,
                quads: test_quads(vec![quad]),
                chunk_ranges: Arc::new(Vec::new()),
                dirty_ranges: Arc::new(Vec::new()),
                generation: 1,
                needs_compaction: false,
            },
        ];

        let (_, allocs1, _) = pack_arena_allocations(&batches);
        let (_, allocs2, _) = pack_arena_allocations(&batches);

        assert_eq!(allocs1, allocs2);
    }

    #[test]
    fn test_arena_compaction_replans_freed_slots_before_growth() {
        let quad = PackedVoxelQuad::new([1, 1, 1], [1, 1], 1, 0, 0, 0);
        let mut existing_allocations = HashMap::new();
        existing_allocations.insert(
            1,
            crate::packed_quad_buffer::PackedQuadArenaAllocation {
                key: 1,
                offset_quads: 0,
                len_quads: 1,
                capacity_quads: 512,
                generation: 1,
            },
        );
        existing_allocations.insert(
            2,
            crate::packed_quad_buffer::PackedQuadArenaAllocation {
                key: 2,
                offset_quads: 512,
                len_quads: 1,
                capacity_quads: 512,
                generation: 1,
            },
        );
        let active_batches = vec![
            PackedQuadBatch {
                key: 2,
                quads: test_quads(vec![quad]),
                chunk_ranges: Arc::new(Vec::new()),
                dirty_ranges: Arc::new(Vec::new()),
                generation: 1,
                needs_compaction: false,
            },
            PackedQuadBatch {
                key: 3,
                quads: test_quads(vec![quad]),
                chunk_ranges: Arc::new(Vec::new()),
                dirty_ranges: Arc::new(Vec::new()),
                generation: 1,
                needs_compaction: false,
            },
        ];

        let (_, _, _, fragmented_next_free) =
            plan_stable_arena_allocations(&existing_allocations, &active_batches, 1024, 512);
        assert_eq!(fragmented_next_free, 1536);
        assert!(should_compact_packed_arena_slots(
            1024,
            fragmented_next_free
        ));

        let (compacted_allocations, dirty_keys, total_required, compacted_next_free) =
            compacted_packed_arena_allocation_plan(&active_batches, 512);
        assert_eq!(total_required, 2);
        assert_eq!(compacted_next_free, 1024);
        assert_eq!(dirty_keys, vec![2, 3]);
        assert_eq!(compacted_allocations[&2].offset_quads, 0);
        assert_eq!(compacted_allocations[&3].offset_quads, 512);
    }

    #[test]
    fn test_empty_batch_list() {
        let batches: Vec<PackedQuadBatch> = vec![];
        let (staging, allocations, total_quads) = pack_arena_allocations(&batches);
        assert_eq!(total_quads, 0);
        assert!(staging.is_empty());
        assert!(allocations.is_empty());
    }

    #[test]
    fn test_total_vertex_count_with_offsets() {
        let batches = vec![PackedQuadBatch {
            key: 1,
            quads: test_quads(vec![PackedVoxelQuad::new([0, 0, 0], [1, 1], 1, 0, 0, 0)]),
            chunk_ranges: Arc::new(Vec::new()),
            dirty_ranges: Arc::new(Vec::new()),
            generation: 1,
            needs_compaction: false,
        }];
        let (_, allocations, total) = pack_arena_allocations(&batches);
        assert_eq!(total, 1);
        let alloc = allocations.get(&1).unwrap();
        let vertex_count = crate::packed_quad_renderer::vertex_count_for_quads(alloc.len_quads);
        assert_eq!(vertex_count, 6);
    }

    #[test]
    fn test_streaming_grid_radius_targets() {
        assert_eq!(DEFAULT_PACKED_VIEW_RADIUS, 16);
        assert_eq!(packed_chunk_count_for_radius(0), 1);
        assert_eq!(packed_chunk_count_for_radius(1), 5);
        assert_eq!(packed_chunk_count_for_radius(2), 13);
        assert_eq!(packed_chunk_count_for_radius(16), 797);
        assert_eq!(packed_chunk_count_for_radius(32), 3209);
    }

    #[test]
    fn test_packed_render_chunks_for_center_matches_radius() {
        let chunks = packed_render_chunks_for_center(IVec2::new(10, -4), 2);
        let keys = chunks
            .iter()
            .map(|chunk| pack_chunk_key(chunk.x, chunk.z))
            .collect::<HashSet<_>>();

        assert_eq!(chunks.len(), 13);
        assert_eq!(keys.len(), 13);
        assert!(keys.contains(&pack_chunk_key(10, -4)));
        assert!(keys.contains(&pack_chunk_key(12, -4)));
        assert!(!keys.contains(&pack_chunk_key(13, -4)));
    }

    #[test]
    fn test_packed_lod_respects_min_cell_size() {
        assert_eq!(packed_lod_step_for_distance_sq(0, 2), 2);
        assert_eq!(
            packed_lod_step_for_distance_sq(
                PACKED_MID_LOD_DISTANCE_CHUNKS * PACKED_MID_LOD_DISTANCE_CHUNKS,
                2
            ),
            2
        );
        assert_eq!(
            packed_lod_step_for_distance_sq(
                PACKED_LOW_LOD_DISTANCE_CHUNKS * PACKED_LOW_LOD_DISTANCE_CHUNKS,
                2
            ),
            4
        );
        assert_eq!(
            packed_lod_step_for_distance_sq(
                PACKED_FAR_LOD_DISTANCE_CHUNKS * PACKED_FAR_LOD_DISTANCE_CHUNKS,
                2
            ),
            8
        );
    }

    #[test]
    fn test_packed_seam_safe_lod_uses_neighbor_step_floor() {
        let pending = PendingChunk {
            x: PACKED_LOW_LOD_DISTANCE_CHUNKS,
            z: 0,
            offset_x: PACKED_LOW_LOD_DISTANCE_CHUNKS,
            offset_z: 0,
            distance_sq: PACKED_LOW_LOD_DISTANCE_CHUNKS * PACKED_LOW_LOD_DISTANCE_CHUNKS,
        };

        assert_eq!(packed_seam_safe_lod_step(pending, 2), 2);
    }

    #[test]
    fn test_estimated_packed_arena_quads_for_radius() {
        assert_eq!(packed_region_count_for_radius(0, 4), 1);
        assert_eq!(packed_region_count_for_radius(16, 4), 62);
        assert_eq!(packed_region_count_for_radius(32, 4), 222);
        assert_eq!(estimated_packed_region_capacity_quads(4), 10240);
        assert_eq!(
            estimated_packed_arena_quads_for_radius_and_region_size(0, 4),
            10240
        );
        assert_eq!(
            estimated_packed_arena_quads_for_radius_and_region_size(16, 4),
            634880
        );
        assert_eq!(next_packed_quad_capacity(0, 634880), 1048576);
        assert_eq!(
            estimated_packed_arena_quads_for_radius_and_region_size(32, 4),
            2273280
        );
        assert_eq!(next_packed_quad_capacity(0, 2273280), 4194304);
    }

    #[test]
    fn test_packed_region_origin_for_chunk() {
        assert_eq!(packed_region_origin_for_chunk(0, 0, 4), (0, 0));
        assert_eq!(packed_region_origin_for_chunk(3, 3, 4), (0, 0));
        assert_eq!(packed_region_origin_for_chunk(4, 4, 4), (4, 4));
        assert_eq!(packed_region_origin_for_chunk(-1, -1, 4), (-4, -4));
        assert_eq!(packed_region_origin_for_chunk(-4, -4, 4), (-4, -4));
        assert_eq!(packed_region_origin_for_chunk(-5, -5, 4), (-8, -8));
    }

    #[test]
    fn test_offset_quads_to_region() {
        let quad = PackedVoxelQuad::new([2, 7, 3], [4, 5], 9, 2, 1, 42);
        let offset = offset_quads_to_region(&[quad], 6, -2, 4, -4);
        assert_eq!(offset.len(), 1);
        assert_eq!(offset[0].origin, [66, 7, 67]);
        assert_eq!(offset[0].size, quad.size);
        assert_eq!(offset[0].block_id, quad.block_id);
        assert_eq!(offset[0].meta, quad.meta);
    }

    #[test]
    fn test_packed_region_world_bounds() {
        let (min, max) = packed_region_world_bounds(pack_chunk_key(4, -4), 4);
        assert_eq!(min, Vec3::new(128.0, 0.0, -128.0));
        assert_eq!(max, Vec3::new(256.0, PACKED_REGION_BOUNDS_MAX_Y, 0.0));
    }

    #[test]
    fn test_packed_gpu_generation_region_radius_for_view() {
        assert_eq!(packed_gpu_generation_region_radius_for_view(16, 4), 4);
        assert_eq!(packed_gpu_generation_region_radius_for_view(8, 4), 2);
        assert_eq!(packed_gpu_generation_region_radius_for_view(0, 4), 1);
        assert_eq!(default_packed_gpu_generation_region_radius(), 4);
    }

    #[test]
    fn test_generated_region_bounds_visible_uses_frustum() {
        let bounds_min = Vec3::new(-0.5, -0.5, 0.25);
        let bounds_max = Vec3::new(0.5, 0.5, 0.75);
        assert!(generated_region_bounds_visible(
            Vec3::new(0.0, 0.0, -2.0),
            Mat4::IDENTITY,
            bounds_min,
            bounds_max,
        ));
        assert!(!generated_region_bounds_visible(
            Vec3::new(0.0, 0.0, -2.0),
            Mat4::IDENTITY,
            Vec3::new(2.0, -0.5, 0.25),
            Vec3::new(3.0, 0.5, 0.75),
        ));
    }

    #[test]
    fn test_packed_memory_reservation_math() {
        assert_eq!(DEFAULT_PACKED_MIN_VRAM_BYTES, 0);
        assert_eq!(DEFAULT_PACKED_MIN_RAM_BYTES, 0);
        assert_eq!(estimated_packed_region_capacity_quads(4), 10_240);
        assert_eq!(
            estimated_packed_arena_quads_for_radius_and_region_size(16, 4),
            634_880
        );
    }

    #[test]
    fn test_packed_quad_face_ranges() {
        let quads = vec![
            PackedVoxelQuad::new([0, 0, 0], [1, 1], 1, 0, 0, 0),
            PackedVoxelQuad::new([0, 1, 0], [1, 1], 1, 0, 0, 0),
            PackedVoxelQuad::new([0, 0, 0], [1, 1], 1, 2, 0, 0),
            PackedVoxelQuad::new([1, 0, 0], [1, 1], 1, 2, 0, 0),
            PackedVoxelQuad::new([0, 0, 0], [1, 1], 1, 5, 0, 0),
        ];

        let ranges = packed_quad_face_ranges(&quads);

        assert_eq!(
            ranges,
            vec![
                PackedQuadFaceRange {
                    face: 0,
                    start_quads: 0,
                    len_quads: 2,
                },
                PackedQuadFaceRange {
                    face: 2,
                    start_quads: 2,
                    len_quads: 2,
                },
                PackedQuadFaceRange {
                    face: 5,
                    start_quads: 4,
                    len_quads: 1,
                },
            ]
        );
    }

    #[test]
    fn test_plan_stable_arena_allocations_reuses_offsets() {
        let quad = PackedVoxelQuad::new([1, 1, 1], [1, 1], 1, 0, 0, 0);
        let batches = vec![
            PackedQuadBatch {
                key: 10,
                quads: test_quads(vec![quad; 4]),
                chunk_ranges: Arc::new(Vec::new()),
                dirty_ranges: Arc::new(Vec::new()),
                generation: 1,
                needs_compaction: false,
            },
            PackedQuadBatch {
                key: 20,
                quads: test_quads(vec![quad; 8]),
                chunk_ranges: Arc::new(Vec::new()),
                dirty_ranges: Arc::new(Vec::new()),
                generation: 1,
                needs_compaction: false,
            },
        ];

        let (allocs, dirty, total_quads, next_free) =
            plan_stable_arena_allocations(&HashMap::new(), &batches, 0, 16);
        assert_eq!(dirty, vec![10, 20]);
        assert_eq!(total_quads, 12);
        assert_eq!(allocs[&10].offset_quads, 0);
        assert_eq!(allocs[&10].capacity_quads, 16);
        assert_eq!(allocs[&20].offset_quads, 16);
        assert_eq!(allocs[&20].capacity_quads, 16);
        assert_eq!(next_free, 32);

        let updated = vec![
            PackedQuadBatch {
                key: 10,
                quads: test_quads(vec![quad; 6]),
                chunk_ranges: Arc::new(Vec::new()),
                dirty_ranges: Arc::new(Vec::new()),
                generation: 2,
                needs_compaction: false,
            },
            PackedQuadBatch {
                key: 20,
                quads: test_quads(vec![quad; 8]),
                chunk_ranges: Arc::new(Vec::new()),
                dirty_ranges: Arc::new(Vec::new()),
                generation: 1,
                needs_compaction: false,
            },
        ];
        let (allocs2, dirty2, total_quads2, next_free2) =
            plan_stable_arena_allocations(&allocs, &updated, next_free, 16);
        assert_eq!(dirty2, vec![10]);
        assert_eq!(total_quads2, 14);
        assert_eq!(allocs2[&10].offset_quads, 0);
        assert_eq!(allocs2[&20].offset_quads, 16);
        assert_eq!(next_free2, 32);
    }

    #[test]
    fn test_rebuild_region_batch_uses_loaded_cache_chunks() {
        let region_key = pack_chunk_key(0, 0);
        let active_key = pack_chunk_key(0, 0);
        let cached_key = pack_chunk_key(1, 0);
        let active_quad = PackedVoxelQuad::new([1, 1, 1], [1, 1], 1, 0, 0, 0);
        let cached_quad = PackedVoxelQuad::new([2, 1, 1], [1, 1], 2, 0, 0, 0);
        let mut batches = PackedQuadBatches::default();
        let mut state = PackedQuadStreamingState::default();
        state.active_render_chunks.insert(active_key);
        state.loaded.insert(
            active_key,
            LoadedPackedQuadChunk {
                entity: Entity::PLACEHOLDER,
                region_key,
                quads: vec![active_quad],
            },
        );
        state.loaded.insert(
            cached_key,
            LoadedPackedQuadChunk {
                entity: Entity::PLACEHOLDER,
                region_key,
                quads: vec![cached_quad],
            },
        );

        rebuild_region_batch(&mut batches, &mut state, region_key);

        assert_eq!(batches.batches.len(), 1);
        assert_eq!(
            batches.batches[0].quads.as_slice(),
            &[active_quad, cached_quad]
        );
        assert_eq!(
            batches.batches[0].chunk_ranges.as_slice(),
            &[
                PackedQuadChunkRange {
                    chunk_key: active_key,
                    start_quads: 0,
                    len_quads: 1,
                    capacity_quads: 1,
                    active: true,
                    resident: true,
                },
                PackedQuadChunkRange {
                    chunk_key: cached_key,
                    start_quads: 1,
                    len_quads: 1,
                    capacity_quads: 1,
                    active: false,
                    resident: true,
                },
            ]
        );
    }

    #[test]
    fn test_region_rebuild_queue_deduplicates_regions() {
        let region_key = pack_chunk_key(0, 0);
        let mut state = PackedQuadStreamingState::default();

        queue_region_rebuild(&mut state, region_key);
        queue_region_rebuild(&mut state, region_key);

        assert_eq!(state.pending_rebuild_regions, vec![region_key]);
    }

    #[test]
    fn test_rebuild_queued_region_batches_respects_frame_budget() {
        let region_a = pack_chunk_key(0, 0);
        let region_b = pack_chunk_key(4, 0);
        let chunk_a = pack_chunk_key(0, 0);
        let chunk_b = pack_chunk_key(4, 0);
        let quad_a = PackedVoxelQuad::new([1, 1, 1], [1, 1], 1, 0, 0, 0);
        let quad_b = PackedVoxelQuad::new([2, 1, 1], [1, 1], 2, 0, 0, 0);
        let mut batches = PackedQuadBatches::default();
        let mut state = PackedQuadStreamingState::default();
        state.active_render_chunks.extend([chunk_a, chunk_b]);
        state.loaded.insert(
            chunk_a,
            LoadedPackedQuadChunk {
                entity: Entity::PLACEHOLDER,
                region_key: region_a,
                quads: vec![quad_a],
            },
        );
        state.loaded.insert(
            chunk_b,
            LoadedPackedQuadChunk {
                entity: Entity::PLACEHOLDER,
                region_key: region_b,
                quads: vec![quad_b],
            },
        );
        queue_region_rebuild(&mut state, region_a);
        queue_region_rebuild(&mut state, region_b);

        assert_eq!(
            rebuild_queued_region_batches(&mut batches, &mut state, 1),
            1
        );
        assert_eq!(state.pending_rebuild_regions.len(), 1);
        assert_eq!(batches.batches.len(), 1);

        assert_eq!(
            rebuild_queued_region_batches(&mut batches, &mut state, 1),
            1
        );
        assert!(state.pending_rebuild_regions.is_empty());
        assert_eq!(batches.batches.len(), 2);
    }

    #[test]
    fn test_append_chunk_to_region_batch_extends_without_rebuild() {
        let region_key = pack_chunk_key(4, -4);
        let chunk_a = pack_chunk_key(4, -4);
        let chunk_b = pack_chunk_key(5, -4);
        let quad_a = PackedVoxelQuad::new([1, 1, 1], [1, 1], 1, 0, 0, 0);
        let quad_b = PackedVoxelQuad::new([2, 2, 2], [1, 1], 2, 1, 0, 0);
        let mut batches = PackedQuadBatches::default();
        let mut state = PackedQuadStreamingState::default();
        state.active_render_chunks.insert(chunk_a);

        append_chunk_to_region_batch_with_capacity_and_mode(
            &mut batches,
            &mut state,
            chunk_a,
            region_key,
            &[quad_a],
            16,
            false,
        );
        assert_eq!(batches.batches.len(), 1);
        assert_eq!(batches.batches[0].key, region_key);
        assert_eq!(batches.batches[0].generation, 1);
        assert_eq!(batches.batches[0].quads.as_slice(), &[quad_a]);
        assert_eq!(batches.batches[0].quads.capacity(), 16);
        assert_eq!(
            batches.batches[0].chunk_ranges.as_slice(),
            &[PackedQuadChunkRange {
                chunk_key: chunk_a,
                start_quads: 0,
                len_quads: 1,
                capacity_quads: 1,
                active: true,
                resident: true,
            }]
        );

        append_chunk_to_region_batch_with_capacity_and_mode(
            &mut batches,
            &mut state,
            chunk_b,
            region_key,
            &[quad_b],
            16,
            false,
        );
        assert_eq!(batches.batches.len(), 1);
        assert_eq!(batches.batches[0].generation, 2);
        assert_eq!(batches.batches[0].quads.as_slice(), &[quad_a, quad_b]);
        assert!(!batches.batches[0].needs_compaction);
        assert!(state.pending_compaction_regions.is_empty());
        assert_eq!(
            batches.batches[0].chunk_ranges.as_slice(),
            &[
                PackedQuadChunkRange {
                    chunk_key: chunk_a,
                    start_quads: 0,
                    len_quads: 1,
                    capacity_quads: 1,
                    active: true,
                    resident: true,
                },
                PackedQuadChunkRange {
                    chunk_key: chunk_b,
                    start_quads: 1,
                    len_quads: 1,
                    capacity_quads: 1,
                    active: false,
                    resident: true,
                },
            ]
        );

        let generation_before_sync = batches.batches[0].generation;
        state.active_render_chunks.remove(&chunk_a);
        state.active_render_chunks.insert(chunk_b);
        sync_region_batch_active_flags(&mut batches, &state, region_key);
        assert_eq!(batches.batches[0].generation, generation_before_sync);
        assert!(!batches.batches[0].chunk_ranges[0].active);
        assert!(batches.batches[0].chunk_ranges[1].active);
    }

    #[test]
    fn test_evict_chunk_tombstones_range_without_generation_bump() {
        let region_key = pack_chunk_key(0, 0);
        let chunk_a = pack_chunk_key(0, 0);
        let chunk_b = pack_chunk_key(1, 0);
        let quad_a = PackedVoxelQuad::new([1, 1, 1], [1, 1], 1, 0, 0, 0);
        let quad_b = PackedVoxelQuad::new([2, 1, 1], [1, 1], 2, 0, 0, 0);
        let mut batches = PackedQuadBatches::default();
        let mut state = PackedQuadStreamingState::default();
        state.active_render_chunks.extend([chunk_a, chunk_b]);

        append_chunk_to_region_batch_with_capacity_and_mode(
            &mut batches,
            &mut state,
            chunk_a,
            region_key,
            &[quad_a],
            16,
            false,
        );
        append_chunk_to_region_batch_with_capacity_and_mode(
            &mut batches,
            &mut state,
            chunk_b,
            region_key,
            &[quad_b],
            16,
            false,
        );
        queue_region_rebuild(&mut state, region_key);
        mark_region_for_deferred_compaction(&mut state, region_key);

        let generation_before_evict = batches.batches[0].generation;
        assert!(evict_chunk_from_region_batch(
            &mut batches,
            &mut state,
            chunk_a,
            region_key
        ));

        assert_eq!(batches.batches.len(), 1);
        assert_eq!(batches.batches[0].generation, generation_before_evict);
        assert_eq!(batches.batches[0].quads.as_slice(), &[quad_a, quad_b]);
        assert!(state.pending_rebuild_regions.is_empty());
        assert!(state.pending_compaction_regions.is_empty());
        assert_eq!(
            batches.batches[0].chunk_ranges.as_slice(),
            &[
                PackedQuadChunkRange {
                    chunk_key: chunk_a,
                    start_quads: 0,
                    len_quads: 0,
                    capacity_quads: 1,
                    active: false,
                    resident: false,
                },
                PackedQuadChunkRange {
                    chunk_key: chunk_b,
                    start_quads: 1,
                    len_quads: 1,
                    capacity_quads: 1,
                    active: true,
                    resident: true,
                },
            ]
        );
    }

    #[test]
    fn test_evict_last_chunk_removes_empty_region_batch() {
        let region_key = pack_chunk_key(0, 0);
        let chunk_key = pack_chunk_key(0, 0);
        let quad = PackedVoxelQuad::new([1, 1, 1], [1, 1], 1, 0, 0, 0);
        let mut batches = PackedQuadBatches::default();
        let mut state = PackedQuadStreamingState::default();

        append_chunk_to_region_batch_with_capacity_and_mode(
            &mut batches,
            &mut state,
            chunk_key,
            region_key,
            &[quad],
            16,
            false,
        );
        assert!(state.region_generations.contains_key(&region_key));

        assert!(evict_chunk_from_region_batch(
            &mut batches,
            &mut state,
            chunk_key,
            region_key
        ));

        assert!(batches.batches.is_empty());
        assert!(!state.region_generations.contains_key(&region_key));
    }

    #[test]
    fn test_append_reuses_tombstone_slot_as_dirty_subrange() {
        let region_key = pack_chunk_key(0, 0);
        let old_chunk = pack_chunk_key(0, 0);
        let live_chunk = pack_chunk_key(1, 0);
        let reused_chunk = pack_chunk_key(2, 0);
        let quad_a = PackedVoxelQuad::new([1, 1, 1], [1, 1], 1, 0, 0, 0);
        let quad_b = PackedVoxelQuad::new([2, 1, 1], [1, 1], 2, 0, 0, 0);
        let quad_c = PackedVoxelQuad::new([3, 1, 1], [1, 1], 3, 0, 0, 0);
        let replacement_quad = PackedVoxelQuad::new([4, 1, 1], [1, 1], 4, 0, 0, 0);
        let mut batches = PackedQuadBatches::default();
        let mut state = PackedQuadStreamingState::default();

        append_chunk_to_region_batch_with_capacity_and_mode(
            &mut batches,
            &mut state,
            old_chunk,
            region_key,
            &[quad_a, quad_b],
            16,
            false,
        );
        append_chunk_to_region_batch_with_capacity_and_mode(
            &mut batches,
            &mut state,
            live_chunk,
            region_key,
            &[quad_c],
            16,
            false,
        );
        assert!(evict_chunk_from_region_batch(
            &mut batches,
            &mut state,
            old_chunk,
            region_key
        ));

        let len_before_reuse = batches.batches[0].quads.len();
        append_chunk_to_region_batch_with_capacity_and_mode(
            &mut batches,
            &mut state,
            reused_chunk,
            region_key,
            &[replacement_quad],
            16,
            false,
        );

        assert_eq!(batches.batches[0].quads.len(), len_before_reuse);
        assert_eq!(batches.batches[0].quads[0], replacement_quad);
        assert_eq!(batches.batches[0].quads[1], quad_b);
        assert_eq!(
            batches.batches[0].chunk_ranges[0],
            PackedQuadChunkRange {
                chunk_key: reused_chunk,
                start_quads: 0,
                len_quads: 1,
                capacity_quads: 2,
                active: false,
                resident: true,
            }
        );
        assert_eq!(
            batches.batches[0].dirty_ranges.as_slice(),
            &[PackedQuadDirtyRange {
                start_quads: 0,
                len_quads: 1,
                generation: 3,
            }]
        );

        let mut existing_allocations = HashMap::new();
        existing_allocations.insert(
            region_key,
            crate::packed_quad_buffer::PackedQuadArenaAllocation {
                key: region_key,
                offset_quads: 0,
                len_quads: len_before_reuse,
                capacity_quads: 16,
                generation: 2,
            },
        );
        let (_, dirty_keys, _, _) =
            plan_stable_arena_allocations(&existing_allocations, &batches.batches, 16, 16);
        assert!(dirty_keys.is_empty());
    }

    #[test]
    fn test_prune_dirty_ranges_keeps_unconfirmed_generations() {
        let mut batch = PackedQuadBatch {
            key: 1,
            quads: Arc::new(vec![PackedVoxelQuad::new([0, 0, 0], [1, 1], 1, 0, 0, 0)]),
            chunk_ranges: Arc::new(Vec::new()),
            dirty_ranges: Arc::new(vec![
                PackedQuadDirtyRange {
                    start_quads: 0,
                    len_quads: 1,
                    generation: 2,
                },
                PackedQuadDirtyRange {
                    start_quads: 0,
                    len_quads: 1,
                    generation: 3,
                },
                PackedQuadDirtyRange {
                    start_quads: 0,
                    len_quads: 1,
                    generation: 5,
                },
            ]),
            generation: 5,
            needs_compaction: false,
        };

        assert_eq!(prune_dirty_ranges_confirmed_through(&mut batch, 3), 2);
        assert_eq!(
            batch.dirty_ranges.as_slice(),
            &[PackedQuadDirtyRange {
                start_quads: 0,
                len_quads: 1,
                generation: 5,
            }]
        );
    }

    #[test]
    fn test_deferred_region_compaction_preserves_chunk_ranges() {
        let region_key = pack_chunk_key(0, 0);
        let chunk_a = pack_chunk_key(0, 0);
        let chunk_b = pack_chunk_key(1, 0);
        let quad_a =
            PackedVoxelQuad::new([0, 10, 0], [32, 32], 3, PackedVoxelFace::PlusY as u8, 0, 0);
        let quad_b =
            PackedVoxelQuad::new([32, 10, 0], [32, 32], 3, PackedVoxelFace::PlusY as u8, 0, 0);
        let mut batches = PackedQuadBatches::default();
        let mut state = PackedQuadStreamingState::default();

        append_chunk_to_region_batch_with_capacity_and_mode(
            &mut batches,
            &mut state,
            chunk_a,
            region_key,
            &[quad_a],
            16,
            true,
        );
        append_chunk_to_region_batch_with_capacity_and_mode(
            &mut batches,
            &mut state,
            chunk_b,
            region_key,
            &[quad_b],
            16,
            true,
        );

        assert_eq!(batches.batches[0].quads.len(), 2);
        assert!(batches.batches[0].needs_compaction);
        assert_eq!(state.pending_compaction_regions, vec![region_key]);
        assert!(compact_deferred_packed_region_batch(
            &mut batches,
            &mut state,
            region_key
        ));
        assert_eq!(batches.batches[0].quads.len(), 2);
        assert!(!batches.batches[0].needs_compaction);
        assert_eq!(
            batches.batches[0].chunk_ranges.as_slice(),
            &[
                PackedQuadChunkRange {
                    chunk_key: chunk_a,
                    start_quads: 0,
                    len_quads: 1,
                    capacity_quads: 1,
                    active: false,
                    resident: true,
                },
                PackedQuadChunkRange {
                    chunk_key: chunk_b,
                    start_quads: 1,
                    len_quads: 1,
                    capacity_quads: 1,
                    active: false,
                    resident: true,
                },
            ]
        );
    }

    #[test]
    fn test_region_compaction_compacts_inside_chunk_ranges_only() {
        let region_key = pack_chunk_key(0, 0);
        let chunk_a = pack_chunk_key(0, 0);
        let chunk_b = pack_chunk_key(1, 0);
        let quad_a =
            PackedVoxelQuad::new([0, 10, 0], [16, 32], 3, PackedVoxelFace::PlusY as u8, 0, 0);
        let quad_b =
            PackedVoxelQuad::new([16, 10, 0], [16, 32], 3, PackedVoxelFace::PlusY as u8, 0, 0);
        let quad_c =
            PackedVoxelQuad::new([32, 10, 0], [16, 32], 3, PackedVoxelFace::PlusY as u8, 0, 0);
        let mut batch = PackedQuadBatch {
            key: region_key,
            quads: test_quads(vec![quad_a, quad_b, quad_c]),
            chunk_ranges: Arc::new(vec![
                PackedQuadChunkRange {
                    chunk_key: chunk_a,
                    start_quads: 0,
                    len_quads: 2,
                    capacity_quads: 2,
                    active: true,
                    resident: true,
                },
                PackedQuadChunkRange {
                    chunk_key: chunk_b,
                    start_quads: 2,
                    len_quads: 1,
                    capacity_quads: 1,
                    active: false,
                    resident: true,
                },
            ]),
            dirty_ranges: Arc::new(Vec::new()),
            generation: 1,
            needs_compaction: true,
        };

        assert!(compact_region_batch_preserving_chunk_ranges(&mut batch));

        assert_eq!(batch.quads.len(), 2);
        assert_eq!(batch.quads[0].origin, [0, 10, 0]);
        assert_eq!(batch.quads[0].size, [32, 32]);
        assert_eq!(batch.quads[1], quad_c);
        assert_eq!(
            batch.chunk_ranges.as_slice(),
            &[
                PackedQuadChunkRange {
                    chunk_key: chunk_a,
                    start_quads: 0,
                    len_quads: 1,
                    capacity_quads: 1,
                    active: true,
                    resident: true,
                },
                PackedQuadChunkRange {
                    chunk_key: chunk_b,
                    start_quads: 1,
                    len_quads: 1,
                    capacity_quads: 1,
                    active: false,
                    resident: true,
                },
            ]
        );
    }

    #[test]
    fn test_active_chunk_ranges_emit_only_active_indirect_commands() {
        let region_key = pack_chunk_key(0, 0);
        let chunk_a = pack_chunk_key(0, 0);
        let chunk_b = pack_chunk_key(1, 0);
        let quad = PackedVoxelQuad::new([0, 0, 0], [1, 1], 1, 0, 0, 0);
        let batch = PackedQuadBatch {
            key: region_key,
            quads: test_quads(vec![quad, quad, quad]),
            chunk_ranges: Arc::new(vec![
                PackedQuadChunkRange {
                    chunk_key: chunk_a,
                    start_quads: 0,
                    len_quads: 1,
                    capacity_quads: 1,
                    active: false,
                    resident: true,
                },
                PackedQuadChunkRange {
                    chunk_key: chunk_b,
                    start_quads: 1,
                    len_quads: 2,
                    capacity_quads: 2,
                    active: true,
                    resident: true,
                },
            ]),
            dirty_ranges: Arc::new(Vec::new()),
            generation: 1,
            needs_compaction: false,
        };
        let mut commands_staging = Vec::new();
        let mut params_staging = Vec::new();
        let mut command_metadata = Vec::new();
        let translation = Vec4::new(0.0, 0.0, 0.0, 32.0);

        for range in batch.chunk_ranges.iter().copied() {
            push_packed_chunk_range_indirect_commands(
                &mut commands_staging,
                &mut params_staging,
                &mut command_metadata,
                PackedChunkRangeIndirectInput {
                    batch: &batch,
                    range,
                    allocation_len_quads: batch.quads.len(),
                    translation,
                    face_range_cull_enabled: true,
                    face_range_min_quads: 4096,
                },
            );
        }

        let (chunk_b_min, chunk_b_max) = packed_chunk_world_bounds(chunk_b);
        assert_eq!(commands_staging.len(), 1);
        assert_eq!(params_staging.len(), 1);
        assert_eq!(command_metadata.len(), 1);
        assert_eq!(commands_staging[0].first_vertex, 6);
        assert_eq!(commands_staging[0].vertex_count, 12);
        assert_eq!(command_metadata[0].batch_key, region_key);
        assert_eq!(command_metadata[0].len_quads, 2);
        assert_eq!(command_metadata[0].bounds_min, chunk_b_min);
        assert_eq!(command_metadata[0].bounds_max, chunk_b_max);
    }

    #[test]
    fn test_draw_command_abi() {
        use crate::packed_quad_buffer::PackedQuadDrawCommand;
        assert_eq!(std::mem::size_of::<PackedQuadDrawCommand>(), 16);
        assert_eq!(std::mem::align_of::<PackedQuadDrawCommand>(), 4);

        let commands = vec![PackedQuadDrawCommand {
            vertex_count: 60,
            instance_count: 1,
            first_vertex: 0,
            first_instance: 4,
        }];
        let bytes = bytemuck::cast_slice::<PackedQuadDrawCommand, u8>(&commands);
        assert_eq!(bytes.len(), 16);

        let roundtrip = bytemuck::cast_slice::<u8, PackedQuadDrawCommand>(bytes);
        assert_eq!(commands, roundtrip);
    }

    #[test]
    fn test_draw_params_abi() {
        use crate::packed_quad_buffer::PackedQuadDrawParams;
        assert_eq!(std::mem::size_of::<PackedQuadDrawParams>(), 16);
        assert_eq!(std::mem::align_of::<PackedQuadDrawParams>(), 4);

        let params = vec![PackedQuadDrawParams {
            chunk_offset: [1.0, 2.0, 3.0, 4.0],
        }];
        let bytes = bytemuck::cast_slice::<PackedQuadDrawParams, u8>(&params);
        assert_eq!(bytes.len(), 16);

        let roundtrip = bytemuck::cast_slice::<u8, PackedQuadDrawParams>(bytes);
        assert_eq!(params, roundtrip);
    }

    #[test]
    fn test_indirect_buffer_packing() {
        let quad = PackedVoxelQuad::new([0, 0, 0], [1, 1], 1, 0, 0, 0);
        let batches = vec![
            PackedQuadBatch {
                key: 200,
                quads: test_quads(vec![quad, quad]),
                chunk_ranges: Arc::new(Vec::new()),
                dirty_ranges: Arc::new(Vec::new()),
                generation: 1,
                needs_compaction: false,
            },
            PackedQuadBatch {
                key: 100,
                quads: test_quads(vec![quad]),
                chunk_ranges: Arc::new(Vec::new()),
                dirty_ranges: Arc::new(Vec::new()),
                generation: 1,
                needs_compaction: false,
            },
        ];

        // 1. Contiguous allocations
        let (_, allocations, _) = pack_arena_allocations(&batches);

        // 2. Sort batches deterministically
        let mut sorted_batches = batches.clone();
        sorted_batches.sort_by_key(|b| b.key);

        let mut commands_staging = Vec::new();
        let mut params_staging = Vec::new();
        let mut command_metadata = Vec::new();

        for batch in &sorted_batches {
            let alloc = allocations.get(&batch.key).copied().unwrap();
            let (tx, tz) = unpack_chunk_key(batch.key);
            let translation = Vec4::new(
                (tx * 32) as f32,
                0.0,
                (tz * 32) as f32,
                alloc.offset_quads as f32,
            );

            push_packed_indirect_command(
                &mut commands_staging,
                &mut params_staging,
                &mut command_metadata,
                PackedIndirectCommandInput {
                    translation,
                    batch_key: batch.key,
                    bounds_min: Vec3::ZERO,
                    bounds_max: Vec3::splat(32.0),
                    start_quads: 0,
                    len_quads: alloc.len_quads,
                    face: None,
                },
            );
        }

        assert_eq!(commands_staging.len(), 2);
        assert_eq!(params_staging.len(), 2);
        assert_eq!(command_metadata.len(), 2);

        // Batch 100 is sorted first
        assert_eq!(commands_staging[0].first_instance, 0);
        assert_eq!(commands_staging[0].vertex_count, 6); // 1 quad * 6 vertices

        // Batch 200 is sorted second
        assert_eq!(commands_staging[1].first_instance, 1);
        assert_eq!(commands_staging[1].vertex_count, 12); // 2 quads * 6 vertices
    }

    #[test]
    fn test_indirect_face_range_command_offsets() {
        let mut commands_staging = Vec::new();
        let mut params_staging = Vec::new();
        let mut command_metadata = Vec::new();
        let translation = Vec4::new(32.0, 0.0, 64.0, 10.0);

        push_packed_indirect_command(
            &mut commands_staging,
            &mut params_staging,
            &mut command_metadata,
            PackedIndirectCommandInput {
                translation,
                batch_key: 42,
                bounds_min: Vec3::ZERO,
                bounds_max: Vec3::splat(32.0),
                start_quads: 3,
                len_quads: 5,
                face: Some(PackedVoxelFace::PlusX as u8),
            },
        );
        push_packed_indirect_command(
            &mut commands_staging,
            &mut params_staging,
            &mut command_metadata,
            PackedIndirectCommandInput {
                translation,
                batch_key: 42,
                bounds_min: Vec3::ZERO,
                bounds_max: Vec3::splat(32.0),
                start_quads: 8,
                len_quads: 2,
                face: Some(PackedVoxelFace::MinusX as u8),
            },
        );

        assert_eq!(commands_staging.len(), 2);
        assert_eq!(params_staging.len(), 2);
        assert_eq!(commands_staging[0].first_vertex, 18);
        assert_eq!(commands_staging[0].vertex_count, 30);
        assert_eq!(commands_staging[0].first_instance, 0);
        assert_eq!(commands_staging[1].first_vertex, 48);
        assert_eq!(commands_staging[1].vertex_count, 12);
        assert_eq!(commands_staging[1].first_instance, 1);
        assert_eq!(command_metadata[0].face, Some(PackedVoxelFace::PlusX as u8));
        assert_eq!(command_metadata[1].len_quads, 2);
    }

    #[test]
    fn test_gpu_cull_metadata_from_indirect_command() {
        let metadata = packed_gpu_cull_metadata_from_command(PackedQuadIndirectCommandMetadata {
            batch_key: 42,
            face: Some(PackedVoxelFace::MinusZ as u8),
            len_quads: 123,
            bounds_min: Vec3::new(1.0, 2.0, 3.0),
            bounds_max: Vec3::new(4.0, 5.0, 6.0),
        });

        assert_eq!(metadata.bounds_min, [1.0, 2.0, 3.0, 0.0]);
        assert_eq!(metadata.bounds_max, [4.0, 5.0, 6.0, 0.0]);
        assert_eq!(
            metadata.meta,
            [123, u32::from(PackedVoxelFace::MinusZ as u8) + 1, 0, 0]
        );
    }

    #[test]
    fn test_should_split_packed_face_ranges_respects_threshold() {
        let ranges = vec![
            PackedQuadFaceRange {
                face: PackedVoxelFace::PlusX as u8,
                start_quads: 0,
                len_quads: 2000,
            },
            PackedQuadFaceRange {
                face: PackedVoxelFace::MinusX as u8,
                start_quads: 2000,
                len_quads: 2500,
            },
        ];

        assert!(!should_split_packed_face_ranges(true, 3999, &ranges, 4096));
        assert!(should_split_packed_face_ranges(true, 4096, &ranges, 4096));
        assert!(!should_split_packed_face_ranges(false, 4096, &ranges, 4096));
        assert!(!should_split_packed_face_ranges(
            true,
            4096,
            &ranges[..1],
            4096
        ));
    }

    #[test]
    fn test_adaptive_packed_streaming_budget() {
        let target = 1.0 / 60.0;

        assert_eq!(adaptive_packed_streaming_budget(4, 0.0, target, true), 4);
        assert_eq!(adaptive_packed_streaming_budget(4, target, target, true), 4);
        assert_eq!(
            adaptive_packed_streaming_budget(4, target * 1.5, target, true),
            2
        );
        assert_eq!(
            adaptive_packed_streaming_budget(4, target * 2.5, target, true),
            1
        );
        assert_eq!(
            adaptive_packed_streaming_budget(4, target * 2.5, target, false),
            4
        );
    }

    #[test]
    fn test_adaptive_packed_background_budget() {
        let target = 1.0 / 60.0;

        assert_eq!(
            adaptive_packed_background_budget(1, target, target, true),
            1
        );
        assert_eq!(
            adaptive_packed_background_budget(1, target * 1.5, target, true),
            0
        );
        assert_eq!(
            adaptive_packed_background_budget(1, target * 1.5, target, false),
            1
        );
    }
}
