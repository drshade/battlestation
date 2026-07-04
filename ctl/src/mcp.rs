//! `bsctl mcp --kind <k>` — a stdio MCP server giving every agent harness
//! the asks queue and read-only world queries (contract in lib.rs). One
//! server per session, spawned by the harness like the hooks are; `--kind`
//! is the same discriminator. Hand-rolled JSON-RPC over stdio lines rather
//! than an SDK: the surface is five requests and two notifications, the
//! crate hand-speaks its other socket protocol too, and the official rust
//! SDK would drag in an async runtime for what is a blocking line loop.
//!
//! The tool DESCRIPTIONS carry the posting norm (the one prompt surface
//! every session of every harness receives) — treat their wording as
//! contract, not copy. So does the initialize result's `instructions`
//! field, for clients that surface it.
//!
//! The agent owns its wait: `ask` takes `wait_secs` (0 = fire-and-forget;
//! omitted = the server's --block-secs default; capped at 24h). Because a
//! wait may now run to hours, the block is PROTOCOL-RESPONSIVE: stdin is
//! multiplexed with the store poll, so a liveness `ping` is ponged
//! immediately, `notifications/cancelled` for the in-flight call stops the
//! block (no response for a cancelled request, per spec — the ask STAYS
//! open: the store is truth and cancellation is transport, not triage),
//! any other request gets a busy error rather than silence, and stdin EOF
//! is shutdown. Progress notifications (~10s) keep resettable client
//! timeouts alive; the store-first design means a client that gives up and
//! kills us anyway loses nothing (the answer lands in the store and is
//! collected via get_ask).

use std::io::{self, Write};
use std::os::fd::RawFd;
use std::time::Duration;

use serde_json::{Value, json};

use crate::{asks, sessions, sys, world};

/// Spec revisions this server knows. The handshake echoes the client's
/// version when it is one of these (all three are wire-compatible for our
/// five-request surface) and otherwise answers with the newest we know —
/// the spec's prescribed downgrade dance. Verified live against Claude Code
/// 2.1.200, codex-cli 0.142.5 and agy 1.0.16 (2026-07-04).
pub const PROTOCOL_VERSIONS: &[&str] = &["2024-11-05", "2025-03-26", "2025-06-18"];

/// The version to answer a client offering `offered`.
pub fn choose_version(offered: &str) -> &'static str {
    PROTOCOL_VERSIONS
        .iter()
        .find(|v| **v == offered)
        .copied()
        .unwrap_or(PROTOCOL_VERSIONS[PROTOCOL_VERSIONS.len() - 1])
}

/// One JSON-RPC response line.
pub fn rpc_result(id: &Value, result: Value) -> String {
    json!({"jsonrpc": "2.0", "id": id, "result": result}).to_string()
}

/// One JSON-RPC error line.
pub fn rpc_error(id: &Value, code: i64, message: &str) -> String {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}}).to_string()
}

/// A tools/call result: one text content block; `is_error` marks tool-level
/// failure (the call itself succeeded — MCP's distinction).
pub fn tool_text(text: &str, is_error: bool) -> Value {
    json!({"content": [{"type": "text", "text": text}], "isError": is_error})
}

/// The posting norm, verbatim in `ask`'s description and the initialize
/// instructions. One source so the two can't drift.
const NORM: &str = "When you need the human's input — a question, a decision, \
a judgment call you cannot defend making alone — you MUST post an ask rather \
than proceed on an assumption. Queue depth, the human seeming busy, or your \
ask seeming small are NEVER reasons not to post: continuing without needed \
feedback is the failure mode this queue exists to prevent.";

