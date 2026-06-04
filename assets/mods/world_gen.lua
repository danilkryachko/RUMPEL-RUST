-- Biome-aware procedural world generation (per-chunk, deterministic).
--
-- Runs once per chunk via rumpel_world::world_gen. All randomness is derived
-- from `rand01`/`chance` (FNV hash of salt + GLOBAL coordinates), so a chunk
-- regenerates identically and chunk borders never disagree.
--
-- Biome classification comes from the Rust terrain sampler:
--   get_biome(local_x, local_z) -> "beach" | "plains" | "forest" | "mountains"
--   sample_world(local_x, local_z) -> { height, biome, surface_block, ... }
-- Coordinates passed to these helpers are chunk-local; the runtime adds the
-- chunk origin before sampling, so callers stay in 0..CHUNK_MAX.

local SEA_LEVEL = 12
local CHUNK_SIZE = Chunk.size or 32
local CHUNK_MAX = CHUNK_SIZE - 1
local ORIGIN_X = Chunk.origin_x or (Chunk.x * CHUNK_SIZE)
local ORIGIN_Z = Chunk.origin_z or (Chunk.z * CHUNK_SIZE)
local MOUNTAIN_SNOW_HEIGHT = 30

-- ── Helpers ──────────────────────────────────────────────────────────────────

local function in_chunk(x, y, z)
    return x >= 0 and x < CHUNK_SIZE
        and y >= 0 and y < CHUNK_SIZE
        and z >= 0 and z < CHUNK_SIZE
end

local function rand_range(salt, x, z, min_value, max_value)
    local span = max_value - min_value + 1
    return min_value + math.floor(rand01(salt, x, z) * span)
end

local function spawn_leaf_sphere(cx, cy, cz, radius)
    for lx = -radius, radius do
        for ly = -radius, radius do
            for lz = -radius, radius do
                if lx * lx + ly * ly + lz * lz <= radius * radius then
                    local px, py, pz = cx + lx, cy + ly, cz + lz
                    if in_chunk(px, py, pz) and get_block(px, py, pz) == "air" then
                        set_block(px, py, pz, "leaves")
                    end
                end
            end
        end
    end
end

-- Giant branching oak. `height` lets forests grow taller trees than plains.
local function spawn_organic_oak(tx, ty, tz, height)
    height = height or 6
    for y = ty, ty + height - 1 do
        if y >= 0 and y < CHUNK_SIZE then
            set_block(tx, y, tz, "wood")
        end
    end
    spawn_leaf_sphere(tx, ty + height, tz, 2)

    local branch_dirs = {
        { x = 1, z = 1 }, { x = -1, z = 1 }, { x = 1, z = -1 }, { x = -1, z = -1 },
    }
    for _, dir in ipairs(branch_dirs) do
        local bx, by, bz = tx + dir.x, ty + height - 2, tz + dir.z
        if in_chunk(bx, by, bz) then
            set_block(bx, by, bz, "wood")
            local bxx, byy, bzz = bx + dir.x, by + 1, bz + dir.z
            if in_chunk(bxx, byy, bzz) then
                set_block(bxx, byy, bzz, "wood")
                spawn_leaf_sphere(bxx, byy + 1, bzz, 2)
            end
        end
    end
end

-- Small pine: tall narrow trunk + stacked leaf rings. Used in mountains.
local function spawn_pine(tx, ty, tz, height)
    for y = ty, ty + height - 1 do
        if y >= 0 and y < CHUNK_SIZE then
            set_block(tx, y, tz, "wood")
        end
    end
    for ring = 0, 2 do
        local ry = ty + height - 1 - ring * 2
        local radius = ring + 1
        for dx = -radius, radius do
            for dz = -radius, radius do
                if math.abs(dx) + math.abs(dz) <= radius then
                    local px, pz = tx + dx, tz + dz
                    if in_chunk(px, ry, pz) and get_block(px, ry, pz) == "air" then
                        set_block(px, ry, pz, "leaves")
                    end
                end
            end
        end
    end
    if in_chunk(tx, ty + height, tz) then
        set_block(tx, ty + height, tz, "leaves")
    end
end

-- ── 1. Sea level water fill ──────────────────────────────────────────────────

for x = 0, CHUNK_MAX do
    for z = 0, CHUNK_MAX do
        for y = 0, SEA_LEVEL do
            if get_block(x, y, z) == "air" then
                set_block(x, y, z, "water")
            end
        end
    end
end

-- ── 2. Biome-driven surface repaint ──────────────────────────────────────────
-- Top soil block follows the biome: sand on beaches, snow on mountain peaks,
-- grass elsewhere. Underwater columns stay as-is.

for x = 0, CHUNK_MAX do
    for z = 0, CHUNK_MAX do
        local h = get_height(x, z)
        if h > SEA_LEVEL and h <= CHUNK_MAX and get_block(x, h, z) ~= "air" then
            local biome = get_biome(x, z)
            if biome == "beach" then
                set_block(x, h, z, "sand")
            elseif biome == "mountains" and h >= MOUNTAIN_SNOW_HEIGHT then
                set_block(x, h, z, "snow")
            end
        end
    end
