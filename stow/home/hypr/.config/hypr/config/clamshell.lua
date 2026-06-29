-- Clamshell handling: keep the internal laptop panel out of the layout while the
-- lid is shut, so its workspaces and pointer area don't strand on a screen you
-- can't see. The panel is auto-detected by connector class (see clamshell.sh),
-- so there is no hardcoded output name -- this works on any laptop and is inert
-- on desktops (no internal panel, and no Lid Switch to fire these binds).
-- Switch binds: https://wiki.hypr.land/Configuring/Basics/Binds/

local clamshell = "$HOME/.config/hypr/scripts/clamshell.sh"

-- React to the lid while the session runs. `locked = true` lets these fire on
-- the lock screen too (you may shut the lid while locked).
hl.bind("switch:on:Lid Switch",  hl.dsp.exec_cmd(clamshell .. " on"),  { locked = true })
hl.bind("switch:off:Lid Switch", hl.dsp.exec_cmd(clamshell .. " off"), { locked = true })

-- Logging in with the lid already shut emits no switch event (lid state is a
-- level, not an edge), so reconcile once at startup.
hl.on("hyprland.start", function()
    hl.exec_cmd(clamshell .. " auto")
end)