/// The `tools/list` payload. Schemas are deliberately plain (string/int/
/// enum/array-of-string) — every harness's schema validator accepts them.
pub fn tools_json() -> Value {
    let urgency = json!({"type": "string", "enum": ["low", "medium", "high"],
        "description": "How urgently the human is needed (you own this; escalate via update_ask)"});
    let estimate = json!({"type": "integer", "minimum": 0,
        "description": "Your estimate of HUMAN minutes needed to handle this"});
    json!([
        {
            "name": "ask",
            "description": format!(
                "Post a question to the human's attention queue and wait for an answer. {NORM} \
                 YOU choose how long you are willing to wait via wait_secs: pass 0 when you have \
                 other work to continue (collect the answer later with get_ask); let the default \
                 ride for a quick back-and-forth; pass a long wait (minutes to hours) when you are \
                 truly blocked and waiting IS the right use of your time. Consider list_asks first — \
                 queue depth and the human's notes tell you how long an answer might take. If the \
                 wait expires the ask stays open; the human sees the queue either way."),
            "inputSchema": {
                "type": "object",
                "properties": {
                    "title": {"type": "string", "description": "One-line summary (the queue row the human triages by)"},
                    "body": {"type": "string", "description": "The full question with enough context to answer from the queue"},
                    "options": {"type": "array", "items": {"type": "string"},
                                "description": "The choices, if this is an A/B decision"},
                    "urgency": urgency,
                    "estimate_min": estimate,
                    "wait_secs": {"type": "integer", "minimum": 0,
                                  "description": "How long you are willing to wait for the answer, in seconds. \
                                   0 = post and return immediately. Omitted = the server default (90). \
                                   Values above 86400 (24h) are clamped."},
                },
                "required": ["title"],
            },
        },
        {
            "name": "notify",
            "description": "Post a non-blocking item to the human's attention queue: a review request, \
                            a completion report, anything the human should see even though you need no \
                            answer. Post these liberally — silent completion is almost as bad as a \
                            silent assumption. Returns the ask id immediately.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "title": {"type": "string", "description": "One-line summary (the queue row)"},
                    "body": {"type": "string", "description": "Detail the human reads from the queue"},
                    "type": {"type": "string", "enum": ["review", "notify"],
                             "description": "review = please look at something; notify = FYI/done"},
                    "urgency": urgency,
                    "estimate_min": estimate,
                },
                "required": ["title"],
            },
        },
        {
            "name": "list_asks",
            "description": "The current attention queue in triage order, every agent's asks included \
                            (age, urgency, estimate, the human's notes). Transparency for coordination — \
                            seeing a deep queue is NOT permission to skip posting; the posting norm is \
                            unconditional.",
            "inputSchema": {"type": "object", "properties": {}},
        },
        {
            "name": "get_ask",
            "description": "One ask by id, any state — how you collect an answer that arrived after \
                            ask's wait window, or re-check one of your open asks. NOTE: answer text \
                            on a still-OPEN ask is the human drafting a reply — visible for context, \
                            but not final until the ask's state is answered.",
            "inputSchema": {
                "type": "object",
                "properties": {"id": {"type": "integer", "description": "The ask id"}},
                "required": ["id"],
            },
        },
        {
            "name": "update_ask",
            "description": "Update YOUR OWN ask's urgency and/or estimate — escalation means raising \
                            urgency, never reordering (only the human orders the queue). Other \
                            sessions' asks and other fields are refused.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "id": {"type": "integer", "description": "The ask id (must be yours and open)"},
                    "urgency": urgency,
                    "estimate_min": estimate,
                },
                "required": ["id"],
            },
        },
        {
            "name": "world",
            "description": "The desktop's full state as JSON: the asks queue, displays, workspaces \
                            (battlespace order), workspace->display preferences, every agent session's \
                            status, and plan usage. Read-only.",
            "inputSchema": {"type": "object", "properties": {}},
        },
    ])
}

/// What one poll of a blocked ask concludes.
#[derive(Debug, PartialEq)]
pub enum Verdict {
    /// Answered: the reply text to return.
    Answered(String),
    /// Dismissed: the human declined; the asker proceeds on judgment.
    Dismissed,
    /// Still open (or the store lost it — same to the poller): keep waiting.
    Open,
}

