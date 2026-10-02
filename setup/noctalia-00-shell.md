# Noctalia (the desktop shell)

`noctalia` arrives with the `cachyos-hypr-noctalia` meta (manifest:
`packages/pacman-base.txt`); Hyprland's `autostart.lua` starts it. Config is
every `*.toml` in `~/.config/noctalia/` (stowed), merged alphabetically and
hot-reloaded on save; `noctalia config validate` checks it.

Runtime state the shell owns (not stowed, machine-local):

- `~/.local/state/noctalia/settings.toml` — GUI overrides; shadow the stowed
  TOML (AGENTS.md gotchas). `state.toml` beside it is unrelated runtime state.
- `~/.local/state/noctalia/plugins/` — catalog plugin checkouts.
- `~/.cache/noctalia/noctalia.log` — the log (rotates at 1 MiB).

Theme templates (`theme.toml`, `[theme.templates] builtin_ids`) write real
files into the themed apps' config dirs on every palette change — derived
output, gitignored where it lands inside a stowed package. `noctalia theme
--list-templates` lists the ids.

Restart with `Hyper+Backspace` (`scripts/restart-shell.sh`, which waits out
the single-instance lock). `noctalia msg --help` lists every IPC verb the
keybinds use; `noctalia msg panel-toggle <bogus>` prints the live panel ids.

## Local build override (bar drag-and-drop)

Until upstream ships plugin API 33 (noctalia-dev/noctalia PR from the
`drshade/noctalia` fork, branch `bar-plugin-drag-drop`), the shell runs from a
local release build installed under `~/.local`, which precedes `/usr/bin` on
PATH so autostart and `restart-shell.sh` pick it up unchanged:

```sh
cd ~/dev/noctalia && just configure release ~/.local && just build release && just install release
```

Build deps: `sudo pacman -S meson just nlohmann-json stb` (plus what
BUILDING.md lists; the CachyOS package already pulled the runtime libs).
`bar_drag = true` in `plugins.toml` depends on this build — on the packaged
5.2.0 the pills would vanish. **Remove once upstream ships it**: `rm
~/.local/bin/noctalia && rm -rf ~/.local/share/noctalia/assets`, set
`bar_drag` back to false only if the packaged version still lacks API 33,
delete this section.

## Leftovers from a Quickshell-era install

The v4 shell and its runtime are separate packages; drop them and their state
once v5 is confirmed working:

```sh
sudo pacman -Rns noctalia-shell noctalia-qs
rm -rf ~/.config/noctalia/plugins ~/.config/noctalia/colors.json ~/.config/noctalia/colorschemes
```
