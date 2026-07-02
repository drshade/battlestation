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

Personal dotfiles, published as a reference for others, for a **CachyOS**
machine running **Hyprland** (Wayland compositor) + **Noctalia** (a
Quickshell-based desktop shell). Configs are version-controlled here and
symlinked into place with **GNU Stow**.

## The Stow model

- Stowable content lives under **`stow/`**, organized **by target root**:
  `stow/<target-group>/<package>/`. Each group has a driver script
  `stow/stow-<group>.sh` that stows that group into its target.
- Each directory inside a target group is a **package** whose internal layout
  mirrors that target. Example: `stow/home/<pkg>/.config/<pkg>/` →
  `~/.config/<pkg>/`.
- **Scripts follow their consumers.** A helper used by a single package lives
  inside that package (e.g. a hypr-only script under
  `stow/home/hypr/.config/hypr/scripts/`). A script consumed by more than one
  package goes in the `bin` package — `stow/home/bin/.local/bin/` →
  `~/.local/bin/` (on `PATH`) — so no package reaches into another's tree.
  Callers still reference it as `$HOME/.local/bin/<name>` rather than a bare
  name: hook/exec environments don't always inherit a full `PATH`.
- **bsctl owns stateful protocols; shell owns glue and recovery.** `ctl/` (a
  Rust crate, repo-tooling namespace like `bin/`) builds `~/.local/bin/bsctl`
  via `make build`. Logic that maintains shared state across multiple
  consumers or is hot-path/JSON-heavy belongs there (currently the whole
  claude-ws protocol: `bsctl hook` for the Claude Code hooks, `bsctl poll`
  for the widget; `claude-ws-status.sh` is kept as the executable reference +
  rollback during the trial). Plain system glue stays shell — and anything
  that must work **when the system is broken** (`restart_crashed_lock.sh`,
  `displays-on.sh`) stays shell *as policy*: a recovery path must never
  depend on a build artifact. bsctl is the one deployed artifact that is
  built rather than symlinked, so it can go stale against its source —
  doctor verifies freshness (fails when tracked `ctl/` files are newer than
  the binary) and `make fix` rebuilds.
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
- After stowing, `~/.config/<app>` is a **real directory** whose *files* are
  symlinks into this repo. Editing the file in `~/.config` and editing it here
  are the same file — there is no copy step and no sync to run.
- **Why `--no-folding`.** The driver scripts pass `--no-folding` to stow. Without
  it, stow "folds": if a target dir doesn't exist, it symlinks the whole
  *package directory* into the repo instead of creating a real dir with
  per-file symlinks. Then anything else that writes a sibling into that dir
  (another app, a system tool) writes *physically into the repo* under the wrong
  package — state landing inside version control by accident. `--no-folding`
  forces real dirs + per-file symlinks, so foreign writes always hit the real
  filesystem, never the repo. This makes the `claude` package's "never track
  state" gitignore promise correct by construction rather than by accident of
  which dir happened to exist first.
- Stow everything with `./stow/stow-home.sh` and `sudo ./stow/stow-root.sh`.

### Source-of-truth rule (important)

**Do not enumerate the package list in prose.** The directories under
`stow/<group>/` are the single source of truth. README and docs must stay
generic. We adopted this after the README's hand-maintained package table
drifted out of sync with reality (it still listed `alacritty` after we had
removed it). If you find yourself writing "the packages are: a, b, c" in a doc,
stop — point at `stow/<group>/` instead.

Consequence: **adding or removing an app requires no doc edit.** The directory's
presence (or absence) under its target group is the registration.

**This applies to negative facts too.** "`stow/root/` is empty", "there's only
one target group today", "X doesn't exist yet" are enumerations in disguise —
they go stale the moment a directory changes, exactly like the alacritty table.
Don't write the current contents (or emptiness) of `stow/<group>/` into prose;
point at the directory. This rule was itself violated: README, AGENTS and the
skill all called `stow/root/` "future/empty" for two days after `keyd` landed
there — and that blind spot is why a dangling `/etc/keyd` symlink went unnoticed.

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

`setup/` holds **per-machine runbook notes** so a fresh install can be rebuilt
and past decisions explained. Naming, the theme/step model, and writing style
live in `setup/README.md`.

