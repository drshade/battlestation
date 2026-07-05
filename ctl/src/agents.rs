//! `bsctl agents` — the agent-session surface. `agents set` is the
//! hook-event endpoint the harness configs call (hook-event JSON on stdin),
//! mirroring claude-ws-status.sh verb-for-verb; `agents get` is the
//! readable query over the same state. A hook must NEVER block or error
//! loudly, so every `set` failure path is silent and the exit code is
//! always 0: bad JSON on stdin is an empty payload, a missing transcript is
//! an empty title, hyprctl being absent (or no owning window) means "write
//! nothing". `--kind` is mandatory — a kindless call is a silent no-op (an
//! outdated caller must stop updating, not guess a harness).

use std::env;
use std::fs;
use std::io::{self, Read, Write};
use std::path::Path;

use serde_json::{Value, json};

use crate::{ipc, proto, sessions, sys};

/// Tolerantly parse everything after `agents set`: `--kind <k>`/`--kind=<k>`,
/// an optional `--session-id <s>`/`--session-id=<s>` override, and the first
/// remaining token as the verb. Parsed HERE, not by clap: a valueless
/// `--kind` (a miswired hook command) must exit 0 silently like every other
/// hook malformation, and a clap `--kind <k>` option errors loudly (exit 2 +
/// stderr) when the value is missing — there is no clap configuration that
/// keeps a required-value option silent. So clap hands the raw tokens
/// through and the tolerant parse lives with the rest of the endpoint's
/// tolerance. `--kind` is MANDATORY (no default): an absent/valueless/empty
/// kind makes the whole call a silent no-op, so an outdated kindless caller
/// simply stops updating — the visible symptom that says "rewire me"
/// without ever breaking a hook. A trailing valueless flag swallows only its
/// own value slot; flag-shaped junk falls into the verb slot and dies as an
/// unknown verb, silently.
fn parse_args(args: &[String]) -> (String, String, Option<&str>) {
    let mut kind = String::new();
    let mut session = String::new();
    let mut verb: Option<&str> = None;
    let mut i = 0;
    while i < args.len() {
        let a = args[i].as_str();
        if a == "--kind" {
            kind = args.get(i + 1).cloned().unwrap_or_default();
            i += 2;
        } else if let Some(v) = a.strip_prefix("--kind=") {
            kind = v.to_string();
            i += 1;
        } else if a == "--session-id" {
            session = args.get(i + 1).cloned().unwrap_or_default();
            i += 2;
        } else if let Some(v) = a.strip_prefix("--session-id=") {
            session = v.to_string();
            i += 1;
        } else {
            if verb.is_none() {
                verb = Some(a);
            }
            i += 1;
        }
    }
    (kind, session, verb)
}

/// Dispatch an `agents set` call: `args` is everything after `set` (the
/// mandatory `--kind`, the verb, the optional `--session-id` override);
/// `argv` is everything after the binary name and is only used for the
/// optional debug log line.
pub fn set(args: &[String], argv: &[String]) -> i32 {
    let (kind, session, verb) = parse_args(args);
    let Some(verb) = verb.filter(|v| !v.is_empty()) else {
        return 0; // the reference exits before even creating the state dir
    };
    if kind.is_empty() {
        return 0; // kindless caller = outdated wiring: write NOTHING, silently
    }
    let dir = sys::state_dir();
    let _ = fs::create_dir_all(&dir);

    let mut input = Vec::new();
    let _ = io::stdin().read_to_end(&mut input);

    if env::var_os("CLAUDE_WS_DEBUG").is_some_and(|v| !v.is_empty()) {
        debug_log(&dir, argv, &input);
    }

    // The argv override outranks the payload's session key everywhere a
    // session id is derived — the payload stays the normal source (harness
    // hooks deliver ids inside the event JSON, not on argv).
    let over = (!session.is_empty()).then_some(session.as_str());
    match verb {
        "subagent-start" => agent_start(&dir, &input, over),
        "subagent-stop" => agent_stop(&dir, &input, over),
        "waiting" | "thinking" | "tooling" | "clear" => {
            session_verb(&dir, verb, &input, &kind, over)
        }
        _ => {} // unknown verbs are ignored, exit 0
    }
    0
}

