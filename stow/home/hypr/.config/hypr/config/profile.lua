-- Detect which monitor profile to use based on connected DRM outputs.
-- Port names alone aren't enough: this laptop uses the same dock ports
-- (DP-2, HDMI-A-2) at home and at the office, so profiles are told apart
-- by the EDID identity string of the monitor plugged into each port.
-- Add new profiles here as you encounter new setups.
-- Profile order matters: first match wins, so put more-specific (more outputs) first.

local PROFILES = {
    {
        name  = "home",
        match = { ["HDMI-A-2"] = "GA2711" }, -- GA2711 1440p monitor (was "CMT" in its EDID before a firmware/EDID change)
    },
    {
        name  = "office",
        match = { ["HDMI-A-2"] = "LG FULL HD" }, -- LG 1080p monitor
    },
    {
        name           = "laptop",
        match          = {},
        require_solo   = true, -- only eDP-1 may be connected
    },
}

local function connected_outputs()
    local found = {}
    local ok, f = pcall(io.popen,
        "grep -l '^connected$' /sys/class/drm/card*-*/status 2>/dev/null | sed 's|.*card[0-9]*-||;s|/status||'")
    if ok and f then
        for line in f:lines() do
            local name = line:match("^%s*(.-)%s*$")
            if name ~= "" then found[name] = true end
        end
        f:close()
    end
    return found
end

local function edid_text(output)
    local ok, f = pcall(io.popen, "strings /sys/class/drm/card*-" .. output .. "/edid 2>/dev/null")
    if not ok or not f then return "" end
    local text = f:read("*a") or ""
    f:close()
    return text
end

local function detect()
    local connected = connected_outputs()
    for _, profile in ipairs(PROFILES) do
        local match = true
        for out, substr in pairs(profile.match) do
            if not connected[out] or not edid_text(out):find(substr, 1, true) then
                match = false
                break
            end
        end
        if match and profile.require_solo then
            for name in pairs(connected) do
                if name ~= "eDP-1" then match = false; break end
            end
        end
        if match then return profile.name end
    end
    return "laptop"
end

return detect()