- `setup/` lives **outside `stow/`**, so it is structurally not a stow package
  and its markdown can never be symlinked into `~/.config`.
- **A note documents only what stowing the repo does NOT already do** — i.e.
  out-of-band system actions (`pacman -S/-Rns`, enabling a *system* service). Config
  changes are applied by `stow`, so they live in the tracked files + git history,
  never as runbook steps. (The first `removing-alacritty` note wrongly listed
  "edit settings.json" and "git rm the package" — both already reproduced by the
  repo — leaving only `pacman -Rns alacritty` as a real step.)
- **The package *list* lives in `packages/*.txt`, not in notes.** The manifests
  are the one home of *what* is installed (`make install-packages` applies them,
  `make drift` diffs them against reality); each entry carries a short `# why`,
  citing its setup note where one exists. Notes keep the reasoning and the
  non-package steps only — a new install means a manifest line first, and a note
  only if there is more to say than "install it". The pacman manifest is split
  by provenance: `pacman-base.txt` is the *accepted* CachyOS-installer baseline
  (changes only when re-baselining) and `pacman.txt` is the deliberate
  additions — new installs always go in `pacman.txt`.
- **Repo-owned systemd *user* services are enabled by stow, never by runbook.**
  A package that owns a user unit also carries the enablement as a relative
  `.wants` symlink beside it — `.config/systemd/user/<target>.wants/<unit> ->
  ../<unit>` (see `stow/home/kdeconnect`, `stow/home/ssh-agent`) — so stowing
  *is* enabling, correct by construction. Runbook notes must not contain
  `systemctl --user enable` for a repo-owned unit; the only out-of-band steps
  are `systemctl --user daemon-reload` + a first `start` (or a re-login).
  Gotcha: stow refuses absolute symlinks inside packages, so a wants link
  cannot point at a vendor unit under `/usr/lib` — bring the unit into the
  package (author it, like kdeconnect, or track a copy of the vendor unit,
  like ssh-agent) and point the wants link at that.
- Keep dotfiles themselves free of transient/setup notes — that knowledge goes
  in `setup/`, not in config-file comments.
- It is a **runbook, not a changelog.** If a decision is reversed, update or
  delete the relevant note rather than appending contradictions. Then check
  whether README / other notes need the same correction (see the alacritty
  drift above — a change in one place often implies edits in others).

## Known gotchas

- **Hyprland runs the Lua (non-legacy) config parser.** Our config is
  `hyprland.lua` + `config/*.lua`, so the legacy `hyprctl` paths silently fail:
  `hyprctl keyword …` returns *"keyword can't work with non-legacy parsers. Use
  eval."* and `hyprctl dispatch <name>` (e.g. `renameworkspace`) is rejected.
  Drive the Lua API instead — `hyprctl eval "hl.monitor({…})"` to set config at
  runtime, `hyprctl dispatch "hl.dsp.<…>(…)"` for dispatchers. Both
  `scripts/rename-workspace.sh` and `scripts/display-scale.sh` hit this. (Also:
  Hyprland snaps fractional `scale` to its own 1/120 grid, so `display-scale.sh`
  steps a fixed ladder rather than computing exact scales — see its header.)
- **The `dpms` dispatcher is toggle-only — it ignores its on/off argument.**
  `hyprctl dispatch 'hl.dsp.dpms("on")'` (string form) toggles *every* monitor at
  once; the table form `hl.dsp.dpms({ monitor = "eDP-1" })` toggles just that one.
  Neither "sets on", so blindly calling either can blank an already-on screen and
  a mixed state can't be fixed with one call. To turn displays on idempotently,
  read each output's `dpmsStatus` and toggle only the ones that are off — that's
  what `scripts/displays-on.sh` does (first-line fix; hypridle's `after_sleep_cmd`
  calls it). `displays-on.sh reset` additionally reloads + reconciles the lid, for
  an output stuck disabled or at a bad mode. Runtime `hl.monitor` mode/scale evals
  only apply to an *enabled* output — on a disabled one they return "ok" but
  no-op; re-enable via `hyprctl reload` first.
