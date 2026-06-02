use bevy::prelude::*;
use bevy::render::extract_resource::ExtractResource;
use std::sync::Arc;

use crate::voxel_packed_quads::{PackedVoxelFace, PackedVoxelQuad};

pub const PACKED_GPU_GENERATION_ENV: &str = "RUMPEL_PACKED_GPU_GENERATION";
pub const PACKED_GPU_GENERATION_WORKGROUP_SIZE: usize = 64;
pub const PACKED_GPU_GENERATION_MAX_SIDE_SEGMENTS_PER_FACE: usize = 3;
pub const PACKED_GPU_GENERATION_MAX_QUADS_PER_COLUMN: usize =
    1 + 4 * PACKED_GPU_GENERATION_MAX_SIDE_SEGMENTS_PER_FACE;

#[derive(Resource, Default, Clone)]
pub struct PackedGpuGenerationBatches {
    pub batches: Vec<PackedGpuGenerationBatch>,
    pub target: Option<PackedGpuGenerationTarget>,
}

impl ExtractResource for PackedGpuGenerationBatches {
    type Source = Self;

    fn extract_resource(source: &Self::Source) -> Self {
        source.clone()
    }
}

#[derive(Debug, Clone)]
pub struct PackedGpuGenerationBatch {
    pub key: u64,
    pub columns: Arc<Vec<PackedGpuSurfaceColumn>>,
    pub params: PackedGpuGenerationParams,
    pub source_chunk_count: usize,
    pub max_output_quads: usize,
    pub translation: Vec4,
    pub bounds_min: Vec3,
    pub bounds_max: Vec3,
    pub generation: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PackedGpuGenerationTarget {
    pub camera_chunk_x: i32,
    pub camera_chunk_z: i32,
    pub center_origin_x: i32,
    pub center_origin_z: i32,
    pub region_size: i32,
    pub region_radius: i32,
    pub view_radius: i32,
    pub contract_generation: u64,
    pub edit_store_generation: u64,
}

impl PackedGpuGenerationTarget {
    #[allow(clippy::too_many_arguments)]
    #[must_use]
    pub fn new(
        camera_chunk_x: i32,
        camera_chunk_z: i32,
        center_origin_x: i32,
        center_origin_z: i32,
        region_size: i32,
        region_radius: i32,
        view_radius: i32,
        contract_generation: u64,
        edit_store_generation: u64,
    ) -> Self {
        Self {
            camera_chunk_x,
            camera_chunk_z,
            center_origin_x,
            center_origin_z,
            region_size,
            region_radius: region_radius.max(0),
            view_radius,
            contract_generation,
            edit_store_generation,
        }
    }

