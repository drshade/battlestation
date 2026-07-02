-- Input configuration

hl.config({
    input = {
        accel_profile = "flat",
        kb_options = "caps:hyper",
        -- Key repeat: Hyprland's defaults (25/600) feel sluggish. rate =
        -- repeats per second once repeating; delay = ms a key is held before
        -- repeating starts. Tune to taste: rate 30-60, delay 200-350 are the
        -- sane bounds; raise delay if you get accidental doubled keystrokes.
        repeat_rate = 60,
        repeat_delay = 220,
    },
})

hl.gesture({ fingers = 4, direction = "horizontal", action = "workspace" })
hl.gesture({ fingers = 3, direction = "down",       action = "close" })
hl.gesture({ fingers = 3, direction = "up",         action = "fullscreen" })
hl.gesture({ fingers = 3, direction = "left",       action = "float" })
