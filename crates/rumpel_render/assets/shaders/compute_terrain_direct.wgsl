struct View {
    view_proj: mat4x4<f32>,
};

struct ChunkParams {
    chunk_offset: vec4<f32>,
    draw: vec4<u32>,
    offsets: vec4<u32>,
};

@group(0) @binding(0) var<storage, read> view: View;
@group(1) @binding(0) var<storage, read> vertex_data: array<u32>;
@group(1) @binding(1) var<storage, read> index_data: array<u32>;
@group(1) @binding(2) var<storage, read> chunk_params: array<ChunkParams>;
@group(1) @binding(3) var block_atlas: texture_2d_array<f32>;
@group(1) @binding(4) var block_atlas_sampler: sampler;

const VERTEX_WORDS_PER_VERTEX: u32 = 13u;

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) repeat_uv: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) @interpolate(flat) tile: u32,
};

fn read_f32(offset: u32) -> f32 {
    return bitcast<f32>(vertex_data[offset]);
}

@vertex
fn vertex(
    @builtin(vertex_index) vertex_index: u32,
    @builtin(instance_index) instance_index: u32,
) -> VertexOutput {
    var out: VertexOutput;

    let params = chunk_params[instance_index];
    let safe_index = min(vertex_index, max(params.draw.x, 1u) - 1u);
    let vertex_id = index_data[params.offsets.y + safe_index];
    let base = params.offsets.x + vertex_id * VERTEX_WORDS_PER_VERTEX;

    let local_position = vec3<f32>(
        read_f32(base + 0u),
        read_f32(base + 1u),
        read_f32(base + 2u)
    );
    let world_position = local_position + params.chunk_offset.xyz;

    out.clip_position = view.view_proj * vec4<f32>(world_position, 1.0);
    out.color = vec4<f32>(
        read_f32(base + 6u),
        read_f32(base + 7u),
        read_f32(base + 8u),
        read_f32(base + 9u)
    );
    out.repeat_uv = vec2<f32>(read_f32(base + 10u), read_f32(base + 11u));
    out.tile = vertex_data[base + 12u];

    return out;
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let atlas_uv = fract(in.repeat_uv);
    return textureSample(block_atlas, block_atlas_sampler, atlas_uv, i32(in.tile)) * in.color;
}
