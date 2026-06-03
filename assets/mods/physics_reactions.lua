-- Physics reactions and block behaviors!

-- 1. Register the Lava block
register_block({
    id = "lava",
    name = "Лава",
    is_solid = false,
    is_transparent = false,
    color = { 0.95, 0.3, 0.05, 1.0 },
    strength = 1.0,
})

-- 2. Define neighbor changed reactions
register_behavior("lava", {
    on_neighbor_changed = function(x, y, z, nx, ny, nz, neighbor)
        if neighbor == "water" then
            print("LAVA: Reacted with water at (" .. x .. ", " .. y .. ", " .. z .. ") -> Turning to obsidian")
            set_block(x, y, z, "obsidian")

            -- Spawn obsidian cooling particles
            for i = 1, 8 do
                local vx = (math.random() - 0.5) * 2.0
                local vy = math.random() * 2.0 + 1.0
                local vz = (math.random() - 0.5) * 2.0
                spawn_particle(x + 0.5, y + 1.1, z + 0.5, vx, vy, vz, 0.3, 0.1, 0.4, 0.8, 0.6, 0.12)
            end
        end
    end
})

register_behavior("water", {
    on_neighbor_changed = function(x, y, z, nx, ny, nz, neighbor)
        if neighbor == "lava" then
            print("WATER: Reacted with lava at (" .. x .. ", " .. y .. ", " .. z .. ") -> Evaporating / turning to cobblestone")
            set_block(x, y, z, "cobblestone")

            -- Spawn water evaporation steam particles
            for i = 1, 8 do
                local vx = (math.random() - 0.5) * 1.5
                local vy = math.random() * 3.0 + 2.0
                local vz = (math.random() - 0.5) * 1.5
                spawn_particle(x + 0.5, y + 1.1, z + 0.5, vx, vy, vz, 0.9, 0.9, 0.95, 0.6, 1.2, 0.25)
            end
        end
    end
})

-- 3. Define the global tool-based progressive mining time
function get_mining_time(block_name, tool_name)
    local tool = (tool_name or "hand"):lower()

    -- Pickaxe blocks
    if block_name == "stone" or block_name == "coal_ore" or block_name == "iron_ore" or
       block_name == "gold_ore" or block_name == "diamond_ore" or block_name == "copper_ore" or
       block_name == "ruby_block" or block_name == "obsidian" or block_name == "cobblestone" or
       block_name == "stone_bricks" then
        if tool == "pickaxe" then
            return 0.4
        else
            return 3.0
        end

    -- Axe blocks
    elseif block_name == "wood" or block_name == "leaves" or block_name == "bookshelf" or
           block_name == "oak_planks" then
        if tool == "axe" then
            return 0.3
        else
            return 1.8
        end

    -- Shovel blocks
    elseif block_name == "dirt" or block_name == "grass" or block_name == "sand" then
        if tool == "shovel" then
            return 0.2
        else
            return 1.0
        end

    -- TNT block
    elseif block_name == "tnt" then
        return 0.3

    -- Lava and Water
    elseif block_name == "lava" or block_name == "water" then
        return 0.1

    else
        return 1.5
    end
end

-- 4. Dynamic Mining Particles & Spawning Behaviors

local function spawn_mining_sparks(x, y, z, r, g, b, count)
    for i = 1, (count or 2) do
        local vx = (math.random() - 0.5) * 3.5
        local vy = math.random() * 3.0 + 1.0
        local vz = (math.random() - 0.5) * 3.5
        local size = 0.08 + math.random() * 0.12
        local life = 0.3 + math.random() * 0.4
        spawn_particle(x + 0.5, y + 0.5, z + 0.5, vx, vy, vz, r, g, b, 0.95, life, size)
    end
end

-- Register mine tick behaviors for solid block groups
local rock_blocks = { "stone", "coal_ore", "iron_ore", "gold_ore", "diamond_ore", "copper_ore", "ruby_block", "obsidian", "cobblestone", "stone_bricks" }
for _, id in ipairs(rock_blocks) do
    register_behavior(id, {
        on_mine_tick = function(x, y, z, tool)
            spawn_mining_sparks(x, y, z, 0.45, 0.45, 0.45, 2)
        end
    })
end

local wood_blocks = { "wood", "leaves", "bookshelf", "oak_planks" }
for _, id in ipairs(wood_blocks) do
    register_behavior(id, {
        on_mine_tick = function(x, y, z, tool)
            spawn_mining_sparks(x, y, z, 0.58, 0.42, 0.25, 2)
        end
    })
end

local soil_blocks = { "dirt", "grass", "sand" }
for _, id in ipairs(soil_blocks) do
    register_behavior(id, {
        on_mine_tick = function(x, y, z, tool)
            spawn_mining_sparks(x, y, z, 0.42, 0.32, 0.22, 2)
        end
    })
end
