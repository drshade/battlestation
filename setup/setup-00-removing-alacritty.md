# Removing alacritty (standardize on kitty)

**Goal:** one terminal — kitty — with alacritty gone.

## Background

A fresh CachyOS + Noctalia install ships two terminals:

- **kitty** — the main terminal. Hyprland sets `TERMINAL = "kitty"`, so
  Super+Return and the btop shortcut launch it.
- **alacritty** — used only by Noctalia's app launcher, which wraps TUI apps
  (any `.desktop` with `Terminal=true`) as `alacritty -e <app>`.

We standardized on kitty. The config that does this — Hyprland's `TERMINAL` and
Noctalia's `appLauncher.terminalCommand = "kitty -e"` — is **already committed in
this repo**, so stowing applies it automatically, and the repo carries no
alacritty package. The one thing the dotfiles can't do is uninstall the system
package.

## Step (per machine)

```sh
sudo pacman -Rns alacritty
```

## Verify

```sh
pacman -Q alacritty   # -> error: package 'alacritty' was not found
```
