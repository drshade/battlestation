# Removing alacritty (standardize on kitty)

**Goal:** use a single terminal emulator — **kitty** — and remove alacritty.

## Background

A fresh CachyOS + Noctalia install ships with *two* terminals wired in:

- **kitty** — the main terminal. Hyprland sets `TERMINAL = "kitty"` in
  `hypr/.config/hypr/config/defaults.lua`, so Super+Return and the btop
  shortcut launch kitty.
- **alacritty** — used only by Noctalia's app launcher. The launcher's
  `terminalCommand` setting wraps terminal apps (any `.desktop` with
  `Terminal=true`, e.g. btop/htop/vim) as `alacritty -e <app>`.

So removing alacritty requires repointing Noctalia at kitty first, otherwise
launching a TUI app from the Noctalia launcher would break.

## Steps

### 1. Point Noctalia's launcher at kitty

In `noctalia/.config/noctalia/settings.json`, under `appLauncher`:

```diff
-        "terminalCommand": "alacritty -e",
+        "terminalCommand": "kitty -e",
```

(Reload Noctalia, or log out/in, for it to pick up the change if it was running.)

### 2. Remove the alacritty Stow package from this repo

```sh
cd ~/dev/cachyos-dotfiles
stow --delete --target="$HOME" alacritty   # removes the ~/.config/alacritty symlink
git rm -r alacritty                         # drop the config from the repo
```

### 3. Uninstall the alacritty system package (needs sudo)

```sh
sudo pacman -Rns alacritty
```

## Verify

```sh
ls ~/.config/alacritty        # should be: No such file or directory
pacman -Q alacritty           # should be: error: package 'alacritty' was not found
grep terminalCommand ~/.config/noctalia/settings.json   # -> "kitty -e"
```

## Done when

- Super+Return opens kitty (unchanged).
- Launching a TUI app (e.g. btop) from the Noctalia launcher opens it in kitty.
- alacritty is gone from the system and the repo.
