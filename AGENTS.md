# AGENTS.md

Guidance for AI agents (and future-me) working in this repo. Read this before
making changes.

## What this repo is

Personal dotfiles for a **CachyOS** machine running **Hyprland** (Wayland
compositor) + **Noctalia** (a Quickshell-based desktop shell). Configs are
version-controlled here and symlinked into place with **GNU Stow**.

## The Stow model

- Each top-level directory is a Stow **package** whose internal layout mirrors
  `$HOME`. Example: `hypr/.config/hypr/` → symlinked to `~/.config/hypr/`.
- After stowing, `~/.config/<app>` is a **symlink into this repo**. Editing the
  file in `~/.config` and editing it here are the same file — there is no copy
  step and no sync to run.
- `./stow-all.sh` stows every package. It treats every top-level dir as a
  package **except** the meta dirs in its `$META` list (currently `setup`).

### Source-of-truth rule (important)

**Do not enumerate the package list in prose.** The directories in the repo are
the single source of truth. README and docs must stay generic. We adopted this
after the README's hand-maintained package table drifted out of sync with
reality (it still listed `alacritty` after we had removed it). If you find
yourself writing "the packages are: a, b, c" in a doc, stop — point at the
directory listing or `stow-all.sh` instead.

Consequence: **adding or removing an app requires no doc edit.** The directory's
presence (or absence) is the registration.

## Adding / removing a package

```sh
# add
mkdir -p newapp/.config && mv ~/.config/newapp newapp/.config/newapp
stow --target="$HOME" newapp

# remove
stow --delete --target="$HOME" oldapp   # drops the ~/.config symlink
git rm -r oldapp                         # drops it from the repo
```

## The setup/ runbook

`setup/` holds **reproducible, per-topic notes** — one `.md` per topic, named
`<verb>-<subject>.md` — documenting what changed, why, and the exact commands,
so a fresh install can be rebuilt and past decisions are explained.

- `setup/` is a **meta dir**: it is NOT a stow package and is excluded in
  `stow-all.sh`'s `$META`, so its markdown is never symlinked into `~/.config`.
- Keep dotfiles themselves free of transient/setup notes — that knowledge goes
  in `setup/`, not in config-file comments.
- It is a **runbook, not a changelog.** If a decision is reversed, update or
  delete the relevant note rather than appending contradictions. Then check
  whether README / other notes need the same correction (see the alacritty
  drift above — a change in one place often implies edits in others).

## Known gotchas

- **Noctalia rewrites its own config.** `noctalia/.config/noctalia/settings.json`
  is written by the running shell. Editing it on disk works, but if Noctalia is
  running it may overwrite your edit on its next settings-write. After editing,
  reload Noctalia (or log out/in) and re-verify with `grep`.
- **`noctalia/.config/noctalia/colors.json`** is regenerated on wallpaper/theme
  changes (matugen-style output), so it churns in diffs. It is currently
  tracked; gitignore it if the noise is annoying.
- **Two-terminal trap (resolved).** Hyprland's terminal is set in
  `hypr/.config/hypr/config/defaults.lua` (`TERMINAL = "kitty"`); Noctalia's
  launcher uses `appLauncher.terminalCommand` in its `settings.json`. These are
  independent — changing one does not change the other. We standardized on
  **kitty** in both (see `setup/removing-alacritty.md`).
- **Secrets / machine-local state** must never be committed. See `.gitignore`
  (e.g. `fish_variables`). When adding a package, scan it for tokens/state
  before staging.
- **Empty dirs** are not tracked by git; that's expected, not a bug.

## Working habits in this repo

- After any symlink/stow operation, verify: `ls -ld ~/.config/<app>` should show
  the symlink, and `readlink -f` it should resolve into this repo.
- Before committing, sanity-check nothing secret was staged:
  `git ls-files | grep -iE 'token|secret|fish_variables'`.
- System-level actions (`pacman -Rns`, `pacman -S`) need sudo — hand the user
  the exact command rather than running it, and record it in the relevant
  `setup/` note so it is reproducible.
