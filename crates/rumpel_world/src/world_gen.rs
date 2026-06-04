use crate::chunk::{CHUNK_SIZE, ChunkData, WorldEditStore};
use bevy::{
    platform::collections::HashMap,
    prelude::{error, info},
};
use noise::{NoiseFn, Perlin};
use rumpel_blocks::AIR_BLOCK_ID;
use rumpel_blocks::{BlockId, BlockRegistry};
use rumpel_coords::{ChunkPos, LocalBlockPos};
use std::{cell::RefCell, fs, rc::Rc};

const TERRAIN_SEED: u32 = 1337;
const TERRAIN_NOISE_SCALE: f64 = 0.02;
const TERRAIN_BASE_HEIGHT: f64 = 10.0;
const TERRAIN_HEIGHT_RANGE: f64 = 40.0;
const BIOME_TEMPERATURE_SEED: u32 = 21_337;
const BIOME_HUMIDITY_SEED: u32 = 31_337;
const BIOME_NOISE_SCALE: f64 = 0.006;
const BIOME_MOUNTAIN_HEIGHT_THRESHOLD: usize = 38;
const BIOME_FOREST_HUMIDITY_THRESHOLD: f32 = 0.58;
const BIOME_ROUGHNESS_THRESHOLD: f32 = 0.26;
const BIOME_DESERT_TEMPERATURE_THRESHOLD: f32 = 0.66;
const BIOME_DESERT_HUMIDITY_THRESHOLD: f32 = 0.35;
const BIOME_SNOW_TEMPERATURE_THRESHOLD: f32 = 0.30;
const BIOME_MOUNTAIN_SNOW_HEIGHT: usize = 30;
const ORE_NOISE_SEED: u32 = 51_337;
const ORE_NOISE_SCALE: f64 = 0.08;

// Canyon biome: large mesa regions carved by narrow channels.
const TERRAIN_CANYON_SEED: u32 = 41_337;
const TERRAIN_CANYON_SCALE: f64 = 0.012;
const TERRAIN_CANYON_THRESHOLD: f32 = 0.62;
const TERRAIN_CANYON_MESA_BONUS: i32 = 8;
const TERRAIN_CANYON_CUT_SEED: u32 = 43_337;
const TERRAIN_CANYON_CUT_SCALE: f64 = 0.05;
const TERRAIN_CANYON_CUT_THRESHOLD: f32 = 0.66;
const TERRAIN_CANYON_CUT_DEPTH: i32 = 14;

// Coast cliff: along the shoreline the gradual beach gradient snaps up to a
// sheer wall wherever the cliff noise is active. Inland heights are untouched.
const TERRAIN_COAST_CLIFF_SEED: u32 = 61_337;
const TERRAIN_COAST_CLIFF_SCALE: f64 = 0.02;
const TERRAIN_COAST_CLIFF_THRESHOLD: f32 = 0.55;
const TERRAIN_COAST_CLIFF_RANGE: i32 = 6;
const TERRAIN_COAST_CLIFF_LIFT: i32 = 10;

const DIRT_DEPTH: usize = 3;
pub const SURFACE_BEACH_HEIGHT_THRESHOLD: usize = 14;
const SURFACE_SHELL_HEIGHT_KERNEL: [usize; 5] = [1, 4, 6, 4, 1];
const SURFACE_SHELL_HEIGHT_RADIUS: i32 = 2;
const SURFACE_EDIT_SCAN_HEADROOM: usize = 24;
const SURFACE_EDIT_SCAN_MAX_Y: usize = 96;
const WORLD_GEN_SCRIPT_PATH: &str = "assets/mods/world_gen.lua";
const CHUNK_SIZE_I32: i32 = CHUNK_SIZE as i32;

#[must_use]
pub fn terrain_generation_contract_version() -> u64 {
    let mut hash = FNV64_OFFSET;
    hash = fnv64(hash, u64::from(TERRAIN_SEED));
    hash = fnv64(hash, TERRAIN_NOISE_SCALE.to_bits());
    hash = fnv64(hash, TERRAIN_BASE_HEIGHT.to_bits());
    hash = fnv64(hash, TERRAIN_HEIGHT_RANGE.to_bits());
    hash = fnv64(hash, DIRT_DEPTH as u64);
    hash = fnv64(hash, CHUNK_SIZE as u64);
    hash = fnv64(hash, u64::from(BIOME_TEMPERATURE_SEED));
    hash = fnv64(hash, u64::from(BIOME_HUMIDITY_SEED));
    hash = fnv64(hash, BIOME_NOISE_SCALE.to_bits());
    hash = fnv64(hash, BIOME_MOUNTAIN_HEIGHT_THRESHOLD as u64);
    hash = fnv64(hash, f64::from(BIOME_FOREST_HUMIDITY_THRESHOLD).to_bits());
    hash = fnv64(hash, f64::from(BIOME_ROUGHNESS_THRESHOLD).to_bits());
    hash = fnv64(hash, f64::from(BIOME_DESERT_TEMPERATURE_THRESHOLD).to_bits());
    hash = fnv64(hash, f64::from(BIOME_DESERT_HUMIDITY_THRESHOLD).to_bits());
    hash = fnv64(hash, f64::from(BIOME_SNOW_TEMPERATURE_THRESHOLD).to_bits());
    hash = fnv64(hash, BIOME_MOUNTAIN_SNOW_HEIGHT as u64);
    hash = fnv64(hash, u64::from(TERRAIN_CANYON_SEED));
    hash = fnv64(hash, TERRAIN_CANYON_SCALE.to_bits());
    hash = fnv64(hash, f64::from(TERRAIN_CANYON_THRESHOLD).to_bits());
    hash = fnv64(hash, TERRAIN_CANYON_MESA_BONUS as i64 as u64);
    hash = fnv64(hash, u64::from(TERRAIN_CANYON_CUT_SEED));
    hash = fnv64(hash, TERRAIN_CANYON_CUT_SCALE.to_bits());
    hash = fnv64(hash, f64::from(TERRAIN_CANYON_CUT_THRESHOLD).to_bits());
    hash = fnv64(hash, TERRAIN_CANYON_CUT_DEPTH as i64 as u64);
    hash = fnv64(hash, u64::from(TERRAIN_COAST_CLIFF_SEED));
    hash = fnv64(hash, TERRAIN_COAST_CLIFF_SCALE.to_bits());
    hash = fnv64(hash, f64::from(TERRAIN_COAST_CLIFF_THRESHOLD).to_bits());
    hash = fnv64(hash, TERRAIN_COAST_CLIFF_RANGE as i64 as u64);
    hash = fnv64(hash, TERRAIN_COAST_CLIFF_LIFT as i64 as u64);
    if let Ok(bytes) = fs::read(WORLD_GEN_SCRIPT_PATH) {
        for byte in bytes {
            hash = fnv64(hash, u64::from(byte));
        }
    }
    hash.max(1)
}

