# PLAN — roadmap

A working document: open items only, with a short log of recently-completed
arcs kept until their lessons stop mattering. Full histories live in git
(structural review: through `c2e56f6`; bsctl build-out and the event-driven /
multi-harness arcs: the commit log of 2026-07-03).

## Where things stand (2026-07-04)

`ctl/` builds `bsctl` — the single owner of the desktop's stateful protocols
(contracts in `ctl/src/lib.rs`), one grammar over a fixed nomenclature
(bs-id = battlespace position, ws-id, display-id/-name): `status` (the
world — text for humans, `--format json --stream` for subscribers),
`ws focus`/`ws send` (all movement), `ws name`/`map`/`prefs`,
`display get/set/reset`, `agents set` (the harness hook endpoint) /
`agents get`/`agents usage` (plan usage, kind-indexed, riding in `status`
too). Nothing watches state files but bsctl:
each bar instance of the **battlestation-workspaces** plugin spawns
`bsctl status --format json --stream` (NDJSON, emit-on-change, ~50ms
event-to-pixel) and renders lines; commands flow back through bsctl
(`ws focus` on click, `ws map set` on drag). Three harnesses live in
production: Claude Code, Codex and Antigravity (`agents set --kind <k>` in
each harness's stowed config; per-kind presentation via the registry in
`Cfg.qml` — adding a harness = one registry entry + one asset dir + one
hooks config).

## Open

1. **The asks queue (design settled 2026-07-04; MVP is human-first).**
   Agents post attention requests — questions, review asks, FYIs — into
   one shared queue; the human triages from one surface instead of
   window-hopping. The chief-of-staff agent (see IDEAS.md north star) is
   deliberately deferred: build the optimal human view first, plug the
   agent into the same store later.

   Settled design points:
   - **Store is truth, RPCs are ephemeral.** Asks live in a flocked
     runtime-dir store (own dir, so the session-file sweep and the stream
     engine's trigger filter stay untangled), states
     open → answered → dismissed, ids short and monotonic. Blocking MCP
     calls are only a bounded fast path (store-first, progress-pinged,
     timeout returns "ask #N still open") — an in-flight RPC is state in
     the wrong place over hour-scale waits; per-harness client timeouts
     and turn fragility make hours-long holds wrong. Late answers land in
     the store; delivery channels (jump-and-type now; headless resume
     later) are a separate, per-harness concern.
   - **Two ownership namespaces, never one ordering field.** Agents own
     their ask's urgency (low/medium/high, updatable — escalation = "my
     urgency rose") and estimate of HUMAN minutes needed. The human owns
     the order: FIFO by default, reordered only by the user (panel drag /
     `asks order set` — the map/prefs pattern). Agents never reorder.
   - **Posting is mandatory, not judged.** An agent needing feedback MUST
     post — proceeding without feedback is the failure mode, and queue
     depth is never a reason to self-censor. Enforced by the only prompt
     surface every session carries: the MCP tool descriptions, plus a
     norm line in each harness's stowed global instructions.
   - **Human quick-tags:** a freeform note per ask ("working on it",
     "need to think") — one click in the panel, visible to all agents via
     the stream; the human half of the coordination conversation.
   - Domain noun: `asks` (over "tasks" — collides with agents' own todo
     lists).

   Stages 1–3 SHIPPED 2026-07-04 (commits 5e52f2e, 46e19d9, ddd7c1e):
   the `bsctl asks` domain, `bsctl mcp` wired into all three harnesses
   (Codex + agy stowed; Claude Code needs the one-time `claude mcp add`
   in setup/deps-00-claude-code.md — user-scope MCP lives in untrackable
   ~/.claude.json), and the queue panel (badge + HYPER+A; reorder ships
   as per-row arrows — variable-height rows resisted the pills' drag
   machinery; the order-set protocol is gesture-agnostic, so drag can
   land later without changes beneath it). Remaining:
   (4) chief of staff in a special workspace, resume-based answer
   delivery for agents that ended their turn, presence-gated escalation
   (toast → phone), and jump-to-exact-window (pid → client →
   focuswindow) — jump currently lands on the workspace.

2. **Next harness: opencode.** The recipe is proven twice (Codex, then
   Antigravity — which surfaced one real integration cost each: Codex's
   trust gate, agy's conversationId payloads): investigate the harness's
   hook/notify surface, wire its config to `bsctl agents set --kind <k>
   <verb>`, add a registry entry + asset dir. Unknown-kind sessions already render
   as claude, so partial integration is safe at every step. (gemini-cli is
   dead — Google folded it into Antigravity, which even squats its
   `~/.gemini` config dir; already integrated, kind `agy`.)

## Parked

- Promote `battlestation-workspaces` to its own repo; this copy becomes an
  install location. Keep the widget's bsctl dependency optional (the
  state-file protocol is simple enough to consume directly).
- Optional CI (gitleaks + shellcheck + `stow --simulate` + `cargo test` on
  push) — the repo is published.
- Drift triage of remaining unmanaged `~/.config` entries as they catch the
  eye; luacheck install if doctor's Lua linting ever needs to be real.
- Shell shims (`displays-on.sh`, clamshell binds) may delegate to
  `bsctl display` once it has long-term trust — each keeps a pure-shell
  fallback (AGENTS.md recovery boundary).

## Recently completed (prune once absorbed)

- **bsctl grammar restructure (2026-07-04):** the whole CLI re-cut for
  humans and machines around domain nouns and a fixed nomenclature
  (bs-id/ws-id/display-id/display-name), designed in full before anyone
  depends on it — the last cheap moment for breaking renames. `hook` →
  `agents set` (name the domain, not the mechanism), `poll` → `agents
  get`; five overlapping movement verbs → `ws focus` (never mutates) +
  `ws send window|workspace [--focus]` over one selector set; `order` →
  `ws map` (set takes ws-ids in battlespace order — stable denotation;
  position-relative payloads compose badly); every read verb `get` with
  `--format text|json`. The pub-sub inverted: watch/`.widget.json`/the
  flock writer election and hot-standby machinery are DELETED — each
  subscriber spawns `bsctl <query> --stream` (NDJSON, emit-on-change,
  EPIPE = clean exit) and the one dispatch power (preference-apply on
  monitor arrival) kept exactly-once via a tiny nonblocking-flock applier
  election. State files renamed `map`/`prefs`, bsctl-private, writers
  flocked; one-time local `mv`, no migration code by design. Keybinds,
  widget QML (net deletion — the stream carries the precomputed
  battlespace join, so the QML resolve died) and all three harness
  configs retargeted in the same change. Lessons: the keybinds header
  comment contradicted the actual HYPER+N follow semantics — behavior,
  not prose, was authority (and the prose is now fixed); Rust ignores
  SIGPIPE, so a CLI meant for pipelines must reset it or `| head`
  panics.
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