/// Append `=== <ts> argv: ...` + the raw stdin payload to <dir>/debug.log.
fn debug_log(dir: &Path, argv: &[String], input: &[u8]) {
    let Ok(mut f) = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("debug.log"))
    else {
        return;
    };
    let _ = writeln!(f, "=== {} argv: {}", sys::debug_ts(), argv.join(" "));
    let _ = f.write_all(input);
    let _ = f.write_all(b"\n");
}

/// subagent-start: write one subagent marker (fast path: no hyprctl). If
/// the payload lacks description or agent_type, fall back to the subagent's
/// meta.json next to the transcript.
fn agent_start(dir: &Path, input: &[u8], over: Option<&str>) {
    let d = proto::parse_payload(input);
    let aid = proto::field(&d, "agent_id");
    if aid.is_empty() {
        return;
    }
    let sid = over
        .map(str::to_string)
        .unwrap_or_else(|| proto::session_id(&d));
    let mut atype = proto::field(&d, "agent_type");
    let mut desc = proto::field(&d, "description");
    if desc.is_empty() || atype.is_empty() {
        read_meta_fallback(&d, &sid, &aid, &mut desc, &mut atype);
    }
    let _ = sys::atomic_write_json(
        dir,
        &format!("{sid}.{aid}"),
        &proto::marker_record(&atype, &desc),
    );
}

/// subagent-stop: remove that subagent marker.
fn agent_stop(dir: &Path, input: &[u8], over: Option<&str>) {
    let d = proto::parse_payload(input);
    let aid = proto::field(&d, "agent_id");
    if aid.is_empty() {
        return;
    }
    let sid = over
        .map(str::to_string)
        .unwrap_or_else(|| proto::session_id(&d));
    let _ = fs::remove_file(dir.join(format!("{sid}.{aid}")));
}

