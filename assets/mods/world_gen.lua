-- Bounded deterministic decor pass for the Rust terrain sampler.
-- Terrain height, biomes, water, caves, and ores are produced in Rust; this script
-- only adds local biome details that fit inside the current chunk.

local SEA_LEVEL = 12
local CHUNK_SIZE = Chunk.size or 32
local CHUNK_HEIGHT = Chunk.height or 320
local CHUNK_MAX = CHUNK_SIZE - 1

local function in_chunk(x, y, z)
    return x >= 0 and x < CHUNK_SIZE
        and y >= 0 and y < CHUNK_HEIGHT
        and z >= 0 and z < CHUNK_SIZE
end

local function clamp(value, min_value, max_value)
    if value < min_value then return min_value end
    if value > max_value then return max_value end
    return value
end

local function rand_range(salt, index, min_value, max_value)
    local span = max_value - min_value + 1
    return min_value + math.floor(rand01(salt, index, Chunk.x + Chunk.z * 8191) * span)
end

local function is_air_or_flower(block)
    return block == "air" or block == "flower_red" or block == "flower_yellow"
end

local function place_if_air(x, y, z, block)
    if in_chunk(x, y, z) and get_block(x, y, z) == "air" then
        set_block(x, y, z, block)
        return true
    end
    return false
end

local function place_replaceable(x, y, z, block)
    if in_chunk(x, y, z) and is_air_or_flower(get_block(x, y, z)) then
        set_block(x, y, z, block)
        return true
    end
    return false
end

local function sample_column(x, z)
    if x < 0 or x >= CHUNK_SIZE or z < 0 or z >= CHUNK_SIZE then
        return nil
    end
    return sample_world(x, z)
end

local function surface_y(sample)
    return clamp((sample and sample.height or 1) - 1, 0, CHUNK_HEIGHT - 1)
end

local function has_vertical_room(sample, blocks_above)
    local y = surface_y(sample)
    return y > SEA_LEVEL and y + blocks_above < CHUNK_HEIGHT
end

local function ground_is(sample, names)
    if sample == nil then return false end
    for _, name in ipairs(names) do
        if sample.surface_block == name then
            return true
        end
    end
    return false
end

local function slope_ok(x, z, max_delta)
    local center = sample_column(x, z)
    if center == nil then return false end
    local h = center.height
    local probes = {
        sample_column(clamp(x - 2, 0, CHUNK_MAX), z),
        sample_column(clamp(x + 2, 0, CHUNK_MAX), z),
        sample_column(x, clamp(z - 2, 0, CHUNK_MAX)),
        sample_column(x, clamp(z + 2, 0, CHUNK_MAX)),
    }
    for _, probe in ipairs(probes) do
        if probe ~= nil and math.abs(probe.height - h) > max_delta then
            return false
        end
    end
    return true
end

local function leaf_sphere(cx, cy, cz, radius, looseness)
    local radius_sq = radius * radius
    for lx = -radius, radius do
        for ly = -radius, radius do
            for lz = -radius, radius do
                local dist = lx * lx + ly * ly + lz * lz
                if dist <= radius_sq + looseness then
                    local px = cx + lx
                    local py = cy + ly
                    local pz = cz + lz
                    if in_chunk(px, py, pz)
                        and get_block(px, py, pz) == "air"
                        and rand01("leaf_gap", px + cy * 17, pz + radius * 31) > 0.10
                    then
                        set_block(px, py, pz, "leaves")
                    end
                end
            end
        end
    end
end

local function trunk_column(x, y, z, height)
    for dy = 0, height - 1 do
        if in_chunk(x, y + dy, z) then
            set_block(x, y + dy, z, "wood")
        end
    end
end

local function spawn_oak(x, y, z, height)
    trunk_column(x, y, z, height)
    leaf_sphere(x, y + height, z, 3, 2)
    local branches = {
        { dx = 1, dz = 0 },
        { dx = -1, dz = 0 },
        { dx = 0, dz = 1 },
        { dx = 0, dz = -1 },
    }
    for i, dir in ipairs(branches) do
        if rand01("oak_branch", x + i * 11, z + height * 23) < 0.78 then
            local bx = x + dir.dx
            local bz = z + dir.dz
            local by = y + math.max(2, height - 2)
            if in_chunk(bx, by, bz) then set_block(bx, by, bz, "wood") end
            if in_chunk(bx + dir.dx, by + 1, bz + dir.dz) then
                set_block(bx + dir.dx, by + 1, bz + dir.dz, "wood")
                leaf_sphere(bx + dir.dx, by + 2, bz + dir.dz, 2, 1)
            end
        end
    end
