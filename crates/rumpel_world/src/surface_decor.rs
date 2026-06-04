use crate::chunk::{CHUNK_SIZE, ChunkData};
use bevy::platform::collections::HashSet;
use rumpel_blocks::{AIR_BLOCK_ID, BlockId, BlockRegistry};

const DEFAULT_GRASS_CAP: i64 = 240;
const DEFAULT_LEAF_CAP: i64 = 240;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DecorInstance {
    pub translation: [f32; 3],
    pub rotation_y: f32,
    pub scale: [f32; 3],
    /// `(color_mix, wind_offset, shade_mix, 1.0)` — совместимо с RUMPEL2 custom data.
    pub custom: [f32; 4],
}

#[derive(Clone, Debug)]
pub struct DecorBlockContext {
    pub grass: BlockId,
    pub leaves: BlockId,
    pub air: BlockId,
    grass_blocker_ids: HashSet<BlockId>,
}

impl DecorBlockContext {
    #[must_use]
    pub fn from_registry(registry: &BlockRegistry) -> Self {
        let grass = registry.get_id("grass").unwrap_or(AIR_BLOCK_ID);
        let leaves = registry.get_id("leaves").unwrap_or(AIR_BLOCK_ID);
        let air = AIR_BLOCK_ID;
        let mut grass_blocker_ids = HashSet::default();
        for id in 0..=u16::MAX {
            let block_id = BlockId::from(id);
            if block_id == air || block_id == grass || block_id == leaves {
                continue;
            }
            if registry
                .get_block(block_id)
                .is_some_and(|data| data.is_solid && !data.is_transparent)
            {
                grass_blocker_ids.insert(block_id);
            }
        }
        Self {
            grass,
            leaves,
            air,
            grass_blocker_ids,
        }
    }

    fn is_grass_blocker(&self, block: BlockId) -> bool {
        self.grass_blocker_ids.contains(&block)
    }
}

#[derive(Clone, Debug, Default)]
pub struct ChunkDecorOutput {
    pub grass: Vec<DecorInstance>,
    pub leaves: Vec<DecorInstance>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ChunkDecorCounts {
    pub grass_instances: usize,
    pub leaf_instances: usize,
    pub grass_original: i64,
    pub leaf_original: i64,
}

#[must_use]
pub fn decor_grass_cap_from_env() -> i64 {
    decor_cap_from_env("RUMPEL_DECOR_GRASS_CAP", DEFAULT_GRASS_CAP)
}

#[must_use]
pub fn decor_leaf_cap_from_env() -> i64 {
    decor_cap_from_env("RUMPEL_DECOR_LEAF_CAP", DEFAULT_LEAF_CAP)
}

fn decor_cap_from_env(name: &str, default: i64) -> i64 {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse::<i64>().ok())
        .unwrap_or(default)
}

#[must_use]
pub fn grass_instances(
    chunk: &ChunkData,
    context: &DecorBlockContext,
    cap: i64,
) -> (Vec<DecorInstance>, i64) {
    let mut full_instances = Vec::new();
    if cap == 0 {
        return (full_instances, 0);
    }
    for z in 0..CHUNK_SIZE {
        for y in 0..CHUNK_SIZE {
            for x in 0..CHUNK_SIZE {
                if chunk.get_block(x, y, z) == context.grass {
                    append_grass_block(chunk, x, y, z, context, &mut full_instances);
                }
            }
        }
    }
    let total = i64::try_from(full_instances.len()).unwrap_or(i64::MAX);
    (cap_decor_instances(full_instances, cap), total)
}

#[must_use]
pub fn leaf_instances(
    chunk: &ChunkData,
    context: &DecorBlockContext,
    cap: i64,
) -> (Vec<DecorInstance>, i64) {
    let mut full_instances = Vec::new();
    for z in 0..CHUNK_SIZE {
        for y in 0..CHUNK_SIZE {
            for x in 0..CHUNK_SIZE {
                if chunk.get_block(x, y, z) == context.leaves {
                    append_leaf_lod_instances(chunk, x, y, z, context, &mut full_instances);
                }
            }
        }
    }
    let original_count = i64::try_from(full_instances.len()).unwrap_or(i64::MAX);
    (cap_decor_instances(full_instances, cap), original_count)
}

