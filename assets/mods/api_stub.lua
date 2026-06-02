---@meta

---@class BlockDefinition
---@field id string
---@field name string
---@field is_solid boolean
---@field is_transparent boolean
---@field color number[]
---@field gravity_affected? boolean
---@field strength? number

---@class StructureBlock
---@field dx integer
---@field dy integer
---@field dz integer
---@field block string

---@class StructureDefinition
---@field name string
---@field chance? number
---@field blocks StructureBlock[]

---@class BlockBehavior
---@field on_broken? fun(x: integer, y: integer, z: integer)
---@field on_step_on? fun(x: integer, y: integer, z: integer)
---@field on_mine_tick? fun(x: integer, y: integer, z: integer, tool: string)
---@field on_neighbor_changed? fun(x: integer, y: integer, z: integer, nx: integer, ny: integer, nz: integer, neighbor: string)

---@class MobDefinition
---@field color? number[]
---@field size? number[]
---@field on_spawn? fun(mob_id: integer)
---@field on_update? fun(mob_id: integer)

---@class MobState
---@field mob_type string
---@field x number
---@field y number
---@field z number
---@field vx number
---@field vy number
---@field vz number
---@field on_ground boolean

---@class PlayerStateTable
---@field x number
---@field y number
---@field z number

---@class TimeStateTable
---@field elapsed_time number
---@field sun_angle number
---@field is_raining boolean
---@field lightning_flash number
---@field time_scale number

---@class ChunkTable
---@field x integer
---@field z integer

---@type table<string, BlockBehavior>
Behaviors = {}

---@type table<string, MobDefinition>
Mobs = {}

---@type table<integer, MobState>
MobStates = {}

---@type PlayerStateTable
PlayerState = {}

---@type TimeStateTable
TimeState = {}

---@type ChunkTable
Chunk = {}

---@param block BlockDefinition
function register_block(block) end

---@param structure StructureDefinition
function register_structure(structure) end

---@param block_id string
---@param callbacks BlockBehavior
function register_behavior(block_id, callbacks) end

---@param block_id string
---@param event_name string
---@param ... unknown
function trigger_behavior(block_id, event_name, ...) end

---@param mob_type string
---@param callbacks MobDefinition
function register_mob(mob_type, callbacks) end

---@param mob_type string
---@param x number
---@param y number
---@param z number
function spawn_mob(mob_type, x, y, z) end

---@param mob_id integer
function despawn_mob(mob_id) end

---@param mob_id integer
---@param mob_type string
function trigger_mob_spawn(mob_id, mob_type) end

---@param mob_id integer
---@param mob_type string
function trigger_mob_update(mob_id, mob_type) end

---@param x number
---@param y number
---@param z number
---@param vx number
---@param vy number
---@param vz number
---@param r number
---@param g number
---@param b number
---@param a number
---@param lifetime number
---@param size number
function spawn_particle(x, y, z, vx, vy, vz, r, g, b, a, lifetime, size) end

---@param x integer
---@param y integer
---@param z integer
---@return string block_id
function get_block(x, y, z) end

---@param x integer
---@param y integer
---@param z integer
---@param block_id string
function set_block(x, y, z, block_id) end

---@param x integer
---@param z integer
---@return integer surface_y
function get_height(x, z) end

---@param block_name string
---@param tool_name? string
---@return number seconds
function get_mining_time(block_name, tool_name) end

---@param dt number
function trigger_world_tick(dt) end