/// The poll decision for [`Verdict`], pure for tests.
pub fn block_verdict(rec: Option<&Value>) -> Verdict {
    let Some(r) = rec else {
        return Verdict::Open; // store churn; the deadline bounds us
    };
    match r.get("state").and_then(Value::as_str).unwrap_or("open") {
        "answered" => Verdict::Answered(
            r.get("answer")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
        ),
        "dismissed" => Verdict::Dismissed,
        _ => Verdict::Open,
    }
}

/// The texts `ask` returns, shared by the fast path and its tests.
pub fn answered_text(id: i64, answer: &str) -> String {
    format!("The human answered ask #{id}: {answer}")
}
pub fn dismissed_text(id: i64) -> String {
    format!(
        "The human dismissed ask #{id} without answering — proceed with your best judgment and \
         do not re-post the same question."
    )
}
pub fn open_text(id: i64) -> String {
    format!(
        "No answer yet — ask #{id} remains open in the queue and the human sees it. Continue \
         other work if you can, check back later with get_ask, or end your turn."
    )
}
/// wait_secs = 0: deliberately distinct from the timeout text — the agent
/// chose not to wait, so there is no "no answer yet" to report.
pub fn posted_text(id: i64) -> String {
    format!(
        "Posted ask #{id} to the queue (not waiting, per wait_secs 0). Collect the answer later \
         with get_ask."
    )
}

/// The wait `ask` actually blocks for: the caller's explicit choice wins
/// (clamped to a 24h sanity cap), the server default covers omission.
pub const WAIT_CAP_SECS: u64 = 86_400;
pub fn effective_wait(requested: Option<u64>, default_secs: u64) -> u64 {
    requested.map_or(default_secs, |w| w.min(WAIT_CAP_SECS))
}

/// What a line arriving MID-BLOCK means. Pure classifier so the multiplex
/// policy is testable without pipes.
#[derive(Debug, PartialEq)]
pub enum MidBlock {
    /// Write this response line and keep blocking (ping pong, busy error,
    /// parse error).
    Reply(String),
    /// Our in-flight request was cancelled: stop blocking and send NOTHING
    /// for it (a cancelled request must not be answered, per spec). The ask
    /// stays open in the store — cancellation is transport, not triage.
    Cancelled,
    /// Lifecycle chatter / responses / foreign cancellations: keep blocking.
    Ignore,
}

pub fn classify_midblock(line: &str, ask_req_id: &Value, ask_id: i64) -> MidBlock {
    let Ok(msg) = serde_json::from_str::<Value>(line) else {
        return MidBlock::Reply(rpc_error(&Value::Null, -32700, "parse error"));
    };
    let id = msg.get("id");
    let method = msg.get("method").and_then(Value::as_str).unwrap_or("");
    match (method, id) {
        ("ping", Some(id)) => MidBlock::Reply(rpc_result(id, json!({}))),
        ("notifications/cancelled", None)
            if msg.pointer("/params/requestId") == Some(ask_req_id) =>
        {
            MidBlock::Cancelled
        }
        // Any other REQUEST would deadlock the client if we sat on it and
        // corrupt ordering if we queued it — an explicit busy error names
        // the way out (answer/dismiss the ask, or cancel this call).
        (m, Some(id)) if !m.is_empty() => MidBlock::Reply(rpc_error(
            id,
            -32000,
            &format!(
                "busy in a blocking ask (ask #{ask_id} is open — answer or dismiss it, or cancel the in-flight call)"
            ),
        )),
        _ => MidBlock::Ignore,
    }
}

/// The session this server speaks for. Resolved from our own /proc ancestry
/// (the harness process is our ancestor; its pid is in exactly one session
/// file) — an MCP server receives no session identity from any harness, so
/// the pid chain is the one honest source. Lazily retried while unresolved:
/// the server often spawns before the first hook writes the session file.
struct Identity {
    kind: String,
    resolved: Option<(String, Option<i64>)>,
}

