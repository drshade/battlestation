//! Pure protocol logic — deterministic, no I/O, unit-tested.
//!
//! Every function mirrors a specific construct in the script references
//! (claude-ws-status.sh / the BarWidget.qml pollScript); comments name the
//! python idiom being replicated so parity stays auditable.

use serde_json::{Value, json};

/// Poll-side GC window for stale subagent markers, seconds (`GC_S = 30 * 60`).
pub const GC_SECS: f64 = 30.0 * 60.0;

/// Parse a hook payload: any failure yields an empty object, mirroring the
/// scripts' `try: d = json.load(...) except Exception: d = {}`.
pub fn parse_payload(input: &[u8]) -> Value {
    serde_json::from_slice(input).unwrap_or_else(|_| Value::Object(serde_json::Map::new()))
}

/// Mirror of python `str(v or "")`: falsy (missing, null, false, 0, "", empty
/// array/object) -> "", `true` -> "True", numbers -> decimal, strings pass
/// through. Non-empty arrays/objects never appear in these fields; their JSON
/// text is a tolerant stand-in for python's `str()`.
pub fn py_str(v: Option<&Value>) -> String {
    match v {
        None | Some(Value::Null) | Some(Value::Bool(false)) => String::new(),
        Some(Value::Bool(true)) => "True".to_string(),
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => {
            if n.as_f64().is_some_and(|f| f == 0.0) {
                String::new()
            } else {
                n.to_string()
            }
        }
        Some(v @ Value::Array(a)) => {
            if a.is_empty() {
                String::new()
            } else {
                v.to_string()
            }
        }
        Some(v @ Value::Object(o)) => {
            if o.is_empty() {
                String::new()
            } else {
                v.to_string()
            }
        }
    }
}

/// `str(d.get(key) or "")`.
pub fn field(d: &Value, key: &str) -> String {
    py_str(d.get(key))
}

/// `str(d.get(key) or default)` — e.g. the poll side's kind -> "claude".
pub fn field_or(d: &Value, key: &str, default: &str) -> String {
    let s = field(d, key);
    if s.is_empty() { default.to_string() } else { s }
}

/// session_id with the scripts' fallback: falsy/missing -> "default".
/// MARKER names only (agent-start/agent-stop) — the session-status verbs
/// require an explicit session_id and no-op without one (see hook.rs), so a
/// "default" session file can never be fabricated.
pub fn session_id(d: &Value) -> String {
    field_or(d, "session_id", "default")
}

/// python-3 `round()`: exact halves go to the even integer (Rust's
/// f64::round goes away from zero instead). Used wherever a reference
/// script's python rounds — usage percentages, display-scale refresh rates.
pub fn round_half_even(f: f64) -> i64 {
    let floor = f.floor();
    if f - floor == 0.5 {
        let below = floor as i64;
        if below % 2 == 0 { below } else { below + 1 }
    } else {
        f.round() as i64
    }
}

/// Mirror of python `int(v)` for the poll side's pid check: int, bool,
/// float (truncated), or integer-string succeed; anything else is None
/// (python raises -> the except path sweeps the session).
pub fn py_int(v: &Value) -> Option<i64> {
    match v {
        Value::Number(n) => n.as_i64().or_else(|| n.as_f64().map(|f| f.trunc() as i64)),
        Value::Bool(b) => Some(*b as i64),
        Value::String(s) => s.trim().parse::<i64>().ok(),
        _ => None,
    }
}

/// Marker filenames are `<sid>.<aid>`. Session ids are UUIDs and agent ids
/// hex — neither contains a dot — so splitting on the FIRST dot is
/// unambiguous (`n.split(".", 1)`). No dot -> a session file name.
pub fn split_marker_name(name: &str) -> Option<(&str, &str)> {
    name.split_once('.')
}

