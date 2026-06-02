struct DrawCommand {
    vertex_count: u32,
    instance_count: u32,
    first_vertex: u32,
    first_instance: u32,
};

struct CullMetadata {
    bounds_min: vec4<f32>,
    bounds_max: vec4<f32>,
};

struct CullConfig {
    clip_from_world: mat4x4<f32>,
    draw: vec4<u32>,
};

struct CullCount {
    visible_command_count: atomic<u32>,
};

@group(0) @binding(0) var<storage, read> source_commands: array<DrawCommand>;
@group(0) @binding(1) var<storage, read> metadata: array<CullMetadata>;
@group(0) @binding(2) var<storage, read_write> output_commands: array<DrawCommand>;
@group(0) @binding(3) var<storage, read> config: CullConfig;
@group(0) @binding(4) var<storage, read_write> count: CullCount;
@group(0) @binding(5) var depth_texture: texture_depth_2d;

fn zero_command() -> DrawCommand {
    return DrawCommand(0u, 0u, 0u, 0u);
}

fn aabb_intersects_clip_frustum(bounds_min: vec3<f32>, bounds_max: vec3<f32>) -> bool {
    var outside_left = true;
    var outside_right = true;
    var outside_bottom = true;
    var outside_top = true;
    var outside_near = true;
    var outside_far = true;

    for (var corner_index = 0u; corner_index < 8u; corner_index = corner_index + 1u) {
        let x = select(bounds_min.x, bounds_max.x, (corner_index & 1u) != 0u);
        let y = select(bounds_min.y, bounds_max.y, (corner_index & 2u) != 0u);
        let z = select(bounds_min.z, bounds_max.z, (corner_index & 4u) != 0u);
        let clip = config.clip_from_world * vec4<f32>(x, y, z, 1.0);

        outside_left = outside_left && clip.x < -clip.w;
        outside_right = outside_right && clip.x > clip.w;
        outside_bottom = outside_bottom && clip.y < -clip.w;
        outside_top = outside_top && clip.y > clip.w;
        outside_near = outside_near && clip.z < 0.0;
        outside_far = outside_far && clip.z > clip.w;
    }

    return !(outside_left || outside_right || outside_bottom || outside_top || outside_near || outside_far);
}

fn aabb_is_visible_occlusion(bounds_min: vec3<f32>, bounds_max: vec3<f32>) -> bool {
    var uv_min = vec2<f32>(1.0, 1.0);
    var uv_max = vec2<f32>(0.0, 0.0);
    var z_max = 0.0;
    var is_behind_near_plane = false;

    // Project 8 corners of AABB into clip space and compute screen UV coordinates
    for (var i = 0u; i < 8u; i = i + 1u) {
        let x = select(bounds_min.x, bounds_max.x, (i & 1u) != 0u);
        let y = select(bounds_min.y, bounds_max.y, (i & 2u) != 0u);
        let z = select(bounds_min.z, bounds_max.z, (i & 4u) != 0u);

        let clip = config.clip_from_world * vec4<f32>(x, y, z, 1.0);
        if (clip.w <= 0.1) {
            is_behind_near_plane = true;
            break;
        }

        let ndc = clip.xyz / clip.w;
        let uv = vec2<f32>(ndc.x, -ndc.y) * 0.5 + vec2<f32>(0.5);

        uv_min = min(uv_min, uv);
        uv_max = max(uv_max, uv);
        z_max = max(z_max, ndc.z);
    }

    if (is_behind_near_plane) {
        return true;
    }

    uv_min = clamp(uv_min, vec2<f32>(0.0, 0.0), vec2<f32>(1.0, 1.0));
    uv_max = clamp(uv_max, vec2<f32>(0.0, 0.0), vec2<f32>(1.0, 1.0));

    let texture_size = vec2<f32>(textureDimensions(depth_texture));
    let pixel_min = vec2<i32>(floor(uv_min * texture_size));
    let pixel_max = vec2<i32>(ceil(uv_max * texture_size));

    let rect_width = pixel_max.x - pixel_min.x;
    let rect_height = pixel_max.y - pixel_min.y;

    let max_occlusion_size = 64;
    if (rect_width > max_occlusion_size || rect_height > max_occlusion_size || rect_width <= 0 || rect_height <= 0) {
        return true;
    }

    var occluded = true;
    for (var py = pixel_min.y; py <= pixel_max.y; py = py + 1) {
        for (var px = pixel_min.x; px <= pixel_max.x; px = px + 1) {
            let depth = textureLoad(depth_texture, vec2<i32>(px, py), 0);
            // Under Reversed-Z near is 1.0, far is 0.0.
            // If depth in buffer is further than closest point (depth < z_max)
            // or background clear (depth <= 0.00001), the chunk is visible.
            if (depth < z_max || depth <= 0.00001) {
                occluded = false;
                break;
            }
        }
        if (!occluded) {
            break;
        }
    }

    return !occluded;
}

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) global_id: vec3<u32>) {
    let command_index = global_id.x;
    if (command_index >= config.draw.x) {
        return;
    }

    let command = source_commands[command_index];
    if (command.vertex_count == 0u || command.instance_count == 0u) {
        if (config.draw.y == 0u) {
            output_commands[command_index] = zero_command();
        }
        return;
    }

    let command_metadata = metadata[command_index];
    var visible = aabb_intersects_clip_frustum(command_metadata.bounds_min.xyz, command_metadata.bounds_max.xyz);
    if (visible && config.draw.z != 0u) {
        visible = aabb_is_visible_occlusion(command_metadata.bounds_min.xyz, command_metadata.bounds_max.xyz);
    }

    if (config.draw.y != 0u) {
        if (visible) {
            let write_index = atomicAdd(&count.visible_command_count, 1u);
            output_commands[write_index] = command;
        }
        return;
    }

    if (visible) {
        output_commands[command_index] = command;
    } else {
        output_commands[command_index] = zero_command();
    }
}