- **Noctalia rewrites its own config.** `stow/home/noctalia/.config/noctalia/settings.json`
  is written by the running shell. Editing it on disk works, but if Noctalia is
  running it may overwrite your edit on its next settings-write. After editing,
  reload Noctalia (or log out/in) and re-verify with `grep`.
- **Noctalia only scans apps (and plugin code) at shell start.** Quickshell's
  desktop-entry service reads `XDG_DATA_DIRS`' `applications/` dirs once — a
  newly installed app (pacman or flatpak) won't appear in the launcher until
  the shell restarts (`Hyper+Backspace`). Same for edits to installed plugin
  files: its file watcher covers Noctalia's own config, not the plugins dir.
  The environment is NOT the problem — uwsm already exports the flatpak dirs
  in `XDG_DATA_DIRS` (verified against the running process, 2026-07-02).
- **hyprlock is the locker on every normal path.** Suspend, idle, and the
  Noctalia session menu (`Hyper+L`) all end at **hyprlock**, driven by
  **hypridle** (`hypr/.config/hypr/hyprlock.conf` + `hypridle.conf`, started from
  `autostart.lua`). Two settings make this hold and must stay in lockstep:
  Noctalia's `general.lockOnSuspend` is `false` (so it doesn't lock on the sleep
  path), and its session-menu **lock entry has a custom `command`,
  `loginctl lock-session`** (`sessionMenu.powerOptions[action=lock].command`).
  That custom command matters: `CompositorService.lock()` runs it and returns
  *before* activating Noctalia's own `WlSessionLock`, so the menu bypasses the
  in-shell locker entirely. `loginctl lock-session` emits the logind Lock signal,
  which hypridle answers with hyprlock. hyprlock being a separate process is the
  whole point — a Noctalia crash on resume can no longer strand the session on a
  black `ext-session-lock`, the failure mode that drove the switch (see
  `setup/deps-00-hyprlock-hypridle.md`). Clear the custom command or re-enable
  `lockOnSuspend` and you're back to the fragile in-shell locker.