end

local function spawn_pine(x, y, z, height)
    trunk_column(x, y, z, height)
    local start_y = y + math.floor(height * 0.38)
    for layer_y = start_y, y + height do
        local t = (layer_y - start_y) / math.max(1, (y + height) - start_y)
        local radius = math.max(1, math.floor(3 - t * 2.2))
        for dx = -radius, radius do
            for dz = -radius, radius do
                if math.abs(dx) + math.abs(dz) <= radius + 1 then
                    place_if_air(x + dx, layer_y, z + dz, "leaves")
                end
            end
        end
    end
    place_if_air(x, y + height + 1, z, "leaves")
end

local function spawn_wetland_tree(x, y, z)
    trunk_column(x, y, z, 4)
    leaf_sphere(x, y + 4, z, 3, 3)
    for dy = 1, 3 do
        place_if_air(x + 2, y + dy, z, "leaves")
        place_if_air(x - 2, y + dy, z, "leaves")
        place_if_air(x, y + dy, z + 2, "leaves")
        place_if_air(x, y + dy, z - 2, "leaves")
    end
end

-- One marker voxel per shrub; surface_decor renders it as a grass-bush billboard
-- instead of a multi-block leaves cluster with tree-crown LOD billboards.
local function spawn_shrub(x, y, z)
    place_replaceable(x, y, z, "leaves")
end

local function spawn_dry_shrub(x, y, z)
    place_replaceable(x, y, z, "wood")
    if rand01("dry_shrub_a", x, z) < 0.42 then place_replaceable(x + 1, y, z, "wood") end
    if rand01("dry_shrub_b", x, z) < 0.42 then place_replaceable(x - 1, y, z, "wood") end
    if rand01("dry_shrub_c", x, z) < 0.42 then place_replaceable(x, y, z + 1, "wood") end
    if rand01("dry_shrub_d", x, z) < 0.42 then place_replaceable(x, y, z - 1, "wood") end
end

local function spawn_rock_cluster(x, y, z, material)
    local radius = rand01("rock_radius", x, z) < 0.35 and 2 or 1
    for dx = -radius, radius do
        for dz = -radius, radius do
            local dist = math.abs(dx) + math.abs(dz)
            if dist <= radius + 1 and rand01("rock_cell", x + dx, z + dz) < 0.68 then
                local h = 1 + math.floor(rand01("rock_height", x + dx, z + dz) * 2)
                for dy = 0, h - 1 do
                    place_replaceable(x + dx, y + dy, z + dz, material)
                end
            end
        end
    end
end

local function spawn_flower_patch(x, y, z, density)
    for dx = -2, 2 do
        for dz = -2, 2 do
            if in_chunk(x + dx, y, z + dz) and rand01("flower_patch", x + dx, z + dz) < density then
                local flower = rand01("flower_color", x + dx, z + dz) < 0.52 and "flower_yellow" or "flower_red"
                place_replaceable(x + dx, y, z + dz, flower)
            end
        end
    end
end

local function decor_density(sample)
    local biome = sample.biome
    if biome == "forest" then return 0.72 end
    if biome == "autumn_forest" then return 0.62 end
    if biome == "taiga" then return 0.56 end
    if biome == "wetlands" then return 0.42 end
    if biome == "plains" then return 0.22 end
    if biome == "mountains" then return 0.18 end
    if biome == "snow" then return 0.16 end
    if biome == "desert" or biome == "canyon" then return 0.08 end
    return 0.04
end

