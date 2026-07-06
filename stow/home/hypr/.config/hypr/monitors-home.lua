-- Home profile: eDP-1 (laptop) + DP-2 (Dell 1080p) + HDMI-A-2 (CMT 1440p)
-- Layout: [Dell 1920x1080] [eDP-1 scaled] [CMT 2560x1440]
-- Regenerate by running nwg-displays and copying monitors.lua here.

hl.monitor({
    output = "eDP-1",
    mode = "2560x1440@165.0",
    position = "1920x0",
    scale = 1.6
})
hl.monitor({
    output = "DP-2",
    mode = "1920x1080@60.0",
    position = "0x0",
    scale = 1.0
})
hl.monitor({
    output = "HDMI-A-2",
    mode = "2560x1440@99.95",
    position = "3520x0",
    scale = 1.0
})
