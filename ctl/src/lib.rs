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
//!   that subagent marker.
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

pub mod hook;
pub mod poll;
pub mod proto;
pub mod sys;
pub mod usage;
pub mod ws;
