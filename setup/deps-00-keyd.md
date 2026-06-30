# keyd (per-keyboard modifier remapping)

`stow/root/keyd/etc/keyd/default.conf` swaps left Super/Alt on the Ducky One2 SF
(matched by USB id `0416:0123`) at the evdev level, so the remap follows the
keyboard and survives hotplug. Hyprland's Lua config can't express this per
device — there's no `hl.device`, and `hyprctl keyword` is rejected by the
non-legacy parser (see AGENTS.md gotchas).

Install keyd *before* stowing, so `/etc/keyd/` is a real dir and stow folds the
`default.conf` symlink into it (rather than symlinking the whole directory):

```sh
sudo pacman -S keyd
sudo ./stow/stow-root.sh
sudo systemctl enable --now keyd
sudo keyd reload          # re-read config after later edits to default.conf
```

`sudo keyd monitor` prints device ids and live keycodes — use it to find the
`vendor:product` for another board to add under `[ids]`.
