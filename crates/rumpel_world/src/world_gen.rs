use crate::chunk::{CHUNK_SIZE, ChunkData};
use bevy::{platform::collections::HashMap, prelude::error};
use noise::{NoiseFn, Perlin};
use rumpel_blocks::AIR_BLOCK_ID;
use rumpel_blocks::{BlockId, BlockRegistry};
use rumpel_coords::ChunkPos;
use std::{cell::RefCell, fs, rc::Rc};

const TERRAIN_SEED: u32 = 1337;
const TERRAIN_NOISE_SCALE: f64 = 0.02;
const TERRAIN_BASE_HEIGHT: f64 = 10.0;
const TERRAIN_HEIGHT_RANGE: f64 = 40.0;
const DIRT_DEPTH: usize = 3;
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
}
