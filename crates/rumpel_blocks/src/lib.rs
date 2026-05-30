use bevy::prelude::*;
use serde::Deserialize;
use std::collections::HashMap;
use std::fs;

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
}

impl Default for BlockRegistry {
    fn default() -> Self {
        let mut registry = Self {
            id_to_data: HashMap::new(),
            string_to_id: HashMap::new(),
            next_id: 1, // 0 is reserved for air if not explicitly defined first
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
        if let Some(existing_id) = self.string_to_id.get(&block.id).copied() {
            self.id_to_data.insert(existing_id, block);
            return existing_id;
        }

        let id = if block.id == "air" {
            AIR_BLOCK_ID
        } else {
            let current = self.next_id;
            self.next_id += 1;
            current
        };

        self.string_to_id.insert(block.id.clone(), id);
        self.id_to_data.insert(id, block);
        id
    }
}
