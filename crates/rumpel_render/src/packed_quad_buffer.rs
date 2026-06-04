use crate::voxel_packed_quads::PackedVoxelQuad;
use bevy::render::render_resource::{Buffer, BufferDescriptor, BufferUsages};
use bevy::render::renderer::{RenderDevice, RenderQueue};
use std::collections::HashMap;

/// Configuration descriptor for creating a `PackedQuadBuffer`.
#[derive(Debug, Clone)]
pub struct PackedQuadBufferDescriptor<'a> {
    /// Maximum number of quads this GPU buffer can hold.
    pub capacity_quads: usize,
    /// Label for debugging purposes in the GPU debugger.
    pub label: &'a str,
}

/// Dynamic GPU upload layer for packed voxel quads.
/// Allocates and manages a `wgpu` / Bevy `RenderDevice` GPU storage buffer
/// for safe copy-free quad uploading.
pub struct PackedQuadBuffer {
    capacity_quads: usize,
    len_quads: usize,
    buffer: Buffer,
    label: String,
}

/// Statistics returned after uploading quads to the GPU.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PackedQuadUploadStats {
    /// The total number of quads requested to be uploaded.
    pub requested_quads: usize,
    /// The number of quads actually written to the GPU buffer.
    pub uploaded_quads: usize,
    /// The number of quads that were discarded due to buffer capacity overflow.
    pub dropped_quads: usize,
    /// The total number of bytes written to the GPU buffer.
    pub uploaded_bytes: u64,
}

impl PackedQuadBuffer {
    /// Creates a new `PackedQuadBuffer` with the specified capacity and label.
    pub fn new(render_device: &RenderDevice, capacity_quads: usize, label: &str) -> Self {
        let size_bytes = packed_quad_buffer_size_bytes(capacity_quads);

        // We include:
        // - STORAGE usage for vertex pulling / MDI shader access.
        // - COPY_DST usage for CPU-to-GPU data uploads using write_buffer.
        // - COPY_SRC usage to support future operations like host-side copybacks or GPU debugging.
        let usage = BufferUsages::STORAGE | BufferUsages::COPY_DST | BufferUsages::COPY_SRC;

        let buffer = render_device.create_buffer(&BufferDescriptor {
            label: Some(label),
            size: size_bytes,
            usage,
            mapped_at_creation: false,
        });

        Self {
            capacity_quads,
            len_quads: 0,
            buffer,
            label: label.to_string(),
        }
    }

    /// Returns the maximum capacity of quads this buffer can store.
    #[inline]
    pub fn capacity_quads(&self) -> usize {
        self.capacity_quads
    }

    /// Returns the number of quads currently loaded into the buffer.
    #[inline]
    pub fn len_quads(&self) -> usize {
        self.len_quads
    }

    /// Returns a reference to Bevy's native `Buffer` resource.
    #[inline]
    pub fn buffer(&self) -> &Buffer {
        &self.buffer
    }

    /// Returns the debugging label of the buffer.
    #[inline]
    pub fn label(&self) -> &str {
        &self.label
    }

    /// Uploads a slice of `PackedVoxelQuad` to the GPU storage buffer.
    /// Safely handles overflow without panicking by dropping excess quads.
    pub fn upload(
        &mut self,
        render_queue: &RenderQueue,
        quads: &[PackedVoxelQuad],
    ) -> PackedQuadUploadStats {
        let requested_quads = quads.len();

        // Calculate the safe upload size under the allocated capacity limit
        let uploaded_quads = requested_quads.min(self.capacity_quads);
        let dropped_quads = requested_quads.saturating_sub(uploaded_quads);

        let size_of_quad = std::mem::size_of::<PackedVoxelQuad>();
        let uploaded_bytes = (uploaded_quads * size_of_quad) as u64;

        if uploaded_quads > 0 {
            let data_slice = &quads[..uploaded_quads];
            render_queue.write_buffer(&self.buffer, 0, bytemuck::cast_slice(data_slice));
        }

        self.len_quads = uploaded_quads;

        PackedQuadUploadStats {
            requested_quads,
            uploaded_quads,
            dropped_quads,
            uploaded_bytes,
        }
    }
}