impl Identity {
    fn get(&mut self) -> (String, Option<i64>) {
        if self.resolved.is_none() {
            let (_, harness) = sys::ancestor_chain(&self.kind);
            self.resolved =
                harness.and_then(|pid| sessions::session_by_pid(&sys::state_dir(), pid));
        }
        self.resolved.clone().unwrap_or((String::new(), None))
    }
}

/// Raw line reader over fd 0 with a carry buffer — BufReader would hide
/// pipelined bytes from poll(2) (buffered in userspace, invisible to the
/// fd), and the block loop must see EVERY line the moment it lands. Used by
/// both the main loop (infinite timeout) and the block loop (500ms slices).
struct LineReader {
    fd: RawFd,
    buf: Vec<u8>,
    eof: bool,
}

enum ReadOutcome {
    Line(String),
    Timeout,
    Eof,
}

impl LineReader {
    fn new() -> Self {
        LineReader {
            fd: 0,
            buf: Vec::new(),
            eof: false,
        }
    }

    /// A complete buffered line, if any (without the newline).
    fn pop_line(&mut self) -> Option<String> {
        let nl = self.buf.iter().position(|&b| b == b'\n')?;
        let line: Vec<u8> = self.buf.drain(..=nl).collect();
        Some(String::from_utf8_lossy(&line[..nl]).into_owned())
    }

    /// One wait slice: buffered line first, else poll(2) up to `timeout_ms`
    /// (negative = forever), one read(2), re-check. A partial line at slice
    /// end reads as Timeout — the carry buffer holds it for the next slice.
    fn wait_line(&mut self, timeout_ms: i32) -> ReadOutcome {
        if let Some(l) = self.pop_line() {
            return ReadOutcome::Line(l);
        }
        if self.eof {
            return ReadOutcome::Eof;
        }
        let mut pfd = [libc::pollfd {
            fd: self.fd,
            events: libc::POLLIN,
            revents: 0,
        }];
        loop {
            let r = unsafe { libc::poll(pfd.as_mut_ptr(), 1, timeout_ms) };
            if r == 0 {
                return ReadOutcome::Timeout;
            }
            if r > 0 {
                break; // POLLIN/POLLHUP both mean "read now" (HUP -> 0 = EOF)
            }
            if io::Error::last_os_error().kind() != io::ErrorKind::Interrupted {
                self.eof = true;
                return ReadOutcome::Eof;
            }
        }
        let mut chunk = [0u8; 4096];
        let n = loop {
            let n = unsafe { libc::read(self.fd, chunk.as_mut_ptr().cast(), chunk.len()) };
            if n >= 0 {
                break n as usize;
            }
            if io::Error::last_os_error().kind() != io::ErrorKind::Interrupted {
                self.eof = true;
                return ReadOutcome::Eof;
            }
        };
        if n == 0 {
            self.eof = true;
            return ReadOutcome::Eof;
        }
        self.buf.extend_from_slice(&chunk[..n]);
        match self.pop_line() {
            Some(l) => ReadOutcome::Line(l),
            None => ReadOutcome::Timeout, // partial line: carried forward
        }
    }

    /// The main loop's read: block until a line or EOF.
    fn next_line(&mut self) -> Option<String> {
        loop {
            match self.wait_line(-1) {
                ReadOutcome::Line(l) => return Some(l),
                ReadOutcome::Timeout => continue, // partial line landed
                ReadOutcome::Eof => return None,
            }
        }
    }
}

/// How a blocked `ask` ended.
enum BlockOutcome {
    /// Respond with this tool text (answered / dismissed / wait expired).
    Text(String),
    /// The call was cancelled: no response; the ask stays open.
    Cancelled,
    /// stdin died mid-block: shut the server down.
    Eof,
}

