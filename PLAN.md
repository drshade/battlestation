# PLAN — bsctl trial & roadmap

A working document. The previous plan (the 2026-07-02 structural review, all
items completed) is retired to git history — see commit `c2e56f6` and earlier.
Same conventions as before: items state the problem/intent and the proposed
move; completed items get a short **Outcome** and are pruned once their
lessons stop mattering.

## Context: what bsctl is

`ctl/` builds `~/.local/bin/bsctl` (`make build`), a Rust binary that owns the
claude-ws status protocol end to end: `bsctl hook <verb>` behind all Claude
Code hooks, `bsctl poll` behind the claude-workspaces widget. Ownership rule
(AGENTS.md): bsctl owns stateful protocols and JSON-heavy logic; shell owns
glue and ALL recovery paths. `claude-ws-status.sh` remains as the executable
protocol reference and instant rollback while the trial runs.

## 1. Trial: live with bsctl (started 2026-07-03)

Phase 1+2 shipped: protocol-identical port (18/18 parity matrix), consumers
switched, ~31× faster per hook event, plus one behavior the sh version lacked
(marker self-heal after the SubagentStart/meta.json race).

**Evaluate over a week or two:**
- Widget correctness day-to-day — bot statuses, sub-bot lifecycles, tooltips,
  no stale/ghost bots.
- Build-step friction — does the doctor staleness check + `make fix` rebuild
  loop feel acceptable, or does the compile step get in the way of the
  live-hackability the scripts had?
- Anything the parity matrix missed.

**Rollback** (if it disappoints): repoint the 10 hook commands in claude
`settings.json` at `claude-ws-status.sh`, revert the widget's Process command
to the embedded python (git history has it). One commit.

**Outcome.** Passed after one day of live use ("really happy, working well"
— user, 2026-07-03); no widget misbehavior, no parity gaps surfaced, the
staleness/`make fix` loop proved comfortable. Proceeded to phase 3 same day.

## 2. On trial success: phase 3 — `bsctl ws` + `bsctl usage` — ✅ DONE (2026-07-03)

- Port `ws.sh` (workspace position↔id mapping over the persisted order file —
  hot path on every workspace keybind, protocol shared with the widget) and
  `claude-usage.sh` (JSON parsing for the usage indicator). Consumers to
  repoint: `keybinds.lua`, claude-workspaces QML (Panel/BarWidget/
  UsageIndicator).
- **Adopt clap at this point** — phase 3 is when bsctl grows human-typed
  commands with arguments worth validating; `clap_complete` gives fish
  completions. Not before: for two machine-called subcommands the extra
  ~15 crates only slow the `make fix` rebuild path.
- Retire `claude-ws-status.sh`, `ws.sh`, `claude-usage.sh` once their
  consumers are switched and the protocol reference moves to ctl/ docs.

**Outcome.** `bsctl ws` (verb-for-verb, byte-identical Lua dispatch strings,
32/32 parity incl. the script's exit-1 off-the-end quirk and textual
pref-token matching) and `bsctl usage` (ttl/flock semantics preserved, 10/10
parity, banker's-rounding replicated; one deliberate improvement — the OAuth
token now travels on curl's stdin, never argv where /proc exposed it).
clap adopted (color/suggestions features dropped to keep rebuilds fast;
Cargo.lock 12→20 crates, binary 604K→1.3M); `bsctl completions` +
`bin/build`-generated fish completions. Consumers switched: `keybinds.lua`,
BarWidget/Panel/UsageIndicator QML. All three scripts retired and the empty
`bin` stow package dissolved with them; protocol contracts now live in
`ctl/src/lib.rs`. 59 tests; live goto round-trip verified through the real
compositor.

## 3. `bsctl display` — display state visibility & management

Display management is the machine's roughest edge (two gotcha entries, a
dpms-toggle quirk, and a lid/reload interaction that stranded the pointer on
an invisible panel — fixed fb27c0a, but diagnosed by hand-rolled jq). The
primitives are scattered: `hyprctl monitors -j` (dense JSON), clamshell.sh,
displays-on.sh, display-scale.sh, monitors_local.lua. Proposed:

