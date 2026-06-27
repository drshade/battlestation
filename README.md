# cachyos-dotfiles

Personal dotfiles for CachyOS (Hyprland + Noctalia), managed with [GNU Stow](https://www.gnu.org/software/stow/).

## Layout

```
cachyos-dotfiles/
├── stow/        # the stow tree — EVERY subdir is a package, no exceptions
│   ├── hypr/.config/hypr/
│   ├── noctalia/.config/noctalia/
│   └── ...
├── setup/       # reproducible per-topic setup notes (never stowed)
├── AGENTS.md    # how to work in this repo (read this first)
├── stow-all.sh  # stow every package
└── README.md
```

Each subdirectory of `stow/` is a Stow **package** whose internal layout mirrors
`$HOME`. Example: `stow/hypr/.config/hypr/` → symlinked to `~/.config/hypr/`.

The set of packages is **defined by the directories in `stow/`** — there is no
hand-maintained list anywhere. Repo meta (docs, scripts) lives *outside* `stow/`,
so there is nothing to exclude and no ignore-list to keep in sync.

See [`setup/`](setup/) for reproducible per-topic notes on how this machine was
configured — a runbook for rebuilds.

## Setup on a new machine

```sh
sudo pacman -S stow
git clone <repo-url> ~/dev/cachyos-dotfiles
cd ~/dev/cachyos-dotfiles
./stow-all.sh
```

## Common commands

```sh
# Stow / re-link / unlink a single package (run from inside stow/)
cd stow
stow --target="$HOME" hypr            # link into place
stow --restow --target="$HOME" hypr   # re-link after adding files
stow --delete --target="$HOME" hypr   # remove symlinks (files stay in repo)
```

## Adding a new app

```sh
mkdir -p stow/newapp/.config
mv ~/.config/newapp stow/newapp/.config/newapp
( cd stow && stow --target="$HOME" newapp )
```

No README or list to update — the new directory under `stow/` *is* the registration.