/// The blocking loop for `ask`: multiplex the store poll (500ms slices)
/// with stdin until the deadline. Store verdicts end the block; mid-block
/// lines are classified by [`classify_midblock`] (pings ponged, our
/// cancellation honored, other requests busy-erroed); a progress
/// notification flows roughly every 10s when the client supplied a
/// progressToken — on resettable client timeouts it is what keeps an
/// hour-scale wait alive.
fn block_on_answer(
    id: i64,
    wait_secs: u64,
    req_id: &Value,
    progress_token: Option<&Value>,
    reader: &mut LineReader,
    out: &mut impl Write,
) -> BlockOutcome {
    let started = std::time::Instant::now();
    let deadline = started + Duration::from_secs(wait_secs);
    let mut last_progress = started;
    loop {
        match block_verdict(asks::record(id).as_ref()) {
            Verdict::Answered(a) => return BlockOutcome::Text(answered_text(id, &a)),
            Verdict::Dismissed => return BlockOutcome::Text(dismissed_text(id)),
            Verdict::Open => {}
        }
        let now = std::time::Instant::now();
        if now >= deadline {
            return BlockOutcome::Text(open_text(id));
        }
        if let Some(tok) = progress_token
            && now.duration_since(last_progress) >= Duration::from_secs(10)
        {
            last_progress = now;
            let note = json!({"jsonrpc": "2.0", "method": "notifications/progress",
                "params": {"progressToken": tok,
                           "progress": started.elapsed().as_secs(),
                           "total": wait_secs}});
            let _ = writeln!(out, "{note}");
            let _ = out.flush();
        }
        match reader.wait_line(500) {
            ReadOutcome::Timeout => {}
            ReadOutcome::Eof => return BlockOutcome::Eof,
            ReadOutcome::Line(l) => match classify_midblock(&l, req_id, id) {
                MidBlock::Ignore => {}
                MidBlock::Cancelled => return BlockOutcome::Cancelled,
                MidBlock::Reply(resp) => {
                    if writeln!(out, "{resp}").is_err() || out.flush().is_err() {
                        return BlockOutcome::Eof;
                    }
                }
            },
        }
    }
}

/// What one handled line tells the server loop to do.
enum Flow {
    /// Write this response line.
    Respond(String),
    /// Nothing to write (notifications, a cancelled call).
    Silent,
    /// stdin died mid-block: exit the loop cleanly.
    Shutdown,
}

/// The per-connection state a tool call may need: identity resolution, the
/// wait default, and the stdin reader the ask block multiplexes.
struct Srv<'a> {
    identity: &'a mut Identity,
    default_wait: u64,
    reader: &'a mut LineReader,
}