/// Helper function to calculate the byte size required for a given quad capacity.
#[inline]
pub fn packed_quad_buffer_size_bytes(capacity_quads: usize) -> u64 {
    (capacity_quads as u64) * (std::mem::size_of::<PackedVoxelQuad>() as u64)
}

/// Represents an allocated block inside the GPU PackedVoxelQuad Arena.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PackedQuadArenaAllocation {
    /// Unique identifier (chunk key) of the batch.
    pub key: u64,
    /// Starting offset of this batch in the storage buffer (in quads).
    pub offset_quads: usize,
    /// Number of active quads in this batch.
    pub len_quads: usize,
    /// Allocated capacity of this batch block (in quads).
    pub capacity_quads: usize,
    /// Generation counter at the time of allocation/upload.
    pub generation: u64,
}

/// Reclaimed arena slot available for reuse after a region is evicted or resized.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PackedQuadArenaFreeSlot {
    pub offset_quads: usize,
    pub capacity_quads: usize,
}

/// Region allocation request for GPU-generated packed arena planning.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PackedGpuGenerationAllocationRequest {
    pub key: u64,
    pub requested_quads: usize,
    pub generation: u64,
}

/// Reusable scratch storage for GPU-generated arena allocation planning.
#[derive(Debug, Default)]
pub struct PackedGpuGenerationArenaAllocationPlanScratch {
    pub remaining_existing: HashMap<u64, PackedQuadArenaAllocation>,
    pub free_slots: Vec<PackedQuadArenaFreeSlot>,
}

/// Plans stable GPU-generated region slots with free-list reuse.
///
/// Evicted regions and undersized slots are returned to a free list. New regions
/// take the smallest fitting free slot before appending at the arena high-water mark.
/// The returned high-water mark is derived from active allocations only.
pub fn plan_gpu_generated_arena_allocations(
    existing_allocations: &HashMap<u64, PackedQuadArenaAllocation>,
    requests: &[PackedGpuGenerationAllocationRequest],
    slot_capacity: impl Fn(usize) -> usize,
) -> (HashMap<u64, PackedQuadArenaAllocation>, usize) {
    let mut sorted_requests = requests.to_vec();
    sorted_requests.sort_unstable_by_key(|request| request.key);
    plan_gpu_generated_arena_allocations_sorted(
        existing_allocations,
        &sorted_requests,
        slot_capacity,
    )
}

/// Plans stable GPU-generated region slots for requests already sorted by key.
///
/// This is the render-prepare hot path variant; callers that cannot guarantee
/// sorted input should use `plan_gpu_generated_arena_allocations`.
pub fn plan_gpu_generated_arena_allocations_sorted(
    existing_allocations: &HashMap<u64, PackedQuadArenaAllocation>,
    sorted_requests: &[PackedGpuGenerationAllocationRequest],
    slot_capacity: impl Fn(usize) -> usize,
) -> (HashMap<u64, PackedQuadArenaAllocation>, usize) {
    let mut new_allocations = HashMap::with_capacity(sorted_requests.len());
    let mut scratch = PackedGpuGenerationArenaAllocationPlanScratch::default();
    let allocated_slot_quads = plan_gpu_generated_arena_allocations_sorted_into(
        existing_allocations,
        sorted_requests,
        slot_capacity,
        &mut new_allocations,
        &mut scratch,
    );
    (new_allocations, allocated_slot_quads)
}

/// Plans stable GPU-generated region slots into reused output and scratch storage.
///
/// This is intended for render-prepare hot paths where sorted requests and
/// persistent scratch buffers are available.
#[must_use]
pub fn gpu_generated_allocation_maps_equivalent(
    existing: &HashMap<u64, PackedQuadArenaAllocation>,
    planned: &HashMap<u64, PackedQuadArenaAllocation>,
) -> bool {
    if existing.len() != planned.len() {
        return false;
    }
    existing.iter().all(|(key, existing_allocation)| {
        planned.get(key).is_some_and(|planned_allocation| {
            existing_allocation.offset_quads == planned_allocation.offset_quads
                && existing_allocation.len_quads == planned_allocation.len_quads
                && existing_allocation.capacity_quads == planned_allocation.capacity_quads
                && existing_allocation.generation == planned_allocation.generation
        })
    })
}

