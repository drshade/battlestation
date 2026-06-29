# zenity (required by the rename-workspace script)

`hypr/scripts/rename-workspace.sh` (bound to `SUPER + SHIFT + R`) prompts for the
new name via zenity. Without it the script exits silently and the bind appears
to do nothing. Stow can't install it:

```sh
sudo pacman -S zenity
```