/// waiting/thinking/tooling/clear — the session-status verbs. `kind` is the
/// harness discriminator from the mandatory `--kind`; it lands in the
/// session record only — markers carry no kind (sub-agents inherit their
/// session's kind in the widget).
fn session_verb(dir: &Path, verb: &str, input: &[u8], kind: &str, over: Option<&str>) {
    let d = proto::parse_payload(input);
    // No session key -> silent no-op. Falling back to a "default" session
    // file here would FABRICATE a session: its pid would be whatever claude
    // process is our ancestor, so the scan could never sweep it while that
    // process lives (observed live 2026-07-02 — a payload-less test
    // invocation planted a phantom bot on the bar). The "default" fallback
    // survives only in the marker names of agent-start/agent-stop, where an
    // orphan is swept by the next scan. The key is session_id, or agy's
    // camelCase conversationId (proto::session_key).
    let sid = over
        .map(str::to_string)
        .unwrap_or_else(|| proto::session_key(&d));
    if sid.is_empty() {
        return;
    }
    let tpath = proto::field(&d, "transcript_path");

    // Subagent-context events carry agent_id (Pre/PostToolUse fired from
    // inside a subagent). Refresh that marker: bump mtime if it exists,
    // recreate if missing. This keeps the poll side's kill-leak GC safe — a
    // LIVE agent keeps its marker fresh via its own tool calls, while a
    // killed agent (which fires no SubagentStop) goes permanently stale.
    let aid = proto::field(&d, "agent_id");
    if !aid.is_empty() {
        let mp = dir.join(format!("{sid}.{aid}"));
        if mp.exists() {
            let _ = sys::touch_now(&mp);
            heal_marker(dir, &d, &sid, &aid, &mp);
        } else {
            let mut atype = proto::field(&d, "agent_type");
            // NB: unlike agent-start, the reference does NOT read description
            // from tool payloads here — it comes from meta.json or stays "".
            let mut desc = String::new();
            read_meta_fallback(&d, &sid, &aid, &mut desc, &mut atype);
            let _ = sys::atomic_write_json(
                dir,
                &format!("{sid}.{aid}"),
                &proto::marker_record(&atype, &desc),
            );
        }
    }

    if verb == "clear" {
        clear_session(dir, &sid);
        return;
    }

    let (pids, harness_pid) = sys::ancestor_chain(kind);
    // Fall back to our immediate parent so the pid field is never omitted.
    let pid = harness_pid.unwrap_or_else(|| std::os::unix::process::parent_id() as i64);

    // compositor boundary: no owning workspace -> exit silently, write
    // nothing. The window address and terminal pid ride the same clients
    // row (best-effort — the address upgrades focus-by-session from
    // workspace to window; term_pid is the terminal owning the window).
    let Some((ws, win, term_pid)) = ipc::client_for_pids(&pids) else {
        return;
    };

    // Title precedence (proto::derive_title), kind-agnostic: transcript
    // ai-title (empty for harnesses without one, e.g. Codex) -> derived from
    // the payload's prompt+cwd (Codex UserPromptSubmit) -> sticky: whatever
    // title the session file already carries -> "".
    let mut transcript_title = String::new();
    if !tpath.is_empty()
        && Path::new(&tpath).is_file()
        && let Ok(bytes) = fs::read(&tpath)
    {
        transcript_title = proto::title_from_transcript(&String::from_utf8_lossy(&bytes));
    }
    let prompt_title =
        proto::title_from_prompt(&proto::field(&d, "cwd"), &proto::field(&d, "prompt"));
    let title = proto::derive_title(&transcript_title, &prompt_title, &existing_title(dir, &sid));
    let _ = write_session(
        dir,
        &sid,
        verb,
        &title,
        ws,
        win.as_deref(),
        pid,
        term_pid,
        kind,
    );
}

/// The sticky-title source: the session file's current title, "" when the
/// file is missing/unparseable (first event of a session).
fn existing_title(dir: &Path, sid: &str) -> String {
    fs::read(dir.join(sid))
        .ok()
        .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
        .map(|v| proto::field(&v, "title"))
        .unwrap_or_default()
}

/// The testable end of the session write path: everything hyprctl/proc
/// derived arrives as plain values (`ws`/`win`/`term_pid` from
/// client_for_pids, `pid` from ancestor_chain), so tests can exercise the
/// write without a compositor.
#[allow(clippy::too_many_arguments)]
pub fn write_session(
    dir: &Path,
    sid: &str,
    status: &str,
    title: &str,
    ws: i64,
    win: Option<&str>,
    pid: i64,
    term_pid: Option<i64>,
    kind: &str,
) -> io::Result<()> {
    sys::atomic_write_json(
        dir,
        sid,
        &proto::session_record(ws, win, status, title, pid, term_pid, kind),
    )
}

/// `rm -f <dir>/<sid> <dir>/<sid>.*` — the session file and all its markers.
/// In-flight temp files start with "." so the prefix sweep never eats them.
fn clear_session(dir: &Path, sid: &str) {
    let _ = fs::remove_file(dir.join(sid));
    let prefix = format!("{sid}.");
    if let Ok(rd) = fs::read_dir(dir) {
        for e in rd.flatten() {
            if e.file_name().to_string_lossy().starts_with(&prefix) {
                let _ = fs::remove_file(e.path());
            }
        }
    }
}

