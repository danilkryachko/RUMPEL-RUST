use crate::chunk::{CHUNK_HEIGHT, CHUNK_SIZE, ChunkData, WorldEditStore};
use bevy::{
    platform::collections::HashMap,
    prelude::{error, info},
};
use noise::{NoiseFn, Perlin};
use rumpel_blocks::AIR_BLOCK_ID;
use rumpel_blocks::{BlockId, BlockRegistry};
use rumpel_coords::{ChunkPos, LocalBlockPos};
use std::{
    cell::RefCell,
    fs,
    rc::Rc,
    sync::{
        OnceLock,
        atomic::{AtomicU32, Ordering},
    },
};

pub const DEFAULT_TERRAIN_SEED: u32 = 1337;
const TERRAIN_CONTINENT_SEED: u32 = 7_137;
const TERRAIN_UPLAND_SEED: u32 = 11_337;
const TERRAIN_RIDGE_SEED: u32 = 17_337;
const TERRAIN_VALLEY_SEED: u32 = 19_337;
const TERRAIN_WARP_X_SEED: u32 = 23_337;
const TERRAIN_WARP_Z_SEED: u32 = 29_337;
const TERRAIN_ARIDITY_SEED: u32 = 37_337;
const TERRAIN_BIOME_VARIATION_SEED: u32 = 41_337;
const TERRAIN_LAKE_SEED: u32 = 43_337;
const TERRAIN_CAVE_PRIMARY_SEED: u32 = 47_337;
const TERRAIN_CAVE_DETAIL_SEED: u32 = 53_337;
const TERRAIN_ORE_SEED: u32 = 59_337;
const TERRAIN_NOISE_SCALE: f64 = 0.018;
const TERRAIN_CONTINENT_SCALE: f64 = 0.0015;
const TERRAIN_UPLAND_SCALE: f64 = 0.0042;
const TERRAIN_RIDGE_SCALE: f64 = 0.009;
const TERRAIN_VALLEY_SCALE: f64 = 0.0024;
const TERRAIN_WARP_SCALE: f64 = 0.0032;
const TERRAIN_WARP_STRENGTH: f64 = 24.0;
const TERRAIN_LAKE_SCALE: f64 = 0.0028;
const TERRAIN_BASE_HEIGHT: f64 = 18.0;
const TERRAIN_HEIGHT_RANGE: f64 = 150.0;
const WORLD_WATER_LEVEL: usize = 12;
const BIOME_TEMPERATURE_SEED: u32 = 21_337;
const BIOME_HUMIDITY_SEED: u32 = 31_337;
const BIOME_NOISE_SCALE: f64 = 0.006;
const BIOME_MOUNTAIN_HEIGHT_THRESHOLD: usize = 82;
const BIOME_FOREST_HUMIDITY_THRESHOLD: f32 = 0.58;
const BIOME_ROUGHNESS_THRESHOLD: f32 = 0.16;
const DIRT_DEPTH: usize = 3;
pub const SURFACE_BEACH_HEIGHT_THRESHOLD: usize = 14;
const SURFACE_SHELL_HEIGHT_KERNEL: [usize; 5] = [1, 4, 6, 4, 1];
const SURFACE_SHELL_HEIGHT_RADIUS: i32 = 2;
const SURFACE_EDIT_SCAN_HEADROOM: usize = 24;
const SURFACE_EDIT_SCAN_MAX_Y: usize = CHUNK_HEIGHT;
const WORLD_GEN_SCRIPT_PATH: &str = "assets/mods/world_gen.lua";
const CHUNK_SIZE_I32: i32 = CHUNK_SIZE as i32;
const CHUNK_HEIGHT_I32: i32 = CHUNK_HEIGHT as i32;
const CAVE_SCALE: f64 = 0.044;
const CAVE_DETAIL_SCALE: f64 = 0.095;
const CAVE_MIN_DEPTH_BELOW_SURFACE: usize = 10;
const ORE_SCALE: f64 = 0.078;
const ORE_SURFACE_SAFETY: usize = 7;

struct TerrainFieldNoise {
    detail: Perlin,
    continent: Perlin,
    upland: Perlin,
    ridge: Perlin,
    valley: Perlin,
    aridity: Perlin,
    biome_variation: Perlin,
    lake: Perlin,
    cave_primary: Perlin,
    cave_detail: Perlin,
    ore: Perlin,
    warp_x: Perlin,
    warp_z: Perlin,
    temperature: Perlin,
    humidity: Perlin,
}

impl TerrainFieldNoise {
    fn new(terrain_seed: u32) -> Self {
        Self {
            detail: Perlin::new(terrain_seed),
            continent: Perlin::new(TERRAIN_CONTINENT_SEED),
            upland: Perlin::new(TERRAIN_UPLAND_SEED),
            ridge: Perlin::new(TERRAIN_RIDGE_SEED),
            valley: Perlin::new(TERRAIN_VALLEY_SEED),
            aridity: Perlin::new(TERRAIN_ARIDITY_SEED),
            biome_variation: Perlin::new(TERRAIN_BIOME_VARIATION_SEED),
            lake: Perlin::new(TERRAIN_LAKE_SEED),
            cave_primary: Perlin::new(TERRAIN_CAVE_PRIMARY_SEED),
            cave_detail: Perlin::new(TERRAIN_CAVE_DETAIL_SEED),
            ore: Perlin::new(TERRAIN_ORE_SEED),
            warp_x: Perlin::new(TERRAIN_WARP_X_SEED),
            warp_z: Perlin::new(TERRAIN_WARP_Z_SEED),
            temperature: Perlin::new(BIOME_TEMPERATURE_SEED),
            humidity: Perlin::new(BIOME_HUMIDITY_SEED),
        }
    }
}

