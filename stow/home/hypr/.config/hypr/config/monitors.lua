-- Monitor wiki https://wiki.hypr.land/Configuring/Basics/Monitors/

-- Load the monitor profile matching the connected outputs (EDID-based
-- detection in config/profile.lua): monitors-home / monitors-office /
-- monitors-laptop at the hypr config root. Falls back to the gitignored
-- machine-local overrides, then a machine-agnostic default.
local profile = require("config.profile")
local ok = pcall(require, "monitors-" .. profile)
if not ok then
    ok = pcall(require, "config.monitors_local")
end
if not ok then
    -- Machine-agnostic default for every output: native resolution at the
    -- highest refresh rate. NOTE: "highrr" maximises Hz, so on a panel whose
    -- native mode is not also its highest-refresh mode (e.g. an ultrawide
    -- offering 1080p@120 alongside 3440x1440@100) it picks the lower
    -- resolution. Tune such a monitor in a monitors-<profile>.lua.
    hl.monitor({
        output    = "",
        mode      = "highrr",  -- highest refresh rate; "preferred" can cap at 60Hz
        position  = "auto",
        scale     = "auto",
    })
end
