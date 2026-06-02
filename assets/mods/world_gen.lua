-- Procedural world generation for «Emerald Grove» Biome in Lua!

local SEA_LEVEL = 12

-- Helper function to spawn spherical leaf crowns
local function spawn_leaf_sphere(cx, cy, cz, radius)
    for lx = -radius, radius do
        for ly = -radius, radius do
            for lz = -radius, radius do
                if lx*lx + ly*ly + lz*lz <= radius*radius then
                    local px = cx + lx
                    local py = cy + ly
                    local pz = cz + lz
                    if px >= 0 and px < 32 and py >= 0 and py < 32 and pz >= 0 and pz < 32 then
                        -- Place leaves if currently air
                        if get_block(px, py, pz) == "air" then
                            set_block(px, py, pz, "leaves")
                        end
                    end
                end
            end
        end
    end
end

-- Helper function to generate Hytale-style giant branching Oak trees with organic roots
local function spawn_organic_oak(tx, ty, tz)
    -- 1. Grow main thick trunk
    for y = ty, ty + 5 do
        if y >= 0 and y < 32 then
            set_block(tx, y, tz, "wood")
        end
    end

    -- 2. Fluffy leaf sphere on top of the trunk
    spawn_leaf_sphere(tx, ty + 6, tz, 2)

    -- 3. Sprout 4 majestic diagonal branches outwards
    local branch_dirs = {
        { x = 1, z = 1 },
        { x = -1, z = 1 },
        { x = 1, z = -1 },
        { x = -1, z = -1 },
    }

    for _, dir in ipairs(branch_dirs) do
        -- First branch block extending diagonally
        local bx = tx + dir.x
        local by = ty + 4
        local bz = tz + dir.z

        if bx >= 0 and bx < 32 and bz >= 0 and bz < 32 and by >= 0 and by < 32 then
            set_block(bx, by, bz, "wood")

            -- Extend branch further out and upwards
            local bxx = bx + dir.x
            local byy = by + 1
            local bzz = bz + dir.z

            if bxx >= 0 and bxx < 32 and bzz >= 0 and bzz < 32 and byy >= 0 and byy < 32 then
                set_block(bxx, byy, bzz, "wood")

                -- Leaf sphere at the tip of the branch!
                spawn_leaf_sphere(bxx, byy + 1, bzz, 2)
            end
        end
    end

    -- 4. Sprout 4 organic roots extending downwards from the trunk base (hugging soil)
    local root_dirs = {
        { x = 1, z = 0 },
        { x = -1, z = 0 },
        { x = 0, z = 1 },
        { x = 0, z = -1 },
    }
    for _, rdir in ipairs(root_dirs) do
        local rx = tx + rdir.x
        local ry = ty
        local rz = tz + rdir.z

        if rx >= 0 and rx < 32 and rz >= 0 and rz < 32 and ry >= 0 and ry < 32 then
            set_block(rx, ry, rz, "wood")

            -- Anchor root underground
            local rxx = rx + rdir.x
            local ryy = ry - 1
            local rzz = rz + rdir.z
            if rxx >= 0 and rxx < 32 and rzz >= 0 and rzz < 32 and ryy >= 0 and ryy < 32 then
                set_block(rxx, ryy, rzz, "wood")
            end
        end
    end
end


-- 1. Fill valleys up to Sea Level with water (Lakes & Streams)
for x = 0, 31 do
    for z = 0, 31 do
        for y = 0, SEA_LEVEL do
            if get_block(x, y, z) == "air" then
                set_block(x, y, z, "water")
            end
        end
    end
end

-- 2. Generate mountain waterfalls
local waterfall_count = 0
for x = 4, 27 do
    for z = 4, 27 do
        local h_curr = get_height(x, z)
        local h_next = get_height(x + 2, z)

        -- Check for a steep drop of 4+ blocks
        if h_curr >= 20 and h_curr > h_next + 4 then
            set_block(x, h_curr, z, "water")
            waterfall_count = waterfall_count + 1
            if waterfall_count >= 2 then
                break
            end
        end
    end
