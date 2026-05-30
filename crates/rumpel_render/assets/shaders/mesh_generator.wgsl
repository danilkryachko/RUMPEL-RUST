struct VoxelData {
    blocks: array<u32>,
};

struct Vertex {
    position: vec3<f32>,
    normal: vec3<f32>,
    uv: vec2<f32>,
};

struct OutputVertices {
    count: atomic<u32>,
    data: array<Vertex>,
};

struct OutputIndices {
    count: atomic<u32>,
    data: array<u32>,
};

@group(0) @binding(0) var<storage, read> chunk_data: VoxelData;
@group(0) @binding(1) var<storage, read_write> out_vertices: OutputVertices;
@group(0) @binding(2) var<storage, read_write> out_indices: OutputIndices;

const CHUNK_SIZE: u32 = 32u;
const CHUNK_AREA: u32 = 1024u; // 32 * 32

fn get_index(x: u32, y: u32, z: u32) -> u32 {
    return x + y * CHUNK_SIZE + z * CHUNK_AREA;
}

fn get_block(x: u32, y: u32, z: u32) -> u32 {
    if (x >= CHUNK_SIZE || y >= CHUNK_SIZE || z >= CHUNK_SIZE) {
        return 0u; // Air for out of bounds (temporarily naive)
    }
    return chunk_data.blocks[get_index(x, y, z)];
}

fn is_solid(block: u32) -> bool {
    return block != 0u;
}

const VOXEL_FACES = array<vec3<i32>, 6>(
    vec3<i32>( 1,  0,  0), // Right
    vec3<i32>(-1,  0,  0), // Left
    vec3<i32>( 0,  1,  0), // Top
    vec3<i32>( 0, -1,  0), // Bottom
    vec3<i32>( 0,  0,  1), // Front
    vec3<i32>( 0,  0, -1)  // Back
);

// We define simple quad vertices for a 1x1x1 cube centered at 0,0,0 or starting at 0,0,0
// We'll generate quads clockwise
fn append_face(x: f32, y: f32, z: f32, face_idx: u32) {
    let base_idx = atomicAdd(&out_vertices.count, 4u);
    let base_index_pos = atomicAdd(&out_indices.count, 6u);
    
    // Simplistic face mapping. For a real voxel engine we need a proper vertex table
    var v0 = vec3<f32>(x, y, z);
    var v1 = vec3<f32>(x, y, z);
    var v2 = vec3<f32>(x, y, z);
    var v3 = vec3<f32>(x, y, z);
    
    let normal_i = VOXEL_FACES[face_idx];
    let normal = vec3<f32>(f32(normal_i.x), f32(normal_i.y), f32(normal_i.z));
    
    if (face_idx == 0u) { // Right (X+)
        v0 += vec3(1.0, 0.0, 1.0); v1 += vec3(1.0, 0.0, 0.0); v2 += vec3(1.0, 1.0, 0.0); v3 += vec3(1.0, 1.0, 1.0);
    } else if (face_idx == 1u) { // Left (X-)
        v0 += vec3(0.0, 0.0, 0.0); v1 += vec3(0.0, 0.0, 1.0); v2 += vec3(0.0, 1.0, 1.0); v3 += vec3(0.0, 1.0, 0.0);
    } else if (face_idx == 2u) { // Top (Y+)
        v0 += vec3(0.0, 1.0, 1.0); v1 += vec3(1.0, 1.0, 1.0); v2 += vec3(1.0, 1.0, 0.0); v3 += vec3(0.0, 1.0, 0.0);
    } else if (face_idx == 3u) { // Bottom (Y-)
        v0 += vec3(0.0, 0.0, 0.0); v1 += vec3(1.0, 0.0, 0.0); v2 += vec3(1.0, 0.0, 1.0); v3 += vec3(0.0, 0.0, 1.0);
    } else if (face_idx == 4u) { // Front (Z+)
        v0 += vec3(0.0, 0.0, 1.0); v1 += vec3(1.0, 0.0, 1.0); v2 += vec3(1.0, 1.0, 1.0); v3 += vec3(0.0, 1.0, 1.0);
    } else if (face_idx == 5u) { // Back (Z-)
        v0 += vec3(1.0, 0.0, 0.0); v1 += vec3(0.0, 0.0, 0.0); v2 += vec3(0.0, 1.0, 0.0); v3 += vec3(1.0, 1.0, 0.0);
    }

    out_vertices.data[base_idx + 0u] = Vertex(v0, normal, vec2<f32>(0.0, 1.0));
    out_vertices.data[base_idx + 1u] = Vertex(v1, normal, vec2<f32>(1.0, 1.0));
    out_vertices.data[base_idx + 2u] = Vertex(v2, normal, vec2<f32>(1.0, 0.0));
    out_vertices.data[base_idx + 3u] = Vertex(v3, normal, vec2<f32>(0.0, 0.0));

    // Two triangles: 0, 1, 2 and 0, 2, 3
    out_indices.data[base_index_pos + 0u] = base_idx + 0u;
    out_indices.data[base_index_pos + 1u] = base_idx + 1u;
    out_indices.data[base_index_pos + 2u] = base_idx + 2u;
    out_indices.data[base_index_pos + 3u] = base_idx + 0u;
    out_indices.data[base_index_pos + 4u] = base_idx + 2u;
    out_indices.data[base_index_pos + 5u] = base_idx + 3u;
}

@compute @workgroup_size(4, 4, 4)
fn main(@builtin(global_invocation_id) global_id: vec3<u32>) {
    let x = global_id.x;
    let y = global_id.y;
    let z = global_id.z;

    if (x >= CHUNK_SIZE || y >= CHUNK_SIZE || z >= CHUNK_SIZE) {
        return;
    }

    let block = get_block(x, y, z);
    if (!is_solid(block)) {
        return;
    }

    let fx = f32(x);
    let fy = f32(y);
    let fz = f32(z);

    // Naive Face Culling check
    for (var i = 0u; i < 6u; i = i + 1u) {
        let dir = VOXEL_FACES[i];
        let nx = i32(x) + dir.x;
        let ny = i32(y) + dir.y;
        let nz = i32(z) + dir.z;

        var draw_face = false;
        
        if (nx < 0 || ny < 0 || nz < 0 || nx >= i32(CHUNK_SIZE) || ny >= i32(CHUNK_SIZE) || nz >= i32(CHUNK_SIZE)) {
            // At chunk boundary, draw face (we'll implement neighbor chunk data passing later)
            draw_face = true;
        } else {
            let neighbor = get_block(u32(nx), u32(ny), u32(nz));
            if (!is_solid(neighbor)) {
                draw_face = true;
            }
        }

        if (draw_face) {
            append_face(fx, fy, fz, i);
        }
    }
}
