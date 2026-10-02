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

## Leftovers from a Quickshell-era install

The v4 shell and its runtime are separate packages; drop them and their state
once v5 is confirmed working:

```sh
sudo pacman -Rns noctalia-shell noctalia-qs
rm -rf ~/.config/noctalia/plugins ~/.config/noctalia/colors.json ~/.config/noctalia/colorschemes
```
