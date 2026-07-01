# battlestation

My **battlestation** — the CachyOS + Hyprland + Noctalia (Quickshell) desktop
configuration and personal customisations for this machine, managed as dotfiles
with [GNU Stow](https://www.gnu.org/software/stow/).

## Layout

```
battlestation/
├── stow/                  # the stow module: trees grouped by target + drivers
│   ├── home/              #   packages stowed into $HOME
│   │   └── <package>/.config/<package>/
│   ├── stow-home.sh       #   stow home/ into $HOME      (run as you)
│   └── stow-root.sh       #   stow root/ into /          (run with sudo)
│   # (future: stow/root/ → system configs like /etc/...)
├── setup/                 # reproducible per-topic setup notes (never stowed)
├── AGENTS.md              # how to work in this repo (read this first)
└── README.md
```

Packages are organized by **target root**: `stow/<target-group>/<package>/`, and
each group has its own driver script `stow/stow-<group>.sh`. A package's internal
layout mirrors its target, e.g. `stow/home/<pkg>/.config/<pkg>/` → `~/.config/<pkg>/`.

Why the `home/` layer when there's only one target today? Because the only thing
that varies between dotfiles is the *destination root* — everything under your
home dir (`~/.config`, `~/.local`, `~/.bashrc`) is the **same** target and needs
no split. A genuinely different root (e.g. `/etc`) becomes `stow/root/` + a
`stow-root.sh`, with no refactor.

Why two scripts instead of one? Home configs must be stowed **as you**; system
configs need **root**. Running everything under sudo would make your `~` symlinks
root-owned. Keeping them separate keeps each at the correct privilege.

The package set is **defined by the directories under `stow/<group>/`** — there
is no hand-maintained list. Repo meta (docs) lives outside `stow/`.

See [`setup/`](setup/) for reproducible per-topic notes on how this machine was
configured — a runbook for rebuilds.

## Setup on a new machine

```sh
sudo pacman -S stow
git clone git@github.com:drshade/battlestation.git ~/dev/battlestation
cd ~/dev/battlestation
./stow/stow-home.sh          # your configs -> $HOME
sudo ./stow/stow-root.sh     # system configs -> /   (no-op until stow/root/ exists)
```

## Common commands

```sh
# Operate on a single package (run from inside its target group)
cd stow/home
stow --target="$HOME" hypr            # link into place
stow --restow --target="$HOME" hypr   # re-link after adding files
stow --delete --target="$HOME" hypr   # remove symlinks (files stay in repo)
```

## Adding a new app (targets $HOME)

```sh
mkdir -p stow/home/newapp/.config
mv ~/.config/newapp stow/home/newapp/.config/newapp
( cd stow/home && stow --target="$HOME" newapp )
```

No README or list to update — the new directory under `stow/home/` *is* the
registration. To add a new *target root*, create `stow/<group>/` and a matching
`stow/stow-<group>.sh`.

# TODO
[ ] stuff installed by flatpak doesn't automatically add to the launcher
[ ] designing and setting up a great keymap (cmd for common, caps for window management maybe?)
[ ] keyboard repeat and delay are too slow