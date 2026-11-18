# Karabiner-Elements (Caps Lock → Hyper on macOS)

The macOS counterpart of xkb `caps:hyper`: the keybind model's DO modifier
(`stow/home-darwin/aerospace`) is Caps Lock sent as Cmd+Alt+Ctrl. Shift stays
out of Hyper so it remains the "…and stay" suffix.

Karabiner rewrites its own `karabiner.json` at runtime, so the repo ships the
rule as an asset (`stow/home-darwin/karabiner`) rather than owning that file.
Enabling it is a one-time click:

1. Open Karabiner-Elements and grant the Input Monitoring and driver
   permissions it asks for.
2. Complex Modifications → Add predefined rule → "battlestation" → enable
   "Caps Lock → Hyper (Cmd+Alt+Ctrl)".
