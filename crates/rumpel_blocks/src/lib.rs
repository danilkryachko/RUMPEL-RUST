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
        let mut registry = Self {
            id_to_data: HashMap::new(),
            string_to_id: HashMap::new(),
            next_id: 1, // 0 is reserved for air if not explicitly defined first
            texture_mappings: Arc::new(RwLock::new(HashMap::new())),
        };

        registry.load_from_file("assets/blocks/base.ron");
        registry
    }
}

impl BlockRegistry {
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
            "grass" => [0, 1, 2],         // grass_top (0), grass_side (1), dirt (2)
            "dirt" => [2, 2, 2],          // dirt (2)
            "stone" => [3, 3, 3],         // stone (3)
            "sand" => [4, 4, 4],          // sand (4)
            "wood" => [6, 5, 6],          // wood_top (6), wood_side (5), wood_top (6)
            "leaves" => [7, 7, 7],        // leaves (7)
            "coal_ore" => [8, 8, 8],      // coal_ore (8)
            "iron_ore" => [9, 9, 9],      // iron_ore (9)
            "copper_ore" => [10, 10, 10], // copper_ore (10)
            "gold_ore" => [11, 11, 11],   // gold_ore (11)
            "diamond_ore" => [12, 12, 12], // diamond_ore (12)
            "emerald_ore" => [13, 13, 13], // emerald_ore (13)
            "lapis_ore" => [14, 14, 14],  // lapis_ore (14)
            "redstone_ore" => [15, 15, 15], // redstone_ore (15)
            "cobblestone" => [16, 16, 16], // cobblestone (16)
            "stone_bricks" => [17, 17, 17], // stone_bricks (17)
            "bricks" => [18, 18, 18],     // bricks (18)
            "oak_planks" => [19, 19, 19], // oak_planks (19)
            "bookshelf" => [20, 20, 20],  // bookshelf (20)
            "glass" => [21, 21, 21],      // glass (21)
            "obsidian" => [22, 22, 22],   // obsidian (22)
            "glowstone" => [23, 23, 23],  // glowstone (23)
            "snow" => [24, 24, 24],       // snow (24)
            "ice" => [25, 25, 25],        // ice (25)
            "gravel" => [26, 26, 26],     // gravel (26)
            "clay" => [27, 27, 27],       // clay (27)
            _ => [3, 3, 3],               // fallback to stone (3)
        };

        if let Ok(mut mappings) = self.texture_mappings.write() {
            mappings.insert(id, tex_indices);
        }

        id
    }
}

