# DICTATION-PLAN — system-wide voice-to-text on the battlestation

Scratch analysis for the "voxtype-style dictation" idea in IDEAS.md. Goal:
a push-to-talk hotkey → speak → transcribed text spooled into whatever
window is focused (terminal, browser field, an agent's prompt), fully
local. This file is the scouting + plan; IDEAS.md keeps only the one-line
pointer.

Status: **research done, not started.** Decisions still open (see bottom).

## The machine reality (recon 2026-07-07)

What's already here vs. what a dictation stack needs:

| Need                | State on this box                                              |
|---------------------|---------------------------------------------------------------|
| Audio capture       | ✅ PipeWire (`pw-record`), Pulse-compat (`parecord`), ffmpeg   |
| Clipboard (Wayland) | ✅ `wl-copy` / `wl-paste` (wl-clipboard)                       |
| Text injection tool | ❌ none — `wtype`, `ydotool`, `dotool` all MISSING             |
| STT engine / model  | ❌ none — no whisper / whisper.cpp / vosk                      |
| GPU                 | Intel Arc B390 (Panther Lake iGPU). **No CUDA** — Vulkan/SYCL or CPU only |

Two consequences that shape everything below:

1. **We must install an injection path.** Cheapest is `wtype` (uses the
   Wayland virtual-keyboard protocol Hyprland already speaks — no daemon,
   no root, no uinput group). `ydotool` works too but needs a systemd user
   daemon + uinput permissions. Every candidate tool also has a
   **clipboard-paste fallback**, which needs only the `wl-clipboard` we
   already have — so a zero-new-typing-tool path exists if wtype is fiddly.
2. **No CUDA.** Anything leaning on `faster-whisper`/Parakeet-on-CUDA is
   CPU-only here unless it has a Vulkan backend. whisper.cpp has Vulkan
   (and SYCL) for Intel; on a modern CPU, quantized whisper/Cohere models
   also run well above realtime, so CPU-only is a viable v1.

## The landscape (all offline-capable, Wayland-native, AUR-installable)

The space moved fast in late 2025 — several Hyprland-first tools now exist.
Ranked by fit for this machine:

- **voxtype** (`peteonrails/voxtype`, Rust) — **the one Omarchy 3.3 shipped**,
  which is the tool the human saw. AUR: `voxtype` / `voxtype-bin`. Deps we
  need: PipeWire (have) + `wtype` (install). Injection fallback chain
  `wtype → dotool → ydotool → clipboard`. Push-to-talk maps *directly* to
  Hyprland press/release binds:
  ```
  bind  = SUPER, V, exec, voxtype record start
  bindr = SUPER, V, exec, voxtype record stop
  ```
  (or `voxtype record toggle`). Engines: Whisper (99 langs, default),
  Parakeet, Moonshine, SenseVoice, Dolphin, Omnilingual, etc. **Explicit
  Intel Vulkan support**: `sudo voxtype setup gpu --enable`. Models via
  `voxtype setup --download`. Claims 9–11× realtime on CPU with a quantized
  1.5 GB model. → **Best fit; recommended.**
- **hyprvoice** (`LeonardoTrapani/hyprvoice`, Go) — native Wayland/Hyprland,
  AUR `hyprvoice-bin` (installs a systemd user service). Same PTT bind/bindr
  shape, `wtype`/`ydotool`/clipboard-with-restore injection, local
  whisper.cpp (tiny → large-v3-turbo) plus many cloud providers. Clean
  PipeWire capture. Strong runner-up; heavier (background service) than
  voxtype's on-demand model.
- **hyprwhspr** (`goodroot/hyprwhspr`, Python) — AUR `hyprwhspr`. Explicitly
  mentions **Waybar/Noctalia** integration and a visualizer; `onnx-asr` for
  fast CPU, optional Intel Vulkan. ydotool-based auto-paste, `Super+Alt+D`
  default. Attractive *because* of the Noctalia tie-in, but Python + ydotool
  daemon is more moving parts than voxtype.
- Also seen: `whisrs` (Rust), `VOXD`, `vocalinux`, `MySuperWhisper`,
  `Somnius/VoxTyper` (a shell wrapper literally named "Voxtype-style"). None
  beat the top three for this box.

**Verdict:** don't build the pipeline from scratch — the "whisper + wtype +
Hypr bind" glue the IDEAS bullet imagined is exactly what voxtype already
is, and it's the tool the human already watched work under Omarchy. Adopt
**voxtype**; keep **hyprwhspr** in reserve if the Noctalia/Waybar visualizer
turns out to matter.

## Proposed integration into battlestation (voxtype path)

Fits the repo's grain — a third-party binary from the AUR, its *config*
stow-managed, its keybind in the Hypr config, its models kept out of git.

1. **Install the binary + injection tool.** `voxtype` (or `voxtype-bin`) and
   `wtype` from the AUR. These are packages, not stow-managed files — the
   parallel is any other installed tool. (Optionally teach `bsctl agents
   doctor` / `bin/doctor` to assert their presence, per the "doctor for the
   unmanaged" idea — otherwise a fresh clone silently lacks dictation.)
2. **Download a model + enable GPU.** `voxtype setup --download` (start with
   a small English whisper model), then try `sudo voxtype setup gpu --enable`
   for Intel Vulkan and benchmark against CPU. Models land in a data dir
   (`~/.local/share/...`) — **gitignored, never stowed** (big binaries; the
   same discipline as any downloaded asset).
3. **New stow package `stow/home/voxtype`** holding `~/.config/voxtype/`
   (model choice, injection method = wtype, formatting prefs). One source of
   truth, symlinked live like every other config.
4. **Keybind in `stow/home/hypr/.config/hypr/config/keybinds.lua`**, in the
   existing numbered-section style. A press/release pair for hold-to-talk
   (`bind` start / `bindr` stop). Pick a key that doesn't collide — the file
   already documents its binds; add one with a `description`. keyd stays out
   of it: PTT needs exec-on-press *and* exec-on-release, which is Hyprland's
   `bind`/`bindr`, not a keyd remap.
5. **Docs.** A `setup/dictation.md` runbook note (current-state steps only,
   per the no-history-in-runbooks rule); the *why* goes in the commit.

## Where this connects to the rest of the desk

- **Voice answers / the Deck.** Once dictation types into the focused
  window, "answer an ask by voice" is just: focus the ask's reply field,
  hold the key, talk. The IDEAS "Voice answers" bullet becomes a thin
  routing layer on top of this, not its own pipeline.
- **Kitty remote-control idea.** With `allow_remote_control` on, dictation
  aimed at a specific session's terminal (rather than merely "focused
  window") becomes possible — voice straight into a named agent's prompt.
- **Guardrail check.** Dictation is opt-in, local, and quiet — it costs no
  attention until invoked, satisfying "attention features must never cost
  attention." It does *not* route through bsctl/the stream, and shouldn't:
  it's a system input method, not part of the agent-status grammar.

## Open decisions (the human's call)

1. **Injection: `wtype` vs. clipboard-paste fallback?** wtype is the clean
   default; clipboard-paste needs nothing new but clobbers the clipboard
   unless the tool restores it (hyprvoice does; confirm voxtype's behavior).
2. **GPU (Intel Vulkan) vs. CPU-only for v1?** CPU is likely fine and
   simpler; Vulkan is a `setup gpu --enable` away if latency disappoints.
3. **Which model / size?** Small-English to start; larger if accuracy on
   accents/jargon matters more than latency.
4. **Which key, and hold-to-talk vs. toggle?** Hold-to-talk (walkie-talkie)
   is the crisp default; toggle is easier one-handed. Needs a free bind.
5. **voxtype vs. hyprwhspr?** Default to voxtype (Omarchy-proven, Rust, Intel
   Vulkan, fewer moving parts). Revisit only if the Noctalia/Waybar
   visualizer or a background-service model is wanted.

Effort: **small.** Realistically an evening — install, download a model,
one config file, one keybind, test into a few windows. No new code; it's
adoption + wiring, exactly the kind of thing the IDEAS bullet hoped.
