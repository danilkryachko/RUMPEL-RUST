use bevy::prelude::*;
use mlua::{Lua, Table};
use rumpel_blocks::{BlockData, BlockRegistry};
use rumpel_world::{chunk::CHUNK_SIZE, world_gen::terrain_height_at};
use std::cell::RefCell;
use std::fs;
use std::path::Path;
use std::rc::Rc;

const MODS_DIR: &str = "assets/mods";
const NON_STARTUP_LUA_FILES: &[&str] = &[
    "api_stub.lua",
    "world_gen.lua",
    "biomes.lua",
    "terrain.lua",
    "decor.lua",
];

#[derive(Debug, Clone)]
pub struct LuaBlockDefinition {
    pub id: String,
    pub name: String,
    pub is_solid: bool,
    pub is_transparent: bool,
    pub color: (f32, f32, f32, f32),
    pub gravity_affected: Option<bool>,
    pub strength: Option<f32>,
    /// Atlas tile indices [top, side, bottom]. When present, overrides the
    /// hardcoded match in `BlockRegistry::register_block`.
    pub textures: Option<[u32; 3]>,
}

impl From<LuaBlockDefinition> for BlockData {
    fn from(definition: LuaBlockDefinition) -> Self {
        Self {
            id: definition.id,
            name: definition.name,
            is_solid: definition.is_solid,
            is_transparent: definition.is_transparent,
            color: definition.color,
            gravity_affected: definition.gravity_affected.unwrap_or(false),
            strength: definition.strength.unwrap_or(1.0),
        }
    }
}

#[derive(Debug, Clone)]
pub struct LuaStructureBlock {
    pub dx: i32,
    pub dy: i32,
    pub dz: i32,
    pub block: String,
}

#[derive(Debug, Clone)]
pub struct LuaStructure {
    pub name: String,
    pub blocks: Vec<LuaStructureBlock>,
    pub chance: f32,
}

#[derive(Resource, Default, Debug, Clone)]
pub struct LuaModStructures {
    pub structures: Vec<LuaStructure>,
}

#[derive(Resource)]
pub struct LuaRuntime(pub std::sync::Mutex<mlua::Lua>);

unsafe impl Send for LuaRuntime {}
unsafe impl Sync for LuaRuntime {}

#[derive(Resource, Default, Debug)]
pub struct ModRegistry {
    blocks: Vec<LuaBlockDefinition>,
    structures: Vec<LuaStructure>,
}

impl ModRegistry {
    pub fn drain_blocks(&mut self) -> impl Iterator<Item = LuaBlockDefinition> + '_ {
        self.blocks.drain(..)
    }

    pub fn drain_structures(&mut self) -> impl Iterator<Item = LuaStructure> + '_ {
        self.structures.drain(..)
    }

    fn register_block(&mut self, block: LuaBlockDefinition) {
        self.blocks.push(block);
    }

    fn register_structure(&mut self, structure: LuaStructure) {
        self.structures.push(structure);
    }
}

