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
        touchpad = {
            -- macOS-style clickfinger: physical click with 1 finger = left,
            -- 2 fingers = right, 3 fingers = middle (replaces corner zones).
            clickfinger_behavior = true,
            -- Only a physical press clicks; a light tap fires too easily
            -- mid-typing or while resting a palm.
            tap_to_click = false,
        },
    },
})

-- The touchpad gets mild acceleration and a little extra speed; mice stay on
-- the flat profile above. A name with no matching device is ignored.
hl.device({
    name = "ven_2c2f:00-2c2f:0034-touchpad",
    accel_profile = "adaptive",
    sensitivity = 0.2,
})

hl.gesture({ fingers = 4, direction = "horizontal", action = "workspace" })
hl.gesture({ fingers = 3, direction = "down",       action = "close" })
hl.gesture({ fingers = 3, direction = "up",         action = "fullscreen" })
hl.gesture({ fingers = 3, direction = "left",       action = "float" })
