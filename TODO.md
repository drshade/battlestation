# TODO

Open follow-ups for the battlestation config. See [`PLAN.md`](PLAN.md) for the
larger structural review; this list is for smaller, concrete tasks.

- [ ] Flatpak-installed apps don't show up in the launcher — likely
      `XDG_DATA_DIRS` missing `/var/lib/flatpak/exports/share` in the
      uwsm/Hyprland env (fix in hypr `environment.lua` + a setup note).
- [ ] Design and set up a great keymap (cmd for common, caps for window
      management, maybe?) — in progress in [`docs/keybinds.md`](docs/keybinds.md).
- [ ] Keyboard repeat and delay are too slow — one line in hypr `input.lua`.