pub fn load_lua_mods(mut commands: Commands, mut block_registry: ResMut<BlockRegistry>) {
    let lua = Lua::new();
    let globals = lua.globals();

    // 1. Initialize global Behaviors table and event helpers in Lua
    let prelude = r#"
        Behaviors = {}
        function register_behavior(block_id, callbacks)
            Behaviors[block_id] = callbacks
        end
        function trigger_behavior(block_id, event_name, ...)
            local callbacks = Behaviors[block_id]
            if not callbacks then
                return
            end
            local handler = callbacks[event_name]
            if not handler and event_name == "on_broken" then
                handler = callbacks.on_block_break
            elseif not handler and event_name == "on_block_break" then
                handler = callbacks.on_broken
            elseif not handler and event_name == "on_placed" then
                handler = callbacks.on_block_place
            elseif not handler and event_name == "on_block_place" then
                handler = callbacks.on_placed
            end
            if handler then
                handler(...)
            end
        end

        Mobs = {}
        MobSpawnQueue = {}
        MobDespawnQueue = {}
        MobStates = {}
        PlayerState = {}
        TimeState = {
            elapsed_time = 1.5707963267948966,
            sun_angle = 1.0,
            is_raining = false,
            lightning_flash = 0.0,
            time_scale = 1.0
        }
        ParticleSpawnQueue = {}
        BlockEditQueue = {}

        function register_mob(mob_type, callbacks)
            Mobs[mob_type] = callbacks
        end

        function spawn_mob(mob_type, x, y, z)
            table.insert(MobSpawnQueue, { mob_type = mob_type, x = x, y = y, z = z })
        end

        function despawn_mob(mob_id)
            table.insert(MobDespawnQueue, mob_id)
        end

        function trigger_mob_spawn(mob_id, mob_type)
            local callbacks = Mobs[mob_type]
            if callbacks and callbacks.on_spawn then
                callbacks.on_spawn(mob_id)
            end
        end

        function trigger_mob_update(mob_id, mob_type)
            local callbacks = Mobs[mob_type]
            if callbacks and callbacks.on_update then
                callbacks.on_update(mob_id)
            end
        end

        function spawn_particle(x, y, z, vx, vy, vz, r, g, b, a, lifetime, size)
            table.insert(ParticleSpawnQueue, {
                x = x, y = y, z = z,
                vx = vx, vy = vy, vz = vz,
                r = r, g = g, b = b, a = a,
                lifetime = lifetime, size = size
            })
        end

        function get_block(x, y, z)
            return "air"
        end

        function set_block(x, y, z, block_id)
            table.insert(BlockEditQueue, { x = x, y = y, z = z, block = block_id })
        end

        ChatCommands = {}
        ChatMessageQueue = {}

        function register_chat_command(command, callback)
            ChatCommands[command] = callback
        end

        function add_chat_message(sender, text, color)
            table.insert(ChatMessageQueue, { sender = sender, text = text, color = color })
        end

        function trigger_chat_command(command, args_str)
            local callback = ChatCommands[command]
            if callback then
                local success, err = pcall(callback, args_str)
                if not success then
                    add_chat_message("System", "Error in /" .. command .. ": " .. tostring(err), { 0.95, 0.25, 0.25, 1.0 })
                end
            else
                add_chat_message("System", "Unknown command: /" .. command .. ". Type /help for assistance.", { 0.95, 0.25, 0.25, 1.0 })
            end
        end

        function trigger_chat_message(sender, text)
            add_chat_message(sender, text, { 0.9, 0.95, 0.9, 1.0 })
        end
    "#;
    if let Err(e) = lua.load(prelude).exec() {
        error!("MODS: Failed to load Behaviors prelude: {:?}", e);
    }

    // 2. Setup block registration API in Lua
    let registered_blocks = Rc::new(RefCell::new(Vec::new()));
    let register_block_blocks = Rc::clone(&registered_blocks);
    let register_block = lua.create_function(move |_, table: Table| {
        let color_table: Table = table.get("color")?;
        let textures: Option<[u32; 3]> =
            table.get::<Table>("textures").ok().and_then(|t| {
                let top = t.get::<u32>("top").or_else(|_| t.get::<u32>(1)).ok()?;
                let side = t.get::<u32>("side").or_else(|_| t.get::<u32>(2)).ok()?;
                let bottom = t.get::<u32>("bottom").or_else(|_| t.get::<u32>(3)).ok()?;
                Some([top, side, bottom])
            });
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
            gravity_affected: table.get("gravity_affected").ok(),
            strength: table.get("strength").ok(),
            textures,
        };

        register_block_blocks.borrow_mut().push(block);
        Ok(())
    });

    if let Ok(f) = register_block {
        let _ = globals.set("register_block", f);
    }

    // 3. Setup structure registration API in Lua
    let registered_structures = Rc::new(RefCell::new(Vec::new()));
    let register_struct_list = Rc::clone(&registered_structures);
    let register_structure = lua.create_function(move |_, table: Table| {
        let blocks_table: Table = table.get("blocks")?;
        let mut blocks = Vec::new();

        let len = blocks_table.len()?;
        for i in 1..=len {
            let entry: Table = blocks_table.get(i)?;
            blocks.push(LuaStructureBlock {
                dx: entry.get("dx")?,
                dy: entry.get("dy")?,
                dz: entry.get("dz")?,
                block: entry.get("block")?,
            });
        }

        let structure = LuaStructure {
            name: table.get("name")?,
            blocks,
            chance: table.get("chance").unwrap_or(0.05),
        };

        register_struct_list.borrow_mut().push(structure);
        Ok(())
    });

    if let Ok(f) = register_structure {
        let _ = globals.set("register_structure", f);
    }

    let get_height = lua.create_function(|_, (x, z): (f64, f64)| {
        let height = terrain_height_at(x.floor() as i32, z.floor() as i32).min(CHUNK_SIZE - 1);
        Ok(height)
    });
    if let Ok(f) = get_height {
        let _ = globals.set("get_height", f);
    }

    // 4. Load all files in the mods directory onto the persistent Lua VM
    let mods_dir = MODS_DIR;
    let mut loaded_count = 0;
    if Path::new(mods_dir).exists()
        && let Ok(entries) = fs::read_dir(mods_dir)
    {
        let mut paths: Vec<_> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| is_startup_lua_mod(p))
            .collect();
        paths.sort_unstable_by(|a, b| {
            a.file_name().cmp(&b.file_name())
        });
        for path in paths {
            if let Ok(script) = fs::read_to_string(&path) {
                if let Err(e) = lua.load(&script).exec() {
                    error!("MODS: Error running mod script {}: {:?}", path.display(), e);
                } else {
                    info!("MODS: Loaded Lua mod script: {}", path.display());
                    loaded_count += 1;
                }
            }
        }
    }

    // 5. Register mod-defined blocks into Rumpel's core BlockRegistry
    for block in registered_blocks.borrow_mut().drain(..) {
        let textures = block.textures;
        let id = block_registry.register_block(block.into());
        if let Some(mapping) = textures {
            block_registry.set_texture_mapping(id, mapping);
        }
        info!("MODS: Registered mod block with numeric id {id}");
    }

    // 6. Register mod-defined structures
    let structures: Vec<LuaStructure> = registered_structures.borrow_mut().drain(..).collect();
    info!(
        "MODS: Loaded {} custom structures from Lua mods",
        structures.len()
    );
    commands.insert_resource(LuaModStructures { structures });

    // 7. Store the persistent Lua VM inside a thread-safe Bevy resource
    commands.insert_resource(LuaRuntime(std::sync::Mutex::new(lua)));
    info!(
        "MODS: Successfully initialized persistent Lua modding runtime with {loaded_count} scripts."
    );
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

    let mut paths: Vec<_> = entries
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| is_startup_lua_mod(p))
        .collect();
    paths.sort_unstable_by(|a, b| a.file_name().cmp(&b.file_name()));

    for path in paths {
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

    // Block registration API
    let registered_blocks = Rc::new(RefCell::new(Vec::new()));
    let register_block_blocks = Rc::clone(&registered_blocks);
    let register_block = lua.create_function(move |_, table: Table| {
        let color_table: Table = table.get("color")?;
        let textures: Option<[u32; 3]> =
            table.get::<Table>("textures").ok().and_then(|t| {
                let top = t.get::<u32>("top").or_else(|_| t.get::<u32>(1)).ok()?;
                let side = t.get::<u32>("side").or_else(|_| t.get::<u32>(2)).ok()?;
                let bottom = t.get::<u32>("bottom").or_else(|_| t.get::<u32>(3)).ok()?;
                Some([top, side, bottom])
            });
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
            gravity_affected: table.get("gravity_affected").ok(),
            strength: table.get("strength").ok(),
            textures,
        };

        register_block_blocks.borrow_mut().push(block);
        Ok(())
    })?;

    globals.set("register_block", register_block)?;

    // Structure registration API
    let registered_structures = Rc::new(RefCell::new(Vec::new()));
    let register_struct_list = Rc::clone(&registered_structures);
    let register_structure = lua.create_function(move |_, table: Table| {
        let blocks_table: Table = table.get("blocks")?;
        let mut blocks = Vec::new();

        let len = blocks_table.len()?;
        for i in 1..=len {
            let entry: Table = blocks_table.get(i)?;
            blocks.push(LuaStructureBlock {
                dx: entry.get("dx")?,
                dy: entry.get("dy")?,
                dz: entry.get("dz")?,
                block: entry.get("block")?,
            });
        }

        let structure = LuaStructure {
            name: table.get("name")?,
            blocks,
            chance: table.get("chance").unwrap_or(0.05),
        };

        register_struct_list.borrow_mut().push(structure);
        Ok(())
    })?;

    globals.set("register_structure", register_structure)?;

    // Load and execute script
    lua.load(script).exec()?;

    // Drain into registry
    for block in registered_blocks.borrow_mut().drain(..) {
        registry.register_block(block);
    }

    for structure in registered_structures.borrow_mut().drain(..) {
        registry.register_structure(structure);
    }

    Ok(())
}