end

-- ── 3. Biome-driven vegetation ───────────────────────────────────────────────
-- Forests: dense tall oaks. Plains: sparse short oaks. Mountains: pines below
-- the snow line. Beaches: bare sand.

for x = 2, CHUNK_MAX - 2, 3 do
    for z = 2, CHUNK_MAX - 2, 3 do
        local h = get_height(x, z)
        if h > SEA_LEVEL and h < CHUNK_MAX - 4 and get_block(x, h, z) == "grass" then
            local biome = get_biome(x, z)
            if biome == "forest" then
                if chance("tree_forest", x, z, 0.55) then
                    spawn_organic_oak(x, h + 1, z, 6)
                end
            elseif biome == "plains" then
                if chance("tree_plains", x, z, 0.12) then
                    spawn_organic_oak(x, h + 1, z, 4)
                end
            elseif biome == "mountains" and h < MOUNTAIN_SNOW_HEIGHT then
                if chance("tree_mountain", x, z, 0.20) then
                    spawn_pine(x, h + 1, z, 5)
                end
            end
        end
    end
end

-- ── 4. Forest floor detail (logs + bushes), forest only ───────────────────────

for k = 1, 4 do
    local lx = rand_range("log_x_" .. k, ORIGIN_X, ORIGIN_Z, 3, CHUNK_MAX - 3)
    local lz = rand_range("log_z_" .. k, ORIGIN_X, ORIGIN_Z, 3, CHUNK_MAX - 3)
    local lh = get_height(lx, lz)
    if lh > SEA_LEVEL and lh < CHUNK_MAX - 3
        and get_biome(lx, lz) == "forest"
        and get_block(lx, lh, lz) == "grass" then
        set_block(lx, lh + 1, lz, "wood")
        if lx + 1 < CHUNK_SIZE then set_block(lx + 1, lh + 1, lz, "wood") end
        if lx - 1 >= 0 then set_block(lx - 1, lh + 1, lz, "wood") end
    end
end

for k = 1, 6 do
    local bx = rand_range("bush_x_" .. k, ORIGIN_X, ORIGIN_Z, 3, CHUNK_MAX - 3)
    local bz = rand_range("bush_z_" .. k, ORIGIN_X, ORIGIN_Z, 3, CHUNK_MAX - 3)
    local bh = get_height(bx, bz)
    if bh > SEA_LEVEL and bh < CHUNK_MAX - 3
        and get_biome(bx, bz) == "forest"
        and get_block(bx, bh, bz) == "grass" then
        set_block(bx, bh + 1, bz, "leaves")
        if bx + 1 < CHUNK_SIZE then set_block(bx + 1, bh + 1, bz, "leaves") end
        if bx - 1 >= 0 then set_block(bx - 1, bh + 1, bz, "leaves") end
        if bz + 1 < CHUNK_SIZE then set_block(bx, bh + 1, bz + 1, "leaves") end
        if bz - 1 >= 0 then set_block(bx, bh + 1, bz - 1, "leaves") end
        set_block(bx, bh + 2, bz, "leaves")
    end
end

-- ── 5. Deterministic structures (no chunk-border duplicates) ──────────────────
-- Each structure is rolled once per chunk, keyed by the chunk's GLOBAL origin,
-- so neighbouring chunks decide independently and a structure appears in only
-- the chunk that rolled it. Anchors are clamped so the footprint stays inside
-- the chunk and cannot be clipped at a border.

local function build_cabin(cx, cz, ch)
    for x = cx - 2, cx + 2 do
        for z = cz - 2, cz + 2 do
            for y = ch, ch + 3 do
                local is_corner = (x == cx - 2 or x == cx + 2) and (z == cz - 2 or z == cz + 2)
                if x == cx - 2 or x == cx + 2 or z == cz - 2 or z == cz + 2 then
                    if y == ch + 3 then
                        set_block(x, y, z, "cobblestone")
                    elseif is_corner then
                        set_block(x, y, z, "wood")
                    else
                        set_block(x, y, z, "oak_planks")
                    end
                else
                    set_block(x, y, z, "air")
                end
            end
        end
    end
    set_block(cx - 2, ch + 1, cz, "glass")
    set_block(cx + 2, ch + 1, cz, "glass")
    set_block(cx, ch, cz - 2, "air")
    set_block(cx, ch + 1, cz - 2, "air")
    for x = cx - 2, cx + 2 do
        for z = cz - 2, cz + 2 do
            set_block(x, ch + 3, z, "cobblestone")
        end
    end
    for y = ch, ch + 4 do
        set_block(cx + 2, y, cz + 1, "stone_bricks")
    end
    set_block(cx, ch, cz, "oak_planks")
    set_block(cx - 1, ch, cz + 1, "bookshelf")
    set_block(cx - 1, ch, cz - 1, "stone")
    set_block(cx - 1, ch + 1, cz - 1, "crystal_ore")
end