- **Crashed Noctalia lock ⇒ stuck `ext-session-lock` (legacy path).** Noctalia's
  `WlSessionLock` is no longer reached in normal use (per above), but the failure
  mode is kept documented in case it's re-enabled: if Noctalia crashes *while
  locked* the compositor keeps the session locked (for security) with nothing
  rendering the password prompt — you land on Hyprland's bare "lock app died"
  recovery screen. The compositor is fine; only the locker died, so **do not log
  out or reboot** (you'd lose every running app). Recover with
  `scripts/restart_crashed_lock.sh`, run **from a text VT** (Ctrl+Alt+F3) since
  the GUI is locked. The on-screen hint Hyprland prints
  (`hyprctl keyword allow_session_lock_restore 1` → `dispatch exec hyprlock`)
  does *not* apply verbatim: the Lua parser rejects `keyword`/bare `dispatch`
  (use `eval` + `hl.*`, per the gotcha above), and the crashed locker is
  `qs -c noctalia-shell`, not hyprlock. The script does the Lua-native equivalent
  (set `misc:allow_session_lock_restore`, ensure the shell is up, re-present the
  lock); you then type your password to unlock normally.
- **`stow/home/noctalia/.config/noctalia/colors.json`** is regenerated on wallpaper/theme
  changes (matugen-style output). It is derived output, not config — the
  wallpaper/color settings in `settings.json` are the source of truth — so it
  is gitignored, untracked, and must stay that way (see "Runtime-rewritten
  tracked files" under working habits).
- **Two-terminal trap (resolved).** Hyprland's terminal is set in
  `stow/home/hypr/.config/hypr/config/defaults.lua` (`TERMINAL = "kitty"`); Noctalia's
  launcher uses `appLauncher.terminalCommand` in its `settings.json`. These are
  independent — changing one does not change the other. We standardized on
  **kitty** in both (see `setup/debloat-00-removing-alacritty.md`).
- **Third-party Noctalia plugins are untracked by design.** `.gitignore`
  default-denies `…/noctalia/plugins/`; the plugin manager owns that code and
  the tracked `plugins.json` is the registry. `!` opt-ins exist only for
  plugins whose source of truth is this repo — see
  `setup/noctalia-00-plugins.md` before adding one.
- **Secrets / machine-local state** must never be committed. See `.gitignore`
  (e.g. `fish_variables`). When adding a package, scan it for tokens/state
  before staging.
- **The `claude` package (`~/.claude`) is default-deny.** `.gitignore` ignores
  *everything* under it and opts in to config only (`settings.json`, `CLAUDE.md`,
  `commands/`, `agents/`, `skills/`, `hooks/`, `output-styles/`). `~/.claude`
  itself is a real dir (only `settings.json` is symlinked in); its
  `.credentials.json`, `projects/` (chat transcripts), history, and caches must
  **never** be tracked — never add an `!` rule that exposes them.
- **Empty dirs** are not tracked by git; that's expected, not a bug.

## Working habits in this repo

- **This repo is shared/published; write tracked content for a stranger.**
  Config comments, docs and commit messages are read by others, so keep them
  audience-neutral — no personal framing ("for my demos", "on my machine") and
  no machine-specific assumptions. The `setup/` style (terse; explain the
  non-obvious *why*, not the obvious) applies to **all** tracked prose, including
  config-file comments.
- **`make`** is the task entrypoint (run it with no target to list everything);
  the targets are thin wrappers over `bin/*` and `stow/stow-*.sh`.
- After any symlink/stow operation, run **`make check`** (`bin/doctor`) — it
  verifies every tracked leaf is a live symlink resolving into this repo (across
  all groups), that `stow --simulate` is conflict-free, that shell/JSON/Lua all
  parse, and that the git clean filters are wired up. It is read-only;
  **`make fix`** (`bin/doctor --fix`) restows and installs the repo-local
  filter config to repair. (Manual spot-check if needed: `ls -ld ~/.config/<app>`
  shows the symlink, `readlink -f` resolves it into this repo.)
- **`make drift`** (`bin/drift`, read-only) reports what the machine has that
  the repo doesn't — unmanaged `~/.config` entries and installed packages not
  declared in `packages/*.txt`. Drift is normal; run it to keep it *visible*.
  Triage package drift line by line — declare (with a `# why`) or remove —
  never by pasting a `pacman -Qqe` dump into the manifest.
- Secret hygiene is enforced by `.githooks/pre-commit` — gitleaks over the
  staged content (a mechanized grep when gitleaks is absent), blocking the
  commit on findings. It only runs via repo-local `git config core.hooksPath
  .githooks`; doctor verifies that wiring, `make fix` installs it. The
  underlying manual check it mechanizes:
  `git ls-files | grep -iE 'token|secret|fish_variables'`.
- **Runtime-rewritten tracked files are handled by mechanism, not vigilance.**
  When an app churns a file at runtime, classify it and wire up the matching
  defense — don't fall back to "remember not to commit it":
  - **Derived output** — regenerable from other tracked config (e.g. Noctalia's
    `colors.json`, rebuilt from the wallpaper/color settings): **gitignore it**.
    Derived ≠ source of truth; tracking it only records churn.
  - **Merged config** — a file this repo owns that the app also rewrites
    runtime keys into (e.g. Claude Code's `settings.json`, where it
    inserts/updates `model` etc. and reorders keys wholesale): a **git clean
    filter**. Three parts, one name: `.gitattributes` maps the file to
    `filter=<name>`; the tracked script `bin/filters/<name>` strips the
    runtime keys and normalizes key order (it is the single source of truth
    for the strip-list); and a repo-local
    `git config filter.<name>.clean bin/filters/<name>` activates it. Runtime
    churn then never shows as dirt, while real config edits still do. The git
    config is repo-local (not versionable), so a fresh clone needs it once —
    see README's new-machine steps; doctor validates it is in place and
    `make fix` (doctor `--fix`) installs it.
- **Avoid `git add -A`** even so — apps like Noctalia rewrite tracked config
  that (unlike the cases above) *is* the source of truth and can't be filtered
  or ignored, so a blanket add still bundles unrelated churn into your commit.
  Stage explicit paths and review `git status` first; if churn lands in the
  wrong commit, split it (unpushed history is safe to tidy).
- System-level actions (`pacman -Rns`, `pacman -S`) need sudo — hand the user
  the exact command rather than running it, and record it so it is
  reproducible: installs as a `packages/*.txt` line (with a `# why`),
  removals and other system actions in the relevant `setup/` note.
