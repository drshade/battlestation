local mainMod = "SUPER"
local noctCall = "qs -c noctalia-shell ipc call "
local launchPrefix = "uwsm app -- " -- if you are not using UWSM, make this empty (e.g. "")
-- Workspace navigation by DISPLAY POSITION rather than raw Hyprland id: ws.sh
-- maps position <-> real id through a persisted order the bar plugin can reorder.
-- So "workspace N" below means the Nth pill, not necessarily Hyprland's ws N.
local ws = "$HOME/.config/hypr/scripts/ws.sh"

-- Universal copy/paste/cut: send Ctrl+Insert / Shift+Insert, honored by GUI apps
-- AND terminals (and Ctrl+Insert avoids Ctrl+C = SIGINT). down -> 50ms -> up
-- gives the synthetic press time to register.
local function send_shortcut(mods, key)
    return function()
        hl.dispatch(hl.dsp.send_key_state({ mods = mods, key = key, state = "down", window = "activewindow" }))
        hl.timer(function()
            hl.dispatch(hl.dsp.send_key_state({ mods = mods, key = key, state = "up", window = "activewindow" }))
        end, { timeout = 50, type = "oneshot" })
    end
end

-- 1. WINDOW MANAGEMENT

hl.bind(mainMod .. " + Escape",      hl.dsp.exec_cmd("hyprctl kill"),                  { description = "Force-kill a window (click to select)" })
hl.bind(mainMod .. " + Q",           hl.dsp.window.close(),                            { description = "Close active window" })
hl.bind(mainMod .. " + ALT + Space", hl.dsp.window.float({ action = "toggle" }),       { description = "Toggle floating" })
hl.bind(mainMod .. " + D",           hl.dsp.window.fullscreen({ mode = 1 }),           { description = "Maximise (keep bar)" })
hl.bind(mainMod .. " + F",           hl.dsp.window.fullscreen(),                       { description = "Fullscreen" })
hl.bind(mainMod .. " + J",           hl.dsp.layout("togglesplit"),                     { description = "Toggle split direction" })
hl.bind(mainMod .. " + L",           hl.dsp.exec_cmd(noctCall .. " lockScreen lock"),  { description = "Lock screen" })
hl.bind(mainMod .. " + ALT + C",     hl.dsp.exec_cmd(noctCall .. " sessionMenu toggle"), { description = "Session menu (logout/reboot)" })

-- Change focus
hl.bind(mainMod .. " + Left",  hl.dsp.focus({ direction = "left" }),  { description = "Focus window left" })
hl.bind(mainMod .. " + Right", hl.dsp.focus({ direction = "right" }), { description = "Focus window right" })
hl.bind(mainMod .. " + Up",    hl.dsp.focus({ direction = "up" }),    { description = "Focus window up" })
hl.bind(mainMod .. " + Down",  hl.dsp.focus({ direction = "down" }),  { description = "Focus window down" })
hl.bind("ALT + Tab",           hl.dsp.window.cycle_next(),            { description = "Cycle windows" })

-- Move active window around current workspace
hl.bind(mainMod .. " + SHIFT + Right", hl.dsp.window.move({ direction = "r" }), { description = "Move window right" })
hl.bind(mainMod .. " + SHIFT + Left",  hl.dsp.window.move({ direction = "l" }), { description = "Move window left" })
hl.bind(mainMod .. " + SHIFT + Up",    hl.dsp.window.move({ direction = "u" }), { description = "Move window up" })
hl.bind(mainMod .. " + SHIFT + Down",  hl.dsp.window.move({ direction = "d" }), { description = "Move window down" })
hl.bind(mainMod .. " + CONTROL + SHIFT + Right", hl.dsp.exec_cmd(ws .. " relative next --move"), { description = "Move window to next workspace" })
hl.bind(mainMod .. " + CONTROL + SHIFT + Left",  hl.dsp.exec_cmd(ws .. " relative prev --move"), { description = "Move window to previous workspace" })

-- Move & Resize with mouse
hl.bind(mainMod .. " + mouse:272", hl.dsp.window.drag(),   { description = "Drag window (mouse)" })
hl.bind(mainMod .. " + mouse:273", hl.dsp.window.resize(), { description = "Resize window (mouse)" })

