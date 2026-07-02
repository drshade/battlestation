# PLAN — critical review & improvement plan

A working document: findings from a structural review (2026-07-02), ordered by
priority. Each item states the problem, the evidence, and the proposed fix.
Delete items as they land (this is a plan, not a changelog).

## What already works — don't disturb

- The stow-by-target-group model, privilege-split drivers, and the
  no-ignore-list / no-package-table rules are sound. Nothing below changes them.
- `setup/` as an out-of-band runbook is the right split. The gaps are in
  *enforcement tooling* — several AGENTS.md rules are currently upheld by
  convention ("remember to…"), which is exactly what principle #1 says to avoid.

---

## P1 — Structural correctness

### 1. Stow folding is a live hazard: use `--no-folding` — ✅ DONE (2026-07-02)

**Problem.** Stow "folds": if the target dir doesn't exist, it symlinks the
*directory* into the repo instead of creating a real dir with symlinked files.
Two consequences today:

- `~/.config/environment.d` is a symlink into the **ssh-agent** package.
  Anything else that writes an env file (another package, a system tool) writes
  *into the repo* under the wrong package.
- `~/.claude` is a real dir only by accident of history (it existed before
  stowing). On a **fresh machine**, stowing `claude` first would fold `~/.claude`
  into the repo — Claude Code would then write `.credentials.json`, transcripts
  and caches *physically inside the repo tree*. `.gitignore` prevents commits,
  but the secrets would live in a directory that gets backed up, published-adjacent,
  and `rm -rf`'d as "just a repo checkout".

**Fix.** Add `--no-folding` to both drivers (`stow/stow-home.sh:15`,
`stow/stow-root.sh:19`). Real dirs + per-file symlinks: apps writing siblings
write to the real filesystem, never into the repo. Restow once after the change
(stow will unfold existing folds on `--restow`). Update AGENTS.md's stow-model
section in the same commit. This makes the `claude` gitignore's "never track
state" promise correct by construction instead of by accident.

**Outcome.** `--no-folding` added to both drivers; AGENTS.md stow-model section
updated with a "Why `--no-folding`" bullet (and corrected the adjacent line:
targets are real dirs whose *files* are symlinks). Restowed all 10 home
packages; `~/.config/environment.d` is now a real dir with a per-file symlink —
fold gone. Scanned `~/.config/*`: no directory symlinks into the repo remain.
`stow --simulate --restow` clean for every package. **Correction (review):**
`stow/root/` is not empty — `keyd` lives there, and `/etc/keyd/default.conf` was
found dangling (still pointing at the pre-rename `~/dev/cachyos-dotfiles` path;
keyd only works because the daemon holds the old config in memory). Fixed by the
user (removed the stale link, re-ran `stow-root.sh`, restarted keyd). Doctor
(P2.4) must still scan root targets for dangling links — this exact failure was
invisible for two days.

### 2. Stop vendoring third-party Noctalia plugins

**Problem.** `keybind-cheatsheet`, `polkit-agent`, `screen-toolkit` (~2.5 MB,
~110 files, incl. 20 i18n files each, `preview.png`s, a compiled `.qsb` shader)
are third-party code from `noctalia-dev/noctalia-plugins`, tracked wholesale.
This duplicates a fact that is derivable (violates "one source of truth"):
`plugins.json` *already* records the source repo and the enabled-plugin set, and
Noctalia's plugin manager reinstalls from it. Every upstream update will land as
a giant meaningless diff — the same churn class as `colors.json`, ×100.

**Per-plugin status** (verified against the `~/dev/legacy-v4-plugins` fork,
2026-07-02):
- `polkit-agent` — byte-identical to upstream. Pure vendoring; de-vendor now.
- `screen-toolkit` — identical except one README line (deps: `wl-screenrec`
  from pacman instead of `wl-screenrec-git` from AUR — CachyOS ships it in
  repos). De-vendor; optionally PR the README fix upstream first.
- `keybind-cheatsheet` — **deliberately patched**: it is exactly the union of
  the two open PR branches (`feat/keybind-cheatsheet-modifier-labels`,
  `fix/merge-sequential-hyprctl-binds`), so nothing vendored is unpushed.
  Keep it vendored *while the PRs are open*; the tracked copy is the deployed
  form of pending upstream work.

**Fix.**
- Gitignore `plugins/*` under the noctalia package, with `!` opt-ins for
  plugins that are sources of truth here: `claude-workspaces` (homegrown,
  `author: tom`) and — temporarily — `keybind-cheatsheet`. Same default-deny
  pattern as the `claude` package.
- Record the keybind-cheatsheet exception in a `setup/` note naming the two
  PRs, so "de-vendor once merged" has a trigger and the exception can't
  silently become permanent.
- `plugins.json` (tracked) remains the declarative registry; the same note
  covers: after stowing, install enabled plugins via Noctalia's plugin manager.
