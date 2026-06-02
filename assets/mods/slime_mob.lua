-- Slime Mob mod in Lua!

-- 1. Register the Slime mob
register_mob("slime", {
    color = { 0.2, 0.85, 0.2, 0.75 }, -- Cute semi-transparent green slime!
    size = { 0.9, 0.9, 0.9 },          -- Almost a perfect 1-block cube

    on_spawn = function(mob_id)
        print("SLIME: Cute green slime with ID " .. mob_id .. " has spawned into the world!")
    end,

    on_update = function(mob_id)
        -- Safe checks for player state and own state
        if not PlayerState.x or not MobStates[mob_id] then
            return
        end

        local px, py, pz = PlayerState.x, PlayerState.y, PlayerState.z
        local state = MobStates[mob_id]
        state.mob_type = "slime"
        local sx, sy, sz = state.x, state.y, state.z

        -- Compute distance to player
        local dx = px - sx
        local dy = py - sy
        local dz = pz - sz
        local dist = math.sqrt(dx*dx + dy*dy + dz*dz)

        -- If player is within chase range (16 blocks)
        if dist <= 16.0 then
            local dist_h = math.sqrt(dx*dx + dz*dz)
            if dist_h > 0.1 then
                -- Slime jumps/hops if it is on the ground
                if state.on_ground then
                    -- Hop direction pointing directly at the player
                    local jump_force_y = 6.2
                    local chase_speed = 3.2

                    state.vx = (dx / dist_h) * chase_speed
                    state.vy = jump_force_y
                    state.vz = (dz / dist_h) * chase_speed

                    print("SLIME [" .. mob_id .. "]: Hopping towards player! (Distance: " .. string.format("%.1f", dist) .. "m)")
                else
                    -- In mid-air: maintain horizontal velocity, just apply drag/friction slightly
                    state.vx = state.vx * 0.98
                    state.vz = state.vz * 0.98
                end
            end
        else
            -- Idle/Wander behavior if player is far away
            if state.on_ground then
                -- Randomly hop in place or slow hop to wander
                if math.random() < 0.05 then
                    local angle = math.random() * math.pi * 2
                    state.vx = math.cos(angle) * 1.5
                    state.vy = 4.0
                    state.vz = math.sin(angle) * 1.5
                else
                    state.vx = 0
                    state.vz = 0
                end
            else
                state.vx = state.vx * 0.95
                state.vz = state.vz * 0.95
            end
        end
    end
})
