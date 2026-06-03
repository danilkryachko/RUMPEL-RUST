struct GenerationJob {
    // x: column_offset, y: column_count, z: output_quad_offset, w: max_output_quads.
    source: vec4<u32>,
    // x: counter_index, y: draw_command_index, z: draw_param_index, w: lod.
    output: vec4<u32>,
    // x: air, y: dirt, z: grass, w: stone.
    palette: vec4<u32>,
};

struct SurfaceColumn {
    // x: local_x, y: local_z, z: width, w: depth.
    local: vec4<u32>,
    // x: own height, y: plus_x height, z: minus_x height, w: plus_z height.
    heights: vec4<u32>,
    // x: minus_z height, y: top block id, z/w reserved.
    material: vec4<u32>,
};

struct GenerationCounter {
    emitted_quads: atomic<u32>,
    dropped_quads: atomic<u32>,
    padding: vec2<u32>,
};

struct DrawCommand {
    vertex_count: u32,
    instance_count: u32,
    first_vertex: u32,
    first_instance: u32,
};

@group(0) @binding(0) var<storage, read> jobs: array<GenerationJob>;
@group(0) @binding(1) var<storage, read> columns: array<SurfaceColumn>;
@group(0) @binding(2) var<storage, read_write> output_quads: array<vec4<u32>>;
@group(0) @binding(3) var<storage, read_write> counters: array<GenerationCounter>;
@group(0) @binding(4) var<storage, read_write> draw_commands: array<DrawCommand>;

const FACE_PLUS_X: u32 = 0u;
const FACE_MINUS_X: u32 = 1u;
const FACE_PLUS_Y: u32 = 2u;
const FACE_PLUS_Z: u32 = 4u;
const FACE_MINUS_Z: u32 = 5u;
const PACKED_QUAD_FLAG_SIDE_BLENDS_TOP_TILE: u32 = 1u;
const DIRT_DEPTH: u32 = 3u;

fn saturating_sub_u32(lhs: u32, rhs: u32) -> u32 {
    if (lhs > rhs) {
        return lhs - rhs;
    }
    return 0u;
}

fn clamp_u16(value: u32) -> u32 {
    return min(value, 65535u);
}

fn pack_quad(origin: vec3<u32>, size: vec2<u32>, block_id: u32, face: u32, lod: u32, flags: u32) -> vec4<u32> {
    let word0 = clamp_u16(origin.x) | (clamp_u16(origin.y) << 16u);
    let word1 = clamp_u16(origin.z) | (clamp_u16(size.x) << 16u);
    let word2 = clamp_u16(size.y) | ((block_id & 0xFFFFu) << 16u);
    let packed_meta = (face & 0x7u) | ((lod & 0x1Fu) << 3u) | ((flags & 0xFFFFFFu) << 8u);
    return vec4<u32>(word0, word1, word2, packed_meta);
}

fn append_quad(job: GenerationJob, origin: vec3<u32>, size: vec2<u32>, block_id: u32, face: u32, flags: u32) {
    if (size.x == 0u || size.y == 0u || block_id == job.palette.x) {
        return;
    }

    let counter_index = job.output.x;
    let local_write_index = atomicAdd(&counters[counter_index].emitted_quads, 1u);
    if (local_write_index >= job.source.w) {
        _ = atomicAdd(&counters[counter_index].dropped_quads, 1u);
        return;
    }

    output_quads[job.source.z + local_write_index] = pack_quad(origin, size, block_id, face, job.output.w, flags);
}

fn append_top(job: GenerationJob, column: SurfaceColumn) {
    let height = column.heights.x;
    let top_block = column.material.y;
    if (height == 0u || top_block == job.palette.x) {
        return;
    }

    append_quad(
        job,
        vec3<u32>(column.local.x, height - 1u, column.local.y),
        vec2<u32>(column.local.z, column.local.w),
        top_block,
        FACE_PLUS_Y,
        0u,
    );
}

