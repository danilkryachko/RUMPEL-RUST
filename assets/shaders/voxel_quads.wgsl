#import bevy_pbr::mesh_functions::{get_world_from_local, mesh_position_local_to_clip}

@group(#{MATERIAL_BIND_GROUP}) @binding(0)
var block_atlas: texture_2d_array<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(1)
var block_atlas_sampler: sampler;

struct Vertex {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) color: vec4<f32>,
    @location(3) repeat_uv: vec2<f32>,
    @location(4) tile: u32,
    @builtin(instance_index) instance_index: u32,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) repeat_uv: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) @interpolate(flat) tile: u32,
};

@vertex
fn vertex(in: Vertex) -> VertexOutput {
    var out: VertexOutput;
    let world_from_local = get_world_from_local(in.instance_index);
    out.clip_position = mesh_position_local_to_clip(world_from_local, vec4<f32>(in.position, 1.0));
    out.repeat_uv = in.repeat_uv;
    out.color = in.color;
    out.tile = in.tile;
    return out;
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let atlas_uv = fract(in.repeat_uv);
    return textureSample(block_atlas, block_atlas_sampler, atlas_uv, i32(in.tile)) * in.color;
}