-- 2. LAUNCHER

hl.bind(mainMod .. " + Return",     hl.dsp.exec_cmd(launchPrefix .. TERMINAL),                 { description = "Open terminal" })
hl.bind(mainMod .. " + E",          hl.dsp.exec_cmd(launchPrefix .. FILE_MANAGER),             { description = "Open file manager" })
hl.bind(mainMod .. " + T",          hl.dsp.exec_cmd(launchPrefix .. EDITOR),                   { description = "Open editor" })
hl.bind(mainMod .. " + W",          hl.dsp.exec_cmd(launchPrefix .. BROWSER),                  { description = "Open browser" })
hl.bind("CONTROL + SHIFT + Escape", hl.dsp.exec_cmd(launchPrefix .. TERMINAL .. " -e btop"),   { description = "Open system monitor (btop)" })
hl.bind(mainMod .. " + Space",      hl.dsp.exec_cmd(noctCall .. "launcher toggle"),            { description = "App launcher" })
hl.bind(mainMod .. " + SHIFT + E",  hl.dsp.exec_cmd(noctCall .. "launcher emoji"),             { description = "Emoji picker" })

-- 3. HARDWARE CONTROLS

-- Audio
hl.bind("XF86AudioRaiseVolume", hl.dsp.exec_cmd(noctCall .. "volume increase"),   { locked = true, repeating = true, description = "Volume up" })
hl.bind("XF86AudioLowerVolume", hl.dsp.exec_cmd(noctCall .. "volume decrease"),   { locked = true, repeating = true, description = "Volume down" })
hl.bind("XF86AudioMute",        hl.dsp.exec_cmd(noctCall .. "volume muteOutput"), { locked = true, repeating = true, description = "Mute output" })
hl.bind("XF86AudioMicMute",     hl.dsp.exec_cmd(noctCall .. "volume muteInput"),  { locked = true, repeating = true, description = "Mute microphone" })

-- Media
hl.bind("XF86AudioPlay",  hl.dsp.exec_cmd(noctCall .. "media playPause"), { locked = true, description = "Play/pause media" })
hl.bind("XF86AudioPause", hl.dsp.exec_cmd(noctCall .. "media playPause"), { locked = true, description = "Play/pause media" })
hl.bind("XF86AudioNext",  hl.dsp.exec_cmd(noctCall .. "media next"),      { locked = true, description = "Next track" })
hl.bind("XF86AudioPrev",  hl.dsp.exec_cmd(noctCall .. "media previous"),  { locked = true, description = "Previous track" })

-- Brightness
hl.bind("XF86MonBrightnessUp",   hl.dsp.exec_cmd(noctCall .. "brightness increase"), { repeating = true, description = "Brightness up" })
hl.bind("XF86MonBrightnessDown", hl.dsp.exec_cmd(noctCall .. "brightness decrease"), { repeating = true, description = "Brightness down" })

-- 4. UTILITIES

-- Screen Capture
hl.bind(mainMod .. " + P",     hl.dsp.exec_cmd(noctCall .. "plugin:screen-toolkit colorPicker"),     { description = "Colour picker" })
hl.bind("Print",               hl.dsp.exec_cmd(noctCall .. "plugin:screen-toolkit annotate"),        { description = "Screenshot (annotate)" })
hl.bind(mainMod .. " + Print", hl.dsp.exec_cmd(noctCall .. "plugin:screen-toolkit annotateWindow"),  { description = "Screenshot window (annotate)" })
hl.bind(mainMod .. " + R",     hl.dsp.exec_cmd(noctCall .. "plugin:screen-toolkit toggle"),          { description = "Screenshot toolkit" })

-- Display scaling
hl.bind(mainMod .. " + comma",  hl.dsp.exec_cmd("$HOME/.config/hypr/scripts/display-scale.sh down"), { description = "Zoom display out (scale down)" })
hl.bind(mainMod .. " + period", hl.dsp.exec_cmd("$HOME/.config/hypr/scripts/display-scale.sh up"),   { description = "Zoom display in (scale up)" })

