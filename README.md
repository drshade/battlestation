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

Scripts consumed by more than one package live in the `bin` package
(`stow/home/bin/.local/bin/` → `~/.local/bin/`, which is on `PATH`); helper
scripts used by a single package stay inside that package.

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
sudo pacman -S stow make
git clone git@github.com:drshade/battlestation.git ~/dev/battlestation
cd ~/dev/battlestation
make stow        # your configs -> $HOME
make stow-root   # system configs -> /   (prompts for sudo)
for f in bin/filters/*; do git config "filter.${f##*/}.clean" "$f"; done
                 # one-time: activate the git clean filters (repo-local config)
make check       # verify everything linked correctly
```

The filter line registers each tracked `bin/filters/<name>` script as the clean
command for its `filter=<name>` entry in `.gitattributes` (they hide runtime
churn in app-rewritten configs — see AGENTS.md). Git config is repo-local, so
this can't ship with the clone; `make fix` performs the same registration.

## Tasks

`make` is the entrypoint — run it with no target for the full list:

```sh
make        # list all targets
make stow   # stow home packages into $HOME
make check  # verify deployment + parse/lint (read-only; bin/doctor)
make fix    # repair: restow + install clean-filter config (bin/doctor --fix)
make drift  # what the machine has that the repo doesn't manage (bin/drift)
```

`check` and `drift` are read-only; `fix` is the only mutating task. Targets are
thin wrappers over `bin/*` and `stow/stow-*.sh` — run those directly if you
prefer. Install `shellcheck` and `luacheck` for full linting; without them
`make check` falls back to `bash -n` / `luac -p`.

Underlying per-package stow commands, when you need finer control:

```sh
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

## Open work

Smaller follow-ups live in [`TODO.md`](TODO.md); the larger structural review is
in [`PLAN.md`](PLAN.md).

## License

[MIT](LICENSE).