-- Beautiful Ambient Butterfly Mob mod in Lua!

register_mob("butterfly", {
    color = { 0.15, 0.72, 0.98, 0.85 }, -- Vibrant sky-blue/cyan translucent wings
    size = { 0.25, 0.25, 0.25 },          -- Extremely small ambient flying insect

    on_spawn = function(mob_id)
        print("BUTTERFLY: Magical cyan butterfly with ID " .. mob_id .. " has emerged into the grove!")
    end,

    on_update = function(mob_id)
        -- Safe checks for player state and own state
        if not PlayerState.x or not MobStates[mob_id] then
            return
        end

        local state = MobStates[mob_id]
        local sx, sy, sz = state.x, state.y, state.z
        local px, py, pz = PlayerState.x, PlayerState.y, PlayerState.z

        -- 1. Y-axis oscillation (flapping wings/fluttering fly animation)
        state.fly_timer = (state.fly_timer or 0) + 0.016

        -- base_y offsets gravity (-18 * dt in Rust), math.sin creates organic oscillation
        local base_y = 0.3
        local flutter = math.sin(state.fly_timer * 11.0) * 1.4
        state.vy = base_y + flutter

        -- 2. Clean 3D Wander paths
        if not state.target_vx or math.random() < 0.02 then
            local angle = math.random() * math.pi * 2
            local speed = 0.6 + math.random() * 0.9
            state.target_vx = math.cos(angle) * speed
            state.target_vz = math.sin(angle) * speed
        end
        state.vx = (state.vx or 0) + (state.target_vx - (state.vx or 0)) * 0.08
        state.vz = (state.vz or 0) + (state.target_vz - (state.vz or 0)) * 0.08

        -- Apply dynamic wind drift to lighter entities like butterflies!
        if _G.WindState or WindState then
            local w = _G.WindState or WindState
            state.vx = state.vx + w.x * w.speed * 0.5 * 0.08
            state.vz = state.vz + w.z * w.speed * 0.5 * 0.08
        end

        -- 3. Keep ground-safe altitude
        local gx = math.floor(sx)
        local gz = math.floor(sz)
        local h_ground = 12.0
        if gx >= 0 and gx < 32 and gz >= 0 and gz < 32 then
            h_ground = get_height(gx, gz)
        end

        if sy < h_ground + 1.2 then
            -- Boost upwards if too close to ground/water
            state.vy = state.vy + 2.0
        elseif sy > h_ground + 5.5 then
            -- Drift downwards if flying too high above the canopy
            state.vy = state.vy - 1.2
        end

        -- 4. Evasion Evasion! Panic-flee from player
        local dx = sx - px
        local dy = sy - py
        local dz = sz - pz
        local dist = math.sqrt(dx*dx + dy*dy + dz*dz)

        if dist <= 5.0 then
            -- Flee away from player camera coordinates!
            local dist_h = math.sqrt(dx*dx + dz*dz)
            if dist_h > 0.05 then
                state.vx = (dx / dist_h) * 4.5
                state.vz = (dz / dist_h) * 4.5
            else
                local angle = math.random() * math.pi * 2
                state.vx = math.cos(angle) * 4.5
                state.vz = math.sin(angle) * 4.5
            end
            state.vy = 3.5 -- Flutter up rapidly!

            -- Spawn extra trail sparkles in panic
            if math.random() < 0.4 then
                spawn_particle(
                    sx, sy, sz,
                    (math.random() - 0.5) * 1.5,
                    -0.5,
                    (math.random() - 0.5) * 1.5,
                    0.2, 0.8, 1.0, 0.9,
                    0.4, 0.04
                )
            end
        end

        -- 5. Cozy glowing cyan fairy dust sparkle trail
        if math.random() < 0.22 then
            spawn_particle(
                sx, sy, sz,
                (math.random() - 0.5) * 0.3,
                -0.1 - math.random() * 0.2,
                (math.random() - 0.5) * 0.3,
                0.15, 0.72, 0.98, 0.85, -- Beautiful translucent cyan sky-blue
                0.7 + math.random() * 0.5,
                0.04 + math.random() * 0.04
            )
        end
    end
})