#[must_use]
pub fn terrain_surface_contract_version() -> u64 {
    let mut hash = fnv64(FNV64_OFFSET, terrain_generation_contract_version());
    hash = fnv64(hash, SURFACE_BEACH_HEIGHT_THRESHOLD as u64);
    hash = fnv64(hash, SURFACE_SHELL_HEIGHT_RADIUS as u64);
    for weight in SURFACE_SHELL_HEIGHT_KERNEL {
        hash = fnv64(hash, weight as u64);
    }
    hash.max(1)
}

#[derive(Clone, Copy, Debug)]
pub struct TerrainBlockPalette {
    pub air: BlockId,
    pub dirt: BlockId,
    pub grass: BlockId,
    pub stone: BlockId,
}

#[derive(Clone, Debug)]
pub struct WorldGenerationContext {
    pub palette: TerrainBlockPalette,
    name_to_id: HashMap<String, BlockId>,
    id_to_name: HashMap<BlockId, String>,
}

impl WorldGenerationContext {
    #[must_use]
    pub fn from_registry(registry: &BlockRegistry) -> Self {
        let mut name_to_id = HashMap::default();
        let mut id_to_name = HashMap::default();

        for id in 0..=u8::MAX {
            let block_id = BlockId::from(id);
            if let Some(block) = registry.get_block(block_id) {
                name_to_id.insert(block.id.clone(), block_id);
                id_to_name.insert(block_id, block.id.clone());
            }
        }

        name_to_id.insert("air".to_string(), AIR_BLOCK_ID);
        id_to_name.insert(AIR_BLOCK_ID, "air".to_string());

        Self {
            palette: TerrainBlockPalette::from_registry(registry),
            name_to_id,
            id_to_name,
        }
    }

    #[must_use]
    pub fn block_name(&self, id: BlockId) -> &str {
        self.id_to_name
            .get(&id)
            .map(String::as_str)
            .unwrap_or("air")
    }

    #[must_use]
    pub fn block_id(&self, name: &str) -> BlockId {
        self.name_to_id.get(name).copied().unwrap_or(AIR_BLOCK_ID)
    }

    /// Resolve the per-biome top-soil blocks once so column generation and
    /// surface sampling agree without repeated registry lookups.
    #[must_use]
    pub fn biome_surface_blocks(&self) -> BiomeSurfaceBlocks {
        let sand = self.block_id("sand");
        let snow = self.block_id("snow");
        BiomeSurfaceBlocks {
            grass: self.palette.grass,
            sand: if sand == AIR_BLOCK_ID {
                self.palette.grass
            } else {
                sand
            },
            snow: if snow == AIR_BLOCK_ID {
                self.palette.grass
            } else {
                snow
            },
        }
    }
}

/// Top-soil block ids selected per biome by [`terrain_biome_surface_block`].
#[derive(Clone, Copy, Debug)]
pub struct BiomeSurfaceBlocks {
    pub grass: BlockId,
    pub sand: BlockId,
    pub snow: BlockId,
}