/// Self-heal a marker with empty type/description (bsctl addition, not in the
/// sh reference): SubagentStart can fire before the agent's meta.json exists
/// (observed live 2026-07-03), leaving a marker with "" fields the touch-only
/// refresh would preserve forever. On tool-call refreshes, refill empty
/// fields from the payload's agent_type + the (by now written) meta.json and
/// rewrite only if something was gained.
fn heal_marker(dir: &Path, d: &Value, sid: &str, aid: &str, mp: &Path) {
    let Some(m) = fs::read(mp)
        .ok()
        .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
    else {
        return;
    };
    let mut atype = proto::field(&m, "type");
    let mut desc = proto::field(&m, "description");
    if !atype.is_empty() && !desc.is_empty() {
        return;
    }
    let before = (atype.clone(), desc.clone());
    if atype.is_empty() {
        atype = proto::field(d, "agent_type");
    }
    read_meta_fallback(d, sid, aid, &mut desc, &mut atype);
    if (atype.clone(), desc.clone()) != before {
        let _ = sys::atomic_write_json(
            dir,
            &format!("{sid}.{aid}"),
            &proto::marker_record(&atype, &desc),
        );
    }
}

/// Shared meta.json fallback:
/// `dirname(transcript_path)/<sid>/subagents/agent-<aid>.meta.json`,
/// filling only the still-empty fields (description / agentType).
fn read_meta_fallback(d: &Value, sid: &str, aid: &str, desc: &mut String, atype: &mut String) {
    let tp = proto::field(d, "transcript_path");
    if tp.is_empty() {
        return;
    }
    let path = proto::meta_json_path(&tp, sid, aid);
    let Ok(bytes) = fs::read(&path) else { return };
    let Ok(meta) = serde_json::from_slice::<Value>(&bytes) else {
        return;
    };
    proto::apply_meta_fallback(&meta, desc, atype);
}

/// The scan records remapped to the published schema, filtered: the on-disk
/// `sid`/`agents` spellings become `session`/`subagents`. Pure — `status`
/// builds its agents section from the same rows, so the two surfaces can
/// never drift.
pub fn agent_rows(recs: Vec<Value>, kind: Option<&str>, session: Option<&str>) -> Vec<Value> {
    recs.into_iter()
        .filter(|r| kind.is_none_or(|k| proto::field(r, "kind") == k))
        .filter(|r| session.is_none_or(|s| proto::field(r, "sid") == s))
        .map(|r| {
            json!({
                "session": r.get("sid").cloned().unwrap_or(Value::Null),
                "kind": r.get("kind").cloned().unwrap_or(Value::Null),
                "status": r.get("status").cloned().unwrap_or(Value::Null),
                "ws": r.get("ws").cloned().unwrap_or(Value::Null),
                "win": r.get("win").cloned().unwrap_or(Value::Null),
                // pid = the harness process; term_pid = the terminal owning
                // its window (kitty). Both null when unresolved.
                "pid": r.get("pid").cloned().unwrap_or(Value::Null),
                "term_pid": r.get("term_pid").cloned().unwrap_or(Value::Null),
                "title": r.get("title").cloned().unwrap_or(Value::Null),
                "subagents": r.get("agents").cloned().unwrap_or_else(|| json!([])),
            })
        })
        .collect()
}

/// One `agents get` result as its JSON line — the streaming form re-runs
/// exactly this (an empty state dir is an honest `[]`, so it never fails).
pub fn get_json(kind: Option<&str>, session: Option<&str>) -> String {
    let recs = sessions::scan(sys::now_f64(), &sys::state_dir(), &sessions::projects_dir());
    Value::Array(agent_rows(recs, kind, session)).to_string()
}

/// `agents get [--kind K] [--session-id S] [--format json]` — the readable
/// query over the session state, sweeping exactly like the scan it wraps
/// (dead pids, orphan/stale markers). Empty results print nothing, not a
/// lonely header.
pub fn get(kind: Option<&str>, session: Option<&str>, json_out: bool) -> i32 {
    if json_out {
        println!("{}", get_json(kind, session));
    } else {
        print!("{}", get_text(kind, session));
    }
    0
}

