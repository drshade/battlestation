---
name: battlestation
description: >-
  Context for "battlestation" — The User's personal machine: a CachyOS +
  Hyprland + Noctalia (Quickshell) desktop whose configuration lives in the
  dotfiles repo at ~/dev/battlestation. Load this BEFORE touching anything
  about THIS machine, whether the user is ASKING how something works or
  TELLING you to change/fix/configure it — displays and monitors (scale,
  resolution, refresh, DPMS, arrangement, multi-monitor), workspaces and
  keybinds, the bar/widgets/wallpaper/theming, audio, packages, services,
  the shell, the agent harnesses, or the `bsctl` tool that drives much of
  it. Reach for it on imperative hardware/desktop requests too ("bump eDP-1
  to 1.75 scale", "rebind X", "restart the bar") — those are exactly what
  it covers, and there is usually a purpose-built command (e.g.
  `bsctl display set scale`) rather than raw hyprctl. The configured set
  grows over time, so load it for ANY machine/desktop/config task even if
  the topic doesn't obviously map to a known tool — the answer very likely
  lives in this repo, and if it doesn't yet, this is where it would be
  added. Applies even from outside the repo directory.
---

# battlestation

**battlestation** is The User's personal desktop configuration for this machine.
It is a single git repository — the source of truth for how the system is set up.

- **Repo:** `~/dev/battlestation` (remote: `git@github.com:drshade/battlestation.git`)
- **OS:** CachyOS (Arch-based)
- **Compositor:** Hyprland (Wayland)
- **Shell/UI:** Noctalia — a Quickshell-based desktop shell
- **Login shell:** fish
- **Dotfile management:** GNU Stow, grouped by target root: `stow/<group>/<pkg>/`
  (`home/` → `$HOME`, `root/` → `/` for system configs like `/etc`)

## First move: read the repo, don't rely on this file

This skill is deliberately thin. The repo documents itself and is the single
source of truth — read it rather than answering from memory:

1. `~/dev/battlestation/AGENTS.md` — **read this first.** Engineering principles
   and conventions for working in the repo.
2. `~/dev/battlestation/README.md` — layout, the stow model, common commands.
3. `~/dev/battlestation/setup/` — per-topic runbook notes on how this machine
   was configured (useful for "why is X set up this way?").
4. The relevant package under `stow/<group>/<pkg>/` for the actual config files.

## What's configured here

Each tool or app configured on this machine is one stow package — a directory
under a target group: `stow/home/` (stowed into `$HOME`) or `stow/root/` (stowed
into `/`, for system configs like `/etc`). This set **grows as the machine is
customised**, and groups come and go, so never assume a fixed list: check what
exists now with

```sh
ls ~/dev/battlestation/stow/*/
```

Each directory holds that tool's config, mirroring its target
(`stow/home/<pkg>/.config/<pkg>/` → `~/.config/<pkg>/`;
`stow/root/<pkg>/etc/<pkg>/` → `/etc/<pkg>/`). If something on the system isn't
configured here yet, this repo is where it would be added.

## Making changes

- Config files are **symlinked into place**, so editing a file under
  `stow/<group>/<pkg>/…` changes the live config immediately — no copy step.
- After **adding new files** to a package, re-stow it:
  `cd ~/dev/battlestation/stow/home && stow --restow --target="$HOME" <pkg>`
  (or run `~/dev/battlestation/stow/stow-home.sh` to restow everything).
- Follow AGENTS.md's principles: correct-by-construction, one source of truth,
  and update the docs/runbook in the same change when structure changes.

## Ground rules

- **Verify current state before acting.** Files, keybinds, and settings change;
  read the actual file rather than trusting this summary or old memory.
- Prefer the repo's own docs over assumptions. When something isn't documented,
  add a note to `setup/` capturing the *why*, per AGENTS.md.
