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
//! While `ask` blocks awaiting an answer, stdin is not read — a ping or
//! cancellation sent mid-block is answered late or never. Accepted: the
//! block is bounded (--block-secs, default 90), progress notifications keep
//! attentive clients patient, and the store-first design means a client
//! that gives up and kills us loses nothing (the ask is already posted; the
//! answer lands in the store and is collected via get_ask).

use std::io::{self, BufRead, Write};
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
                "Post a question to the human's attention queue and wait briefly for an answer. {NORM} \
                 If no answer arrives within the wait window the ask stays open — continue other work \
                 if you can, check back with get_ask, or end your turn; the human sees the queue."),
            "inputSchema": {
                "type": "object",
                "properties": {
                    "title": {"type": "string", "description": "One-line summary (the queue row the human triages by)"},
                    "body": {"type": "string", "description": "The full question with enough context to answer from the queue"},
                    "options": {"type": "array", "items": {"type": "string"},
                                "description": "The choices, if this is an A/B decision"},
                    "urgency": urgency,
                    "estimate_min": estimate,
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
                            ask's wait window, or re-check one of your open asks.",
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

/// The blocking loop for `ask`: poll the store every 500ms until the
/// deadline, emitting a progress notification roughly every 10s when the
/// client supplied a progressToken. Returns the tool text.
fn block_on_answer(
    id: i64,
    block_secs: u64,
    progress_token: Option<&Value>,
    out: &mut impl Write,
) -> String {
    let started = std::time::Instant::now();
    let deadline = started + Duration::from_secs(block_secs);
    let mut last_progress = started;
    loop {
        match block_verdict(asks::record(id).as_ref()) {
            Verdict::Answered(a) => return answered_text(id, &a),
            Verdict::Dismissed => return dismissed_text(id),
            Verdict::Open => {}
        }
        let now = std::time::Instant::now();
        if now >= deadline {
            return open_text(id);
        }
        if let Some(tok) = progress_token
            && now.duration_since(last_progress) >= Duration::from_secs(10)
        {
            last_progress = now;
            let note = json!({"jsonrpc": "2.0", "method": "notifications/progress",
                "params": {"progressToken": tok,
                           "progress": started.elapsed().as_secs(),
                           "total": block_secs}});
            let _ = writeln!(out, "{note}");
            let _ = out.flush();
        }
        std::thread::sleep(Duration::from_millis(500));
    }
}

/// Handle one tools/call; returns the result payload.
fn call_tool(
    name: &str,
    args: &Value,
    identity: &mut Identity,
    block_secs: u64,
    progress_token: Option<&Value>,
    out: &mut impl Write,
) -> Value {
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
    match name {
        "ask" | "notify" => {
            if s("title").is_empty() {
                return tool_text("title is required", true);
            }
            let (session, ws) = identity.get();
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
                &identity.kind,
                &session,
                ws,
            ) {
                Ok(id) => id,
                Err(e) => return tool_text(&format!("posting failed: {e}"), true),
            };
            if name == "notify" {
                return tool_text(&format!("Posted ask #{id} to the queue."), false);
            }
            tool_text(&block_on_answer(id, block_secs, progress_token, out), false)
        }
        "list_asks" => tool_text(&asks::get_json(None), false),
        "get_ask" => match args.get("id").and_then(Value::as_i64) {
            Some(id) => match asks::record(id) {
                Some(r) => tool_text(&r.to_string(), false),
                None => tool_text(&format!("no ask {id}"), true),
            },
            None => tool_text("id is required", true),
        },
        "update_ask" => {
            let Some(id) = args.get("id").and_then(Value::as_i64) else {
                return tool_text("id is required", true);
            };
            let (session, _) = identity.get();
            let owner = asks::record(id)
                .map(|r| crate::proto::field(&r, "session"))
                .unwrap_or_default();
            if owner != session {
                return tool_text(
                    &format!(
                        "ask {id} was not posted by this session; only your own asks can be updated"
                    ),
                    true,
                );
            }
            let u = s("urgency");
            let u = (!u.is_empty()).then(|| urgency("medium"));
            if u.is_none() && estimate.is_none() {
                return tool_text("nothing to update: pass urgency and/or estimate_min", true);
            }
            // The CLI verb enforces the same open-only + field rules; reuse
            // it so the two surfaces cannot drift. Its stderr is invisible
            // here — the generic text is enough (the caller can get_ask).
            match asks::update(id, u.as_deref(), estimate) {
                0 => tool_text(&format!("ask #{id} updated"), false),
                _ => tool_text(&format!("ask {id} is not open"), true),
            }
        }
        "world" => tool_text(&world::snapshot().to_string(), false),
        other => tool_text(&format!("unknown tool {other}"), true),
    }
}

/// One request line -> zero or one response lines (notifications produce
/// none). `out` is threaded through for the ask fast path's progress.
fn handle_line(
    line: &str,
    identity: &mut Identity,
    block_secs: u64,
    out: &mut impl Write,
) -> Option<String> {
    let Ok(msg) = serde_json::from_str::<Value>(line) else {
        return Some(rpc_error(&Value::Null, -32700, "parse error"));
    };
    let id = msg.get("id").cloned();
    let method = msg.get("method").and_then(Value::as_str).unwrap_or("");
    let params = msg.get("params").cloned().unwrap_or_else(|| json!({}));
    match (method, id) {
        // Responses to our notifications don't exist; a message with no
        // method is a client response — nothing of ours awaits one.
        ("", _) => None,
        ("initialize", Some(id)) => {
            let offered = params
                .get("protocolVersion")
                .and_then(Value::as_str)
                .unwrap_or("");
            Some(rpc_result(
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
        ("ping", Some(id)) => Some(rpc_result(&id, json!({}))),
        ("tools/list", Some(id)) => Some(rpc_result(&id, json!({"tools": tools_json()}))),
        ("tools/call", Some(id)) => {
            let name = params.get("name").and_then(Value::as_str).unwrap_or("");
            let args = params
                .get("arguments")
                .cloned()
                .unwrap_or_else(|| json!({}));
            let token = params.pointer("/_meta/progressToken").cloned();
            let result = call_tool(name, &args, identity, block_secs, token.as_ref(), out);
            Some(rpc_result(&id, result))
        }
        // Tolerated notifications: lifecycle chatter we need nothing from.
        ("notifications/initialized", None) | ("notifications/cancelled", None) => None,
        (_, Some(id)) => Some(rpc_error(&id, -32601, "method not found")),
        (_, None) => None, // unknown notification: ignore by contract
    }
}

/// The server loop: line in, response out, until stdin closes (the harness
/// ending the session is the shutdown signal — exit 0).
pub fn run(kind: &str, block_secs: u64) -> i32 {
    let mut identity = Identity {
        kind: kind.to_string(),
        resolved: None,
    };
    let stdin = io::stdin();
    let mut out = io::stdout();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        if let Some(resp) = handle_line(&line, &mut identity, block_secs, &mut out)
            && (writeln!(out, "{resp}").is_err() || out.flush().is_err())
        {
            break; // client gone mid-write: done, not broken
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

    #[test]
    fn protocol_shapes() {
        let mut ident = Identity {
            kind: "claude".into(),
            resolved: Some((String::new(), None)),
        };
        let mut sink = Vec::new();
        // initialize echoes a known version and carries instructions
        let resp = handle_line(
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26"}}"#,
            &mut ident, 0, &mut sink,
        )
        .unwrap();
        let v: Value = serde_json::from_str(&resp).unwrap();
        assert_eq!(v["result"]["protocolVersion"], "2025-03-26");
        assert_eq!(v["result"]["serverInfo"]["name"], "bsctl");
        assert!(!v["result"]["instructions"].as_str().unwrap().is_empty());
        // notifications produce nothing
        assert!(
            handle_line(
                r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
                &mut ident,
                0,
                &mut sink
            )
            .is_none()
        );
        assert!(
            handle_line(
                r#"{"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":9}}"#,
                &mut ident,
                0,
                &mut sink
            )
            .is_none()
        );
        // ping pongs; unknown methods error; garbage is a parse error
        let pong = handle_line(
            r#"{"jsonrpc":"2.0","id":2,"method":"ping"}"#,
            &mut ident,
            0,
            &mut sink,
        )
        .unwrap();
        assert!(pong.contains(r#""result":{}"#));
        let err = handle_line(
            r#"{"jsonrpc":"2.0","id":3,"method":"resources/list"}"#,
            &mut ident,
            0,
            &mut sink,
        )
        .unwrap();
        assert!(err.contains("-32601"));
        let parse = handle_line("not json", &mut ident, 0, &mut sink).unwrap();
        assert!(parse.contains("-32700"));
        assert!(sink.is_empty(), "no progress without a blocked ask");
    }
}
