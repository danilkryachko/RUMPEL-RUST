use bevy::prelude::*;
use serde::Deserialize;
use std::collections::HashMap;
use std::fs;
use std::sync::{Arc, RwLock};

pub type BlockId = u16;

pub const AIR_BLOCK_ID: BlockId = 0;

#[derive(Deserialize, Debug, Clone)]
pub struct BlockData {
    pub id: String,
    pub name: String,
    pub is_solid: bool,
    pub is_transparent: bool,
    pub color: (f32, f32, f32, f32),
    #[serde(default)]
    pub gravity_affected: bool,
    #[serde(default = "default_strength")]
    pub strength: f32,
}

fn default_strength() -> f32 {
    1.0
}

#[derive(Deserialize, Debug)]
pub struct BlocksConfig {
    pub blocks: Vec<BlockData>,
}

#[derive(Resource)]
pub struct BlockRegistry {
    id_to_data: HashMap<BlockId, BlockData>,
    string_to_id: HashMap<String, BlockId>,
    next_id: BlockId,
    // Thread-safe map shared with the bevy_voxel_world meshing threads
    pub texture_mappings: Arc<RwLock<HashMap<BlockId, [u32; 3]>>>,
}

impl Default for BlockRegistry {
    fn default() -> Self {
        let mut registry = Self::empty();

        registry.load_from_file("assets/blocks/base.ron");
        registry
    }
}