/// `" ".join(s.split())` — collapse whitespace runs to single spaces and
/// strip the ends, so titles stay one line.
pub fn collapse_ws(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Session title: the LAST transcript line containing `"type":"ai-title"`
/// (grep + tail -n1 in the sh reference), parsed as JSON, its aiTitle
/// whitespace-collapsed. Any failure — no match, bad JSON, non-string
/// aiTitle (python's `.split()` would raise) — yields "".
pub fn title_from_transcript(content: &str) -> String {
    let Some(line) = content
        .lines()
        .rev()
        .find(|l| l.contains(r#""type":"ai-title""#))
    else {
        return String::new();
    };
    let Ok(v) = serde_json::from_str::<Value>(line) else {
        return String::new();
    };
    match v.get("aiTitle") {
        Some(Value::String(s)) => collapse_ws(s),
        _ => String::new(),
    }
}

/// Poll-side GC backstop decision for ONE marker (see the pollScript): sweep
/// only when the marker is stale (untouched > GC_SECS) AND its subagent
/// transcript is missing or equally frozen. A live agent refreshes its marker
/// mtime on every tool call, so fresh markers are never candidates; one GC'd
/// during a long permission wait is recreated by the next call (self-healing).
pub fn gc_should_delete(now: f64, marker_mtime: f64, transcript_mtime: Option<f64>) -> bool {
    if now - marker_mtime <= GC_SECS {
        return false;
    }
    match transcript_mtime {
        None => true,
        Some(t) => now - t > GC_SECS,
    }
}

/// Subagent meta.json path relative to the transcript:
/// `dirname(transcript_path)/<sid>/subagents/agent-<aid>.meta.json`.
pub fn meta_json_path(transcript_path: &str, sid: &str, aid: &str) -> std::path::PathBuf {
    let dir = std::path::Path::new(transcript_path)
        .parent()
        // os.path.dirname("/") == "/" (parent of bare "/" is None in Rust)
        .unwrap_or_else(|| std::path::Path::new("/"));
    dir.join(sid)
        .join("subagents")
        .join(format!("agent-{aid}.meta.json"))
}

/// Fill EMPTY desc/atype from a parsed meta.json:
/// `desc = desc or str(m.get("description") or "")`,
/// `atype = atype or str(m.get("agentType") or "")`.
pub fn apply_meta_fallback(meta: &Value, desc: &mut String, atype: &mut String) {
    if desc.is_empty() {
        *desc = field(meta, "description");
    }
    if atype.is_empty() {
        *atype = field(meta, "agentType");
    }
}

/// Marker file content: `{"type": ..., "description": ...}`.
pub fn marker_record(atype: &str, desc: &str) -> Value {
    json!({"type": atype, "description": desc})
}

/// Session file content:
/// `{"ws": <int>, "status": ..., "kind": "claude", "title": ..., "pid": <int>}`.
pub fn session_record(ws: i64, status: &str, title: &str, pid: i64) -> Value {
    json!({"ws": ws, "status": status, "kind": "claude", "title": title, "pid": pid})
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marker_name_splits_on_first_dot() {
        assert_eq!(split_marker_name("sid.aid"), Some(("sid", "aid")));
        // agent ids never contain dots, but FIRST-dot is the contract
        assert_eq!(split_marker_name("sid.a.b"), Some(("sid", "a.b")));
        assert_eq!(split_marker_name("plain-session-id"), None);
        assert_eq!(
            split_marker_name("0198f2-uuid.9d0aa1"),
            Some(("0198f2-uuid", "9d0aa1"))
        );
    }

    #[test]
    fn gc_decision() {
        let now = 100_000.0;
        let fresh = now - 60.0;
        let stale = now - GC_SECS - 1.0;
        let boundary = now - GC_SECS; // exactly GC_SECS old: python `>` keeps it
        // fresh marker: never swept, transcript irrelevant
        assert!(!gc_should_delete(now, fresh, None));
        assert!(!gc_should_delete(now, fresh, Some(stale)));
        assert!(!gc_should_delete(now, boundary, None));
        // stale marker: swept unless the transcript is alive
        assert!(gc_should_delete(now, stale, None));
        assert!(gc_should_delete(now, stale, Some(stale)));
        assert!(!gc_should_delete(now, stale, Some(fresh)));
        assert!(!gc_should_delete(now, stale, Some(boundary)));
    }

    #[test]
    fn rounding_is_pythons_half_to_even() {
        assert_eq!(round_half_even(34.2), 34);
        assert_eq!(round_half_even(34.6), 35);
        assert_eq!(round_half_even(62.5), 62); // python round(62.5) == 62
        assert_eq!(round_half_even(63.5), 64);
        assert_eq!(round_half_even(-2.5), -2);
        assert_eq!(round_half_even(-1.5), -2);
        assert_eq!(round_half_even(0.0), 0);
        assert_eq!(round_half_even(100.0), 100);
        assert_eq!(round_half_even(120.001), 120); // a live refreshRate
    }

    #[test]
    fn payload_field_extraction() {
        let d: Value =
            serde_json::from_str(r#"{"session_id":"s1","agent_id":"a1","n":5,"z":0,"t":true,"f":false,"nul":null,"e":""}"#)
                .unwrap();
        assert_eq!(field(&d, "session_id"), "s1");
        assert_eq!(field(&d, "missing"), "");
        assert_eq!(field(&d, "nul"), "");
        assert_eq!(field(&d, "e"), "");
        assert_eq!(field(&d, "n"), "5"); // str(5)
        assert_eq!(field(&d, "z"), ""); // 0 is falsy
        assert_eq!(field(&d, "t"), "True"); // str(True)
        assert_eq!(field(&d, "f"), ""); // False is falsy
        assert_eq!(session_id(&d), "s1");
        let empty = parse_payload(b"not json at all");
        assert_eq!(session_id(&empty), "default");
        assert_eq!(field(&empty, "transcript_path"), "");
        assert_eq!(field_or(&empty, "kind", "claude"), "claude");
    }

    #[test]
    fn py_int_mirrors_python_int() {
        assert_eq!(py_int(&json!(42)), Some(42));
        assert_eq!(py_int(&json!(42.9)), Some(42)); // int() truncates
        assert_eq!(py_int(&json!("42")), Some(42));
        assert_eq!(py_int(&json!(" 42 ")), Some(42)); // int() strips whitespace
        assert_eq!(py_int(&json!(true)), Some(1));
        assert_eq!(py_int(&json!("4.2")), None); // int("4.2") raises
        assert_eq!(py_int(&json!(null)), None);
        assert_eq!(py_int(&json!([1])), None);
    }

    #[test]
    fn title_whitespace_collapsing() {
        assert_eq!(collapse_ws("  a \t\n b   c  "), "a b c");
        assert_eq!(collapse_ws(""), "");
        assert_eq!(collapse_ws(" \t "), "");
    }

    #[test]
    fn title_from_transcript_takes_last_match() {
        let t = concat!(
            r#"{"type":"other","aiTitle":"nope"}"#,
            "\n",
            r#"{"type":"ai-title","aiTitle":"First title"}"#,
            "\n",
            r#"{"foo":1}"#,
            "\n",
            r#"{"type":"ai-title","aiTitle":"  Second\ttitle  here "}"#,
            "\n"
        );
        assert_eq!(title_from_transcript(t), "Second title here");
        assert_eq!(title_from_transcript(""), "");
        assert_eq!(title_from_transcript(r#"{"type":"x"}"#), "");
        // matching line but broken JSON -> ""
        assert_eq!(title_from_transcript(r#"{"type":"ai-title", broken"#), "");
        // matching line without aiTitle -> ""
        assert_eq!(title_from_transcript(r#"{"type":"ai-title"}"#), "");
        // non-string aiTitle: python's .split() would raise -> ""
        assert_eq!(
            title_from_transcript(r#"{"type":"ai-title","aiTitle":7}"#),
            ""
        );
    }

    #[test]
    fn meta_path_layout() {
        assert_eq!(
            meta_json_path("/home/u/.claude/projects/p/tr.jsonl", "sid", "aid"),
            std::path::Path::new("/home/u/.claude/projects/p/sid/subagents/agent-aid.meta.json")
        );
    }

    #[test]
    fn meta_fallback_fills_only_empty() {
        let m: Value =
            serde_json::from_str(r#"{"description":"from meta","agentType":"explore"}"#).unwrap();
        let (mut d, mut a) = (String::new(), String::new());
        apply_meta_fallback(&m, &mut d, &mut a);
        assert_eq!((d.as_str(), a.as_str()), ("from meta", "explore"));
        let (mut d, mut a) = ("payload desc".to_string(), "payload type".to_string());
        apply_meta_fallback(&m, &mut d, &mut a);
        assert_eq!((d.as_str(), a.as_str()), ("payload desc", "payload type"));
    }
}
