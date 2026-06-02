use bevy::{
    image::BevyDefault,
    prelude::*,
    render::{
        Render, RenderApp, RenderSystems,
        render_asset::RenderAssets,
        render_graph::{self, RenderGraph, RenderLabel},
        render_resource::{
            binding_types::{
                sampler, storage_buffer_read_only_sized, storage_buffer_sized, texture_2d_array,
            },
            *,
        },
        renderer::{RenderContext, RenderDevice, RenderQueue},
        texture::GpuImage,
        view::{ExtractedView, ViewDepthTexture, ViewTarget},
    },
};
use bevy_asset::{embedded_asset, load_embedded_asset};
use std::{
    collections::{HashMap, HashSet},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Instant,
};
use wgpu::{QuerySet, QuerySetDescriptor, QueryType, RenderPassTimestampWrites};

use crate::packed_quad_gpu_generation::{
    PackedGpuGenerationBatch, PackedGpuGenerationBatches, PackedGpuGenerationJob,
    packed_gpu_generation_enabled_from_env, packed_gpu_generation_workgroups,
};
use crate::packed_quad_pipeline::{PreparedPackedQuadBatch, PreparedPackedQuadBatches};

const PACKED_FACE_DEBUG_ENV: &str = "RUMPEL_PACKED_FACE_DEBUG";
const PACKED_FACE_RANGE_CULL_ENV: &str = "RUMPEL_PACKED_FACE_RANGE_CULL";
const PACKED_FOG_END_ENV: &str = "RUMPEL_PACKED_FOG_END";
const PACKED_FOG_START_ENV: &str = "RUMPEL_PACKED_FOG_START";
const PACKED_GPU_TIMESTAMPS_ENV: &str = "RUMPEL_PACKED_GPU_TIMESTAMPS";
const PACKED_CPU_VISIBLE_COMPACT_ENV: &str = "RUMPEL_PACKED_CPU_VISIBLE_COMPACT";
const PACKED_GPU_CULL_ENV: &str = "RUMPEL_PACKED_GPU_CULL";
const DEFAULT_PACKED_FACE_RANGE_CULL: bool = true;
const DEFAULT_PACKED_FOG_END: f32 = 360.0;
const DEFAULT_PACKED_FOG_START: f32 = 160.0;
const PACKED_GPU_CULL_WORKGROUP_SIZE: usize = 64;
const PACKED_GPU_TIMESTAMP_QUERY_COUNT: u32 = 2;
const PACKED_GPU_TIMESTAMP_BUFFER_SIZE: u64 = 16;
const PACKED_VIEW_BUFFER_SIZE: u64 = std::mem::size_of::<PackedQuadViewBuffer>() as u64;
const PACKED_GPU_GENERATED_CULL_SIGNATURE_OFFSET: u64 = 14_695_981_039_346_656_037;
const PACKED_GPU_GENERATED_CULL_SIGNATURE_PRIME: u64 = 1_099_511_628_211;

static RENDERER_LOGGED: AtomicBool = AtomicBool::new(false);

/// GPU view data consumed as six vec4 lanes by the WGSL storage buffer.
#[derive(Clone, Copy)]
#[repr(C)]
struct PackedQuadViewBuffer {
    view_projection_columns: [[f32; 4]; 4],
    camera_position_and_fog_start: [f32; 4],
    fog_color_and_end: [f32; 4],
}

unsafe impl bytemuck::Zeroable for PackedQuadViewBuffer {}
unsafe impl bytemuck::Pod for PackedQuadViewBuffer {}

/// Label representing our custom PackedVoxelQuad rendering pass in the Render Graph.
#[derive(Clone, Eq, PartialEq, Hash, Debug, RenderLabel)]
pub struct PackedQuadRenderLabel;

/// Label representing the opt-in GPU indirect command culling pass.
#[derive(Clone, Eq, PartialEq, Hash, Debug, RenderLabel)]
pub struct PackedQuadCullLabel;

/// Label representing the opt-in GPU PackedVoxelQuad generation pass.
#[derive(Clone, Eq, PartialEq, Hash, Debug, RenderLabel)]
pub struct PackedQuadGenerationLabel;

/// Render World component storing the camera view-projection uniform buffer and bind group.
#[derive(Component)]
pub struct PackedQuadViewUniform {
    pub buffer: Buffer,
    pub bind_group: BindGroup,
}

/// Resource in the Render World storing the custom pipeline and its bind group layouts.
#[derive(Resource)]
pub struct PackedQuadPipeline {
    pub pipeline_id: CachedRenderPipelineId,
    pub view_bind_group_layout: BindGroupLayout,
    pub quad_bind_group_layout: BindGroupLayout,
}

/// Resource in the Render World storing the packed indirect cull compute pipeline.
#[derive(Resource)]
pub struct PackedQuadCullPipeline {
    pub pipeline_id: CachedComputePipelineId,
    pub bind_group_layout: BindGroupLayout,
}

#[derive(Resource)]
pub struct PackedQuadGenerationPipeline {
    pub generate_pipeline_id: CachedComputePipelineId,
    pub finalize_pipeline_id: CachedComputePipelineId,
    pub bind_group_layout: BindGroupLayout,
}

#[derive(Resource, Default)]
pub struct PreparedPackedGpuGeneratedDraw {
    pub enabled: bool,
    pub regions: Vec<PreparedPackedGpuGeneratedRegion>,
    pub total_column_count: usize,
    pub max_column_count: usize,
    pub total_max_output_quads: usize,
    pub source_chunk_count: usize,
    pub generation_bind_group: Option<BindGroup>,
    pub render_bind_group: Option<BindGroup>,
    pub indirect_buffer: Option<Buffer>,
    pub command_count: usize,
    pub arena_generation: u64,
    pub cull_metadata_signature: u64,
    dispatched: AtomicBool,
}

pub struct PreparedPackedGpuGeneratedRegion {
    pub key: u64,
    pub generation: u64,
    pub column_count: usize,
    pub max_output_quads: usize,
    pub arena_offset_quads: usize,
    pub arena_capacity_quads: usize,
    pub draw_command_index: usize,
    pub bounds_min: Vec3,
    pub bounds_max: Vec3,
}

impl PreparedPackedGpuGeneratedDraw {
    fn disable(&mut self) {
        self.enabled = false;
        self.regions.clear();
        self.total_column_count = 0;
        self.max_column_count = 0;
        self.total_max_output_quads = 0;
        self.source_chunk_count = 0;
        self.generation_bind_group = None;
        self.render_bind_group = None;
        self.indirect_buffer = None;
        self.command_count = 0;
        self.arena_generation = 0;
        self.cull_metadata_signature = 0;
        self.dispatched.store(false, Ordering::Release);
    }

    fn mark_pending(&self) {
        self.dispatched.store(false, Ordering::Release);
    }

    fn mark_dispatched(&self) {
        self.dispatched.store(true, Ordering::Release);
    }

    fn was_dispatched(&self) -> bool {
        self.dispatched.load(Ordering::Acquire)
    }

    fn matches_regions(
        &self,
        planned: &[PreparedPackedGpuGeneratedRegion],
        arena_generation: u64,
    ) -> bool {
        self.enabled
            && self.was_dispatched()
            && self.arena_generation == arena_generation
            && self.regions.len() == planned.len()
            && self.regions.iter().zip(planned).all(|(existing, planned)| {
                existing.key == planned.key
                    && existing.generation == planned.generation
                    && existing.column_count == planned.column_count
                    && existing.max_output_quads == planned.max_output_quads
                    && existing.arena_offset_quads == planned.arena_offset_quads
                    && existing.arena_capacity_quads == planned.arena_capacity_quads
                    && existing.draw_command_index == planned.draw_command_index
                    && existing.bounds_min == planned.bounds_min
                    && existing.bounds_max == planned.bounds_max
            })
    }

    fn matches_batches(
        &self,
        batches: &[&PackedGpuGenerationBatch],
        arena_generation: u64,
    ) -> bool {
        self.enabled
            && self.was_dispatched()
            && self.arena_generation == arena_generation
            && self.regions.len() == batches.len()
            && self.regions.iter().zip(batches).enumerate().all(
                |(draw_command_index, (region, batch))| {
                    region.key == batch.key
                        && region.generation == batch.generation
                        && region.column_count == batch.columns.len()
                        && region.max_output_quads == batch.max_output_quads.max(1)
                        && region.draw_command_index == draw_command_index
                        && region.bounds_min == batch.bounds_min
                        && region.bounds_max == batch.bounds_max
                },
            )
    }
}

#[derive(Resource, Default)]
struct PackedGpuGenerationBuffers {
    jobs_buffer: Option<Buffer>,
    jobs_capacity: usize,
    columns_buffer: Option<Buffer>,
    columns_capacity: usize,
    counters_capacity: usize,
    counter_buffer: Option<Buffer>,
    draw_params_buffer: Option<Buffer>,
    draw_params_capacity: usize,
    indirect_buffer: Option<Buffer>,
    indirect_capacity_commands: usize,
}

#[derive(Resource)]
struct PackedQuadGpuTimestampProfiler {
    enabled: bool,
    supported: bool,
    query_set: Option<QuerySet>,
    resolve_buffer: Option<Buffer>,
    readback_buffer: Option<Buffer>,
    timestamp_period_ns: f32,
    pending_readback: Arc<AtomicBool>,
    map_requested: Arc<AtomicBool>,
    mapped_readback: Arc<AtomicBool>,
    last_gpu_pass_us: AtomicU64,
}

