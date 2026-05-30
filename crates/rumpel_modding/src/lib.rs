use bevy::prelude::*;
use mlua::{Lua, Table};
use rumpel_blocks::{BlockData, BlockRegistry};
use std::cell::RefCell;
use std::fs;
use std::path::Path;
use std::rc::Rc;

const MODS_DIR: &str = "assets/mods";

#[derive(Debug)]
pub struct LuaBlockDefinition {
    pub id: String,
    pub name: String,
    pub is_solid: bool,
    pub is_transparent: bool,
    pub color: (f32, f32, f32, f32),
}

impl From<LuaBlockDefinition> for BlockData {
    fn from(definition: LuaBlockDefinition) -> Self {
        Self {
            id: definition.id,
            name: definition.name,
            is_solid: definition.is_solid,
            is_transparent: definition.is_transparent,
            color: definition.color,
        }
    }
}

#[derive(Resource, Default, Debug)]
pub struct ModRegistry {
    blocks: Vec<LuaBlockDefinition>,
}

impl ModRegistry {
    pub fn drain_blocks(&mut self) -> impl Iterator<Item = LuaBlockDefinition> + '_ {
        self.blocks.drain(..)
    }

    fn register_block(&mut self, block: LuaBlockDefinition) {
        self.blocks.push(block);
    }
}

pub fn load_lua_mods(mut block_registry: ResMut<BlockRegistry>) {
    let mut mod_registry = ModRegistry::default();

    match load_lua_mod_directory(MODS_DIR, &mut mod_registry) {
        Ok(loaded_count) => {
            for block in mod_registry.drain_blocks() {
                let id = block_registry.register_block(block.into());
                info!("Registered mod block with numeric id {id}");
            }

            info!("Loaded {loaded_count} Lua mod scripts from {MODS_DIR}");
        }
        Err(error) => {
            error!("Failed to load Lua mods from {MODS_DIR}: {error}");
        }
    }
}

pub fn load_lua_mod_directory(
    mods_dir: impl AsRef<Path>,
    registry: &mut ModRegistry,
) -> Result<usize, String> {
    let mods_dir = mods_dir.as_ref();
    if !mods_dir.exists() {
        return Ok(0);
    }

    let mut loaded_count = 0;
    let entries = fs::read_dir(mods_dir)
        .map_err(|error| format!("could not read {}: {error}", mods_dir.display()))?;

    for entry in entries {
        let entry = entry.map_err(|error| format!("could not read mod entry: {error}"))?;
        let path = entry.path();
        if path.extension().and_then(|extension| extension.to_str()) != Some("lua") {
            continue;
        }

        let script = fs::read_to_string(&path)
            .map_err(|error| format!("could not read {}: {error}", path.display()))?;
        run_lua_mod(&script, registry).map_err(|error| format!("{}: {error}", path.display()))?;
        loaded_count += 1;
    }

    Ok(loaded_count)
}

pub fn run_lua_mod(script: &str, registry: &mut ModRegistry) -> mlua::Result<()> {
    let lua = Lua::new();
    let globals = lua.globals();

    let registered_blocks = Rc::new(RefCell::new(Vec::new()));
    let register_block_blocks = Rc::clone(&registered_blocks);
    let register_block = lua.create_function(move |_, table: Table| {
        let color_table: Table = table.get("color")?;
        let block = LuaBlockDefinition {
            id: table.get("id")?,
            name: table.get("name")?,
            is_solid: table.get("is_solid")?,
            is_transparent: table.get("is_transparent")?,
            color: (
                color_table.get(1)?,
                color_table.get(2)?,
                color_table.get(3)?,
                color_table.get(4)?,
            ),
        };

        register_block_blocks.borrow_mut().push(block);
        Ok(())
    })?;

    globals.set("register_block", register_block)?;
    lua.load(script).exec()?;

    for block in registered_blocks.borrow_mut().drain(..) {
        registry.register_block(block);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lua_mod_can_register_block_definition() {
        let mut registry = ModRegistry::default();

        run_lua_mod(
            r#"
            register_block({
                id = "ruby_ore",
                name = "Ruby Ore",
                is_solid = true,
                is_transparent = false,
                color = { 0.9, 0.05, 0.12, 1.0 },
            })
            "#,
            &mut registry,
        )
        .expect("Lua mod should register a valid block");

        let blocks: Vec<_> = registry.drain_blocks().collect();
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].id, "ruby_ore");
        assert_eq!(blocks[0].color, (0.9, 0.05, 0.12, 1.0));
    }
}
