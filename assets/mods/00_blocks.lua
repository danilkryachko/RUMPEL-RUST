-- Canonical block registry. Loads first (alphabetical sort).
-- Atlas tile reference (28 layers, indices 0-27):
--   0=grass_top  1=grass_side  2=dirt       3=stone      4=sand
--   5=wood_side  6=wood_top    7=leaves     8=coal_ore   9=iron_ore
--  10=copper_ore 11=gold_ore  12=diamond_ore 13=emerald_ore 14=lapis_ore
--  15=redstone_ore 16=cobblestone 17=stone_bricks 18=bricks 19=oak_planks
--  20=bookshelf  21=glass     22=obsidian   23=glowstone  24=snow
--  25=ice        26=gravel    27=clay
-- air (id 0) is bootstrapped from base.ron and must not be re-registered here.

-- ── Terrain surface ──────────────────────────────────────────────────────────

register_block({
    id = "grass",
    name = "Трава",
    is_solid = true,
    is_transparent = false,
    color = { 0.2, 0.8, 0.2, 1.0 },
    textures = { top = 0, side = 1, bottom = 2 },
})

register_block({
    id = "dirt",
    name = "Земля",
    is_solid = true,
    is_transparent = false,
    color = { 0.4, 0.2, 0.1, 1.0 },
    textures = { top = 2, side = 2, bottom = 2 },
})

register_block({
    id = "stone",
    name = "Камень",
    is_solid = true,
    is_transparent = false,
    color = { 0.5, 0.5, 0.5, 1.0 },
    textures = { top = 3, side = 3, bottom = 3 },
})

register_block({
    id = "sand",
    name = "Песок",
    is_solid = true,
    is_transparent = false,
    color = { 0.86, 0.78, 0.51, 1.0 },
    gravity_affected = true,
    textures = { top = 4, side = 4, bottom = 4 },
})

register_block({
    id = "gravel",
    name = "Гравий",
    is_solid = true,
    is_transparent = false,
    color = { 0.55, 0.52, 0.48, 1.0 },
    gravity_affected = true,
    textures = { top = 26, side = 26, bottom = 26 },
})

register_block({
    id = "snow",
    name = "Снег",
    is_solid = true,
    is_transparent = false,
    color = { 0.95, 0.97, 1.0, 1.0 },
    textures = { top = 24, side = 24, bottom = 24 },
})

register_block({
    id = "clay",
    name = "Глина",
    is_solid = true,
    is_transparent = false,
    color = { 0.6, 0.62, 0.7, 1.0 },
    textures = { top = 27, side = 27, bottom = 27 },
})

-- ── Flora & wood ─────────────────────────────────────────────────────────────

register_block({
    id = "wood",
    name = "Дерево",
    is_solid = true,
    is_transparent = false,
    color = { 0.55, 0.35, 0.16, 1.0 },
    textures = { top = 6, side = 5, bottom = 6 },
})

register_block({
    id = "leaves",
    name = "Листья",
    is_solid = true,
    is_transparent = true,
    color = { 0.15, 0.5, 0.15, 1.0 },
    textures = { top = 7, side = 7, bottom = 7 },
})

-- ── Ores ─────────────────────────────────────────────────────────────────────

register_block({
    id = "coal_ore",
    name = "Угольная руда",
    is_solid = true,
    is_transparent = false,
    color = { 0.15, 0.15, 0.15, 1.0 },
    strength = 2.0,
    textures = { top = 8, side = 8, bottom = 8 },
})

register_block({
    id = "iron_ore",
    name = "Железная руда",
    is_solid = true,
    is_transparent = false,
    color = { 0.75, 0.65, 0.55, 1.0 },
    strength = 2.5,
    textures = { top = 9, side = 9, bottom = 9 },
})

register_block({
    id = "copper_ore",
    name = "Медная руда",
    is_solid = true,
    is_transparent = false,
    color = { 0.72, 0.45, 0.2, 1.0 },
    strength = 2.0,
    textures = { top = 10, side = 10, bottom = 10 },
})

