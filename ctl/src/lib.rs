//! bsctl — battlestation repo control tool.
//!
//! Owns the desktop's stateful protocols (AGENTS.md ownership rule: bsctl
//! owns stateful protocols and JSON-heavy logic; shell owns glue and
//! recovery). This header is the durable home of those protocol contracts.
//! bsctl is both sides of every one of them: the only WRITER of its state
//! files, and the query/stream surface everything else reads through —
//! nothing but bsctl touches the files underneath.
//!
//! # Nomenclature
//!
//! Four coordinate systems, kept distinct by flag name everywhere:
//!
//! - `bs-id` — battlespace id: the 1-based position in the resolved map
//!   (what SUPER+N means). Battlespaces are the display-layer remap of
//!   workspaces; reordering pills renumbers battlespaces, never workspaces.
//! - `ws-id` — the Hyprland workspace id: immutable, stable across
//!   reorders (what scripts and preferences mean).
//! - `display-id` — 1-based over the ENABLED outputs sorted by (x, y),
//!   leftmost first (ties break by name, so the numbering is
//!   deterministic). A pure remap: it renumbers when outputs come and go.
//! - `display-name` — the compositor's output name (`eDP-1`). The only
//!   way to address an ABSENT display (an unplugged output has no id).
//!
//! Selector flags say which system they speak (`--bs-id`, `--bs-rel`,
//! `--ws-id`, `--display-id`, `--display-name`); anything read-shaped takes
//! `--format text|json` and the queries that serve subscribers take
//! `--stream` (json only — see "Streaming").
//!
//! # Agent sessions (`bsctl agents set` / `bsctl agents get`)
//!
//! Tracks each agent-harness instance's status for the Noctalia
//! battlestation-workspaces widget. Multi-harness by construction: ONE flat
//! dir holds sessions from ANY harness, discriminated per session by
//! `kind`. State lives as small JSON files in
//! `${XDG_RUNTIME_DIR:-/tmp}/battlestation-ws/`:
//!
//! - `<session_id>` — one file per live session:
//!   `{"ws": <int>, "win": "<addr>"|null, "status":
//!     "waiting"|"thinking"|"tooling", "kind": "<harness>",
//!     "title": "<aiTitle>", "pid": <int>}`.
//!   `ws` is the Hyprland workspace id owning the session's terminal window
//!   (found by walking /proc ancestors against `hyprctl clients -j`), and
//!   `win` is that same clients row's window address (null when the row
//!   carried none) — both refresh on every hook event, so a moved terminal
//!   heals; `win` is what upgrades `ws focus --session` from
//!   jump-to-workspace to jump-to-window. `pid`
//!   is the harness process — the nearest ancestor whose comm matches the
//!   kind (harness binaries are named after their kind: comm `claude` /
//!   `codex`, both verified live; comm is the kernel's 15-char truncation,
//!   so longer kinds match on their truncation) — and the scan sweeps
//!   session files whose pid is dead. `status` is semantic; mapping states
//!   to colours is the widget's concern. `title` (the bot's hover tooltip)
//!   is kind-agnostic precedence, first non-empty wins: (1) the LAST
//!   `"type":"ai-title"` record in the transcript, whitespace-collapsed to
//!   one line (Claude; harmlessly empty for harnesses without ai-title
//!   records); (2) derived from the payload when it carries `prompt`
//!   (Codex UserPromptSubmit): `basename(cwd): <first non-blank prompt
//!   line>` capped at 60 chars; (3) STICKY — the title already in the
//!   session file, so a title set once persists across events that carry
//!   nothing; (4) "".
//! - `<session_id>.<agent_id>` — one marker per RUNNING subagent:
//!   `{"type": "<agent_type>", "description": "<text>"}`. Markers carry no
//!   `kind` — a subagent inherits its session's kind in the widget.
//!   Marker MTIME is the subagent's start time (shown as elapsed time in
//!   the widget) and is refreshed by the agent's own tool calls, which is
//!   what keeps the scan's GC honest: a live agent's marker never goes
//!   stale, a killed agent's (no SubagentStop fires) does, and is swept
//!   after 30 minutes unless its subagent transcript is still being
//!   written. Session ids are UUIDs and agent ids hex — neither contains a
//!   dot — so splitting a marker name on the FIRST dot is unambiguous.
//!
//! Writers are atomic (temp file + rename in the same dir); temp names are
//! `.<name>.tmp`, and the leading dot is what makes the scan's dotfile skip
//! (and `clear`'s `<sid>.*` sweep) ignore them. `debug.log` (appended when
//! `CLAUDE_WS_DEBUG` is set: argv + raw stdin per call) is diagnostics, not
//! protocol — the scan skips it by name.
//!
//! `agents set --kind <harness> <verb> [--session-id <s>]` is the hook
//! endpoint the harness configs call, hook-event JSON on stdin. It is
//! silent-tolerant BY CONTRACT — always exits 0, even on unknown/missing
//! verbs or a missing/valueless/empty `--kind` — because a harness hook
//! must never block or error loudly; that is why its flags are parsed from
//! raw tokens rather than by clap (a clap required-value option exits 2
//! loudly when the value is missing). `--kind` is MANDATORY with no
//! default: a kindless call is a silent no-op, so an outdated caller
//! visibly stops updating instead of guessing a harness. `--session-id` is
//! an argv override that outranks the payload's session key everywhere one
//! is derived; the payload stays the normal source (harness hooks deliver
//! ids inside the event JSON). The session key is `session_id`, falling
//! back to `conversationId` — Antigravity (`agy`) payloads are protojson
//! camelCase and identify the session by conversationId only (verified
//! against the contract docs embedded in the agy binary). Verbs:
//!
//! - `waiting`/`thinking`/`tooling` — (re)write the session file with that
//!   status; events carrying an agent_id also refresh (or recreate/heal)
//!   that subagent marker. A payload with NO session key (empty/absent/bad
//!   JSON) is a silent no-op for all four session verbs: falling back to a
//!   "default" session file would fabricate a phantom session whose pid is
//!   the caller's own harness ancestor, unsweepable while that process
//!   lives. Only the marker names of subagent-start/subagent-stop keep the
//!   historical "default" sid fallback (an orphan marker is swept by the
//!   next scan).
//! - `clear` — remove the session file AND its subagent markers.
//! - `subagent-start` / `subagent-stop` — write/remove one subagent marker
//!   (fast path: no hyprctl), falling back to the subagent's meta.json
//!   next to the transcript for missing type/description.
//!
//! `agents get [--kind K] [--session-id S] [--format text|json]` is the
//! readable query over the same state, sweeping like every scan (dead
//! pids, orphan and stale markers). Its published schema — `[{session,
//! kind, status, ws, win, title, subagents: [{id, type, description,
//! started}]}]` — deliberately respells the on-disk `sid`/`agents` keys;
//! the `status` world feed emits the identical rows (shared code, so the
//! two surfaces can never drift).
//!
//! # The asks queue (`bsctl asks`)
//!
//! Attention requests: agents post asks — questions, review requests,
//! FYIs — into one shared queue and the human triages them from one
//! surface. The store is the SOURCE OF TRUTH and RPCs are ephemeral: an
//! ask outlives any connection (the stage-2 MCP `ask` tool may block
//! briefly on a fresh ask as a fast path, but a timeout returns "ask #N
//! still open" and the answer lands here to be collected later — an
//! in-flight RPC is state in the wrong place over hour-scale waits).
//!
//! State lives in `${XDG_RUNTIME_DIR:-/tmp}/battlestation-asks/` — its own
//! dir, a SIBLING of battlestation-ws (the session scan owns that dir's
//! non-dot namespace, and the stream engine's trigger filter serves whole
//! directories). Runtime lifetime on purpose: asks reference sessions and
//! neither survives a reboot. Two files: `asks.json`, one object
//! `{"next_id": <int>, "asks": [{id, session, kind, ws, type, title,
//! body, options, urgency, estimate_min, note, state, answer, created,
//! answered_at}]}` written atomically (a corrupt store reads as fresh —
//! the ids it named are gone with it); and `order`, the HUMAN's queue
//! order (ask ids, map-file format). Writers hold a blocking exclusive
//! flock on `<dir>/.lock` across read-modify-write; readers ride the
//! atomic renames lock-free (the map/prefs locking rule).
//!
//! TWO OWNERSHIP NAMESPACES, never one field. Agents own `urgency`
//! (low|medium|high) and `estimate_min` (their estimate of HUMAN minutes
//! needed) — `asks update` may change exactly those, so escalation means
//! "my urgency rose", never "I moved myself up". The human owns the rest:
//! the reply text and the completion, DECOUPLED on purpose. `asks reply`
//! sets/updates the text without touching state (empty clears): on an
//! open ask it is a DRAFT, and because the block loop releases on STATE,
//! a draft never releases a blocked asker — the human keeps revising
//! while "still working on it", while an agent peeking via get_ask sees
//! the reply-in-progress. `asks complete` is the releasing transition
//! (legal with no text — an ack is an answer); `asks answer` composes
//! reply + complete in one locked write (open only — re-answering would
//! clobber what the asker may have collected); `asks reopen` walks
//! answered back to open KEEPING the text as a draft, with the honest
//! caveat that an asker who already collected the answer can't have it
//! recalled — reopen governs the queue, not the past. Also the human's:
//! `note` (the freeform quick-tag: "working on it"; visible to every
//! agent through the stream — the human half of the coordination
//! conversation), dismissal, and the ORDER: the queue is FIFO by
//! `created`, and only the human reorders (`asks order set`, the panel
//! drag). POSTING IS MANDATORY, NOT JUDGED: an agent needing
//! feedback MUST post — proceeding without feedback is the failure mode,
//! and queue depth is never a reason to self-censor. The norm is carried
//! by the stage-2 MCP tool descriptions (the one prompt surface every
//! session gets) plus each harness's stowed global instructions.
//!
//! Resolved queue order, emitted by every reader: open asks the order
//! file lists (in list order, tokens matching ids textually), then
//! remaining open asks FIFO by `created`, then answered-but-not-dismissed
//! asks (FIFO — awaiting collection; the order file deliberately does not
//! apply to them, they are past triage). Dismissed asks are excluded
//! everywhere except `asks get --id`, the direct record lookup (any
//! state — the debugging view). States: open -> answered (complete or
//! answer) or -> dismissed (from ANY state: dismissing an open ask is
//! declining to answer; idempotent, unknown ids are quiet); answered ->
//! open again via reopen.
//!
//! # MCP (`bsctl mcp --kind <k>`)
//!
//! A stdio MCP server giving every harness the asks queue and read-only
//! world queries — one server per session, spawned by the harness's MCP
//! config the way the hooks spawn `agents set`, with the same `--kind`
//! discriminator. Hand-rolled JSON-RPC 2.0 over stdio lines (no SDK: the
//! needed surface is five requests — initialize, ping, tools/list,
//! tools/call — plus tolerated lifecycle notifications and outbound
//! notifications/progress; an SDK would bring an async runtime to a
//! blocking line loop). The handshake echoes the client's protocol version
//! when known (2024-11-05 / 2025-03-26 / 2025-06-18 — wire-compatible for
//! this surface), else answers the newest known.
//!
//! SESSION IDENTITY: no harness tells an MCP server which session spawned
//! it, so the server resolves its own — the /proc ancestor walk names the
//! harness process (comm == kind, the hook rule) and the session file
//! recording that pid IS the session; its sid + ws ride every posted ask.
//! Unresolvable (headless, hooks not yet fired) degrades to posting with
//! empty session/null ws and retries per call — never fails.
//!
//! TOOLS: `ask` (post a question, store-first, then block awaiting the
//! answer; answered -> the answer text, dismissed -> "proceed on judgment,
//! don't re-post" — dismissal is also the human's release valve for a
//! blocked agent — wait expiry -> "ask #N remains open" as a SUCCESS: the
//! RPC is only a fast path and the store outlives it), `notify`
//! (non-blocking review/FYI post), `list_asks` / `get_ask` (the queue and
//! the late-answer collection), `update_ask` (urgency/estimate only, own
//! asks only — the two-namespaces rule enforced at the tool boundary),
//! `world` (the status object; the MVP deliberately exposes NO mutating
//! world tools — that needs the consent design). The tool DESCRIPTIONS
//! carry the mandatory-post norm — they are the one prompt surface every
//! session of every harness receives — as does the initialize
//! `instructions` field; their wording is contract, not copy.
//!
//! THE WAIT IS AGENT-OWNED: `ask` takes `wait_secs` — 0 = post and return
//! immediately (collect via get_ask), omitted = the server's --block-secs
//! default (90), explicit values clamped to 24h. Hour-scale waits are
//! legitimate (a truly blocked agent waiting IS using its time well), so
//! the block is PROTOCOL-RESPONSIVE: stdin is poll(2)-multiplexed with the
//! 500ms store poll — a `ping` is ponged mid-block, `notifications/
//! cancelled` for the in-flight call ends the block with NO response (per
//! spec) while the ask STAYS open (the store is truth; cancellation is
//! transport, not triage), any other request gets a -32000 busy error
//! naming the open ask (silence would deadlock the client), and stdin EOF
//! is shutdown (exit 0). Progress notifications flow ~10s when the client
//! sent a progressToken — on resettable client timeouts they are what
//! keeps a long wait alive. Waits beyond a client's own tool-timeout
//! ceiling die client-side; the ask survives that too.
//!
//! WIRING (per harness, verified live 2026-07-04): Claude Code reads user-
//! scope servers from `~/.claude.json` only (settings.json `mcpServers` is
//! ignored — probed empirically on 2.1.200), which is unstowable state, so
//! registration is a one-time `claude mcp add --scope user` documented in
//! setup/deps-00-claude-code.md. Codex: `[mcp_servers.battlestation]` in
//! the stowed config.toml (the filter keeps `mcp_servers` sections).
//! Antigravity: `~/.gemini/config/mcp_config.json` (its embedded docs name
//! it; standard mcpServers shape), stowed alongside hooks.json. Codex and
//! agy exec the command directly (no shell), so both wire through
//! `/bin/sh -c 'exec "$HOME/..."'` for expansion; Claude stores the
//! absolute path at add time. CLIENT TOOL-TIMEOUT CEILINGS, raised to 2h
//! for the long waits: Claude Code via MCP_TOOL_TIMEOUT=7200000 (ms; env
//! var name verified in the 2.1.201 binary) in the stowed settings.json
//! `env` block (the claude-settings filter whitelists `env`); Codex via
//! `tool_timeout_sec = 7200` (key verified in the 0.142.5 binary) on the
//! server entry. Antigravity exposes no attributable MCP timeout knob
//! (probed 1.0.16) — its ceiling is whatever the binary defaults to, and
//! long agy waits may die client-side (the ask survives).
//!
//! # Presence (`bsctl presence`)
//!
//! Is the human at the desk? hypridle is the machine's idle authority: the
//! stowed listener (hypridle.conf, timeout 120s) runs `presence set idle`
//! when the desk goes quiet and `presence set active` on resume — the
//! human never runs `set` by hand in normal operation; bsctl only records
//! the report. The file is `${XDG_RUNTIME_DIR:-/tmp}/battlestation-asks/
//! presence.json` — the ASKS dir on purpose: presence is
//! attention-adjacent state, the stream engine already watches that dir
//! (the non-dot name rides the existing trigger filter, so a presence flip
//! wakes every subscriber), and nothing there reads by pattern. One object
//! `{"state": "active"|"idle", "since": <float secs>}`, atomic write;
//! setting the SAME state twice never bumps `since` — hypridle's
//! on-timeout can re-fire without a resume between, and the idle duration
//! must accumulate across fires, not reset. Runtime lifetime: presence
//! dies with the boot, like the sessions it contextualizes.
//!
//! Before hypridle's first transition after a boot or restart the file is
//! absent and presence reads `unknown` — hypridle has no startup hook to
//! prime it, and an honest unknown beats a guessed active. Readers:
//! `presence get` (text `active` / `idle 43m` / `unknown`; json adds
//! `since` + `idle_secs`), the world's `human` section, and the MCP `ask`
//! timeout text, which tells a waiting agent how long the human has been
//! idle (a minute or more) so it can weigh continuing other work against
//! a longer wait. Recovery boundary: hypridle's lock/sleep duties never
//! depend on bsctl — a missing/broken binary just fails the report
//! commands harmlessly.
//!
//! # The battlespace map (`bsctl ws map`)
//!
//! Navigate/move by BATTLESPACE instead of Hyprland's immutable workspace
//! id. Hyprland has no way to renumber a workspace, so "reordering" is
//! purely a display-layer remap: ws-ids stay put, and a persisted list
//! maps bs-id <-> ws-id. The battlestation-workspaces bar plugin renders
//! battlespace order (pills are labelled by bs-id) and drives
//! `bsctl ws map set` when a pill is dragged — bsctl is the file's only
//! author, and the widget learns the result back through its status
//! stream, never from the file.
//!
//! The map file, `${XDG_STATE_HOME:-$HOME/.local/state}/
//! battlestation-workspaces/map`, is ws-ids in battlespace order,
//! space-separated with a trailing newline: `3 1 2 5 4\n`. Resolved order =
//! the listed ids that currently exist (in list order), followed by any
//! live workspaces not listed, ascending; missing/empty file => identity.
//! Tokens match live ids TEXTUALLY ("07" never matches id 7, and a
//! repeated token repeats in the output). "Live" excludes workspaces whose
//! name starts with `special:` — matching what the bar shows.
//!
//! `map get` prints the resolved join (bs-id, ws-id, name, display,
//! windows, active) — THE learning view of the model. `map set <ws-id>..`
//! replaces the file with the FULL list; the payload is ws-ids, never
//! positions — a permutation of positions would be relative to the map it
//! replaces and compose confusingly across rapid writes, while ws-ids
//! denote stably (and are exactly what a pill drag produces). `map reset`
//! truncates IN PLACE, falling back to identity order.
//!
//! # Workspace->display preference (`bsctl ws prefs`)
//!
//! Hyprland evacuates a disappearing display's workspaces to the survivors
//! and never moves them back on replug — so a workspace CAN prefer an
//! output, and an arriving output collects its workspaces. The prefs file
//! (`prefs`, the map file's sibling, same persistence rationale: a
//! preference is user INTENT and intent outlives boots — workspace ids are
//! stable habits under global numbering) is plain text, one `<ws-id>
//! <output>` pair per line, sorted by id, trailing newline. Missing/empty
//! file = no preferences; unparseable lines are skipped (one corrupt line
//! loses one preference, never the file); a duplicated id keeps the last
//! line (later = newer intent).
//!
//! SPARSE BY DESIGN: most workspaces have no preference and are entirely
//! Hyprland's business — a new workspace is born unmanaged and stays so
//! until explicitly homed. The ONLY writers are the two homing verbs:
//! `prefs add (--bs-id|--ws-id) (--display-id|--display-name)` — explicit;
//! `--display-name` is accepted VERBATIM even when absent (pre-declaring a
//! home for the dock display is the feature's main move) while
//! `--display-id` must resolve (an id only numbers what's plugged in) —
//! and `ws send workspace`, because an explicit move IS the user homing
//! that workspace (unconditional overwrite). Reorders, drags and every
//! bulk operation never stamp; the stream engine and Hyprland never stamp
//! (evacuations and automatic restores are not intent — that asymmetry IS
//! the model). Unplug is a non-event: nothing is recorded at teardown
//! time, so there is no race against Hyprland's own evacuation. `prefs rm
//! (--bs-id|--ws-id|--all)` deletes (idempotent; `--all` writes the empty
//! file rather than deleting it).
//!
//! APPLYING: `prefs get` lists the preferences with reality annotations
//! (output present, workspace live). `prefs reconcile` moves every live
//! workspace that prefers a PRESENT output and sits elsewhere, via the
//! bounded settle machinery (`ws::restore_strays`: re-read placement every
//! 200ms, up to 5 passes, stop after a clean one) — a display gaining
//! workspaces then SHOWS one of its preferred ones (never a dead id;
//! focusing a dead id would CREATE it), other displays keep their view,
//! and the keyboard ends on the focused display. The stream engine runs
//! the same apply scoped to the arriving output on `monitoraddedv2` — its
//! only dispatch power (see "Streaming").
//!
//! ## Locking
//!
//! Every read-modify-write of the map/prefs files (map set/reset, prefs
//! add/rm, the send-workspace stamp) holds a blocking exclusive flock on
//! `<dir>/.lock` across load->modify->save, so concurrent writers can't
//! lose each other's update. READERS take no lock: every write lands by
//! atomic rename (or a single write(2) for the map), so a reader sees old
//! or new bytes, never a torn file. Best-effort: if the lock file can't be
//! created the write proceeds unlocked — state must never be droppable
//! because /run filled up.
//!
//! # The world (`bsctl status`)
//!
//! The full state in one object — text is the at-a-glance human overview
//! (the asks queue FIRST — who needs you outranks what the desk looks
//! like — then displays, their battlespaces, agents, prefs waiting for
//! absent displays), json is the machine feed and, with `--stream`, THE
//! subscription the widget lives on. Schema (one compact line):
//!
//! `{"asks": [ ...exactly `asks get`'s resolved rows... ],
//!   "displays": [{id, name, x, y, focused, activeWs, specialShowing}],
//!   "workspaces": [{ws, bs, name, display, windows, active, pref}],
//!   "prefs": [{ws, display, present, live}],
//!   "agents": [ ...exactly `agents get`'s rows... ],
//!   "usage": { ...exactly `agents usage`'s object... },
//!   "human": {state, idle_secs}}`
//!
//! `workspaces` is the battlespace join in battlespace order; `name` is
//! null while a workspace is unnamed (Hyprland names every workspace its
//! own number until someone renames it); `pref` is null for the unhomed
//! majority; `active` = some display is showing it. If EITHER compositor
//! query fails, `displays` and `workspaces` are `null` — not `[]`, which
//! would read as a true empty world — so a consumer keeps its last state
//! across a compositor restart. `prefs` is file truth and always present,
//! but its annotations degrade to null without a compositor to ask;
//! `asks` is file truth ([] when the queue is empty, never null),
//! `agents` is file+proc truth, and `human` is file truth (state
//! "unknown" before hypridle's first report; `idle_secs` is 0 while
//! active — the direct answer, never null-as-no-data — the accumulated
//! duration while idle, and null only while unknown) — none of the
//! three ever null.
//!
//! # Streaming (`--stream`)
//!
//! Any subscriber-shaped query — `status`, `agents get`, `asks get`,
//! `ws map get`, `ws prefs get`, `display get` — takes `--stream`: emit
//! the full result
//! now, then re-emit it whenever it changes. `--stream` is orthogonal to
//! `--format`; the FRAMING follows the format. json: NDJSON, one compact
//! line per emission — the machine dialect. text: the same renders framed
//! for eyes — on a tty, clear screen + cursor home before each emission
//! (watch(1)-style; `bsctl status --stream` is a live dashboard), piped,
//! emissions separated by one blank line (the first unseparated). The tty
//! check is isatty(1), once at startup. An empty text result stays empty
//! (no lonely headers): a tty clears to blank — the world really emptied —
//! and a pipe sees a lone separator. Nothing watches bsctl's files
//! but bsctl: subscribers spawn the process and read stdout. Each
//! subscriber owns its own process — there is no shared output file, which
//! is why the old single-writer flock election and its hot-standby
//! machinery no longer exist.
//!
//! The engine folds three wake sources into one re-evaluation: ONE inotify
//! fd watching the runtime agent-state dir, the persistent map/prefs dir
//! and the asks dir (each dir's protocol files are exactly its non-dot
//! entries), Hyprland's `.socket2.sock` event stream, and a 10s tick
//! (session pids dying and markers aging are invisible to inotify). Bursts
//! coalesce ~50ms (bounded rounds so a steady stream can't starve
//! emission); emissions dedupe on the serialized result, so a subscriber
//! is never woken for nothing. The socket2 events that warrant
//! re-evaluation: workspace(v2), createworkspace(v2), destroyworkspace(v2),
//! moveworkspace(v2), renameworkspace, focusedmon(v2), monitoradded(v2),
//! monitorremoved(v2), openwindow, closewindow, movewindow(v2),
//! activespecial(v2), configreloaded; everything else — notably the noisy
//! windowtitle*/activewindow*, which nothing rendered depends on — is
//! ignored. Error surface: the FIRST evaluation's failure aborts with its
//! exit code (a bad selector must fail loudly at spawn time, not stream
//! nothing); later failures skip the emission (mid-flux state resolves by
//! the next event). A closed stdout is the one CLEAN exit (0): the
//! subscriber left, streaming to nobody is done, not broken. A socket2
//! disconnect (compositor restart) rides the transient error path: log to
//! stderr once, sleep 1s, re-init and reconnect; if socket2 can't connect
//! at all (no Hyprland), the stream runs DEGRADED — file events keep
//! flowing, compositor-derived sections ride the null path.
//!
//! ONE deliberate exception to the engine's read-only role: on
//! `monitoraddedv2` the arriving output's preferences are applied (the
//! prefs contract above). Several subscribers may be streaming at once and
//! a double apply would double the dispatch and the focus churn, so a
//! single APPLIER is elected: first process to see an arrival wins a
//! NONBLOCKING flock on `<state-dir>/.apply.lock` and keeps it for its
//! lifetime; losers skip, knowing a winner exists; when the winner dies
//! the kernel drops its lock and the next arrival elects a survivor. The
//! apply runs AFTER emitting, so subscribers see the world before the
//! moves land, then again as they do.
//!
//! # Movement (`bsctl ws focus` / `bsctl ws send`)
//!
//! One selector grammar, mutation split: `focus` never mutates, `send`
//! always does, `--focus` on send follows the moved thing. `focus
//! (--bs-id|--bs-rel|--ws-id|--display-id|--display-name|--session)`
//! focuses the workspace (wherever it lives), the display (its active
//! workspace), or an agent session's terminal WINDOW — `--session` (focus
//! only; selectors earn their spots) resolves the session record and
//! dispatches its `win` address, falling back to its `ws` when the record
//! predates the address capture; an unknown session errors loudly (a
//! caller bug, not a keybind grazing the edge).
//! `send window <same selectors> [--focus]` moves the active window — a
//! display target means that display's ACTIVE workspace. `send workspace
//! (--display-id|--display-name) [--focus]` moves the active workspace to
//! a display, keeping its ws-id (workspace ids are untouched by monitor
//! moves, so the map never needs migration), stamps the preference (see
//! prefs), and pins focus explicitly rather than trusting the move's
//! version-dependent inherent focus — `--focus` lands on the moved
//! workspace, stay re-focuses the source display. Sending to the display
//! it's already on is a no-op.
//!
//! Selector edges: a bs-id off the map's end is a SILENT exit 1 — keybinds
//! hit it constantly (SUPER+8 with five workspaces) and their stderr goes
//! nowhere useful; `--bs-rel` steps from the active workspace's position,
//! clamped to the ends (an active outside the order, e.g. special,
//! defaults to position 1; an empty world is a no-op exit 0); focusing a
//! nonexistent `--ws-id` CREATES that workspace (Hyprland semantics,
//! documented in the help); unknown display ids/names error loudly listing
//! the valid numbering.
//!
//! Dispatch goes through the Lua API (`hl.dsp.*` strings) because the Lua
//! config parser rejects the legacy `hyprctl dispatch` forms; the exact
//! strings are the live-verified known-good ones (Hyprland 0.55.4:
//! `hl.dsp.focus({ workspace/monitor })`, `hl.dsp.window.move({ workspace,
//! follow })`, `hl.dsp.workspace.move({ workspace, monitor })`,
//! `hl.dsp.workspace.rename({ workspace, name })`), unchanged by the CLI
//! restructure and pinned byte-for-byte in tests.
//!
//! `ws name get/set/rm` reads and writes workspace display names (rename
//! never renumbers; an empty name resets to the id's number — one Hyprland
//! call covers both set and reset). `name rm` accepts the row filters
//! (`--all`, one workspace, one display's workspaces) and resets every
//! match.
//!
//! # Display management (`bsctl display`)
//!
//! The human/diagnostic tool for display state; the AUTOMATED recovery
//! paths stay shell per AGENTS.md (hypridle's after_sleep_cmd ->
//! displays-on.sh, the lid binds -> clamshell.sh,
//! restart_crashed_lock.sh forever).
//!
//! Core semantic constraint (AGENTS.md gotcha): the dpms dispatcher is
//! TOGGLE-ONLY. The string form `hl.dsp.dpms("on")` toggles EVERY monitor;
//! the table form `hl.dsp.dpms({ monitor = "X" })` toggles exactly one;
//! neither can "set". So every dpms mutation here is read-before-toggle:
//! read `dpmsStatus` from `j/monitors all`, emit the TABLE-FORM toggle
//! only when the state differs. Corollaries: `set dpms` is idempotent by
//! construction, and disabled outputs are refused (their dpmsStatus is
//! meaningless — a lid-disabled panel reports dpms on — and runtime
//! dpms/eval calls no-op on disabled outputs; `reset`/reload re-enables).
//!
//! - `get [--display-id|--display-name] [--format]` — per-output table
//!   (from `j/monitors all`, with the display-id column; disabled outputs
//!   show `-`) + lid state (`/proc/acpi/button/lid/*/state`; no lid device
//!   -> desktop, line omitted) + WARNING lines with remedies. A selector
//!   narrows the table; the lid line and warnings stay — they are the
//!   report's value and cost nothing. Warnings never affect the exit code.
//! - `set dpms (--display-id|--display-name) (--on|--off)` — safe dpms
//!   targeting as above; names resolve against ALL outputs so the
//!   disabled-panel refusal still fires; unknown targets error listing the
//!   valid ones.
//! - `reset` — displays-on.sh's reset flow, argument-free on purpose
//!   (recovery must be dumb): `reload` (re-applies monitors.lua), shell
//!   out to clamshell.sh auto (it OWNS the lid policy), then dpms-on all
//!   enabled outputs with bounded re-read retries.
//!
//! # Display scale (`bsctl display set scale`)
//!
//! Step a monitor's scale along a fixed ladder at runtime, preserving its
//! mode. The selector is optional and defaults to the FOCUSED monitor
//! (what the zoom keybinds mean). The ladder is `1.0 1.25 1.5 1.75 2.0
//! 2.5 3.0` (rung 0 = native). Hyprland snaps fractional scales to its own
//! 1/120 grid and the achievable values are irregular per panel, so the
//! REPORTED scale can't drive deterministic stepping — the rung INDEX is
//! persisted per monitor instead, at
//! `${XDG_RUNTIME_DIR:-/tmp}/hypr-display-scale.<name>` (the index in
//! decimal + newline; runtime-dir state, gone on reboot — like scale
//! itself, which reverts to monitors.lua on reload).
//!
//! `--up`/`--down` start from the saved index when present, else the rung
//! nearest the reported scale (ties toward the lower rung); step ±1; clamp
//! to the ladder; persist the NEW index; then eval `hl.monitor({ output,
//! mode, position, scale })` — mode AND position preserved from the
//! monitor's current state (re-declaring with position "auto" would let
//! Hyprland RE-PLACE the output and migrate its workspaces). `--value <v>`
//! evals the given scale (positive finite; Hyprland snaps it to the 1/120
//! grid, so the applied value may differ slightly) and persists the
//! NEAREST rung so later steps stay deterministic from wherever it landed.
//! `--reset` deletes the state file and evals `scale = "auto"` — the
//! quoted Lua STRING, vs a bare Lua number for explicit scales; the
//! quoting is semantic. State is written before the dispatch, so the exit
//! code is the eval's and a rejected eval still leaves the index
//! persisted. After any scale eval: right-hand neighbors are re-flowed to
//! absolute targets (Hyprland re-flows auto-positioned monitors on SOME
//! runtime changes but not others — never apply relative deltas to
//! observed state) and strayed workspaces are settled home
//! (`ws::restore_strays`).
//!
//! # Plan usage (`bsctl agents usage`)
//!
//! Plan usage per harness kind, indexed for the day codex/agy grow usage
//! endpoints: `{"<kind>": {"sessionPct": <int>, "sessionResets": "...",
//! "weeklyPct": <int>, "weeklyResets": "..."}}` — kinds with nothing known
//! are simply absent, so `{}` means "nothing known", never an error (text
//! renders the house table, or nothing at all when empty). One provider
//! exists today (claude, the OAuth usage endpoint — the same data as
//! `/usage`); adding one is a single entry in usage.rs's provider table.
//! `--kind` filters to one kind.
//!
//! The claude reading is cached at `${XDG_CACHE_HOME:-$HOME/.cache}/
//! claude-usage.json` (ttl 240s; the cache stores the bare reading — kind
//! indexing is output shape). Refreshes are serialized with flock(2) on
//! `<cache>.lock`, so every poller and streamer combined pays at most one
//! fetch per TTL — losers block, then serve whatever the winner cached.
//! Every failed refresh (non-200, malformed body, missing token) serves
//! the STALE cache instead of nothing — stale beats absent, and the
//! untouched mtime means the next call retries — so a rate-limited (429)
//! or expired-token (401) request never clobbers or blanks a good value.
//! The fetch is bounded (`curl --max-time 6`): it sits on the stream
//! engine's tick path and must never hang a subscriber unbounded. The
//! OAuth token comes from `~/.claude/.credentials.json`
//! (`.claudeAiOauth.accessToken`; missing/unreadable -> nothing known) and
//! is passed to curl on stdin, never in argv.
//!
//! # Hyprland IPC (`ipc.rs`)
//!
//! Every compositor-facing call goes through [`ipc`]: a direct client of
//! the request socket
//! `$XDG_RUNTIME_DIR/hypr/$HYPRLAND_INSTANCE_SIGNATURE/.socket.sock`, one
//! request per connection (write command, shutdown write, read reply to
//! EOF). Instance discovery: the env var first (empty counts as unset),
//! else the newest-mtime dir under `$XDG_RUNTIME_DIR/hypr/` — the
//! restart_crashed_lock.sh walk, so VT/recovery contexts still resolve.
//!
//! Wire format, verified live against Hyprland 0.55.4 (2026-07-03):
//! `j/<query>` returns JSON (`j/monitors all`, `j/workspaces`,
//! `j/activeworkspace`, `j/clients` verified); a leading `/` is rejected
//! ("unknown request") and `[[BATCH]]` is accepted but unnecessary for
//! single requests. `dispatch <cmd>` / `reload` / `eval <lua>` reply with
//! the literal `ok` on success, an error string on rejection (verified
//! with a no-op `hl.dsp.focus` refocus of the active workspace; for eval,
//! with a read-only `return 1+1` and an `hl.monitor` re-assert of a
//! disabled output, which no-ops per the AGENTS.md gotcha). hyprctl prints
//! those error strings to STDOUT and still exits 0 — a rejected dispatch
//! has never been a non-zero exit — and bsctl keeps that exit-code
//! contract.
//!
//! Fallback policy: on ANY socket failure (connect, io, unparseable JSON,
//! non-ok dispatch reply) the same request is retried by spawning hyprctl,
//! which always speaks the running compositor's protocol on this rolling
//! release — the safety net for wire-format drift. A non-ok reply means
//! the request was rejected, not half-executed, so the retry cannot
//! double-fire.
//!
//! The EVENT socket, `.socket2.sock` (same instance dir), is exposed via
//! `ipc::event_socket_path()`: connect, send nothing, read
//! newline-delimited `EVENT>>DATA` lines forever (verified live, Hyprland
//! 0.55.4). It has no hyprctl fallback — it is a stream, not a request —
//! so its consumer (the `--stream` engine) treats connect failure as
//! degraded mode.

pub mod agents;
pub mod asks;
pub mod display;
pub mod ipc;
pub mod mcp;
pub mod presence;
pub mod proto;
pub mod scale;
pub mod sessions;
pub mod stream;
pub mod sys;
pub mod usage;
pub mod world;
pub mod ws;
