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
//! # claude-ws status protocol (`bsctl hook` / `bsctl poll`)
//!
//! Reports each Claude Code instance's status for the Noctalia
//! claude-workspaces widget. State lives as small JSON files in one flat
//! dir, `${XDG_RUNTIME_DIR:-/tmp}/claude-ws/`:
//!
//! - `<session_id>` — one file per live session:
//!   `{"ws": <int>, "status": "waiting"|"thinking"|"tooling",
//!     "kind": "claude", "title": "<aiTitle>", "pid": <int>}`.
//!   `ws` is the Hyprland workspace id owning the session's terminal window
//!   (found by walking /proc ancestors against `hyprctl clients -j`). `pid`
//!   is the Claude Code process — the nearest ancestor whose comm is exactly
//!   `claude` — and the poll side sweeps session files whose pid is dead.
//!   `status` is semantic; mapping states to colours is the widget's
//!   concern. `kind` identifies the agent (`claude` here; the widget is
//!   ready for codex/gemini/... writing the same protocol with their own
//!   kind). `title` is the session's current aiTitle — the LAST
//!   `"type":"ai-title"` record in the transcript, whitespace-collapsed to
//!   one line — shown in the bot's hover tooltip.
//! - `<session_id>.<agent_id>` — one marker per RUNNING subagent:
//!   `{"type": "<agent_type>", "description": "<text>"}`.
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
//! Hook side (`bsctl hook <verb>`, hook-event JSON on stdin; silent-tolerant
//! by contract — always exits 0, even on unknown/missing verbs):
//! - `waiting`/`thinking`/`tooling` — (re)write the session file with that
//!   status; events carrying an agent_id also refresh (or recreate/heal)
//!   that subagent marker. A payload WITHOUT a session_id (empty/absent/bad
//!   JSON) is a silent no-op for all four session verbs: falling back to a
//!   "default" session file would fabricate a phantom session whose pid is
//!   the caller's own claude ancestor, unsweepable while that process lives.
//!   Only the marker names of agent-start/agent-stop keep the historical
//!   "default" sid fallback (an orphan marker is swept by the next poll).
//! - `clear` — remove the session file AND its subagent markers.
//! - `agent-start` / `agent-stop` — write/remove one subagent marker
//!   (fast path: no hyprctl), falling back to the subagent's meta.json next
//!   to the transcript for missing type/description.
//!
//! Poll side (`bsctl poll`): one flat pass emitting a single JSON array line
//! (`[{sid, ws, status, kind, title, agents: [{id, type, description,
//! started}]}]`), sweeping dead-pid sessions, orphan markers and stale
//! subagent markers on the way.
//!
//! # Workspace display order (`bsctl ws`)
//!
//! Navigate/move by DISPLAY POSITION instead of Hyprland's immutable
//! workspace id. Hyprland has no way to renumber a workspace, so
//! "reordering" is purely a display-layer remap: real ids stay put, and a
//! persisted preference list maps position <-> real id. The claude-workspaces
//! bar plugin renders the same order (pills are labelled by position) and
//! writes the same file when a pill is dragged.
//!
//! The order file, `${XDG_STATE_HOME:-$HOME/.local/state}/claude-workspaces/
//! order`, is just real ids in preferred order, space-separated with a
//! trailing newline: `3 1 2 5 4\n`. The plugin FileView-watches and parses
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
pub mod ws;
