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
│   ├── root/              #   packages stowed into /     (system configs, e.g. /etc)
│   ├── stow-home.sh       #   stow home/ into $HOME      (run as you)
│   └── stow-root.sh       #   stow root/ into /          (run with sudo)
├── setup/                 # reproducible per-topic setup notes (never stowed)
├── AGENTS.md              # how to work in this repo (read this first)
└── README.md
```

Packages are organized by **target root**: `stow/<target-group>/<package>/`, and
each group has its own driver script `stow/stow-<group>.sh`. A package's internal
layout mirrors its target, e.g. `stow/home/<pkg>/.config/<pkg>/` → `~/.config/<pkg>/`.

Why the `home/` layer at all? Because the only thing that varies between dotfiles
is the *destination root* — everything under your home dir (`~/.config`,
`~/.local`, `~/.bashrc`) is the **same** target and needs no split. A genuinely
different root (e.g. `/etc`) is a separate group `stow/root/` + a `stow-root.sh`,
with no refactor — that's where system configs like `keyd`'s live.

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
sudo ./stow/stow-root.sh     # system configs -> /
```

## Common commands

```sh
# Operate on a single package (run from inside its target group)
cd stow/home
stow --target="$HOME" hypr            # link into place
stow --restow --target="$HOME" hypr   # re-link after adding files
stow --delete --target="$HOME" hypr   # remove symlinks (files stay in repo)
```

## Checking the repo

```sh
bin/doctor        # verify deployment: every tracked file is a live symlink into
                  # the repo, stow simulates clean, shell/JSON/Lua all parse
bin/doctor --fix  # the only mutating mode — restow to repair broken links
bin/drift         # read-only: what the machine has that the repo doesn't manage
```

Both default to read-only (dry-run). Install `shellcheck` and `luacheck` for
full linting; without them `bin/doctor` falls back to `bash -n` / `luac -p`.

## Adding a new app (targets $HOME)

```sh
mkdir -p stow/home/newapp/.config
mv ~/.config/newapp stow/home/newapp/.config/newapp
( cd stow/home && stow --target="$HOME" newapp )
```

No README or list to update — the new directory under `stow/home/` *is* the
registration. To add a new *target root*, create `stow/<group>/` and a matching
`stow/stow-<group>.sh`.

## Open work

Smaller follow-ups live in [`TODO.md`](TODO.md); the larger structural review is
in [`PLAN.md`](PLAN.md).

## License

[MIT](LICENSE).