pub fn plan_gpu_generated_arena_allocations_sorted_into(
    existing_allocations: &HashMap<u64, PackedQuadArenaAllocation>,
    sorted_requests: &[PackedGpuGenerationAllocationRequest],
    slot_capacity: impl Fn(usize) -> usize,
    new_allocations: &mut HashMap<u64, PackedQuadArenaAllocation>,
    scratch: &mut PackedGpuGenerationArenaAllocationPlanScratch,
) -> usize {
    debug_assert!(
        sorted_requests
            .windows(2)
            .all(|window| window[0].key <= window[1].key)
    );
    new_allocations.clear();
    if new_allocations.capacity() < sorted_requests.len() {
        new_allocations.reserve(sorted_requests.len() - new_allocations.capacity());
    }

    scratch.remaining_existing.clear();
    let remaining_capacity = existing_allocations.len().min(sorted_requests.len());
    if scratch.remaining_existing.capacity() < remaining_capacity {
        scratch
            .remaining_existing
            .reserve(remaining_capacity - scratch.remaining_existing.capacity());
    }

    scratch.free_slots.clear();
    if scratch.free_slots.capacity() < existing_allocations.len() {
        scratch
            .free_slots
            .reserve(existing_allocations.len() - scratch.free_slots.capacity());
    }

    for (key, allocation) in existing_allocations {
        if sorted_requests
            .binary_search_by_key(key, |request| request.key)
            .is_ok()
        {
            scratch.remaining_existing.insert(*key, *allocation);
        } else {
            scratch.free_slots.push(PackedQuadArenaFreeSlot {
                offset_quads: allocation.offset_quads,
                capacity_quads: allocation.capacity_quads,
            });
        }
    }

    for request in sorted_requests.iter().copied() {
        let requested_quads = request.requested_quads.max(1);

        if let Some(existing) = scratch.remaining_existing.remove(&request.key) {
            if requested_quads <= existing.capacity_quads {
                new_allocations.insert(
                    request.key,
                    PackedQuadArenaAllocation {
                        key: request.key,
                        offset_quads: existing.offset_quads,
                        len_quads: requested_quads,
                        capacity_quads: existing.capacity_quads,
                        generation: request.generation,
                    },
                );
                continue;
            }
            scratch.free_slots.push(PackedQuadArenaFreeSlot {
                offset_quads: existing.offset_quads,
                capacity_quads: existing.capacity_quads,
            });
        }

        let requested_capacity_quads = slot_capacity(requested_quads);
        let (offset_quads, capacity_quads) = if let Some(reused_slot) =
            take_best_fit_arena_free_slot(&mut scratch.free_slots, requested_capacity_quads)
        {
            (reused_slot.offset_quads, reused_slot.capacity_quads)
        } else {
            (
                arena_slot_high_water_mark(new_allocations, &scratch.free_slots),
                requested_capacity_quads,
            )
        };

        new_allocations.insert(
            request.key,
            PackedQuadArenaAllocation {
                key: request.key,
                offset_quads,
                len_quads: requested_quads,
                capacity_quads,
                generation: request.generation,
            },
        );
    }

    active_arena_slot_high_water_mark(new_allocations)
}

fn take_best_fit_arena_free_slot(
    free_slots: &mut Vec<PackedQuadArenaFreeSlot>,
    required_capacity_quads: usize,
) -> Option<PackedQuadArenaFreeSlot> {
    let best_index = free_slots
        .iter()
        .enumerate()
        .filter(|(_, slot)| slot.capacity_quads >= required_capacity_quads)
        .min_by_key(|(_, slot)| (slot.capacity_quads, slot.offset_quads))
        .map(|(index, _)| index)?;

    Some(free_slots.swap_remove(best_index))
}