register_block({
    id = "gold_ore",
    name = "Золотая руда",
    is_solid = true,
    is_transparent = false,
    color = { 0.9, 0.8, 0.1, 1.0 },
    strength = 3.0,
    textures = { top = 11, side = 11, bottom = 11 },
})

register_block({
    id = "diamond_ore",
    name = "Алмазная руда",
    is_solid = true,
    is_transparent = false,
    color = { 0.4, 0.85, 0.9, 1.0 },
    strength = 4.0,
    textures = { top = 12, side = 12, bottom = 12 },
})

register_block({
    id = "emerald_ore",
    name = "Изумрудная руда",
    is_solid = true,
    is_transparent = false,
    color = { 0.1, 0.8, 0.3, 1.0 },
    strength = 4.0,
    textures = { top = 13, side = 13, bottom = 13 },
})

register_block({
    id = "lapis_ore",
    name = "Лазуритовая руда",
    is_solid = true,
    is_transparent = false,
    color = { 0.1, 0.2, 0.8, 1.0 },
    strength = 3.0,
    textures = { top = 14, side = 14, bottom = 14 },
})

register_block({
    id = "redstone_ore",
    name = "Красностонная руда",
    is_solid = true,
    is_transparent = false,
    color = { 0.75, 0.1, 0.1, 1.0 },
    strength = 3.0,
    textures = { top = 15, side = 15, bottom = 15 },
})

-- ── Crafted / structural ─────────────────────────────────────────────────────

register_block({
    id = "cobblestone",
    name = "Булыжник",
    is_solid = true,
    is_transparent = false,
    color = { 0.45, 0.45, 0.45, 1.0 },
    strength = 3.0,
    textures = { top = 16, side = 16, bottom = 16 },
})

register_block({
    id = "stone_bricks",
    name = "Каменные кирпичи",
    is_solid = true,
    is_transparent = false,
    color = { 0.48, 0.48, 0.48, 1.0 },
    strength = 4.0,
    textures = { top = 17, side = 17, bottom = 17 },
})

register_block({
    id = "bricks",
    name = "Кирпичи",
    is_solid = true,
    is_transparent = false,
    color = { 0.7, 0.35, 0.25, 1.0 },
    strength = 3.5,
    textures = { top = 18, side = 18, bottom = 18 },
})

register_block({
    id = "oak_planks",
    name = "Дубовые доски",
    is_solid = true,
    is_transparent = false,
    color = { 0.64, 0.5, 0.3, 1.0 },
    strength = 2.0,
    textures = { top = 19, side = 19, bottom = 19 },
})

register_block({
    id = "bookshelf",
    name = "Книжная полка",
    is_solid = true,
    is_transparent = false,
    color = { 0.6, 0.45, 0.25, 1.0 },
    strength = 1.5,
    textures = { top = 20, side = 20, bottom = 20 },
})

register_block({
    id = "glass",
    name = "Стекло",
    is_solid = true,
    is_transparent = true,
    color = { 0.9, 0.95, 1.0, 0.3 },
    strength = 0.5,
    textures = { top = 21, side = 21, bottom = 21 },
})

register_block({
    id = "obsidian",
    name = "Обсидиан",
    is_solid = true,
    is_transparent = false,
    color = { 0.1, 0.05, 0.15, 1.0 },
    strength = 10.0,
    textures = { top = 22, side = 22, bottom = 22 },
})

register_block({
    id = "glowstone",
    name = "Светокамень",
    is_solid = true,
    is_transparent = false,
    color = { 0.95, 0.85, 0.5, 1.0 },
    strength = 0.3,
    textures = { top = 23, side = 23, bottom = 23 },
})

register_block({
    id = "ice",
    name = "Лёд",
    is_solid = true,
    is_transparent = true,
    color = { 0.75, 0.85, 0.98, 0.8 },
    strength = 0.5,
    textures = { top = 25, side = 25, bottom = 25 },
})

-- ── Liquids ───────────────────────────────────────────────────────────────────
-- Uses ice tile (25) as placeholder until a dedicated water tile is added.

register_block({
    id = "water",
    name = "Вода",
    is_solid = false,
    is_transparent = true,
    color = { 0.1, 0.4, 0.9, 0.65 },
    textures = { top = 25, side = 25, bottom = 25 },
})
