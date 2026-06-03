-- Register copper ore block with custom strength
register_block({
    id = "copper_ore",
    name = "Copper Ore",
    is_solid = true,
    is_transparent = false,
    color = { 0.78, 0.38, 0.16, 1.0 },
    gravity_affected = false,
    strength = 2.0,
})

-- Register a custom mod block "ruby_block" that falls under gravity!
register_block({
    id = "ruby_block",
    name = "Ruby Block (Sand-like)",
    is_solid = true,
    is_transparent = false,
    color = { 0.9, 0.05, 0.15, 1.0 },
    gravity_affected = true,
    strength = 3.0,
})

-- Register a custom copper totem structure
register_structure({
    name = "copper_totem",
    chance = 0.04, -- 4% chance per grass block spot
    blocks = {
        { dx = 0, dy = 0, dz = 0, block = "stone" },
        { dx = 0, dy = 1, dz = 0, block = "stone" },
        { dx = 0, dy = 2, dz = 0, block = "copper_ore" },
    }
})

-- Register a custom ruby arch structure
register_structure({
    name = "ruby_arch",
    chance = 0.02, -- 2% chance per grass block spot
    blocks = {
        { dx = -1, dy = 0, dz = 0, block = "stone" },
        { dx = -1, dy = 1, dz = 0, block = "stone" },
        { dx = 1, dy = 0, dz = 0, block = "stone" },
        { dx = 1, dy = 1, dz = 0, block = "stone" },
        { dx = 0, dy = 2, dz = 0, block = "ruby_block" },
        { dx = -1, dy = 2, dz = 0, block = "ruby_block" },
        { dx = 1, dy = 2, dz = 0, block = "ruby_block" },
    }
})

-- Register a custom mod block "stardust_ore" that glows like falling stars!
register_block({
    id = "stardust_ore",
    name = "Fallen Stardust",
    is_solid = true,
    is_transparent = false,
    color = { 0.2, 0.85, 1.0, 1.0 }, -- glowing light blue
    gravity_affected = false,
    strength = 1.5,
})