fn arena_slot_high_water_mark(
    allocations: &HashMap<u64, PackedQuadArenaAllocation>,
    free_slots: &[PackedQuadArenaFreeSlot],
) -> usize {
    allocations
        .values()
        .map(|allocation| {
            allocation
                .offset_quads
                .saturating_add(allocation.capacity_quads)
        })
        .chain(
            free_slots
                .iter()
                .map(|slot| slot.offset_quads.saturating_add(slot.capacity_quads)),
        )
        .max()
        .unwrap_or(0)
}

fn active_arena_slot_high_water_mark(
    allocations: &HashMap<u64, PackedQuadArenaAllocation>,
) -> usize {
    allocations
        .values()
        .map(|allocation| {
            allocation
                .offset_quads
                .saturating_add(allocation.capacity_quads)
        })
        .max()
        .unwrap_or(0)
}

/// Statistics for the GPU PackedVoxelQuad Arena.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PackedQuadArenaStats {
    /// Total capacity of the GPU storage buffer (in quads).
    pub total_capacity_quads: usize,
    /// Number of quads currently in use across all allocations.
    pub used_quads: usize,
    /// Number of quads occupied by stable allocation slots before the free tail.
    pub allocated_slot_quads: usize,
    /// Number of free quads in the arena (total_capacity - used).
    pub free_quads: usize,
    /// Total number of bytes currently uploaded/written to the GPU.
    pub uploaded_bytes: u64,
    /// Cumulative count of GPU storage buffer reallocations.
    pub reallocations: usize,
    /// Cumulative count of in-place allocation compactions.
    pub compactions: usize,
}

/// A standard WebGPU/wgpu indirect draw command for non-indexed rendering.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(C)]
pub struct PackedQuadDrawCommand {
    /// Number of vertices to draw (quads * 6).
    pub vertex_count: u32,
    /// Number of instances to draw (always 1).
    pub instance_count: u32,
    /// Index of the first vertex to draw (always 0).
    pub first_vertex: u32,
    /// Parameter array index passed as instance_index to the vertex shader.
    pub first_instance: u32,
}

unsafe impl bytemuck::Zeroable for PackedQuadDrawCommand {}
unsafe impl bytemuck::Pod for PackedQuadDrawCommand {}

/// GPU structured buffer element storing translation and offset for a chunk batch.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
#[repr(C)]
pub struct PackedQuadDrawParams {
    /// Physical chunk world translation (x, y, z) and base quad offset (w).
    pub chunk_offset: [f32; 4],
}

unsafe impl bytemuck::Zeroable for PackedQuadDrawParams {}
unsafe impl bytemuck::Pod for PackedQuadDrawParams {}

/// GPU-side coarse culling metadata for one packed indirect draw command.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
#[repr(C)]
pub struct PackedQuadCullCommandMetadata {
    /// Conservative world-space AABB minimum plus padding.
    pub bounds_min: [f32; 4],
    /// Conservative world-space AABB maximum plus padding.
    pub bounds_max: [f32; 4],
    /// x: len_quads, y: face + 1 or 0 when unsplit, z/w reserved.
    pub meta: [u32; 4],
}

unsafe impl bytemuck::Zeroable for PackedQuadCullCommandMetadata {}
unsafe impl bytemuck::Pod for PackedQuadCullCommandMetadata {}

/// GPU-side culling config.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(C)]
pub struct PackedQuadCullConfig {
    pub command_count: u32,
    pub face_range_cull: u32,
    pub compact_output: u32,
    pub _padding: u32,
}

unsafe impl bytemuck::Zeroable for PackedQuadCullConfig {}
unsafe impl bytemuck::Pod for PackedQuadCullConfig {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_packed_quad_buffer_size_bytes() {
        assert_eq!(packed_quad_buffer_size_bytes(0), 0);
        assert_eq!(packed_quad_buffer_size_bytes(1), 16);
        assert_eq!(packed_quad_buffer_size_bytes(1024), 16384);
    }

