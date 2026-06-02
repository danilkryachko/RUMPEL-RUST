-- Weather and World Events Mod in Lua!

-- Global Wind State accessible by all mod scripts!
WindState = {
    x = 0.5,
    y = 0.0,
    z = -0.3,
    speed = 1.0,
    change_timer = 0.0
}

local weather_timer = 0
local is_raining = false
local spawn_timer = 0
local butterfly_spawn_timer = 0

-- Falling Star Event System
local star_active = false
local star_x, star_y, star_z = 0, 0, 0
local star_vx, star_vy, star_vz = 0, 0, 0
local star_life = 0
local star_target_x, star_target_y, star_target_z = 0, 0, 0

function trigger_world_tick(dt)
    -- Safe checks for player state
    if not PlayerState.x then
        return
    end

    local px, py, pz = PlayerState.x, PlayerState.y, PlayerState.z
    local sun = TimeState.sun_angle or 1.0

    -- Update Dynamic Wind State
    WindState.change_timer = WindState.change_timer + dt

    -- Smoothly shift base wind angle using a slowly changing sine/cosine wave
    local base_angle = WindState.change_timer * 0.03
    local target_x = math.cos(base_angle) * 0.6
    local target_z = math.sin(base_angle * 1.3) * 0.6

    -- Smoothly interpolate current wind direction towards target direction
    WindState.x = WindState.x + (target_x - WindState.x) * dt * 0.2
    WindState.z = WindState.z + (target_z - WindState.z) * dt * 0.2

    -- In normal weather, wind speed is gentle (0.8 - 1.5). During storms, it becomes violent!
    local base_speed = 0.8 + math.sin(WindState.change_timer * 0.08) * 0.5
    if is_raining then
        -- Violent, gusty storm wind speeds!
        WindState.speed = base_speed + 2.5 + math.sin(WindState.change_timer * 0.4) * 0.8

        -- Periodic wind howling alerts during storms
        WindState.howl_timer = (WindState.howl_timer or 0) + dt
        if WindState.howl_timer >= 24.0 then
            WindState.howl_timer = 0.0
            if WindState.speed > 2.8 then
                add_chat_message("Astronomer", "💨 The wind is howling through the trees, bending the rain!", { 0.6, 0.8, 1.0 })
            end
        end
    else
        WindState.speed = base_speed
        WindState.howl_timer = 0.0
    end

    -- 1. Dynamic Weather System (Rain storms)
    weather_timer = weather_timer + dt
    if weather_timer >= 45.0 then
        weather_timer = 0.0
        is_raining = not is_raining
        if is_raining then
            print("WEATHER: A rain storm has started! Dark clouds roll in...")
        else
            print("WEATHER: The rain has stopped. The sky is clearing up.")
        end
    end

    if is_raining then
        -- Spawn falling blue rain particles above the player's head tracking their position!
        -- Localized rain particle effect is extremely efficient and beautiful!
        for i = 1, 15 do
            local rx = px + (math.random() - 0.5) * 20.0
            local ry = py + 12.0
            local rz = pz + (math.random() - 0.5) * 20.0

            -- Speed vector: falling down fast, dynamically influenced by wind direction and speed!
            local vx = WindState.x * WindState.speed * 4.2
            local vy = -16.0 - math.random() * 2.0
            local vz = WindState.z * WindState.speed * 4.2

            -- Light blue, semi-transparent unlit drops
            spawn_particle(rx, ry, rz, vx, vy, vz, 0.45, 0.65, 0.95, 0.55, 0.8, 0.06)
        end

        -- Random chance to trigger a lightning strike (approx 0.6% chance per tick)
        if math.random() < 0.006 then
            local lx = math.floor(px + (math.random() - 0.5) * 32.0)
            local lz = math.floor(pz + (math.random() - 0.5) * 32.0)

            if lx >= 0 and lx < 32 and lz >= 0 and lz < 32 then
                local ly = get_height(lx, lz)
                if ly > 0 and ly < 30 then
                    -- Spawn vertical glowing particle column (sky to ground)
                    for y_offset = 0, 30 do
                        local py_pos = ly + y_offset
                        -- Pure white core particle
                        spawn_particle(lx, py_pos, lz, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0, 0.25, 0.28)
                        -- Cyan outer glow particle (scattered slightly for jagged/natural volumetric look)
                        if math.random() < 0.5 then
                            local gx = lx + (math.random() - 0.5) * 0.4
                            local gz = lz + (math.random() - 0.5) * 0.4
                            spawn_particle(gx, py_pos, gz, 0.0, 0.0, 0.0, 0.2, 0.85, 1.0, 0.8, 0.35, 0.42)
                        end
                    end

                    -- Spawn high-energy spark splashes on the ground (25 particles)
                    for i = 1, 25 do
                        local vx = (math.random() - 0.5) * 6.0
                        local vy = math.random() * 4.0 + 1.0
                        local vz = (math.random() - 0.5) * 6.0
                        spawn_particle(lx, ly + 0.5, lz, vx, vy, vz, 0.3, 0.9, 1.0, 0.95, 0.6, 0.12)
                    end

                    -- Trigger blinding full-screen lighting flash in Rust/Bevy
                    TimeState.lightning_flash = 1.0

                    -- Send a chat boom announcement
                    add_chat_message("Astronomer", "BOOM! A lightning bolt has struck the grove nearby!", { 1.0, 0.35, 0.15 })
                end
            end
        end
    end

    -- 1.5. Dynamic Visual Wind Ambiance (Falling Leaves & Chimney Smoke)
    -- Oak leaf particles drift down from trees near the player (more leaves fall in high wind!)
    local leaf_spawn_chance = 0.04
    if WindState.speed > 2.0 then
        leaf_spawn_chance = 0.22
    end

    if math.random() < leaf_spawn_chance then
        for k = 1, math.random(1, 2) do
            -- Spawn slightly above and upwind from the player camera coordinates
            local lx = px - (WindState.x * 12.0) + (math.random() - 0.5) * 16.0
            local ly = py + 4.0 + math.random() * 6.0
            local lz = pz - (WindState.z * 12.0) + (math.random() - 0.5) * 16.0

            -- Leaf velocity: falls slowly while drifting horizontally with the wind
            local vx = WindState.x * WindState.speed * 2.2 + (math.random() - 0.5) * 0.3
            local vy = -1.2 - math.random() * 0.8
            local vz = WindState.z * WindState.speed * 2.2 + (math.random() - 0.5) * 0.3

            -- Forest green, semi-translucent leaf particles
            local lr = 0.22 + math.random() * 0.08
            local lg = 0.55 + math.random() * 0.12
            local lb = 0.18 + math.random() * 0.06
            local lsize = 0.08 + math.random() * 0.06
            local llifetime = 2.5 + math.random() * 1.5

            spawn_particle(lx, ly, lz, vx, vy, vz, lr, lg, lb, 0.85, llifetime, lsize)
        end
    end

    -- Cabin Chimney Smoke Emitter: rises up from stone chimney and drifts with the wind!
    local cabin_h = get_height(10, 10)
    if cabin_h > 0 and cabin_h < 30 then
        local chimney_x = 12.0
        local chimney_z = 11.0
        local chimney_y = cabin_h + 5.0

        WindState.smoke_timer = (WindState.smoke_timer or 0) + dt
        if WindState.smoke_timer >= 0.35 then
            WindState.smoke_timer = 0.0

            local sm_x = chimney_x + (math.random() - 0.5) * 0.2
            local sm_z = chimney_z + (math.random() - 0.5) * 0.2

            -- Rise up while being pushed horizontally by the wind vector!
            local vx = WindState.x * WindState.speed * 0.8
            local vy = 1.0 + math.random() * 0.4
            local vz = WindState.z * WindState.speed * 0.8

            local life = 1.8 + math.random() * 0.8
            local size = 0.12 + math.random() * 0.08
            spawn_particle(sm_x, chimney_y, sm_z, vx, vy, vz, 0.45, 0.45, 0.5, 0.5, life, size)
        end
    end

    -- 2. Night-time Mob Spawning (Zombies) & Cozy Forest Fireflies
    if sun < -0.3 then
        -- Reset daytime butterfly timer during the night
        butterfly_spawn_timer = 0
        -- Spawn cute neon green-yellow glowing firefly particles inside the grove!
        -- Floating slowly in the air around the player camera
        for i = 1, 3 do
            local fx = px + (math.random() - 0.5) * 26.0
            local fy = py + (math.random() - 0.5) * 4.0 + 1.0
            local fz = pz + (math.random() - 0.5) * 26.0

            local vx = (math.random() - 0.5) * 0.8 + WindState.x * WindState.speed * 0.4
            local vy = math.random() * 0.4 + 0.1
            local vz = (math.random() - 0.5) * 0.8 + WindState.z * WindState.speed * 0.4

            local size = 0.05 + math.random() * 0.05
            local life = 1.0 + math.random() * 1.5

            spawn_particle(fx, fy, fz, vx, vy, vz, 0.65, 0.95, 0.1, 0.9, life, size)
        end

        spawn_timer = spawn_timer + dt
        if spawn_timer >= 3.5 then
            spawn_timer = 0.0

            -- Only spawn if there are fewer than 8 zombies to prevent clogging
            local zombie_count = 0
            for id, mob in pairs(MobStates) do
                -- MobStates can contain dead/despawned mobs, so only check alive types
                if mob.x and mob.mob_type == "zombie" then
                    zombie_count = zombie_count + 1
                end
            end

            if zombie_count < 6 then
                -- Spawn a zombie near the player on solid ground!
                local zx = px + (math.random() - 0.5) * 36.0
                local zz = pz + (math.random() - 0.5) * 36.0

                -- Bounds check
                if zx >= 0 and zx < 32 and zz >= 0 and zz < 32 then
                    local zh = get_height(math.floor(zx), math.floor(zz))
                    if zh > 0 and zh < 30 then
                        spawn_mob("zombie", zx, zh + 2.0, zz)
                    end
                end
            end
        end
    else
        -- Daytime reset spawner timer
        spawn_timer = 0

        -- Daytime: spawn beautiful ambient butterflies near the player
        butterfly_spawn_timer = butterfly_spawn_timer + dt
        if butterfly_spawn_timer >= 5.0 then
            butterfly_spawn_timer = 0.0

            local butterfly_count = 0
            for id, mob in pairs(MobStates) do
                if mob.x and mob.mob_type == "butterfly" then
                    butterfly_count = butterfly_count + 1
                end
            end

            if butterfly_count < 5 then
                -- Spawn a butterfly near the player on solid ground
                local bx = px + (math.random() - 0.5) * 32.0
                local bz = pz + (math.random() - 0.5) * 32.0

                if bx >= 0 and bx < 32 and bz >= 0 and bz < 32 then
                    local bh = get_height(math.floor(bx), math.floor(bz))
                    if bh > 12 and bh < 28 then
                        spawn_mob("butterfly", bx, bh + 2.0, bz)
                    end
                end
            end
        end
    end

    -- 3. Falling Star Simulation & Stardust spawn
    if sun < -0.4 then
        if not star_active then
            -- 0.3% chance per tick to trigger a falling star (approx once per minute of night)
            if math.random() < 0.003 then
                local target_x = math.floor(px + (math.random() - 0.5) * 32.0)
                local target_z = math.floor(pz + (math.random() - 0.5) * 32.0)

                -- Bound target within standard chunk coordinates for safety
                if target_x >= 0 and target_x < 32 and target_z >= 0 and target_z < 32 then
                    local target_y = get_height(target_x, target_z)
                    if target_y > 0 and target_y < 30 then
                        star_active = true
                        star_target_x = target_x
                        star_target_y = target_y
                        star_target_z = target_z

                        -- Start high in the sky and offset
                        star_x = target_x - 18.0
                        star_z = target_z - 18.0
                        star_y = target_y + 36.0

                        -- Fly for exactly 1.2 seconds
                        local duration = 1.2
                        star_vx = 18.0 / duration
                        star_vz = 18.0 / duration
                        star_vy = -36.0 / duration
                        star_life = duration

                        add_chat_message("Astronomer", "Look! A blazing shooting star is shooting across the night sky!", { 0.2, 0.85, 1.0 })
                    end
                end
            end
        else
            -- Animate active falling star
            star_life = star_life - dt
            star_x = star_x + star_vx * dt
            star_y = star_y + star_vy * dt
            star_z = star_z + star_vz * dt

            -- Spawn beautiful sparkling trails of particles
            for i = 1, 3 do
                local p_vx = (math.random() - 0.5) * 0.8
                local p_vy = (math.random() - 0.5) * 0.8
                local p_vz = (math.random() - 0.5) * 0.8
                -- Cyan trails
                spawn_particle(star_x, star_y, star_z, p_vx, p_vy, p_vz, 0.2, 0.85, 1.0, 0.95, 0.6, 0.16)
                -- Golden core particles
                spawn_particle(star_x, star_y, star_z, p_vx * 0.5, p_vy * 0.5, p_vz * 0.5, 1.0, 0.9, 0.2, 0.95, 0.4, 0.1)
            end

            if star_life <= 0 then
                -- Impact landing!
                star_active = false

                -- Place the stardust ore block at the target ground location
                set_block(star_target_x, star_target_y + 1, star_target_z, "stardust_ore")

                -- 35+ particles high-energy impact burst!
                for i = 1, 40 do
                    local vx = (math.random() - 0.5) * 7.5
                    local vy = math.random() * 5.0 + 1.5
                    local vz = (math.random() - 0.5) * 7.5

                    -- Bright glowing neon trails splashing outwards
                    spawn_particle(star_target_x, star_target_y + 1.2, star_target_z, vx, vy, vz, 0.2, 0.85, 1.0, 0.9, 1.4, 0.15)
                end

                add_chat_message("Astronomer", "The shooting star has landed at (" .. star_target_x .. ", " .. (star_target_y + 1) .. ", " .. star_target_z .. ")! Go mine the stardust block!", { 0.15, 0.95, 0.45 })
            end
        end
    else
        -- Daytime safety reset
        star_active = false
    end

    -- Always sync weather rain state to global TimeState table
    TimeState.is_raining = is_raining
end

function toggle_weather_rain()
    is_raining = not is_raining
    weather_timer = 0.0
    TimeState.is_raining = is_raining
    if is_raining then
        print("WEATHER: Rain toggled ON via chat command!")
        add_chat_message("System", "Weather toggled: Rain storm started!", { 0.15, 0.72, 0.98 })
    else
        print("WEATHER: Rain toggled OFF via chat command!")
        add_chat_message("System", "Weather toggled: Skies cleared up.", { 0.95, 0.85, 0.1 })
    end
end