fn append_side_segment(job: GenerationJob, column: SurfaceColumn, face: u32, low_y: u32, high_y: u32, block_id: u32) {
    if (high_y <= low_y) {
        return;
    }

    let segment_height = high_y - low_y;
    var origin = vec3<u32>(column.local.x, low_y, column.local.y);
    var size = vec2<u32>(segment_height, column.local.w);

    if (face == FACE_PLUS_X) {
        origin = vec3<u32>(column.local.x + column.local.z - 1u, low_y, column.local.y);
        size = vec2<u32>(segment_height, column.local.w);
    } else if (face == FACE_MINUS_X) {
        origin = vec3<u32>(column.local.x, low_y, column.local.y);
        size = vec2<u32>(segment_height, column.local.w);
    } else if (face == FACE_PLUS_Z) {
        origin = vec3<u32>(column.local.x, low_y, column.local.y + column.local.w - 1u);
        size = vec2<u32>(column.local.z, segment_height);
    } else if (face == FACE_MINUS_Z) {
        origin = vec3<u32>(column.local.x, low_y, column.local.y);
        size = vec2<u32>(column.local.z, segment_height);
    }

    let side_blend = select(0u, PACKED_QUAD_FLAG_SIDE_BLENDS_TOP_TILE, job.output.w > 0u && block_id == column.material.y);
    append_quad(job, origin, size, block_id, face, side_blend);
}

fn append_grass_side_segments(job: GenerationJob, column: SurfaceColumn, face: u32, low_y: u32, high_y: u32) {
    let width = column.local.z;
    let depth = column.local.w;
    let height = column.heights.x;

    if (width > 1u || depth > 1u) {
        let vegetated_depth = max(width, depth);
        let grass_low = max(low_y, saturating_sub_u32(height, vegetated_depth));
        append_side_segment(job, column, face, low_y, min(grass_low, high_y), job.palette.y);
        append_side_segment(job, column, face, max(grass_low, low_y), high_y, job.palette.z);
        return;
    }

    let dirt_low = max(low_y, saturating_sub_u32(height, DIRT_DEPTH));
    let grass_low = max(low_y, saturating_sub_u32(height, 1u));
    append_side_segment(job, column, face, low_y, min(dirt_low, high_y), job.palette.w);
    append_side_segment(job, column, face, max(dirt_low, low_y), min(grass_low, high_y), job.palette.y);
    append_side_segment(job, column, face, max(grass_low, low_y), high_y, job.palette.z);
}

fn append_side(job: GenerationJob, column: SurfaceColumn, face: u32, neighbor_height: u32) {
    let height = column.heights.x;
    if (neighbor_height >= height) {
        return;
    }

    let top_block = column.material.y;
    if (top_block != job.palette.z) {
        append_side_segment(job, column, face, neighbor_height, height, top_block);
        return;
    }

    append_grass_side_segments(job, column, face, neighbor_height, height);
}

@compute @workgroup_size(64)
fn generate(@builtin(global_invocation_id) id: vec3<u32>) {
    let index = id.x;
    let job_index = id.y;
    let job = jobs[job_index];
    if (index >= job.source.y) {
        return;
    }

    let column = columns[job.source.x + index];
    append_top(job, column);
    append_side(job, column, FACE_PLUS_X, column.heights.y);
    append_side(job, column, FACE_MINUS_X, column.heights.z);
    append_side(job, column, FACE_PLUS_Z, column.heights.w);
    append_side(job, column, FACE_MINUS_Z, column.material.x);
}

@compute @workgroup_size(1)
fn finalize(@builtin(global_invocation_id) id: vec3<u32>) {
    let job_index = id.x;
    let job = jobs[job_index];
    let emitted = min(atomicLoad(&counters[job.output.x].emitted_quads), job.source.w);
    let vertex_count = min(emitted, 715827882u) * 6u;
    draw_commands[job.output.y] = DrawCommand(vertex_count, 1u, 0u, job.output.z);
}