#[must_use]
pub fn build_chunk_decor(
    chunk: &ChunkData,
    context: &DecorBlockContext,
    grass_cap: i64,
    leaf_cap: i64,
) -> (Vec<DecorInstance>, Vec<DecorInstance>, ChunkDecorCounts) {
    let (grass, grass_original) = grass_instances(chunk, context, grass_cap);
    let (leaves, leaf_original) = leaf_instances(chunk, context, leaf_cap);
    let counts = ChunkDecorCounts {
        grass_instances: grass.len(),
        leaf_instances: leaves.len(),
        grass_original,
        leaf_original,
    };
    (grass, leaves, counts)
}

#[must_use]
pub fn resolve_chunk_decor(
    decor: &ChunkDecorOutput,
    chunk: &ChunkData,
    context: &DecorBlockContext,
    grass_cap: i64,
    leaf_cap: i64,
) -> (Vec<DecorInstance>, Vec<DecorInstance>, ChunkDecorCounts) {
    if decor.grass.is_empty() && decor.leaves.is_empty() {
        return build_chunk_decor(chunk, context, grass_cap, leaf_cap);
    }

    let grass_original = i64::try_from(decor.grass.len()).unwrap_or(i64::MAX);
    let leaf_original = i64::try_from(decor.leaves.len()).unwrap_or(i64::MAX);
    let grass = cap_decor_instances(decor.grass.clone(), grass_cap);
    let leaves = cap_decor_instances(decor.leaves.clone(), leaf_cap);
    let counts = ChunkDecorCounts {
        grass_instances: grass.len(),
        leaf_instances: leaves.len(),
        grass_original,
        leaf_original,
    };
    (grass, leaves, counts)
}

fn append_grass_block(
    chunk: &ChunkData,
    x: usize,
    y: usize,
    z: usize,
    context: &DecorBlockContext,
    instances: &mut Vec<DecorInstance>,
) {
    let grass_seed = local_noise(
        axis_to_i32(x) * 17 + 31,
        axis_to_i32(y) * 13 + 47,
        axis_to_i32(z) * 19 + 59,
    );
    let clearance = grass_clearance(chunk, x, y, z, context);
    if clearance.blockers >= 3 {
        return;
    }
    let clump_count = 5 + ((grass_seed >> 6).abs() % 3);
    for clump_index in 0..clump_count {
        let clump_seed = local_noise(
            axis_to_i32(x) * 37 + clump_index * 11,
            axis_to_i32(y) * 23 + 5,
            axis_to_i32(z) * 41 + clump_index * 17,
        );
        let spread_x = i32_to_f32((clump_seed >> 1) & 31) / 31.0;
        let spread_z = i32_to_f32((clump_seed >> 7) & 31) / 31.0;
        let mut offset_x = 0.14 + spread_x * 0.72;
        let mut offset_z = 0.14 + spread_z * 0.72;
        let yaw = i32_to_f32(clump_seed % 6283) / 1000.0;
        let mut height = 0.42 + i32_to_f32((clump_seed >> 8) & 7) * 0.018;
        let mut width = 0.96 + i32_to_f32((clump_seed >> 11) & 5) * 0.006;
        let mut color_mix = i32_to_f32((clump_seed >> 15) & 7) / 7.0 * 0.20;
        let wind_offset = i32_to_f32((clump_seed >> 18) & 255) / 255.0;
        if clearance.blockers > 0 {
            offset_x = (offset_x + clearance.avoidance[0] * 0.30).clamp(0.26, 0.74);
            offset_z = (offset_z + clearance.avoidance[2] * 0.30).clamp(0.26, 0.74);
            width *= (1.0 - i32_to_f32(clearance.blockers) * 0.24).max(0.42);
            height *= (1.0 - i32_to_f32(clearance.blockers) * 0.10).max(0.68);
            if clearance.east {
                offset_x = offset_x.min(0.40);
            }
            if clearance.west {
                offset_x = offset_x.max(0.60);
            }
            if clearance.south {
                offset_z = offset_z.min(0.40);
            }
            if clearance.north {
                offset_z = offset_z.max(0.60);
            }
            if clearance.east && clearance.west {
                offset_x = 0.50;
                width *= 0.56;
            }
            if clearance.south && clearance.north {
                offset_z = 0.50;
                width *= 0.56;
            }
        }
        height *= 1.26 + i32_to_f32((clump_seed >> 21) & 3) * 0.045;
        width *= 0.56 + i32_to_f32((clump_seed >> 24) & 3) * 0.035;
        color_mix = color_mix.min(0.24);
        push_decor_instance(
            instances,
            yaw,
            [width, height, width],
            [x as f32 + offset_x, y as f32 + 0.006, z as f32 + offset_z],
            [color_mix, wind_offset, 0.0, 1.0],
        );
    }
}

