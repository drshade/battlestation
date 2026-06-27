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
3. **Separate namespaces.** Stow only ever reads `stow/<group>/`, so packages —
   the only stowable namespace — live there. The stow driver scripts live at the
   `stow/` module root (co-located with the trees they deploy, but never inside a
   group, so they are structurally un-stowable). Project meta (docs, runbooks)
   lives at the repo root. Mixing packages with non-packages *inside a group* is
   what forced the old ignore-list — don't.
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

- Stowable content lives under **`stow/`**, organized **by target root**:
  `stow/<target-group>/<package>/`. Each group has a driver script
  `stow/stow-<group>.sh` that stows that group into its target.
- Each directory inside a target group is a **package** whose internal layout
  mirrors that target. Example: `stow/home/<pkg>/.config/<pkg>/` →
  `~/.config/<pkg>/`.
- Everything inside a target group is a package, **by construction** — there is
  no ignore-list. The driver scripts sit at the `stow/` module root, never
  inside a group, so they are never stowable. Project meta (`README.md`,
  `AGENTS.md`, `setup/`) lives at the repo root.
- **Why group by target.** The only axis that varies between dotfiles is the
  destination *root*. Everything under `$HOME` (`~/.config`, `~/.local`,
  `~/.bashrc`) is the same target and needs no split — a package just mirrors
  the deeper path. A genuinely different root (e.g. `/etc`) becomes a new group
  `stow/root/` with its own `stow/stow-root.sh`.
- **Why split scripts by target.** Home configs must be stowed as the normal
  user; system configs need root. One combined script run under sudo would make
  `~` symlinks root-owned. So `stow-home.sh` refuses to run as root and
  `stow-root.sh` refuses to run as non-root — each enforces its own privilege.
- Adding a target group = create `stow/<group>/` **and** a matching
  `stow/stow-<group>.sh`; the script name mirrors the dir name, so the pairing
  is self-evident.
- After stowing, `~/.config/<app>` is a **symlink into this repo**. Editing the
  file in `~/.config` and editing it here are the same file — there is no copy
  step and no sync to run.
- Stow everything with `./stow/stow-home.sh` (and `sudo ./stow/stow-root.sh`
  once `stow/root/` has packages).

### Source-of-truth rule (important)

**Do not enumerate the package list in prose.** The directories under
`stow/<group>/` are the single source of truth. README and docs must stay
generic. We adopted this after the README's hand-maintained package table
drifted out of sync with reality (it still listed `alacritty` after we had
removed it). If you find yourself writing "the packages are: a, b, c" in a doc,
stop — point at `stow/<group>/` instead.

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

`setup/` holds **reproducible, per-topic notes** so a fresh install can be
rebuilt and past decisions explained. Filenames follow a
`<theme>-<order>-<description>` convention documented in `setup/README.md`.

- `setup/` lives **outside `stow/`**, so its markdown can never be stowed.
- **A note documents only what stowing can't reproduce** — out-of-band system
  actions (`pacman`, enabling a service). Config changes are applied by `stow`
  and live in the tracked files + git history, never as runbook steps.
- Config-file comments are for code; setup knowledge goes in `setup/`.
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
  **kitty** in both (see `setup/setup-00-removing-alacritty.md`).
- **Secrets / machine-local state** must never be committed. See `.gitignore`
  (e.g. `fish_variables`). When adding a package, scan it for tokens/state
  before staging.
- **Empty dirs** are not tracked by git; that's expected, not a bug.

## Working habits in this repo

- After any symlink/stow operation, verify: `ls -ld ~/.config/<app>` should show
  the symlink, and `readlink -f` it should resolve into this repo.
- Before committing, sanity-check nothing secret was staged:
  `git ls-files | grep -iE 'token|secret|fish_variables'`.
- **Avoid `git add -A`.** Apps like Noctalia rewrite their own tracked config at
  runtime (see gotchas), so a blanket add silently bundles unrelated churn into
  your commit. Stage explicit paths and review `git status` first; if churn lands
  in the wrong commit, split it (unpushed history is safe to tidy).
- System-level actions (`pacman -Rns`, `pacman -S`) need sudo — hand the user
  the exact command rather than running it, and record it in the relevant
  `setup/` note so it is reproducible.
