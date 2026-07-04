//! The session scan — one flat pass over the battlestation-ws state dir,
//! yielding one record per live session:
//!
//! `{sid, ws, win, status, kind, title, agents: [{id, type, description, started}]}`
//!
//! (The record keys are the historical on-disk spellings; the `agents get` /
//! `status` surfaces remap them to the published `session`/`subagents`
//! schema.)
//!
//! Self-cleaning: a session file whose pid is dead OR that doesn't parse is
//! deleted along with its markers, as are orphan markers whose session file
//! is gone. Dotfiles are the writer's in-flight temp files; skip them (and
//! debug.log). GC backstop: a subagent killed mid-turn fires NO SubagentStop,
//! so its marker would leak until session end — sweep markers untouched for
//! GC_SECS whose transcript is missing or equally frozen.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::{proto, sys};

/// `os.path.expanduser("~/.claude/projects")` — via $HOME. ($HOME is always
/// set under a session; if it somehow isn't, the glob just never matches and
/// stale markers lose their transcript rescue, same as a bogus HOME.)
pub fn projects_dir() -> PathBuf {
    std::env::var_os("HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("~"))
        .join(".claude/projects")
}

/// The whole scan pass, with `now` and both roots injected for tests.
pub fn scan(now: f64, dir: &Path, proj: &Path) -> Vec<Value> {
    let names: Vec<String> = match fs::read_dir(dir) {
        Ok(rd) => rd
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect(),
        Err(_) => Vec::new(),
    };

    let mut sess: Vec<String> = Vec::new();
    let mut marks: HashMap<String, Vec<String>> = HashMap::new();
    for n in names {
        if n.starts_with('.') || n == "debug.log" {
            continue;
        }
        match proto::split_marker_name(&n) {
            Some((sid, aid)) => marks
                .entry(sid.to_string())
                .or_default()
                .push(aid.to_string()),
            None => sess.push(n),
        }
    }
    sess.sort();

    fn rm(p: &Path) {
        let _ = fs::remove_file(p);
    }

    let mut out = Vec::new();
    for sid in sess {
        let p = dir.join(&sid);
        let rec: Option<Value> = fs::read(&p)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok());
        // ok = the file parsed AND /proc/<int(pid)> exists; any failure on
        // the way (no pid key, non-int pid, non-object record) counts as dead.
        let alive = rec
            .as_ref()
            .and_then(|r| r.get("pid"))
            .and_then(proto::py_int)
            .is_some_and(|pid| Path::new(&format!("/proc/{pid}")).exists());
        let Some(rec) = rec.filter(|_| alive) else {
            rm(&p);
            for aid in marks.remove(&sid).unwrap_or_default() {
                rm(&dir.join(format!("{sid}.{aid}")));
            }
            continue;
        };

        let mut agents: Vec<(f64, String, Value)> = Vec::new();
        for aid in marks.remove(&sid).unwrap_or_default() {
            let mp = dir.join(format!("{sid}.{aid}"));
            // Any failure below (stat, read, parse) skips the agent silently
            // WITHOUT deleting the marker — the reference's `except: pass`.
            let Some(started) = fs::metadata(&mp).ok().as_ref().and_then(sys::mtime_f64) else {
                continue;
            };
            if now - started > proto::GC_SECS
                && proto::gc_should_delete(now, started, transcript_mtime(proj, &sid, &aid))
            {
                rm(&mp);
                continue;
            }
            let Some(m) = fs::read(&mp)
                .ok()
                .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
            else {
                continue;
            };
            agents.push((
                started,
                aid.clone(),
                json!({
                    "id": aid,
                    "type": proto::field(&m, "type"),
                    "description": proto::field(&m, "description"),
                    "started": started as i64, // int() truncation
                }),
            ));
        }
        // sort key (started, id) — started as the FLOAT mtime, like the reference
        agents.sort_by(|a, b| {
            a.0.partial_cmp(&b.0)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.1.cmp(&b.1))
        });
        let agents: Vec<Value> = agents.into_iter().map(|t| t.2).collect();

        out.push(json!({
            "sid": sid,
            "ws": rec.get("ws").cloned().unwrap_or(Value::Null),
            "win": rec.get("win").cloned().unwrap_or(Value::Null),
            "status": proto::field(&rec, "status"),
            "kind": proto::field_or(&rec, "kind", "claude"),
            "title": proto::field(&rec, "title"),
            "agents": agents,
        }));
    }

    // Orphan markers: their session file is gone (or was swept above).
    for (sid, aids) in marks {
        for aid in aids {
            rm(&dir.join(format!("{sid}.{aid}")));
        }
    }
    out
}

/// The session whose record carries `pid` — the MCP server's identity
/// resolution: its own /proc ancestor walk names the harness process, and
/// the session file that recorded the same pid IS its session. Returns
/// (sid, ws). A plain read, no sweeping — identity lookups must not race
/// the scans that own GC.
pub fn session_by_pid(dir: &Path, pid: i64) -> Option<(String, Option<i64>)> {
    for e in fs::read_dir(dir).ok()?.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') || name == "debug.log" || proto::split_marker_name(&name).is_some()
        {
            continue;
        }
        let Some(rec) = fs::read(e.path())
            .ok()
            .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
        else {
            continue;
        };
        if rec.get("pid").and_then(proto::py_int) == Some(pid) {
            return Some((name, rec.get("ws").and_then(proto::py_int)));
        }
    }
    None
}

/// `glob(PROJ + "/*/<sid>/subagents/agent-<aid>.jsonl")` — first match's
/// mtime, or None. glob's `*` never matches dotfiles, so hidden project dirs
/// are skipped.
fn transcript_mtime(proj: &Path, sid: &str, aid: &str) -> Option<f64> {
    for e in fs::read_dir(proj).ok()?.flatten() {
        if e.file_name().to_string_lossy().starts_with('.') {
            continue;
        }
        let cand = e
            .path()
            .join(sid)
            .join("subagents")
            .join(format!("agent-{aid}.jsonl"));
        if let Ok(md) = fs::metadata(&cand) {
            return sys::mtime_f64(&md);
        }
    }
    None
}
