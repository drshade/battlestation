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

## 2. On trial success: phase 3 — `bsctl ws` + `bsctl usage`

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

## 3. Ideas parked behind phase 3

- `bsctl watch`: a small daemon inotify-watching the state dir, maintaining
  one consolidated JSON the widget observes via Quickshell `FileView` —
  event-driven bar (sub-bots appear the instant a hook writes) instead of the
  2s poll.
- `display-scale.sh` port (the other sh+embedded-python hybrid, per-monitor
  state files) — second wave, only after bsctl has earned trust. Recovery
  scripts (`restart_crashed_lock.sh`, `displays-on.sh`, `clamshell.sh`) stay
  shell permanently, per the AGENTS.md policy.

## Parked (non-bsctl)

- Promote the `claude-workspaces` plugin to its own repo; this copy becomes an
  install location. If that happens, keep the widget's bsctl dependency
  optional (the state-file protocol is simple enough to consume directly).
- De-vendor `keybind-cheatsheet` when upstream PRs merge — trigger and links
  live in `setup/noctalia-00-plugins.md`.
- Optional CI (gitleaks + shellcheck + `stow --simulate` + `cargo test` on
  push) — the repo is published.
- Drift triage of remaining unmanaged `~/.config` entries as they catch the
  eye; luacheck install if doctor's Lua linting ever needs to be real.
