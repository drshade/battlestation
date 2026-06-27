# AGENTS.md

Guidance for AI agents (and future-me) working in this repo. Read this before
making changes.

## Engineering principles (read first)

This repo is a **foundation** that will be built on for a long time. Treat
structure as a first-class concern; do not build on a messy base.

1. **Correct by construction over correct by convention.** Prefer designs where
   the wrong thing is *impossible*, not merely *discouraged*. Example: every
   directory inside a target group under `stow/` is a package — there is no
   ignore-list to forget. We chose this over a root-level `META` exclusion list
   precisely because exclusion lists rely on humans remembering to maintain them.
2. **One source of truth.** Never duplicate a fact that can be derived. The
   package set is the directory listing under `stow/<group>/`, not a table in a
   doc. (A hand-maintained package table once drifted and contradicted reality —
   don't reintroduce that class of bug.)
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

- All stowable content lives under **`stow/`**, organized **by target root**:
  `stow/<target-group>/<package>/`. The group name maps to a `stow --target`
  (see `target_for()` in `stow-all.sh`): `home → $HOME`, `root → /`.
- Each directory inside a target group is a **package** whose internal layout
  mirrors that target. Example: `stow/home/hypr/.config/hypr/` →
  `~/.config/hypr/`.
- Everything inside a target group is a package, **by construction** — there is
  no ignore-list. Repo meta (`README.md`, `AGENTS.md`, `setup/`, `stow-all.sh`)
  lives *outside* `stow/`, so it can never be stowed by accident.
- **Why group by target.** The only axis that varies between dotfiles is the
  destination *root*. Everything under `$HOME` (`~/.config`, `~/.local`,
  `~/.bashrc`) is the same target and needs no split — a package just mirrors
  the deeper path. A genuinely different root (e.g. `/etc`) becomes a new group
  `stow/root/`. Adding a target = add a case to `target_for()` **and** create
  the matching `stow/<group>/` dir; the guard errors on any unregistered group,
  so the two cannot drift.
- After stowing, `~/.config/<app>` is a **symlink into this repo**. Editing the
  file in `~/.config` and editing it here are the same file — there is no copy
  step and no sync to run.
- `./stow-all.sh` stows every package in every group into its mapped target.

### Source-of-truth rule (important)

**Do not enumerate the package list in prose.** The directories under
`stow/<group>/` are the single source of truth. README and docs must stay
generic. We adopted this after the README's hand-maintained package table
drifted out of sync with reality (it still listed `alacritty` after we had
removed it). If you find yourself writing "the packages are: a, b, c" in a doc,
stop — point at `stow/` or `stow-all.sh` instead.

Consequence: **adding or removing an app requires no doc edit.** The directory's
presence (or absence) under its target group is the registration.

## Adding / removing a package

```sh
# add (targets $HOME)
mkdir -p stow/home/newapp/.config && mv ~/.config/newapp stow/home/newapp/.config/newapp
( cd stow/home && stow --target="$HOME" newapp )

# remove
( cd stow/home && stow --delete --target="$HOME" oldapp )   # drops ~/.config symlink
git rm -r stow/home/oldapp                                  # drops it from the repo
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

- **Noctalia rewrites its own config.** `stow/home/noctalia/.config/noctalia/settings.json`
  is written by the running shell. Editing it on disk works, but if Noctalia is
  running it may overwrite your edit on its next settings-write. After editing,
  reload Noctalia (or log out/in) and re-verify with `grep`.
- **`stow/home/noctalia/.config/noctalia/colors.json`** is regenerated on wallpaper/theme
  changes (matugen-style output), so it churns in diffs. It is currently
  tracked; gitignore it if the noise is annoying.
- **Two-terminal trap (resolved).** Hyprland's terminal is set in
  `stow/home/hypr/.config/hypr/config/defaults.lua` (`TERMINAL = "kitty"`); Noctalia's
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