/// One `agents get` text result — the table (newline-terminated), or ""
/// for no sessions. Shared by the one-shot form and its `--stream` text
/// framing; like [`get_json`], it never fails.
pub fn get_text(kind: Option<&str>, session: Option<&str>) -> String {
    let recs = sessions::scan(sys::now_f64(), &sys::state_dir(), &sessions::projects_dir());
    let rows = agent_rows(recs, kind, session);
    if rows.is_empty() {
        return String::new();
    }
    proto::render_table(&AGENT_HEADERS, &agent_cells(&rows)) + "\n"
}

/// The agents table's shape, shared with `status`'s agents section. The
/// identifying-but-long SESSION uuid sits last so the columns eyes actually
/// scan (kind/status/where/title) come first. `pid`/`term_pid` are
/// deliberately JSON-only: they are machine data (scripting, signalling a
/// process), and two numeric columns would only clutter a human glance.
pub const AGENT_HEADERS: [&str; 6] = ["KIND", "STATUS", "WS", "SUBAGENTS", "TITLE", "SESSION"];

/// [`AGENT_HEADERS`]'s cells for one set of published rows (SUBAGENTS empty
/// at zero — most sessions run none).
pub fn agent_cells(rows: &[Value]) -> Vec<Vec<String>> {
    rows.iter()
        .map(|r| {
            let subs = r["subagents"].as_array().map_or(0, Vec::len);
            vec![
                proto::field(r, "kind"),
                proto::field(r, "status"),
                r.get("ws")
                    .and_then(Value::as_i64)
                    .map(|w| w.to_string())
                    .unwrap_or_default(),
                if subs == 0 {
                    String::new()
                } else {
                    subs.to_string()
                },
                proto::field(r, "title"),
                proto::field(r, "session"),
            ]
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn arg_parse_is_tolerant() {
        let p = |v: &[&str]| {
            let a = s(v);
            let (k, sid, verb) = parse_args(&a);
            (k, sid, verb.map(str::to_string))
        };
        assert_eq!(
            p(&["waiting"]),
            ("".into(), "".into(), Some("waiting".to_string()))
        );
        assert_eq!(
            p(&["--kind", "codex", "tooling"]),
            ("codex".into(), "".into(), Some("tooling".to_string()))
        );
        assert_eq!(
            p(&["--kind=codex", "tooling"]),
            ("codex".into(), "".into(), Some("tooling".to_string()))
        );
        // valueless --kind swallows the verb slot -> silent no-op upstream
        assert_eq!(p(&["--kind"]), ("".into(), "".into(), None));
        // --kind with a value but no verb -> no verb, still silent
        assert_eq!(p(&["--kind", "codex"]), ("codex".into(), "".into(), None));
        // empty kind value -> kindless -> silent no-op upstream (kind is
        // mandatory; there is no default)
        assert_eq!(
            p(&["--kind=", "waiting"]),
            ("".into(), "".into(), Some("waiting".to_string()))
        );
        // flag-shaped junk is just an unknown verb
        assert_eq!(
            p(&["--bogus"]),
            ("".into(), "".into(), Some("--bogus".to_string()))
        );
        assert_eq!(parse_args(&[]), ("".into(), "".into(), None));
        // the --session-id override parses in any position, both spellings
        assert_eq!(
            p(&["--kind", "codex", "waiting", "--session-id", "sx"]),
            ("codex".into(), "sx".into(), Some("waiting".to_string()))
        );
        assert_eq!(
            p(&["--session-id=sx", "--kind=codex", "clear"]),
            ("codex".into(), "sx".into(), Some("clear".to_string()))
        );
        // a valueless --session-id swallows only its own slot
        assert_eq!(
            p(&["--kind", "codex", "waiting", "--session-id"]),
            ("codex".into(), "".into(), Some("waiting".to_string()))
        );
    }
}
