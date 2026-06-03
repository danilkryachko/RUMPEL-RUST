-- Chat Commands Mod in Lua!

-- 1. Help Command
register_chat_command("help", function()
    add_chat_message("System", "Available commands:", { 0.9, 0.9, 0.9 })
    add_chat_message("", "  /help          - Show this command list", { 0.8, 0.8, 0.8 })
    add_chat_message("", "  /spawn <mob>   - Spawn a mob ('slime', 'zombie', 'butterfly')", { 0.8, 0.8, 0.8 })
    add_chat_message("", "  /time <time>   - Set sun angle ('day', 'night')", { 0.8, 0.8, 0.8 })
    add_chat_message("", "  /tnt           - Place a TNT block at your feet", { 0.8, 0.8, 0.8 })
    add_chat_message("", "  /rain          - Toggle localized storm downpour", { 0.8, 0.8, 0.8 })
    add_chat_message("", "  /clear         - Clear all messages in chat history", { 0.8, 0.8, 0.8 })
end)

-- 2. Spawn Mob Command
register_chat_command("spawn", function(args_str)
    local mob = args_str:trim():lower()
    if mob == "" then
        add_chat_message("System", "Usage: /spawn <slime | zombie | butterfly>", { 0.95, 0.25, 0.25 })
        return
    end

    if mob == "slime" or mob == "zombie" or mob == "butterfly" then
        if PlayerState.x then
            spawn_mob(mob, PlayerState.x, PlayerState.y + 1.5, PlayerState.z)
            add_chat_message("System", "Queued spawning of '" .. mob .. "' at player coords.", { 0.15, 0.72, 0.98 })
        else
            add_chat_message("System", "Player position is not loaded yet.", { 0.95, 0.25, 0.25 })
        end
    else
        add_chat_message("System", "Unknown mob '" .. mob .. "'. Valid types: slime, zombie, butterfly", { 0.95, 0.25, 0.25 })
    end
end)

-- 3. Time Control Command
register_chat_command("time", function(args_str)
    local target = args_str:trim():lower()
    if target == "" then
        add_chat_message("System", "Usage: /time <day | night>", { 0.95, 0.25, 0.25 })
        return
    end

    if target == "day" then
        TimeState.elapsed_time = 1.570796
        TimeState.sun_angle = 1.0
        add_chat_message("System", "Time set to Day (sun angle: 1.0)", { 0.95, 0.85, 0.1 })
    elseif target == "night" then
        TimeState.elapsed_time = -1.570796
        TimeState.sun_angle = -1.0
        add_chat_message("System", "Time set to Night (sun angle: -1.0)", { 0.65, 0.45, 0.95 })
    else
        add_chat_message("System", "Invalid time. Use 'day' or 'night'.", { 0.95, 0.25, 0.25 })
    end
end)

-- 4. TNT Spawning Command
register_chat_command("tnt", function()
    if PlayerState.x then
        local tx = math.floor(PlayerState.x)
        local ty = math.floor(PlayerState.y - 0.2)
        local tz = math.floor(PlayerState.z)

        if tx >= 0 and tx < 32 and ty >= 0 and ty < 32 and tz >= 0 and tz < 32 then
            set_block(tx, ty, tz, "tnt")
            add_chat_message("System", "Placed TNT under your feet at (" .. tx .. ", " .. ty .. ", " .. tz .. ")", { 0.95, 0.35, 0.05 })
        else
            add_chat_message("System", "Cannot place TNT outside chunk bounds.", { 0.95, 0.25, 0.25 })
        end
    else
        add_chat_message("System", "Player position is not loaded yet.", { 0.95, 0.25, 0.25 })
    end
end)

-- 5. Rain Toggle Command
register_chat_command("rain", function()
    if toggle_weather_rain then
        toggle_weather_rain()
    else
        add_chat_message("System", "Weather module is not loaded yet.", { 0.95, 0.25, 0.25 })
    end
end)

-- 6. Clear Command
register_chat_command("clear", function()
    -- Send magic command consumed in Rust queue to clear history
    add_chat_message("System", "CLEAR_CHAT_LOG", { 1.0, 1.0, 1.0 })
end)

-- Helper string trimming functions for Lua
function string.trim(s)
    return s:match("^%s*(.-)%s*$")
end