impl PackedQuadGpuTimestampProfiler {
    fn new(render_device: &RenderDevice, render_queue: &RenderQueue) -> Self {
        let enabled = env_flag(PACKED_GPU_TIMESTAMPS_ENV);
        let supported = enabled
            && render_device
                .features()
                .contains(WgpuFeatures::TIMESTAMP_QUERY);
        crate::packed_quad_pipeline::record_packed_quad_gpu_timestamp_status(enabled, supported);

        if !enabled {
            return Self::disabled(false);
        }

        if !supported {
            info!(
                "PACKED QUAD RENDERER: RUMPEL_PACKED_GPU_TIMESTAMPS requested, but TIMESTAMP_QUERY is not enabled on this device"
            );
            return Self::disabled(true);
        }

        let query_set = render_device
            .wgpu_device()
            .create_query_set(&QuerySetDescriptor {
                label: Some("packed_quad_gpu_timestamp_query_set"),
                ty: QueryType::Timestamp,
                count: PACKED_GPU_TIMESTAMP_QUERY_COUNT,
            });
        let resolve_buffer = render_device.create_buffer(&BufferDescriptor {
            label: Some("packed_quad_gpu_timestamp_resolve_buffer"),
            size: PACKED_GPU_TIMESTAMP_BUFFER_SIZE,
            usage: BufferUsages::QUERY_RESOLVE | BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let readback_buffer = render_device.create_buffer(&BufferDescriptor {
            label: Some("packed_quad_gpu_timestamp_readback_buffer"),
            size: PACKED_GPU_TIMESTAMP_BUFFER_SIZE,
            usage: BufferUsages::MAP_READ | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        info!(
            timestamp_period_ns = render_queue.get_timestamp_period(),
            "PACKED QUAD RENDERER: GPU timestamp profiling enabled"
        );

        Self {
            enabled: true,
            supported: true,
            query_set: Some(query_set),
            resolve_buffer: Some(resolve_buffer),
            readback_buffer: Some(readback_buffer),
            timestamp_period_ns: render_queue.get_timestamp_period(),
            pending_readback: Arc::new(AtomicBool::new(false)),
            map_requested: Arc::new(AtomicBool::new(false)),
            mapped_readback: Arc::new(AtomicBool::new(false)),
            last_gpu_pass_us: AtomicU64::new(0),
        }
    }

    fn disabled(requested: bool) -> Self {
        Self {
            enabled: requested,
            supported: false,
            query_set: None,
            resolve_buffer: None,
            readback_buffer: None,
            timestamp_period_ns: 0.0,
            pending_readback: Arc::new(AtomicBool::new(false)),
            map_requested: Arc::new(AtomicBool::new(false)),
            mapped_readback: Arc::new(AtomicBool::new(false)),
            last_gpu_pass_us: AtomicU64::new(0),
        }
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
            return;
        };

        let data = readback_buffer.slice(..).get_mapped_range();
        if data.len() >= PACKED_GPU_TIMESTAMP_BUFFER_SIZE as usize {
            let begin = u64::from_le_bytes(data[0..8].try_into().expect("timestamp begin bytes"));
            let end = u64::from_le_bytes(data[8..16].try_into().expect("timestamp end bytes"));
            if end >= begin {
                let elapsed_us = ((end - begin) as f64 * f64::from(self.timestamp_period_ns)
                    / 1_000.0)
                    .round()
                    .clamp(0.0, u64::MAX as f64) as u64;
                self.last_gpu_pass_us.store(elapsed_us, Ordering::Relaxed);
                crate::packed_quad_pipeline::record_packed_quad_gpu_pass_time(elapsed_us);
            }
        }
        drop(data);
        readback_buffer.unmap();
        self.map_requested.store(false, Ordering::Release);
        self.pending_readback.store(false, Ordering::Release);
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
            return;
        };

        let mapped_readback = Arc::clone(&self.mapped_readback);
        let map_requested = Arc::clone(&self.map_requested);
        let pending_readback = Arc::clone(&self.pending_readback);
        readback_buffer
            .slice(..)
            .map_async(MapMode::Read, move |result| {
                if let Err(error) = result {
                    warn!("PACKED QUAD RENDERER: GPU timestamp readback failed: {error}");
                    map_requested.store(false, Ordering::Release);
                    pending_readback.store(false, Ordering::Release);
                    return;
                }
                mapped_readback.store(true, Ordering::Release);
            });
    }

    fn try_begin_query(&self) -> bool {
        self.enabled
            && self.supported
            && self
                .pending_readback
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
    }

    fn timestamp_writes(&self) -> Option<RenderPassTimestampWrites<'_>> {
        self.query_set
            .as_ref()
            .map(|query_set| RenderPassTimestampWrites {
                query_set,
                beginning_of_pass_write_index: Some(0),
                end_of_pass_write_index: Some(1),
            })
    }

    fn finish_query(&self, render_context: &mut RenderContext) {
        let (Some(query_set), Some(resolve_buffer), Some(readback_buffer)) =
            (&self.query_set, &self.resolve_buffer, &self.readback_buffer)
        else {
            self.pending_readback.store(false, Ordering::Release);
            return;
        };

        let encoder = render_context.command_encoder();
        encoder.resolve_query_set(
            query_set,
            0..PACKED_GPU_TIMESTAMP_QUERY_COUNT,
            resolve_buffer,
            0,
        );
        encoder.copy_buffer_to_buffer(
            resolve_buffer,
            0,
            readback_buffer,
            0,
            PACKED_GPU_TIMESTAMP_BUFFER_SIZE,
        );
    }
}

/// Helper function to calculate the number of vertices needed for a given number of quads.
/// Safely prevents multiplication overflow and bounds checking.
#[inline]
pub fn vertex_count_for_quads(quads_count: usize) -> u32 {
    quads_count
        .checked_mul(6)
        .and_then(|val| u32::try_from(val).ok())
        .unwrap_or(u32::MAX)
}

fn point_inside_bounds(point: Vec3, bounds_min: Vec3, bounds_max: Vec3) -> bool {
    point.x >= bounds_min.x
        && point.x <= bounds_max.x
        && point.y >= bounds_min.y
        && point.y <= bounds_max.y
        && point.z >= bounds_min.z
        && point.z <= bounds_max.z
}

fn face_points_toward_view(
    face: u8,
    view_position: Vec3,
    bounds_min: Vec3,
    bounds_max: Vec3,
) -> bool {
    match face {
        0 => view_position.x >= bounds_min.x,
        1 => view_position.x <= bounds_max.x,
        2 => view_position.y >= bounds_min.y,
        3 => view_position.y <= bounds_max.y,
        4 => view_position.z >= bounds_min.z,
        5 => view_position.z <= bounds_max.z,
        _ => true,
    }
}

fn batch_is_visible(
    batch: &PreparedPackedQuadBatch,
    view_position: Vec3,
    clip_from_world: Mat4,
) -> bool {
    point_inside_bounds(view_position, batch.bounds_min, batch.bounds_max)
        || crate::packed_quad_pipeline::aabb_intersects_clip_frustum(
            clip_from_world,
            batch.bounds_min,
            batch.bounds_max,
        )
}

fn bounds_are_visible(bounds_min: Vec3, bounds_max: Vec3, clip_from_world: Mat4) -> bool {
    crate::packed_quad_pipeline::aabb_intersects_clip_frustum(
        clip_from_world,
        bounds_min,
        bounds_max,
    )
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct IndirectVisibilityEstimate {
    visible_commands: usize,
    visible_batches: usize,
    visible_quads: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct VisibleIndirectSelection {
    indices: Vec<usize>,
    commands: Vec<crate::packed_quad_buffer::PackedQuadDrawCommand>,
    visible_batches: usize,
    visible_quads: usize,
    considered_commands: usize,
}

fn collect_visible_indirect_commands(
    metadata: &[crate::packed_quad_pipeline::PackedQuadIndirectCommandMetadata],
    draw_commands: &[crate::packed_quad_buffer::PackedQuadDrawCommand],
    command_count: usize,
    view_position: Vec3,
    clip_from_world: Mat4,
    face_range_cull_enabled: bool,
) -> VisibleIndirectSelection {
    let mut visible_batch_keys = HashSet::new();
    let mut selection = VisibleIndirectSelection {
        indices: Vec::with_capacity(command_count.min(draw_commands.len())),
        commands: Vec::with_capacity(command_count.min(draw_commands.len())),
        visible_batches: 0,
        visible_quads: 0,
        considered_commands: 0,
    };

    for (index, (command, draw_command)) in metadata
        .iter()
        .zip(draw_commands.iter())
        .take(command_count)
        .enumerate()
    {
        if command.len_quads == 0 {
            continue;
        }
        selection.considered_commands += 1;

        let view_inside_batch =
            point_inside_bounds(view_position, command.bounds_min, command.bounds_max);
        if !view_inside_batch
            && !bounds_are_visible(command.bounds_min, command.bounds_max, clip_from_world)
        {
            continue;
        }

        if face_range_cull_enabled
            && !view_inside_batch
            && let Some(face) = command.face
            && !face_points_toward_view(face, view_position, command.bounds_min, command.bounds_max)
        {
            continue;
        }

        selection.indices.push(index);
        selection.commands.push(*draw_command);
        selection.visible_quads += command.len_quads;
        visible_batch_keys.insert(command.batch_key);
    }

    selection.visible_batches = visible_batch_keys.len();
    selection
}

fn estimate_visible_indirect_commands(
    commands: &[crate::packed_quad_pipeline::PackedQuadIndirectCommandMetadata],
    command_count: usize,
    view_position: Vec3,
    clip_from_world: Mat4,
    face_range_cull_enabled: bool,
) -> IndirectVisibilityEstimate {
    let mut visible_batch_keys = HashSet::new();
    let mut visible_commands = 0;
    let mut visible_quads = 0;

    for command in commands.iter().take(command_count) {
        if command.len_quads == 0 {
            continue;
        }

        let view_inside_batch =
            point_inside_bounds(view_position, command.bounds_min, command.bounds_max);
        if !view_inside_batch
            && !bounds_are_visible(command.bounds_min, command.bounds_max, clip_from_world)
        {
            continue;
        }

        if face_range_cull_enabled
            && !view_inside_batch
            && let Some(face) = command.face
            && !face_points_toward_view(face, view_position, command.bounds_min, command.bounds_max)
        {
            continue;
        }

        visible_commands += 1;
        visible_quads += command.len_quads;
        visible_batch_keys.insert(command.batch_key);
    }

    IndirectVisibilityEstimate {
        visible_commands,
        visible_batches: visible_batch_keys.len(),
        visible_quads,
    }
}

fn packed_fog_range_from_env() -> (f32, f32) {
    let start = env_f32(PACKED_FOG_START_ENV).unwrap_or(DEFAULT_PACKED_FOG_START);
    let end = env_f32(PACKED_FOG_END_ENV).unwrap_or(DEFAULT_PACKED_FOG_END);
    if end <= start + 1.0 {
        return (DEFAULT_PACKED_FOG_START, DEFAULT_PACKED_FOG_END);
    }
    (start, end)
}

/// System running in the Prepare stage of the Render schedule.
/// Extracts view projection matrices and uploads them to GPU uniform buffers.
pub fn prepare_packed_quad_view_uniforms(
    mut commands: Commands,
    render_device: Res<RenderDevice>,
    render_queue: Res<RenderQueue>,
    views: Query<(Entity, &ExtractedView, Option<&PackedQuadViewUniform>)>,
    pipeline: Res<PackedQuadPipeline>,
    prepared_batches: Res<PreparedPackedQuadBatches>,
) {
    let system_started_at = Instant::now();

    // Perform one-shot logging once batches have been prepared
    if !prepared_batches.batches.is_empty() && !RENDERER_LOGGED.load(Ordering::Relaxed) {
        let batches_count = prepared_batches.batches.len();
        let mut total_quads = 0;
        for batch in prepared_batches.batches.values() {
            total_quads += batch.len_quads;
        }
        let total_vertices = vertex_count_for_quads(total_quads);

        info!(
            "PACKED QUAD RENDERER: Enabled!\n\
             - Prepared batches count: {}\n\
             - Total quads: {}\n\
             - Total vertex count: {}",
            batches_count, total_quads, total_vertices
        );

        RENDERER_LOGGED.store(true, Ordering::Relaxed);
    }

    for (entity, extracted_view, existing_uniform) in &views {
        let view_proj = extracted_view.clip_from_world.unwrap_or_else(|| {
            extracted_view.clip_from_view * extracted_view.world_from_view.affine().inverse()
        });
        let view_position = extracted_view.world_from_view.translation();
        let (fog_start, fog_end) = packed_fog_range_from_env();
        let view_data = PackedQuadViewBuffer {
            view_projection_columns: view_proj.to_cols_array_2d(),
            camera_position_and_fog_start: [
                view_position.x,
                view_position.y,
                view_position.z,
                fog_start,
            ],
            fog_color_and_end: [0.529, 0.808, 0.922, fog_end],
        };

        if let Some(uniform) = existing_uniform {
            render_queue.write_buffer(&uniform.buffer, 0, bytemuck::bytes_of(&view_data));
            continue;
        }

        let buffer = render_device.create_buffer(&BufferDescriptor {
            label: Some("packed_quad_view_buffer"),
            size: PACKED_VIEW_BUFFER_SIZE,
            usage: BufferUsages::STORAGE | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        render_queue.write_buffer(&buffer, 0, bytemuck::bytes_of(&view_data));

        let bind_group = render_device.create_bind_group(
            Some("packed_quad_view_bind_group"),
            &pipeline.view_bind_group_layout,
            &BindGroupEntries::sequential((buffer.as_entire_buffer_binding(),)),
        );

        commands
            .entity(entity)
            .insert(PackedQuadViewUniform { buffer, bind_group });
    }

    crate::packed_quad_pipeline::record_packed_quad_view_prepare(
        system_started_at
            .elapsed()
            .as_micros()
            .min(u128::from(u64::MAX)) as u64,
    );
}

#[allow(clippy::too_many_arguments)]
fn prepare_packed_gpu_generated_draw(
    render_device: Res<RenderDevice>,
    render_queue: Res<RenderQueue>,
    packed_pipeline: Option<Res<PackedQuadPipeline>>,
    generation_pipeline: Option<Res<PackedQuadGenerationPipeline>>,
    atlas: Res<crate::packed_quad_pipeline::PackedQuadBlockAtlas>,
    gpu_images: Res<RenderAssets<GpuImage>>,
    extracted_batches: Res<PackedGpuGenerationBatches>,
    prepared_palette: Res<crate::packed_quad_pipeline::PreparedPackedQuadBlockTexturePalette>,
    mut arena: ResMut<crate::packed_quad_pipeline::PackedQuadGpuArena>,
    mut buffers: ResMut<PackedGpuGenerationBuffers>,
    mut prepared: ResMut<PreparedPackedGpuGeneratedDraw>,
    mut gpu_cull: ResMut<crate::packed_quad_pipeline::PreparedPackedQuadGpuCull>,
) {
    if !packed_gpu_generation_enabled_from_env() {
        prepared.disable();
        return;
    }

    let (
        Some(packed_pipeline),
        Some(generation_pipeline),
        Some(gpu_atlas),
        Some(texture_palette_buffer),
    ) = (
        packed_pipeline.as_deref(),
        generation_pipeline.as_deref(),
        gpu_images.get(&atlas.handle),
        prepared_palette.buffer.as_ref(),
    )
    else {
        prepared.disable();
        return;
    };

    let mut sorted_batches = extracted_batches
        .batches
        .iter()
        .filter(|batch| !batch.columns.is_empty() && batch.max_output_quads > 0)
        .collect::<Vec<_>>();
    sorted_batches.sort_by_key(|batch| batch.key);

    if sorted_batches.is_empty() {
        prepared.disable();
        return;
    }

    let has_first_instance = render_device
        .features()
        .contains(bevy::render::render_resource::WgpuFeatures::INDIRECT_FIRST_INSTANCE);
    if sorted_batches.len() > 1 && !has_first_instance {
        prepared.disable();
        return;
    }

    if prepared.matches_batches(&sorted_batches, arena.generation)
        && prepared.generation_bind_group.is_some()
        && prepared.render_bind_group.is_some()
        && prepared.indirect_buffer.is_some()
    {
        crate::packed_quad_pipeline::record_packed_gpu_generation_prepare(
            arena.capacity_quads,
            arena.next_free_quads,
            prepared.total_max_output_quads,
            prepared.total_column_count,
            sorted_batches.len(),
            prepared.source_chunk_count,
        );
        prepare_generated_gpu_cull(
            &render_device,
            &render_queue,
            &mut gpu_cull,
            &prepared.regions,
            prepared.indirect_buffer.as_ref(),
            Some(prepared.cull_metadata_signature),
        );
        return;
    }

    let existing_allocations = prepared
        .regions
        .iter()
        .map(|region| {
            (
                region.key,
                crate::packed_quad_buffer::PackedQuadArenaAllocation {
                    key: region.key,
                    offset_quads: region.arena_offset_quads,
                    len_quads: region.max_output_quads,
                    capacity_quads: region.arena_capacity_quads,
                    generation: region.generation,
                },
            )
        })
        .collect::<HashMap<_, _>>();

    let allocation_requests = sorted_batches
        .iter()
        .map(
            |batch| crate::packed_quad_buffer::PackedGpuGenerationAllocationRequest {
                key: batch.key,
                requested_quads: batch.max_output_quads,
                generation: batch.generation,
            },
        )
        .collect::<Vec<_>>();
    let (new_allocations, next_free_quads) =
        crate::packed_quad_buffer::plan_gpu_generated_arena_allocations(
            &existing_allocations,
            &allocation_requests,
            next_packed_gpu_generation_slot_capacity,
        );

    let mut planned_regions = Vec::with_capacity(sorted_batches.len());
    let mut total_max_output_quads = 0usize;
    let mut total_column_count = 0usize;
    let mut max_column_count = 0usize;
    let mut source_chunk_count = 0usize;

    for (draw_command_index, batch) in sorted_batches.iter().enumerate() {
        let requested_quads = batch.max_output_quads.max(1);
        let Some(allocation) = new_allocations.get(&batch.key) else {
            continue;
        };

        total_max_output_quads = total_max_output_quads.saturating_add(requested_quads);
        total_column_count = total_column_count.saturating_add(batch.columns.len());
        max_column_count = max_column_count.max(batch.columns.len());
        source_chunk_count = source_chunk_count.saturating_add(batch.source_chunk_count);

        planned_regions.push(PreparedPackedGpuGeneratedRegion {
            key: batch.key,
            generation: batch.generation,
            column_count: batch.columns.len(),
            max_output_quads: requested_quads,
            arena_offset_quads: allocation.offset_quads,
            arena_capacity_quads: allocation.capacity_quads,
            draw_command_index,
            bounds_min: batch.bounds_min,
            bounds_max: batch.bounds_max,
        });
    }

    if arena.buffer.is_none() || next_free_quads > arena.capacity_quads {
        let next_capacity =
            next_packed_gpu_generation_arena_capacity(arena.capacity_quads, next_free_quads);
        arena.buffer = Some(render_device.create_buffer(&BufferDescriptor {
            label: Some("packed_gpu_generation_arena_buffer"),
            size: (next_capacity
                * std::mem::size_of::<crate::voxel_packed_quads::PackedVoxelQuad>())
                as u64,
            usage: BufferUsages::STORAGE | BufferUsages::COPY_DST | BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        }));
        arena.capacity_quads = next_capacity;
        arena.stats.total_capacity_quads = next_capacity;
        arena.generation = arena.generation.saturating_add(1);
    }

    arena.allocations = new_allocations;
    arena.next_free_quads = next_free_quads;
    arena.stats.used_quads = total_max_output_quads;
    arena.stats.allocated_slot_quads = next_free_quads;
    arena.stats.free_quads = arena.capacity_quads.saturating_sub(total_max_output_quads);
    arena.stats.uploaded_bytes = 0;

    if prepared.matches_regions(&planned_regions, arena.generation)
        && prepared.generation_bind_group.is_some()
        && prepared.render_bind_group.is_some()
        && prepared.indirect_buffer.is_some()
    {
        crate::packed_quad_pipeline::record_packed_gpu_generation_prepare(
            arena.capacity_quads,
            next_free_quads,
            total_max_output_quads,
            total_column_count,
            sorted_batches.len(),
            source_chunk_count,
        );
        prepare_generated_gpu_cull(
            &render_device,
            &render_queue,
            &mut gpu_cull,
            &planned_regions,
            prepared.indirect_buffer.as_ref(),
            Some(prepared.cull_metadata_signature),
        );
        return;
    }

    if buffers.jobs_buffer.is_none() || buffers.jobs_capacity < sorted_batches.len() {
        let next_capacity = next_packed_gpu_generation_buffer_capacity(sorted_batches.len(), 16);
        buffers.jobs_buffer = Some(render_device.create_buffer(&BufferDescriptor {
            label: Some("packed_gpu_generation_jobs_buffer"),
            size: (next_capacity * std::mem::size_of::<PackedGpuGenerationJob>()) as u64,
            usage: BufferUsages::STORAGE | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        }));
        buffers.jobs_capacity = next_capacity;
    }

    if buffers.columns_buffer.is_none() || buffers.columns_capacity < total_column_count {
        let next_capacity = next_packed_gpu_generation_buffer_capacity(total_column_count, 256);
        buffers.columns_buffer =
            Some(
                render_device.create_buffer(&BufferDescriptor {
                    label: Some("packed_gpu_generation_columns_buffer"),
                    size: (next_capacity
                        * std::mem::size_of::<
                            crate::packed_quad_gpu_generation::PackedGpuSurfaceColumn,
                        >()) as u64,
                    usage: BufferUsages::STORAGE | BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                }),
            );
        buffers.columns_capacity = next_capacity;
    }

    if buffers.counter_buffer.is_none() || buffers.counters_capacity < sorted_batches.len() {
        let next_capacity = next_packed_gpu_generation_buffer_capacity(sorted_batches.len(), 16);
        buffers.counter_buffer =
            Some(render_device.create_buffer(&BufferDescriptor {
                label: Some("packed_gpu_generation_counter_buffer"),
                size: (next_capacity
                    * std::mem::size_of::<
                        crate::packed_quad_gpu_generation::PackedGpuGenerationCounter,
                    >()) as u64,
                usage: BufferUsages::STORAGE | BufferUsages::COPY_DST | BufferUsages::COPY_SRC,
                mapped_at_creation: false,
            }));
        buffers.counters_capacity = next_capacity;
    }

    if buffers.draw_params_buffer.is_none() || buffers.draw_params_capacity < sorted_batches.len() {
        let next_capacity = next_packed_gpu_generation_buffer_capacity(sorted_batches.len(), 16);
        buffers.draw_params_buffer = Some(render_device.create_buffer(&BufferDescriptor {
            label: Some("packed_gpu_generation_draw_params_buffer"),
            size: (next_capacity
                * std::mem::size_of::<crate::packed_quad_buffer::PackedQuadDrawParams>())
                as u64,
            usage: BufferUsages::STORAGE | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        }));
        buffers.draw_params_capacity = next_capacity;
    }

    if buffers.indirect_buffer.is_none()
        || buffers.indirect_capacity_commands < sorted_batches.len()
    {
        let next_capacity = next_packed_gpu_generation_buffer_capacity(sorted_batches.len(), 16);
        buffers.indirect_buffer = Some(render_device.create_buffer(&BufferDescriptor {
            label: Some("packed_gpu_generation_indirect_buffer"),
            size: (next_capacity
                * std::mem::size_of::<crate::packed_quad_buffer::PackedQuadDrawCommand>())
                as u64,
            usage: BufferUsages::INDIRECT | BufferUsages::STORAGE | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        }));
        buffers.indirect_capacity_commands = next_capacity;
    }

    let Some(jobs_buffer) = buffers.jobs_buffer.as_ref() else {
        prepared.disable();
        return;
    };
    let Some(columns_buffer) = buffers.columns_buffer.as_ref() else {
        prepared.disable();
        return;
    };
    let Some(counter_buffer) = buffers.counter_buffer.as_ref() else {
        prepared.disable();
        return;
    };
    let Some(draw_params_buffer) = buffers.draw_params_buffer.as_ref() else {
        prepared.disable();
        return;
    };
    let Some(indirect_buffer) = buffers.indirect_buffer.as_ref() else {
        prepared.disable();
        return;
    };
    let Some(arena_buffer) = arena.buffer.as_ref() else {
        prepared.disable();
        return;
    };

    let mut jobs = Vec::with_capacity(sorted_batches.len());
    let mut columns = Vec::with_capacity(total_column_count);
    let mut draw_params = Vec::with_capacity(sorted_batches.len());
    let mut column_offset = 0usize;

    for (draw_index, (batch, region)) in sorted_batches.iter().zip(&planned_regions).enumerate() {
        let mut params = batch.params;
        params.config[0] = batch.columns.len().min(u32::MAX as usize) as u32;
        params.config[1] = region.max_output_quads.min(u32::MAX as usize) as u32;
        jobs.push(PackedGpuGenerationJob::new(
            params,
            column_offset,
            region.arena_offset_quads,
            draw_index,
            draw_index,
            draw_index,
        ));
        let mut translation = batch.translation;
        translation.w = region.arena_offset_quads as f32;
        draw_params.push(crate::packed_quad_buffer::PackedQuadDrawParams {
            chunk_offset: translation.to_array(),
        });
        columns.extend_from_slice(batch.columns.as_slice());
        column_offset = column_offset.saturating_add(batch.columns.len());
    }

    render_queue.write_buffer(jobs_buffer, 0, bytemuck::cast_slice(&jobs));
    render_queue.write_buffer(columns_buffer, 0, bytemuck::cast_slice(&columns));
    render_queue.write_buffer(draw_params_buffer, 0, bytemuck::cast_slice(&draw_params));

    let generation_bind_group = render_device.create_bind_group(
        Some("packed_gpu_generation_bind_group"),
        &generation_pipeline.bind_group_layout,
        &BindGroupEntries::sequential((
            jobs_buffer.as_entire_buffer_binding(),
            columns_buffer.as_entire_buffer_binding(),
            arena_buffer.as_entire_buffer_binding(),
            counter_buffer.as_entire_buffer_binding(),
            indirect_buffer.as_entire_buffer_binding(),
        )),
    );

    let render_bind_group = render_device.create_bind_group(
        Some("packed_gpu_generation_render_bind_group"),
        &packed_pipeline.quad_bind_group_layout,
        &BindGroupEntries::sequential((
            arena_buffer.as_entire_buffer_binding(),
            draw_params_buffer.as_entire_buffer_binding(),
            texture_palette_buffer.as_entire_buffer_binding(),
            BindingResource::TextureView(&gpu_atlas.texture_view),
            BindingResource::Sampler(&gpu_atlas.sampler),
        )),
    );

    prepared.enabled = true;
    prepared.regions = planned_regions;
    prepared.total_column_count = total_column_count;
    prepared.max_column_count = max_column_count;
    prepared.total_max_output_quads = total_max_output_quads;
    prepared.source_chunk_count = source_chunk_count;
    prepared.command_count = sorted_batches.len();
    prepared.arena_generation = arena.generation;
    prepared.cull_metadata_signature = generated_regions_cull_metadata_signature(&prepared.regions);
    prepared.generation_bind_group = Some(generation_bind_group);
    prepared.render_bind_group = Some(render_bind_group);
    prepared.indirect_buffer = Some(indirect_buffer.clone());
    prepared.mark_pending();

    crate::packed_quad_pipeline::record_packed_gpu_generation_prepare(
        arena.capacity_quads,
        next_free_quads,
        total_max_output_quads,
        total_column_count,
        sorted_batches.len(),
        source_chunk_count,
    );
    prepare_generated_gpu_cull(
        &render_device,
        &render_queue,
        &mut gpu_cull,
        &prepared.regions,
        prepared.indirect_buffer.as_ref(),
        Some(prepared.cull_metadata_signature),
    );
}

fn generated_region_cull_metadata(
    region: &PreparedPackedGpuGeneratedRegion,
) -> crate::packed_quad_pipeline::PackedQuadIndirectCommandMetadata {
    crate::packed_quad_pipeline::PackedQuadIndirectCommandMetadata {
        batch_key: region.key,
        face: None,
        len_quads: region.max_output_quads,
        bounds_min: region.bounds_min,
        bounds_max: region.bounds_max,
    }
}

fn update_generated_cull_signature(signature: u64, value: u64) -> u64 {
    (signature ^ value).wrapping_mul(PACKED_GPU_GENERATED_CULL_SIGNATURE_PRIME)
}

fn generated_regions_cull_metadata_signature(regions: &[PreparedPackedGpuGeneratedRegion]) -> u64 {
    let mut signature = PACKED_GPU_GENERATED_CULL_SIGNATURE_OFFSET;
    signature = update_generated_cull_signature(signature, regions.len() as u64);
    for (index, region) in regions.iter().enumerate() {
        signature = update_generated_cull_signature(signature, index as u64);
        signature = update_generated_cull_signature(signature, region.key);
        signature = update_generated_cull_signature(signature, region.max_output_quads as u64);
        for value in region.bounds_min.to_array() {
            signature = update_generated_cull_signature(signature, u64::from(value.to_bits()));
        }
        for value in region.bounds_max.to_array() {
            signature = update_generated_cull_signature(signature, u64::from(value.to_bits()));
        }
    }
    signature
}

fn generated_cull_config_signature(config: crate::packed_quad_buffer::PackedQuadCullConfig) -> u64 {
    let mut signature = PACKED_GPU_GENERATED_CULL_SIGNATURE_OFFSET;
    signature = update_generated_cull_signature(signature, u64::from(config.command_count));
    signature = update_generated_cull_signature(signature, u64::from(config.face_range_cull));
    signature = update_generated_cull_signature(signature, u64::from(config.compact_output));
    signature
}

fn prepare_generated_gpu_cull(
    render_device: &RenderDevice,
    render_queue: &RenderQueue,
    gpu_cull: &mut crate::packed_quad_pipeline::PreparedPackedQuadGpuCull,
    regions: &[PreparedPackedGpuGeneratedRegion],
    source_indirect_buffer: Option<&Buffer>,
    metadata_signature: Option<u64>,
) {
    let command_count = regions.len();
    let generated_cull_enabled = env_flag_default(PACKED_GPU_CULL_ENV, true)
        && command_count > 0
        && source_indirect_buffer.is_some();
    if !generated_cull_enabled {
        gpu_cull.disable();
        crate::packed_quad_pipeline::record_packed_quad_gpu_cull_prepare(false, 0, false, false);
        return;
    }

    let has_indirect_count = render_device
        .features()
        .contains(bevy::render::render_resource::WgpuFeatures::MULTI_DRAW_INDIRECT_COUNT);
    let compact_enabled = has_indirect_count;
    let next_capacity = if gpu_cull.metadata_buffer.is_none()
        || gpu_cull.output_indirect_buffer.is_none()
        || command_count > gpu_cull.capacity_commands
    {
        if gpu_cull.capacity_commands == 0 {
            command_count.max(16).next_power_of_two()
        } else {
            command_count
                .next_power_of_two()
                .max(gpu_cull.capacity_commands * 2)
        }
    } else {
        gpu_cull.capacity_commands
    };

    let metadata_buffer_recreated = next_capacity != gpu_cull.capacity_commands
        || gpu_cull.metadata_buffer.is_none()
        || gpu_cull.output_indirect_buffer.is_none();
    if metadata_buffer_recreated {
        gpu_cull.metadata_buffer = Some(render_device.create_buffer(&BufferDescriptor {
            label: Some("packed_gpu_generated_cull_metadata_buffer"),
            size: (next_capacity
                * std::mem::size_of::<crate::packed_quad_buffer::PackedQuadCullCommandMetadata>())
                as u64,
            usage: BufferUsages::STORAGE | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        }));
        gpu_cull.output_indirect_buffer = Some(render_device.create_buffer(&BufferDescriptor {
            label: Some("packed_gpu_generated_cull_output_indirect_buffer"),
            size: (next_capacity
                * std::mem::size_of::<crate::packed_quad_buffer::PackedQuadDrawCommand>())
                as u64,
            usage: BufferUsages::INDIRECT | BufferUsages::STORAGE | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        }));
        gpu_cull.capacity_commands = next_capacity;
    }

    let config_buffer_recreated = gpu_cull.config_buffer.is_none();
    if config_buffer_recreated {
        gpu_cull.config_buffer = Some(render_device.create_buffer(&BufferDescriptor {
            label: Some("packed_gpu_generated_cull_config_buffer"),
            size: std::mem::size_of::<crate::packed_quad_buffer::PackedQuadCullConfig>() as u64,
            usage: BufferUsages::STORAGE | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        }));
    }
    if gpu_cull.count_buffer.is_none() {
        gpu_cull.count_buffer = Some(render_device.create_buffer(&BufferDescriptor {
            label: Some("packed_gpu_generated_cull_count_buffer"),
            size: std::mem::size_of::<u32>() as u64,
            usage: BufferUsages::INDIRECT | BufferUsages::STORAGE | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        }));
    }

    let cull_config = crate::packed_quad_buffer::PackedQuadCullConfig {
        command_count: command_count.min(u32::MAX as usize) as u32,
        face_range_cull: 0,
        compact_output: u32::from(compact_enabled),
        _padding: 0,
    };
    let metadata_signature =
        metadata_signature.unwrap_or_else(|| generated_regions_cull_metadata_signature(regions));
    let config_signature = generated_cull_config_signature(cull_config);

    if (metadata_buffer_recreated || gpu_cull.metadata_signature != metadata_signature)
        && let Some(metadata_buffer) = &gpu_cull.metadata_buffer
    {
        let cull_metadata = regions
            .iter()
            .map(generated_region_cull_metadata)
            .map(crate::packed_quad_pipeline::packed_gpu_cull_metadata_from_command)
            .collect::<Vec<_>>();
        render_queue.write_buffer(metadata_buffer, 0, bytemuck::cast_slice(&cull_metadata));
    }
    if (config_buffer_recreated || gpu_cull.config_signature != config_signature)
        && let Some(config_buffer) = &gpu_cull.config_buffer
    {
        render_queue.write_buffer(config_buffer, 0, bytemuck::bytes_of(&cull_config));
    }

    gpu_cull.enabled = true;
    gpu_cull.compact_enabled = compact_enabled;
    gpu_cull.count_supported = has_indirect_count;
    gpu_cull.command_count = command_count;
    gpu_cull.metadata_signature = metadata_signature;
    gpu_cull.config_signature = config_signature;
    gpu_cull.reset_dispatched();
    crate::packed_quad_pipeline::record_packed_quad_gpu_cull_prepare(
        true,
        command_count,
        has_indirect_count,
        compact_enabled,
    );
}

fn next_packed_gpu_generation_buffer_capacity(required: usize, minimum: usize) -> usize {
    required
        .max(minimum)
        .checked_next_power_of_two()
        .unwrap_or(usize::MAX)
}

fn next_packed_gpu_generation_slot_capacity(required_quads: usize) -> usize {
    next_packed_gpu_generation_buffer_capacity(required_quads, 1024)
}

fn next_packed_gpu_generation_arena_capacity(
    current_capacity_quads: usize,
    required_quads: usize,
) -> usize {
    let minimum = current_capacity_quads.max(1024);
    let requested = required_quads.max(minimum);
    requested.checked_next_power_of_two().unwrap_or(usize::MAX)
}

pub struct PackedQuadGenerationNode {
    state: PackedQuadGenerationState,
}

enum PackedQuadGenerationState {
    Loading,
    Ready,
}

impl Default for PackedQuadGenerationNode {
    fn default() -> Self {
        Self {
            state: PackedQuadGenerationState::Loading,
        }
    }
}

impl render_graph::Node for PackedQuadGenerationNode {
    fn update(&mut self, world: &mut World) {
        let pipeline = world.resource::<PackedQuadGenerationPipeline>();
        let pipeline_cache = world.resource::<PipelineCache>();

        match self.state {
            PackedQuadGenerationState::Loading => {
                let generate_ready = matches!(
                    pipeline_cache.get_compute_pipeline_state(pipeline.generate_pipeline_id),
                    CachedPipelineState::Ok(_)
                );
                let finalize_ready = matches!(
                    pipeline_cache.get_compute_pipeline_state(pipeline.finalize_pipeline_id),
                    CachedPipelineState::Ok(_)
                );
                if generate_ready && finalize_ready {
                    self.state = PackedQuadGenerationState::Ready;
                }
            }
            PackedQuadGenerationState::Ready => {}
        }
    }

    fn run(
        &self,
        _graph: &mut render_graph::RenderGraphContext,
        render_context: &mut RenderContext,
        world: &World,
    ) -> Result<(), render_graph::NodeRunError> {
        if matches!(self.state, PackedQuadGenerationState::Loading) {
            return Ok(());
        }

        let Some(prepared) = world.get_resource::<PreparedPackedGpuGeneratedDraw>() else {
            return Ok(());
        };
        if !prepared.enabled
            || prepared.command_count == 0
            || prepared.max_column_count == 0
            || prepared.was_dispatched()
        {
            return Ok(());
        }

        let Some(bind_group) = prepared.generation_bind_group.as_ref() else {
            return Ok(());
        };

        let pipeline_cache = world.resource::<PipelineCache>();
        let pipeline = world.resource::<PackedQuadGenerationPipeline>();
        let (Some(generate_pipeline), Some(finalize_pipeline)) = (
            pipeline_cache.get_compute_pipeline(pipeline.generate_pipeline_id),
            pipeline_cache.get_compute_pipeline(pipeline.finalize_pipeline_id),
        ) else {
            return Ok(());
        };

        let Some(counter_buffer) = world
            .get_resource::<PackedGpuGenerationBuffers>()
            .and_then(|buffers| buffers.counter_buffer.as_ref())
        else {
            return Ok(());
        };

        render_context
            .command_encoder()
            .clear_buffer(counter_buffer, 0, None);

        {
            let mut pass =
                render_context
                    .command_encoder()
                    .begin_compute_pass(&ComputePassDescriptor {
                        label: Some("packed_gpu_generation_generate_pass"),
                        timestamp_writes: None,
                    });
            pass.set_pipeline(generate_pipeline);
            pass.set_bind_group(0, bind_group, &[]);
            pass.dispatch_workgroups(
                packed_gpu_generation_workgroups(prepared.max_column_count),
                prepared.command_count.min(u32::MAX as usize) as u32,
                1,
            );
        }

        {
            let mut pass =
                render_context
                    .command_encoder()
                    .begin_compute_pass(&ComputePassDescriptor {
                        label: Some("packed_gpu_generation_finalize_pass"),
                        timestamp_writes: None,
                    });
            pass.set_pipeline(finalize_pipeline);
            pass.set_bind_group(0, bind_group, &[]);
            pass.dispatch_workgroups(prepared.command_count.min(u32::MAX as usize) as u32, 1, 1);
        }

        prepared.mark_dispatched();
        Ok(())
    }
}

/// Compute node that writes a GPU-culled indirect command buffer for packed terrain.
pub struct PackedQuadCullNode {
    state: PackedQuadCullState,
}

enum PackedQuadCullState {
    Loading,
    Ready,
}

impl Default for PackedQuadCullNode {
    fn default() -> Self {
        Self {
            state: PackedQuadCullState::Loading,
        }
    }
}

impl render_graph::Node for PackedQuadCullNode {
    fn update(&mut self, world: &mut World) {
        let pipeline = world.resource::<PackedQuadCullPipeline>();
        let pipeline_cache = world.resource::<PipelineCache>();

        match self.state {
            PackedQuadCullState::Loading => {
                if let CachedPipelineState::Ok(_) =
                    pipeline_cache.get_compute_pipeline_state(pipeline.pipeline_id)
                {
                    self.state = PackedQuadCullState::Ready;
                }
            }
            PackedQuadCullState::Ready => {}
        }
    }

    fn run(
        &self,
        graph: &mut render_graph::RenderGraphContext,
        render_context: &mut RenderContext,
        world: &World,
    ) -> Result<(), render_graph::NodeRunError> {
        if matches!(self.state, PackedQuadCullState::Loading) {
            return Ok(());
        }

        let Some(gpu_cull) =
            world.get_resource::<crate::packed_quad_pipeline::PreparedPackedQuadGpuCull>()
        else {
            return Ok(());
        };
        if !gpu_cull.enabled || gpu_cull.command_count == 0 {
            return Ok(());
        }

        let node_start = Instant::now();
        let pipeline_cache = world.resource::<PipelineCache>();
        let pipeline = world.resource::<PackedQuadCullPipeline>();
        let Some(compute_pipeline) = pipeline_cache.get_compute_pipeline(pipeline.pipeline_id)
        else {
            return Ok(());
        };
        let Some(cull_source) = PackedQuadGpuCullSource::from_world(world) else {
            return Ok(());
        };
        let command_count = cull_source.command_count().min(gpu_cull.command_count);
        if command_count == 0 {
            return Ok(());
        }

        let view_entity = graph.view_entity();
        let Some(view_uniform) = world.get::<PackedQuadViewUniform>(view_entity) else {
            return Ok(());
        };
        let Some(extracted_view) = world.get::<ExtractedView>(view_entity) else {
            return Ok(());
        };
        let view_position = extracted_view.world_from_view.translation();
        let clip_from_world = extracted_view.clip_from_world.unwrap_or_else(|| {
            extracted_view.clip_from_view * extracted_view.world_from_view.affine().inverse()
        });

        let (
            Some(metadata_buffer),
            Some(output_indirect_buffer),
            Some(config_buffer),
            Some(count_buffer),
        ) = (
            gpu_cull.metadata_buffer.as_ref(),
            gpu_cull.output_indirect_buffer.as_ref(),
            gpu_cull.config_buffer.as_ref(),
            gpu_cull.count_buffer.as_ref(),
        )
        else {
            return Ok(());
        };

        let estimate = estimate_visible_indirect_commands(
            cull_source.metadata(),
            command_count,
            view_position,
            clip_from_world,
            cull_source.face_range_cull_enabled(),
        );

        let bind_group = render_context.render_device().create_bind_group(
            Some("packed_quad_gpu_cull_bind_group"),
            &pipeline.bind_group_layout,
            &BindGroupEntries::sequential((
                cull_source.indirect_buffer().as_entire_buffer_binding(),
                metadata_buffer.as_entire_buffer_binding(),
                output_indirect_buffer.as_entire_buffer_binding(),
                config_buffer.as_entire_buffer_binding(),
                count_buffer.as_entire_buffer_binding(),
            )),
        );

        if gpu_cull.compact_enabled {
            render_context
                .command_encoder()
                .clear_buffer(count_buffer, 0, None);
        }

        {
            let mut pass =
                render_context
                    .command_encoder()
                    .begin_compute_pass(&ComputePassDescriptor {
                        label: Some("packed_quad_gpu_cull_pass"),
                        timestamp_writes: None,
                    });
            pass.set_pipeline(compute_pipeline);
            pass.set_bind_group(0, &view_uniform.bind_group, &[]);
            pass.set_bind_group(1, &bind_group, &[]);
            let workgroups = command_count.div_ceil(PACKED_GPU_CULL_WORKGROUP_SIZE);
            pass.dispatch_workgroups(workgroups.min(u32::MAX as usize) as u32, 1, 1);
        }

        gpu_cull.mark_dispatched();
        if cull_source.is_generated() {
            crate::packed_quad_pipeline::record_packed_gpu_generation_visible_draws(
                estimate.visible_commands,
                estimate.visible_quads,
            );
        } else {
            crate::packed_quad_pipeline::record_packed_quad_visible_draws(
                estimate.visible_batches,
                estimate.visible_quads,
            );
        }
        let node_us = node_start.elapsed().as_micros().min(u128::from(u64::MAX)) as u64;
        crate::packed_quad_pipeline::record_packed_quad_gpu_cull_node(
            node_us,
            estimate.visible_commands,
            estimate.visible_quads,
        );

        Ok(())
    }
}

enum PackedQuadGpuCullSource<'a> {
    Generated {
        indirect_buffer: &'a Buffer,
        metadata: Vec<crate::packed_quad_pipeline::PackedQuadIndirectCommandMetadata>,
        command_count: usize,
    },
    CpuPrepared {
        indirect_buffer: &'a Buffer,
        metadata: &'a [crate::packed_quad_pipeline::PackedQuadIndirectCommandMetadata],
        command_count: usize,
    },
}

impl<'a> PackedQuadGpuCullSource<'a> {
    fn from_world(world: &'a World) -> Option<Self> {
        if let Some(generated) = world.get_resource::<PreparedPackedGpuGeneratedDraw>()
            && generated.enabled
            && generated.was_dispatched()
            && generated.command_count > 0
            && let Some(indirect_buffer) = generated.indirect_buffer.as_ref()
        {
            return Some(Self::Generated {
                indirect_buffer,
                metadata: generated
                    .regions
                    .iter()
                    .map(generated_region_cull_metadata)
                    .collect(),
                command_count: generated.command_count,
            });
        }

        let indirect_draw =
            world.get_resource::<crate::packed_quad_pipeline::PreparedPackedQuadIndirectDraw>()?;
        if !indirect_draw.is_indirect_enabled || indirect_draw.command_count == 0 {
            return None;
        }
        Some(Self::CpuPrepared {
            indirect_buffer: indirect_draw.indirect_buffer.as_ref()?,
            metadata: &indirect_draw.command_metadata,
            command_count: indirect_draw.command_count,
        })
    }

    fn indirect_buffer(&self) -> &Buffer {
        match self {
            Self::Generated {
                indirect_buffer, ..
            }
            | Self::CpuPrepared {
                indirect_buffer, ..
            } => indirect_buffer,
        }
    }

    fn metadata(&self) -> &[crate::packed_quad_pipeline::PackedQuadIndirectCommandMetadata] {
        match self {
            Self::Generated { metadata, .. } => metadata,
            Self::CpuPrepared { metadata, .. } => metadata,
        }
    }

    fn command_count(&self) -> usize {
        match self {
            Self::Generated { command_count, .. } | Self::CpuPrepared { command_count, .. } => {
                *command_count
            }
        }
    }

    fn face_range_cull_enabled(&self) -> bool {
        matches!(self, Self::CpuPrepared { .. })
            && env_flag_default(PACKED_FACE_RANGE_CULL_ENV, DEFAULT_PACKED_FACE_RANGE_CULL)
    }

    fn is_generated(&self) -> bool {
        matches!(self, Self::Generated { .. })
    }
}

/// A custom Render Graph node that executes the PackedVoxelQuad vertex pulling render pass.
pub struct PackedQuadRenderNode {
    state: PackedQuadRenderState,
}

enum PackedQuadRenderState {
    Loading,
    Ready,
}

impl Default for PackedQuadRenderNode {
    fn default() -> Self {
        Self {
            state: PackedQuadRenderState::Loading,
        }
    }
}

impl render_graph::Node for PackedQuadRenderNode {
    fn update(&mut self, world: &mut World) {
        let pipeline = world.resource::<PackedQuadPipeline>();
        let pipeline_cache = world.resource::<PipelineCache>();

        match self.state {
            PackedQuadRenderState::Loading => {
                if let CachedPipelineState::Ok(_) =
                    pipeline_cache.get_render_pipeline_state(pipeline.pipeline_id)
                {
                    self.state = PackedQuadRenderState::Ready;
                }
            }
            PackedQuadRenderState::Ready => {}
        }
    }

    fn run(
        &self,
        graph: &mut render_graph::RenderGraphContext,
        render_context: &mut RenderContext,
        world: &World,
    ) -> Result<(), render_graph::NodeRunError> {
        if matches!(self.state, PackedQuadRenderState::Loading) {
            return Ok(());
        }
        let node_start = Instant::now();
        let mut render_draw_calls = 0usize;
        let mut render_items_considered = 0usize;

        let pipeline_cache = world.resource::<PipelineCache>();
        let pipeline = world.resource::<PackedQuadPipeline>();
        let prepared_batches = world.resource::<PreparedPackedQuadBatches>();
        let indirect_draw =
            world.get_resource::<crate::packed_quad_pipeline::PreparedPackedQuadIndirectDraw>();
        let gpu_cull =
            world.get_resource::<crate::packed_quad_pipeline::PreparedPackedQuadGpuCull>();
        let timestamp_profiler = world.get_resource::<PackedQuadGpuTimestampProfiler>();
        if let Some(profiler) = timestamp_profiler {
            profiler.collect_mapped_result();
        }

        let Some(render_pipeline) = pipeline_cache.get_render_pipeline(pipeline.pipeline_id) else {
            return Ok(());
        };

        // Fetch view-specific components using graph.view_entity()
        let view_entity = graph.view_entity();
        let Some(view_target) = world.get::<ViewTarget>(view_entity) else {
            return Ok(());
        };
        let Some(depth_texture) = world.get::<ViewDepthTexture>(view_entity) else {
            return Ok(());
        };
        let Some(view_uniform) = world.get::<PackedQuadViewUniform>(view_entity) else {
            return Ok(());
        };
        let Some(extracted_view) = world.get::<ExtractedView>(view_entity) else {
            return Ok(());
        };
        let view_position = extracted_view.world_from_view.translation();
        let clip_from_world = extracted_view.clip_from_world.unwrap_or_else(|| {
            extracted_view.clip_from_view * extracted_view.world_from_view.affine().inverse()
        });

        let mut drawn_indirect = false;

        if let Some(gpu_generated) = world.get_resource::<PreparedPackedGpuGeneratedDraw>()
            && gpu_generated.enabled
            && gpu_generated.was_dispatched()
        {
            render_items_considered = gpu_generated.command_count;
            let generated_gpu_cull = gpu_cull.and_then(|cull| {
                (cull.enabled
                    && cull.was_dispatched()
                    && cull.command_count == gpu_generated.command_count)
                    .then_some(cull)
            });

            if let (Some(bind_group), Some(indirect_buffer)) = (
                gpu_generated.render_bind_group.as_ref(),
                gpu_generated.indirect_buffer.as_ref(),
            ) {
                let timestamp_query_started =
                    timestamp_profiler.is_some_and(PackedQuadGpuTimestampProfiler::try_begin_query);
                let timestamp_writes = timestamp_query_started
                    .then(|| {
                        timestamp_profiler
                            .and_then(PackedQuadGpuTimestampProfiler::timestamp_writes)
                    })
                    .flatten();
                let mut render_pass =
                    render_context
                        .command_encoder()
                        .begin_render_pass(&RenderPassDescriptor {
                            label: Some("packed_gpu_generation_render_pass"),
                            color_attachments: &[Some(view_target.get_color_attachment())],
                            depth_stencil_attachment: Some(
                                depth_texture.get_attachment(StoreOp::Store),
                            ),
                            timestamp_writes,
                            occlusion_query_set: None,
                        });

                render_pass.set_pipeline(render_pipeline);
                render_pass.set_bind_group(0, &view_uniform.bind_group, &[]);
                render_pass.set_bind_group(1, bind_group, &[]);

                if let Some(cull) = generated_gpu_cull {
                    let draw_indirect_buffer = cull
                        .output_indirect_buffer
                        .as_ref()
                        .unwrap_or(indirect_buffer);
                    if cull.compact_enabled {
                        if let Some(count_buffer) = cull.count_buffer.as_ref() {
                            render_pass.multi_draw_indirect_count(
                                draw_indirect_buffer,
                                0,
                                count_buffer,
                                0,
                                gpu_generated.command_count.min(u32::MAX as usize) as u32,
                            );
                            render_draw_calls = usize::from(gpu_generated.command_count > 0);
                        }
                    } else {
                        render_pass.multi_draw_indirect(
                            draw_indirect_buffer,
                            0,
                            gpu_generated.command_count.min(u32::MAX as usize) as u32,
                        );
                        render_draw_calls = usize::from(gpu_generated.command_count > 0);
                    }
                    crate::packed_quad_pipeline::record_packed_quad_cpu_visible_indirect(false, 0);
                } else {
                    let visible_regions = gpu_generated
                        .regions
                        .iter()
                        .filter(|region| {
                            crate::packed_quad_pipeline::generated_region_bounds_visible(
                                view_position,
                                clip_from_world,
                                region.bounds_min,
                                region.bounds_max,
                            )
                        })
                        .collect::<Vec<_>>();
                    let draw_command_stride = std::mem::size_of::<
                        crate::packed_quad_buffer::PackedQuadDrawCommand,
                    >() as u64;
                    let mut visible_quads = 0usize;
                    for region in &visible_regions {
                        visible_quads = visible_quads.saturating_add(region.max_output_quads);
                        let draw_offset = region
                            .draw_command_index
                            .saturating_mul(draw_command_stride as usize);
                        render_pass.draw_indirect(indirect_buffer, draw_offset as u64);
                    }
                    render_draw_calls = visible_regions.len();
                    crate::packed_quad_pipeline::record_packed_gpu_generation_visible_draws(
                        visible_regions.len(),
                        visible_quads,
                    );
                    crate::packed_quad_pipeline::record_packed_quad_cpu_visible_indirect(false, 0);
                }
                drop(render_pass);
                if timestamp_query_started && let Some(profiler) = timestamp_profiler {
                    profiler.finish_query(render_context);
                }
            } else {
                crate::packed_quad_pipeline::record_packed_gpu_generation_visible_draws(0, 0);
            }

            drawn_indirect = true;
        }

        if let Some(indirect) = indirect_draw
            && indirect.is_indirect_enabled
            && !drawn_indirect
            && let (Some(bind_group), Some(indirect_buffer)) =
                (&indirect.bind_group, &indirect.indirect_buffer)
        {
            let gpu_cull_count = gpu_cull.and_then(|cull| {
                (cull.compact_enabled
                    && cull.was_dispatched()
                    && cull.command_count == indirect.command_count)
                    .then_some(cull.count_buffer.as_ref())
                    .flatten()
            });
            let cpu_visible_compact_requested =
                env_flag_default(PACKED_CPU_VISIBLE_COMPACT_ENV, true);

            let visible_selection = gpu_cull_count.is_none().then(|| {
                collect_visible_indirect_commands(
                    &indirect.command_metadata,
                    &indirect.commands,
                    indirect.command_count,
                    view_position,
                    clip_from_world,
                    env_flag_default(PACKED_FACE_RANGE_CULL_ENV, DEFAULT_PACKED_FACE_RANGE_CULL),
                )
            });

            let cpu_compact_buffer = visible_selection
                .as_ref()
                .and_then(|selection| {
                    let requested =
                        cpu_visible_compact_requested && !selection.commands.is_empty();
                    if !requested {
                        return None;
                    }
                    let cpu_visible_indirect = world
                        .get_resource::<crate::packed_quad_pipeline::PackedQuadCpuVisibleIndirectBuffer>()?;
                    let buffer = cpu_visible_indirect.buffer.as_ref()?;
                    (selection.commands.len() <= cpu_visible_indirect.capacity_commands)
                        .then_some(buffer)
                });

            if let (Some(selection), Some(buffer)) =
                (visible_selection.as_ref(), cpu_compact_buffer)
            {
                let render_queue = world.resource::<RenderQueue>();
                render_queue.write_buffer(buffer, 0, bytemuck::cast_slice(&selection.commands));
            }

            let timestamp_query_started =
                timestamp_profiler.is_some_and(PackedQuadGpuTimestampProfiler::try_begin_query);
            let timestamp_writes = timestamp_query_started
                .then(|| {
                    timestamp_profiler.and_then(PackedQuadGpuTimestampProfiler::timestamp_writes)
                })
                .flatten();
            let mut render_pass =
                render_context
                    .command_encoder()
                    .begin_render_pass(&RenderPassDescriptor {
                        label: Some("packed_quad_indirect_render_pass"),
                        color_attachments: &[Some(view_target.get_color_attachment())],
                        depth_stencil_attachment: Some(
                            depth_texture.get_attachment(StoreOp::Store),
                        ),
                        timestamp_writes,
                        occlusion_query_set: None,
                    });

            render_pass.set_pipeline(render_pipeline);
            render_pass.set_bind_group(0, &view_uniform.bind_group, &[]);
            render_pass.set_bind_group(1, bind_group, &[]);

            if let Some(count_buffer) = gpu_cull_count {
                let draw_indirect_buffer = gpu_cull
                    .and_then(|cull| cull.output_indirect_buffer.as_ref())
                    .unwrap_or(indirect_buffer);
                render_pass.multi_draw_indirect_count(
                    draw_indirect_buffer,
                    0,
                    count_buffer,
                    0,
                    indirect.command_count as u32,
                );
                render_draw_calls = usize::from(indirect.command_count > 0);
                render_items_considered = indirect.command_count;
                crate::packed_quad_pipeline::record_packed_quad_cpu_visible_indirect(false, 0);
            } else if indirect.draw_mode == "multi-indirect" && !cpu_visible_compact_requested {
                render_pass.multi_draw_indirect(indirect_buffer, 0, indirect.command_count as u32);
                render_draw_calls = usize::from(indirect.command_count > 0);
                render_items_considered = indirect.command_count;
                let mut batch_keys = HashSet::new();
                let mut visible_quads = 0;
                for command in indirect
                    .command_metadata
                    .iter()
                    .take(indirect.command_count)
                {
                    visible_quads += command.len_quads;
                    batch_keys.insert(command.batch_key);
                }
                crate::packed_quad_pipeline::record_packed_quad_visible_draws(
                    batch_keys.len(),
                    visible_quads,
                );
                crate::packed_quad_pipeline::record_packed_quad_cpu_visible_indirect(false, 0);
            } else if let Some(selection) = visible_selection.as_ref() {
                render_items_considered = selection.considered_commands;
                crate::packed_quad_pipeline::record_packed_quad_visible_draws(
                    selection.visible_batches,
                    selection.visible_quads,
                );
                if let Some(buffer) = cpu_compact_buffer {
                    render_pass.multi_draw_indirect(
                        buffer,
                        0,
                        selection.commands.len().min(u32::MAX as usize) as u32,
                    );
                    render_draw_calls = 1;
                    crate::packed_quad_pipeline::record_packed_quad_cpu_visible_indirect(
                        true,
                        selection.commands.len(),
                    );
                } else {
                    let stride =
                        std::mem::size_of::<crate::packed_quad_buffer::PackedQuadDrawCommand>()
                            as u64;
                    for index in &selection.indices {
                        render_pass.draw_indirect(indirect_buffer, *index as u64 * stride);
                        render_draw_calls += 1;
                    }
                    crate::packed_quad_pipeline::record_packed_quad_cpu_visible_indirect(
                        false,
                        selection.commands.len(),
                    );
                }
            }
            drop(render_pass);
            if timestamp_query_started && let Some(profiler) = timestamp_profiler {
                profiler.finish_query(render_context);
            }
            drawn_indirect = true;
        }

        if !drawn_indirect {
            let timestamp_query_started =
                timestamp_profiler.is_some_and(PackedQuadGpuTimestampProfiler::try_begin_query);
            let timestamp_writes = timestamp_query_started
                .then(|| {
                    timestamp_profiler.and_then(PackedQuadGpuTimestampProfiler::timestamp_writes)
                })
                .flatten();
            let mut render_pass =
                render_context
                    .command_encoder()
                    .begin_render_pass(&RenderPassDescriptor {
                        label: Some("packed_quad_render_pass"),
                        color_attachments: &[Some(view_target.get_color_attachment())],
                        depth_stencil_attachment: Some(
                            depth_texture.get_attachment(StoreOp::Store),
                        ),
                        timestamp_writes,
                        occlusion_query_set: None,
                    });

            render_pass.set_pipeline(render_pipeline);
            render_pass.set_bind_group(0, &view_uniform.bind_group, &[]);

            let mut visible_batches = 0;
            let mut visible_quads = 0;
            for batch in prepared_batches.batches.values() {
                if batch.len_quads == 0 {
                    continue;
                }
                render_items_considered += 1;
                if !batch_is_visible(batch, view_position, clip_from_world) {
                    continue;
                }

                if let Some(bind_group) = &batch.bind_group {
                    render_pass.set_bind_group(1, bind_group, &[]);
                }

                let view_inside_batch =
                    point_inside_bounds(view_position, batch.bounds_min, batch.bounds_max);
                if !env_flag_default(PACKED_FACE_RANGE_CULL_ENV, DEFAULT_PACKED_FACE_RANGE_CULL)
                    || batch.face_ranges.is_empty()
                {
                    render_pass.draw(0..vertex_count_for_quads(batch.len_quads), 0..1);
                    render_draw_calls += 1;
                    visible_batches += 1;
                    visible_quads += batch.len_quads;
                    continue;
                }

                for range in &batch.face_ranges {
                    if range.len_quads == 0 {
                        continue;
                    }
                    if !view_inside_batch
                        && !face_points_toward_view(
                            range.face,
                            view_position,
                            batch.bounds_min,
                            batch.bounds_max,
                        )
                    {
                        continue;
                    }

                    let start_vertex = vertex_count_for_quads(range.start_quads);
                    let end_vertex = vertex_count_for_quads(range.start_quads + range.len_quads);
                    render_pass.draw(start_vertex..end_vertex, 0..1);
                    render_draw_calls += 1;
                    visible_batches += 1;
                    visible_quads += range.len_quads;
                }
            }
            crate::packed_quad_pipeline::record_packed_quad_visible_draws(
                visible_batches,
                visible_quads,
            );
            drop(render_pass);
            if timestamp_query_started && let Some(profiler) = timestamp_profiler {
                profiler.finish_query(render_context);
            }
        }
        let render_node_us = node_start.elapsed().as_micros().min(u128::from(u64::MAX)) as u64;
        crate::packed_quad_pipeline::record_packed_quad_render_node(
            render_node_us,
            render_draw_calls,
            render_items_considered,
        );

        Ok(())
    }
}

/// Plugin responsible for setting up and executing the custom vertex-pulled packed quad renderer.
pub struct PackedQuadRendererPlugin;

impl Plugin for PackedQuadRendererPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "../assets/shaders/packed_quad.wgsl");
        embedded_asset!(app, "../assets/shaders/packed_quad_cull.wgsl");
        embedded_asset!(app, "../assets/shaders/packed_quad_generate.wgsl");

        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };

        render_app.init_resource::<PreparedPackedGpuGeneratedDraw>();
        render_app.init_resource::<PackedGpuGenerationBuffers>();
        render_app.add_systems(
            Render,
            (
                prepare_packed_quad_view_uniforms,
                prepare_packed_gpu_generated_draw
                    .after(crate::packed_quad_pipeline::prepare_packed_quad_buffers),
            )
                .in_set(RenderSystems::Prepare),
        );
    }

