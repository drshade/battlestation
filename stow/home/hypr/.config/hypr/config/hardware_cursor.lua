-- Cursor rendering.
--
-- Hyprland draws the cursor on a separate hardware plane by default. This keeps
-- cursor motion decoupled from the frame pipeline, so it stays smooth even when
-- the compositor is under load -- but the framebuffer-based Wayland screencopy
-- (xdg-desktop-portal-hyprland -> PipeWire) does NOT capture the hardware plane,
-- so the cursor is invisible in screen shares (Teams, Firefox, OBS, etc.).
--
-- false = full hardware cursor (smoothest; invisible in captures).
-- 2     = auto: hardware normally, software only while a capture client runs.
-- true  = always software (visible in captures; can stutter under load).

hl.config({
    cursor = {
        no_hardware_cursors = true,
    },
})
