-- Look and feel configuration

hl.config({
    general = {
        gaps_in = 1,
        gaps_out = 1,
        border_size = 1,
        extend_border_grab_area = 10,
        resize_on_border = false,
        col = {
            active_border = {
                colors = { CACHYLGREEN, CACHYDGREEN },
                angle = 45,
            },
            inactive_border = CACHYGRAY,
        },
    },
    group = {
        -- Tabs are created ONLY by a HYPER+SHIFT drop (keybinds.lua, "Tabs"):
        -- a plain drop never merges into a stack (drag_into_group = 0) and a
        -- newly opened window never auto-joins the focused stack (auto_group).
        drag_into_group = 0,
        auto_group = false,
        col = {
            border_active = CACHYLBLUE,
            border_inactive = CACHYGRAY,
            border_locked_active = CACHYDBLUE,
            border_locked_inactive = CACHYGRAY,
        },
        groupbar = {
            -- Defaults (8px text in a 14px bar) are unreadable on the 34" at
            -- scale 1; sized to match the bar/UI font. Logical px, so scale-aware.
            font_family = "DejaVu LGC Sans",
            font_size = 12,
            font_weight_active = "bold",
            height = 24,
            indicator_height = 3,
            gaps_in = 4,
            -- Filled, blurred tabs: without `gradients` a tab is just its title
            -- drawn straight over the (translucent) window, which is unreadable.
            -- Dark fills so the white title has contrast; the active tab is the
            -- teal one, inactive tabs recede into the CachyOS dark blue.
            -- Hide the bar the instant a stack is down to one window; the reaper in
            -- keybinds.lua ("Tabs") dissolves that leftover group within a second.
            disable_when_only = true,
            gradients = true,
            blur = true,
            text_color = CACHYWHITE,
            text_color_inactive = CACHYGREY,
            col = {
                active = "rgba(007d6fee)",   -- CACHYDGREEN, slightly translucent
                inactive = "rgba(111826ee)", -- CACHYDBLUE
                locked_active = CACHYMBLUE,
                locked_inactive = "rgba(111826ee)",
            },
        },
    },
    decoration = {
        dim_special = 0.3,
        rounding = 5,
        active_opacity = 0.95,
        inactive_opacity = 0.85,
        fullscreen_opacity = 1,
        blur = {
            size = 5,
            passes = 4,
            special = true,
        },
    },
})