local function build_well(wx, wz, wh)
    for x = wx - 1, wx + 1 do
        for z = wz - 1, wz + 1 do
            if x == wx and z == wz then
                for y = wh - 4, wh do
                    set_block(x, y, z, "water")
                end
            else
                set_block(x, wh, z, "cobblestone")
            end
        end
    end
    for y = wh + 1, wh + 3 do
        set_block(wx - 1, y, wz, "wood")
        set_block(wx + 1, y, wz, "wood")
    end
    for x = wx - 1, wx + 1 do
        for z = wz - 1, wz + 1 do
            set_block(x, wh + 4, z, "cobblestone")
        end
    end
end

-- A chunk hosts at most one structure. Cabins prefer settled biomes; wells are
-- rarer and only roll when no cabin was placed in this chunk.
local placed_structure = false

if chance("cabin_v2", ORIGIN_X, ORIGIN_Z, 0.05) then
    local ax = rand_range("cabin_ax", ORIGIN_X, ORIGIN_Z, 3, CHUNK_MAX - 3)
    local az = rand_range("cabin_az", ORIGIN_X, ORIGIN_Z, 3, CHUNK_MAX - 3)
    local biome = get_biome(ax, az)
    local ah = get_height(ax, az)
    if (biome == "plains" or biome == "forest") and ah > SEA_LEVEL and ah < CHUNK_MAX - 4 then
        build_cabin(ax, az, ah)
        placed_structure = true
    end
end

if not placed_structure and chance("well_v2", ORIGIN_X, ORIGIN_Z, 0.04) then
    local ax = rand_range("well_ax", ORIGIN_X, ORIGIN_Z, 2, CHUNK_MAX - 2)
    local az = rand_range("well_az", ORIGIN_X, ORIGIN_Z, 2, CHUNK_MAX - 2)
    local biome = get_biome(ax, az)
    local ah = get_height(ax, az)
    if biome ~= "beach" and ah > SEA_LEVEL and ah < CHUNK_MAX - 5 then
        build_well(ax, az, ah)
        placed_structure = true
    end
end

-- ── 6. Subterranean glowing crystal grottos ───────────────────────────────────

for c = 1, 4 do
    local kx = rand_range("cave_x_" .. c, ORIGIN_X, ORIGIN_Z, 4, CHUNK_MAX - 4)
    local kz = rand_range("cave_z_" .. c, ORIGIN_X, ORIGIN_Z, 4, CHUNK_MAX - 4)
    local ky = rand_range("cave_y_" .. c, ORIGIN_X, ORIGIN_Z, 3, 9)
    for dx = -1, 1 do
        for dy = -1, 1 do
            for dz = -1, 1 do
                local px, py, pz = kx + dx, ky + dy, kz + dz
                if in_chunk(px, py, pz) then
                    if dx * dx + dy * dy + dz * dz <= 1 then
                        set_block(px, py, pz, "air")
                    elseif chance("crystal_cave_" .. c, px, pz + py * CHUNK_SIZE, 0.35) then
                        set_block(px, py, pz, "crystal_ore")
                    end
                end
            end
        end
    end
end

-- ── 7. Wildflowers (plains + forest grass only) ───────────────────────────────

for x = 0, CHUNK_MAX do
    for z = 0, CHUNK_MAX do
        local h = get_height(x, z)
        if h > SEA_LEVEL and h < CHUNK_MAX and get_block(x, h, z) == "grass" then
            local biome = get_biome(x, z)
            if biome == "plains" or biome == "forest" then
                local roll = rand01("flower", x, z)
                if roll < 0.04 then
                    set_block(x, h + 1, z, "flower_red")
                elseif roll < 0.08 then
                    set_block(x, h + 1, z, "flower_yellow")
                end
            end
        end
    end
end

-- ── 8. Ambient mobs (deterministic spots, land biomes only) ───────────────────

for k = 1, 3 do
    local sx = rand_range("slime_x_" .. k, ORIGIN_X, ORIGIN_Z, 2, CHUNK_MAX - 2)
    local sz = rand_range("slime_z_" .. k, ORIGIN_X, ORIGIN_Z, 2, CHUNK_MAX - 2)
    local h = get_height(sx, sz)
    if h > SEA_LEVEL and h < CHUNK_MAX - 3 and get_biome(sx, sz) ~= "beach"
        and chance("slime_spawn_" .. k, sx, sz, 0.5) then
        spawn_mob("slime", sx, h + 2, sz)
    end
end

for k = 1, 4 do
    local bx = rand_range("bfly_x_" .. k, ORIGIN_X, ORIGIN_Z, 2, CHUNK_MAX - 2)
    local bz = rand_range("bfly_z_" .. k, ORIGIN_X, ORIGIN_Z, 2, CHUNK_MAX - 2)
    local h = get_height(bx, bz)
    local biome = get_biome(bx, bz)
    if h > SEA_LEVEL and h < CHUNK_MAX - 3 and (biome == "plains" or biome == "forest")
        and chance("bfly_spawn_" .. k, bx, bz, 0.6) then
        spawn_mob("butterfly", bx, h + 2, bz)
    end
end