#[derive(Clone, Copy)]
struct GrassClearance {
    east: bool,
    west: bool,
    south: bool,
    north: bool,
    blockers: i32,
    avoidance: [f32; 3],
}

fn grass_clearance(
    chunk: &ChunkData,
    x: usize,
    y: usize,
    z: usize,
    context: &DecorBlockContext,
) -> GrassClearance {
    let east = is_grass_blocker_at_offset(chunk, x, y, z, [1, 0, 0], context);
    let west = is_grass_blocker_at_offset(chunk, x, y, z, [-1, 0, 0], context);
    let south = is_grass_blocker_at_offset(chunk, x, y, z, [0, 0, 1], context);
    let north = is_grass_blocker_at_offset(chunk, x, y, z, [0, 0, -1], context);
    let blockers = i32::from(east) + i32::from(west) + i32::from(south) + i32::from(north);
    let mut avoidance = [0.0, 0.0, 0.0];
    if east {
        avoidance[0] -= 1.0;
    }
    if west {
        avoidance[0] += 1.0;
    }
    if south {
        avoidance[2] -= 1.0;
    }
    if north {
        avoidance[2] += 1.0;
    }
    avoidance = normalize_or_zero(avoidance);
    GrassClearance {
        east,
        west,
        south,
        north,
        blockers,
        avoidance,
    }
}

fn is_grass_blocker_at_offset(
    chunk: &ChunkData,
    x: usize,
    y: usize,
    z: usize,
    offset: [i32; 3],
    context: &DecorBlockContext,
) -> bool {
    let block = block_at_offset(chunk, x, y, z, offset, context.air);
    context.is_grass_blocker(block)
}

fn cap_decor_instances(full_instances: Vec<DecorInstance>, cap: i64) -> Vec<DecorInstance> {
    if cap < 0 {
        return full_instances;
    }
    let cap_usize = usize::try_from(cap).unwrap_or(usize::MAX);
    if full_instances.len() <= cap_usize {
        return full_instances;
    }
    let mut instances = Vec::with_capacity(cap_usize);
    for index in 0..cap_usize {
        let source_index =
            ((index as f32) * (full_instances.len() as f32) / (cap_usize as f32)).floor() as usize;
        if let Some(instance) = full_instances.get(source_index).copied() {
            instances.push(instance);
        }
    }
    instances
}

