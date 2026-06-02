use crate::chunk::{CHUNK_SIZE, ChunkData, WorldEditStore};
use bevy::{platform::collections::HashMap, prelude::error};
use noise::{NoiseFn, Perlin};
use rumpel_blocks::AIR_BLOCK_ID;
use rumpel_blocks::{BlockId, BlockRegistry};
use rumpel_coords::{ChunkPos, LocalBlockPos};
use std::{cell::RefCell, fs, rc::Rc};

const TERRAIN_SEED: u32 = 1337;
const TERRAIN_NOISE_SCALE: f64 = 0.02;
const TERRAIN_BASE_HEIGHT: f64 = 10.0;
const TERRAIN_HEIGHT_RANGE: f64 = 40.0;
const DIRT_DEPTH: usize = 3;
pub const SURFACE_BEACH_HEIGHT_THRESHOLD: usize = 14;
const SURFACE_SHELL_HEIGHT_KERNEL: [usize; 5] = [1, 4, 6, 4, 1];
const SURFACE_SHELL_HEIGHT_RADIUS: i32 = 2;
const SURFACE_EDIT_SCAN_HEADROOM: usize = 24;
const SURFACE_EDIT_SCAN_MAX_Y: usize = 96;
const WORLD_GEN_SCRIPT_PATH: &str = "assets/mods/world_gen.lua";
const LUA_WORLD_GEN_CHUNK: ChunkPos = ChunkPos { x: 0, z: 0 };

#[must_use]
pub fn terrain_generation_contract_version() -> u64 {
    let mut hash = FNV64_OFFSET;
    hash = fnv64(hash, u64::from(TERRAIN_SEED));
    hash = fnv64(hash, TERRAIN_NOISE_SCALE.to_bits());
    hash = fnv64(hash, TERRAIN_BASE_HEIGHT.to_bits());
    hash = fnv64(hash, TERRAIN_HEIGHT_RANGE.to_bits());
    hash = fnv64(hash, DIRT_DEPTH as u64);
    hash = fnv64(hash, CHUNK_SIZE as u64);
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

#[must_use]
pub fn terrain_height_at(global_x: i32, global_z: i32) -> usize {
    let perlin = Perlin::new(TERRAIN_SEED);
    terrain_height_with_noise(global_x, global_z, &perlin)
}

#[must_use]
pub fn terrain_height_with_noise(global_x: i32, global_z: i32, perlin: &Perlin) -> usize {
    let noise_val = perlin.get([
        f64::from(global_x) * TERRAIN_NOISE_SCALE,
        f64::from(global_z) * TERRAIN_NOISE_SCALE,
    ]);

    ((noise_val + 1.0) * 0.5 * TERRAIN_HEIGHT_RANGE + TERRAIN_BASE_HEIGHT) as usize
}

#[must_use]
pub fn terrain_perlin() -> Perlin {
    Perlin::new(TERRAIN_SEED)
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

pub fn generate_chunk(pos: ChunkPos, registry: &BlockRegistry) -> ChunkData {
    let context = WorldGenerationContext::from_registry(registry);
    generate_chunk_with_context(pos, &context)
}

#[must_use]
pub fn generate_chunk_with_context(pos: ChunkPos, context: &WorldGenerationContext) -> ChunkData {
    let mut chunk = ChunkData::default();
    let perlin = Perlin::new(TERRAIN_SEED);

    for x in 0..CHUNK_SIZE {
        for z in 0..CHUNK_SIZE {
            let global_x = pos.x * CHUNK_SIZE as i32 + x as i32;
            let global_z = pos.z * CHUNK_SIZE as i32 + z as i32;
            let height = terrain_height_with_noise(global_x, global_z, &perlin);

            for y in 0..CHUNK_SIZE {
                let block_id = terrain_block_at_height(y, height, context.palette);
                if block_id != context.palette.air {
                    chunk.set_block(x, y, z, block_id);
                }
            }
        }
    }

    apply_lua_world_gen(pos, &mut chunk, context);

    chunk
}

fn apply_lua_world_gen(pos: ChunkPos, chunk: &mut ChunkData, context: &WorldGenerationContext) {
    if pos != LUA_WORLD_GEN_CHUNK {
        return;
    }

    let Ok(script) = fs::read_to_string(WORLD_GEN_SCRIPT_PATH) else {
        return;
    };

    let lua = mlua::Lua::new();
    let globals = lua.globals();

    let Ok(chunk_table) = lua.create_table() else {
        return;
    };
    let _ = chunk_table.set("x", pos.x);
    let _ = chunk_table.set("z", pos.z);
    let _ = globals.set("Chunk", chunk_table);

    let blocks_cell = Rc::new(RefCell::new(chunk.blocks.clone()));
    let id_to_name = context.id_to_name.clone();
    let name_to_id = context.name_to_id.clone();

    let get_block_buffer = Rc::clone(&blocks_cell);
    let get_block = lua.create_function(move |_, (x, y, z): (usize, usize, usize)| {
        if x < CHUNK_SIZE && y < CHUNK_SIZE && z < CHUNK_SIZE {
            let id = get_block_buffer.borrow()[ChunkData::get_index(x, y, z)];
            Ok(id_to_name
                .get(&id)
                .cloned()
                .unwrap_or_else(|| "air".to_string()))
        } else {
            Ok("air".to_string())
        }
    });
    if let Ok(function) = get_block {
        let _ = globals.set("get_block", function);
    }

    let set_block_buffer = Rc::clone(&blocks_cell);
    let set_block =
        lua.create_function(move |_, (x, y, z, name): (usize, usize, usize, String)| {
            if x < CHUNK_SIZE && y < CHUNK_SIZE && z < CHUNK_SIZE {
                let id = name_to_id.get(&name).copied().unwrap_or(AIR_BLOCK_ID);
                set_block_buffer.borrow_mut()[ChunkData::get_index(x, y, z)] = id;
            }
            Ok(())
        });
    if let Ok(function) = set_block {
        let _ = globals.set("set_block", function);
    }

    let get_height = lua.create_function(move |_, (x, z): (i32, i32)| {
        let global_x = pos.x * CHUNK_SIZE as i32 + x;
        let global_z = pos.z * CHUNK_SIZE as i32 + z;
        Ok(terrain_height_at(global_x, global_z).min(CHUNK_SIZE - 1))
    });
    if let Ok(function) = get_height {
        let _ = globals.set("get_height", function);
    }

    let spawn_mob =
        lua.create_function(|_, (_mob_type, _x, _y, _z): (String, f32, f32, f32)| Ok(()));
    if let Ok(function) = spawn_mob {
        let _ = globals.set("spawn_mob", function);
    }

    let seed = 1337_i64 + i64::from(pos.x) * 73_856_093 + i64::from(pos.z) * 19_349_663;
    let _ = lua.load(format!("math.randomseed({seed})")).exec();

    if let Err(error) = lua.load(&script).set_name(WORLD_GEN_SCRIPT_PATH).exec() {
        error!("WORLD_GEN: Lua post-pass failed for chunk {pos:?}: {error:?}");
    }

    chunk.blocks = blocks_cell.borrow().clone();
}

const FNV64_OFFSET: u64 = 14_695_981_039_346_656_037;
const FNV64_PRIME: u64 = 1_099_511_628_211;

fn fnv64(hash: u64, value: u64) -> u64 {
    (hash ^ value).wrapping_mul(FNV64_PRIME)
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