    fn finish(&self, app: &mut App) {
        let asset_server = app.world().resource::<AssetServer>();
        let shader = load_embedded_asset!(asset_server, "../assets/shaders/packed_quad.wgsl");
        let cull_shader =
            load_embedded_asset!(asset_server, "../assets/shaders/packed_quad_cull.wgsl");
        let generation_shader =
            load_embedded_asset!(asset_server, "../assets/shaders/packed_quad_generate.wgsl");

        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };

        let render_device = render_app.world().resource::<RenderDevice>().clone();
        let render_queue = render_app.world().resource::<RenderQueue>().clone();
        let pipeline_cache = render_app.world().resource::<PipelineCache>();

        // 1. Create bind group layouts and descriptors
        let view_entries = BindGroupLayoutEntries::sequential(
            ShaderStages::VERTEX_FRAGMENT | ShaderStages::COMPUTE,
            (storage_buffer_read_only_sized(false, None),),
        );
        let view_bind_group_layout =
            render_device.create_bind_group_layout("packed_quad_view_layout", &view_entries);
        let render_view_layout_desc =
            BindGroupLayoutDescriptor::new("packed_quad_view_layout", &view_entries);
        let cull_view_layout_desc =
            BindGroupLayoutDescriptor::new("packed_quad_view_layout", &view_entries);

