-- Hytale Visual Palette Blocks Mod in Lua!

-- 1. Register Glowing Crystal Ore (neon teal-cyan glowing block)
register_block({
    id = "crystal_ore",
    name = "Светящийся кристалл",
    is_solid = true,
    is_transparent = false,
    color = { 0.2, 0.9, 0.95, 1.0 },
    strength = 1.5,
})

-- 2. Register Wild Red Rose
register_block({
    id = "flower_red",
    name = "Лесная роза",
    is_solid = false,
    is_transparent = true,
    color = { 0.95, 0.12, 0.22, 0.9 },
    strength = 0.1,
})

-- 3. Register Yellow Dandelion
register_block({
    id = "flower_yellow",
    name = "Одуванчик",
    is_solid = false,
    is_transparent = true,
    color = { 0.95, 0.85, 0.1, 0.9 },
    strength = 0.1,
})
