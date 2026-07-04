# PLAN — roadmap

A working document: open items only, with a short log of recently-completed
arcs kept until their lessons stop mattering. Full histories live in git
(structural review: through `c2e56f6`; bsctl build-out and the event-driven /
multi-harness arcs: the commit log of 2026-07-03).

## Where things stand (2026-07-03)

`ctl/` builds `bsctl` — the single owner of the desktop's stateful protocols
(contracts in `ctl/src/lib.rs`): the multi-harness **battlestation-ws** status
protocol (`hook --kind <harness>` / `poll` / `watch`), workspace display
order incl. display-level verbs, display management (status/dpms/reset/scale),
and the usage cache. The **battlestation-workspaces** plugin is pure
presentation: `bsctl watch` (flock-elected, hot-standby failover) folds agent
hooks AND Hyprland's `.socket2.sock` events into one atomic `.widget.json`;
the widget FileView-watches it — no polling, no compositor-snapshot reads,
~50ms event-to-pixel. Three harnesses live in production: Claude Code, Codex
and Antigravity (hooks in each harness's stowed config; per-kind presentation via the
registry in `Cfg.qml` — adding a harness = one registry entry + one asset dir
+ one hooks config).

## Open

1. **Multi-monitor: workspace-assignment memory (design settled 2026-07-04).**
   The original "deeper half" mostly dissolved: position addressing already
   reaches spawned ws 11+, HYPER+Fn moves workspaces between displays, and
   per-monitor NUMBERING was considered and rejected — global positions stay
   (a workspace's number never changes when it migrates; SUPER+N is a
   teleport, not a local switch). User disclaimer: hasn't lived with global
   numbering long; may revisit. What remains is the real daily gap:
   **workspace→monitor assignment lives nowhere**, so undock/lid-close
   evacuates display 2's workspaces and REPLUG never restores them.

   Plan (one contained job): `bsctl watch` already sees every
   moveworkspace/monitorremoved/monitoradded on socket2 and holds the live
   assignment map — persist it. On `monitorremoved`, snapshot which ids
   lived there; on `monitoradded` of a known monitor, dispatch them back
   (reuse ws::restore_plan — the scale-guard machinery, verbatim). Plus a
   manual `bsctl ws reconcile` verb. Deliberate design point: this gives
   watch dispatch powers for the first time (today it is a passive mirror) —
   scope them to exactly this trigger and document in lib.rs's contract.
   Snapshot-on-removal (not continuous persistence) keeps transient
   evacuations (the scale-bug class) out of the memory. Order file
   untouched; no schema change. Accepted wart stands: slide-animation
   direction / 4-finger gesture follow raw id order and can disagree with
   pill order after drags.

2. **Next harness: opencode.** The recipe is proven twice (Codex, then
   Antigravity — which surfaced one real integration cost each: Codex's
   trust gate, agy's conversationId payloads): investigate the harness's
   hook/notify surface, wire its config to `bsctl hook --kind <k> <verb>`,
   add a registry entry + asset dir. Unknown-kind sessions already render
   as claude, so partial integration is safe at every step. (gemini-cli is
   dead — Google folded it into Antigravity, which even squats its
   `~/.gemini` config dir; already integrated, kind `agy`.)

## Parked

- Promote `battlestation-workspaces` to its own repo; this copy becomes an
  install location. Keep the widget's bsctl dependency optional (the
  state-file protocol is simple enough to consume directly).
- De-vendor `keybind-cheatsheet` when upstream PRs merge — trigger and links
  in `setup/noctalia-00-plugins.md`
  (https://github.com/noctalia-dev/legacy-v4-plugins/pull/937).
- Optional CI (gitleaks + shellcheck + `stow --simulate` + `cargo test` on
  push) — the repo is published.
- Drift triage of remaining unmanaged `~/.config` entries as they catch the
  eye; luacheck install if doctor's Lua linting ever needs to be real.
- Shell shims (`displays-on.sh`, clamshell binds) may delegate to
  `bsctl display` once it has long-term trust — each keeps a pure-shell
  fallback (AGENTS.md recovery boundary).

## Recently completed (prune once absorbed)

- **Event-driven endgame (2026-07-03):** `bsctl watch` subscribes to
  `.socket2.sock`; `.widget.json` = `{sessions, compositor}`; the widget
  dropped its settle taps, refresh nudges, occupancy timer and both
  Quickshell staleness workarounds. Lesson kept: Noctalia's service layer
  snapshots miss property-only changes and never refreshes monitors —
  upstream bugs worth reporting someday.
- **Multi-harness (2026-07-03):** state dir → `battlestation-ws`;
  `--kind` mandatory (kindless caller = outdated wiring = silent no-op);
  pid detection matches comm==kind; titles use a 4-step sticky precedence
  (Codex transcripts carry no title). Codex live: hooks in the stowed
  `~/.codex/config.toml` (trust-table churn handled by a TOML section
  whitelist filter), spark/bracket/pause faces, sub-bots inherit the
  session's kind. Plugin renamed `battlestation-workspaces`; order file
  migrated alongside.
- **Scale hardening (2026-07-03):** three single-monitor-era bugs — position
  "auto" re-placement, neighbor workspace evacuation, missing neighbor
  re-flow — fixed with pinned positions, snapshot-and-restore, and
  absolute-target reflow (idempotent against Hyprland's own). Lesson:
  Hyprland re-flows auto-positioned monitors on SOME runtime changes but not
  others; never apply relative deltas to observed state.
