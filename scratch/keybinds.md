# Hyprland keybinds — design doc

Target design for the keymap. Once settled, we fold it into:
  stow/home/hypr/.config/hypr/config/keybinds.lua
Markers: ADD / CUT / CHANGE / ?(open)   ~~strike~~ = rejected.
"was: …" shows the current/live bind, so we know each change's migration cost.

---

## The model

One verb per modifier — say it in a word:

- **SUPER = GO** — change *where you're looking*. Never creates, destroys, or
  moves a window. Pure motion.
- **HYPER (Caps) = DO** — change *the world*. Launch apps; act on the focused
  window (move / resize / state / throw it elsewhere); act on the system.
- **SHIFT = "…with the window"** — a consistent *suffix*, never its own plane.
  Whatever SUPER does to your focus, +SHIFT does dragging the window along.
- **ALT / CTRL = almost nothing** — reserved for app-internal + rare exceptions.

Locked decisions:
1. SUPER is *strictly* motion — all window-state ops live on HYPER.
2. Throwing a window (HYPER+N) **stays put** by default; +SHIFT = send & follow.
3. Copy/paste/cut/undo stay on SUPER+C/V/X/Z (grandfathered habit exception).
4. Arrows = the within-workspace axis: SUPER moves *focus*, HYPER moves the
   *window*. Across-workspace = numbers only (no relative nav, no hjkl).

Mnemonic spine — the number row says it all:
  SUPER+N = go there · HYPER+N = throw it there · HYPER+SHIFT+N = throw it & go.

---

## SUPER — GO (motion only)

| Keys          | Action                          | Note          |
| ------------- | ------------------------------- | ------------- |
| SUPER 1–0     | Focus workspace N               |               |
| SUPER ←/→/↑/↓ | Focus window (within workspace) | unchanged     |
| SUPER SHIFT ↓ | Go to next empty workspace      | was: SUPER CTRL ↓ |
| SUPER Tab     | Cycle windows (MRU)             | was: ALT Tab  |
| SUPER S       | Toggle scratchpad (a peek)      |               |
| SUPER A       | Notification history (a peek)   |               |
| SUPER C/V/X/Z | Copy / paste / cut / undo       | grandfathered |
| SUPER CTRL V  | Clipboard history               |               |

## HYPER (Caps) — DO

### Launch
| Keys        | Action       | Note               |
| ----------- | ------------ | ------------------ |
| HYPER T     | Terminal     | was: SUPER Return  |
| HYPER B     | Browser      | was: SUPER W       |
| HYPER N     | File manager | was: SUPER E       |
| HYPER Space | App launcher | was: SUPER Space   |
| HYPER SHIFT E | Emoji picker | was: SUPER SHIFT E |

### Window state
| Keys         | Action                 | Note                        |
| ------------ | ---------------------- | --------------------------- |
| HYPER Q      | Close window           | was: SUPER Q                |
| HYPER Escape | Force-kill picker      | was: SUPER Escape           |
| HYPER F      | Fullscreen             | was: SUPER F                |
| HYPER D      | Maximise (keep bar)    | was: SUPER D                |
| HYPER O      | Toggle float           | was: SUPER ALT Space        |
| HYPER J      | Toggle split direction | was: SUPER J                |
| HYPER LMB    | Drag/move window       | CHANGE: was SUPER LMB       |
| HYPER RMB    | Resize window          | CHANGE: was SUPER RMB       |

### Window motion (mirrors SUPER: *me* → *it*)
| Keys            | Action                              | Note                     |
| --------------- | ----------------------------------- | ------------------------ |
| HYPER ←/→/↑/↓   | Move window within workspace        | was: SUPER SHIFT arrows  |
| HYPER 1–0       | Send window to workspace N (stay)   | was: SUPER ALT 1–0       |
| HYPER SHIFT 1–0 | Send window to workspace N & follow | was: SUPER SHIFT 1–0     |
| HYPER S         | Send window to scratchpad           | was: SUPER SHIFT S       |
| HYPER R         | Rename workspace                    | was: SUPER SHIFT R       |

### System (acting on the machine)
| Keys            | Action                 | Note                |
| --------------- | ---------------------- | ------------------- |
| HYPER Print     | Screenshot toolkit     | was: SUPER R        |
| HYPER L         | Session menu (logout…) | was: SUPER ALT C    |
| HYPER ,/.       | Display scale down/up  | was: SUPER ,/.      |
| HYPER Backspace | Restart Noctalia shell | was: SUPER Backspace|

## Left alone (third bucket — no modifier discipline)
| Keys                           | Action                  |
| ------------------------------ | ----------------------- |
| Print                          | Annotate screenshot     |
| XF86Audio Raise/Lower/Mute     | Volume up / down / mute |
| XF86AudioMicMute               | Mute microphone         |
| XF86Audio Play/Pause/Next/Prev | Media controls          |
| XF86MonBrightness Up/Down      | Brightness up / down    |

---

## Cut from the current config
- Instant lock (SUPER L) — lock via the session menu (HYPER L) instead.
- Relative workspace nav: SUPER CTRL ←/→, SUPER CTRL ALT ←/→, SUPER scroll.
- ALT Tab (replaced by SUPER Tab), btop (CTRL SHIFT Esc).
- Colour picker (SUPER P), wallpaper cycle (SUPER SHIFT W).
- Annotate active window (SUPER Print), editor launch, HYPER Return alias.

## Freed keys (parking lot for anything you want back)
SUPER: P, R, W, E, T, D, F, J, Q, Escape, Backspace, comma, period, most letters.
HYPER: Return, M, P, W(=browser is B), C, G, I, U, Y, K … plenty.
Candidates to rehome if missed: colour picker, wallpaper cycle.

## Settled — ready to fold into .lua
- Mouse drag/resize → HYPER LMB/RMB. ✓
- Rename workspace → HYPER R. ✓
- Across-workspace = numbers-only; relative nav + scroll dropped for good. ✓
- next-empty → SUPER SHIFT ↓ (GO plane). ✓

Next step: implement in keybinds.lua (input.lua already has caps:hyper).

CONFIRMED (tested live): caps:hyper → Caps fires as MOD3 (modmask 32), a clean
plane distinct from SUPER (MOD4/64). So HYPER binds use the token "MOD3" in
hl.bind strings, e.g. hl.bind("MOD3 + Q", ...). No xkb changes needed.
Test method: bound MOD3/MOD5/SUPER/CAPS to G via `hyprctl dispatch hl.bind(...)`,
pressed Caps+G, MOD3 notification fired. Cleared with `hyprctl reload`.