impl BlockRegistry {
    #[must_use]
    pub fn empty() -> Self {
        Self {
            id_to_data: HashMap::new(),
            string_to_id: HashMap::new(),
            next_id: 1, // 0 is reserved for air if not explicitly defined first
            texture_mappings: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn load_from_file(&mut self, path: &str) {
        if let Ok(content) = fs::read_to_string(path) {
            if let Ok(config) = ron::from_str::<BlocksConfig>(&content) {
                for block in config.blocks {
                    self.register_block(block);
                }
                println!("Loaded {} blocks from config.", self.id_to_data.len());
            } else {
                eprintln!("Failed to parse blocks config: {}", path);
            }
        } else {
            eprintln!("Failed to read blocks config: {}", path);
        }
    }

    pub fn get_block(&self, id: BlockId) -> Option<&BlockData> {
        self.id_to_data.get(&id)
    }

    pub fn get_id(&self, string_id: &str) -> Option<BlockId> {
        self.string_to_id.get(string_id).copied()
    }

    pub fn register_block(&mut self, block: BlockData) -> BlockId {
        let id = if let Some(existing_id) = self.string_to_id.get(&block.id).copied() {
            self.id_to_data.insert(existing_id, block.clone());
            existing_id
        } else {
            let id = if block.id == "air" {
                AIR_BLOCK_ID
            } else {
                let current = self.next_id;
                self.next_id += 1;
                current
            };
            self.string_to_id.insert(block.id.clone(), id);
            self.id_to_data.insert(id, block.clone());
            id
        };

        // Determine texture indices in the 28-layer atlas based on block string ID
        let tex_indices = match block.id.as_str() {
            "grass" => [0, 1, 2],           // grass_top (0), grass_side (1), dirt (2)
            "dirt" => [2, 2, 2],            // dirt (2)
            "stone" => [3, 3, 3],           // stone (3)
            "sand" => [4, 4, 4],            // sand (4)
            "wood" => [6, 5, 6],            // wood_top (6), wood_side (5), wood_top (6)
            "leaves" => [7, 7, 7],          // leaves (7)
            "coal_ore" => [8, 8, 8],        // coal_ore (8)
            "iron_ore" => [9, 9, 9],        // iron_ore (9)
            "copper_ore" => [10, 10, 10],   // copper_ore (10)
            "gold_ore" => [11, 11, 11],     // gold_ore (11)
            "diamond_ore" => [12, 12, 12],  // diamond_ore (12)
            "emerald_ore" => [13, 13, 13],  // emerald_ore (13)
            "lapis_ore" => [14, 14, 14],    // lapis_ore (14)
            "redstone_ore" => [15, 15, 15], // redstone_ore (15)
            "cobblestone" => [16, 16, 16],  // cobblestone (16)
            "stone_bricks" => [17, 17, 17], // stone_bricks (17)
            "bricks" => [18, 18, 18],       // bricks (18)
            "oak_planks" => [19, 19, 19],   // oak_planks (19)
            "bookshelf" => [20, 20, 20],    // bookshelf (20)
            "glass" => [21, 21, 21],        // glass (21)
            "obsidian" => [22, 22, 22],     // obsidian (22)
            "glowstone" => [23, 23, 23],    // glowstone (23)
            "snow" => [24, 24, 24],         // snow (24)
            "ice" => [25, 25, 25],          // ice (25)
            "gravel" => [26, 26, 26],       // gravel (26)
            "clay" => [27, 27, 27],         // clay (27)
            _ => [3, 3, 3],                 // fallback to stone (3)
        };

        if let Ok(mut mappings) = self.texture_mappings.write() {
            mappings.insert(id, tex_indices);
        }

        id
    }

    pub fn set_texture_mapping(&self, block_id: BlockId, mapping: [u32; 3]) {
        if let Ok(mut mappings) = self.texture_mappings.write() {
            mappings.insert(block_id, mapping);
        }
    }

    #[must_use]
    pub fn material_contract_version(&self) -> u64 {
        let mut block_ids = self.id_to_data.keys().copied().collect::<Vec<_>>();
        block_ids.sort_unstable();
        self.material_contract_version_for_blocks(&block_ids)
    }

    #[must_use]
    pub fn material_contract_version_for_blocks(&self, block_ids: &[BlockId]) -> u64 {
        let mut ids = block_ids.to_vec();
        ids.sort_unstable();
        ids.dedup();

        let texture_mappings = self.texture_mappings.read().ok();
        let mut hash = FNV64_OFFSET;
        for block_id in ids {
            hash = fnv64(hash, u64::from(block_id));
            if let Some(block) = self.id_to_data.get(&block_id) {
                hash = hash_block_data(hash, block);
            }
            if let Some(mapping) = texture_mappings
                .as_ref()
                .and_then(|mappings| mappings.get(&block_id))
            {
                for tile in mapping {
                    hash = fnv64(hash, u64::from(*tile));
                }
            }
        }
        hash.max(1)
    }
}

const FNV64_OFFSET: u64 = 14_695_981_039_346_656_037;
const FNV64_PRIME: u64 = 1_099_511_628_211;

fn fnv64(hash: u64, value: u64) -> u64 {
    (hash ^ value).wrapping_mul(FNV64_PRIME)
}

fn hash_str(mut hash: u64, value: &str) -> u64 {
    for byte in value.as_bytes() {
        hash = fnv64(hash, u64::from(*byte));
    }
    hash
}

fn hash_bool(hash: u64, value: bool) -> u64 {
    fnv64(hash, u64::from(value))
}

fn hash_block_data(mut hash: u64, block: &BlockData) -> u64 {
    hash = hash_str(hash, &block.id);
    hash = hash_str(hash, &block.name);
    hash = hash_bool(hash, block.is_solid);
    hash = hash_bool(hash, block.is_transparent);
    for channel in [
        block.color.0,
        block.color.1,
        block.color.2,
        block.color.3,
        block.strength,
    ] {
        hash = fnv64(hash, u64::from(channel.to_bits()));
    }
    hash_bool(hash, block.gravity_affected)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_block(id: &str, texture_name: &str) -> BlockData {
        BlockData {
            id: id.to_string(),
            name: texture_name.to_string(),
            is_solid: true,
            is_transparent: false,
            color: (1.0, 1.0, 1.0, 1.0),
            gravity_affected: false,
            strength: 1.0,
        }
    }

    #[test]
    fn material_contract_version_tracks_relevant_block_materials() {
        let mut registry = BlockRegistry::empty();
        let grass = registry.register_block(test_block("grass", "Grass"));
        let base = registry.material_contract_version_for_blocks(&[grass]);

        let stone = registry.register_block(test_block("stone", "Stone"));
        assert_eq!(
            base,
            registry.material_contract_version_for_blocks(&[grass]),
            "unrelated block registrations should not invalidate a relevant-block contract"
        );
        assert_ne!(
            base,
            registry.material_contract_version_for_blocks(&[stone])
        );

        let mut changed = test_block("grass", "Grass V2");
        changed.color = (0.25, 0.5, 0.75, 1.0);
        registry.register_block(changed);
        assert_ne!(
            base,
            registry.material_contract_version_for_blocks(&[grass])
        );
    }

    #[test]
    fn material_contract_version_tracks_direct_texture_mapping_changes() {
        let mut registry = BlockRegistry::empty();
        let grass = registry.register_block(test_block("grass", "Grass"));
        let base = registry.material_contract_version_for_blocks(&[grass]);

        registry
            .texture_mappings
            .write()
            .expect("texture mappings lock")
            .insert(grass, [11, 12, 13]);

        assert_ne!(
            base,
            registry.material_contract_version_for_blocks(&[grass])
        );
    }
}