for x = 2, CHUNK_MAX - 2, 4 do
    for z = 2, CHUNK_MAX - 2, 4 do
        local sample = sample_column(x, z)
        if sample ~= nil and has_vertical_room(sample, 12) and slope_ok(x, z, 5) then
            local y = surface_y(sample) + 1
            local density = decor_density(sample)
            local roll = rand01("major_decor", x, z)
            if roll < density and ground_is(sample, { "grass", "snow", "sand", "clay", "gravel" }) then
                if sample.biome == "forest" or sample.biome == "autumn_forest" then
                    spawn_oak(x, y, z, rand_range("oak_height", x + z * 37, 5, 8))
                elseif sample.biome == "taiga" or sample.biome == "snow" then
                    spawn_pine(x, y, z, rand_range("pine_height", x + z * 41, 7, 11))
                elseif sample.biome == "wetlands" then
                    spawn_wetland_tree(x, y, z)
                elseif sample.biome == "mountains" then
                    spawn_pine(x, y, z, rand_range("mountain_pine_height", x + z * 43, 5, 8))
                elseif sample.biome == "desert" or sample.biome == "canyon" then
                    spawn_dry_shrub(x, y, z)
                elseif sample.biome == "plains" then
                    if rand01("plains_tree_vs_shrub", x, z) < 0.32 then
                        spawn_oak(x, y, z, rand_range("plains_oak_height", x + z * 47, 4, 6))
                    else
                        spawn_shrub(x, y, z)
                    end
                end
            elseif roll < density + 0.18 and ground_is(sample, { "grass", "snow", "sand", "gravel", "stone" }) then
                if sample.biome == "mountains" or sample.biome == "canyon" or sample.biome == "snow" then
                    spawn_rock_cluster(x, y, z, sample.biome == "snow" and "snow" or "stone")
                elseif sample.biome == "desert" then
                    spawn_rock_cluster(x, y, z, "sand")
                elseif sample.biome ~= "river" and sample.biome ~= "beach" then
                    spawn_shrub(x, y, z)
                end
            end
        end
    end
end

for x = 1, CHUNK_MAX - 1 do
    for z = 1, CHUNK_MAX - 1 do
        local sample = sample_column(x, z)
        if sample ~= nil and has_vertical_room(sample, 2) then
            local y = surface_y(sample) + 1
            if sample.surface_block == "grass" and sample.biome ~= "mountains" then
                local patch_density = sample.biome == "plains" and 0.020 or 0.010
                if sample.biome == "forest" or sample.biome == "autumn_forest" then
                    patch_density = 0.014
                elseif sample.biome == "wetlands" then
                    patch_density = 0.006
                end
                if rand01("flower_seed", x, z) < patch_density then
                    spawn_flower_patch(x, y, z, sample.biome == "plains" and 0.34 or 0.18)
                end
            elseif sample.surface_block == "sand"
                and (sample.biome == "desert" or sample.biome == "canyon")
                and rand01("desert_detail", x, z) < 0.012
            then
                spawn_dry_shrub(x, y, z)
            end
        end
    end
end

local function flat_area(cx, cz, radius, max_delta, allowed_blocks)
    local center = sample_column(cx, cz)
    if center == nil or not has_vertical_room(center, 7) then return nil end
    local base = center.height
    for x = cx - radius, cx + radius do
        for z = cz - radius, cz + radius do
            local s = sample_column(x, z)
            if s == nil or math.abs(s.height - base) > max_delta or not ground_is(s, allowed_blocks) then
                return nil
            end
        end
    end
    return center
end

local function spawn_ruin(cx, y, cz)
    for x = cx - 2, cx + 2 do
        for z = cz - 2, cz + 2 do
            if x == cx - 2 or x == cx + 2 or z == cz - 2 or z == cz + 2 then
                place_replaceable(x, y, z, "cobblestone")
                if rand01("ruin_wall", x, z) < 0.44 then
                    place_replaceable(x, y + 1, z, "cobblestone")
                end
            end
        end
    end
    place_replaceable(cx, y, cz, "stone_bricks")
    if rand01("ruin_crystal", cx, cz) < 0.38 then
        place_replaceable(cx, y + 1, cz, "crystal_ore")
    end
end

if chance("chunk_ruin", 0, 0, 0.035) then
    local cx = rand_range("ruin_x", 1, 8, 23)
    local cz = rand_range("ruin_z", 2, 8, 23)
    local sample = flat_area(cx, cz, 3, 2, { "grass", "sand", "gravel", "stone" })
    if sample ~= nil and (sample.biome == "plains"
        or sample.biome == "forest"
        or sample.biome == "autumn_forest"
        or sample.biome == "desert"
        or sample.biome == "canyon")
    then
        spawn_ruin(cx, surface_y(sample) + 1, cz)
    end
end

local mob_roll = rand01("ambient_mob_chunk", Chunk.x, Chunk.z)
if mob_roll < 0.10 then
    local x = rand_range("mob_x", 1, 4, 27)
    local z = rand_range("mob_z", 1, 4, 27)
    local sample = sample_column(x, z)
    if sample ~= nil and has_vertical_room(sample, 4) and sample.surface_block == "grass" then
        if sample.biome == "plains" or sample.biome == "wetlands" then
            spawn_mob("slime", x, surface_y(sample) + 2, z)
        elseif sample.biome == "forest" or sample.biome == "autumn_forest" then
            spawn_mob("butterfly", x, surface_y(sample) + 3, z)
        end
    end
end