        let quad_entries = BindGroupLayoutEntries::sequential(
            ShaderStages::VERTEX_FRAGMENT,
            (
                storage_buffer_read_only_sized(false, None),
                storage_buffer_read_only_sized(false, None),
                storage_buffer_read_only_sized(false, None),
                texture_2d_array(TextureSampleType::Float { filterable: true }),
                sampler(SamplerBindingType::Filtering),
            ),
        );
        let quad_bind_group_layout =
            render_device.create_bind_group_layout("packed_quad_storage_layout", &quad_entries);
        let quad_layout_desc =
            BindGroupLayoutDescriptor::new("packed_quad_storage_layout", &quad_entries);

        let cull_entries = BindGroupLayoutEntries::sequential(
            ShaderStages::COMPUTE,
            (
                storage_buffer_read_only_sized(false, None),
                storage_buffer_read_only_sized(false, None),
                storage_buffer_sized(false, None),
                storage_buffer_read_only_sized(false, None),
                storage_buffer_sized(false, None),
            ),
        );
        let cull_bind_group_layout =
            render_device.create_bind_group_layout("packed_quad_gpu_cull_layout", &cull_entries);
        let cull_layout_desc =
            BindGroupLayoutDescriptor::new("packed_quad_gpu_cull_layout", &cull_entries);
        let generation_entries = BindGroupLayoutEntries::sequential(
            ShaderStages::COMPUTE,
            (
                storage_buffer_read_only_sized(false, None),
                storage_buffer_read_only_sized(false, None),
                storage_buffer_sized(false, None),
                storage_buffer_sized(false, None),
                storage_buffer_sized(false, None),
            ),
        );
        let generation_bind_group_layout = render_device
            .create_bind_group_layout("packed_gpu_generation_layout", &generation_entries);
        let generation_generate_layout_desc = BindGroupLayoutDescriptor::new(
            "packed_gpu_generation_generate_layout",
            &generation_entries,
        );
        let generation_finalize_layout_desc = BindGroupLayoutDescriptor::new(
            "packed_gpu_generation_finalize_layout",
            &generation_entries,
        );

