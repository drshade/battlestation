# PLAN — roadmap

A working document: open items only, with a short log of recently-completed
arcs kept until their lessons stop mattering. Full histories live in git
(structural review: through `c2e56f6`; bsctl build-out and the event-driven /
multi-harness arcs: the commit log of 2026-07-03).

## Where things stand (2026-07-04)

`ctl/` builds `bsctl` — the single owner of the desktop's stateful protocols
(contracts in `ctl/src/lib.rs`): the multi-harness **battlestation-ws** status
protocol (`hook --kind <harness>` / `poll` / `watch`), workspace display
order incl. display-level verbs, the sparse workspace→display preference
(stamped only by `ws prefer`/`ws movetodisplay`, applied when an output
arrives; `ws prefs`/`forget`/`reconcile`), display management
(status/dpms/reset/scale), and the usage cache. The **battlestation-workspaces** plugin is pure
presentation: `bsctl watch` (flock-elected, hot-standby failover) folds agent
hooks AND Hyprland's `.socket2.sock` events into one atomic `.widget.json`;
the widget FileView-watches it — no polling, no compositor-snapshot reads,
~50ms event-to-pixel. Three harnesses live in production: Claude Code, Codex
and Antigravity (hooks in each harness's stowed config; per-kind presentation via the
registry in `Cfg.qml` — adding a harness = one registry entry + one asset dir
+ one hooks config).

## Open

1. **Next harness: opencode.** The recipe is proven twice (Codex, then
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

- **Workspace→display preference (2026-07-04):** a workspace CAN prefer an
  output; most never do — sparse by design. Only two writers, both
  explicit homing acts: `ws prefer` and `ws movetodisplay`; reorders, bulk
  operations, watch and Hyprland never stamp. Applied when the output
  arrives (`bsctl watch` on monitoraddedv2, its only dispatch power;
  `ws reconcile` manually; `prefs`/`forget` inspect and drop). Unplug is a
  non-event: Hyprland evacuates, bsctl does nothing. Protocol in lib.rs;
  the `preferred` file sits beside `order` (intent outlives boots). Two
  lessons shaped this arc. First: the initial implementation snapshotted
  assignments AT REMOVAL and grew two live-verified races for it
  (Hyprland's evacuation burst outruns the removal event past any coalesce
  window, and the flock winner tends to die WITH the monitor its bar sits
  on) — machinery that writing at intent time deletes outright. Second:
  stamp policy creep — bulk stamps (reorder/save-layout) quietly made
  EVERY workspace managed; the goal was homing a few, with Hyprland doing
  the rest. `ws swapdisplays` (+ its HYPER+F(9+n) binds) was dropped in
  the same trim as not worth its complexity. Position addressing, HYPER+Fn
  display moves and global numbering were already in place; per-monitor
  numbering stays rejected (user disclaimer: may revisit after living with
  it). Accepted wart stands: slide-animation direction / 4-finger gesture
  follow raw id order and can disagree with pill order after drags.
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


# bsctl improvements
- we should have better ergonomics, e.g.:
  # this set of commands + switches allows for many combinations:
  #   switch focus to other workspace -                   'ws focus --ws <x>'
  #   move window to other workspace -                    'ws focus --ws <x> --active-window-follows'
  #   switch focus to other display -                     'ws focus --display <d>'
  #   move window to active workspace on other display -  'ws focus --display <d> --active-window-follows'
  #   move workspace to other display (keeps ws-id) -     'ws focus --display <d> --active-workspace-follows'
  #   switch focus to left/right workspace -              'ws focus --ws-rel +1'
  #                                                       'ws focus --ws-rel -1'
  #   move window to left/right workspace -               'ws focus --ws-rel +1 --active-window-follows' #                                                       'ws focus --ws-rel -1 --active-window-follows'
  # '--active-workspace-follows' really should only work with --display-id - doesn't make sense, unless we try and merge or something (but i suspect that is unexpected behaviour)
  #
  # some nomenclature:
  #   <bs-id>         -> the battlespace id (which maps one-to-one to a hyprland workspace id)
  #   <ws-id>         -> the hyprland workspace id 
  #   <display-id>    -> the hyprland display id (1, 2, 3) from left to right
  #   <display-name>  -> the hyprland display name (e.g. 'eDP-1', 'DP-1')

  bsctl ws focus [--bs-id <bs-id>|--bs-rel <+num|-num>|--ws-id <ws-id>|--display-id <display-id>]
  bsctl ws send window [--bs-id <bs-id>|--bs-rel <+num|-num>|--ws-id <ws-id>] [--focus]                  # send active window to workspace, optionally focusing too
  bsctl ws send workspace --display-id <display-id> [--focus]     # send active workspace to display, optionally focusing too

  # Allow getting, setting and removing names flexibly
  bsctl ws name get [--all|--bs-id <bs-id>|--ws-id <ws-id>|--display-id <display-id>] # default --all
  bsctl ws name set [--bs-id <bs-id>|--ws-id <ws-id>] --name <name>
  bsctl ws name rm [--all|--bs-id <bs-id>|--ws-id <ws-id>|--display-id <display-id>]

  # Query workspaces order (bs-id to ws-id mapping), optionally filtered by display?
  bsctl ws map get  # needs thought on what this should return based on who queries it?
  bsctl ws map set  # needs thought on how this should look, but needs to allow for moving order of workspaces (bs to ws map)

  # preferences
  bsctl ws prefs get [--all|--bs-id <ws-id>|--ws-id <ws-id>] # default --all
  bsctl ws prefs add [--bs-id <bs-id>|--ws-id <ws-id>] --display-id <display-id>
  bsctl ws prefs rm [--bs-id <bs-id>|--ws-id <ws-id>|--all]
  bsctl ws prefs reconcile

  # displays
  bsctl display get [--all|--display-id <display-id>] # default --all
  bsctl display set dpms --display-id <display-id> [--on|--off]
  bsctl display set scale --display-id <display-id> [--reset|--value <scale-value>] 

  # agent sessions
  bsctl agents set --kind <harness> [waiting|thinking|tooling|clear|subagent-start|subagent-stop] [--session-id <session-id>]
  bsctl agents get [--kind <harness>] [--all|--session-id <session-id>] # default --all

  # for any of the above commands which are queried programmatically (maybe by widget or by hyprctl) we should add a --format [text|json|csv|whatever_makes_sense]
  # and my ideal is that anything that queries workspaces, agent sessions etc should really be invoking bsctl, and not reading the underlying files directly (and bsctl should manage the file lock to ensure that concurrent reads/writes are correctly locked).
  # i would prefer that nothing directly watches the files underneath - but rather poll or called 'bsctl watch' for changes (and we need to discuss whatever functionality this should have to make it easy), and we republish the full state of the world to that subscriber on each change through an output stream. For this - do we add a --stream switch to those queries which need them e.g. 'bsctl agents get --all --format json --stream' will write the state to stdout, and continue to write on each update.