    #[must_use]
    pub fn loaded_regions(self) -> usize {
        let side = self
            .region_radius
            .saturating_mul(2)
            .saturating_add(1)
            .max(1) as usize;
        side.saturating_mul(side)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PackedGpuGenerationCacheContract {
    pub region_size_chunks: u32,
    pub cell_size: u32,
    pub lod: u32,
    pub worldgen_contract_version: u64,
    pub material_contract_version: u64,
    pub terrain_palette: [u32; 4],
    pub surface_top_material: u32,
}

impl PackedGpuGenerationCacheContract {
    #[must_use]
    pub fn new(
        region_size_chunks: i32,
        requested_cell_size: usize,
        terrain_palette: [u16; 4],
        surface_top_material: u16,
        worldgen_contract_version: u64,
        material_contract_version: u64,
    ) -> Self {
        let cell_size = normalize_surface_cell_size(requested_cell_size);
        Self {
            region_size_chunks: region_size_chunks.max(1) as u32,
            cell_size: cell_size as u32,
            lod: u32::from(packed_gpu_generation_lod_for_cell_size(cell_size)),
            worldgen_contract_version,
            material_contract_version,
            terrain_palette: terrain_palette.map(u32::from),
            surface_top_material: u32::from(surface_top_material),
        }
    }

    #[must_use]
    pub fn generation(&self) -> u64 {
        let mut hash = FNV64_OFFSET;
        hash = fnv64(hash, self.worldgen_contract_version);
        hash = fnv64(hash, self.material_contract_version);
        hash = fnv64(hash, u64::from(self.region_size_chunks));
        hash = fnv64(hash, u64::from(self.cell_size));
        hash = fnv64(hash, u64::from(self.lod));
        for block_id in self.terrain_palette {
            hash = fnv64(hash, u64::from(block_id));
        }
        hash = fnv64(hash, u64::from(self.surface_top_material));
        hash.max(1)
    }
}

#[derive(Debug, Clone)]
pub struct GeneratedRegionCacheEntry {
    pub key: u64,
    pub columns: Arc<Vec<PackedGpuSurfaceColumn>>,
    pub params: PackedGpuGenerationParams,
    pub source_chunk_count: usize,
    pub max_output_quads: usize,
    pub translation: Vec4,
    pub bounds_min: Vec3,
    pub bounds_max: Vec3,
    pub generation: u64,
    pub contract: PackedGpuGenerationCacheContract,
    pub edit_store_generation: u64,
    pub last_seen_frame: u64,
}

impl GeneratedRegionCacheEntry {
    #[must_use]
    pub fn to_batch(&self) -> PackedGpuGenerationBatch {
        PackedGpuGenerationBatch {
            key: self.key,
            columns: Arc::clone(&self.columns),
            params: self.params,
            source_chunk_count: self.source_chunk_count,
            max_output_quads: self.max_output_quads,
            translation: self.translation,
            bounds_min: self.bounds_min,
            bounds_max: self.bounds_max,
            generation: self.generation,
        }
    }
}

#[derive(Resource, Default, Clone)]
pub struct GeneratedRegionCache {
    pub entries: std::collections::HashMap<u64, GeneratedRegionCacheEntry>,
    pub frame: u64,
}

impl GeneratedRegionCache {
    #[must_use]
    pub fn next_frame(&mut self) -> u64 {
        self.frame = self.frame.saturating_add(1).max(1);
        self.frame
    }
}

/// Returns true when at least one chunk inside the packed region intersects the circular view radius.
#[must_use]
pub fn region_has_active_chunks(
    region_origin_x: i32,
    region_origin_z: i32,
    region_size: i32,
    center_chunk: IVec2,
    view_radius: i32,
) -> bool {
    let region_size = region_size.max(1);
    let radius_sq = i64::from(view_radius.max(0)).pow(2);
    for chunk_z in region_origin_z..region_origin_z + region_size {
        for chunk_x in region_origin_x..region_origin_x + region_size {
            let dx = i64::from(chunk_x - center_chunk.x);
            let dz = i64::from(chunk_z - center_chunk.y);
            if dx * dx + dz * dz <= radius_sq {
                return true;
            }
        }
    }
    false
}

const FNV64_OFFSET: u64 = 14_695_981_039_346_656_037;
const FNV64_PRIME: u64 = 1_099_511_628_211;

fn fnv64(hash: u64, value: u64) -> u64 {
    (hash ^ value).wrapping_mul(FNV64_PRIME)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct PackedGpuGenerationParams {
    /// x: column_count, y: max_output_quads, z: lod, w: reserved.
    pub config: [u32; 4],
    /// x: air, y: dirt, z: grass, w: stone.
    pub palette: [u32; 4],
}

unsafe impl bytemuck::Zeroable for PackedGpuGenerationParams {}
unsafe impl bytemuck::Pod for PackedGpuGenerationParams {}

impl PackedGpuGenerationParams {
    #[must_use]
    pub fn new(
        column_count: usize,
        max_output_quads: usize,
        lod: u8,
        air: u16,
        dirt: u16,
        grass: u16,
        stone: u16,
    ) -> Self {
        Self {
            config: [
                column_count.min(u32::MAX as usize) as u32,
                max_output_quads.min(u32::MAX as usize) as u32,
                u32::from(lod),
                0,
            ],
            palette: [
                u32::from(air),
                u32::from(dirt),
                u32::from(grass),
                u32::from(stone),
            ],
        }
    }

    #[inline]
    #[must_use]
    pub fn column_count(&self) -> usize {
        self.config[0] as usize
    }

    #[inline]
    #[must_use]
    pub fn max_output_quads(&self) -> usize {
        self.config[1] as usize
    }

    #[inline]
    #[must_use]
    pub fn lod(&self) -> u32 {
        self.config[2]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct PackedGpuGenerationJob {
    /// x: column_offset, y: column_count, z: output_quad_offset, w: max_output_quads.
    pub source: [u32; 4],
    /// x: counter_index, y: draw_command_index, z: draw_param_index, w: lod.
    pub output: [u32; 4],
    /// x: air, y: dirt, z: grass, w: stone.
    pub palette: [u32; 4],
}

unsafe impl bytemuck::Zeroable for PackedGpuGenerationJob {}
unsafe impl bytemuck::Pod for PackedGpuGenerationJob {}

impl PackedGpuGenerationJob {
    #[must_use]
    pub fn new(
        params: PackedGpuGenerationParams,
        column_offset: usize,
        output_quad_offset: usize,
        counter_index: usize,
        draw_command_index: usize,
        draw_param_index: usize,
    ) -> Self {
        Self {
            source: [
                column_offset.min(u32::MAX as usize) as u32,
                params.column_count().min(u32::MAX as usize) as u32,
                output_quad_offset.min(u32::MAX as usize) as u32,
                params.max_output_quads().min(u32::MAX as usize) as u32,
            ],
            output: [
                counter_index.min(u32::MAX as usize) as u32,
                draw_command_index.min(u32::MAX as usize) as u32,
                draw_param_index.min(u32::MAX as usize) as u32,
                params.lod(),
            ],
            palette: params.palette,
        }
    }

    #[inline]
    #[must_use]
    pub fn column_count(&self) -> usize {
        self.source[1] as usize
    }

    #[inline]
    #[must_use]
    pub fn max_output_quads(&self) -> usize {
        self.source[3] as usize
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct PackedGpuSurfaceColumn {
    /// x: local_x, y: local_z, z: width, w: depth.
    pub local: [u32; 4],
    /// x: own height, y: plus_x height, z: minus_x height, w: plus_z height.
    pub heights: [u32; 4],
    /// x: minus_z height, y: top block id, z/w reserved.
    pub material: [u32; 4],
}

unsafe impl bytemuck::Zeroable for PackedGpuSurfaceColumn {}
unsafe impl bytemuck::Pod for PackedGpuSurfaceColumn {}

impl PackedGpuSurfaceColumn {
    #[must_use]
    pub fn from_parts(local: [usize; 4], heights: [usize; 5], top_block: u16) -> Self {
        Self {
            local: [
                local[0].min(u32::MAX as usize) as u32,
                local[1].min(u32::MAX as usize) as u32,
                local[2].min(u32::MAX as usize) as u32,
                local[3].min(u32::MAX as usize) as u32,
            ],
            heights: [
                heights[0].min(u32::MAX as usize) as u32,
                heights[1].min(u32::MAX as usize) as u32,
                heights[2].min(u32::MAX as usize) as u32,
                heights[3].min(u32::MAX as usize) as u32,
            ],
            material: [
                heights[4].min(u32::MAX as usize) as u32,
                u32::from(top_block),
                0,
                0,
            ],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct PackedGpuGenerationCounter {
    pub emitted_quads: u32,
    pub dropped_quads: u32,
    pub _padding: [u32; 2],
}

unsafe impl bytemuck::Zeroable for PackedGpuGenerationCounter {}
unsafe impl bytemuck::Pod for PackedGpuGenerationCounter {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PackedGpuGenerationLayout {
    pub chunk_count: usize,
    pub cell_size: usize,
    pub columns_per_chunk: usize,
    pub max_quads_per_chunk: usize,
    pub output_capacity_quads: usize,
    pub output_buffer_bytes: u64,
    pub column_buffer_bytes: u64,
}

impl PackedGpuGenerationLayout {
    #[must_use]
    pub fn for_chunks(chunk_count: usize, requested_cell_size: usize) -> Self {
        let cell_size = normalize_surface_cell_size(requested_cell_size);
        let columns_per_chunk = packed_gpu_generation_columns_per_chunk(cell_size);
        let max_quads_per_chunk = packed_gpu_generation_max_quads_per_chunk(cell_size);
        let output_capacity_quads = columns_per_chunk
            .saturating_mul(PACKED_GPU_GENERATION_MAX_QUADS_PER_COLUMN)
            .saturating_mul(chunk_count);
        let total_columns = columns_per_chunk.saturating_mul(chunk_count);

        Self {
            chunk_count,
            cell_size,
            columns_per_chunk,
            max_quads_per_chunk,
            output_capacity_quads,
            output_buffer_bytes: byte_len_for_items::<PackedVoxelQuad>(output_capacity_quads),
            column_buffer_bytes: byte_len_for_items::<PackedGpuSurfaceColumn>(total_columns),
        }
    }
}

#[must_use]
pub fn packed_gpu_generation_enabled_from_env() -> bool {
    std::env::var(PACKED_GPU_GENERATION_ENV).is_ok_and(|value| {
        matches!(
            value.to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        )
    })
}

#[must_use]
pub fn normalize_surface_cell_size(requested_cell_size: usize) -> usize {
    requested_cell_size.clamp(1, rumpel_world::chunk::CHUNK_SIZE)
}

#[must_use]
pub fn packed_gpu_generation_columns_per_chunk(requested_cell_size: usize) -> usize {
    let cell_size = normalize_surface_cell_size(requested_cell_size);
    let cells_per_axis = rumpel_world::chunk::CHUNK_SIZE.div_ceil(cell_size);
    cells_per_axis * cells_per_axis
}

#[must_use]
pub fn packed_gpu_generation_max_quads_per_chunk(requested_cell_size: usize) -> usize {
    packed_gpu_generation_columns_per_chunk(requested_cell_size)
        * PACKED_GPU_GENERATION_MAX_QUADS_PER_COLUMN
}

#[must_use]
pub fn packed_gpu_generation_lod_for_cell_size(requested_cell_size: usize) -> u8 {
    match normalize_surface_cell_size(requested_cell_size) {
        1 => 0,
        2 => 1,
        3 | 4 => 2,
        _ => 3,
    }
}

#[must_use]
pub fn packed_gpu_generation_workgroups(column_count: usize) -> u32 {
    column_count
        .div_ceil(PACKED_GPU_GENERATION_WORKGROUP_SIZE)
        .min(u32::MAX as usize) as u32
}

#[must_use]
pub fn packed_voxel_quad_words(quad: PackedVoxelQuad) -> [u32; 4] {
    [
        u32::from(quad.origin[0]) | (u32::from(quad.origin[1]) << 16),
        u32::from(quad.origin[2]) | (u32::from(quad.size[0]) << 16),
        u32::from(quad.size[1]) | (u32::from(quad.block_id) << 16),
        quad.meta,
    ]
}

#[must_use]
pub fn packed_gpu_generation_top_quad(
    local_x: u16,
    local_z: u16,
    width: u16,
    depth: u16,
    height: u16,
    block_id: u16,
    lod: u8,
) -> PackedVoxelQuad {
    PackedVoxelQuad::new(
        [local_x, height.saturating_sub(1), local_z],
        [width, depth],
        block_id,
        PackedVoxelFace::PlusY as u8,
        lod,
        0,
    )
}

fn byte_len_for_items<T>(items: usize) -> u64 {
    items
        .checked_mul(std::mem::size_of::<T>())
        .and_then(|bytes| u64::try_from(bytes).ok())
        .unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packed_gpu_generation_host_structs_are_storage_safe() {
        assert_eq!(std::mem::size_of::<PackedGpuGenerationParams>(), 32);
        assert_eq!(std::mem::align_of::<PackedGpuGenerationParams>(), 4);
        assert_eq!(std::mem::size_of::<PackedGpuGenerationJob>(), 48);
        assert_eq!(std::mem::align_of::<PackedGpuGenerationJob>(), 4);
        assert_eq!(std::mem::size_of::<PackedGpuSurfaceColumn>(), 48);
        assert_eq!(std::mem::align_of::<PackedGpuSurfaceColumn>(), 4);
        assert_eq!(std::mem::size_of::<PackedGpuGenerationCounter>(), 16);
        assert_eq!(std::mem::align_of::<PackedGpuGenerationCounter>(), 4);
    }

    #[test]
    fn packed_gpu_generation_capacity_tracks_surface_lod_cell_size() {
        let full_res = PackedGpuGenerationLayout::for_chunks(3, 1);
        assert_eq!(full_res.columns_per_chunk, 32 * 32);
        assert_eq!(
            full_res.max_quads_per_chunk,
            32 * 32 * PACKED_GPU_GENERATION_MAX_QUADS_PER_COLUMN
        );
        assert_eq!(
            full_res.output_capacity_quads,
            full_res.max_quads_per_chunk * 3
        );
        assert_eq!(
            full_res.output_buffer_bytes,
            (full_res.output_capacity_quads * std::mem::size_of::<PackedVoxelQuad>()) as u64
        );

        let coarse = PackedGpuGenerationLayout::for_chunks(2, 8);
        assert_eq!(coarse.columns_per_chunk, 4 * 4);
        assert_eq!(
            coarse.column_buffer_bytes,
            (coarse.columns_per_chunk * 2 * std::mem::size_of::<PackedGpuSurfaceColumn>()) as u64
        );
    }

    #[test]
    fn packed_gpu_generation_workgroup_math_is_bounded() {
        assert_eq!(packed_gpu_generation_workgroups(0), 0);
        assert_eq!(packed_gpu_generation_workgroups(1), 1);
        assert_eq!(packed_gpu_generation_workgroups(64), 1);
        assert_eq!(packed_gpu_generation_workgroups(65), 2);
        assert_eq!(packed_gpu_generation_workgroups(usize::MAX), u32::MAX);
    }

    #[test]
    fn packed_gpu_generation_cache_contract_generation_tracks_inputs() {
        let base = PackedGpuGenerationCacheContract::new(4, 2, [0, 1, 2, 3], 4, 10, 20);
        let same = PackedGpuGenerationCacheContract::new(4, 2, [0, 1, 2, 3], 4, 10, 20);

        assert_eq!(base, same);
        assert_eq!(base.generation(), same.generation());
        assert_ne!(base.generation(), 0);
        assert_eq!(base.cell_size, 2);
        assert_eq!(base.lod, 1);

        assert_ne!(
            base.generation(),
            PackedGpuGenerationCacheContract::new(4, 4, [0, 1, 2, 3], 4, 10, 20).generation()
        );
        assert_ne!(
            base.generation(),
            PackedGpuGenerationCacheContract::new(8, 2, [0, 1, 2, 3], 4, 10, 20).generation()
        );
        assert_ne!(
            base.generation(),
            PackedGpuGenerationCacheContract::new(4, 2, [0, 1, 9, 3], 4, 10, 20).generation()
        );
        assert_ne!(
            base.generation(),
            PackedGpuGenerationCacheContract::new(4, 2, [0, 1, 2, 3], 9, 10, 20).generation()
        );
    }

    #[test]
    fn packed_gpu_generation_cache_contract_generation_tracks_contract_versions() {
        let base = PackedGpuGenerationCacheContract::new(4, 2, [0, 1, 2, 3], 4, 1, 1);

        assert_ne!(
            base.generation(),
            PackedGpuGenerationCacheContract::new(4, 2, [0, 1, 2, 3], 4, 2, 1).generation()
        );
        assert_ne!(
            base.generation(),
            PackedGpuGenerationCacheContract::new(4, 2, [0, 1, 2, 3], 4, 1, 2).generation()
        );
    }

    #[test]
    fn packed_gpu_generation_job_tracks_region_offsets() {
        let params = PackedGpuGenerationParams::new(128, 512, 2, 0, 1, 2, 3);
        let job = PackedGpuGenerationJob::new(params, 64, 1024, 5, 6, 7);

        assert_eq!(job.source, [64, 128, 1024, 512]);
        assert_eq!(job.output, [5, 6, 7, 2]);
        assert_eq!(job.palette, [0, 1, 2, 3]);
        assert_eq!(job.column_count(), 128);
        assert_eq!(job.max_output_quads(), 512);
    }

    #[test]
    fn packed_gpu_generation_packs_same_quad_words_as_vertex_pulling_shader() {
        let quad = PackedVoxelQuad::new([1, 2, 3], [4, 5], 42, 2, 1, 100);
        let bytes = bytemuck::bytes_of(&quad);
        let raw_words = [
            u32::from_le_bytes(bytes[0..4].try_into().expect("word 0 bytes")),
            u32::from_le_bytes(bytes[4..8].try_into().expect("word 1 bytes")),
            u32::from_le_bytes(bytes[8..12].try_into().expect("word 2 bytes")),
            u32::from_le_bytes(bytes[12..16].try_into().expect("word 3 bytes")),
        ];

        assert_eq!(packed_voxel_quad_words(quad), raw_words);
        assert_eq!(
            packed_voxel_quad_words(quad),
            [131_073, 262_147, 2_752_517, 25_600 + 10]
        );
    }

    #[test]
    fn packed_gpu_generation_top_quad_matches_packed_abi() {
        let quad = packed_gpu_generation_top_quad(4, 6, 8, 10, 25, 7, 2);
        assert_eq!(quad.origin, [4, 24, 6]);
        assert_eq!(quad.size, [8, 10]);
        assert_eq!(quad.block_id, 7);
        assert_eq!(quad.face(), PackedVoxelFace::PlusY as u8);
        assert_eq!(quad.lod(), 2);
    }

    #[test]
    fn packed_gpu_generation_target_tracks_stable_window_signature() {
        let base = PackedGpuGenerationTarget::new(1, 2, 0, 0, 4, 1, 16, 10, 20);
        let same = PackedGpuGenerationTarget::new(1, 2, 0, 0, 4, 1, 16, 10, 20);
        let moved = PackedGpuGenerationTarget::new(2, 2, 0, 0, 4, 1, 16, 10, 20);
        let edited = PackedGpuGenerationTarget::new(1, 2, 0, 0, 4, 1, 16, 10, 21);

        assert_eq!(base, same);
        assert_ne!(base, moved);
        assert_ne!(base, edited);
        assert_eq!(base.loaded_regions(), 9);
        assert_eq!(
            PackedGpuGenerationTarget::new(1, 2, 0, 0, 4, 0, 16, 10, 20).loaded_regions(),
            1
        );
    }

    #[test]
    fn region_has_active_chunks_respects_view_radius() {
        let center = IVec2::new(0, 0);
        assert!(region_has_active_chunks(0, 0, 4, center, 16));
        assert!(!region_has_active_chunks(64, 64, 4, center, 8));
    }

    #[test]
    fn region_has_active_chunks_detects_partial_region_overlap() {
        let center = IVec2::new(0, 0);
        assert!(region_has_active_chunks(6, 0, 4, center, 8));
        assert!(!region_has_active_chunks(10, 0, 4, center, 8));
    }

    #[test]
    fn packed_gpu_generation_shader_is_valid_wgsl() {
        let source = include_str!("../assets/shaders/packed_quad_generate.wgsl");
        let module = naga::front::wgsl::parse_str(source)
            .expect("packed quad generation shader should parse as WGSL");
        let mut validator = naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::empty(),
        );
        validator
            .validate(&module)
            .expect("packed quad generation shader should validate");
    }
}
