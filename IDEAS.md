# IDEAS — the unranked pool

Improvements and features worth considering, big and small, dreamed without
commitment. PLAN.md owns what we're actually doing; this file is where ideas
wait without pressure. Prune freely — a deleted idea that mattered will come
back on its own.

The lens for all of it: this machine is an **agent operator's desk** — one
human working with many agents across many projects — and the scarce
resource is **human attention**. Everything built so far (status bots,
battlespaces, the stream) observes agents well. The headroom is in routing
attention and letting agents participate: today the human notices, decides,
switches, and answers everything by hand.

## North star: from status lights to attention routing

The bar answers "what are my agents doing?". The next system should answer
"**who needs me next, why, and what's the fastest way to serve them?**" —
treat attention like a scheduler treats CPU. Most ideas below are fragments
of this one reframe.

- **The ask queue (the big bet).** Agents post structured *asks* instead of
  just flipping to `waiting`: a question, a decision (with options), a
  review request, a permission prompt, a done-report. `bsctl asks` becomes a
  domain (add / list / answer / dismiss; asks ride the status stream like
  everything else). The human processes a queue — email-triage instead of
  window-hopping. The widget's Panel popover pattern (Panel.qml is
  literally a text-input-plus-buttons template) renders the queue; simple
  asks (approve/deny, pick A/B) are answered *from the bar* without leaving
  the current workspace. Hook side: Claude's Notification hook already
  carries permission-prompt payloads; Codex has PermissionRequest wired;
  the raw material exists.
- **`bsctl agents next` + a keybind.** Focus the workspace of the
  longest-waiting agent (or the head of the ask queue). One key: "take me
  to whoever needs me most." A cycle variant walks all waiting agents.
  Cheap — the stream already knows wait order; it's a selector plus a
  dispatch.
- **Richer status vocabulary.** `waiting` is overloaded: finished-and-idle,
  blocked-on-question, blocked-on-permission, and errored all render the
  same orange face. The hooks can discriminate (Stop vs Notification vs
  PermissionRequest vs PostToolUseFailure). Distinct semantic statuses →
  distinct faces/colors/urgency. The Cfg.qml registry and asset model
  already support new statuses; the contract change is one lib.rs section.

## Signals and escalation

Today the ONLY consumer of agent status is the bar. A `waiting` agent
changes a 24px face and nothing else.

- **Toast on transitions.** Noctalia's ToastService is already imported by
  the plugin — a toast when an agent flips to waiting (or posts an ask) is
  nearly one line. Policy-gated so it never becomes noise.
- **Escalation ladder.** 0–2 min waiting: bar face. Longer: toast. Longer
  still + human away: phone. Each step policy, not hardcode.
- **Phone push via KDE Connect.** The daemon already runs (repo-owned user
  unit); zero plugins are configured. `kdeconnect-cli --ping-msg` (or the
  notification-sync plugin) gives "agent X waited 10m" on the phone for
  free. The pairing exists; the bridge is unbuilt.
- **Presence as a first-class signal.** Nothing on the machine knows if the
  human is present — hypridle has no idle listeners, Noctalia's idle block
  is disabled. Greenfield: an idle listener feeding `bsctl` (world gains
  `human: {present, idleSecs}`) gates the whole ladder — never toast an
  empty chair, always push to the phone when away. Agents could query it
  too: "user is away; batch questions instead of blocking."
- **While-you-were-out digest.** On return from idle: one toast/panel
  summarizing what changed — who finished, who's blocked, who errored.
  Needs only the transitions log (below) and the presence edge.
- **Do-not-disturb / focus mode.** One toggle that holds all escalation and
  batches asks. The control-center shortcut pattern (KeepAwake) is the
  template.

## The agent-legible desktop

bsctl made the desktop legible to *this* conversation's tooling. Make it
legible to every agent, on purpose.

- **`bsctl mcp` — the world model as an MCP server.** Expose status, the
  battlespace map, displays, prefs, presence, usage as tools any harness
  can call; expose asks so agents file them structurally. Cross-agent
  awareness falls out: agent A sees agent B compiling in ws 3 and stays
  off the shared build lock. Mutations (focus, send) behind a consent
  model — which is itself an ask ("Claude wants to open ws 7 — allow?").
  No harness config churn: MCP is the one integration every harness
  already speaks.
- **Kitty remote control as the answer channel.** `allow_remote_control` +
  `listen_on` are off today. On, an agent (or the quick-reply popover) can
  send text to the exact terminal running a session (`kitten @ send-text
  --match`), read scrollback for context, or open panes. This is the
  missing write-path that makes answer-from-the-bar real rather than
  focus-and-type.
- **Click a bot → focus its window, not just its workspace.** The session
  pid is in the state files; pid → Hyprland client → `focuswindow`. Today a
  click reaches the workspace and the human hunts for the pane.
- **Screen as agent input, with consent.** screen-toolkit already does
  capture + OCR. A bsctl-mediated "agent requests a look at region/output
  X" (consent toast, then capture) turns the existing overlay kit into
  agent vision without new machinery.
