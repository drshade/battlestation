//! bsctl — battlestation repo control tool.
//!
//! Owns the desktop's stateful protocols (AGENTS.md ownership rule: bsctl
//! owns stateful protocols and JSON-heavy logic; shell owns glue and
//! recovery). This header is the durable home of those protocol contracts —
//! originally the headers of `claude-ws-status.sh`, `ws.sh` and
//! `claude-usage.sh`, which remain only as executable references/rollback
//! while the bsctl trial runs. Observable behavior is byte-for-byte
//! compatible with them, so consumers can switch freely.
//!
//! # battlestation-ws status protocol (`bsctl hook` / `bsctl poll`)
//!
//! Reports each agent-harness instance's status for the Noctalia
//! battlestation-workspaces widget. The protocol is multi-harness: ONE flat dir
//! holds sessions from ANY harness (Claude Code today; codex/gemini/...
//! write the same records via `bsctl hook --kind <harness>`), discriminated
//! per session by `kind`. State lives as small JSON files in
//! `${XDG_RUNTIME_DIR:-/tmp}/battlestation-ws/`:
//!
//! - `<session_id>` — one file per live session:
//!   `{"ws": <int>, "status": "waiting"|"thinking"|"tooling",
//!     "kind": "<harness>", "title": "<aiTitle>", "pid": <int>}`.
//!   `ws` is the Hyprland workspace id owning the session's terminal window
//!   (found by walking /proc ancestors against `hyprctl clients -j`). `pid`
//!   is the harness process — the nearest ancestor whose comm matches the
//!   kind (harness binaries are named after their kind: comm `claude` /
//!   `codex`, both verified live; comm is the kernel's 15-char truncation,
//!   so longer kinds match on their truncation) — and the poll side sweeps
//!   session files whose pid is dead. `status` is semantic; mapping states
//!   to colours is the widget's concern. `kind` is the harness
//!   discriminator — `bsctl hook --kind`, MANDATORY, no default: a kindless
//!   call is a silent no-op, so an outdated caller visibly stops updating
//!   instead of guessing a harness. The widget renders per-kind (kindBySid
//!   feeds each bot's `kind`) and falls back to the Claude presentation for
//!   unknown kinds, so a new harness needs no widget change to appear.
//!   `title` (the bot's hover tooltip) is kind-agnostic precedence, first
//!   non-empty wins: (1) the LAST `"type":"ai-title"` record in the
//!   transcript, whitespace-collapsed to one line (Claude; harmlessly empty
//!   for harnesses without ai-title records); (2) derived from the payload
//!   when it carries `prompt` (Codex UserPromptSubmit):
//!   `basename(cwd): <first non-blank prompt line>` capped at 60 chars;
//!   (3) STICKY — the title already in the session file, so a title set
//!   once persists across events that carry nothing; (4) "".
//! - `<session_id>.<agent_id>` — one marker per RUNNING subagent:
//!   `{"type": "<agent_type>", "description": "<text>"}`. Markers carry no
//!   `kind` — a sub-agent inherits its session's kind in the widget.
//!   Marker MTIME is the subagent's start time (shown as elapsed time in the
//!   widget) and is refreshed by the agent's own tool calls, which is what
//!   keeps the poll side's GC honest: a live agent's marker never goes
//!   stale, a killed agent's (no SubagentStop fires) does, and is swept
//!   after 30 minutes unless its subagent transcript is still being
//!   written. Session ids are UUIDs and agent ids hex — neither contains a
//!   dot — so splitting a marker name on the FIRST dot is unambiguous.
//!
//! Writers are atomic (temp file + rename in the same dir); temp names are
//! `.<name>.tmp`, and the leading dot is what makes the poll side's dotfile
//! skip (and `clear`'s `<sid>.*` sweep) ignore them. `debug.log` (appended
//! when `CLAUDE_WS_DEBUG` is set: argv + raw stdin per hook call) is
//! diagnostics, not protocol — the poll side skips it by name.
//!
//! Hook side (`bsctl hook --kind <harness> <verb>`, hook-event JSON on
//! stdin; silent-tolerant by contract — always exits 0, even on
//! unknown/missing verbs or a missing/valueless/empty `--kind` (a no-op:
//! kind is mandatory), which is why --kind is parsed in hook.rs rather than
//! by clap):
//! - `waiting`/`thinking`/`tooling` — (re)write the session file with that
//!   status; events carrying an agent_id also refresh (or recreate/heal)
//!   that subagent marker. The session key is `session_id`, falling back to
//!   `conversationId` — Antigravity (`agy`) hook payloads are protojson
//!   camelCase and identify the session by conversationId only (verified
//!   against the contract docs embedded in the agy binary). A payload with
//!   NEITHER (empty/absent/bad JSON) is a silent no-op for all four session
//!   verbs: falling back to a "default" session file would fabricate a
//!   phantom session whose pid is the caller's own claude ancestor,
//!   unsweepable while that process lives. Only the marker names of
//!   agent-start/agent-stop keep the historical "default" sid fallback (an
//!   orphan marker is swept by the next poll).
//! - `clear` — remove the session file AND its subagent markers.
//! - `agent-start` / `agent-stop` — write/remove one subagent marker
//!   (fast path: no hyprctl), falling back to the subagent's meta.json next
//!   to the transcript for missing type/description.
//!
//! Poll side (`bsctl poll`): one flat pass emitting a single JSON array line
//! (`[{sid, ws, status, kind, title, agents: [{id, type, description,
//! started}]}]`), sweeping dead-pid sessions, orphan markers and stale
//! subagent markers on the way. NOTE the deliberate divergence from watch:
//! poll stays this bare sessions array (a stable one-shot debugging tool);
//! watch nests the same array inside an object.
//!
//! Watch side (`bsctl watch`): a long-lived daemon folding BOTH state
//! sources the widget renders — agent state and compositor state — into
//! `<state-dir>/.widget.json`, so the widget FileView-watches one file
//! instead of polling anything. The file is one compact JSON object plus
//! trailing newline, written atomically:
//!
//! `{"sessions": [ ...exactly poll's array... ],
//!   "compositor": {"workspaces": [{id, name, monitor, windows}],
//!                  "monitors": [{name, x, y, focused, activeWs,
//!                                specialShowing}]}}`
//!
//! The compositor section is built fresh each recompute from
//! `ipc::json("workspaces")` + `ipc::json("monitors")` (~1ms socket
//! queries). Workspaces exclude `special:*` names (the bar's rule);
//! `activeWs` is the monitor's activeWorkspace id; `specialShowing` is
//! whether its specialWorkspace name is non-empty. If either query fails,
//! `"compositor": null` is emitted and the widget keeps its last compositor
//! state. The dot-prefixed output name makes it structurally invisible to
//! poll's dotfile skip and clear's `<sid>.*` sweep. Single writer with
//! seamless failover: an exclusive flock on `<state-dir>/.widget.lock` —
//! every bar instance runs one watcher, losers block in flock as hot
//! standbys and take over the moment the winner dies (on acquiring: write
//! once, then loop). Rewrites are driven by inotify events on non-dot state
//! entries AND by Hyprland's `.socket2.sock` event stream (both coalesced
//! ~50ms), plus a 10s tick (pid death and marker aging are invisible to
//! inotify), and land only when the serialization actually changed, so the
//! widget is never woken for nothing. The socket2 events that trigger a
//! recompute: workspace(v2), createworkspace(v2), destroyworkspace(v2),
//! moveworkspace(v2), renameworkspace, focusedmon(v2), monitoradded(v2),
//! monitorremoved(v2), openwindow, closewindow, movewindow(v2),
//! activespecial(v2), configreloaded; everything else — notably the noisy
//! windowtitle*/activewindow*, which nothing rendered depends on — is
//! ignored. A `monitoraddedv2` additionally applies the arriving output's
//! workspace->display preferences — the one place watch dispatches instead
//! of mirroring (see "Workspace->display preference"). A
//! socket2 disconnect (compositor restart) rides the transient
//! error path: log to stderr once, sleep 1s, re-init (lock included) and
//! reconnect. If socket2 can't connect at all (no Hyprland), watch runs
//! DEGRADED — agent state only, compositor null. `bsctl poll` remains a
//! subcommand — one-shot debugging and the documented fallback if watch
//! misbehaves. On acquiring the lock, the watcher also runs a best-effort
//! one-time migration of protocol files out of the pre-rename `claude-ws`
//! dir, and of the order-file state dir out of the pre-rename
//! `claude-workspaces` dir (see watch.rs).
//!
//! # Workspace display order (`bsctl ws`)
//!
//! Navigate/move by DISPLAY POSITION instead of Hyprland's immutable
//! workspace id. Hyprland has no way to renumber a workspace, so
//! "reordering" is purely a display-layer remap: real ids stay put, and a
//! persisted preference list maps position <-> real id. The
//! battlestation-workspaces bar plugin renders the same order (pills are
//! labelled by position) and drives `bsctl ws set` when a pill is dragged —
//! bsctl is the file's only writer.
//!
//! The order file, `${XDG_STATE_HOME:-$HOME/.local/state}/
//! battlestation-workspaces/order`, is just real ids in preferred order,
//! space-separated with a trailing newline: `3 1 2 5 4\n`. The plugin FileView-watches and parses
//! this exact format, so writes must stay byte-compatible, and `reset`
//! truncates the file IN PLACE (the truncation is what fires the watch).
//!
//! Resolved display order = the preferred ids that currently exist (in
//! preference order), followed by any live workspaces not listed, ascending.
//! Missing/empty file => identity (1,2,3,...). "Live" excludes workspaces
//! whose name starts with `special:` — matching what the bar shows.
//!
//! Dispatch goes through the Lua API (`hl.dsp.*` strings), because the Lua
//! config parser rejects legacy `hyprctl dispatch workspace N` forms; the
//! exact strings this crate emits are the script-era known-good ones.
//! Renaming never renumbers: `rename <id> [name]` only sets the display name
//! (empty name resets it to the id's number).
//!
//! ## Display numbering (`display` / `movetodisplay`)
//!
//! Display N = the Nth ENABLED output sorted by (x, y) position, 1-based —
//! leftmost is display 1. Like workspace positions, the number is a pure
//! display-layer remap (output names stay the compositor's; ties break by
//! name). The Lua dispatcher forms — `hl.dsp.focus({ monitor })` and
//! `hl.dsp.workspace.move({ workspace, monitor })` — were discovered by
//! live probing (Hyprland 0.55.4; the legacy `focusmonitor`/
//! `moveworkspacetomonitor` names are rejected by the Lua parser) and are
//! pinned byte-for-byte in tests. `movetodisplay` pins
//! focus explicitly after the move (follow -> the moved workspace, stay ->
//! back to the source display) rather than trusting the move's
//! version-dependent inherent focus. Workspace ids are untouched by monitor
//! moves, so the order file needs no migration when workspaces change
//! displays.
//!
//! ## Workspace->display preference (the `ws` preference verbs / watch)
//!
//! Hyprland evacuates a disappearing display's workspaces to the survivors
//! and never moves them back on replug — so each workspace can have a
//! preferred display, and an arriving display collects its workspaces.
//! `${XDG_STATE_HOME:-$HOME/.local/state}/battlestation-workspaces/
//! preferred` is the order file's sibling with the same persistence
//! rationale: a preference is user INTENT, and intent outlives boots
//! (workspace ids are stable habits under global numbering). Plain text,
//! one `<id> <output>` pair per line, sorted by id, trailing newline;
//! missing/empty file = no preferences; unparseable lines are skipped
//! (one corrupt line loses one preference, never the file); writes are
//! atomic (temp+rename).
//!
//! SPARSE BY DESIGN: most workspaces have no preference and are entirely
//! Hyprland's business — a new workspace is born unmanaged and stays so
//! until the user explicitly homes it, and a reconcile only ever touches
//! the homed few. Preferences are stamped at INTENT time, never at
//! teardown time (when a display disappears there is nothing to record
//! and no race to lose — Hyprland's evacuation is left alone), and the
//! only writers are the two homing verbs: `ws prefer <id> [<output>]` —
//! explicit; a named output is accepted verbatim even when absent
//! (pre-declaring a home is legitimate), unnamed stamps the live
//! workspace's current display — and `ws movetodisplay`, because an
//! explicit move IS the user homing that workspace (unconditional
//! overwrite). Reorders (`ws set`, pill drags) and every bulk operation
//! never stamp. `ws forget (<id> | --all)` deletes. Watch and Hyprland never stamp: evacuations
//! and automatic restores are not intent — that asymmetry IS the model.
//!
//! APPLYING: `ws prefs` lists the preferences with reality annotations
//! (output presence, workspace liveness); `ws reconcile` moves every live
//! workspace that prefers a PRESENT output and sits elsewhere, via the
//! bounded settle machinery (`ws::restore_strays`) — a display gaining
//! workspaces then SHOWS one of its preferred ones (never a dead id —
//! focusing a dead id would CREATE it), other displays keep their view,
//! and the keyboard ends on the focused display. `bsctl watch` runs the
//! same apply scoped to the arriving output on `monitoraddedv2` — watch's
//! ONLY dispatch power; everywhere else it remains a passive mirror of
//! compositor and agent state.
//!
//! # Usage cache (`bsctl usage`)
//!
//! Fetches Claude Code plan usage from the OAuth usage endpoint (the same
//! data as `/usage`) and emits one compact JSON line for the widget:
//! `{"sessionPct": <int>, "sessionResets": "...", "weeklyPct": <int>,
//! "weeklyResets": "..."}`.
//!
//! The reading is cached at `${XDG_CACHE_HOME:-$HOME/.cache}/
//! claude-usage.json` (ttl 240s) so the per-monitor pollers share a single
//! API call; refreshes are serialized with flock(2) on `<cache>.lock` —
//! losers block, then serve whatever the winner cached. On anything but a
//! clean 200 with a well-formed body, nothing is printed and the cache is
//! kept, so a rate-limited (429) or expired-token (401) request never
//! clobbers a good value; the widget just keeps its current numbers. The
//! OAuth token comes from `~/.claude/.credentials.json`
//! (`.claudeAiOauth.accessToken`; missing/unreadable -> silent exit 0) and
//! is passed to curl on stdin, never in argv.
//!
//! # Hyprland IPC (`ipc.rs`)
//!
//! Every compositor-facing call goes through [`ipc`]: a direct client of the
//! request socket
//! `$XDG_RUNTIME_DIR/hypr/$HYPRLAND_INSTANCE_SIGNATURE/.socket.sock`, one
//! request per connection (write command, shutdown write, read reply to
//! EOF). Instance discovery: the env var first (empty counts as unset), else
//! the newest-mtime dir under `$XDG_RUNTIME_DIR/hypr/` — the
//! restart_crashed_lock.sh walk, so VT/recovery contexts still resolve.
//!
//! Wire format, verified live against Hyprland 0.55.4 (2026-07-03):
//! `j/<query>` returns JSON (`j/monitors all`, `j/workspaces`,
//! `j/activeworkspace`, `j/clients` verified); a leading `/` is rejected
//! ("unknown request") and `[[BATCH]]` is accepted but unnecessary for
//! single requests. `dispatch <cmd>` / `reload` / `eval <lua>` reply with
//! the literal `ok` on success, an error string on rejection (verified with
//! a no-op `hl.dsp.focus` refocus of the active workspace; for eval, with a
//! read-only `return 1+1` and an `hl.monitor` re-assert of a disabled
//! output, which no-ops per the AGENTS.md gotcha). hyprctl prints those
//! error strings to STDOUT and still exits 0 — a rejected dispatch has never
//! been a non-zero exit — and bsctl keeps that exit-code contract.
//!
//! Fallback policy: on ANY socket failure (connect, io, unparseable JSON,
//! non-ok dispatch reply) the same request is retried by spawning hyprctl,
//! which always speaks the running compositor's protocol on this rolling
//! release — the safety net for wire-format drift. A non-ok reply means the
//! request was rejected, not half-executed, so the retry cannot double-fire.
//!
//! The EVENT socket, `.socket2.sock` (same instance dir), is exposed via
//! `ipc::event_socket_path()`: connect, send nothing, read newline-delimited
//! `EVENT>>DATA` lines forever (verified live, Hyprland 0.55.4). It has no
//! hyprctl fallback — it is a stream, not a request — so its one consumer
//! (`bsctl watch`) treats connect failure as degraded mode.
//!
//! # Display management (`bsctl display`)
//!
//! The human/diagnostic tool for display state; the AUTOMATED recovery paths
//! stay shell per AGENTS.md (hypridle's after_sleep_cmd -> displays-on.sh,
//! the lid binds -> clamshell.sh, restart_crashed_lock.sh forever).
//!
//! Core semantic constraint (AGENTS.md gotcha): the dpms dispatcher is
//! TOGGLE-ONLY. The string form `hl.dsp.dpms("on")` toggles EVERY monitor;
//! the table form `hl.dsp.dpms({ monitor = "X" })` toggles exactly one;
//! neither can "set". So every dpms mutation here is read-before-toggle:
//! read `dpmsStatus` from `j/monitors all`, emit the TABLE-FORM toggle only
//! when the state differs. Corollaries: `on`/`off <output>` are idempotent
//! by construction, and disabled outputs are refused (their dpmsStatus is
//! meaningless — a lid-disabled panel reports dpms on — and runtime
//! dpms/eval calls no-op on disabled outputs; `reset`/reload re-enables).
//!
//! - `status [--json]` — per-output table (from `j/monitors all`) + lid
//!   state (`/proc/acpi/button/lid/*/state`; no lid device -> desktop, line
//!   omitted) + WARNING lines with remedies (internal panel enabled while
//!   lid closed, mixed dpms, enabled output with dpms off). Warnings never
//!   affect the exit code — it is a report.
//! - `on|off <output>` — safe dpms targeting as above; unknown outputs error
//!   listing the valid names.
//! - `reset` — displays-on.sh's reset flow: `reload` (re-applies
//!   monitors.lua), shell out to clamshell.sh auto (it OWNS the lid policy),
//!   then dpms-on all enabled outputs with bounded re-read retries.
//!
//! # Display scale (`bsctl display scale`)
//!
//! Step the FOCUSED monitor's scale up/down a fixed ladder at runtime,
//! preserving its mode — display-scale.sh verb-for-verb. The ladder is
//! `1.0 1.25 1.5 1.75 2.0 2.5 3.0` (rung 0 = native). Hyprland snaps
//! fractional scales to its own 1/120 grid and the achievable values are
//! irregular per panel, so the REPORTED scale can't drive deterministic
//! stepping — the rung INDEX is persisted per monitor instead, at
//! `${XDG_RUNTIME_DIR:-/tmp}/hypr-display-scale.<name>` (the index in
//! decimal + newline, e.g. `1\n`; runtime-dir state, gone on reboot — like
//! scale itself, which reverts to monitors.lua on reload).
//!
//! `up`/`down` start from the saved index when present, else the rung
//! nearest the reported scale (ties toward the lower rung); step ±1; clamp
//! to the ladder; persist the NEW index; then eval
//! `hl.monitor({ output = "<name>", mode = "<WxH@RR>", position = "auto",
//! scale = <rung formatted %.5f> })` — mode preserved from the monitor's
//! current WxH + python-`round()`ed refresh rate. `reset` deletes the state
//! file and evals the same chunk with `scale = "auto"` — the quoted Lua
//! STRING, vs a bare Lua number for ladder rungs; the quoting is semantic.
//! No focused monitor (or a malformed focused entry): silent exit 0. State
//! is written before the dispatch, so the exit code is the eval's and a
//! rejected eval still leaves the stepped index persisted (script parity).

pub mod display;
pub mod hook;
pub mod ipc;
pub mod poll;
pub mod proto;
pub mod scale;
pub mod sys;
pub mod usage;
pub mod watch;
pub mod ws;
