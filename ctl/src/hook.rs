//! `bsctl hook <verb>` — the hook-event endpoint, mirroring
//! claude-ws-status.sh verb-for-verb. A hook must NEVER block or error
//! loudly, so every failure path is silent and the exit code is always 0:
//! bad JSON on stdin is an empty payload, a missing transcript is an empty
//! title, hyprctl being absent (or no owning window) means "write nothing".

use std::env;
use std::fs;
use std::io::{self, Read, Write};
use std::path::Path;

use serde_json::Value;

use crate::{proto, sys};

/// Dispatch a hook verb. `argv` is everything after the binary name and is
/// only used for the optional debug log line.
pub fn run(verb: Option<&str>, argv: &[String]) -> i32 {
    let Some(verb) = verb.filter(|v| !v.is_empty()) else {
        return 0; // the reference exits before even creating the state dir
    };
    let dir = sys::state_dir();
    let _ = fs::create_dir_all(&dir);

    let mut input = Vec::new();
    let _ = io::stdin().read_to_end(&mut input);

    if env::var_os("CLAUDE_WS_DEBUG").is_some_and(|v| !v.is_empty()) {
        debug_log(&dir, argv, &input);
    }

    match verb {
        "agent-start" => agent_start(&dir, &input),
        "agent-stop" => agent_stop(&dir, &input),
        "waiting" | "thinking" | "tooling" | "clear" => session_verb(&dir, verb, &input),
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

/// agent-start: write one subagent marker (fast path: no hyprctl). If the
/// payload lacks description or agent_type, fall back to the subagent's
/// meta.json next to the transcript.
fn agent_start(dir: &Path, input: &[u8]) {
    let d = proto::parse_payload(input);
    let aid = proto::field(&d, "agent_id");
    if aid.is_empty() {
        return;
    }
    let sid = proto::session_id(&d);
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

/// agent-stop: remove that subagent marker.
fn agent_stop(dir: &Path, input: &[u8]) {
    let d = proto::parse_payload(input);
    let aid = proto::field(&d, "agent_id");
    if aid.is_empty() {
        return;
    }
    let sid = proto::session_id(&d);
    let _ = fs::remove_file(dir.join(format!("{sid}.{aid}")));
}

/// waiting/thinking/tooling/clear — the session-status verbs.
fn session_verb(dir: &Path, verb: &str, input: &[u8]) {
    let d = proto::parse_payload(input);
    let sid = proto::session_id(&d);
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

    let (pids, claude_pid) = sys::ancestor_chain();
    // Fall back to our immediate parent so the pid field is never omitted.
    let pid = claude_pid.unwrap_or_else(|| std::os::unix::process::parent_id() as i64);

    // hyprctl boundary: no owning workspace -> exit silently, write nothing.
    let Some(ws) = sys::workspace_for_pids(&pids) else {
        return;
    };

    let mut title = String::new();
    if !tpath.is_empty()
        && Path::new(&tpath).is_file()
        && let Ok(bytes) = fs::read(&tpath)
    {
        title = proto::title_from_transcript(&String::from_utf8_lossy(&bytes));
    }
    let _ = write_session(dir, &sid, verb, &title, ws, pid);
}

/// The testable end of the session write path: everything hyprctl/proc
/// derived arrives as plain values (`ws` from workspace_for_pids, `pid` from
/// ancestor_chain), so tests can exercise the write without a compositor.
pub fn write_session(
    dir: &Path,
    sid: &str,
    status: &str,
    title: &str,
    ws: i64,
    pid: i64,
) -> io::Result<()> {
    sys::atomic_write_json(dir, sid, &proto::session_record(ws, status, title, pid))
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