    #[test]
    fn test_upload_stats_calculation() {
        // Test empty slice
        let capacity = 10;
        let quads: &[PackedVoxelQuad] = &[];
        let requested = quads.len();
        let uploaded = requested.min(capacity);
        let dropped = requested.saturating_sub(uploaded);
        let bytes = (uploaded * 16) as u64;

        assert_eq!(requested, 0);
        assert_eq!(uploaded, 0);
        assert_eq!(dropped, 0);
        assert_eq!(bytes, 0);

        // Test within capacity
        let quads = [
            PackedVoxelQuad::new([0, 0, 0], [1, 1], 1, 0, 0, 0),
            PackedVoxelQuad::new([1, 1, 1], [2, 2], 2, 1, 0, 0),
        ];
        let requested = quads.len();
        let uploaded = requested.min(capacity);
        let dropped = requested.saturating_sub(uploaded);
        let bytes = (uploaded * 16) as u64;

        assert_eq!(requested, 2);
        assert_eq!(uploaded, 2);
        assert_eq!(dropped, 0);
        assert_eq!(bytes, 32);

        // Test overflow
        let low_capacity = 1;
        let requested = quads.len();
        let uploaded = requested.min(low_capacity);
        let dropped = requested.saturating_sub(uploaded);
        let bytes = (uploaded * 16) as u64;

        assert_eq!(requested, 2);
        assert_eq!(uploaded, 1);
        assert_eq!(dropped, 1);
        assert_eq!(bytes, 16);
    }

    #[test]
    fn test_plan_gpu_generated_arena_allocations_reuses_same_key() {
        let slot_capacity = |required: usize| required.max(16).next_power_of_two();
        let existing = HashMap::from([(
            1_u64,
            PackedQuadArenaAllocation {
                key: 1,
                offset_quads: 0,
                len_quads: 100,
                capacity_quads: 1024,
                generation: 1,
            },
        )]);
        let requests = [PackedGpuGenerationAllocationRequest {
            key: 1,
            requested_quads: 200,
            generation: 2,
        }];
        let (allocs, high_water) =
            plan_gpu_generated_arena_allocations(&existing, &requests, slot_capacity);
        assert_eq!(allocs[&1].offset_quads, 0);
        assert_eq!(allocs[&1].capacity_quads, 1024);
        assert_eq!(allocs[&1].generation, 2);
        assert_eq!(high_water, 1024);
    }

    #[test]
    fn test_plan_gpu_generated_arena_allocations_reuses_evicted_slot() {
        let slot_capacity = |required: usize| required.max(16).next_power_of_two();
        let existing = HashMap::from([
            (
                1_u64,
                PackedQuadArenaAllocation {
                    key: 1,
                    offset_quads: 0,
                    len_quads: 100,
                    capacity_quads: 1024,
                    generation: 1,
                },
            ),
            (
                2_u64,
                PackedQuadArenaAllocation {
                    key: 2,
                    offset_quads: 1024,
                    len_quads: 100,
                    capacity_quads: 1024,
                    generation: 1,
                },
            ),
        ]);
        let requests = [PackedGpuGenerationAllocationRequest {
            key: 3,
            requested_quads: 100,
            generation: 1,
        }];
        let (allocs, high_water) =
            plan_gpu_generated_arena_allocations(&existing, &requests, slot_capacity);
        assert_eq!(allocs[&3].offset_quads, 0);
        assert_eq!(allocs[&3].capacity_quads, 1024);
        assert_eq!(high_water, 1024);
    }

    #[test]
    fn test_gpu_generated_allocation_maps_equivalent_detects_slot_changes() {
        let existing = HashMap::from([(
            7_u64,
            PackedQuadArenaAllocation {
                key: 7,
                offset_quads: 0,
                len_quads: 64,
                capacity_quads: 128,
                generation: 3,
            },
        )]);
        let unchanged = HashMap::from([(
            7_u64,
            PackedQuadArenaAllocation {
                key: 7,
                offset_quads: 0,
                len_quads: 64,
                capacity_quads: 128,
                generation: 3,
            },
        )]);
        let shifted = HashMap::from([(
            7_u64,
            PackedQuadArenaAllocation {
                key: 7,
                offset_quads: 128,
                len_quads: 64,
                capacity_quads: 128,
                generation: 3,
            },
        )]);

        assert!(gpu_generated_allocation_maps_equivalent(
            &existing, &unchanged
        ));
        assert!(!gpu_generated_allocation_maps_equivalent(
            &existing, &shifted
        ));
    }

