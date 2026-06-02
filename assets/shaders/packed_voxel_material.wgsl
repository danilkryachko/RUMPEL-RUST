#import bevy_pbr::mesh_view_bindings::view

struct RumpelDrawParams {
    chunk_translation_and_offset: vec4<f32>,
    fog_color_and_start: vec4<f32>,
    fog_end_and_padding: vec4<f32>,
};

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<storage, read> quads: array<vec4<u32>>;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var<uniform> draw_params: RumpelDrawParams;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var<storage, read> block_tiles: array<vec4<u32>>;
@group(#{MATERIAL_BIND_GROUP}) @binding(3) var block_atlas: texture_2d_array<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(4) var block_atlas_sampler: sampler;

const PACKED_QUAD_FLAG_SIDE_BLENDS_TOP_TILE: u32 = 1u;
const PACKED_SIDE_TOP_TILE_BLEND: f32 = 0.28;

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) @interpolate(flat) face: u32,
    @location(1) @interpolate(flat) block_id: u32,
    @location(2) repeat_uv: vec2<f32>,
    @location(3) @interpolate(flat) flags: u32,
    @location(4) world_position: vec3<f32>,
};

fn get_origin_x(quad: vec4<u32>) -> u32 {
    return quad.x & 0xFFFFu;
}

fn get_origin_y(quad: vec4<u32>) -> u32 {
    return (quad.x >> 16u) & 0xFFFFu;
}

fn get_origin_z(quad: vec4<u32>) -> u32 {
    return quad.y & 0xFFFFu;
}

fn get_size_u(quad: vec4<u32>) -> u32 {
    return (quad.y >> 16u) & 0xFFFFu;
}

fn get_size_v(quad: vec4<u32>) -> u32 {
    return quad.z & 0xFFFFu;
}

fn get_block_id(quad: vec4<u32>) -> u32 {
    return (quad.z >> 16u) & 0xFFFFu;
}

fn get_face(quad: vec4<u32>) -> u32 {
    return quad.w & 0x7u;
}

fn get_flags(quad: vec4<u32>) -> u32 {
    return quad.w >> 8u;
}

fn corner_pos_uv(face: u32, corner_index: u32, size_u: u32, size_v: u32) -> vec4<f32> {
    let u = f32(size_u);
    let v = f32(size_v);

    if (face == 0u) {
        if (corner_index == 1u) {
            return vec4<f32>(0.0, 0.0, 0.0, u);
        }
        if (corner_index == 2u || corner_index == 4u) {
            return vec4<f32>(u, 0.0, 0.0, 0.0);
        }
        if (corner_index == 5u) {
            return vec4<f32>(u, v, v, 0.0);
        }
        return vec4<f32>(0.0, v, v, u);
    }

    if (face == 1u) {
        if (corner_index == 1u) {
            return vec4<f32>(0.0, v, v, u);
        }
        if (corner_index == 2u || corner_index == 4u) {
            return vec4<f32>(u, v, v, 0.0);
        }
        if (corner_index == 5u) {
            return vec4<f32>(u, 0.0, 0.0, 0.0);
        }
        return vec4<f32>(0.0, 0.0, 0.0, u);
    }

    if (face == 2u) {
        if (corner_index == 1u) {
            return vec4<f32>(0.0, v, 0.0, v);
        }
        if (corner_index == 2u || corner_index == 4u) {
            return vec4<f32>(u, v, u, v);
        }
        if (corner_index == 5u) {
            return vec4<f32>(u, 0.0, u, 0.0);
        }
        return vec4<f32>(0.0, 0.0, 0.0, 0.0);
    }

    if (face == 4u) {
        if (corner_index == 1u) {
            return vec4<f32>(u, 0.0, u, v);
        }
        if (corner_index == 2u || corner_index == 4u) {
            return vec4<f32>(u, v, u, 0.0);
        }
        if (corner_index == 5u) {
            return vec4<f32>(0.0, v, 0.0, 0.0);
        }
        return vec4<f32>(0.0, 0.0, 0.0, v);
    }

    if (face == 5u) {
        if (corner_index == 1u) {
            return vec4<f32>(0.0, 0.0, 0.0, v);
        }
        if (corner_index == 2u || corner_index == 4u) {
            return vec4<f32>(0.0, v, 0.0, 0.0);
        }
        if (corner_index == 5u) {
            return vec4<f32>(u, v, u, 0.0);
        }
        return vec4<f32>(u, 0.0, u, v);
    }

    // MinusY and any invalid face use the simple bottom-face basis.
    if (corner_index == 1u) {
        return vec4<f32>(u, 0.0, u, 0.0);
    }
    if (corner_index == 2u || corner_index == 4u) {
        return vec4<f32>(u, v, u, v);
    }
    if (corner_index == 5u) {
        return vec4<f32>(0.0, v, 0.0, v);
    }
    return vec4<f32>(0.0, 0.0, 0.0, 0.0);
}