end

-- 3. Spawn giant Oak trees procedurally across grass surfaces
for x = 2, 29, 4 do
    for z = 2, 29, 4 do
        -- Skip well/cabin spots to avoid collisions
        if not (x >= 8 and x <= 12 and z >= 8 and z <= 12) and
           not (x >= 20 and x <= 24 and z >= 20 and z <= 24) then

            local h = get_height(x, z)
            if h > SEA_LEVEL and h < 24 then
                -- 60% chance to grow a tree at this grid coordinate
                if math.random() < 0.6 then
                    spawn_organic_oak(x, h + 1, z)
                end
            end
        end
    end
end

-- 3.5 Spawn Forest Floor Details (Fallen mossy logs & bushy leafy undergrowth)
-- Spawn 4 poваленные бревна on grass
for k = 1, 4 do
    local lx = math.random(3, 28)
    local lz = math.random(3, 28)
    local lh = get_height(lx, lz)
    if lh > SEA_LEVEL and lh < 28 then
        if get_block(lx, lh, lz) == "grass" then
            -- Place 3 horizontal logs on the ground
            set_block(lx, lh + 1, lz, "wood")
            if lx + 1 < 32 then set_block(lx + 1, lh + 1, lz, "wood") end
            if lx - 1 >= 0 then set_block(lx - 1, lh + 1, lz, "wood") end
        end
    end
end

-- Spawn 6 small organic leafy bushes
for k = 1, 6 do
    local bx = math.random(3, 28)
    local bz = math.random(3, 28)
    local bh = get_height(bx, bz)
    if bh > SEA_LEVEL and bh < 28 then
        if get_block(bx, bh, bz) == "grass" then
            set_block(bx, bh + 1, bz, "leaves")
            set_block(bx + 1, bh + 1, bz, "leaves")
            set_block(bx - 1, bh + 1, bz, "leaves")
            set_block(bx, bh + 1, bz + 1, "leaves")
            set_block(bx, bh + 1, bz - 1, "leaves")
            set_block(bx, bh + 2, bz, "leaves")
        end
    end
end


-- 4. Build a Cozy Wooden Forest Cabin at coordinate (10, h, 10)
local cx, cz = 10, 10
local ch = get_height(cx, cz)
if ch > SEA_LEVEL and ch < 26 then
    print("WORLD_GEN: Erecting Cozy Wooden Forest Cabin...")
    -- Spawn outline walls
    for x = cx - 2, cx + 2 do
        for z = cz - 2, cz + 2 do
            for y = ch, ch + 3 do
                local is_corner = (x == cx - 2 or x == cx + 2) and (z == cz - 2 or z == cz + 2)
                if x == cx - 2 or x == cx + 2 or z == cz - 2 or z == cz + 2 then
                    if y == ch + 3 then
                        -- Stone roof trim
                        set_block(x, y, z, "cobblestone")
                    elseif is_corner then
                        -- Corner logs
                        set_block(x, y, z, "wood")
                    else
                        -- Oak plank walls
                        set_block(x, y, z, "oak_planks")
                    end
                else
                    -- Hollow out inside
                    set_block(x, y, z, "air")
                end
            end
        end
    end

    -- Insert glass windows
    set_block(cx - 2, ch + 1, cz, "glass")
    set_block(cx + 2, ch + 1, cz, "glass")

    -- Open doorway on front
    set_block(cx, ch, cz - 2, "air")
    set_block(cx, ch + 1, cz - 2, "air")

    -- Solid cobblestone ceiling roof
    for x = cx - 2, cx + 2 do
        for z = cz - 2, cz + 2 do
            set_block(x, ch + 3, z, "cobblestone")
        end
    end

    -- Stone brick chimney on right wall
    set_block(cx + 2, ch, cz + 1, "stone_bricks")
    set_block(cx + 2, ch + 1, cz + 1, "stone_bricks")
    set_block(cx + 2, ch + 2, cz + 1, "stone_bricks")
    set_block(cx + 2, ch + 3, cz + 1, "stone_bricks")
    set_block(cx + 2, ch + 4, cz + 1, "stone_bricks")

    -- 4.5 Add Cozy Cabin Interior Furniture & Magical Lighting
    -- Oak planks dining table in center
    set_block(cx, ch, cz, "oak_planks")
    -- Cozy bookshelf in the corner
    set_block(cx - 1, ch, cz + 1, "bookshelf")
    -- Magical glowing crystal lantern on a stone pedestal in the opposite corner
    set_block(cx - 1, ch, cz - 1, "stone")
    set_block(cx - 1, ch + 1, cz - 1, "crystal_ore")
