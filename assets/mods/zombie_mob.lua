-- Zombie Mob AI in Lua!

register_mob("zombie", {
    color = { 0.15, 0.42, 0.22, 1.0 }, -- Decayed dark green/teal zombie skin
    size = { 0.8, 1.8, 0.8 },          -- Standard player dimensions (tall!)

    on_spawn = function(mob_id)
        print("ZOMBIE: A decayed zombie (ID " .. mob_id .. ") has risen from the dark ground!")
    end,

    on_update = function(mob_id)
        if not PlayerState.x or not MobStates[mob_id] then
            return
        end

        local px, py, pz = PlayerState.x, PlayerState.y, PlayerState.z
        local state = MobStates[mob_id]
        state.mob_type = "zombie"
        local sx, sy, sz = state.x, state.y, state.z
        local sun = TimeState.sun_angle or 1.0

        -- Compute distance
        local dx = px - sx
        local dy = py - sy
        local dz = pz - sz
        local dist = math.sqrt(dx*dx + dy*dy + dz*dz)

        -- 1. Daylight Combustion (Zombies burn during the day!)
        if sun > 0.1 then
            state.burn_timer = (state.burn_timer or 0) + 0.016 -- Approx frame delta

            -- Spawn brilliant fire flame particles dancing on the zombie's body!
            if math.random() < 0.25 then
                local fx = sx + (math.random() - 0.5) * 0.8
                local fy = sy + math.random() * 1.6
                local fz = sz + (math.random() - 0.5) * 0.8

                local vx = (math.random() - 0.5) * 1.2
                local vy = math.random() * 2.0 + 1.2
                local vz = (math.random() - 0.5) * 1.2

                spawn_particle(fx, fy, fz, vx, vy, vz, 0.95, 0.35, 0.05, 0.9, 0.6, 0.15)
            end

            -- Zombies slow down and stop under combustion
            state.vx = state.vx * 0.85
            state.vz = state.vz * 0.85

            if state.burn_timer >= 4.0 then
                print("ZOMBIE [" .. mob_id .. "]: Burned to ashes under direct sunlight.")

                -- Spawn a big puff of grey ash particles on death
                for i = 1, 12 do
                    local vx = (math.random() - 0.5) * 2.0
                    local vy = math.random() * 2.5
                    local vz = (math.random() - 0.5) * 2.0
                    spawn_particle(sx, sy + 0.9, sz, vx, vy, vz, 0.25, 0.25, 0.25, 0.7, 0.6, 0.12)
                end

                despawn_mob(mob_id)
            end
            return
        end

        -- Extinguish burns at night
        state.burn_timer = 0

        -- 2. Night-time Chase AI (Walks steadily towards player)
        if dist <= 24.0 then
            local dist_h = math.sqrt(dx*dx + dz*dz)
            if dist_h > 0.5 then
                local speed = 1.9
                state.vx = (dx / dist_h) * speed
                state.vz = (dz / dist_h) * speed

                -- Simple climbing AI: if player is higher than zombie and zombie hits a wall/block, jump!
                if dy > 0.4 and state.on_ground and math.random() < 0.18 then
                    state.vy = 5.2
                    print("ZOMBIE [" .. mob_id .. "]: Climbing block to pursue player!")
                end

                -- Occasional zombie growl in logs
                if math.random() < 0.003 then
                    print("ZOMBIE [" .. mob_id .. "]: *Urrrggghhh... brains...*")
                end
            else
                state.vx = 0
                state.vz = 0
            end
        else
            -- Idle wander
            if state.on_ground then
                if math.random() < 0.02 then
                    local angle = math.random() * math.pi * 2
                    state.vx = math.cos(angle) * 0.8
                    state.vz = math.sin(angle) * 0.8
                else
                    state.vx = state.vx * 0.9
                    state.vz = state.vz * 0.9
                end
            end
        end
    end
})
