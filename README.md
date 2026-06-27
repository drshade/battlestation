# cachyos-dotfiles

Personal dotfiles for CachyOS (Hyprland + Noctalia), managed with [GNU Stow](https://www.gnu.org/software/stow/).

Each top-level directory is a Stow **package** mirroring the layout under `$HOME`.
For example, `hypr/.config/hypr/` symlinks to `~/.config/hypr/`.

See [`setup/`](setup/) for reproducible per-topic notes on how this machine was
configured (e.g. removing packages, wiring up apps) — a runbook for rebuilds.

## Setup on a new machine

```sh
sudo pacman -S stow
git clone <repo-url> ~/dev/cachyos-dotfiles
cd ~/dev/cachyos-dotfiles
stow --target="$HOME" hypr noctalia fish alacritty kitty btop
```

## Common commands

```sh
# Symlink a package into place
stow --target="$HOME" hypr

# Re-link after adding new files to a package
stow --restow --target="$HOME" hypr

# Remove the symlinks (does not delete the files in the repo)
stow --delete --target="$HOME" hypr
```

## Adding a new app

```sh
mkdir -p newapp/.config
mv ~/.config/newapp newapp/.config/newapp
stow --target="$HOME" newapp
```

## Packages

| Package     | Tracks            |
|-------------|-------------------|
| `hypr`      | `~/.config/hypr`      |
| `noctalia`  | `~/.config/noctalia`  |
| `fish`      | `~/.config/fish`      |
| `alacritty` | `~/.config/alacritty` |
| `kitty`     | `~/.config/kitty`     |
| `btop`      | `~/.config/btop`      |