static ACTIVE_TERRAIN_SEED: AtomicU32 = AtomicU32::new(DEFAULT_TERRAIN_SEED);
static TERRAIN_FIELD_NOISE: OnceLock<TerrainFieldNoise> = OnceLock::new();

/// Sets the procedural terrain seed for this process and initializes noise tables once.
pub fn init_active_world_terrain(terrain_seed: u32) {
    ACTIVE_TERRAIN_SEED.store(terrain_seed, Ordering::Relaxed);
    let _ = TERRAIN_FIELD_NOISE.get_or_init(|| TerrainFieldNoise::new(terrain_seed));
}

#[must_use]
pub fn active_terrain_seed() -> u32 {
    ACTIVE_TERRAIN_SEED.load(Ordering::Relaxed)
}

#[must_use]
fn terrain_field_noise() -> &'static TerrainFieldNoise {
    TERRAIN_FIELD_NOISE.get_or_init(|| TerrainFieldNoise::new(active_terrain_seed()))
}

#[must_use]
pub fn terrain_generation_contract_version() -> u64 {
    let mut hash = FNV64_OFFSET;
    hash = fnv64(hash, u64::from(active_terrain_seed()));
    hash = fnv64(hash, u64::from(TERRAIN_CONTINENT_SEED));
    hash = fnv64(hash, u64::from(TERRAIN_UPLAND_SEED));
    hash = fnv64(hash, u64::from(TERRAIN_RIDGE_SEED));
    hash = fnv64(hash, u64::from(TERRAIN_VALLEY_SEED));
    hash = fnv64(hash, u64::from(TERRAIN_WARP_X_SEED));
    hash = fnv64(hash, u64::from(TERRAIN_WARP_Z_SEED));
    hash = fnv64(hash, u64::from(TERRAIN_ARIDITY_SEED));
    hash = fnv64(hash, u64::from(TERRAIN_BIOME_VARIATION_SEED));
    hash = fnv64(hash, u64::from(TERRAIN_LAKE_SEED));
    hash = fnv64(hash, u64::from(TERRAIN_CAVE_PRIMARY_SEED));
    hash = fnv64(hash, u64::from(TERRAIN_CAVE_DETAIL_SEED));
    hash = fnv64(hash, u64::from(TERRAIN_ORE_SEED));
    hash = fnv64(hash, TERRAIN_NOISE_SCALE.to_bits());
    hash = fnv64(hash, TERRAIN_CONTINENT_SCALE.to_bits());
    hash = fnv64(hash, TERRAIN_UPLAND_SCALE.to_bits());
    hash = fnv64(hash, TERRAIN_RIDGE_SCALE.to_bits());
    hash = fnv64(hash, TERRAIN_VALLEY_SCALE.to_bits());
    hash = fnv64(hash, TERRAIN_WARP_SCALE.to_bits());
    hash = fnv64(hash, TERRAIN_WARP_STRENGTH.to_bits());
    hash = fnv64(hash, TERRAIN_LAKE_SCALE.to_bits());
    hash = fnv64(hash, TERRAIN_BASE_HEIGHT.to_bits());
    hash = fnv64(hash, TERRAIN_HEIGHT_RANGE.to_bits());
    hash = fnv64(hash, WORLD_WATER_LEVEL as u64);
    hash = fnv64(hash, CAVE_SCALE.to_bits());
    hash = fnv64(hash, CAVE_DETAIL_SCALE.to_bits());
    hash = fnv64(hash, CAVE_MIN_DEPTH_BELOW_SURFACE as u64);
    hash = fnv64(hash, ORE_SCALE.to_bits());
    hash = fnv64(hash, ORE_SURFACE_SAFETY as u64);
    hash = fnv64(hash, DIRT_DEPTH as u64);
    hash = fnv64(hash, CHUNK_SIZE as u64);
    hash = fnv64(hash, CHUNK_HEIGHT as u64);
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
    River,
    Wetlands,
    Plains,
    Forest,
    AutumnForest,
    Taiga,
    Mountains,
    Snow,
    Desert,
    Canyon,
}