        let mut shader_defs = Vec::new();
        if env_flag(PACKED_FACE_DEBUG_ENV) {
            shader_defs.push("PACKED_FACE_DEBUG".into());
        }

        // 2. Queue Render Pipeline
        let pipeline_id = pipeline_cache.queue_render_pipeline(RenderPipelineDescriptor {
            label: Some("packed_quad_render_pipeline".into()),
            layout: vec![render_view_layout_desc, quad_layout_desc],
            vertex: VertexState {
                shader: shader.clone(),
                shader_defs: shader_defs.clone(),
                entry_point: Some("vertex".into()),
                buffers: vec![], // No vertex buffers: we are doing vertex pulling!
            },
            fragment: Some(FragmentState {
                shader,
                shader_defs,
                entry_point: Some("fragment".into()),
                targets: vec![Some(ColorTargetState {
                    format: TextureFormat::bevy_default(),
                    blend: Some(BlendState::REPLACE),
                    write_mask: ColorWrites::ALL,
                })],
            }),
            primitive: PrimitiveState {
                topology: PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: FrontFace::Ccw,
                cull_mode: Some(Face::Back),
                unclipped_depth: false,
                polygon_mode: PolygonMode::Fill,
                conservative: false,
            },
            depth_stencil: Some(DepthStencilState {
                format: TextureFormat::Depth32Float,
                depth_write_enabled: true,
                depth_compare: CompareFunction::GreaterEqual,
                stencil: StencilState::default(),
                bias: DepthBiasState::default(),
            }),
            multisample: MultisampleState::default(),
            push_constant_ranges: vec![],
            zero_initialize_workgroup_memory: false,
        });
        let cull_pipeline_id = pipeline_cache.queue_compute_pipeline(ComputePipelineDescriptor {
            label: Some("packed_quad_gpu_cull_pipeline".into()),
            layout: vec![cull_view_layout_desc, cull_layout_desc],
            shader: cull_shader,
            shader_defs: vec![],
            entry_point: Some(std::borrow::Cow::Borrowed("main")),
            push_constant_ranges: vec![],
            zero_initialize_workgroup_memory: false,
        });
        let generate_pipeline_id =
            pipeline_cache.queue_compute_pipeline(ComputePipelineDescriptor {
                label: Some("packed_gpu_generation_generate_pipeline".into()),
                layout: vec![generation_generate_layout_desc],
                shader: generation_shader.clone(),
                shader_defs: vec![],
                entry_point: Some(std::borrow::Cow::Borrowed("generate")),
                push_constant_ranges: vec![],
                zero_initialize_workgroup_memory: false,
            });
        let finalize_pipeline_id =
            pipeline_cache.queue_compute_pipeline(ComputePipelineDescriptor {
                label: Some("packed_gpu_generation_finalize_pipeline".into()),
                layout: vec![generation_finalize_layout_desc],
                shader: generation_shader,
                shader_defs: vec![],
                entry_point: Some(std::borrow::Cow::Borrowed("finalize")),
                push_constant_ranges: vec![],
                zero_initialize_workgroup_memory: false,
            });

