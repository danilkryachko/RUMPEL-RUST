struct View {
    view_proj: mat4x4<f32>,
    camera_position_and_fog_start: vec4<f32>,
    fog_color_and_end: vec4<f32>,
};

struct DrawCommand {
    vertex_count: u32,
    instance_count: u32,
    first_vertex: u32,
    first_instance: u32,
};

struct CullMetadata {
    bounds_min: vec4<f32>,
    bounds_max: vec4<f32>,
    data: vec4<u32>,
};

struct CullConfig {
    command_count: u32,
    face_range_cull: u32,
    compact_output: u32,
    padding: u32,
};

struct CullCount {
    visible_command_count: atomic<u32>,
};

@group(0) @binding(0) var<storage, read> view: View;
@group(1) @binding(0) var<storage, read> src_commands: array<DrawCommand>;
@group(1) @binding(1) var<storage, read> metadata: array<CullMetadata>;
@group(1) @binding(2) var<storage, read_write> dst_commands: array<DrawCommand>;
@group(1) @binding(3) var<storage, read> config: CullConfig;
@group(1) @binding(4) var<storage, read_write> count: CullCount;

fn point_inside_bounds(point: vec3<f32>, bounds_min: vec3<f32>, bounds_max: vec3<f32>) -> bool {
    return point.x >= bounds_min.x
        && point.x <= bounds_max.x
        && point.y >= bounds_min.y
        && point.y <= bounds_max.y
        && point.z >= bounds_min.z
        && point.z <= bounds_max.z;
}

fn aabb_intersects_clip_frustum(bounds_min: vec3<f32>, bounds_max: vec3<f32>) -> bool {
    let corners = array<vec3<f32>, 8>(
        vec3<f32>(bounds_min.x, bounds_min.y, bounds_min.z),
        vec3<f32>(bounds_max.x, bounds_min.y, bounds_min.z),
        vec3<f32>(bounds_min.x, bounds_max.y, bounds_min.z),
        vec3<f32>(bounds_max.x, bounds_max.y, bounds_min.z),
        vec3<f32>(bounds_min.x, bounds_min.y, bounds_max.z),
        vec3<f32>(bounds_max.x, bounds_min.y, bounds_max.z),
        vec3<f32>(bounds_min.x, bounds_max.y, bounds_max.z),
        vec3<f32>(bounds_max.x, bounds_max.y, bounds_max.z),
    );

    var outside_left = true;
    var outside_right = true;
    var outside_bottom = true;
    var outside_top = true;
    var outside_near = true;
    var outside_far = true;

    for (var i = 0u; i < 8u; i = i + 1u) {
        let clip = view.view_proj * vec4<f32>(corners[i], 1.0);
        outside_left = outside_left && clip.x < -clip.w;
        outside_right = outside_right && clip.x > clip.w;
        outside_bottom = outside_bottom && clip.y < -clip.w;
        outside_top = outside_top && clip.y > clip.w;
        outside_near = outside_near && clip.z < 0.0;
        outside_far = outside_far && clip.z > clip.w;
    }

    return !(outside_left || outside_right || outside_bottom || outside_top || outside_near || outside_far);
}

fn face_points_toward_view(face_marker: u32, view_position: vec3<f32>, bounds_min: vec3<f32>, bounds_max: vec3<f32>) -> bool {
    if (face_marker == 0u) {
        return true;
    }

    let face = face_marker - 1u;
    if (face == 0u) {
        return view_position.x >= bounds_min.x;
    }
    if (face == 1u) {
        return view_position.x <= bounds_max.x;
    }
    if (face == 2u) {
        return view_position.y >= bounds_min.y;
    }
    if (face == 3u) {
        return view_position.y <= bounds_max.y;
    }
    if (face == 4u) {
        return view_position.z >= bounds_min.z;
    }
    if (face == 5u) {
        return view_position.z <= bounds_max.z;
    }
    return true;
}

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    let index = id.x;
    if (index >= config.command_count) {
        return;
    }

    let command = src_commands[index];
    let command_metadata = metadata[index];
    let bounds_min = command_metadata.bounds_min.xyz;
    let bounds_max = command_metadata.bounds_max.xyz;
    let view_position = view.camera_position_and_fog_start.xyz;
    let view_inside_batch = point_inside_bounds(view_position, bounds_min, bounds_max);

    var visible = command.vertex_count > 0u
        && command.instance_count > 0u
        && (view_inside_batch || aabb_intersects_clip_frustum(bounds_min, bounds_max));

    if (visible && config.face_range_cull != 0u && !view_inside_batch) {
        visible = face_points_toward_view(command_metadata.data.y, view_position, bounds_min, bounds_max);
    }

    if (config.compact_output != 0u) {
        if (visible) {
            let write_index = atomicAdd(&count.visible_command_count, 1u);
            dst_commands[write_index] = command;
        }
        return;
    }

    if (visible) {
        dst_commands[index] = command;
    } else {
        dst_commands[index] = DrawCommand(0u, 0u, 0u, 0u);
    }
}