fn is_startup_lua_mod(path: &Path) -> bool {
    if path.extension().and_then(|extension| extension.to_str()) != Some("lua") {
        return false;
    }

    path.file_name()
        .and_then(|file_name| file_name.to_str())
        .is_none_or(|file_name| !NON_STARTUP_LUA_FILES.contains(&file_name))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Minimal prelude that sets up Behaviors and trigger_behavior with aliasing.
    const MINI_PRELUDE: &str = r#"
        Behaviors = {}
        function register_behavior(block_id, callbacks)
            Behaviors[block_id] = callbacks
        end
        function trigger_behavior(block_id, event_name, ...)
            local callbacks = Behaviors[block_id]
            if not callbacks then return end
            local handler = callbacks[event_name]
            if not handler and event_name == "on_broken" then
                handler = callbacks.on_block_break
            elseif not handler and event_name == "on_block_break" then
                handler = callbacks.on_broken
            elseif not handler and event_name == "on_placed" then
                handler = callbacks.on_block_place
            elseif not handler and event_name == "on_block_place" then
                handler = callbacks.on_placed
            end
            if handler then handler(...) end
        end
        function spawn_particle(...) end
    "#;

    #[test]
    fn trigger_behavior_calls_on_block_break_handler() {
        let lua = Lua::new();
        lua.load(MINI_PRELUDE).exec().unwrap();
        lua.load(
            r#"
            _LastPos = nil
            register_behavior("test_ore", {
                on_block_break = function(x, y, z)
                    _LastPos = { x = x, y = y, z = z }
                end
            })
            trigger_behavior("test_ore", "on_block_break", 5, 10, 15)
        "#,
        )
        .exec()
        .expect("trigger_behavior must call the handler");

        let pos: mlua::Table = lua
            .globals()
            .get("_LastPos")
            .expect("on_block_break handler must have set _LastPos");
        assert_eq!(pos.get::<i32>("x").unwrap(), 5);
        assert_eq!(pos.get::<i32>("y").unwrap(), 10);
        assert_eq!(pos.get::<i32>("z").unwrap(), 15);
    }

    #[test]
    fn trigger_behavior_aliases_on_broken_to_on_block_break() {
        let lua = Lua::new();
        lua.load(MINI_PRELUDE).exec().unwrap();
        lua.load(
            r#"
            _AliasWorked = false
            register_behavior("legacy_block", {
                on_broken = function(x, y, z)
                    _AliasWorked = true
                end
            })
            -- Rust fires "on_block_break"; legacy mod only defines "on_broken".
            trigger_behavior("legacy_block", "on_block_break", 0, 0, 0)
        "#,
        )
        .exec()
        .expect("alias test should succeed");

        let alias_worked: bool = lua.globals().get("_AliasWorked").unwrap();
        assert!(alias_worked, "on_broken must be called when on_block_break is triggered");
    }

    #[test]
    fn trigger_behavior_aliases_on_placed_to_on_block_place() {
        let lua = Lua::new();
        lua.load(MINI_PRELUDE).exec().unwrap();
        lua.load(
            r#"
            _PlaceWorked = false
            register_behavior("legacy_block", {
                on_placed = function(x, y, z)
                    _PlaceWorked = true
                end
            })
            trigger_behavior("legacy_block", "on_block_place", 0, 0, 0)
        "#,
        )
        .exec()
        .expect("place alias test should succeed");

        let worked: bool = lua.globals().get("_PlaceWorked").unwrap();
        assert!(worked, "on_placed must be called when on_block_place is triggered");
    }

    #[test]
    fn trigger_behavior_chain_reaction_lua_side() {
        let lua = Lua::new();
        lua.load(MINI_PRELUDE).exec().unwrap();
        lua.load(
            r#"
            _ChainCount = 0
            register_behavior("chain_block", {
                on_block_break = function(x, y, z)
                    _ChainCount = _ChainCount + 1
                    -- Trigger one nested chain call (stops at depth 1 to avoid infinite loop)
                    if _ChainCount == 1 then
                        trigger_behavior("chain_block", "on_block_break", x + 1, y, z)
                    end
                end
            })
            trigger_behavior("chain_block", "on_block_break", 10, 5, 10)
        "#,
        )
        .exec()
        .expect("chain reaction test should succeed");

        let count: i32 = lua.globals().get("_ChainCount").unwrap();
        assert_eq!(count, 2, "chain reaction must call handler twice (initial + one nested)");
    }

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
                gravity_affected = false,
                strength = 3.5,
            })
            "#,
            &mut registry,
        )
        .expect("Lua mod should register a valid block");

        let blocks: Vec<_> = registry.drain_blocks().collect();
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].id, "ruby_ore");
        assert_eq!(blocks[0].color, (0.9, 0.05, 0.12, 1.0));
        assert_eq!(blocks[0].strength, Some(3.5));
        assert_eq!(blocks[0].textures, None);
    }

    #[test]
    fn lua_mod_can_register_block_with_named_textures() {
        let mut registry = ModRegistry::default();

        run_lua_mod(
            r#"
            register_block({
                id = "emerald_ore",
                name = "Emerald Ore",
                is_solid = true,
                is_transparent = false,
                color = { 0.1, 0.9, 0.4, 1.0 },
                textures = { top = 13, side = 13, bottom = 13 },
            })
            "#,
            &mut registry,
        )
        .expect("Lua mod should register a block with named textures");

        let blocks: Vec<_> = registry.drain_blocks().collect();
        assert_eq!(blocks[0].textures, Some([13, 13, 13]));
    }

    #[test]
    fn lua_mod_can_register_block_with_array_textures() {
        let mut registry = ModRegistry::default();

        run_lua_mod(
            r#"
            register_block({
                id = "grass",
                name = "Grass",
                is_solid = true,
                is_transparent = false,
                color = { 0.2, 0.8, 0.2, 1.0 },
                textures = { 0, 1, 2 },
            })
            "#,
            &mut registry,
        )
        .expect("Lua mod should register a block with array textures");

        let blocks: Vec<_> = registry.drain_blocks().collect();
        assert_eq!(blocks[0].textures, Some([0, 1, 2]));
    }

    #[test]
    fn lua_mod_can_register_structure() {
        let mut registry = ModRegistry::default();

        run_lua_mod(
            r#"
            register_structure({
                name = "totem",
                chance = 0.08,
                blocks = {
                    { dx = 0, dy = 0, dz = 0, block = "stone" },
                    { dx = 0, dy = 1, dz = 0, block = "stone" },
                    { dx = 0, dy = 2, dz = 0, block = "copper_ore" },
                }
            })
            "#,
            &mut registry,
        )
        .expect("Lua mod should register a valid structure");

        let structures: Vec<_> = registry.drain_structures().collect();
        assert_eq!(structures.len(), 1);
        assert_eq!(structures[0].name, "totem");
        assert_eq!(structures[0].blocks.len(), 3);
        assert_eq!(structures[0].blocks[2].block, "copper_ore");
    }
}
