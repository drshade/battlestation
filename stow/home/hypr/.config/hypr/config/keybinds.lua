-- Keybind model: one verb per modifier.
--   SUPER = GO  — change where you're looking; never creates/moves a window.
--   HYPER = DO  — launch apps, act on the focused window, act on the system.
--   SHIFT       — "…with the window" suffix (e.g. send-and-follow).
--   ALT / CTRL  — almost nothing; reserved for app-internal + rare exceptions.
-- The number row is the whole story:
--   SUPER+N go there · HYPER+N throw it there · HYPER+SHIFT+N throw it & go.
--
-- Section headers below MUST stay in the "-- N. Name" form: the noctalia
-- keybind-cheatsheet parser uses them to categorise binds (it matches each
-- bind's description, or its literal prefix before "..", to the nearest header).
-- Full design rationale: scratch/keybinds.md
local mainMod  = "SUPER"
local hyperMod = "MOD3" -- caps:hyper makes Caps fire as MOD3 (verified live); see input.lua
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

-- 1. Navigate · Super

-- Focus workspace by display position (one loop so they list contiguously in
-- the keybind cheatsheet).
for i = 1, 10 do
    hl.bind(mainMod .. " + " .. (i % 10), hl.dsp.exec_cmd(ws .. " goto " .. i), { description = "Focus workspace " .. i })
end

-- Focus window within the current workspace
hl.bind(mainMod .. " + Left",  hl.dsp.focus({ direction = "left" }),  { description = "Focus window left" })
hl.bind(mainMod .. " + Right", hl.dsp.focus({ direction = "right" }), { description = "Focus window right" })
hl.bind(mainMod .. " + Up",    hl.dsp.focus({ direction = "up" }),    { description = "Focus window up" })
hl.bind(mainMod .. " + Down",  hl.dsp.focus({ direction = "down" }),  { description = "Focus window down" })

hl.bind(mainMod .. " + SHIFT + Down", hl.dsp.focus({ workspace = "empty" }), { description = "Go to next empty workspace" })
hl.bind(mainMod .. " + Tab",          hl.dsp.window.cycle_next(),            { description = "Cycle windows" })
hl.bind(mainMod .. " + S",            hl.dsp.workspace.toggle_special(),     { description = "Toggle scratchpad" })
hl.bind(mainMod .. " + A",            hl.dsp.exec_cmd(noctCall .. "notifications toggleHistory"), { description = "Notification history" })

-- 2. Launch · Hyper

-- Opens in the focused window's cwd (term-here.sh walks to the foreground
-- process and cds there); falls back to $HOME for non-terminal windows.
hl.bind(hyperMod .. " + T",         hl.dsp.exec_cmd(launchPrefix .. "$HOME/.config/hypr/scripts/term-here.sh " .. TERMINAL), { description = "Open terminal" })
hl.bind(hyperMod .. " + B",         hl.dsp.exec_cmd(launchPrefix .. BROWSER),      { description = "Open browser" })
hl.bind(hyperMod .. " + N",         hl.dsp.exec_cmd(launchPrefix .. FILE_MANAGER), { description = "Open file manager" })
-- Opens VS Code at the focused terminal's cwd (code-here.sh via focused-cwd.sh);
-- opens with no folder when the focused window isn't a terminal.
hl.bind(hyperMod .. " + E",         hl.dsp.exec_cmd(launchPrefix .. "$HOME/.config/hypr/scripts/code-here.sh"), { description = "Open VS Code" })
hl.bind(hyperMod .. " + Space",     hl.dsp.exec_cmd(noctCall .. "launcher toggle"), { description = "App launcher" })
hl.bind(hyperMod .. " + SHIFT + E", hl.dsp.exec_cmd(noctCall .. "launcher emoji"),  { description = "Emoji picker" })

-- 3. Window · Hyper