- Longer term consider promoting `claude-workspaces` to its own repo and
  treating this copy as an install location.

### 3. Runtime-rewritten configs: replace the "avoid `git add -A`" convention with tooling

**Problem.** Noctalia rewrites `settings.json`; Claude Code rewrites
`settings.json` (a `/model` selection dirtied it *today*); `colors.json` churns
on every wallpaper change. The current defense is a working-habits paragraph in
AGENTS.md — correct-by-convention, the class of fix the repo's own principles
reject.

**Fix (layered):**
- `colors.json`: gitignore it. It's derived output (matugen-style), not config —
  AGENTS.md already half-suggests this. Derived ≠ source of truth.
- Add a `churn` check to the doctor script (P2.4): list tracked files that are
  dirty *and* known to be runtime-rewritten (a small annotated list in the
  script), so `make check` says "settings.json is dirty — intentional?" before
  any commit.
- Optionally a pre-commit hook (P2.6) that blocks staging a known-churn file
  unless the commit message / an env var acknowledges it.

---

## P2 — Missing tooling (the "sticky tape" replacement)

The repo has good rules and zero enforcement. One entrypoint + three small
scripts close the gap. Keep them at `bin/` in the repo root (repo tooling, not
stowable content — same reasoning as the driver scripts living outside groups).

### 4. `bin/doctor` — verify the deployed state

Checks, each a few lines:
- every package in every group is actually stowed: for each leaf, target exists,
  is a symlink, `readlink -f` resolves into this repo (mechanizes the manual
  `ls -ld` habit in AGENTS.md);
- `stow --simulate --restow` runs clean for every package (catches conflicts);
- shellcheck over `**/*.sh` (12 hypr scripts and growing — currently unlinted);
- parse checks: `jq .` every tracked `*.json`, `luacheck`/`lua -e loadfile` the
  hypr Lua tree (a typo in `keybinds.lua` today is only caught by Hyprland
  failing live);
- known-churn dirty-file report (P1.3).

### 5. `bin/drift` — make the "living repo" claim inspectable

The repo's stated purpose is *visibility* of the machine's customisation, but
there is no way to see what the machine has that the repo doesn't:

- list `~/.config/*` (and `~/.local/share/applications`, `/etc` candidates)
  that are **not** repo-managed — today that's ~24 dirs incl. real config like
  `gtk-3.0`, `qt6ct`, `teams-for-linux`, `micro`, `uwsm`;
- diff installed packages against the declared manifest (P3.7):
  `pacman -Qqe` vs `packages/pacman.txt`, same for AUR/flatpak.

Output is a triage list, not an error — drift is normal; invisible drift is the
bug. (An in-script "known, deliberately unmanaged" list is acceptable here: it
is a *report filter*, not a correctness mechanism.)

### 6. Enforced secret hygiene

AGENTS.md says "before committing, grep for secrets" — convention again.
- Track hooks in-repo: `.githooks/pre-commit` + one-time
  `git config core.hooksPath .githooks` (add to the new-machine steps).
  Hook = gitleaks (or the existing grep, mechanized) over *staged* content, plus
  the churn-file guard from P1.3.
- Optional: a GitHub Action running gitleaks + shellcheck + `stow --simulate`
  on push, since the repo is published.

### 7. A `Makefile`/`justfile` as the single entrypoint

`make stow`, `make check` (doctor), `make drift`. Self-documenting surface for
"what can I do in this repo" — today that knowledge lives only in prose. Targets
just call `bin/*` and `stow/stow-*.sh`; no logic in make itself.

---

## P3 — Coverage gaps

### 8. Declarative package manifest

**Problem.** Installed software is recorded only as imperative `pacman -S` lines
scattered through `setup/` notes. A rebuild means re-reading every note; there
is no way to ask "is the machine's package set what the repo says?".

**Fix.** `packages/pacman.txt` (+ `aur.txt`, `flatpak.txt` as needed) — one
name per line, comments for *why*. `bin/drift` diffs them against reality;
a `make install-packages` target applies them. `setup/` notes then carry only
the non-package steps (service enablement, key generation) and the reasoning —
the *list* has one home. Seed the lists from the existing setup notes +
`pacman -Qqe` triage.

### 9. Standardize systemd user-service enablement

kdeconnect is enabled via a stow-managed `graphical-session.target.wants/`
symlink (correct by construction — commit 687337c); ssh-agent is enabled via a
`systemctl --user enable` runbook step. Two mechanisms for the same fact. Adopt
the wants-symlink pattern for every user service this repo owns, and document it
in AGENTS.md as *the* way; the runbook keeps only genuinely out-of-band actions.

### 10. Machine-local overlays are currently invisible

`monitors_local.lua` is gitignored — per-machine truth that is *unversioned and
unrecoverable*, on a machine whose display recovery was tricky enough to earn
two gotcha entries. Options, in increasing weight:
- (a) track `monitors_local.lua.example` + a doctor check that warns when the
  real file is missing;