        render_app.insert_resource(PackedQuadPipeline {
            pipeline_id,
            view_bind_group_layout,
            quad_bind_group_layout,
        });
        render_app.insert_resource(PackedQuadCullPipeline {
            pipeline_id: cull_pipeline_id,
            bind_group_layout: cull_bind_group_layout,
        });
        render_app.insert_resource(PackedQuadGenerationPipeline {
            generate_pipeline_id,
            finalize_pipeline_id,
            bind_group_layout: generation_bind_group_layout,
        });
        render_app.insert_resource(PackedQuadGpuTimestampProfiler::new(
            &render_device,
            &render_queue,
        ));

        // 3. Register Node in Render Graph
        let mut render_graph = render_app.world_mut().resource_mut::<RenderGraph>();
        if let Some(core_3d_graph) =
            render_graph.get_sub_graph_mut(bevy::core_pipeline::core_3d::graph::Core3d)
        {
            core_3d_graph.add_node(
                PackedQuadGenerationLabel,
                PackedQuadGenerationNode::default(),
            );
            core_3d_graph.add_node(PackedQuadCullLabel, PackedQuadCullNode::default());
            core_3d_graph.add_node(PackedQuadRenderLabel, PackedQuadRenderNode::default());
            // Insert right after Main Pass ends, before post-processing/tonemapping
            core_3d_graph.add_node_edge(
                bevy::core_pipeline::core_3d::graph::Node3d::EndMainPass,
                PackedQuadGenerationLabel,
            );
            core_3d_graph.add_node_edge(PackedQuadGenerationLabel, PackedQuadCullLabel);
            core_3d_graph.add_node_edge(PackedQuadCullLabel, PackedQuadRenderLabel);
            core_3d_graph.add_node_edge(
                PackedQuadRenderLabel,
                bevy::core_pipeline::core_3d::graph::Node3d::StartMainPassPostProcessing,
            );
        }
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

fn env_f32(name: &str) -> Option<f32> {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse::<f32>().ok())
        .filter(|value| value.is_finite())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packed_quad_view_buffer_uses_six_vec4_lanes() {
        assert_eq!(std::mem::size_of::<PackedQuadViewBuffer>(), 96);
        assert_eq!(std::mem::align_of::<PackedQuadViewBuffer>(), 4);
    }

