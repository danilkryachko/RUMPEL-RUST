struct VoxelData {
    blocks: array<u32>,
};

struct DataRanges {
    vertex_start: u32,
    vertex_end: u32,
    index_start: u32,
    index_end: u32,
    neighbor_mask: u32,
};

struct DrawIndexedIndirect {
    index_count: atomic<u32>,
    instance_count: u32,
    first_index: u32,
    vertex_offset: u32,
    first_instance: u32,
}

struct TexturePalette {
    tiles: array<u32>,
};

struct BoundaryData {
    blocks: array<u32>,
};

@group(0) @binding(0) var<storage, read> chunk_data: VoxelData;
@group(0) @binding(1) var<uniform> data_range: DataRanges;
@group(0) @binding(2) var<storage, read_write> vertex_data: array<u32>;
@group(0) @binding(3) var<storage, read_write> index_data: array<u32>;
@group(0) @binding(4) var<storage, read_write> counters: DrawIndexedIndirect;
@group(0) @binding(5) var<storage, read> texture_palette: TexturePalette;
@group(0) @binding(6) var<storage, read> boundary_data: BoundaryData;

const CHUNK_SIZE: u32 = 32u;
const CHUNK_AREA: u32 = 1024u; // 32 * 32
const CHUNK_FACE_BLOCK_COUNT: u32 = 1024u;
const TEXTURE_PALETTE_BLOCKS: u32 = 256u;
const TEXTURES_PER_BLOCK: u32 = 3u;
const FALLBACK_TEXTURE_TILE: u32 = 3u;
const VERTEX_WORDS_PER_VERTEX: u32 = 13u;
fn get_index(x: u32, y: u32, z: u32) -> u32 {
    return x + y * CHUNK_SIZE + z * CHUNK_AREA;
}

fn get_block(x: u32, y: u32, z: u32) -> u32 {
    if (x >= CHUNK_SIZE || y >= CHUNK_SIZE || z >= CHUNK_SIZE) {
        return 0u;
    }
    return chunk_data.blocks[get_index(x, y, z)];
}

fn is_solid(block: u32) -> bool {
    return block != 0u;
}

fn boundary_face_index(face_idx: u32) -> u32 {
    return face_idx;
}

fn boundary_block(face_idx: u32, x: u32, y: u32, z: u32) -> u32 {
    var a = 0u;
    var b = 0u;
    if (face_idx == 0u || face_idx == 1u) {
        a = y;
        b = z;
    } else if (face_idx == 2u || face_idx == 3u) {
        a = x;
        b = z;
    } else {
        a = x;
        b = y;
    }
    return boundary_data.blocks[boundary_face_index(face_idx) * CHUNK_FACE_BLOCK_COUNT + a + b * CHUNK_SIZE];
}

fn face_texture_slot(face_idx: u32) -> u32 {
    if (face_idx == 2u) {
        return 0u;
    }
    if (face_idx == 3u) {
        return 2u;
    }
    return 1u;
}

fn face_light(face_idx: u32) -> f32 {
    if (face_idx == 2u) {
        return 1.0;
    }
    if (face_idx == 3u) {
        return 0.48;
    }
    return 0.74;
}

fn face_tile(block: u32, face_idx: u32) -> u32 {
    if (block >= TEXTURE_PALETTE_BLOCKS) {
        return FALLBACK_TEXTURE_TILE;
    }
    return texture_palette.tiles[block * TEXTURES_PER_BLOCK + face_texture_slot(face_idx)];
}

const VOXEL_FACES = array<vec3<i32>, 6>(
    vec3<i32>( 1,  0,  0), // Right
    vec3<i32>(-1,  0,  0), // Left
    vec3<i32>( 0,  1,  0), // Top
    vec3<i32>( 0, -1,  0), // Bottom
    vec3<i32>( 0,  0,  1), // Front
    vec3<i32>( 0,  0, -1)  // Back
);

fn write_f32(offset: u32, value: f32) {
    vertex_data[offset] = bitcast<u32>(value);
}