- **Foundation: a Hyprland IPC module** (`ctl/src/ipc.rs`) — talk to
  `.socket.sock` directly (`std` UnixStream, zero new crates): `j/<query>` +
  `dispatch <cmd>`, instance discovery via `$HYPRLAND_INSTANCE_SIGNATURE`
  with a newest-instance-dir fallback (VT contexts). **Socket-first,
  hyprctl-fallback**: on connect/protocol failure, fall back to spawning
  hyprctl (which always matches the running compositor on a rolling
  release). All existing call sites (`hook`'s clients lookup, `ws`) migrate
  onto it; `.socket2.sock` (the event stream) is what `bsctl watch` and
  event-driven display reconciliation will build on later.
- `bsctl display status` — one readable table: per output enabled/dpms/mode/
  scale/position/description, plus lid state and a WARNING line when state is
  inconsistent (internal panel enabled while lid closed, dpms mixed, an
  output stuck at a non-native mode).
- `bsctl display on|off <output>` — safe dpms targeting (the table-form
  toggle + read-before-toggle logic from displays-on.sh, per the gotcha).
- `bsctl display reset` — the displays-on.sh reset flow: reload + reconcile
  lid + re-assert modes.
- Boundary with the recovery policy (AGENTS.md): bsctl display is the
  *human/diagnostic* tool. The two automated hooks stay tiny shell shims —
  hypridle's `after_sleep_cmd` and the lid binds — and
  `restart_crashed_lock.sh` stays shell forever. Once bsctl display has
  earned trust the shims may *delegate* to it, but each keeps a
  last-ditch pure-shell fallback path.
- Subsumes the parked `display-scale.sh` port (`bsctl display scale
  up|down|reset`) — same domain, same state files.

## 4. Ideas parked behind the above

- `bsctl watch`: a small daemon inotify-watching the state dir, maintaining
  one consolidated JSON the widget observes via Quickshell `FileView` —
  event-driven bar (sub-bots appear the instant a hook writes) instead of the
  2s poll. With the IPC module it can also subscribe to `.socket2.sock`
  events (workspace/monitor changes) — the real-time primitive everything
  else wants.
- **Multi-monitor workspace model** (requirements captured 2026-07-03,
  design-for-now/implement-later): today all 10 bound workspaces live on
  monitor 1; a second monitor spawns ws 11-20 which have no keybinds and no
  place in the display order. Wanted: workspace↔monitor assignment as a
  first-class `bsctl ws` concept — e.g. "send workspace 3 to display 2"
  (`hl.dsp`/`moveworkspacetomonitor`), per-monitor display orders (the order
  file may need a schema step), and keybinds that address positions across
  monitors. DESIGN CONSTRAINT for anything touching the ws protocol or the
  order file now: don't bake in the single-monitor assumption harder than it
  already is.
- `display-scale.sh` port (`bsctl display scale up|down|reset`) — remaining
  piece of item 3's domain once the core lands.

## Parked (non-bsctl)

- Promote the `claude-workspaces` plugin to its own repo; this copy becomes an
  install location. If that happens, keep the widget's bsctl dependency
  optional (the state-file protocol is simple enough to consume directly).
- De-vendor `keybind-cheatsheet` when upstream PRs merge — trigger and links
  live in `setup/noctalia-00-plugins.md`. (https://github.com/noctalia-dev/legacy-v4-plugins/pull/937)
- Optional CI (gitleaks + shellcheck + `stow --simulate` + `cargo test` on
  push) — the repo is published.
- Drift triage of remaining unmanaged `~/.config` entries as they catch the
  eye; luacheck install if doctor's Lua linting ever needs to be real.
