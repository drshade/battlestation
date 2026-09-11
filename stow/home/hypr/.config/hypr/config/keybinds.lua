-- Keybind model: one verb per modifier.
--   SUPER = GO  — change where you're looking; never creates/moves a window.
--   HYPER = DO  — launch apps, act on the focused window, act on the system.
--   SHIFT       — "…with the window" suffix (e.g. send-and-follow).
--   ALT / CTRL  — almost nothing; reserved for app-internal + rare exceptions.
-- The number row is the whole story for WORKSPACES:
--   SUPER+N go there · HYPER+N throw the window there & follow ·
--   HYPER+SHIFT+N throw it & stay.
-- The F-row is the same story one level up, for DISPLAYS:
--   SUPER+Fn look at display n · HYPER+Fn take this workspace there & follow
--   · HYPER+SHIFT+Fn send it & stay.
--
-- Section headers below MUST stay in the "-- N. Name" form: the noctalia
-- keybind-cheatsheet parser uses them to categorise binds (it matches each
-- bind's description, or its literal prefix before "..", to the nearest header).
-- (Design rationale lives in git history: docs/keybinds.md, retired once implemented.)
local mainMod  = "SUPER"
local hyperMod = "MOD3" -- caps:hyper makes Caps fire as MOD3 (verified live); see input.lua
local noctCall = "qs -c noctalia-shell ipc call "
local launchPrefix = "uwsm app -- " -- if you are not using UWSM, make this empty (e.g. "")
-- Workspace navigation by BATTLESPACE id (bs-id) rather than raw Hyprland
-- ws-id: bsctl maps bs-id <-> ws-id through a persisted map the bar plugin
-- can reorder. So "workspace N" below means the Nth pill (bs-id N), not
-- necessarily Hyprland's ws N.
local ws = "$HOME/.local/bin/bsctl ws"

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

-- Focus workspace by battlespace id (one loop so they list contiguously in
-- the keybind cheatsheet).
for i = 1, 10 do
    hl.bind(mainMod .. " + " .. (i % 10), hl.dsp.exec_cmd(ws .. " focus --bs-id " .. i), { description = "Focus workspace " .. i })
end

-- Focus window within the current workspace
hl.bind(mainMod .. " + Left",  hl.dsp.focus({ direction = "left" }),  { description = "Focus window left" })
hl.bind(mainMod .. " + Right", hl.dsp.focus({ direction = "right" }), { description = "Focus window right" })
hl.bind(mainMod .. " + Up",    hl.dsp.focus({ direction = "up" }),    { description = "Focus window up" })
hl.bind(mainMod .. " + Down",  hl.dsp.focus({ direction = "down" }),  { description = "Focus window down" })

hl.bind(mainMod .. " + SHIFT + Down", hl.dsp.focus({ workspace = "empty" }), { description = "Go to next empty workspace" })
hl.bind(mainMod .. " + Tab",          hl.dsp.window.cycle_next(),            { description = "Cycle windows" })
hl.bind(mainMod .. " + S",            hl.dsp.workspace.toggle_special(),     { description = "Toggle scratchpad" })
hl.bind(mainMod .. " + A",            hl.dsp.exec_cmd(noctCall .. "plugin:battlestation-workspaces asks"), { description = "The Deck (agent asks queue)" })
hl.bind(mainMod .. " + N",            hl.dsp.exec_cmd(noctCall .. "notifications toggleHistory"), { description = "Notification history" })

-- 2. Launch · Hyper

-- Opens in the focused window's cwd (term-here.sh walks to the foreground
-- process and cds there); falls back to $HOME for non-terminal windows.
hl.bind(hyperMod .. " + T",         hl.dsp.exec_cmd(launchPrefix .. "$HOME/.config/hypr/scripts/term-here.sh " .. TERMINAL), { description = "Open terminal" })
hl.bind(hyperMod .. " + B",         hl.dsp.exec_cmd(launchPrefix .. BROWSER),      { description = "Open browser" })
-- Opens the file manager at the focused terminal's cwd (dolphin-here.sh via
-- focused-cwd.sh); opens at its default location otherwise.
hl.bind(hyperMod .. " + N",         hl.dsp.exec_cmd(launchPrefix .. "$HOME/.config/hypr/scripts/dolphin-here.sh"), { description = "Open file manager" })
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

