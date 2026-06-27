# cachyos-dotfiles

Personal dotfiles for CachyOS (Hyprland + Noctalia), managed with [GNU Stow](https://www.gnu.org/software/stow/).

## Layout

```
cachyos-dotfiles/
├── stow/              # all stowable content, grouped by target root
│   └── home/          #   → stowed into $HOME
│       ├── hypr/.config/hypr/
│       ├── noctalia/.config/noctalia/
│       └── ...
│   # (future: stow/root/ → stowed into /  for system configs)
├── setup/             # reproducible per-topic setup notes (never stowed)
├── AGENTS.md          # how to work in this repo (read this first)
├── stow-all.sh        # stow every package into its target
└── README.md
```

Packages are organized by **target root**: `stow/<target-group>/<package>/`. The
group name maps to a `stow --target` (see `stow-all.sh`): `home → $HOME`. A
package's internal layout mirrors that target, e.g. `stow/home/hypr/.config/hypr/`
→ `~/.config/hypr/`.

Why the `home/` layer when there's only one target today? Because the only thing
that varies between dotfiles is the *destination root* — everything under your
home dir (`~/.config`, `~/.local`, `~/.bashrc`) is the **same** target and needs
no split. A genuinely different root (e.g. `/etc`, which needs sudo) becomes
`stow/root/` with zero refactor.

The package set is **defined by the directories under `stow/<group>/`** — there
is no hand-maintained list. Repo meta (docs, scripts) lives *outside* `stow/`, so
there is nothing to exclude.

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
registration. To add a new *target root*, see `stow-all.sh`'s `target_for()`.