/// Handle one tools/call; returns the flow for the whole request (the ask
/// path may end cancelled — no response — or discover EOF mid-block).
fn call_tool(
    name: &str,
    args: &Value,
    req_id: &Value,
    srv: &mut Srv,
    progress_token: Option<&Value>,
    out: &mut impl Write,
) -> Flow {
    let s = |k: &str| args.get(k).and_then(Value::as_str).unwrap_or("");
    let urgency = |d: &str| {
        let u = s("urgency");
        if ["low", "medium", "high"].contains(&u) {
            u.to_string()
        } else {
            d.to_string()
        }
    };
    let estimate = args.get("estimate_min").and_then(Value::as_i64);
    let done = |v: Value| Flow::Respond(rpc_result(req_id, v));
    match name {
        "ask" | "notify" => {
            if s("title").is_empty() {
                return done(tool_text("title is required", true));
            }
            let (session, ws) = srv.identity.get();
            let ask_type = if name == "ask" {
                "question"
            } else if s("type") == "review" {
                "review"
            } else {
                "notify"
            };
            let options: Vec<String> = args
                .get("options")
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(Value::as_str)
                        .map(str::to_string)
                        .collect()
                })
                .unwrap_or_default();
            let id = match asks::create(
                ask_type,
                s("title"),
                s("body"),
                &options,
                &urgency("medium"),
                estimate,
                &srv.identity.kind,
                &session,
                ws,
            ) {
                Ok(id) => id,
                Err(e) => return done(tool_text(&format!("posting failed: {e}"), true)),
            };
            if name == "notify" {
                return done(tool_text(&format!("Posted ask #{id} to the queue."), false));
            }
            let wait = effective_wait(
                args.get("wait_secs").and_then(Value::as_u64),
                srv.default_wait,
            );
            if wait == 0 {
                return done(tool_text(&posted_text(id), false));
            }
            match block_on_answer(id, wait, req_id, progress_token, srv.reader, out) {
                BlockOutcome::Text(t) => done(tool_text(&t, false)),
                BlockOutcome::Cancelled => Flow::Silent,
                BlockOutcome::Eof => Flow::Shutdown,
            }
        }
        "list_asks" => done(tool_text(&asks::get_json(None), false)),
        "get_ask" => match args.get("id").and_then(Value::as_i64) {
            Some(id) => match asks::record(id) {
                Some(r) => done(tool_text(&r.to_string(), false)),
                None => done(tool_text(&format!("no ask {id}"), true)),
            },
            None => done(tool_text("id is required", true)),
        },
        "update_ask" => {
            let Some(id) = args.get("id").and_then(Value::as_i64) else {
                return done(tool_text("id is required", true));
            };
            let (session, _) = srv.identity.get();
            let owner = asks::record(id)
                .map(|r| crate::proto::field(&r, "session"))
                .unwrap_or_default();
            if owner != session {
                return done(tool_text(
                    &format!(
                        "ask {id} was not posted by this session; only your own asks can be updated"
                    ),
                    true,
                ));
            }
            let u = s("urgency");
            let u = (!u.is_empty()).then(|| urgency("medium"));
            if u.is_none() && estimate.is_none() {
                return done(tool_text(
                    "nothing to update: pass urgency and/or estimate_min",
                    true,
                ));
            }
            // The CLI verb enforces the same open-only + field rules; reuse
            // it so the two surfaces cannot drift. Its stderr is invisible
            // here — the generic text is enough (the caller can get_ask).
            match asks::update(id, u.as_deref(), estimate) {
                0 => done(tool_text(&format!("ask #{id} updated"), false)),
                _ => done(tool_text(&format!("ask {id} is not open"), true)),
            }
        }
        "world" => done(tool_text(&world::snapshot().to_string(), false)),
        other => done(tool_text(&format!("unknown tool {other}"), true)),
    }
}

/// One request line -> zero or one response lines (notifications produce
/// none). `out` is threaded through for the ask fast path's progress.
fn handle_line(line: &str, srv: &mut Srv, out: &mut impl Write) -> Flow {
    let Ok(msg) = serde_json::from_str::<Value>(line) else {
        return Flow::Respond(rpc_error(&Value::Null, -32700, "parse error"));
    };
    let id = msg.get("id").cloned();
    let method = msg.get("method").and_then(Value::as_str).unwrap_or("");
    let params = msg.get("params").cloned().unwrap_or_else(|| json!({}));
    match (method, id) {
        // Responses to our notifications don't exist; a message with no
        // method is a client response — nothing of ours awaits one.
        ("", _) => Flow::Silent,
        ("initialize", Some(id)) => {
            let offered = params
                .get("protocolVersion")
                .and_then(Value::as_str)
                .unwrap_or("");
            Flow::Respond(rpc_result(
                &id,
                json!({
                    "protocolVersion": choose_version(offered),
                    "capabilities": {"tools": {"listChanged": false}},
                    "serverInfo": {"name": "bsctl", "version": env!("CARGO_PKG_VERSION")},
                    "instructions": format!(
                        "This server is the desktop's shared attention queue. {NORM} \
                         Use notify for anything the human should see without needing an answer."),
                }),
            ))
        }
        ("ping", Some(id)) => Flow::Respond(rpc_result(&id, json!({}))),
        ("tools/list", Some(id)) => Flow::Respond(rpc_result(&id, json!({"tools": tools_json()}))),
        ("tools/call", Some(id)) => {
            let name = params.get("name").and_then(Value::as_str).unwrap_or("");
            let args = params
                .get("arguments")
                .cloned()
                .unwrap_or_else(|| json!({}));
            let token = params.pointer("/_meta/progressToken").cloned();
            call_tool(name, &args, &id, srv, token.as_ref(), out)
        }
        // Tolerated notifications: lifecycle chatter we need nothing from
        // OUTSIDE a block (a cancellation landing here names a request that
        // already completed — nothing to cancel).
        ("notifications/initialized", None) | ("notifications/cancelled", None) => Flow::Silent,
        (_, Some(id)) => Flow::Respond(rpc_error(&id, -32601, "method not found")),
        (_, None) => Flow::Silent, // unknown notification: ignore by contract
    }
}

