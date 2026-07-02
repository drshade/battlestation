-- Software cursor rendering.
--
-- Hyprland draws the cursor on a separate hardware plane by default, which the
-- framebuffer-based Wayland screencopy (xdg-desktop-portal-hyprland -> PipeWire)
-- does not capture -- so the cursor is invisible when sharing your screen in
-- Teams, Firefox, OBS, etc. Forcing software cursors renders it into the frame
-- that gets captured, at the cost of a negligible amount of cursor latency.

hl.config({
    cursor = {
        no_hardware_cursors = true,
    },
})
