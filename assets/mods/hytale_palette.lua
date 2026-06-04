-- Hytale Visual Palette Blocks Mod

-- 1. Glowing Crystal Ore — uses diamond_ore tile (12) as closest teal/cyan match
register_block({
    id = "crystal_ore",
    name = "Светящийся кристалл",
    is_solid = true,
    is_transparent = false,
    color = { 0.2, 0.9, 0.95, 1.0 },
    strength = 1.5,
    textures = { top = 12, side = 12, bottom = 12 },
})

-- 2. Wild Red Rose — redstone_ore tile (15) as a reddish tint
register_block({
    id = "flower_red",
    name = "Лесная роза",
    is_solid = false,
    is_transparent = true,
    color = { 0.95, 0.12, 0.22, 0.9 },
    strength = 0.1,
    textures = { top = 15, side = 15, bottom = 15 },
})

-- 3. Yellow Dandelion — gold_ore tile (11) as a yellow tint
register_block({
    id = "flower_yellow",
    name = "Одуванчик",
    is_solid = false,
    is_transparent = true,
    color = { 0.95, 0.85, 0.1, 0.9 },
    strength = 0.1,
    textures = { top = 11, side = 11, bottom = 11 },
})
