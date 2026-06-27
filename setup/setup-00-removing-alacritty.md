# Removing alacritty (standardize on kitty)

The kitty config (Hyprland `TERMINAL`, Noctalia `terminalCommand`) is already in
the repo, so stowing handles it. The only out-of-band step is removing the
system package:

```sh
sudo pacman -Rns alacritty
```