- **`focused-cwd.sh` generalized.** "What is the human looking at" (focused
  window, its cwd, its project) as a queryable — context for any agent that
  wants to meet you where you are.

## Fleet and projects

- **Project registry.** The prefs concept, one level up: project dir →
  battlespace, preferred display, harness, session name. `bsctl project
  open dropkick` = create/focus the workspace, name it, spawn the terminal,
  resume the session. A morning `bsctl project up` starts the whole desk.
  Guardrail: sparse by design, like prefs — register only the projects you
  actually home (the stamp-policy-creep lesson).
- **Transitions journal + `bsctl agents stats`.** Session files are
  ephemeral by design; a tiny append-only log of status transitions
  (session, kind, ws, status, ts) unlocks: time-in-state, wait-time
  per project ("which project starves?"), a daily standup digest ("14
  sessions, 5 projects, median unblock 4m"), and the
  while-you-were-out diff. Runtime-dir for the log too — stats are a
  day-scale concern.
- **Per-project cost.** Correlate usage-API deltas with which sessions were
  active — rough per-project spend attribution from data already flowing.
- **A hardware attention key.** keyd owns the board; a dedicated key (or
  layer) for "next waiting agent" / "toggle ask queue" makes attention
  routing muscle memory. keyd config is one stanza.

## Widget growth (the cheap seats)

The stream already delivers more than the widget renders:

- **Subagent elapsed time** — `started` is parsed and diffed but never
  shown; Cfg.fmtTime exists. Show it on the sub-bot tooltip at least.
- **Badge counters on commanders** — squad size as a corner badge (BotIcon
  has free layout room), not just the lean.
- **Render prefs** — the entire prefs section of the stream is dark. A pin
  glyph on pills whose workspace has a home; a ghost pill (or bar hint) for
  prefs waiting on absent displays.
- **Per-kind usage** — agents usage is kind-indexed; UsageIndicator reads
  only `.claude`. Render whatever kinds appear.
- **Unused inputs** — scroll on the widget (cycle workspaces?),
  middle-click (close?), double-click, press-and-hold on a bot (peek its
  ask / transcript tail). All currently unbound.
- **Harness hook normalization** — agy has no SessionEnd/clear (its bot
  lingers until GC) and no session-start; Codex's PermissionRequest
  deserves a distinct status rather than plain waiting. One pass over
  three configs.

## Integration health

- **`bsctl agents doctor`.** bin/doctor checks the repo; nothing checks the
  collaboration layer. Verify: harness binaries present, hook lines wired
  and current in each config, Codex trust hashes valid (silently-dropped
  hooks are invisible today — the exact failure the `--kind`-mandatory rule
  was designed around), stream flowing, session files fresh. "make check"
  for the agent plane.
- **Doctor for the unmanaged** — `~/.local/bin` (where bsctl and all three
  harness binaries live) has no drift visibility at all.

## Small ergonomics

- **Fish functions/abbreviations** — completions are generated at build
  time, but `functions/` is empty: `wsn` (next waiting), `bss` (status),
  an `agents` abbreviation family. Two-minute wins.
- **`bsctl status` on a spare surface** — the tty stream dashboard is new;
  a keybind that opens it in a floating/special-workspace kitty makes it a
  one-key mission control. (A dedicated Noctalia panel is the bigger
  sibling.)
- **Stream over SSH** — `ssh battlestation bsctl status --stream` already
  works by construction; document it. The laptop watches the desk.
- **Session-id ergonomics** — short-prefix matching for `--session-id`
  (uuids are hostile); maybe `--ws`/`--kind` as agents-get filters that
  compose.

## Out there

- **Voice answers.** Push-to-talk → whisper → the focused agent (or the
  ask queue's head). Answering a yes/no ask while making coffee.
- **Drag a bot to a pill = move that session there.** The session's
  terminal window follows (pid → client → movetoworkspacesilent). The
  bots stop being decoration and become handles on the sessions.
- **Day replay.** The transitions journal rendered as a timeline — where
  attention went, where agents starved, when the human was the bottleneck.
- **Phone dashboard.** The stream websockified read-only; the status table
  on a phone from the couch. (KDE Connect covers alerts; this is the
  ambient view.)
- **The kit.** battlestation-workspaces + bsctl as an installable "agent
  operator's desk" for other people — the parked plugin-repo promotion,
  grown into a product-shaped thing. The repo is already written for
  strangers; this is the stranger-shaped payoff.

## Guardrails (lessons that should discipline all of this)

- **Sparse by design.** Management is opt-in per thing (prefs taught this
  twice). Registries and queues must not creep toward "everything managed."
- **Write at intent time, never at teardown time** — the race class that
  killed snapshot-on-removal will reappear in any "remember state when X
  disappears" idea.
- **One grammar.** New capability = new noun/verb in bsctl, contracts in
  lib.rs, consumers through the stream. No side channels; nothing watches
  state files but bsctl.
- **Recovery stays shell.** Anything that must work when the desk is broken
  cannot depend on bsctl, the stream, or a panel.
- **Attention features must never cost attention.** Every signal idea above
  ships policy-gated and default-quiet; the bar earned trust by being
  glanceable, not chatty.