hl.bind(hyperMod .. " + Q",      hl.dsp.window.close(),                      { description = "Close active window" })
hl.bind(hyperMod .. " + Escape", hl.dsp.exec_cmd("hyprctl kill"),            { description = "Force-kill a window (click to select)" })
hl.bind(hyperMod .. " + F",      hl.dsp.window.fullscreen(),                 { description = "Fullscreen" })
hl.bind(hyperMod .. " + D",      hl.dsp.window.fullscreen({ mode = 1 }),     { description = "Maximise (keep bar)" })
hl.bind(hyperMod .. " + O",      hl.dsp.window.float({ action = "toggle" }), { description = "Toggle floating" })
hl.bind(hyperMod .. " + J",      hl.dsp.layout("togglesplit"),               { description = "Toggle split direction" })
hl.bind(hyperMod .. " + M",      hl.dsp.exec_cmd("$HOME/.config/hypr/scripts/layout-toggle.sh"), { description = "Toggle layout mode (dwindle / scrolling)" })
hl.bind(hyperMod .. " + mouse:272", hl.dsp.window.drag(),   { description = "Drag window (mouse)" })
hl.bind(hyperMod .. " + mouse:273", hl.dsp.window.resize(), { description = "Resize window (mouse)" })

-- Move the window within the workspace (mirrors SUPER focus arrows)
hl.bind(hyperMod .. " + Left",  hl.dsp.window.move({ direction = "l" }), { description = "Move window left" })
hl.bind(hyperMod .. " + Right", hl.dsp.window.move({ direction = "r" }), { description = "Move window right" })
hl.bind(hyperMod .. " + Up",    hl.dsp.window.move({ direction = "u" }), { description = "Move window up" })
hl.bind(hyperMod .. " + Down",  hl.dsp.window.move({ direction = "d" }), { description = "Move window down" })

-- 4. Workspace · Hyper

-- Throw the active window to a workspace by display position.
-- Plain = send & stay; SHIFT = send & follow. (One loop per group so each lists
-- contiguously in the cheatsheet.)
for i = 1, 10 do
    hl.bind(hyperMod .. " + " .. (i % 10), hl.dsp.exec_cmd(ws .. " movewindow " .. i), { description = "Send window to workspace " .. i })
end
for i = 1, 10 do
    hl.bind(hyperMod .. " + SHIFT + " .. (i % 10), hl.dsp.exec_cmd(ws .. " movewindow " .. i .. " --follow"), { description = "Send window to workspace " .. i .. " & follow" })
end

hl.bind(hyperMod .. " + S", hl.dsp.window.move({ workspace = "special" }),                  { description = "Send window to scratchpad" })
hl.bind(hyperMod .. " + R", hl.dsp.exec_cmd(noctCall .. "plugin:claude-workspaces rename"), { description = "Rename workspace" })

-- 5. System · Hyper

hl.bind(hyperMod .. " + Print",     hl.dsp.exec_cmd(noctCall .. "plugin:screen-toolkit toggle"),       { description = "Screenshot toolkit" })
hl.bind(hyperMod .. " + L",         hl.dsp.exec_cmd(noctCall .. "sessionMenu toggle"),                 { description = "Session menu (lock/logout/reboot)" })
hl.bind(hyperMod .. " + comma",     hl.dsp.exec_cmd("$HOME/.config/hypr/scripts/display-scale.sh down"), { description = "Zoom display out (scale down)" })
hl.bind(hyperMod .. " + period",    hl.dsp.exec_cmd("$HOME/.config/hypr/scripts/display-scale.sh up"),   { description = "Zoom display in (scale up)" })
hl.bind(hyperMod .. " + Backspace", hl.dsp.exec_cmd("qs -c noctalia-shell kill; sleep 1; qs -c noctalia-shell"), { description = "Restart Noctalia shell" })

-- 6. Edit · Super

-- Universal copy/paste/cut/undo (grandfathered onto SUPER) + clipboard history
hl.bind(mainMod .. " + C",           send_shortcut("CTRL", "Insert"),                   { description = "Copy" })
hl.bind(mainMod .. " + V",           send_shortcut("SHIFT", "Insert"),                  { description = "Paste" })
hl.bind(mainMod .. " + X",           send_shortcut("CTRL", "X"),                        { description = "Cut" })
hl.bind(mainMod .. " + Z",           send_shortcut("CTRL", "Z"),                        { description = "Undo" })
hl.bind(mainMod .. " + CONTROL + V", hl.dsp.exec_cmd(noctCall .. "launcher clipboard"), { description = "Clipboard history" })

-- 7. Hardware

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

-- Screenshot (quick): annotate full screen on the Print key
hl.bind("Print", hl.dsp.exec_cmd(noctCall .. "plugin:screen-toolkit annotate"), { description = "Screenshot (annotate)" })
