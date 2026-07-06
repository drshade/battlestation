-- Named workspaces (see windowrules.lua for the apps routed to them)
local workspaceNames = {
    [1] = "Comms",
    [9] = "BGA",
}

-- Per-workspace layout overrides (see https://wiki.hypr.land/Configuring/Workspace-Rules/)
local workspaceLayoutOpts = {
    [1] = { ["dwindle:force_split"] = 2 }, -- always split top/bottom -> vertical stack of windows
}

-- Keep workspaces 1..N alive even when empty, so they stay on the bar.
-- Hyprland destroys empty non-persistent workspaces, which makes them disappear.
for i = 1, 9 do
    hl.workspace_rule({
        workspace    = tostring(i),
        persistent   = true,
        default_name = workspaceNames[i],
        layout_opts  = workspaceLayoutOpts[i],
    })
end
