-- TNT Mod in Lua!

-- 1. Register the TNT block with high visibility red color
register_block({
    id = "tnt",
    name = "Динамит",
    is_solid = true,
    is_transparent = false,
    color = { 0.9, 0.15, 0.15, 1.0 },
    strength = 1.0,
})

-- 2. Register behavior for TNT
register_behavior("tnt", {
    on_block_break = function(x, y, z)
        print("TNT: Exploded at coordinate: " .. x .. ", " .. y .. ", " .. z)

        -- Spawn 45 brilliant explosion fire embers and smoke cloud particles!
        for i = 1, 45 do
            local vx = (math.random() - 0.5) * 7.0
            local vy = (math.random() - 0.2) * 8.0 + 2.0
            local vz = (math.random() - 0.5) * 7.0
            local size = 0.2 + math.random() * 0.4
            local life = 0.4 + math.random() * 0.5

            if math.random() < 0.35 then
                -- Fire ember (bright vibrant orange/red)
                spawn_particle(x + 0.5, y + 0.5, z + 0.5, vx, vy, vz, 0.95, 0.35, 0.05, 1.0, life, size)
            else
                -- Smoke cloud (dark/mid grey)
                spawn_particle(x + 0.5, y + 0.5, z + 0.5, vx, vy, vz, 0.35, 0.35, 0.35, 0.8, life, size)
            end
        end

        local r = 3
        for dx = -r, r do
            for dy = -r, r do
                for dz = -r, r do
                    if dx*dx + dy*dy + dz*dz <= r*r then
                        local wx = x + dx
                        local wy = y + dy
                        local wz = z + dz

                        local current = get_block(wx, wy, wz)
                        if current ~= "air" then
                            set_block(wx, wy, wz, "air")

                            -- Chain reaction: adjacent TNT detonates
                            if current == "tnt" then
                                trigger_behavior("tnt", "on_block_break", wx, wy, wz)
                            end
                        end
                    end
                end
            end
        end
    end,

    on_step_on = function(x, y, z)
        print("TNT: Stepped on TNT at " .. x .. ", " .. y .. ", " .. z .. " -> Detonating!")
        set_block(x, y, z, "air")
        trigger_behavior("tnt", "on_block_break", x, y, z)
    end
})