    #[test]
    fn test_plan_gpu_generated_arena_allocations_shifting_window_stays_bounded() {
        let slot_capacity = |required: usize| required.max(16).next_power_of_two();
        let mut existing = HashMap::new();
        let mut high_water = 0usize;

        for step in 0_u64..12 {
            let active_keys = (step..step.saturating_add(3)).collect::<Vec<_>>();
            let requests = active_keys
                .iter()
                .map(|key| PackedGpuGenerationAllocationRequest {
                    key: *key,
                    requested_quads: 512,
                    generation: step,
                })
                .collect::<Vec<_>>();
            let (allocs, next_high_water) =
                plan_gpu_generated_arena_allocations(&existing, &requests, slot_capacity);
            existing = allocs;
            high_water = high_water.max(next_high_water);
            assert!(
                next_high_water <= 3 * 1024,
                "step {step}: slot high water {next_high_water} exceeded active-window budget"
            );
        }

        assert!(high_water <= 3 * 1024);
    }

    #[test]
    fn test_plan_gpu_generated_arena_allocations_sorted_matches_unsorted() {
        let slot_capacity = |required: usize| required.max(16).next_power_of_two();
        let existing = HashMap::from([
            (
                1_u64,
                PackedQuadArenaAllocation {
                    key: 1,
                    offset_quads: 0,
                    len_quads: 128,
                    capacity_quads: 256,
                    generation: 1,
                },
            ),
            (
                4_u64,
                PackedQuadArenaAllocation {
                    key: 4,
                    offset_quads: 256,
                    len_quads: 128,
                    capacity_quads: 512,
                    generation: 1,
                },
            ),
        ]);
        let requests = [
            PackedGpuGenerationAllocationRequest {
                key: 4,
                requested_quads: 300,
                generation: 2,
            },
            PackedGpuGenerationAllocationRequest {
                key: 2,
                requested_quads: 128,
                generation: 1,
            },
            PackedGpuGenerationAllocationRequest {
                key: 1,
                requested_quads: 64,
                generation: 2,
            },
        ];
        let mut sorted_requests = requests.to_vec();
        sorted_requests.sort_unstable_by_key(|request| request.key);

        let unsorted_plan =
            plan_gpu_generated_arena_allocations(&existing, &requests, slot_capacity);
        let sorted_plan =
            plan_gpu_generated_arena_allocations_sorted(&existing, &sorted_requests, slot_capacity);
        let mut into_allocations = HashMap::with_capacity(8);
        let mut into_scratch = PackedGpuGenerationArenaAllocationPlanScratch::default();
        let into_high_water = plan_gpu_generated_arena_allocations_sorted_into(
            &existing,
            &sorted_requests,
            slot_capacity,
            &mut into_allocations,
            &mut into_scratch,
        );

        assert_eq!(sorted_plan, unsorted_plan);
        assert_eq!((into_allocations, into_high_water), unsorted_plan);
    }

    #[test]
    fn test_gpu_cull_abi() {
        assert_eq!(std::mem::size_of::<PackedQuadCullCommandMetadata>(), 48);
        assert_eq!(std::mem::align_of::<PackedQuadCullCommandMetadata>(), 4);
        assert_eq!(std::mem::size_of::<PackedQuadCullConfig>(), 16);
        assert_eq!(std::mem::align_of::<PackedQuadCullConfig>(), 4);

        let metadata = [PackedQuadCullCommandMetadata {
            bounds_min: [1.0, 2.0, 3.0, 0.0],
            bounds_max: [4.0, 5.0, 6.0, 0.0],
            meta: [7, 2, 0, 0],
        }];
        let bytes = bytemuck::cast_slice::<PackedQuadCullCommandMetadata, u8>(&metadata);
        assert_eq!(bytes.len(), 48);
        let roundtrip = bytemuck::cast_slice::<u8, PackedQuadCullCommandMetadata>(bytes);
        assert_eq!(metadata, roundtrip);
    }
}
