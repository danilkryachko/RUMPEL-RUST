-- Block behaviors: on_block_break / on_block_place particle effects.
-- Block defs for crystal_ore / flowers live in hytale_palette.lua; glowstone in 00_blocks.lua.

-- ── Helpers ────────────────────────────────────────────────────────────────

local function burst(x, y, z, count, r, g, b, speed, life_base, size_base)
    for _ = 1, count do
        local vx = (math.random() - 0.5) * speed
        local vy = math.random() * speed * 0.6 + speed * 0.2
        local vz = (math.random() - 0.5) * speed
        local life = life_base + math.random() * life_base
        local sz   = size_base + math.random() * size_base
        spawn_particle(x + 0.5, y + 0.5, z + 0.5, vx, vy, vz, r, g, b, 0.9, life, sz)
    end
end

-- ── crystal_ore ────────────────────────────────────────────────────────────

register_behavior("crystal_ore", {
    on_block_break = function(x, y, z)
        -- Teal sparkle burst
        burst(x, y, z, 18, 0.15, 0.9, 0.85, 5.0, 0.3, 0.07)
        -- Brighter white-cyan highlight sparks
        for _ = 1, 6 do
            local vx = (math.random() - 0.5) * 8.0
            local vy = math.random() * 6.0 + 1.0
            local vz = (math.random() - 0.5) * 8.0
            spawn_particle(x + 0.5, y + 0.8, z + 0.5, vx, vy, vz, 0.7, 1.0, 0.95, 1.0, 0.5, 0.1)
        end
    end
})

-- ── glowstone ─────────────────────────────────────────────────────────────

register_behavior("glowstone", {
    on_block_place = function(x, y, z)
        -- Warm amber glow puff on placement
        burst(x, y, z, 12, 0.98, 0.82, 0.3, 3.0, 0.4, 0.12)
        -- Small bright white sparks
        burst(x, y, z, 5, 1.0, 0.96, 0.7, 4.5, 0.25, 0.07)
    end
})

-- ── flower_red ─────────────────────────────────────────────────────────────

register_behavior("flower_red", {
    on_block_break = function(x, y, z)
        -- Pink/red petal burst
        burst(x, y, z, 14, 0.92, 0.15, 0.22, 4.0, 0.35, 0.08)
        -- Pale-pink floaters
        burst(x, y, z, 5, 1.0, 0.65, 0.7, 2.5, 0.6, 0.05)
    end,

    on_block_place = function(x, y, z)
        -- Tiny green puff (freshly planted)
        burst(x, y, z, 5, 0.3, 0.75, 0.2, 1.5, 0.25, 0.05)
    end
})

-- ── flower_yellow ──────────────────────────────────────────────────────────

register_behavior("flower_yellow", {
    on_block_break = function(x, y, z)
        -- Yellow petal burst
        burst(x, y, z, 14, 0.97, 0.88, 0.1, 4.0, 0.35, 0.08)
        -- Pale-yellow floaters
        burst(x, y, z, 5, 1.0, 0.95, 0.55, 2.5, 0.6, 0.05)
    end,

    on_block_place = function(x, y, z)
        -- Tiny green puff (freshly planted)
        burst(x, y, z, 5, 0.3, 0.75, 0.2, 1.5, 0.25, 0.05)
    end
})