fn write_vertex(base: u32, position: vec3<f32>, normal: vec3<f32>, uv: vec2<f32>, color: vec4<f32>, tile: u32) {
    write_f32(base + 0u, position.x);
    write_f32(base + 1u, position.y);
    write_f32(base + 2u, position.z);
    write_f32(base + 3u, normal.x);
    write_f32(base + 4u, normal.y);
    write_f32(base + 5u, normal.z);
    write_f32(base + 6u, color.r);
    write_f32(base + 7u, color.g);
    write_f32(base + 8u, color.b);
    write_f32(base + 9u, color.a);
    write_f32(base + 10u, uv.x);
    write_f32(base + 11u, uv.y);
    vertex_data[base + 12u] = tile;
}

fn append_face(x: f32, y: f32, z: f32, block: u32, face_idx: u32) {
    let local_i_idx = atomicAdd(&counters.index_count, 6u);
    let local_v_idx = (local_i_idx / 6u) * 4u;

    let face_words = VERTEX_WORDS_PER_VERTEX * 4u;
    if ((local_v_idx * VERTEX_WORDS_PER_VERTEX + face_words) > (data_range.vertex_end - data_range.vertex_start)) {
        return;
    }
    if ((local_i_idx + 6u) > (data_range.index_end - data_range.index_start)) {
        return;
    }

    let base_v = local_v_idx * VERTEX_WORDS_PER_VERTEX + data_range.vertex_start;
    let base_i = local_i_idx + data_range.index_start;
    
    // Simplistic face mapping. For a real voxel engine we need a proper vertex table
    var v0 = vec3<f32>(x, y, z);
    var v1 = vec3<f32>(x, y, z);
    var v2 = vec3<f32>(x, y, z);
    var v3 = vec3<f32>(x, y, z);
    
    let normal_i = VOXEL_FACES[face_idx];
    let normal = vec3<f32>(f32(normal_i.x), f32(normal_i.y), f32(normal_i.z));
    let tile = face_tile(block, face_idx);
    let shade = face_light(face_idx);
    let color = vec4<f32>(shade, shade, shade, 1.0);
    
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

    write_vertex(base_v + 0u * VERTEX_WORDS_PER_VERTEX, v0, normal, vec2<f32>(0.0, 1.0), color, tile);
    write_vertex(base_v + 1u * VERTEX_WORDS_PER_VERTEX, v1, normal, vec2<f32>(1.0, 1.0), color, tile);
    write_vertex(base_v + 2u * VERTEX_WORDS_PER_VERTEX, v2, normal, vec2<f32>(1.0, 0.0), color, tile);
    write_vertex(base_v + 3u * VERTEX_WORDS_PER_VERTEX, v3, normal, vec2<f32>(0.0, 0.0), color, tile);

    // Two triangles: 0, 1, 2 and 0, 2, 3
    // Indices must be relative to the local mesh, not the global vertex buffer
    index_data[base_i + 0u] = local_v_idx + 0u;
    index_data[base_i + 1u] = local_v_idx + 1u;
    index_data[base_i + 2u] = local_v_idx + 2u;
    index_data[base_i + 3u] = local_v_idx + 0u;
    index_data[base_i + 4u] = local_v_idx + 2u;
    index_data[base_i + 5u] = local_v_idx + 3u;
}

@compute @workgroup_size(4, 4, 4)
fn main(@builtin(global_invocation_id) global_id: vec3<u32>) {
    let x = global_id.x;
    let y = global_id.y;
    let z = global_id.z;

    // First invocation initializes the non-atomic indirect draw fields
    if (x == 0u && y == 0u && z == 0u) {
        counters.instance_count = 1u;
        counters.first_index = 0u;
        counters.vertex_offset = 0u;
        counters.first_instance = 0u;
    }

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
            draw_face = !is_solid(boundary_block(i, x, y, z));
        } else {
            let neighbor = get_block(u32(nx), u32(ny), u32(nz));
            if (!is_solid(neighbor)) {
                draw_face = true;
            }
        }

        if (draw_face) {
            append_face(fx, fy, fz, block, i);
        }
    }
}
