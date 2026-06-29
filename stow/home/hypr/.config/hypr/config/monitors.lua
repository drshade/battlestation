-- Monitor wiki https://wiki.hypr.land/Configuring/Basics/Monitors/

-- Machine-agnostic default for every output: native resolution at the highest
-- refresh rate. NOTE: "highrr" maximises Hz, so on a panel whose native mode is
-- not also its highest-refresh mode (e.g. an ultrawide offering 1080p@120
-- alongside 3440x1440@100) it picks the lower resolution. Tune such a monitor
-- per-machine in monitors_local.lua — see setup/displays-00-*.
hl.monitor({
    output    = "",
    mode      = "highrr",  -- highest refresh rate; "preferred" can cap at 60Hz
    position  = "auto",
    scale     = "auto",
})

-- Machine-local monitor overrides, kept out of the shared repo (gitignored).
-- No-op when the file is absent.
pcall(require, "config.monitors_local")