    #[test]
    fn packed_gpu_generation_cull_metadata_uses_region_bounds() {
        let region = PreparedPackedGpuGeneratedRegion {
            key: 42,
            generation: 7,
            column_count: 16,
            max_output_quads: 1234,
            arena_offset_quads: 2048,
            arena_capacity_quads: 4096,
            draw_command_index: 3,
            bounds_min: Vec3::new(-32.0, 0.0, 64.0),
            bounds_max: Vec3::new(96.0, 128.0, 192.0),
        };

        let metadata = generated_region_cull_metadata(&region);
        assert_eq!(metadata.batch_key, 42);
        assert_eq!(metadata.face, None);
        assert_eq!(metadata.len_quads, 1234);
        assert_eq!(metadata.bounds_min, region.bounds_min);
        assert_eq!(metadata.bounds_max, region.bounds_max);

        let gpu_metadata =
            crate::packed_quad_pipeline::packed_gpu_cull_metadata_from_command(metadata);
        assert_eq!(gpu_metadata.bounds_min, [-32.0, 0.0, 64.0, 0.0]);
        assert_eq!(gpu_metadata.bounds_max, [96.0, 128.0, 192.0, 0.0]);
        assert_eq!(gpu_metadata.meta, [1234, 0, 0, 0]);
    }