end

-- 5. Build Cobblestone Forest Well at coordinate (22, hw, 22)
local wx, wz = 22, 22
local wh = get_height(wx, wz)
if wh > SEA_LEVEL and wh < 26 then
    print("WORLD_GEN: Erecting Cobblestone Forest Well...")
    -- 3x3 circular base
    for x = wx - 1, wx + 1 do
        for z = wz - 1, wz + 1 do
            if x == wx and z == wz then
                -- Water shaft down
                for y = wh - 4, wh do
                    set_block(x, y, z, "water")
                end
            else
                -- Cobblestone borders
                set_block(x, wh, z, "cobblestone")
            end
        end
    end

    -- Wood pillars going up 3 blocks
    for y = wh + 1, wh + 3 do
        set_block(wx - 1, y, wz, "wood")
        set_block(wx + 1, y, wz, "wood")
    end

    -- Cobblestone well roof cap
    for x = wx - 1, wx + 1 do
        for z = wz - 1, wz + 1 do
            set_block(x, wh + 4, z, "cobblestone")
        end
    end
end

-- 6. Generate Subterranean Glowing Crystal Caves (Grottos)
for c = 1, 4 do
    local kx = math.random(4, 27)
    local kz = math.random(4, 27)
    local ky = math.random(3, 9)

    -- Carve out a pocket inside stone
    for dx = -1, 1 do
        for dy = -1, 1 do
            for dz = -1, 1 do
                local px = kx + dx
                local py = ky + dy
                local pz = kz + dz
                if px >= 0 and px < 32 and py >= 0 and py < 32 and pz >= 0 and pz < 32 then
                    if dx*dx + dy*dy + dz*dz <= 1 then
                        set_block(px, py, pz, "air")
                    else
                        -- Lined walls with glowing crystal blocks
                        if math.random() < 0.35 then
                            set_block(px, py, pz, "crystal_ore")
                        end
                    end
                end
            end
        end
    end
end

-- 7. Scatter lush fields of Wildflowers (roses & dandelions)
for x = 0, 31 do
    for z = 0, 31 do
        local h = get_height(x, z)
        if h > SEA_LEVEL and h < 31 then
            if get_block(x, h, z) == "grass" then
                local rand = math.random()
                if rand < 0.04 then
                    -- Red rose flower
                    set_block(x, h + 1, z, "flower_red")
                elseif rand < 0.08 then
                    -- Yellow dandelion flower
                    set_block(x, h + 1, z, "flower_yellow")
                end
            end
        end
    end
end

-- 8. Spawn procedural Slime mobs on top of grass surfaces
local slime_spots = {
    { x = 5, z = 5 },
    { x = 15, z = 15 },
    { x = 27, z = 27 },
}

for _, spot in ipairs(slime_spots) do
    local h = get_height(spot.x, spot.z)
    if h > SEA_LEVEL and h < 28 then
        spawn_mob("slime", spot.x, h + 2, spot.z)
    end
end

-- 9. Spawn beautiful ambient butterflies near wildflower meadows
local butterfly_spots = {
    { x = 7, z = 12 },
    { x = 11, z = 24 },
    { x = 20, z = 6 },
    { x = 25, z = 18 },
    { x = 16, z = 29 },
}

for _, spot in ipairs(butterfly_spots) do
    local h = get_height(spot.x, spot.z)
    if h > SEA_LEVEL and h < 28 then
        spawn_mob("butterfly", spot.x, h + 2, spot.z)
    end
end
