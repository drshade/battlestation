# DICTATION-PLAN — push-to-talk dictation via a Noctalia plugin

Goal: hold a key → speak → transcribed text typed into whatever window is
focused, fully local. Refined from the earlier "adopt voxtype" draft: the
orchestrator is now a **new Noctalia plugin** — dictation is a totally
isolated concern, and the widget solves the two things a bare keybind
can't: **visual status** (recording / transcribing / error at a glance)
and **audio device selection** (a settings surface for picking the mic).

Status: **design settled, not started.** The focus-race (window focus
changing between key-release and injection) is accepted, mitigated by the
widget's busy state rather than engineered away.

## The machine reality (recon 2026-07-07)

| Need                | State on this box                                            |
|---------------------|--------------------------------------------------------------|
| Audio capture       | ✅ PipeWire (`pw-record`), Pulse-compat, ffmpeg              |
| Clipboard (Wayland) | ✅ `wl-copy` / `wl-paste`                                    |
| Text injection      | ❌ none — `wtype` / `ydotool` / `dotool` all missing         |
| STT engine / model  | ❌ none — no whisper.cpp, no models                          |
| Microphone          | ⚠️ **UNVERIFIED** — recon never checked a working mic exists |
| GPU                 | Intel Arc B390 (Panther Lake). No CUDA — Vulkan/SYCL or CPU  |

**Step 0 of implementation: the mic check.** `wpctl status` to list
sources, a 5-second `pw-record` + playback. Mic quality drives whisper
accuracy more than model size; nothing else matters if this fails.

## Why a plugin instead of adopting voxtype

The earlier draft picked **voxtype** (the Omarchy 3.3 tool) as the whole
solution. The refinement: what we actually want — live status in the bar,
device picking, a place for future polish — needs the orchestrator to
**own the pipeline**, not shell out to a black box. Wrapping voxtype would
leave the plugin guessing at state ("is it recording? transcribing?");
driving the primitives directly makes every transition observable because
the plugin *is* the thing running them. And the pipeline is genuinely
small: record a wav, run whisper on it, type the result.

The landscape research stands as reference (voxtype, hyprvoice, hyprwhspr
— see git history of this file for the full survey). voxtype remains the
**escape hatch**: if our glue disappoints, it's the proven packaged
alternative, and its engine variety (Parakeet, Moonshine, …) is there if
plain whisper.cpp underwhelms. Everything web-sourced carries the usual
caveat: **verify flags against the installed binaries, not the README.**

## Architecture: the `dictation` plugin

A sibling of `battlestation-workspaces`, following its exact conventions
(manifest entryPoints, pluginSettings persistence, direct-argv Processes —
no shell anywhere, per the repo rule).

```
stow/home/noctalia/.config/noctalia/plugins/dictation/
  manifest.json     entryPoints: main, barWidget, settings
  Main.qml          state machine + IPC functions + the pipeline Processes
  BarWidget.qml     the status glyph (the whole reason this is a plugin)
  Settings.qml      mic picker, model choice, max-duration, injection prefs
```

**State machine** (owned by Main.qml, rendered by BarWidget.qml):

```
idle → recording → transcribing → injecting → idle
                 ↘ error (glyph + toast) → idle
```

- *idle*: dim mic glyph (or hidden — settings choice).
- *recording*: unmissable — red/pulsing. Also the stuck-mic tell.
- *transcribing/injecting*: busy spinner. This is the focus-race
  mitigation: while the glyph is busy, you know text is still in flight.
- *error*: glyph flash + one toast (whisper failed, wtype missing, no mic).
  Errors surface loud, never swallowed.

**IPC surface** — same grammar as the asks panel's Super+A bind:

```
qs -c noctalia-shell ipc call plugin:dictation start   # key press
qs -c noctalia-shell ipc call plugin:dictation stop    # key release
qs -c noctalia-shell ipc call plugin:dictation cancel  # bail, discard audio
```

**The pipeline** (three Processes, run by Main.qml):

1. `pw-record --target <chosen-source> /run/user/.../dictation.wav`
   — started on `start`, killed on `stop`.
2. `whisper-cli -m <model> -f dictation.wav --no-timestamps …` → stdout.
3. `wtype -` with the text on stdin (stdin avoids argv quoting entirely).

**Stuck-mic backstop:** a max-duration timer (default ~60s, a setting)
auto-stops recording. This covers the known Hyprland `bindr` gotcha —
release SUPER before V and the release bind may not fire; without a cap
that's an open mic forever.

**Device selection:** Settings.qml lists PipeWire sources (parse
`wpctl status` / `pw-dump` via Process; check whether Noctalia's own audio
service already exposes sources before hand-rolling). Chosen node persists
in pluginSettings; default = system default source.

## What gets installed (the non-plugin half)

- **`wtype`** (AUR) — the injector. Note honestly: installing it grants
  *every* process in the session the virtual-keyboard capability, not just
  the plugin. On a desk full of agents with shell access that's a real
  posture change — accepted deliberately (it's the same power the kitty
  remote-control idea wants), recorded here so it's a decision, not a drift.
- **`whisper.cpp`** (AUR, provides `whisper-cli`) — CPU first; Intel
  Vulkan build only if latency disappoints.
- **A model** — start `small.en`; ggml files live in a data dir,
  **gitignored, never stowed**.
- Keybind in `keybinds.lua` (numbered-section style, with description):
  `bind` SUPER+backslash → `start`, `bindr` SUPER+backslash → `stop`.
  (Super+V was the first proposal — it's taken by paste; Super+\ chosen
  via Deck ask #22.) Install binaries *before* the keybind lands — the
  symlinked config goes live instantly.
- `setup/dictation.md` runbook note (current steps only; why → commit msg).
- Optionally: `bin/doctor` asserts `wtype`/`whisper-cli`/model present.

## Verification checklist (the acceptance test, not an afterthought)

- Mic sanity: record + play back 5s, correct device selected via settings.
- Dictate into: a kitty prompt, a browser text field, an agent's prompt.
- The jargon test: say `bsctl`, `Noctalia`, `Hyprland`, `Quickshell`,
  client/project names — small.en will mangle some; decide if it's livable
  or bump the model.
- Unicode / punctuation survives wtype.
- Hold SUPER+\, release SUPER first, then \ — confirm the backstop catches
  the stuck recording and the glyph shows it.
- Switch windows mid-transcription — observe where text lands; confirm the
  busy glyph made it predictable (accepted behaviour, not a bug).
- Kill whisper mid-run / remove the model — error state renders, toast
  fires, plugin returns to idle.

## Open decisions (genuinely the human's)

1. **Key + mode** — DECIDED (Deck #22): **Super+\ hold-to-talk**. Super+V
   was rejected — it's paste. Toggle stays a settings flag; hold default.
2. **Idle glyph visible or hidden?** Cosmetic, decide at first render.

Everything else (model size, CPU vs Vulkan, wtype vs clipboard fallback)
is try-the-default-and-escalate, not a decision to park on.

Effort: **a weekend-ish.** The pipeline is small but BarWidget/Settings/
state-machine QML is real code — more than the "one evening" of the
adopt-voxtype draft, in exchange for status and device UX that draft
couldn't give.