@vertex
fn vertex(
    @location(0) dummy_pos: vec3<f32>,
    @location(2) local_vertex_id: vec2<f32>,
) -> VertexOutput {
    var out: VertexOutput;

    let vertex_index = u32(local_vertex_id.x);
    let base_quad_offset = u32(draw_params.chunk_translation_and_offset.w);
    let quad_index = base_quad_offset + vertex_index / 6u;
    let corner_index = vertex_index % 6u;

    let quad = quads[quad_index];

    let origin_x = get_origin_x(quad);
    let origin_y = get_origin_y(quad);
    let origin_z = get_origin_z(quad);

    let size_u = get_size_u(quad);
    let size_v = get_size_v(quad);

    let block_id = get_block_id(quad);
    let face = get_face(quad);

    let origin = vec3<f32>(f32(origin_x), f32(origin_y), f32(origin_z));

    let pos_uv = corner_pos_uv(face, corner_index, size_u, size_v);
    let du = pos_uv.x;
    let dv = pos_uv.y;
    let repeat_uv = pos_uv.zw;

    var local_pos = vec3<f32>(0.0, 0.0, 0.0);
    if (face == 0u) {
        local_pos = vec3<f32>(origin.x + 1.0, origin.y + du, origin.z + dv);
    } else if (face == 1u) {
        local_pos = vec3<f32>(origin.x, origin.y + du, origin.z + dv);
    } else if (face == 2u) {
        local_pos = vec3<f32>(origin.x + du, origin.y + 1.0, origin.z + dv);
    } else if (face == 3u) {
        local_pos = vec3<f32>(origin.x + du, origin.y, origin.z + dv);
    } else if (face == 4u) {
        local_pos = vec3<f32>(origin.x + du, origin.y + dv, origin.z + 1.0);
    } else if (face == 5u) {
        local_pos = vec3<f32>(origin.x + du, origin.y + dv, origin.z);
    }

    let chunk_translation = draw_params.chunk_translation_and_offset.xyz;
    let world_position = local_pos + chunk_translation;
    out.clip_position = view.clip_from_world * vec4<f32>(world_position, 1.0);
    out.face = face;
    out.block_id = block_id;
    out.repeat_uv = repeat_uv;
    out.flags = get_flags(quad);
    out.world_position = world_position;

    return out;
}

fn face_tile(block_id: u32, face: u32) -> u32 {
    let tiles = block_tiles[min(block_id, 255u)];
    if (face == 2u) {
        return tiles.x;
    }
    if (face == 3u) {
        return tiles.z;
    }
    return tiles.y;
}

fn is_side_face(face: u32) -> bool {
    return face != 2u && face != 3u;
}

fn face_light(face: u32) -> f32 {
    if (face == 2u) {
        return 1.0;
    }
    if (face == 3u) {
        return 0.48;
    }
    if (face == 0u) {
        return 0.76;
    }
    if (face == 1u) {
        return 0.62;
    }
    if (face == 4u) {
        return 0.70;
    }
    return 0.84;
}

fn apply_face_light(color: vec4<f32>, face: u32) -> vec4<f32> {
    let light = face_light(face);
    let ambient = vec3<f32>(0.025, 0.03, 0.025);
    let lit = color.rgb * light + ambient * (1.0 - light);
    return vec4<f32>(clamp(lit, vec3<f32>(0.0), vec3<f32>(1.0)), color.a);
}

fn apply_distance_fog(color: vec4<f32>, world_position: vec3<f32>) -> vec4<f32> {
    let camera_position = view.world_position;
    let fog_start = draw_params.fog_color_and_start.w;
    let fog_end = max(draw_params.fog_end_and_padding.x, fog_start + 1.0);
    let fog_color = draw_params.fog_color_and_start.xyz;
    let camera_distance = distance(world_position, camera_position);
    let fog = smoothstep(fog_start, fog_end, camera_distance) * 0.50;
    let fogged = mix(color.rgb, fog_color, fog);
    return vec4<f32>(fogged, 1.0);
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
#ifdef PACKED_FACE_DEBUG
    // Return a distinct debug color depending on face direction
    var color = vec3<f32>(1.0, 1.0, 1.0);
    if (in.face == 0u) { // PlusX
        color = vec3<f32>(0.9, 0.2, 0.2); // Vibrant Red
    } else if (in.face == 1u) { // MinusX
        color = vec3<f32>(0.6, 0.1, 0.1); // Dark Red
    } else if (in.face == 2u) { // PlusY
        color = vec3<f32>(0.2, 0.9, 0.2); // Vibrant Green
    } else if (in.face == 3u) { // MinusY
        color = vec3<f32>(0.1, 0.6, 0.1); // Dark Green
    } else if (in.face == 4u) { // PlusZ
        color = vec3<f32>(0.2, 0.2, 0.9); // Vibrant Blue
    } else if (in.face == 5u) { // MinusZ
        color = vec3<f32>(0.1, 0.1, 0.6); // Dark Blue
    }

    // Slightly blend with block_id to add visual variation
    let tint = f32(in.block_id % 10u) * 0.03;
    return vec4<f32>(clamp(color + vec3<f32>(tint, tint, tint), vec3<f32>(0.0), vec3<f32>(1.0)), 1.0);
#else
    let tile = face_tile(in.block_id, in.face);
    let uv = fract(in.repeat_uv);
    let base = textureSample(block_atlas, block_atlas_sampler, uv, i32(tile));
    var color = base;
    if (((in.flags & PACKED_QUAD_FLAG_SIDE_BLENDS_TOP_TILE) != 0u) && is_side_face(in.face)) {
        let tiles = block_tiles[min(in.block_id, 255u)];
        let top = textureSample(block_atlas, block_atlas_sampler, uv, i32(tiles.x));
        color = mix(base, top, PACKED_SIDE_TOP_TILE_BLEND);
    }
    return apply_distance_fog(apply_face_light(color, in.face), in.world_position);
#endif
}