impl TerrainBiome {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Beach => "beach",
            Self::River => "river",
            Self::Wetlands => "wetlands",
            Self::Plains => "plains",
            Self::Forest => "forest",
            Self::AutumnForest => "autumn_forest",
            Self::Taiga => "taiga",
            Self::Mountains => "mountains",
            Self::Snow => "snow",
            Self::Desert => "desert",
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
    pub aridity: f32,
    pub river: f32,
    pub lake: f32,
    pub mountain: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TerrainColumnSample {
    pub global_x: i32,
    pub global_z: i32,
    pub height: usize,
    pub biome: TerrainBiome,
    pub temperature: f32,
    pub humidity: f32,
    pub roughness: f32,
    pub aridity: f32,
    pub river: f32,
    pub lake: f32,
    pub mountain: f32,
    pub hill: f32,
    pub valley: f32,
}

#[derive(Clone, Copy, Debug)]
struct TerrainShapeSample {
    height: usize,
    river: f64,
    lake: f64,
    mountain: f64,
    hill: f64,
    valley: f64,
    continent: f64,
    upland: f64,
    aridity: f64,
    biome_variation: f64,
}

#[must_use]
pub fn terrain_height_at(global_x: i32, global_z: i32) -> usize {
    terrain_height_with_noise(global_x, global_z, &terrain_field_noise().detail)
}

#[must_use]
pub fn terrain_height_with_noise(global_x: i32, global_z: i32, perlin: &Perlin) -> usize {
    terrain_shape_with_noise(global_x, global_z, perlin).height
}

#[must_use]
pub fn terrain_column_at(global_x: i32, global_z: i32) -> TerrainColumnSample {
    terrain_column_with_noise(global_x, global_z, &terrain_field_noise().detail)
}

#[must_use]
pub fn terrain_column_with_noise(
    global_x: i32,
    global_z: i32,
    perlin: &Perlin,
) -> TerrainColumnSample {
    let shape = terrain_shape_with_noise(global_x, global_z, perlin);
    let temperature = terrain_temperature_at(global_x, global_z);
    let humidity = terrain_humidity_at(global_x, global_z);
    let roughness = terrain_roughness_at(global_x, global_z);
    let biome = terrain_biome_from_fields(&shape, temperature, humidity, roughness);

    TerrainColumnSample {
        global_x,
        global_z,
        height: shape.height,
        biome,
        temperature,
        humidity,
        roughness,
        aridity: shape.aridity as f32,
        river: shape.river as f32,
        lake: shape.lake as f32,
        mountain: shape.mountain as f32,
        hill: shape.hill as f32,
        valley: shape.valley as f32,
    }
}

fn terrain_shape_with_noise(global_x: i32, global_z: i32, perlin: &Perlin) -> TerrainShapeSample {
    let noise = terrain_field_noise();
    let (warped_x, warped_z) = terrain_warped_coords(global_x, global_z, noise);

    let continent = perlin01(
        &noise.continent,
        warped_x,
        warped_z,
        TERRAIN_CONTINENT_SCALE,
    );
    let upland = perlin01(&noise.upland, warped_x, warped_z, TERRAIN_UPLAND_SCALE);
    let valley = perlin01(&noise.valley, warped_x, warped_z, TERRAIN_VALLEY_SCALE);
    let aridity = perlin01(&noise.aridity, warped_x, warped_z, BIOME_NOISE_SCALE * 0.72);
    let biome_variation = perlin01(
        &noise.biome_variation,
        warped_x,
        warped_z,
        BIOME_NOISE_SCALE * 1.15,
    );
    let lake_noise = perlin01(&noise.lake, warped_x, warped_z, TERRAIN_LAKE_SCALE);
    let detail = fbm01(
        perlin,
        warped_x,
        warped_z,
        TERRAIN_NOISE_SCALE,
        4,
        2.05,
        0.48,
    );
    let ridge_raw = noise.ridge.get([
        warped_x * TERRAIN_RIDGE_SCALE,
        warped_z * TERRAIN_RIDGE_SCALE,
    ]);
    let ridge = (1.0 - ridge_raw.abs().clamp(0.0, 1.0)).powf(2.35);

    let mountain_weight = smoothstep(0.58, 0.88, continent) * smoothstep(0.48, 0.86, upland);
    let hill_weight =
        smoothstep(0.30, 0.68, upland) * (1.0 - mountain_weight * 0.70).clamp(0.0, 1.0);
    let broad_land = smoothstep(0.12, 0.92, continent);
    let valley_axis = 1.0 - ((valley - 0.5).abs() * 2.0).clamp(0.0, 1.0);
    let valley_cut = smoothstep(0.70, 0.96, valley_axis);
    let river = smoothstep(0.86, 0.985, valley_axis)
        * smoothstep(0.14, 0.72, continent)
        * (1.0 - smoothstep(0.62, 0.95, upland) * 0.55);
    let lake = smoothstep(0.78, 0.94, lake_noise)
        * smoothstep(0.10, 0.68, continent)
        * (1.0 - smoothstep(0.50, 0.86, upland));

    let mut height = TERRAIN_BASE_HEIGHT
        + broad_land * 24.0
        + hill_weight * 34.0
        + mountain_weight * ridge * 112.0
        + (detail - 0.5) * (8.0 + hill_weight * 10.0 + mountain_weight * 18.0)
        - valley_cut * (8.0 + mountain_weight * 24.0)
        - river * (8.0 + hill_weight * 8.0 + mountain_weight * 12.0)
        - lake * 8.0;

    if river > 0.76 && height <= WORLD_WATER_LEVEL as f64 + 20.0 {
        height = height.min(WORLD_WATER_LEVEL as f64 + (1.0 - river) * 8.0);
    }
    if lake > 0.82 && height <= WORLD_WATER_LEVEL as f64 + 12.0 {
        height = height.min(WORLD_WATER_LEVEL as f64 + (1.0 - lake) * 6.0);
    }

    let max_height = CHUNK_HEIGHT.saturating_sub(32) as f64;
    TerrainShapeSample {
        height: height.round().clamp(4.0, max_height) as usize,
        river,
        lake,
        mountain: mountain_weight,
        hill: hill_weight,
        valley: valley_axis,
        continent,
        upland,
        aridity,
        biome_variation,
    }
}

#[must_use]
pub fn terrain_perlin() -> Perlin {
    Perlin::new(active_terrain_seed())
}

#[must_use]
pub fn terrain_temperature_at(global_x: i32, global_z: i32) -> f32 {
    normalized_perlin_sample(
        global_x,
        global_z,
        BIOME_NOISE_SCALE,
        &terrain_field_noise().temperature,
    )
}

#[must_use]
pub fn terrain_humidity_at(global_x: i32, global_z: i32) -> f32 {
    normalized_perlin_sample(
        global_x,
        global_z,
        BIOME_NOISE_SCALE,
        &terrain_field_noise().humidity,
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

fn terrain_biome_from_fields(
    shape: &TerrainShapeSample,
    temperature: f32,
    humidity: f32,
    roughness: f32,
) -> TerrainBiome {
    let height = shape.height;
    let temperature = f64::from(temperature);
    let humidity = f64::from(humidity);
    let roughness = f64::from(roughness);
    let cold = (1.0 - temperature).clamp(0.0, 1.0);
    let wet = humidity;
    let dry = (shape.aridity * (1.0 - humidity * 0.55)).clamp(0.0, 1.0);
    let high = smoothstep(54.0, 132.0, height as f64);

    if shape.river >= 0.70 && height <= WORLD_WATER_LEVEL + 8 {
        return TerrainBiome::River;
    }
    if height <= SURFACE_BEACH_HEIGHT_THRESHOLD {
        return TerrainBiome::Beach;
    }
    if shape.lake >= 0.66 && height <= WORLD_WATER_LEVEL + 10 && wet >= 0.48 {
        return TerrainBiome::Wetlands;
    }
    if cold >= 0.78 && (height >= 48 || shape.mountain >= 0.35) {
        return TerrainBiome::Snow;
    }
    if dry >= 0.72 && temperature >= 0.52 && wet <= 0.44 {
        if roughness >= 0.10 || shape.valley >= 0.78 || shape.upland >= 0.58 {
            return TerrainBiome::Canyon;
        }
        return TerrainBiome::Desert;
    }
    if height >= BIOME_MOUNTAIN_HEIGHT_THRESHOLD
        || roughness >= f64::from(BIOME_ROUGHNESS_THRESHOLD)
        || shape.mountain >= 0.62
    {
        return if cold >= 0.55 {
            TerrainBiome::Snow
        } else {
            TerrainBiome::Mountains
        };
    }
    if cold >= 0.52 && wet >= 0.38 {
        return TerrainBiome::Taiga;
    }
    if wet >= f64::from(BIOME_FOREST_HUMIDITY_THRESHOLD) && shape.continent >= 0.22 && dry <= 0.58 {
        return if shape.biome_variation >= 0.66 && temperature <= 0.70 {
            TerrainBiome::AutumnForest
        } else {
            TerrainBiome::Forest
        };
    }
    if wet >= 0.72 && height <= WORLD_WATER_LEVEL + 18 && high <= 0.40 {
        return TerrainBiome::Wetlands;
    }

    TerrainBiome::Plains
}

#[must_use]
pub fn terrain_biome_at(global_x: i32, global_z: i32) -> TerrainBiome {
    terrain_column_at(global_x, global_z).biome
}

#[must_use]
pub fn terrain_world_sample_at(
    global_x: i32,
    global_z: i32,
    context: &WorldGenerationContext,
    surface_material: BlockId,
) -> WorldTerrainSample {
    let column = terrain_column_at(global_x, global_z);
    let height = column.height;
    let chunk_height = height.min(CHUNK_HEIGHT - 1);
    let surface_block = terrain_surface_block_for_column(&column, context, surface_material);
    let subsurface_block = terrain_subsurface_block_for_column(&column, context, surface_material);

    WorldTerrainSample {
        global_x,
        global_z,
        height,
        chunk_height,
        biome: column.biome,
        surface_block,
        subsurface_block,
        temperature: column.temperature,
        humidity: column.humidity,
        roughness: column.roughness,
        aridity: column.aridity,
        river: column.river,
        lake: column.lake,
        mountain: column.mountain,
    }
}

fn normalized_perlin_sample(global_x: i32, global_z: i32, scale: f64, perlin: &Perlin) -> f32 {
    let value = perlin.get([f64::from(global_x) * scale, f64::from(global_z) * scale]);
    ((value + 1.0) * 0.5).clamp(0.0, 1.0) as f32
}

fn terrain_warped_coords(global_x: i32, global_z: i32, noise: &TerrainFieldNoise) -> (f64, f64) {
    let x = f64::from(global_x);
    let z = f64::from(global_z);
    let warp_x = noise
        .warp_x
        .get([x * TERRAIN_WARP_SCALE, z * TERRAIN_WARP_SCALE])
        * TERRAIN_WARP_STRENGTH;
    let warp_z = noise.warp_z.get([
        (x + 1024.0) * TERRAIN_WARP_SCALE,
        (z - 1024.0) * TERRAIN_WARP_SCALE,
    ]) * TERRAIN_WARP_STRENGTH;

    (x + warp_x, z + warp_z)
}

fn perlin01(perlin: &Perlin, x: f64, z: f64, scale: f64) -> f64 {
    ((perlin.get([x * scale, z * scale]) + 1.0) * 0.5).clamp(0.0, 1.0)
}

fn perlin01_3d(perlin: &Perlin, x: f64, y: f64, z: f64, scale: f64) -> f64 {
    ((perlin.get([x * scale, y * scale, z * scale]) + 1.0) * 0.5).clamp(0.0, 1.0)
}

fn fbm01(
    perlin: &Perlin,
    x: f64,
    z: f64,
    scale: f64,
    octaves: usize,
    lacunarity: f64,
    persistence: f64,
) -> f64 {
    let mut frequency = scale;
    let mut amplitude = 1.0;
    let mut value = 0.0;
    let mut amplitude_sum = 0.0;

    for _ in 0..octaves {
        value += perlin01(perlin, x, z, frequency) * amplitude;
        amplitude_sum += amplitude;
        frequency *= lacunarity;
        amplitude *= persistence;
    }

    if amplitude_sum <= f64::EPSILON {
        0.5
    } else {
        value / amplitude_sum
    }
}

fn smoothstep(edge0: f64, edge1: f64, value: f64) -> f64 {
    if edge1 <= edge0 {
        return if value >= edge1 { 1.0 } else { 0.0 };
    }
    let t = ((value - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
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

fn block_id_or(context: &WorldGenerationContext, name: &str, fallback: BlockId) -> BlockId {
    let id = context.block_id(name);
    if id == context.palette.air && name != "air" {
        fallback
    } else {
        id
    }
}

fn terrain_surface_block_for_column(
    column: &TerrainColumnSample,
    context: &WorldGenerationContext,
    surface_material: BlockId,
) -> BlockId {
    let sand = block_id_or(context, "sand", surface_material);
    let snow = block_id_or(context, "snow", context.palette.grass);
    let gravel = block_id_or(context, "gravel", context.palette.stone);
    let clay = block_id_or(context, "clay", context.palette.dirt);

    match column.biome {
        TerrainBiome::Beach | TerrainBiome::Desert => sand,
        TerrainBiome::River => {
            if column.humidity >= 0.58 || column.lake >= 0.55 {
                clay
            } else {
                sand
            }
        }
        TerrainBiome::Wetlands => {
            if column.lake >= 0.58 {
                clay
            } else {
                context.palette.grass
            }
        }
        TerrainBiome::Canyon => {
            if column.roughness >= 0.14 {
                context.palette.stone
            } else {
                sand
            }
        }
        TerrainBiome::Snow => snow,
        TerrainBiome::Mountains if column.roughness >= 0.18 || column.mountain >= 0.70 => {
            if column.height >= 150 && column.temperature <= 0.45 {
                snow
            } else {
                context.palette.stone
            }
        }
        TerrainBiome::Taiga if column.height >= 92 && column.temperature <= 0.32 => snow,
        TerrainBiome::Mountains if column.river >= 0.45 => gravel,
        TerrainBiome::Plains
        | TerrainBiome::Forest
        | TerrainBiome::AutumnForest
        | TerrainBiome::Taiga
        | TerrainBiome::Mountains => context.palette.grass,
    }
}

fn terrain_subsurface_block_for_column(
    column: &TerrainColumnSample,
    context: &WorldGenerationContext,
    surface_material: BlockId,
) -> BlockId {
    let sand = block_id_or(context, "sand", surface_material);
    let gravel = block_id_or(context, "gravel", context.palette.stone);
    let clay = block_id_or(context, "clay", context.palette.dirt);

    match column.biome {
        TerrainBiome::Beach | TerrainBiome::Desert | TerrainBiome::Canyon => sand,
        TerrainBiome::River | TerrainBiome::Wetlands if column.lake >= 0.50 => clay,
        TerrainBiome::River if column.river >= 0.65 => gravel,
        TerrainBiome::Mountains if column.roughness >= 0.18 => context.palette.stone,
        _ => context.palette.dirt,
    }
}

fn terrain_block_at_column_height(
    y: usize,
    column: &TerrainColumnSample,
    context: &WorldGenerationContext,
    surface_material: BlockId,
) -> BlockId {
    if y >= column.height {
        return context.palette.air;
    }

    if y == column.height - 1 {
        return terrain_surface_block_for_column(column, context, surface_material);
    }

    let subsurface_depth = match column.biome {
        TerrainBiome::Beach | TerrainBiome::Desert | TerrainBiome::Canyon => 5,
        TerrainBiome::River | TerrainBiome::Wetlands => 4,
        TerrainBiome::Snow | TerrainBiome::Taiga => 3,
        TerrainBiome::Mountains => 2,
        TerrainBiome::Plains | TerrainBiome::Forest | TerrainBiome::AutumnForest => DIRT_DEPTH,
    };

    if y > column.height.saturating_sub(subsurface_depth + 1) {
        terrain_subsurface_block_for_column(column, context, surface_material)
    } else {
        context.palette.stone
    }
}

fn is_cave_air(global_x: i32, world_y: usize, global_z: i32, column: &TerrainColumnSample) -> bool {
    if world_y <= 2 || world_y + CAVE_MIN_DEPTH_BELOW_SURFACE >= column.height {
        return false;
    }

    let noise = terrain_field_noise();
    let x = f64::from(global_x);
    let y = world_y as f64;
    let z = f64::from(global_z);
    let primary = perlin01_3d(&noise.cave_primary, x, y, z, CAVE_SCALE);
    let detail = perlin01_3d(
        &noise.cave_detail,
        x + 91.0,
        y - 37.0,
        z - 55.0,
        CAVE_DETAIL_SCALE,
    );
    let depth = column.height.saturating_sub(world_y);
    let depth_weight = smoothstep(
        CAVE_MIN_DEPTH_BELOW_SURFACE as f64,
        (CAVE_MIN_DEPTH_BELOW_SURFACE + 22) as f64,
        depth as f64,
    );
    let mountain_bonus = f64::from(column.mountain) * 0.05;
    let river_suppression = f64::from(column.river.max(column.lake)) * 0.10;
    let threshold = 0.705 - mountain_bonus + river_suppression;

    (primary * 0.74 + detail * 0.26) > threshold && depth_weight > 0.35
}

fn ore_block_for(
    global_x: i32,
    world_y: usize,
    global_z: i32,
    column: &TerrainColumnSample,
    context: &WorldGenerationContext,
) -> Option<BlockId> {
    let depth = column.height.saturating_sub(world_y);
    if world_y < 4 || depth < ORE_SURFACE_SAFETY {
        return None;
    }

    let noise = terrain_field_noise();
    let ore_noise = perlin01_3d(
        &noise.ore,
        f64::from(global_x) + 211.0,
        world_y as f64 - 67.0,
        f64::from(global_z) - 149.0,
        ORE_SCALE,
    );
    let rarity = 0.772 - (depth.min(96) as f64 / 96.0) * 0.045 - f64::from(column.mountain) * 0.018;
    if ore_noise < rarity {
        return None;
    }

    let roll = worldgen_rand01("ore_kind", global_x, global_z + world_y as i32 * 4099);
    let name = if world_y <= 18 {
        if roll < 0.09 {
            "diamond_ore"
        } else if roll < 0.22 {
            "redstone_ore"
        } else if roll < 0.34 {
            "lapis_ore"
        } else if roll < 0.58 {
            "iron_ore"
        } else {
            "coal_ore"
        }
    } else if world_y <= 46 {
        if roll < 0.12 {
            "gold_ore"
        } else if roll < 0.34 {
            "copper_ore"
        } else if roll < 0.62 {
            "iron_ore"
        } else {
            "coal_ore"
        }
    } else if column.mountain >= 0.62 && roll < 0.16 {
        "emerald_ore"
    } else if column.lake >= 0.62 && roll < 0.10 {
        "crystal_ore"
    } else if roll < 0.22 {
        "copper_ore"
    } else if roll < 0.50 {
        "iron_ore"
    } else {
        "coal_ore"
    };

    let block = context.block_id(name);
    (block != context.palette.air).then_some(block)
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
pub fn is_terrain_shell_block(block: BlockId, palette: TerrainBlockPalette, sand: BlockId) -> bool {
    block == palette.stone || block == palette.dirt || block == palette.grass || block == sand
}

#[must_use]
pub fn is_world_terrain_shell_block(block: BlockId, context: &WorldGenerationContext) -> bool {
    is_terrain_shell_block(block, context.palette, context.block_id("sand"))
        || block == context.block_id("snow")
        || block == context.block_id("gravel")
        || block == context.block_id("clay")
}

#[must_use]
pub fn terrain_column_top_in_chunk(
    chunk: &ChunkData,
    local_x: usize,
    local_z: usize,
    palette: TerrainBlockPalette,
    sand: BlockId,
) -> TerrainSurfaceSample {
    for y in (0..CHUNK_HEIGHT).rev() {
        let block = chunk.get_block(local_x, y, local_z);
        if block == palette.air {
            continue;
        }
        if is_terrain_shell_block(block, palette, sand) {
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

#[must_use]
pub fn terrain_column_top_in_chunk_with_context(
    chunk: &ChunkData,
    local_x: usize,
    local_z: usize,
    context: &WorldGenerationContext,
) -> TerrainSurfaceSample {
    for y in (0..CHUNK_HEIGHT).rev() {
        let block = chunk.get_block(local_x, y, local_z);
        if block == context.palette.air {
            continue;
        }
        if is_world_terrain_shell_block(block, context) {
            return TerrainSurfaceSample {
                height: y + 1,
                top_block: block,
            };
        }
    }
    TerrainSurfaceSample {
        height: 0,
        top_block: context.palette.air,
    }
}

#[must_use]
pub fn terrain_surface_cell_sample_from_chunk_local(
    chunk: &ChunkData,
    local_x: usize,
    local_z: usize,
    width: usize,
    depth: usize,
    palette: TerrainBlockPalette,
    sand: BlockId,
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
            height_sum += terrain_column_top_in_chunk(chunk, x, z, palette, sand).height;
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
        terrain_column_top_in_chunk(chunk, center_x, center_z, palette, sand).top_block;
    if top_block == palette.air && height > 0 {
        top_block = chunk.get_block(center_x, height - 1, center_z);
    }

    TerrainSurfaceSample { height, top_block }
}

#[must_use]
pub fn terrain_surface_cell_sample_from_chunk_local_with_context(
    chunk: &ChunkData,
    local_x: usize,
    local_z: usize,
    width: usize,
    depth: usize,
    context: &WorldGenerationContext,
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
            height_sum += terrain_column_top_in_chunk_with_context(chunk, x, z, context).height;
            sample_count += 1;
        }
    }

    if sample_count == 0 {
        return TerrainSurfaceSample {
            height: 0,
            top_block: context.palette.air,
        };
    }

    let height = (height_sum + sample_count / 2) / sample_count;
    let center_x = (local_x + width / 2).min(CHUNK_SIZE - 1);
    let center_z = (local_z + depth / 2).min(CHUNK_SIZE - 1);
    let mut top_block =
        terrain_column_top_in_chunk_with_context(chunk, center_x, center_z, context).top_block;
    if top_block == context.palette.air && height > 0 {
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
    edit_store: &WorldEditStore,
) -> TerrainSurfaceSample {
    let chunk_x = world_x.div_euclid(CHUNK_SIZE as i32);
    let chunk_z = world_z.div_euclid(CHUNK_SIZE as i32);
    let local_x = usize::try_from(world_x.rem_euclid(CHUNK_SIZE as i32)).unwrap_or(0);
    let local_z = usize::try_from(world_z.rem_euclid(CHUNK_SIZE as i32)).unwrap_or(0);
    let sand = context.block_id("sand");
    let generated = crate::chunk_gen_cache::cached_chunk(
        ChunkPos::new(chunk_x, chunk_z),
        context,
        edit_store,
    );
    let sample = terrain_surface_cell_sample_from_chunk_local_with_context(
        &generated.chunk,
        local_x,
        local_z,
        width,
        depth,
        context,
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
    let perlin = terrain_perlin();
    let sand = block_id_or(context, "sand", context.palette.dirt);
    let water = block_id_or(context, "water", context.palette.air);
    let ice = block_id_or(context, "ice", water);

    for x in 0..CHUNK_SIZE {
        for z in 0..CHUNK_SIZE {
            let global_x = pos.x * CHUNK_SIZE as i32 + x as i32;
            let global_z = pos.z * CHUNK_SIZE as i32 + z as i32;
            let column = terrain_column_with_noise(global_x, global_z, &perlin);
            let solid_top = column.height.min(CHUNK_HEIGHT);

            for y in 0..solid_top {
                if is_cave_air(global_x, y, global_z, &column) {
                    continue;
                }

                let mut block_id = terrain_block_at_column_height(y, &column, context, sand);
                if block_id == context.palette.stone
                    && let Some(ore) = ore_block_for(global_x, y, global_z, &column, context)
                {
                    block_id = ore;
                }
                if block_id != context.palette.air {
                    chunk.set_block(x, y, z, block_id);
                }
            }

            if water != context.palette.air
                && solid_top <= WORLD_WATER_LEVEL
                && WORLD_WATER_LEVEL < CHUNK_HEIGHT
            {
                for y in solid_top..=WORLD_WATER_LEVEL {
                    let fluid = if y == WORLD_WATER_LEVEL
                        && matches!(column.biome, TerrainBiome::Snow | TerrainBiome::Taiga)
                    {
                        ice
                    } else {
                        water
                    };
                    if fluid != context.palette.air {
                        chunk.set_block(x, y, z, fluid);
                    }
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
    edit_store: &WorldEditStore,
) -> usize {
    terrain_surface_cell_sample_from_world_cached(
        world_x,
        world_z,
        width,
        depth,
        context,
        edit_store,
    )
    .height
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
    let _ = chunk_table.set("height", CHUNK_HEIGHT);
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
        table.set("aridity", sample.aridity)?;
        table.set("river", sample.river)?;
        table.set("lake", sample.lake)?;
        table.set("mountain", sample.mountain)?;

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

    let get_height_stats = Rc::clone(&stats);
    let get_height = lua.create_function(move |_, (x, z): (i32, i32)| {
        get_height_stats.borrow_mut().height_requests += 1;
        let global_x = pos.x * CHUNK_SIZE_I32 + x;
        let global_z = pos.z * CHUNK_SIZE_I32 + z;
        Ok(terrain_height_at(global_x, global_z).min(CHUNK_HEIGHT - 1))
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
        && (0..CHUNK_HEIGHT_I32).contains(&y)
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
            (5, "water"),
            (6, "ice"),
            (7, "snow"),
            (8, "gravel"),
            (9, "clay"),
            (10, "coal_ore"),
            (11, "iron_ore"),
            (12, "copper_ore"),
            (13, "gold_ore"),
            (14, "diamond_ore"),
            (15, "emerald_ore"),
            (16, "lapis_ore"),
            (17, "redstone_ore"),
            (18, "crystal_ore"),
            (19, "wood"),
            (20, "leaves"),
            (21, "cobblestone"),
            (22, "stone_bricks"),
            (23, "flower_red"),
            (24, "flower_yellow"),
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
    fn terrain_height_field_has_lowlands_and_vertical_headroom() {
        let mut min_sample = (0, 0, usize::MAX);
        let mut max_sample = (0, 0, 0usize);

        for z in (-1024..=1024).step_by(16) {
            for x in (-1024..=1024).step_by(16) {
                let height = terrain_height_at(x, z);
                if height < min_sample.2 {
                    min_sample = (x, z, height);
                }
                if height > max_sample.2 {
                    max_sample = (x, z, height);
                }
            }
        }

        if std::env::var_os("RUMPEL_PRINT_GOLDEN").is_some() {
            eprintln!("terrain min={min_sample:?} max={max_sample:?}");
        }

        assert!(
            min_sample.2 <= SURFACE_BEACH_HEIGHT_THRESHOLD,
            "terrain should expose lowland/beach samples, min_sample={min_sample:?}"
        );
        assert!(
            max_sample.2 > CHUNK_SIZE * 2,
            "terrain should visibly use the taller chunk headroom, max_sample={max_sample:?}"
        );
        assert!(max_sample.2 < CHUNK_HEIGHT);
    }

    #[test]
    fn terrain_height_field_has_no_single_column_walls() {
        let perlin = terrain_perlin();
        let max_neighbor_delta = (-160..=160)
            .step_by(8)
            .flat_map(|z| {
                (-160..=160).step_by(8).map(move |x| {
                    let center = terrain_height_with_noise(x, z, &perlin);
                    let east = terrain_height_with_noise(x + 1, z, &perlin);
                    let south = terrain_height_with_noise(x, z + 1, &perlin);
                    center.abs_diff(east).max(center.abs_diff(south))
                })
            })
            .max()
            .unwrap_or(0);

        assert!(
            max_neighbor_delta <= 18,
            "single-column height jumps should stay terrain-like, max_delta={max_neighbor_delta}"
        );
    }

    #[test]
    fn terrain_world_sample_contract_is_deterministic() {
        let context = test_world_context();
        let first = terrain_world_sample_at(128, -64, &context, 4);
        let second = terrain_world_sample_at(128, -64, &context, 4);

        assert_eq!(first, second);
        assert_eq!(first.biome, terrain_biome_at(128, -64));
        assert_eq!(first.chunk_height, first.height.min(CHUNK_HEIGHT - 1));
        assert!((0.0..=1.0).contains(&first.temperature));
        assert!((0.0..=1.0).contains(&first.humidity));
        assert!((0.0..=1.0).contains(&first.roughness));
        assert!((0.0..=1.0).contains(&first.aridity));
        assert!((0.0..=1.0).contains(&first.river));
        assert!((0.0..=1.0).contains(&first.lake));
        assert!((0.0..=1.0).contains(&first.mountain));
        assert!(!first.biome.as_str().is_empty());
        assert!(!context.block_name(first.surface_block).is_empty());
    }

    #[test]
    fn terrain_world_sample_uses_surface_material_for_beach() {
        let context = test_world_context();
        let sand = context.block_id("sand");
        let (x, z) = (-304, -80);
        assert!(terrain_height_at(x, z) <= SURFACE_BEACH_HEIGHT_THRESHOLD);

        let sample = terrain_world_sample_at(x, z, &context, sand);

        assert_eq!(sample.biome, TerrainBiome::Beach);
        assert_eq!(sample.surface_block, sand);
    }

    #[test]
    fn terrain_sampler_exposes_rich_biome_set() {
        let biomes = (-2048..=2048)
            .step_by(32)
            .flat_map(|z| {
                (-2048..=2048)
                    .step_by(32)
                    .map(move |x| terrain_biome_at(x, z).as_str())
            })
            .collect::<std::collections::BTreeSet<_>>();

        assert!(
            biomes.len() >= 7,
            "terrain sampler should expose a broad biome set, got {biomes:?}"
        );
        for expected in ["beach", "plains", "forest", "mountains", "snow", "desert"] {
            assert!(
                biomes.contains(expected),
                "terrain sampler should expose {expected}, got {biomes:?}"
            );
        }
    }

    #[test]
    fn terrain_material_sampler_uses_biome_specific_surface_blocks() {
        let context = test_world_context();
        let sand = context.block_id("sand");
        let mut found_snow = false;
        let mut found_gravel_or_clay = false;
        let mut found_stone_mountain = false;

        for z in (-2048..=2048).step_by(16) {
            for x in (-2048..=2048).step_by(16) {
                let sample = terrain_world_sample_at(x, z, &context, sand);
                let name = context.block_name(sample.surface_block);
                found_snow |= matches!(sample.biome, TerrainBiome::Snow | TerrainBiome::Taiga)
                    && name == "snow";
                found_gravel_or_clay |=
                    matches!(sample.biome, TerrainBiome::River | TerrainBiome::Wetlands)
                        && (name == "gravel" || name == "clay" || name == "sand");
                found_stone_mountain |= sample.biome == TerrainBiome::Mountains && name == "stone";
            }
        }

        assert!(found_snow, "snow/taiga biomes should produce snow surfaces");
        assert!(
            found_gravel_or_clay,
            "river/wetland biomes should produce wet shoreline materials"
        );
        assert!(
            found_stone_mountain,
            "mountain biomes should expose stone surfaces on rough peaks"
        );
    }

    #[test]
    fn underground_sampler_exposes_caves_and_ores() {
        let context = test_world_context();
        let mut found_cave = false;
        let mut found_ore = false;

        for z in (-512..=512).step_by(16) {
            for x in (-512..=512).step_by(16) {
                let column = terrain_column_at(x, z);
                for y in (4..column.height.saturating_sub(CAVE_MIN_DEPTH_BELOW_SURFACE)).step_by(4)
                {
                    found_cave |= is_cave_air(x, y, z, &column);
                    found_ore |= ore_block_for(x, y, z, &column, &context)
                        .is_some_and(|ore| context.block_name(ore).ends_with("_ore"));
                    if found_cave && found_ore {
                        return;
                    }
                }
            }
        }

        assert!(
            found_cave,
            "terrain sampler should carve deterministic underground caves"
        );
        assert!(
            found_ore,
            "terrain sampler should place deterministic underground ores"
        );
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
        assert!(local_block_index(0, CHUNK_HEIGHT_I32, 0).is_none());
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

        let beach = terrain_surface_cell_sample_with_noise(-304, -80, 1, 1, palette, sand, &perlin);
        assert!(beach.height <= SURFACE_BEACH_HEIGHT_THRESHOLD);
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
                raw_height: 26,
                shell_height: 26,
                cell_height: 26,
                top_block: palette.grass,
            },
            GoldenSurfaceCase {
                x: -65,
                z: 17,
                width: 1,
                depth: 1,
                raw_height: 57,
                shell_height: 57,
                cell_height: 57,
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
                raw_height: 39,
                shell_height: 39,
                cell_height: 39,
                top_block: palette.grass,
            },
            GoldenSurfaceCase {
                x: 127,
                z: -93,
                width: 2,
                depth: 2,
                raw_height: 22,
                shell_height: 22,
                cell_height: 23,
                top_block: palette.grass,
            },
            GoldenSurfaceCase {
                x: 512,
                z: 384,
                width: 4,
                depth: 3,
                raw_height: 52,
                shell_height: 52,
                cell_height: 52,
                top_block: palette.grass,
            },
            GoldenSurfaceCase {
                x: -304,
                z: -80,
                width: 1,
                depth: 1,
                raw_height: 7,
                shell_height: 7,
                cell_height: 7,
                top_block: sand,
            },
        ];

        let print_golden = std::env::var_os("RUMPEL_PRINT_GOLDEN").is_some();
        for case in cases {
            let raw = terrain_height_with_noise(case.x, case.z, &perlin);
            let shell = terrain_surface_shell_height_with_noise(case.x, case.z, &perlin);
            let sample = terrain_surface_cell_sample_with_noise(
                case.x, case.z, case.width, case.depth, palette, sand, &perlin,
            );
            if print_golden {
                eprintln!(
                    "GoldenSurfaceCase {{ x: {}, z: {}, width: {}, depth: {}, raw_height: {}, shell_height: {}, cell_height: {}, top_block: {}, }},",
                    case.x,
                    case.z,
                    case.width,
                    case.depth,
                    raw,
                    shell,
                    sample.height,
                    sample.top_block
                );
                continue;
            }
            assert_eq!(
                raw, case.raw_height,
                "raw terrain height changed at ({}, {})",
                case.x, case.z
            );
            assert_eq!(
                shell, case.shell_height,
                "surface shell height changed at ({}, {})",
                case.x, case.z
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