-- Throw the active window to a workspace by battlespace id.
-- Plain = send & follow; SHIFT = send & stay. (One loop per group so each lists
-- contiguously in the cheatsheet.)
for i = 1, 10 do
    hl.bind(hyperMod .. " + " .. (i % 10), hl.dsp.exec_cmd(ws .. " send window --bs-id " .. i .. " --focus"), { description = "Send window to workspace " .. i .. " & follow" })
end
for i = 1, 10 do
    hl.bind(hyperMod .. " + SHIFT + " .. (i % 10), hl.dsp.exec_cmd(ws .. " send window --bs-id " .. i), { description = "Send window to workspace " .. i })
end

hl.bind(hyperMod .. " + S", hl.dsp.window.move({ workspace = "special" }),                  { description = "Send window to scratchpad" })
hl.bind(hyperMod .. " + R", hl.dsp.exec_cmd(noctCall .. "plugin:battlestation-workspaces rename"), { description = "Rename workspace" })
-- Clear any manual pill reordering: the battlespace map falls back to
-- identity (bs-id N = the Nth live ws-id, ascending).
hl.bind(hyperMod .. " + SHIFT + Backspace", hl.dsp.exec_cmd(ws .. " map reset"), { description = "Reset workspace order to default" })

-- 5. Displays

-- Display id n = the nth enabled output LEFT TO RIGHT (bsctl numbers displays
-- by x-position, not connector name), mirroring the number-row verb grammar.
for i = 1, 3 do
    hl.bind(mainMod .. " + F" .. i, hl.dsp.exec_cmd(ws .. " focus --display-id " .. i), { description = "Focus display " .. i })
end
for i = 1, 3 do
    hl.bind(hyperMod .. " + F" .. i, hl.dsp.exec_cmd(ws .. " send workspace --display-id " .. i .. " --focus"), { description = "Send workspace to display " .. i .. " & follow" })
end
for i = 1, 3 do
    hl.bind(hyperMod .. " + SHIFT + F" .. i, hl.dsp.exec_cmd(ws .. " send workspace --display-id " .. i), { description = "Send workspace to display " .. i })
end

-- 6. System · Hyper

hl.bind(hyperMod .. " + Print",     hl.dsp.exec_cmd(noctCall .. "plugin:screen-toolkit toggle"),       { description = "Screenshot toolkit" })
hl.bind(hyperMod .. " + L",         hl.dsp.exec_cmd(noctCall .. "sessionMenu toggle"),                 { description = "Session menu (lock/logout/reboot)" })
hl.bind(hyperMod .. " + comma",     hl.dsp.exec_cmd("$HOME/.local/bin/bsctl display set scale --down"), { description = "Zoom display out (scale down)" })
hl.bind(hyperMod .. " + period",    hl.dsp.exec_cmd("$HOME/.local/bin/bsctl display set scale --up"),   { description = "Zoom display in (scale up)" })
hl.bind(hyperMod .. " + Backspace", hl.dsp.exec_cmd("qs -c noctalia-shell kill; sleep 1; qs -c noctalia-shell"), { description = "Restart Noctalia shell" })

-- 7. Edit · Super

-- Universal copy/paste/cut/undo (grandfathered onto SUPER) + clipboard history
hl.bind(mainMod .. " + C",           send_shortcut("CTRL", "Insert"),                   { description = "Copy" })
hl.bind(mainMod .. " + V",           send_shortcut("SHIFT", "Insert"),                  { description = "Paste" })
hl.bind(mainMod .. " + X",           send_shortcut("CTRL", "X"),                        { description = "Cut" })
hl.bind(mainMod .. " + Z",           send_shortcut("CTRL", "Z"),                        { description = "Undo" })
hl.bind(mainMod .. " + CONTROL + V", hl.dsp.exec_cmd(noctCall .. "launcher clipboard"), { description = "Clipboard history" })