- (b) a per-host stow group (`stow/home@<hostname>/`) once a second machine
  exists — don't build this speculatively.
Pick (a) now; it also documents the expected schema.

---

## P4 — Hygiene

### 11. Cross-cutting scripts live inside the hypr package

`claude-ws-status.sh` (called from the **claude** package's hooks),
`claude-usage.sh`, and `ws.sh` (shared with the **noctalia** claude-workspaces
plugin) sit in `hypr/.config/hypr/scripts/` — cross-package coupling on hypr
internals, wired by absolute paths in `keybinds.lua` and `settings.json`.
Add a `stow/home/bin/` package (`.local/bin/`, already on PATH) for scripts
consumed by more than one package; hypr-only helpers stay put. Migrate the
three above; update the referencing paths in the same commit.

### 12. Published-repo polish — ✅ DONE (2026-07-02)

- **`scratch/` is tracked.** `keybinds.md` is a genuine design doc — move it to
  `docs/` (or `setup/` as a design note) and gitignore `scratch/`; a published
  repo shouldn't ship its scratch space ("write tracked content for a stranger").
- **README TODO section** — move to a tracked `TODO.md` or GitHub issues; the
  README is the front page for strangers. Note one TODO ("designing a great
  keymap") is already underway in `scratch/keybinds.md` — link them.
- **No LICENSE.** The repo is "published as a reference for others" and vendors
  MIT-licensed plugin code; add a LICENSE (MIT fits) — also legally required for
  the vendored code while P1.2 is pending.

**Outcome.** `git mv scratch/keybinds.md docs/keybinds.md`; `scratch/` gitignored
and now empty/untracked. README's raw `# TODO` block replaced with an "Open work"
section linking `TODO.md` (new) + `PLAN.md`; the three TODO items moved into
`TODO.md`, with the keymap item linking `docs/keybinds.md`. Added `LICENSE` (MIT,
"Copyright (c) 2026 drshade") and a License section in the README.

### 13. Stale "stow/root is future/empty" prose misled every agent — ✅ DONE (2026-07-02)

`README.md:16` ("future: stow/root/"), `README.md:49` ("no-op until stow/root/
exists") and `AGENTS.md:79-80` ("once stow/root/ has packages") were written
when root *was* empty (fbd09a4) and never updated when keyd landed (27731fd).
Consistent-but-wrong prose across README + AGENTS + the skill (which mentions
only `stow/home/`) is why the P1.1 outcome claimed root was empty — and why the
dangling `/etc/keyd` link went unnoticed. Same bug class as the alacritty table
drift the source-of-truth rule was written for, in negative form.

**Fix.** Delete the three qualifiers; make SKILL.md point at `stow/<group>/`
generally; extend AGENTS.md's source-of-truth rule to negative facts ("X is
empty / doesn't exist yet" is also an enumeration — don't write it in prose).

**Outcome.** README: dropped the "(future: stow/root/)" tree comment (now shows
`root/` as a real group), the "(no-op until stow/root/ exists)" qualifier, and
the "only one target today" phrasing (now cites keyd as the live root config).
AGENTS.md: dropped "once `stow/root/` has packages"; added a "negative facts too"
paragraph to the source-of-truth rule, citing this exact miss. SKILL.md: intro,
"what's configured", and making-changes now say `stow/<group>/` and `ls
stow/*/`, naming both `home/` and `root/`. (The live `/etc/keyd` dangling link
flagged in the P1.1 correction has since been fixed by the user.)

### 14. Known follow-ups already on record

- Flatpak apps missing from the launcher (README TODO) — likely `XDG_DATA_DIRS`
  not including `/var/lib/flatpak/exports/share` in the uwsm/Hyprland env;
  fix belongs in the hypr `environment.lua` + a setup note.
- Keyboard repeat/delay (README TODO) — one line in `input.lua`.
- GTK (`gtk-3.0/4.0`), Qt (`qt5ct/qt6ct`, `xsettingsd`) theming is unmanaged —
  odd gap for a theming-centric setup; adopt once themes are deliberate.

---

## Suggested sequencing

1. ✅ **P1.1** `--no-folding` (small diff, removes the standing hazard) —
   then restow + verify.
2. **P1.2 + P1.3** de-vendor plugins, gitignore `colors.json` (kills ~90% of
   future diff noise).
3. **P2.7 + P2.4** Makefile + doctor (the enforcement spine); fold shellcheck
   fixes in as they surface.
4. **P2.6** hooks; **P2.5 + P3.8** drift + package manifest (one feature —
   drift needs the manifest to be useful).
5. **P3.9, P3.10, P4** as individual small commits.

Each step updates AGENTS.md/README in the same commit where it changes a rule
(per principle #4).