fn append_leaf_lod_instances(
    chunk: &ChunkData,
    x: usize,
    y: usize,
    z: usize,
    context: &DecorBlockContext,
    instances: &mut Vec<DecorInstance>,
) {
    let leaf_seed = local_noise(
        axis_to_i32(x) * 43 + 11,
        axis_to_i32(y) * 47 + 17,
        axis_to_i32(z) * 53 + 23,
    );
    let air_neighbors = leaf_air_neighbor_count(chunk, x, y, z, context);
    let base_jitter = leaf_base_jitter(leaf_seed);
    let mass_bias = leaf_mass_bias(chunk, x, y, z, context);
    if air_neighbors == 0 {
        append_leaf_instance(
            instances,
            LeafInstanceSpec {
                position: (x, y, z),
                instance_seed: leaf_seed ^ 0x0d4f2a,
                offset: add_vec3(scale_vec3(mass_bias, 0.18), scale_vec3(base_jitter, 0.22)),
                scale_multiplier: 1.24,
                vertical_multiplier: 1.02,
            },
        );
        append_leaf_instance(
            instances,
            LeafInstanceSpec {
                position: (x, y, z),
                instance_seed: leaf_seed ^ 0x21c7a5,
                offset: add_vec3(
                    add_vec3(
                        scale_vec3(leaf_filler_offset(leaf_seed, 0), 0.74),
                        scale_vec3(base_jitter, 0.16),
                    ),
                    scale_vec3(mass_bias, 0.12),
                ),
                scale_multiplier: 1.08,
                vertical_multiplier: 0.92,
            },
        );
        append_leaf_instance(
            instances,
            LeafInstanceSpec {
                position: (x, y, z),
                instance_seed: leaf_seed ^ 0x39f02d,
                offset: add_vec3(
                    add_vec3(
                        scale_vec3(leaf_filler_offset(leaf_seed, 1), 0.66),
                        scale_vec3(base_jitter, -0.14),
                    ),
                    scale_vec3(mass_bias, 0.10),
                ),
                scale_multiplier: 1.02,
                vertical_multiplier: 0.88,
            },
        );
        return;
    }
    append_leaf_instance(
        instances,
        LeafInstanceSpec {
            position: (x, y, z),
            instance_seed: leaf_seed,
            offset: add_vec3(scale_vec3(base_jitter, 0.58), scale_vec3(mass_bias, 0.32)),
            scale_multiplier: 2.42,
            vertical_multiplier: 1.46,
        },
    );
    if air_neighbors >= 2 {
        append_leaf_instance(
            instances,
            LeafInstanceSpec {
                position: (x, y, z),
                instance_seed: leaf_seed ^ 0x4f1bbc,
                offset: add_vec3(
                    add_vec3(
                        scale_vec3(leaf_filler_offset(leaf_seed, 0), 1.12),
                        scale_vec3(base_jitter, 0.42),
                    ),
                    scale_vec3(mass_bias, 0.14),
                ),
                scale_multiplier: 1.30,
                vertical_multiplier: 1.04,
            },
        );
    }
    for face in [
        LeafFaceSpec {
            offset: [1, 0, 0],
            salt: 0x777_111,
        },
        LeafFaceSpec {
            offset: [-1, 0, 0],
            salt: 0x777_222,
        },
        LeafFaceSpec {
            offset: [0, 0, 1],
            salt: 0x777_333,
        },
        LeafFaceSpec {
            offset: [0, 0, -1],
            salt: 0x777_444,
        },
    ] {
        append_leaf_face_instance_if_air(
            chunk,
            instances,
            (x, y, z),
            context,
            leaf_seed,
            base_jitter,
            face,
        );
    }
}

#[derive(Clone, Copy)]
struct LeafInstanceSpec {
    position: (usize, usize, usize),
    instance_seed: i32,
    offset: [f32; 3],
    scale_multiplier: f32,
    vertical_multiplier: f32,
}

#[derive(Clone, Copy)]
struct LeafFaceSpec {
    offset: [i32; 3],
    salt: i32,
}

fn append_leaf_face_instance_if_air(
    chunk: &ChunkData,
    instances: &mut Vec<DecorInstance>,
    position: (usize, usize, usize),
    context: &DecorBlockContext,
    leaf_seed: i32,
    base_jitter: [f32; 3],
    face: LeafFaceSpec,
) {
    let (x, y, z) = position;
    if block_at_offset(chunk, x, y, z, face.offset, context.air) != context.air {
        return;
    }
    append_leaf_instance(
        instances,
        LeafInstanceSpec {
            position,
            instance_seed: leaf_seed ^ face.salt,
            offset: add_vec3(
                [
                    face.offset[0] as f32 * 0.62,
                    face.offset[1] as f32 * 0.50,
                    face.offset[2] as f32 * 0.62,
                ],
                scale_vec3(base_jitter, 0.55),
            ),
            scale_multiplier: 1.14,
            vertical_multiplier: 0.96,
        },
    );
}