    #[test]
    fn packed_gpu_generation_cull_signatures_track_uploaded_state() {
        fn test_region(generation: u64, bounds_max: Vec3) -> PreparedPackedGpuGeneratedRegion {
            PreparedPackedGpuGeneratedRegion {
                key: 42,
                generation,
                column_count: 16,
                max_output_quads: 1234,
                arena_offset_quads: 2048,
                arena_capacity_quads: 4096,
                draw_command_index: 3,
                bounds_min: Vec3::new(-32.0, 0.0, 64.0),
                bounds_max,
            }
        }

        assert_eq!(
            generated_regions_cull_metadata_signature(&[test_region(
                7,
                Vec3::new(96.0, 128.0, 192.0)
            )]),
            generated_regions_cull_metadata_signature(&[test_region(
                8,
                Vec3::new(96.0, 128.0, 192.0)
            )])
        );
        assert_ne!(
            generated_regions_cull_metadata_signature(&[test_region(
                7,
                Vec3::new(96.0, 128.0, 192.0)
            )]),
            generated_regions_cull_metadata_signature(&[test_region(
                7,
                Vec3::new(97.0, 128.0, 192.0)
            )])
        );

        let base_config = crate::packed_quad_buffer::PackedQuadCullConfig {
            command_count: 1,
            face_range_cull: 0,
            compact_output: 0,
            _padding: 0,
        };
        let compact_config = crate::packed_quad_buffer::PackedQuadCullConfig {
            compact_output: 1,
            ..base_config
        };
        assert_ne!(
            generated_cull_config_signature(base_config),
            generated_cull_config_signature(compact_config)
        );
    }

    #[test]
    fn packed_gpu_generation_prepared_matches_batches_for_render_prepare_skip() {
        let batch = PackedGpuGenerationBatch {
            key: 42,
            columns: std::sync::Arc::new(vec![
                crate::packed_quad_gpu_generation::PackedGpuSurfaceColumn::from_parts(
                    [0, 0, 1, 1],
                    [4, 4, 4, 4, 4],
                    2,
                ),
            ]),
            params: crate::packed_quad_gpu_generation::PackedGpuGenerationParams::new(
                1, 7, 0, 0, 1, 2, 3,
            ),
            source_chunk_count: 1,
            max_output_quads: 7,
            translation: Vec4::ZERO,
            bounds_min: Vec3::new(-1.0, 0.0, 2.0),
            bounds_max: Vec3::new(3.0, 4.0, 5.0),
            generation: 9,
        };
        let prepared = PreparedPackedGpuGeneratedDraw {
            enabled: true,
            regions: vec![PreparedPackedGpuGeneratedRegion {
                key: batch.key,
                generation: batch.generation,
                column_count: batch.columns.len(),
                max_output_quads: batch.max_output_quads,
                arena_offset_quads: 16,
                arena_capacity_quads: 32,
                draw_command_index: 0,
                bounds_min: batch.bounds_min,
                bounds_max: batch.bounds_max,
            }],
            arena_generation: 3,
            ..Default::default()
        };
        prepared.mark_dispatched();

        assert!(prepared.matches_batches(&[&batch], 3));

        let mut changed_generation = batch.clone();
        changed_generation.generation = changed_generation.generation.saturating_add(1);
        assert!(!prepared.matches_batches(&[&changed_generation], 3));

        let mut changed_bounds = batch.clone();
        changed_bounds.bounds_max.x += 1.0;
        assert!(!prepared.matches_batches(&[&changed_bounds], 3));
    }

    #[test]
    fn packed_quad_cull_shader_is_valid_wgsl() {
        let source = include_str!("../assets/shaders/packed_quad_cull.wgsl");
        let module =
            naga::front::wgsl::parse_str(source).expect("packed quad cull shader should parse");
        let mut validator = naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::empty(),
        );
        validator
            .validate(&module)
            .expect("packed quad cull shader should validate");
    }

    #[test]
    fn test_vertex_count_for_quads() {
        assert_eq!(vertex_count_for_quads(0), 0);
        assert_eq!(vertex_count_for_quads(1), 6);
        assert_eq!(vertex_count_for_quads(10), 60);
        // Overflow safety
        assert_eq!(vertex_count_for_quads(usize::MAX), u32::MAX);
    }

    #[test]
    fn test_aabb_intersects_identity_clip_frustum() {
        assert!(crate::packed_quad_pipeline::aabb_intersects_clip_frustum(
            Mat4::IDENTITY,
            Vec3::new(-0.5, -0.5, 0.25),
            Vec3::new(0.5, 0.5, 0.75),
        ));
        assert!(!crate::packed_quad_pipeline::aabb_intersects_clip_frustum(
            Mat4::IDENTITY,
            Vec3::new(2.0, -0.5, 0.25),
            Vec3::new(3.0, 0.5, 0.75),
        ));
    }

    #[test]
    fn test_collect_visible_indirect_commands_preserves_draw_command_order() {
        let draw_commands = vec![
            crate::packed_quad_buffer::PackedQuadDrawCommand {
                vertex_count: 60,
                instance_count: 1,
                first_vertex: 0,
                first_instance: 10,
            },
            crate::packed_quad_buffer::PackedQuadDrawCommand {
                vertex_count: 120,
                instance_count: 1,
                first_vertex: 60,
                first_instance: 11,
            },
        ];
        let metadata = vec![
            crate::packed_quad_pipeline::PackedQuadIndirectCommandMetadata {
                batch_key: 1,
                face: None,
                len_quads: 10,
                bounds_min: Vec3::new(-0.5, -0.5, 0.25),
                bounds_max: Vec3::new(0.5, 0.5, 0.75),
            },
            crate::packed_quad_pipeline::PackedQuadIndirectCommandMetadata {
                batch_key: 2,
                face: None,
                len_quads: 20,
                bounds_min: Vec3::new(2.0, -0.5, 0.25),
                bounds_max: Vec3::new(3.0, 0.5, 0.75),
            },
        ];

        let selection = collect_visible_indirect_commands(
            &metadata,
            &draw_commands,
            metadata.len(),
            Vec3::new(0.0, 0.0, -2.0),
            Mat4::IDENTITY,
            false,
        );

        assert_eq!(selection.indices, vec![0]);
        assert_eq!(selection.commands, vec![draw_commands[0]]);
        assert_eq!(selection.visible_batches, 1);
        assert_eq!(selection.visible_quads, 10);
        assert_eq!(selection.considered_commands, 2);
    }

    #[test]
    fn test_point_inside_bounds() {
        assert!(point_inside_bounds(
            Vec3::new(1.0, 2.0, 3.0),
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(4.0, 4.0, 4.0),
        ));
        assert!(!point_inside_bounds(
            Vec3::new(5.0, 2.0, 3.0),
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(4.0, 4.0, 4.0),
        ));
    }

    #[test]
    fn test_face_points_toward_view() {
        let bounds_min = Vec3::new(0.0, 0.0, 0.0);
        let bounds_max = Vec3::new(10.0, 10.0, 10.0);

        assert!(face_points_toward_view(
            0,
            Vec3::new(20.0, 5.0, 5.0),
            bounds_min,
            bounds_max,
        ));
        assert!(!face_points_toward_view(
            0,
            Vec3::new(-1.0, 5.0, 5.0),
            bounds_min,
            bounds_max,
        ));
        assert!(face_points_toward_view(
            3,
            Vec3::new(5.0, -1.0, 5.0),
            bounds_min,
            bounds_max,
        ));
        assert!(!face_points_toward_view(
            3,
            Vec3::new(5.0, 20.0, 5.0),
            bounds_min,
            bounds_max,
        ));
    }
}
