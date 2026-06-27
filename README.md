# cachyos-dotfiles

Personal dotfiles for CachyOS (Hyprland + Noctalia), managed with [GNU Stow](https://www.gnu.org/software/stow/).

Each top-level directory is a Stow **package** mirroring the layout under `$HOME`.
For example, `hypr/.config/hypr/` symlinks to `~/.config/hypr/`.

The set of packages is **defined by the directories in this repo** — there is no
hand-maintained list to keep in sync. A package is any top-level dir except the
meta dirs listed in `$META` below (e.g. `setup/`, which holds docs, not configs).

See [`setup/`](setup/) for reproducible per-topic notes on how this machine was
configured (e.g. removing packages, wiring up apps) — a runbook for rebuilds.

## Setup on a new machine

```sh
sudo pacman -S stow
git clone <repo-url> ~/dev/cachyos-dotfiles
cd ~/dev/cachyos-dotfiles
./stow-all.sh
```

## stow-all.sh

Stows every package (every top-level dir except meta dirs):

```sh
#!/usr/bin/env sh
set -eu
cd "$(dirname "$0")"
META="setup"                       # dirs that are NOT stow packages
for d in */; do
  d="${d%/}"
  case " $META " in *" $d "*) continue ;; esac
  stow --restow --target="$HOME" "$d"
done
```

## Common commands

```sh
# Symlink a single package into place
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

No README or list to update — the new directory *is* the registration.