fn append_leaf_instance(instances: &mut Vec<DecorInstance>, spec: LeafInstanceSpec) {
    let (x, y, z) = spec.position;
    let instance_seed = spec.instance_seed;
    let offset = spec.offset;
    let scale_multiplier = spec.scale_multiplier;
    let vertical_multiplier = spec.vertical_multiplier;
    let yaw = i32_to_f32(instance_seed % 6283) / 1000.0;
    let scale = (0.96 + i32_to_f32((instance_seed >> 6) & 7) * 0.012) * scale_multiplier;
    let vertical_scale =
        (0.94 + i32_to_f32((instance_seed >> 10) & 7) * 0.014) * vertical_multiplier;
    let warm_mix = i32_to_f32((instance_seed >> 14) & 7) / 18.0;
    let wind_offset = i32_to_f32((instance_seed >> 18) & 255) / 255.0;
    let shade_mix = i32_to_f32((instance_seed >> 26) & 7) / 14.0;
    push_decor_instance(
        instances,
        yaw,
        [scale, vertical_scale, scale],
        [
            x as f32 + 0.5 + offset[0],
            y as f32 + 0.5 + offset[1],
            z as f32 + 0.5 + offset[2],
        ],
        [warm_mix, wind_offset, shade_mix, 1.0],
    );
}

fn leaf_filler_offset(leaf_seed: i32, index: i32) -> [f32; 3] {
    let angle = i32_to_f32((leaf_seed >> (index * 5)) & 1023) / 1023.0 * std::f32::consts::TAU;
    let radius = 0.22 + i32_to_f32((leaf_seed >> (index * 7 + 11)) & 255) / 255.0 * 0.20;
    let y_offset = -0.14 + i32_to_f32((leaf_seed >> (index * 6 + 19)) & 255) / 255.0 * 0.28;
    [angle.cos() * radius, y_offset, angle.sin() * radius]
}

fn leaf_base_jitter(leaf_seed: i32) -> [f32; 3] {
    [
        (i32_to_f32(leaf_seed & 255) / 255.0 - 0.5) * 0.22,
        (i32_to_f32((leaf_seed >> 8) & 255) / 255.0 - 0.5) * 0.12,
        (i32_to_f32((leaf_seed >> 16) & 255) / 255.0 - 0.5) * 0.22,
    ]
}

fn leaf_mass_bias(
    chunk: &ChunkData,
    x: usize,
    y: usize,
    z: usize,
    context: &DecorBlockContext,
) -> [f32; 3] {
    let mut bias = [0.0, 0.0, 0.0];
    for offset in [
        [1, 0, 0],
        [-1, 0, 0],
        [0, 1, 0],
        [0, -1, 0],
        [0, 0, 1],
        [0, 0, -1],
    ] {
        if block_at_offset(chunk, x, y, z, offset, context.air) == context.leaves {
            bias[0] += offset[0] as f32;
            bias[1] += offset[1] as f32;
            bias[2] += offset[2] as f32;
        }
    }
    normalize_or_zero(bias)
}

fn leaf_air_neighbor_count(
    chunk: &ChunkData,
    x: usize,
    y: usize,
    z: usize,
    context: &DecorBlockContext,
) -> i32 {
    let mut count = 0;
    for offset in [
        [1, 0, 0],
        [-1, 0, 0],
        [0, 1, 0],
        [0, -1, 0],
        [0, 0, 1],
        [0, 0, -1],
    ] {
        if block_at_offset(chunk, x, y, z, offset, context.air) == context.air {
            count += 1;
        }
    }
    count
}

fn block_at_offset(
    chunk: &ChunkData,
    x: usize,
    y: usize,
    z: usize,
    offset: [i32; 3],
    air: BlockId,
) -> BlockId {
    let Some(x) = offset_axis(x, offset[0]) else {
        return air;
    };
    let Some(y) = offset_axis(y, offset[1]) else {
        return air;
    };
    let Some(z) = offset_axis(z, offset[2]) else {
        return air;
    };
    if x >= CHUNK_SIZE || y >= CHUNK_SIZE || z >= CHUNK_SIZE {
        return air;
    }
    chunk.get_block(x, y, z)
}