-- Dictation: tap to start recording, tap again to transcribe and type into
-- the focused window (the Dictation Noctalia plugin owns the pw-record →
-- whisper-cli → wtype pipeline). A bare, modifier-LESS key on purpose: text
-- typed while a physical modifier is held combines with it in the compositor
-- (digits became SUPER+N workspace jumps under the original Super+\ hold
-- bind), and modifier-release binds proved undetectable here — so the fix is
-- structural: with a tap-toggle on a plain key, nothing is held at injection
-- time. (Supersedes Deck ask #22's Super+\.)
hl.bind("F12", hl.dsp.exec_cmd(noctCall .. "plugin:battlestation-dictation toggle"), { description = "Dictate (tap: start / stop)" })

-- 9. Tabs · Hyper

-- Tab groups ("groups" in Hyprland): a stack of tiled windows in one tile
-- with a tab bar. The whole model is the mouse drag, no keyboard verbs:
--   HYPER + drag body       -> move the tile (a stack moves as one)   [native]
--   HYPER + drag a tab      -> pull that window out into its own tile [native]
--   HYPER + SHIFT + drag    -> drop onto a tile: tab them together, or join
--                              the stack that is already there        [below]
-- Hyprland has no drop-creates-group and its drop-merge is not modifier
-- aware, so the SHIFT drop is done here. decorations.lua turns off
-- drag_into_group and auto_group so a plain drop and a freshly opened
-- window never form tabs by accident.
--
-- Why ONE function bind that owns the drag, and a sampling timer:
--  * Hyprland ends an active drag at the very start of a mouse-button event,
--    BEFORE it runs any bind for that release (ensureMouseBindState). So no
--    release bind can hit-test the tile under the pointer: by then the drop
--    has happened and dwindle has put the dropped window AT the cursor. The
--    only reliable way to know what was underneath is to watch during the
--    drag and use the last tile seen.
--  * A function bind that dispatches window.drag() is marked releasePending
--    by Hyprland, so it is called again on release — press starts the drag
--    and the sampler, release stops the sampler and groups. Press/release are
--    told apart by the in-flight state (function binds get no arguments).

-- The tiled, visible window under the cursor on the monitor under the
-- cursor, ignoring `except`. Non-current stack members report visible=false
-- (NOT hidden=true, verified live) and share the stack's geometry, so they
-- must be skipped or the hit is ambiguous. A window being dragged is floating
-- for the duration, so it is skipped too.
local function tile_under_cursor(except)
    local p = hl.get_cursor_pos()
    local mon = hl.get_monitor_at_cursor()
    if not p or not mon then return nil end
    local ws = mon.active_special_workspace or mon.active_workspace
    if not ws then return nil end
    for _, w in ipairs(hl.get_workspace_windows(ws)) do
        if (not except or w.address ~= except.address) and not w.floating and w.visible
            and p.x >= w.at.x and p.x < w.at.x + w.size.x
            and p.y >= w.at.y and p.y < w.at.y + w.size.y then
            return w
        end
    end
    return nil
end

local tab_drag = { window = nil, target = nil } -- in flight while `window` is set
local tab_sampler = hl.timer(function()
    if not tab_drag.window then return end
    tab_drag.target = tile_under_cursor(tab_drag.window) -- nil over empty space / self
end, { timeout = 40, type = "repeat" })
tab_sampler:set_enabled(false)

hl.bind(hyperMod .. " + SHIFT + mouse:272", function()
    if not tab_drag.window then
        -- PRESS: start the native drag (focuses the grabbed window), then follow the pointer.
        hl.dispatch(hl.dsp.window.drag())
        tab_drag.window = hl.get_active_window()
        tab_drag.target = nil
        tab_sampler:set_enabled(true)
        return
    end
    -- RELEASE: the drop already happened; pair with the last tile seen under the pointer.
    tab_sampler:set_enabled(false)
    local dropped, target = tab_drag.window, tab_drag.target
    tab_drag.window, tab_drag.target = nil, nil
    hl.dispatch(hl.dsp.window.drag()) -- release half of the drag dispatcher (no-op if already ended)
    if not dropped or not target or target.address == dropped.address then return end
    hl.timer(function()
        if not target.group then
            hl.dispatch(hl.dsp.group.toggle({ window = "address:" .. target.address }))
        end
        if target.group and not target.group.locked then
            target.group:add(dropped)
        end
    end, { timeout = 30, type = "oneshot" })
end, { description = "Drag window onto a tile to tab them; onto a stack to join it (mouse)" })

-- Reaper: a stack whose last companion was pulled out is still a group of
-- one (blue border, and a tab bar until disable_when_only hides it). Nothing
-- in the Lua event set announces group membership changes, so poll: once a
-- second, dissolve any single-member group. Cheap (a handful of windows) and
-- nothing here ever creates a 1-window group on purpose -- the drop handler
-- toggles and adds within one tick.
hl.timer(function()
    for _, w in ipairs(hl.get_windows()) do
        if w.group and w.group.size == 1 then
            hl.dispatch(hl.dsp.group.toggle({ window = "address:" .. w.address }))
        end
    end
end, { timeout = 1000, type = "repeat" })

-- 8. Hardware

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