/// The server loop: line in, response out, until stdin closes (the harness
/// ending the session is the shutdown signal — exit 0). Reads through
/// `LineReader` — the same carry buffer the block loop multiplexes — so
/// pipelined requests are never stranded in a BufReader the block loop's
/// poll(2) cannot see.
pub fn run(kind: &str, default_wait: u64) -> i32 {
    let mut identity = Identity {
        kind: kind.to_string(),
        resolved: None,
    };
    let mut reader = LineReader::new();
    let mut out = io::stdout();
    // The reader is borrowed twice per iteration (next_line here, the block
    // loop inside handle_line) — sequentially, never at once.
    while let Some(line) = reader.next_line() {
        if line.trim().is_empty() {
            continue;
        }
        let mut srv = Srv {
            identity: &mut identity,
            default_wait,
            reader: &mut reader,
        };
        match handle_line(&line, &mut srv, &mut out) {
            Flow::Silent => {}
            Flow::Shutdown => break,
            Flow::Respond(resp) => {
                if writeln!(out, "{resp}").is_err() || out.flush().is_err() {
                    break; // client gone mid-write: done, not broken
                }
            }
        }
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_negotiation_echoes_known_else_newest() {
        assert_eq!(choose_version("2025-06-18"), "2025-06-18");
        assert_eq!(choose_version("2024-11-05"), "2024-11-05");
        assert_eq!(choose_version("2099-01-01"), "2025-06-18");
        assert_eq!(choose_version(""), "2025-06-18");
    }

    #[test]
    fn block_verdict_reads_states() {
        assert_eq!(block_verdict(None), Verdict::Open);
        let mk = |state: &str, answer: Value| json!({"state": state, "answer": answer});
        assert_eq!(block_verdict(Some(&mk("open", Value::Null))), Verdict::Open);
        assert_eq!(
            block_verdict(Some(&mk("answered", json!("go with blue")))),
            Verdict::Answered("go with blue".into())
        );
        assert_eq!(
            block_verdict(Some(&mk("dismissed", Value::Null))),
            Verdict::Dismissed
        );
    }

    #[test]
    fn tools_carry_the_norm_and_plain_schemas() {
        let tools = tools_json();
        let names: Vec<&str> = tools
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap())
            .collect();
        assert_eq!(
            names,
            [
                "ask",
                "notify",
                "list_asks",
                "get_ask",
                "update_ask",
                "world"
            ]
        );
        for t in tools.as_array().unwrap() {
            assert!(
                !t["description"].as_str().unwrap().trim().is_empty(),
                "{} needs a description",
                t["name"]
            );
            assert_eq!(t["inputSchema"]["type"], "object");
        }
        // the norm rides ask's description verbatim
        assert!(tools[0]["description"].as_str().unwrap().contains(NORM));
    }

    /// handle_line sugar for tests: Respond -> Some(line), else None
    /// (Shutdown never occurs on these inputs).
    fn drive(line: &str, ident: &mut Identity, sink: &mut Vec<u8>) -> Option<String> {
        let mut reader = LineReader::new();
        let mut srv = Srv {
            identity: ident,
            default_wait: 0,
            reader: &mut reader,
        };
        match handle_line(line, &mut srv, sink) {
            Flow::Respond(r) => Some(r),
            _ => None,
        }
    }

    #[test]
    fn protocol_shapes() {
        let mut ident = Identity {
            kind: "claude".into(),
            resolved: Some((String::new(), None)),
        };
        let mut sink = Vec::new();
        // initialize echoes a known version and carries instructions
        let resp = drive(
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26"}}"#,
            &mut ident, &mut sink,
        )
        .unwrap();
        let v: Value = serde_json::from_str(&resp).unwrap();
        assert_eq!(v["result"]["protocolVersion"], "2025-03-26");
        assert_eq!(v["result"]["serverInfo"]["name"], "bsctl");
        assert!(!v["result"]["instructions"].as_str().unwrap().is_empty());
        // notifications produce nothing
        assert!(
            drive(
                r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
                &mut ident,
                &mut sink
            )
            .is_none()
        );
        assert!(
            drive(
                r#"{"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":9}}"#,
                &mut ident,
                &mut sink
            )
            .is_none()
        );
        // ping pongs; unknown methods error; garbage is a parse error
        let pong = drive(
            r#"{"jsonrpc":"2.0","id":2,"method":"ping"}"#,
            &mut ident,
            &mut sink,
        )
        .unwrap();
        assert!(pong.contains(r#""result":{}"#));
        let err = drive(
            r#"{"jsonrpc":"2.0","id":3,"method":"resources/list"}"#,
            &mut ident,
            &mut sink,
        )
        .unwrap();
        assert!(err.contains("-32601"));
        let parse = drive("not json", &mut ident, &mut sink).unwrap();
        assert!(parse.contains("-32700"));
        assert!(sink.is_empty(), "no progress without a blocked ask");
    }

    #[test]
    fn wait_is_agent_owned_with_a_sane_cap() {
        assert_eq!(effective_wait(None, 90), 90); // omitted -> server default
        assert_eq!(effective_wait(Some(0), 90), 0); // fire-and-forget
        assert_eq!(effective_wait(Some(3600), 90), 3600); // hours are legitimate
        assert_eq!(effective_wait(Some(999_999), 90), WAIT_CAP_SECS); // clamped
        assert_eq!(effective_wait(None, 0), 0); // server may default to no block
    }

    #[test]
    fn midblock_classification() {
        let req = json!(7);
        // ping is ponged while blocked
        match classify_midblock(r#"{"jsonrpc":"2.0","id":42,"method":"ping"}"#, &req, 3) {
            MidBlock::Reply(r) => {
                assert!(r.contains(r#""id":42"#) && r.contains(r#""result":{}"#))
            }
            other => panic!("{other:?}"),
        }
        // OUR cancellation stops the block; a foreign one is ignored
        assert_eq!(
            classify_midblock(
                r#"{"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":7}}"#,
                &req,
                3
            ),
            MidBlock::Cancelled
        );
        assert_eq!(
            classify_midblock(
                r#"{"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":8}}"#,
                &req,
                3
            ),
            MidBlock::Ignore
        );
        // another REQUEST gets a busy error naming the open ask
        match classify_midblock(
            r#"{"jsonrpc":"2.0","id":43,"method":"tools/list"}"#,
            &req,
            3,
        ) {
            MidBlock::Reply(r) => assert!(r.contains("-32000") && r.contains("ask #3"), "{r}"),
            other => panic!("{other:?}"),
        }
        // garbage is answered as a parse error, not ignored
        match classify_midblock("junk", &req, 3) {
            MidBlock::Reply(r) => assert!(r.contains("-32700")),
            other => panic!("{other:?}"),
        }
        // responses / other notifications are ignored
        assert_eq!(
            classify_midblock(r#"{"jsonrpc":"2.0","id":9,"result":{}}"#, &req, 3),
            MidBlock::Ignore
        );
    }
}