fn offset_axis(value: usize, offset: i32) -> Option<usize> {
    if offset < 0 {
        value.checked_sub(usize::try_from(offset.unsigned_abs()).ok()?)
    } else {
        value.checked_add(usize::try_from(offset).ok()?)
    }
}

fn push_decor_instance(
    instances: &mut Vec<DecorInstance>,
    yaw: f32,
    scale: [f32; 3],
    translation: [f32; 3],
    custom: [f32; 4],
) {
    instances.push(DecorInstance {
        translation,
        rotation_y: yaw,
        scale,
        custom,
    });
}

fn i32_to_f32(value: i32) -> f32 {
    f32::from(i16::try_from(value).unwrap_or(i16::MAX))
}

fn local_noise(x: i32, y: i32, z: i32) -> i32 {
    let mut value = x.wrapping_mul(73_856_093);
    value ^= y.wrapping_mul(19_349_663);
    value ^= z.wrapping_mul(83_492_791);
    value ^= value >> 13;
    value.abs()
}

fn axis_to_i32(value: usize) -> i32 {
    i32::try_from(value).expect("chunk axis fits i32")
}

fn normalize_or_zero(value: [f32; 3]) -> [f32; 3] {
    let length_sq = value[0] * value[0] + value[1] * value[1] + value[2] * value[2];
    if length_sq <= 0.001 {
        return [0.0, 0.0, 0.0];
    }
    let inv = length_sq.sqrt().recip();
    [value[0] * inv, value[1] * inv, value[2] * inv]
}

fn add_vec3(left: [f32; 3], right: [f32; 3]) -> [f32; 3] {
    [left[0] + right[0], left[1] + right[1], left[2] + right[2]]
}

fn scale_vec3(value: [f32; 3], scale: f32) -> [f32; 3] {
    [value[0] * scale, value[1] * scale, value[2] * scale]
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::platform::collections::HashSet;
    use rumpel_blocks::BlockRegistry;

    fn test_context() -> DecorBlockContext {
        let registry = BlockRegistry::default();
        DecorBlockContext::from_registry(&registry)
    }

    fn grass_layer_chunk(context: &DecorBlockContext) -> ChunkData {
        let mut chunk = ChunkData::default();
        for z in 0..CHUNK_SIZE {
            for x in 0..CHUNK_SIZE {
                chunk.set_block(x, 0, z, context.grass);
            }
        }
        chunk
    }

    #[test]
    fn capped_grass_instances_sample_across_entire_chunk() {
        let context = test_context();
        let chunk = grass_layer_chunk(&context);

        let (instances, original_count) = grass_instances(&chunk, &context, 16);
        let min_z = instances
            .iter()
            .map(|instance| instance.translation[2])
            .fold(f32::INFINITY, f32::min);
        let max_z = instances
            .iter()
            .map(|instance| instance.translation[2])
            .fold(f32::NEG_INFINITY, f32::max);

        assert_eq!(instances.len(), 16);
        assert!(original_count > 16);
        assert!(min_z < 2.0);
        assert!(max_z > 13.0);
    }

    #[test]
    fn interior_leaf_blocks_keep_volume_instances() {
        let context = DecorBlockContext {
            grass: 2,
            leaves: 7,
            air: 0,
            grass_blocker_ids: HashSet::default(),
        };
        let mut chunk = ChunkData::default();
        let center = 8;
        for (x, y, z) in [
            (center, center, center),
            (center + 1, center, center),
            (center - 1, center, center),
            (center, center + 1, center),
            (center, center - 1, center),
            (center, center, center + 1),
            (center, center, center - 1),
        ] {
            chunk.set_block(x, y, z, context.leaves);
        }

        let mut instances = Vec::new();
        append_leaf_lod_instances(&chunk, center, center, center, &context, &mut instances);

        assert_eq!(instances.len(), 3);
    }

    #[test]
    fn zero_cap_returns_empty_grass() {
        let context = test_context();
        let chunk = grass_layer_chunk(&context);
        let (instances, original) = grass_instances(&chunk, &context, 0);
        assert!(instances.is_empty());
        assert_eq!(original, 0);
    }
}