impl TerrainBlockPalette {
    #[must_use]
    pub fn from_registry(registry: &BlockRegistry) -> Self {
        Self {
            air: registry.get_id("air").unwrap_or(0),
            dirt: registry.get_id("dirt").unwrap_or(0),
            grass: registry.get_id("grass").unwrap_or(0),
            stone: registry.get_id("stone").unwrap_or(0),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TerrainSurfaceSample {
    pub height: usize,
    pub top_block: BlockId,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TerrainBiome {
    Beach,
    Plains,
    Forest,
    Mountains,
    Desert,
    Snow,
    Canyon,
}

impl TerrainBiome {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Beach => "beach",
            Self::Plains => "plains",
            Self::Forest => "forest",
            Self::Mountains => "mountains",
            Self::Desert => "desert",
            Self::Snow => "snow",
            Self::Canyon => "canyon",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WorldTerrainSample {
    pub global_x: i32,
    pub global_z: i32,
    pub height: usize,
    pub chunk_height: usize,
    pub biome: TerrainBiome,
    pub surface_block: BlockId,
    pub subsurface_block: BlockId,
    pub temperature: f32,
    pub humidity: f32,
    pub roughness: f32,
}

#[must_use]
pub fn terrain_height_at(global_x: i32, global_z: i32) -> usize {
    let perlin = Perlin::new(TERRAIN_SEED);
    terrain_height_with_noise(global_x, global_z, &perlin)
}

#[must_use]
pub fn terrain_height_with_noise(global_x: i32, global_z: i32, perlin: &Perlin) -> usize {
    let mut height = base_terrain_height_with_noise(global_x, global_z, perlin);

    if terrain_canyon_value_at(global_x, global_z) >= TERRAIN_CANYON_THRESHOLD {
        height = height.saturating_add(TERRAIN_CANYON_MESA_BONUS);
        if terrain_canyon_cut_value_at(global_x, global_z) >= TERRAIN_CANYON_CUT_THRESHOLD {
            height = height.saturating_sub(TERRAIN_CANYON_CUT_DEPTH);
        }
    }

    let sea_level = SURFACE_BEACH_HEIGHT_THRESHOLD as i32;
    if height > sea_level
        && height <= sea_level + TERRAIN_COAST_CLIFF_RANGE
        && terrain_coast_cliff_value_at(global_x, global_z) >= TERRAIN_COAST_CLIFF_THRESHOLD
    {
        height = sea_level + TERRAIN_COAST_CLIFF_LIFT;
    }

    usize::try_from(height.max(0)).unwrap_or(0)
}

/// Raw heightfield from the base terrain Perlin, before canyon/cliff
/// modifiers. Used internally so feature noise lookups never feed back into
/// themselves through [`terrain_height_with_noise`].
fn base_terrain_height_with_noise(global_x: i32, global_z: i32, perlin: &Perlin) -> i32 {
    let noise_val = perlin.get([
        f64::from(global_x) * TERRAIN_NOISE_SCALE,
        f64::from(global_z) * TERRAIN_NOISE_SCALE,
    ]);
    ((noise_val + 1.0) * 0.5 * TERRAIN_HEIGHT_RANGE + TERRAIN_BASE_HEIGHT) as i32
}

#[must_use]
fn terrain_canyon_value_at(global_x: i32, global_z: i32) -> f32 {
    normalized_perlin_sample(
        global_x,
        global_z,
        TERRAIN_CANYON_SCALE,
        &Perlin::new(TERRAIN_CANYON_SEED),
    )
}

#[must_use]
fn terrain_canyon_cut_value_at(global_x: i32, global_z: i32) -> f32 {
    normalized_perlin_sample(
        global_x,
        global_z,
        TERRAIN_CANYON_CUT_SCALE,
        &Perlin::new(TERRAIN_CANYON_CUT_SEED),
    )
}

#[must_use]
fn terrain_coast_cliff_value_at(global_x: i32, global_z: i32) -> f32 {
    normalized_perlin_sample(
        global_x,
        global_z,
        TERRAIN_COAST_CLIFF_SCALE,
        &Perlin::new(TERRAIN_COAST_CLIFF_SEED),
    )
}

#[must_use]
pub fn terrain_perlin() -> Perlin {
    Perlin::new(TERRAIN_SEED)
}

#[must_use]
pub fn terrain_temperature_at(global_x: i32, global_z: i32) -> f32 {
    normalized_perlin_sample(
        global_x,
        global_z,
        BIOME_NOISE_SCALE,
        &Perlin::new(BIOME_TEMPERATURE_SEED),
    )
}

#[must_use]
pub fn terrain_humidity_at(global_x: i32, global_z: i32) -> f32 {
    normalized_perlin_sample(
        global_x,
        global_z,
        BIOME_NOISE_SCALE,
        &Perlin::new(BIOME_HUMIDITY_SEED),
    )
}

#[must_use]
pub fn terrain_roughness_at(global_x: i32, global_z: i32) -> f32 {
    let center = terrain_height_at(global_x, global_z);
    let neighbor_delta = [
        terrain_height_at(global_x + 2, global_z),
        terrain_height_at(global_x - 2, global_z),
        terrain_height_at(global_x, global_z + 2),
        terrain_height_at(global_x, global_z - 2),
    ]
    .into_iter()
    .map(|height| height.abs_diff(center))
    .max()
    .unwrap_or(0);

    (neighbor_delta as f32 / TERRAIN_HEIGHT_RANGE as f32).clamp(0.0, 1.0)
}

#[must_use]
pub fn terrain_biome_at(global_x: i32, global_z: i32) -> TerrainBiome {
    let height = terrain_height_at(global_x, global_z);
    if height <= SURFACE_BEACH_HEIGHT_THRESHOLD {
        return TerrainBiome::Beach;
    }

    if terrain_canyon_value_at(global_x, global_z) >= TERRAIN_CANYON_THRESHOLD {
        return TerrainBiome::Canyon;
    }

    let roughness = terrain_roughness_at(global_x, global_z);
    if height >= BIOME_MOUNTAIN_HEIGHT_THRESHOLD || roughness >= BIOME_ROUGHNESS_THRESHOLD {
        return TerrainBiome::Mountains;
    }

    let temperature = terrain_temperature_at(global_x, global_z);
    let humidity = terrain_humidity_at(global_x, global_z);

    if temperature <= BIOME_SNOW_TEMPERATURE_THRESHOLD {
        return TerrainBiome::Snow;
    }
    if temperature >= BIOME_DESERT_TEMPERATURE_THRESHOLD
        && humidity < BIOME_DESERT_HUMIDITY_THRESHOLD
    {
        return TerrainBiome::Desert;
    }
    if humidity >= BIOME_FOREST_HUMIDITY_THRESHOLD {
        TerrainBiome::Forest
    } else {
        TerrainBiome::Plains
    }
}

/// Top-soil block for a biome column, painted into the chunk shell so packed,
/// GPU, and sampling paths all see the biome surface without a Lua repaint.
#[must_use]
pub fn terrain_biome_surface_block(
    biome: TerrainBiome,
    height: usize,
    surface: BiomeSurfaceBlocks,
) -> BlockId {
    match biome {
        TerrainBiome::Beach | TerrainBiome::Desert | TerrainBiome::Canyon => surface.sand,
        TerrainBiome::Snow => surface.snow,
        TerrainBiome::Mountains if height >= BIOME_MOUNTAIN_SNOW_HEIGHT => surface.snow,
        _ => surface.grass,
    }
}

/// Deterministic 3D value in `[0, 1)` from a salt and global block coordinate.
///
/// 3D analogue of [`worldgen_rand01`] for scattering ores and other
/// volume-based features that vary with depth.
#[must_use]
pub fn worldgen_rand3d(salt: &str, global_x: i32, world_y: i32, global_z: i32) -> f64 {
    let mut hash = fnv64(FNV64_OFFSET, 0x3D0F_FE5E_7C0D_EBAD_u64);
    hash = fnv64(hash, (i64::from(global_x)) as u64);
    hash = fnv64(hash, (i64::from(world_y)) as u64);
    hash = fnv64(hash, (i64::from(global_z)) as u64);
    for byte in salt.as_bytes() {
        hash = fnv64(hash, u64::from(*byte));
    }
    let mantissa = hash >> 11;
    mantissa as f64 / ((1_u64 << 53) as f64)
}

/// Smooth 3D ore-vein field in `[0, 1]`, used to grow connected ore pockets
/// rather than isolated specks.
#[must_use]
pub fn terrain_ore_noise_at(global_x: i32, world_y: i32, global_z: i32) -> f32 {
    let perlin = Perlin::new(ORE_NOISE_SEED);
    let value = perlin.get([
        f64::from(global_x) * ORE_NOISE_SCALE,
        f64::from(world_y) * ORE_NOISE_SCALE,
        f64::from(global_z) * ORE_NOISE_SCALE,
    ]);
    ((value + 1.0) * 0.5).clamp(0.0, 1.0) as f32
}

#[must_use]
pub fn terrain_world_sample_at(
    global_x: i32,
    global_z: i32,
    context: &WorldGenerationContext,
    surface_material: BlockId,
) -> WorldTerrainSample {
    let height = terrain_height_at(global_x, global_z);
    let chunk_height = height.min(CHUNK_SIZE - 1);
    let biome = terrain_biome_at(global_x, global_z);
    let mut surface_blocks = context.biome_surface_blocks();
    if surface_material != context.palette.air {
        surface_blocks.sand = surface_material;
    }
    let surface_block = terrain_biome_surface_block(biome, height, surface_blocks);
    let subsurface_block =
        terrain_block_at_height(height.saturating_sub(2), height, context.palette);

    WorldTerrainSample {
        global_x,
        global_z,
        height,
        chunk_height,
        biome,
        surface_block,
        subsurface_block,
        temperature: terrain_temperature_at(global_x, global_z),
        humidity: terrain_humidity_at(global_x, global_z),
        roughness: terrain_roughness_at(global_x, global_z),
    }
}

fn normalized_perlin_sample(global_x: i32, global_z: i32, scale: f64, perlin: &Perlin) -> f32 {
    let value = perlin.get([f64::from(global_x) * scale, f64::from(global_z) * scale]);
    ((value + 1.0) * 0.5).clamp(0.0, 1.0) as f32
}

#[must_use]
pub fn terrain_surface_shell_height_with_noise(
    global_x: i32,
    global_z: i32,
    perlin: &Perlin,
) -> usize {
    let mut weighted_sum = 0;
    let mut weight_sum = 0;

    for (kernel_z, weight_z) in SURFACE_SHELL_HEIGHT_KERNEL.iter().copied().enumerate() {
        let sample_z = global_z + kernel_z as i32 - SURFACE_SHELL_HEIGHT_RADIUS;
        for (kernel_x, weight_x) in SURFACE_SHELL_HEIGHT_KERNEL.iter().copied().enumerate() {
            let sample_x = global_x + kernel_x as i32 - SURFACE_SHELL_HEIGHT_RADIUS;
            let weight = weight_x * weight_z;
            weighted_sum += terrain_height_with_noise(sample_x, sample_z, perlin) * weight;
            weight_sum += weight;
        }
    }

    (weighted_sum + weight_sum / 2) / weight_sum
}

#[must_use]
pub fn terrain_surface_cell_height_with_noise(
    global_x: i32,
    global_z: i32,
    width: usize,
    depth: usize,
    perlin: &Perlin,
) -> usize {
    let mut height_sum = 0;
    let mut sample_count = 0;

    for offset_z in 0..depth {
        for offset_x in 0..width {
            height_sum += terrain_surface_shell_height_with_noise(
                global_x + offset_x as i32,
                global_z + offset_z as i32,
                perlin,
            );
            sample_count += 1;
        }
    }

    (height_sum + sample_count / 2)
        .checked_div(sample_count)
        .unwrap_or(0)
}

#[must_use]
pub fn terrain_surface_top_block(
    height: usize,
    palette: TerrainBlockPalette,
    surface_material: BlockId,
) -> BlockId {
    if height <= SURFACE_BEACH_HEIGHT_THRESHOLD && surface_material != palette.air {
        surface_material
    } else {
        terrain_block_at_height(height.saturating_sub(1), height, palette)
    }
}

#[must_use]
pub fn terrain_surface_cell_sample_with_noise(
    global_x: i32,
    global_z: i32,
    width: usize,
    depth: usize,
    palette: TerrainBlockPalette,
    surface_material: BlockId,
    perlin: &Perlin,
) -> TerrainSurfaceSample {
    let height = terrain_surface_cell_height_with_noise(global_x, global_z, width, depth, perlin);
    let top_block = terrain_surface_top_block(height, palette, surface_material);

    TerrainSurfaceSample { height, top_block }
}

#[must_use]
pub fn terrain_block_at_surface_world(
    global_x: i32,
    world_y: usize,
    global_z: i32,
    palette: TerrainBlockPalette,
    edit_store: &WorldEditStore,
    perlin: &Perlin,
) -> BlockId {
    let chunk_x = global_x.div_euclid(CHUNK_SIZE as i32);
    let chunk_z = global_z.div_euclid(CHUNK_SIZE as i32);
    let local_x = global_x.rem_euclid(CHUNK_SIZE as i32);
    let local_z = global_z.rem_euclid(CHUNK_SIZE as i32);
    let chunk_pos = ChunkPos::new(chunk_x, chunk_z);

    if let (Ok(local_x), Ok(local_z), Ok(local_y)) = (
        u8::try_from(local_x),
        u8::try_from(local_z),
        u16::try_from(world_y),
    ) {
        let local = LocalBlockPos::new(local_x, local_y, local_z);
        if let Some(block) = edit_store.block_at(chunk_pos, local) {
            return block;
        }
    }

    let surface_height = terrain_height_with_noise(global_x, global_z, perlin);
    terrain_block_at_height(world_y, surface_height, palette)
}

#[must_use]
pub fn terrain_surface_column_top_height_with_edits(
    global_x: i32,
    global_z: i32,
    palette: TerrainBlockPalette,
    edit_store: &WorldEditStore,
    perlin: &Perlin,
) -> usize {
    let procedural = terrain_surface_shell_height_with_noise(global_x, global_z, perlin);
    let scan_top = procedural
        .saturating_add(SURFACE_EDIT_SCAN_HEADROOM)
        .clamp(1, SURFACE_EDIT_SCAN_MAX_Y);
    let mut top = 0usize;

    for world_y in 0..scan_top {
        let block = terrain_block_at_surface_world(
            global_x, world_y, global_z, palette, edit_store, perlin,
        );
        if block != palette.air {
            top = top.max(world_y + 1);
        }
    }

    if top == 0 { procedural } else { top }
}

#[must_use]
pub fn terrain_surface_shell_height_with_edits(
    global_x: i32,
    global_z: i32,
    palette: TerrainBlockPalette,
    edit_store: &WorldEditStore,
    perlin: &Perlin,
) -> usize {
    let mut weighted_sum = 0;
    let mut weight_sum = 0;

    for (kernel_z, weight_z) in SURFACE_SHELL_HEIGHT_KERNEL.iter().copied().enumerate() {
        let sample_z = global_z + kernel_z as i32 - SURFACE_SHELL_HEIGHT_RADIUS;
        for (kernel_x, weight_x) in SURFACE_SHELL_HEIGHT_KERNEL.iter().copied().enumerate() {
            let sample_x = global_x + kernel_x as i32 - SURFACE_SHELL_HEIGHT_RADIUS;
            let weight = weight_x * weight_z;
            weighted_sum += terrain_surface_column_top_height_with_edits(
                sample_x, sample_z, palette, edit_store, perlin,
            ) * weight;
            weight_sum += weight;
        }
    }

    (weighted_sum + weight_sum / 2) / weight_sum
}

#[allow(clippy::too_many_arguments)]
#[must_use]
pub fn terrain_surface_cell_height_with_edits(
    global_x: i32,
    global_z: i32,
    width: usize,
    depth: usize,
    palette: TerrainBlockPalette,
    edit_store: &WorldEditStore,
    perlin: &Perlin,
) -> usize {
    let mut height_sum = 0;
    let mut sample_count = 0;

    for offset_z in 0..depth {
        for offset_x in 0..width {
            height_sum += terrain_surface_shell_height_with_edits(
                global_x + offset_x as i32,
                global_z + offset_z as i32,
                palette,
                edit_store,
                perlin,
            );
            sample_count += 1;
        }
    }

    (height_sum + sample_count / 2)
        .checked_div(sample_count)
        .unwrap_or(0)
}

#[allow(clippy::too_many_arguments)]
#[must_use]
pub fn terrain_surface_cell_sample_with_edits(
    global_x: i32,
    global_z: i32,
    width: usize,
    depth: usize,
    palette: TerrainBlockPalette,
    surface_material: BlockId,
    edit_store: &WorldEditStore,
    perlin: &Perlin,
) -> TerrainSurfaceSample {
    let height = terrain_surface_cell_height_with_edits(
        global_x, global_z, width, depth, palette, edit_store, perlin,
    );
    let sample_x = global_x + (width / 2) as i32;
    let sample_z = global_z + (depth / 2) as i32;
    let top_y = height.saturating_sub(1);
    let edited_top =
        terrain_block_at_surface_world(sample_x, top_y, sample_z, palette, edit_store, perlin);
    let top_block = if edited_top != palette.air {
        edited_top
    } else {
        terrain_surface_top_block(height, palette, surface_material)
    };

    TerrainSurfaceSample { height, top_block }
}

#[must_use]
pub fn terrain_surface_wall_block_at_y(
    top_block: BlockId,
    surface_height: usize,
    y: usize,
    width: usize,
    depth: usize,
    palette: TerrainBlockPalette,
) -> BlockId {
    if top_block != palette.grass {
        return top_block;
    }

    if width > 1 || depth > 1 {
        let vegetated_depth = width.max(depth);
        if y.saturating_add(vegetated_depth) >= surface_height {
            palette.grass
        } else {
            palette.dirt
        }
    } else {
        terrain_block_at_height(y, surface_height, palette)
    }
}

#[must_use]
pub fn terrain_block_at_height(
    y: usize,
    surface_height: usize,
    palette: TerrainBlockPalette,
) -> BlockId {
    if y >= surface_height {
        palette.air
    } else if y == surface_height - 1 {
        palette.grass
    } else if y > surface_height.saturating_sub(DIRT_DEPTH + 1) {
        palette.dirt
    } else {
        palette.stone
    }
}

#[must_use]
pub fn is_terrain_shell_block(
    block: BlockId,
    palette: TerrainBlockPalette,
    sand: BlockId,
    snow: BlockId,
) -> bool {
    block == palette.stone
        || block == palette.dirt
        || block == palette.grass
        || block == sand
        || block == snow
}

#[must_use]
pub fn terrain_column_top_in_chunk(
    chunk: &ChunkData,
    local_x: usize,
    local_z: usize,
    palette: TerrainBlockPalette,
    sand: BlockId,
    snow: BlockId,
) -> TerrainSurfaceSample {
    for y in (0..CHUNK_SIZE).rev() {
        let block = chunk.get_block(local_x, y, local_z);
        if block == palette.air {
            continue;
        }
        if is_terrain_shell_block(block, palette, sand, snow) {
            return TerrainSurfaceSample {
                height: y + 1,
                top_block: block,
            };
        }
    }
    TerrainSurfaceSample {
        height: 0,
        top_block: palette.air,
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "Surface cell sampling threads chunk geometry, palette, and biome shell blocks."
)]
#[must_use]
pub fn terrain_surface_cell_sample_from_chunk_local(
    chunk: &ChunkData,
    local_x: usize,
    local_z: usize,
    width: usize,
    depth: usize,
    palette: TerrainBlockPalette,
    sand: BlockId,
    snow: BlockId,
) -> TerrainSurfaceSample {
    let mut height_sum = 0usize;
    let mut sample_count = 0usize;

    for dz in 0..depth {
        for dx in 0..width {
            let x = local_x + dx;
            let z = local_z + dz;
            if x >= CHUNK_SIZE || z >= CHUNK_SIZE {
                continue;
            }
            height_sum += terrain_column_top_in_chunk(chunk, x, z, palette, sand, snow).height;
            sample_count += 1;
        }
    }

    if sample_count == 0 {
        return TerrainSurfaceSample {
            height: 0,
            top_block: palette.air,
        };
    }

    let height = (height_sum + sample_count / 2) / sample_count;
    let center_x = (local_x + width / 2).min(CHUNK_SIZE - 1);
    let center_z = (local_z + depth / 2).min(CHUNK_SIZE - 1);
    let mut top_block =
        terrain_column_top_in_chunk(chunk, center_x, center_z, palette, sand, snow).top_block;
    if top_block == palette.air && height > 0 {
        top_block = chunk.get_block(center_x, height - 1, center_z);
    }

    TerrainSurfaceSample { height, top_block }
}

#[must_use]
pub fn terrain_surface_cell_sample_from_world_cached(
    world_x: i32,
    world_z: i32,
    width: usize,
    depth: usize,
    context: &WorldGenerationContext,
) -> TerrainSurfaceSample {
    let chunk_x = world_x.div_euclid(CHUNK_SIZE as i32);
    let chunk_z = world_z.div_euclid(CHUNK_SIZE as i32);
    let local_x = usize::try_from(world_x.rem_euclid(CHUNK_SIZE as i32)).unwrap_or(0);
    let local_z = usize::try_from(world_z.rem_euclid(CHUNK_SIZE as i32)).unwrap_or(0);
    let sand = context.block_id("sand");
    let snow = context.block_id("snow");
    let generated =
        crate::chunk_gen_cache::cached_chunk(ChunkPos::new(chunk_x, chunk_z), context);
    let sample = terrain_surface_cell_sample_from_chunk_local(
        &generated.chunk,
        local_x,
        local_z,
        width,
        depth,
        context.palette,
        sand,
        snow,
    );
    if sample.height > 0 {
        return sample;
    }

    let perlin = terrain_perlin();
    terrain_surface_cell_sample_with_noise(
        world_x,
        world_z,
        width,
        depth,
        context.palette,
        sand,
        &perlin,
    )
}

pub fn generate_chunk(pos: ChunkPos, registry: &BlockRegistry) -> ChunkData {
    let context = WorldGenerationContext::from_registry(registry);
    generate_chunk_with_context(pos, &context)
}

#[must_use]
pub fn generate_chunk_with_context(pos: ChunkPos, context: &WorldGenerationContext) -> ChunkData {
    let mut chunk = ChunkData::default();
    let perlin = Perlin::new(TERRAIN_SEED);
    let surface_blocks = context.biome_surface_blocks();

    for x in 0..CHUNK_SIZE {
        for z in 0..CHUNK_SIZE {
            let global_x = pos.x * CHUNK_SIZE as i32 + x as i32;
            let global_z = pos.z * CHUNK_SIZE as i32 + z as i32;
            let height = terrain_height_with_noise(global_x, global_z, &perlin);
            let biome = terrain_biome_at(global_x, global_z);
            let surface_block = terrain_biome_surface_block(biome, height, surface_blocks);
            // Clamp the surface row to the chunk ceiling so canyon mesas and
            // mountain columns whose computed height > CHUNK_SIZE still receive
            // a biome surface block instead of bare stone at the top.
            let surface_y = height.saturating_sub(1).min(CHUNK_SIZE - 1);

            for y in 0..CHUNK_SIZE {
                let mut block_id = terrain_block_at_height(y, height, context.palette);
                if y == surface_y && y < height {
                    block_id = surface_block;
                }
                if block_id != context.palette.air {
                    chunk.set_block(x, y, z, block_id);
                }
            }
        }
    }

    apply_lua_world_gen(pos, &mut chunk, context);

    chunk
}

#[derive(Clone)]
pub struct GeneratedChunk {
    pub chunk: ChunkData,
    pub decor: crate::surface_decor::ChunkDecorOutput,
}

#[must_use]
pub fn generate_chunk_uncached(pos: ChunkPos, context: &WorldGenerationContext) -> GeneratedChunk {
    GeneratedChunk {
        chunk: generate_chunk_with_context(pos, context),
        decor: crate::surface_decor::ChunkDecorOutput::default(),
    }
}

#[must_use]
pub fn terrain_surface_cell_height_from_world_cached(
    world_x: i32,
    world_z: i32,
    width: usize,
    depth: usize,
    context: &WorldGenerationContext,
) -> usize {
    terrain_surface_cell_sample_from_world_cached(world_x, world_z, width, depth, context).height
}

fn apply_lua_world_gen(pos: ChunkPos, chunk: &mut ChunkData, context: &WorldGenerationContext) {
    let Ok(script) = fs::read_to_string(WORLD_GEN_SCRIPT_PATH) else {
        return;
    };

    let lua = mlua::Lua::new();
    let globals = lua.globals();
    let origin_x = pos.x * CHUNK_SIZE_I32;
    let origin_z = pos.z * CHUNK_SIZE_I32;
    let sand_block = context.block_id("sand");

    let Ok(chunk_table) = lua.create_table() else {
        return;
    };
    let _ = chunk_table.set("x", pos.x);
    let _ = chunk_table.set("z", pos.z);
    let _ = chunk_table.set("size", CHUNK_SIZE);
    let _ = chunk_table.set("origin_x", origin_x);
    let _ = chunk_table.set("origin_z", origin_z);
    let _ = globals.set("Chunk", chunk_table);

    let blocks_cell = Rc::new(RefCell::new(chunk.blocks.clone()));
    let id_to_name = context.id_to_name.clone();
    let name_to_id = context.name_to_id.clone();
    let stats = Rc::new(RefCell::new(LuaWorldGenStats::default()));

    let get_block_buffer = Rc::clone(&blocks_cell);
    let get_block_stats = Rc::clone(&stats);
    let get_block = lua.create_function(move |_, (x, y, z): (i32, i32, i32)| {
        if let Some(index) = local_block_index(x, y, z) {
            let id = get_block_buffer.borrow()[index];
            Ok(id_to_name
                .get(&id)
                .cloned()
                .unwrap_or_else(|| "air".to_string()))
        } else {
            get_block_stats.borrow_mut().get_out_of_bounds += 1;
            Ok("air".to_string())
        }
    });
    if let Ok(function) = get_block {
        let _ = globals.set("get_block", function);
    }

    let set_block_buffer = Rc::clone(&blocks_cell);
    let set_block_stats = Rc::clone(&stats);
    let set_block = lua.create_function(move |_, (x, y, z, name): (i32, i32, i32, String)| {
        let mut stats = set_block_stats.borrow_mut();
        stats.set_attempts += 1;

        let Some(index) = local_block_index(x, y, z) else {
            stats.set_out_of_bounds += 1;
            return Ok(());
        };

        let Some(id) = name_to_id.get(&name).copied() else {
            stats.unknown_block_names += 1;
            return Ok(());
        };

        set_block_buffer.borrow_mut()[index] = id;
        stats.set_applied += 1;
        Ok(())
    });
    if let Ok(function) = set_block {
        let _ = globals.set("set_block", function);
    }

    let local_origin_x = origin_x;
    let to_world_x = lua.create_function(move |_, x: i32| Ok(local_origin_x + x));
    if let Ok(function) = to_world_x {
        let _ = globals.set("chunk_to_world_x", function);
    }

    let local_origin_z = origin_z;
    let to_world_z = lua.create_function(move |_, z: i32| Ok(local_origin_z + z));
    if let Ok(function) = to_world_z {
        let _ = globals.set("chunk_to_world_z", function);
    }

    let local_origin_x = origin_x;
    let to_chunk_x = lua.create_function(move |_, x: i32| Ok(x - local_origin_x));
    if let Ok(function) = to_chunk_x {
        let _ = globals.set("world_to_chunk_x", function);
    }

    let local_origin_z = origin_z;
    let to_chunk_z = lua.create_function(move |_, z: i32| Ok(z - local_origin_z));
    if let Ok(function) = to_chunk_z {
        let _ = globals.set("world_to_chunk_z", function);
    }

    let sample_context = context.clone();
    let sample_stats = Rc::clone(&stats);
    let sample_world = lua.create_function(move |lua, (x, z): (i32, i32)| {
        sample_stats.borrow_mut().world_sample_requests += 1;
        let global_x = origin_x + x;
        let global_z = origin_z + z;
        let sample = terrain_world_sample_at(global_x, global_z, &sample_context, sand_block);
        let table = lua.create_table()?;

        table.set("x", global_x)?;
        table.set("z", global_z)?;
        table.set("local_x", x)?;
        table.set("local_z", z)?;
        table.set("height", sample.height)?;
        table.set("chunk_height", sample.chunk_height)?;
        table.set("biome", sample.biome.as_str())?;
        table.set(
            "surface_block",
            sample_context.block_name(sample.surface_block),
        )?;
        table.set(
            "subsurface_block",
            sample_context.block_name(sample.subsurface_block),
        )?;
        table.set("temperature", sample.temperature)?;
        table.set("humidity", sample.humidity)?;
        table.set("roughness", sample.roughness)?;

        Ok(table)
    });
    if let Ok(function) = sample_world {
        let _ = globals.set("sample_world", function);
    }

    let biome_context = context.clone();
    let get_biome = lua.create_function(move |_, (x, z): (i32, i32)| {
        let sample =
            terrain_world_sample_at(origin_x + x, origin_z + z, &biome_context, sand_block);
        Ok(sample.biome.as_str().to_string())
    });
    if let Ok(function) = get_biome {
        let _ = globals.set("get_biome", function);
    }

    let rand_stats = Rc::clone(&stats);
    let rand01 = lua.create_function(move |_, (salt, x, z): (String, i32, i32)| {
        rand_stats.borrow_mut().deterministic_random_requests += 1;
        Ok(worldgen_rand01(&salt, origin_x + x, origin_z + z))
    });
    if let Ok(function) = rand01 {
        let _ = globals.set("rand01", function);
    }

    let chance_stats = Rc::clone(&stats);
    let chance = lua.create_function(
        move |_, (salt, x, z, probability): (String, i32, i32, f64)| {
            chance_stats.borrow_mut().deterministic_random_requests += 1;
            let probability = probability.clamp(0.0, 1.0);
            Ok(worldgen_rand01(&salt, origin_x + x, origin_z + z) < probability)
        },
    );
    if let Ok(function) = chance {
        let _ = globals.set("chance", function);
    }

    let rand3d_stats = Rc::clone(&stats);
    let rand3d = lua.create_function(move |_, (salt, x, y, z): (String, i32, i32, i32)| {
        rand3d_stats.borrow_mut().deterministic_random_requests += 1;
        Ok(worldgen_rand3d(&salt, origin_x + x, y, origin_z + z))
    });
    if let Ok(function) = rand3d {
        let _ = globals.set("rand3d", function);
    }

    let ore_noise = lua.create_function(move |_, (x, y, z): (i32, i32, i32)| {
        Ok(f64::from(terrain_ore_noise_at(origin_x + x, y, origin_z + z)))
    });
    if let Ok(function) = ore_noise {
        let _ = globals.set("ore_noise", function);
    }

    let get_height_stats = Rc::clone(&stats);
    let get_height = lua.create_function(move |_, (x, z): (i32, i32)| {
        get_height_stats.borrow_mut().height_requests += 1;
        let global_x = pos.x * CHUNK_SIZE_I32 + x;
        let global_z = pos.z * CHUNK_SIZE_I32 + z;
        Ok(terrain_height_at(global_x, global_z).min(CHUNK_SIZE - 1))
    });
    if let Ok(function) = get_height {
        let _ = globals.set("get_height", function);
    }

    let spawn_stats = Rc::clone(&stats);
    let spawn_mob =
        lua.create_function(move |_, (_mob_type, _x, _y, _z): (String, f32, f32, f32)| {
            spawn_stats.borrow_mut().mob_spawn_intents += 1;
            Ok(())
        });
    if let Ok(function) = spawn_mob {
        let _ = globals.set("spawn_mob", function);
    }

    let seed = 1337_i64 + i64::from(pos.x) * 73_856_093 + i64::from(pos.z) * 19_349_663;
    let _ = lua.load(format!("math.randomseed({seed})")).exec();

    if let Err(error) = lua.load(&script).set_name(WORLD_GEN_SCRIPT_PATH).exec() {
        error!("WORLD_GEN: Lua post-pass failed for chunk {pos:?}: {error:?}");
    }

    let stats = stats.borrow();
    if stats.has_reportable_work() {
        info!(
            "WORLD_GEN: Lua post-pass stats for chunk {pos:?}: set_applied={}/{} set_oob={} get_oob={} unknown_blocks={} height={} samples={} deterministic_random={} mob_intents={}",
            stats.set_applied,
            stats.set_attempts,
            stats.set_out_of_bounds,
            stats.get_out_of_bounds,
            stats.unknown_block_names,
            stats.height_requests,
            stats.world_sample_requests,
            stats.deterministic_random_requests,
            stats.mob_spawn_intents
        );
    }

    chunk.blocks = blocks_cell.borrow().clone();
}

#[derive(Default, Debug)]
struct LuaWorldGenStats {
    set_attempts: usize,
    set_applied: usize,
    set_out_of_bounds: usize,
    get_out_of_bounds: usize,
    unknown_block_names: usize,
    height_requests: usize,
    world_sample_requests: usize,
    deterministic_random_requests: usize,
    mob_spawn_intents: usize,
}

impl LuaWorldGenStats {
    fn has_reportable_work(&self) -> bool {
        self.set_attempts > 0
            || self.get_out_of_bounds > 0
            || self.unknown_block_names > 0
            || self.world_sample_requests > 0
            || self.mob_spawn_intents > 0
    }
}

fn local_block_index(x: i32, y: i32, z: i32) -> Option<usize> {
    if (0..CHUNK_SIZE_I32).contains(&x)
        && (0..CHUNK_SIZE_I32).contains(&y)
        && (0..CHUNK_SIZE_I32).contains(&z)
    {
        Some(ChunkData::get_index(
            usize::try_from(x).ok()?,
            usize::try_from(y).ok()?,
            usize::try_from(z).ok()?,
        ))
    } else {
        None
    }
}

fn worldgen_rand01(salt: &str, global_x: i32, global_z: i32) -> f64 {
    let mut hash = fnv64(FNV64_OFFSET, 0xA11C_EC0D_E133_7A11);
    hash = fnv64(hash, (i64::from(global_x)) as u64);
    hash = fnv64(hash, (i64::from(global_z)) as u64);
    for byte in salt.as_bytes() {
        hash = fnv64(hash, u64::from(*byte));
    }

    let mantissa = hash >> 11;
    mantissa as f64 / ((1_u64 << 53) as f64)
}

const FNV64_OFFSET: u64 = 14_695_981_039_346_656_037;
const FNV64_PRIME: u64 = 1_099_511_628_211;

fn fnv64(hash: u64, value: u64) -> u64 {
    (hash ^ value).wrapping_mul(FNV64_PRIME)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_world_context() -> WorldGenerationContext {
        let palette = TerrainBlockPalette {
            air: 0,
            dirt: 1,
            grass: 2,
            stone: 3,
        };
        let mut name_to_id = HashMap::default();
        let mut id_to_name = HashMap::default();
        for (id, name) in [
            (palette.air, "air"),
            (palette.dirt, "dirt"),
            (palette.grass, "grass"),
            (palette.stone, "stone"),
            (4, "sand"),
        ] {
            name_to_id.insert(name.to_string(), id);
            id_to_name.insert(id, name.to_string());
        }

        WorldGenerationContext {
            palette,
            name_to_id,
            id_to_name,
        }
    }

    #[test]
    fn terrain_generation_contract_version_is_stable_and_nonzero() {
        let version = terrain_generation_contract_version();

        assert_ne!(version, 0);
        assert_eq!(version, terrain_generation_contract_version());
    }

    #[test]
    fn terrain_surface_contract_version_is_stable_and_nonzero() {
        let version = terrain_surface_contract_version();

        assert_ne!(version, 0);
        assert_eq!(version, terrain_surface_contract_version());
    }

    #[test]
    fn terrain_height_sampling_is_deterministic_across_regions() {
        let perlin = terrain_perlin();
        let samples = [
            (-256, -256),
            (-65, 17),
            (0, 0),
            (31, 31),
            (127, -93),
            (512, 384),
        ];

        for (x, z) in samples {
            assert_eq!(
                terrain_height_with_noise(x, z, &perlin),
                terrain_height_with_noise(x, z, &perlin)
            );
            assert_eq!(
                terrain_height_at(x, z),
                terrain_height_with_noise(x, z, &perlin)
            );
        }
    }

    #[test]
    fn terrain_world_sample_contract_is_deterministic() {
        let context = test_world_context();
        let first = terrain_world_sample_at(128, -64, &context, 4);
        let second = terrain_world_sample_at(128, -64, &context, 4);

        assert_eq!(first, second);
        assert_eq!(first.biome, terrain_biome_at(128, -64));
        assert_eq!(first.chunk_height, first.height.min(CHUNK_SIZE - 1));
        assert!((0.0..=1.0).contains(&first.temperature));
        assert!((0.0..=1.0).contains(&first.humidity));
        assert!((0.0..=1.0).contains(&first.roughness));
        assert!(!first.biome.as_str().is_empty());
        assert!(!context.block_name(first.surface_block).is_empty());
    }

    #[test]
    fn terrain_world_sample_uses_surface_material_for_beach() {
        let context = test_world_context();
        let sand = context.block_id("sand");
        let (x, z) = (-128..=128)
            .flat_map(|z| (-128..=128).map(move |x| (x, z)))
            .find(|(x, z)| terrain_height_at(*x, *z) <= SURFACE_BEACH_HEIGHT_THRESHOLD)
            .expect("deterministic terrain should expose a beach-height sample");

        let sample = terrain_world_sample_at(x, z, &context, sand);

        assert_eq!(sample.biome, TerrainBiome::Beach);
        assert_eq!(sample.surface_block, sand);
    }

    #[test]
    fn deterministic_worldgen_random_is_coordinate_stable() {
        let first = worldgen_rand01("emerald_tree", 12, -4);
        let second = worldgen_rand01("emerald_tree", 12, -4);
        let different_salt = worldgen_rand01("emerald_flower", 12, -4);
        let different_position = worldgen_rand01("emerald_tree", 13, -4);

        assert_eq!(first, second);
        assert!((0.0..1.0).contains(&first));
        assert_ne!(first, different_salt);
        assert_ne!(first, different_position);
    }

    #[test]
    fn lua_worldgen_local_index_rejects_out_of_chunk_positions() {
        assert_eq!(local_block_index(0, 0, 0), Some(0));
        assert!(local_block_index(-1, 0, 0).is_none());
        assert!(local_block_index(0, CHUNK_SIZE_I32, 0).is_none());
        assert!(local_block_index(0, 0, CHUNK_SIZE_I32).is_none());
    }

    #[test]
    fn terrain_surface_sampling_picks_beach_and_grass_materials() {
        let palette = TerrainBlockPalette {
            air: 0,
            dirt: 1,
            grass: 2,
            stone: 3,
        };
        let sand = 4;
        let perlin = terrain_perlin();

        let beach = (-128..=128)
            .flat_map(|z| (-128..=128).map(move |x| (x, z)))
            .map(|(x, z)| {
                terrain_surface_cell_sample_with_noise(x, z, 1, 1, palette, sand, &perlin)
            })
            .find(|sample| sample.height <= SURFACE_BEACH_HEIGHT_THRESHOLD)
            .expect("deterministic terrain should expose a beach-height sample");
        assert_eq!(beach.top_block, sand);

        let grass = (-128..=128)
            .flat_map(|z| (-128..=128).map(move |x| (x, z)))
            .map(|(x, z)| {
                terrain_surface_cell_sample_with_noise(x, z, 1, 1, palette, sand, &perlin)
            })
            .find(|sample| sample.height > SURFACE_BEACH_HEIGHT_THRESHOLD)
            .expect("deterministic terrain should expose a grass-height sample");
        assert_eq!(grass.top_block, palette.grass);
    }

    #[test]
    fn terrain_surface_edits_can_raise_generated_column_height() {
        let perlin = terrain_perlin();
        let palette = TerrainBlockPalette {
            air: 0,
            dirt: 1,
            grass: 2,
            stone: 3,
        };
        let mut edit_store = WorldEditStore::default();
        let global_x = 16;
        let global_z = 16;
        let before = terrain_surface_column_top_height_with_edits(
            global_x,
            global_z,
            palette,
            &edit_store,
            &perlin,
        );
        assert!(before > 0);

        let edit_y = before.saturating_add(4);
        assert!(edit_store.apply_edit(crate::chunk::WorldBlockEdit::new(
            ChunkPos::new(
                global_x.div_euclid(CHUNK_SIZE as i32),
                global_z.div_euclid(CHUNK_SIZE as i32)
            ),
            LocalBlockPos::new(
                global_x.rem_euclid(CHUNK_SIZE as i32) as u8,
                u16::try_from(edit_y).expect("edited surface within chunk"),
                global_z.rem_euclid(CHUNK_SIZE as i32) as u8,
            ),
            palette.grass,
        )));

        let after = terrain_surface_column_top_height_with_edits(
            global_x,
            global_z,
            palette,
            &edit_store,
            &perlin,
        );
        assert_eq!(after, edit_y + 1);
        assert_eq!(
            terrain_block_at_surface_world(
                global_x,
                edit_y,
                global_z,
                palette,
                &edit_store,
                &perlin,
            ),
            palette.grass
        );
    }

    #[test]
    fn terrain_surface_wall_sampling_matches_material_layers() {
        let palette = TerrainBlockPalette {
            air: 0,
            dirt: 1,
            grass: 2,
            stone: 3,
        };
        let height = 20;

        assert_eq!(
            terrain_surface_wall_block_at_y(palette.grass, height, 0, 1, 1, palette),
            palette.stone
        );
        assert_eq!(
            terrain_surface_wall_block_at_y(palette.grass, height, 18, 1, 1, palette),
            palette.dirt
        );
        assert_eq!(
            terrain_surface_wall_block_at_y(palette.grass, height, 19, 1, 1, palette),
            palette.grass
        );
        assert_eq!(
            terrain_surface_wall_block_at_y(palette.grass, height, 16, 4, 4, palette),
            palette.grass
        );
        assert_eq!(
            terrain_surface_wall_block_at_y(4, height, 2, 1, 1, palette),
            4
        );
    }

    #[test]
    fn terrain_surface_sampling_matches_golden_regions() {
        let palette = TerrainBlockPalette {
            air: 0,
            dirt: 1,
            grass: 2,
            stone: 3,
        };
        let sand = 4;
        let perlin = terrain_perlin();
        struct GoldenSurfaceCase {
            x: i32,
            z: i32,
            width: usize,
            depth: usize,
            raw_height: usize,
            shell_height: usize,
            cell_height: usize,
            top_block: BlockId,
        }

        let cases = [
            GoldenSurfaceCase {
                x: -256,
                z: -256,
                width: 1,
                depth: 1,
                raw_height: 29,
                shell_height: 29,
                cell_height: 29,
                top_block: palette.grass,
            },
            GoldenSurfaceCase {
                x: -65,
                z: 17,
                width: 1,
                depth: 1,
                raw_height: 40,
                shell_height: 40,
                cell_height: 40,
                top_block: palette.grass,
            },
            GoldenSurfaceCase {
                x: 0,
                z: 0,
                width: 1,
                depth: 1,
                raw_height: 30,
                shell_height: 30,
                cell_height: 30,
                top_block: palette.grass,
            },
            GoldenSurfaceCase {
                x: 31,
                z: 31,
                width: 1,
                depth: 1,
                raw_height: 41,
                shell_height: 41,
                cell_height: 41,
                top_block: palette.grass,
            },
            GoldenSurfaceCase {
                x: 127,
                z: -93,
                width: 2,
                depth: 2,
                raw_height: 42,
                shell_height: 42,
                cell_height: 42,
                top_block: palette.grass,
            },
            GoldenSurfaceCase {
                // Canyon mesa lifts the raw height by +8 here; the surface
                // kernel still averages into the surrounding base level.
                x: 512,
                z: 384,
                width: 4,
                depth: 3,
                raw_height: 48,
                shell_height: 44,
                cell_height: 48,
                top_block: palette.grass,
            },
            GoldenSurfaceCase {
                // Coast cliff: base column is at sea level but neighbours in
                // the smoothing kernel are lifted, so the shell/cell heights
                // climb above the beach threshold and the surface is grass.
                x: -179,
                z: -512,
                width: 1,
                depth: 1,
                raw_height: 14,
                shell_height: 18,
                cell_height: 18,
                top_block: palette.grass,
            },
        ];

        for case in cases {
            assert_eq!(
                terrain_height_with_noise(case.x, case.z, &perlin),
                case.raw_height,
                "raw terrain height changed at ({}, {})",
                case.x,
                case.z
            );
            assert_eq!(
                terrain_surface_shell_height_with_noise(case.x, case.z, &perlin),
                case.shell_height,
                "surface shell height changed at ({}, {})",
                case.x,
                case.z
            );
            let sample = terrain_surface_cell_sample_with_noise(
                case.x, case.z, case.width, case.depth, palette, sand, &perlin,
            );
            assert_eq!(
                sample,
                TerrainSurfaceSample {
                    height: case.cell_height,
                    top_block: case.top_block,
                },
                "surface cell sample changed at ({}, {}) for {}x{}",
                case.x,
                case.z,
                case.width,
                case.depth
            );
        }
    }
}
