# AGENTS.md

Guidance for AI agents (and future-me) working in this repo. Read this before
making changes.

## Engineering principles (read first)

This repo is a **foundation** that will be built on for a long time. Treat
structure as a first-class concern; do not build on a messy base.

1. **Correct by construction over correct by convention.** Prefer designs where
   the wrong thing is *impossible*, not merely *discouraged*. Example: every
   subdir of `stow/` is a package — there is no ignore-list to forget. We chose
   this over a root-level `META` exclusion list precisely because exclusion
   lists rely on humans remembering to maintain them.
2. **One source of truth.** Never duplicate a fact that can be derived. The
   package set is the directory listing of `stow/`, not a table in a doc. (A
   hand-maintained package table once drifted and contradicted reality — don't
   reintroduce that class of bug.)
3. **Separate namespaces.** Stowable content lives in `stow/`; repo meta (docs,
   scripts, runbooks) lives outside it. Mixing the two is what forced the old
   ignore-list.
4. **Design before editing structure.** When a change affects layout or
   conventions, stop and think it through; then update the docs that describe it
   in the same change so nothing drifts.
5. **Capture reasoning, not just actions.** Record *why* in AGENTS.md / `setup/`
   notes so future decisions have context.

When in doubt, choose the option that is simpler to keep correct six months from
now, even if it costs a little more today.

## What this repo is

Personal dotfiles for a **CachyOS** machine running **Hyprland** (Wayland
compositor) + **Noctalia** (a Quickshell-based desktop shell). Configs are
version-controlled here and symlinked into place with **GNU Stow**.

## The Stow model

- All Stow packages live under **`stow/`**. Each subdirectory of `stow/` is a
  package whose internal layout mirrors `$HOME`. Example:
  `stow/hypr/.config/hypr/` → symlinked to `~/.config/hypr/`.
- Everything in `stow/` is a package, **by construction** — there is no
  ignore-list. Repo meta (`README.md`, `AGENTS.md`, `setup/`, `stow-all.sh`)
  lives *outside* `stow/`, so it can never be stowed by accident.
- After stowing, `~/.config/<app>` is a **symlink into this repo**. Editing the
  file in `~/.config` and editing it here are the same file — there is no copy
  step and no sync to run.
- `./stow-all.sh` stows every package: `cd stow && stow --restow -t ~ */`.

### Source-of-truth rule (important)

**Do not enumerate the package list in prose.** The directories under `stow/`
are the single source of truth. README and docs must stay generic. We adopted
this after the README's hand-maintained package table drifted out of sync with
reality (it still listed `alacritty` after we had removed it). If you find
yourself writing "the packages are: a, b, c" in a doc, stop — point at `stow/`
or `stow-all.sh` instead.

Consequence: **adding or removing an app requires no doc edit.** The directory's
presence (or absence) under `stow/` is the registration.

## Adding / removing a package

```sh
# add
mkdir -p stow/newapp/.config && mv ~/.config/newapp stow/newapp/.config/newapp
( cd stow && stow --target="$HOME" newapp )

# remove
( cd stow && stow --delete --target="$HOME" oldapp )   # drops ~/.config symlink
git rm -r stow/oldapp                                  # drops it from the repo
```

## The setup/ runbook

`setup/` holds **reproducible, per-topic notes** — one `.md` per topic, named
`<verb>-<subject>.md` — documenting what changed, why, and the exact commands,
so a fresh install can be rebuilt and past decisions are explained.

- `setup/` lives **outside `stow/`**, so it is structurally not a stow package
  and its markdown can never be symlinked into `~/.config`.
- Keep dotfiles themselves free of transient/setup notes — that knowledge goes
  in `setup/`, not in config-file comments.
- It is a **runbook, not a changelog.** If a decision is reversed, update or
  delete the relevant note rather than appending contradictions. Then check
  whether README / other notes need the same correction (see the alacritty
  drift above — a change in one place often implies edits in others).

## Known gotchas

- **Noctalia rewrites its own config.** `stow/noctalia/.config/noctalia/settings.json`
  is written by the running shell. Editing it on disk works, but if Noctalia is
  running it may overwrite your edit on its next settings-write. After editing,
  reload Noctalia (or log out/in) and re-verify with `grep`.
- **`stow/noctalia/.config/noctalia/colors.json`** is regenerated on wallpaper/theme
  changes (matugen-style output), so it churns in diffs. It is currently
  tracked; gitignore it if the noise is annoying.
- **Two-terminal trap (resolved).** Hyprland's terminal is set in
  `stow/hypr/.config/hypr/config/defaults.lua` (`TERMINAL = "kitty"`); Noctalia's
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