-- Theming and Wallpaper
hl.bind(mainMod .. " + SHIFT + W", hl.dsp.exec_cmd(noctCall .. " wallpaper toggle"), { description = "Cycle wallpaper" })

-- Restart the Noctalia shell (detached, exactly like autostart)
hl.bind(mainMod .. " + Backspace", hl.dsp.exec_cmd("qs -c noctalia-shell kill; sleep 1; qs -c noctalia-shell"), { description = "Restart Noctalia shell" })

-- Editing: universal copy/paste/cut/undo (see send_shortcut helper) + clipboard history
hl.bind(mainMod .. " + C",           send_shortcut("CTRL", "Insert"),                  { description = "Copy" })
hl.bind(mainMod .. " + V",           send_shortcut("SHIFT", "Insert"),                 { description = "Paste" })
hl.bind(mainMod .. " + X",           send_shortcut("CTRL", "X"),                       { description = "Cut" })
hl.bind(mainMod .. " + Z",           send_shortcut("CTRL", "Z"),                       { description = "Undo" })
hl.bind(mainMod .. " + CONTROL + V", hl.dsp.exec_cmd(noctCall .. "launcher clipboard"), { description = "Clipboard history" })

-- 5. WORKSPACES

-- One loop per modifier group (not interleaved) so each group's binds register
-- contiguously and list together in the keybind cheatsheet. (The cheatsheet's
-- "merge sequential" only works for file-parsed binds, not the hyprctl fallback
-- our lua config uses, so they won't collapse into a range — just stay grouped.)
for i = 1, 10 do
    hl.bind(mainMod .. " + " .. (i % 10), hl.dsp.exec_cmd(ws .. " goto " .. i), { description = "Go to workspace " .. i })
end
for i = 1, 10 do
    hl.bind(mainMod .. " + SHIFT + " .. (i % 10), hl.dsp.exec_cmd(ws .. " movewindow " .. i .. " --follow"), { description = "Move window to workspace " .. i })
end
for i = 1, 10 do
    hl.bind(mainMod .. " + ALT + " .. (i % 10), hl.dsp.exec_cmd(ws .. " movewindow " .. i), { description = "Send window to workspace " .. i })
end

hl.bind(mainMod .. " + CONTROL + Right",       hl.dsp.exec_cmd(ws .. " relative next"),        { description = "Next workspace" })
hl.bind(mainMod .. " + CONTROL + Left",        hl.dsp.exec_cmd(ws .. " relative prev"),        { description = "Previous workspace" })
hl.bind(mainMod .. " + CONTROL + Down",        hl.dsp.focus({ workspace = "empty" }),          { description = "Go to next empty workspace" })
hl.bind(mainMod .. " + CONTROL + ALT + Right", hl.dsp.exec_cmd(ws .. " relative next --move"), { description = "Move window to next workspace" })
hl.bind(mainMod .. " + CONTROL + ALT + Left",  hl.dsp.exec_cmd(ws .. " relative prev --move"), { description = "Move window to previous workspace" })

-- Scroll through workspaces in display order
hl.bind(mainMod .. " + mouse_down", hl.dsp.exec_cmd(ws .. " relative next"), { description = "Scroll to next workspace" })
hl.bind(mainMod .. " + mouse_up",   hl.dsp.exec_cmd(ws .. " relative prev"), { description = "Scroll to previous workspace" })

-- Special workspace (scratchpad)
hl.bind(mainMod .. " + SHIFT + S", hl.dsp.window.move({ workspace = "special" }),               { description = "Move window to scratchpad" })
hl.bind(mainMod .. " + S",         hl.dsp.workspace.toggle_special(),                           { description = "Toggle scratchpad" })
hl.bind(mainMod .. " + SHIFT + R", hl.dsp.exec_cmd("$HOME/.config/hypr/scripts/rename-workspace.sh"), { description = "Rename workspace" })

-- 6. NOTIFICATIONS

hl.bind(mainMod .. " + A", hl.dsp.exec_cmd(noctCall .. "notifications toggleHistory"), { description = "Notification history" })
