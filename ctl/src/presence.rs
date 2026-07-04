//! `bsctl presence` — is the human at the desk? hypridle is the machine's
//! idle authority: its stowed listener reports `set idle` after the idle
//! timeout and `set active` on resume; bsctl only records what it is told
//! and serves it back (the world's `human` section, `presence get`, and
//! the MCP ask-timeout note). Contract in lib.rs.

use std::fs;

use serde_json::{Value, json};

use crate::{asks, sys};

/// `<asks-dir>/presence.json` — the asks dir on purpose: presence is
/// attention-adjacent state, the stream engine already watches that dir
/// (a non-dot name rides the existing trigger filter), and nothing else
/// reads the dir by pattern. Runtime lifetime like its neighbors.
pub fn presence_file() -> std::path::PathBuf {
    asks::asks_dir().join("presence.json")
}

/// The recorded report, if any; missing/corrupt reads as no report (the
/// desk before hypridle's first transition — "unknown", not a guess).
pub fn read() -> Option<Value> {
    let v: Value = serde_json::from_str(&fs::read_to_string(presence_file()).ok()?).ok()?;
    v.is_object().then_some(v)
}

// ---- pure logic (proto.rs-style: deterministic, unit-tested) ---------------

/// The record a `set` writes. Setting the SAME state twice keeps the old
/// `since`: hypridle's on-timeout can fire again without an intervening
/// resume (a re-arm after a brief wake the listener never saw as input),
/// and the idle duration must accumulate across those fires, not reset.
pub fn updated(prev: Option<&Value>, state: &str, now: f64) -> Value {
    let since = prev
        .filter(|p| p.get("state").and_then(Value::as_str) == Some(state))
        .and_then(|p| p.get("since").and_then(Value::as_f64))
        .unwrap_or(now);
    json!({"state": state, "since": since})
}

/// Seconds idle, only meaningful while idle: None when active/unknown.
fn idle_secs(rec: Option<&Value>, now: f64) -> Option<i64> {
    let r = rec?;
    if r.get("state").and_then(Value::as_str) != Some("idle") {
        return None;
    }
    let since = r.get("since").and_then(Value::as_f64)?;
    Some((now - since).max(0.0) as i64)
}

/// The world's `human` section: `{"state", "idle_secs"}` — "unknown" when
/// no report exists (file truth; never null — an unwritten file is not a
/// lost compositor).
pub fn human_json(rec: Option<&Value>, now: f64) -> Value {
    let state = rec
        .and_then(|r| r.get("state").and_then(Value::as_str))
        .unwrap_or("unknown");
    json!({"state": state, "idle_secs": idle_secs(rec, now)})
}

/// `presence get --format json`: the `human` shape plus the raw `since`.
pub fn get_json(rec: Option<&Value>, now: f64) -> Value {
    let mut v = human_json(rec, now);
    v["since"] = rec
        .and_then(|r| r.get("since").cloned())
        .unwrap_or(Value::Null);
    v
}

/// The one-line text: `active` / `idle 43m` / `unknown`. Reuses the asks
/// age humanizer (seconds -> largest whole unit) for the idle duration.
pub fn brief(rec: Option<&Value>, now: f64) -> String {
    let state = rec
        .and_then(|r| r.get("state").and_then(Value::as_str))
        .unwrap_or("unknown");
    match idle_secs(rec, now) {
        Some(s) => format!("{state} {}", asks::age(s as f64, 0.0)),
        None => state.to_string(),
    }
}

/// Whole minutes idle, for the MCP ask-timeout note: None unless idle.
pub fn idle_minutes() -> Option<i64> {
    idle_secs(read().as_ref(), sys::now_f64()).map(|s| s / 60)
}

// ---- verbs ---------------------------------------------------------------------

/// `presence set active|idle` — hypridle's listener is the caller (the
/// human never runs this by hand in normal operation). Read-modify-write
/// under the asks dir's write lock (the same `.lock`; both critical
/// sections are tiny) so a racing set can't lose the no-bump rule.
pub fn set(state: &str) -> i32 {
    let dir = asks::asks_dir();
    if let Err(e) = fs::create_dir_all(&dir) {
        eprintln!("bsctl presence set: {}: {e}", dir.display());
        return 1;
    }
    let lock = fs::File::create(dir.join(".lock"))
        .ok()
        .filter(|l| sys::flock_exclusive(l, false));
    let rec = updated(read().as_ref(), state, sys::now_f64());
    let out = match sys::atomic_write_json(&dir, "presence.json", &rec) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("bsctl presence set: {}: {e}", presence_file().display());
            1
        }
    };
    drop(lock);
    out
}

/// `presence get [--format]`.
pub fn get(json_out: bool) -> i32 {
    let rec = read();
    let now = sys::now_f64();
    if json_out {
        println!("{}", get_json(rec.as_ref(), now));
    } else {
        println!("{}", brief(rec.as_ref(), now));
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_state_keeps_since_transition_bumps_it() {
        let first = updated(None, "idle", 100.0);
        assert_eq!(first, json!({"state": "idle", "since": 100.0}));
        // a repeated on-timeout fire must accumulate, not reset
        let again = updated(Some(&first), "idle", 250.0);
        assert_eq!(again["since"], 100.0);
        // a real transition takes the new timestamp
        let active = updated(Some(&again), "active", 300.0);
        assert_eq!(active, json!({"state": "active", "since": 300.0}));
        // corrupt prev (no since) degrades to now, never panics
        let healed = updated(Some(&json!({"state": "idle"})), "idle", 400.0);
        assert_eq!(healed["since"], 400.0);
    }

    #[test]
    fn shapes_and_idle_arithmetic() {
        let idle = json!({"state": "idle", "since": 100.0});
        assert_eq!(
            human_json(Some(&idle), 160.0),
            json!({"state": "idle", "idle_secs": 60})
        );
        assert_eq!(
            get_json(Some(&idle), 160.0),
            json!({"state": "idle", "idle_secs": 60, "since": 100.0})
        );
        // active: idle_secs is null, not 0 — the duration is meaningless
        let active = json!({"state": "active", "since": 100.0});
        assert_eq!(
            human_json(Some(&active), 160.0),
            json!({"state": "active", "idle_secs": Value::Null})
        );
        // no report: unknown, everything else null
        assert_eq!(
            human_json(None, 1.0),
            json!({"state": "unknown", "idle_secs": Value::Null})
        );
        assert_eq!(get_json(None, 1.0)["since"], Value::Null);
        // clock skew clamps like the asks age does
        assert_eq!(human_json(Some(&idle), 50.0)["idle_secs"], 0);
    }

    #[test]
    fn brief_is_the_one_line_form() {
        assert_eq!(brief(None, 1.0), "unknown");
        let active = json!({"state": "active", "since": 100.0});
        assert_eq!(brief(Some(&active), 160.0), "active");
        let idle = json!({"state": "idle", "since": 100.0});
        assert_eq!(brief(Some(&idle), 145.0), "idle 45s");
        assert_eq!(brief(Some(&idle), 2800.0), "idle 45m");
    }
}
