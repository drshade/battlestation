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
/// require an explicit session key and no-op without one (see hook.rs), so a
/// "default" session file can never be fabricated.
pub fn session_id(d: &Value) -> String {
    field_or(d, "session_id", "default")
}

/// The session-status verbs' session key: `session_id` (Claude Code, Codex),
/// falling back to `conversationId` — Antigravity (`agy`) hook payloads are
/// protojson camelCase and identify the session by conversationId only.
/// "" when neither is present (hook.rs no-ops rather than fabricating).
pub fn session_key(d: &Value) -> String {
    let sid = field(d, "session_id");
    if sid.is_empty() {
        field(d, "conversationId")
    } else {
        sid
    }
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

/// Char budget for payload-derived titles (transcript aiTitles arrive
/// pre-sized; prompts don't).
pub const PROMPT_TITLE_MAX: usize = 60;

/// Payload-derived title for harnesses whose transcripts carry no ai-title
/// (Codex): `basename(cwd): <first non-blank line of prompt>`,
/// whitespace-collapsed and truncated to [`PROMPT_TITLE_MAX`] chars (a cut
/// gets a trailing `…`). Missing pieces degrade: no usable prompt line -> ""
/// (so the precedence chain falls through); no cwd basename -> the bare
/// prompt line.
pub fn title_from_prompt(cwd: &str, prompt: &str) -> String {
    let Some(line) = prompt.lines().map(collapse_ws).find(|l| !l.is_empty()) else {
        return String::new();
    };
    let base = std::path::Path::new(cwd)
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let full = if base.is_empty() {
        line
    } else {
        format!("{base}: {line}")
    };
    truncate_chars(&full, PROMPT_TITLE_MAX)
}

/// Cap `s` at `max` chars, replacing the tail with `…` when cut (the
/// ellipsis occupies the last char of the budget).
fn truncate_chars(s: &str, max: usize) -> String {
    match s.char_indices().nth(max) {
        None => s.to_string(),
        Some(_) => {
            let mut t: String = s.chars().take(max - 1).collect();
            t.push('…');
            t
        }
    }
}

/// Session-title precedence, kind-agnostic: (1) transcript ai-title scan
/// (empty for harnesses without one) -> (2) payload-derived prompt title ->
/// (3) STICKY: the existing session file's title, so a title set once
/// persists across events that carry nothing -> (4) "".
pub fn derive_title(transcript: &str, prompt: &str, sticky: &str) -> String {
    [transcript, prompt, sticky]
        .iter()
        .find(|t| !t.is_empty())
        .map_or_else(String::new, |t| t.to_string())
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
/// `{"ws": <int>, "win": <addr>|null, "status": ..., "kind": ..., "title":
/// ..., "pid": <int>, "term_pid": <int>|null}`. `kind` is the harness
/// discriminator (`bsctl agents set --kind`, mandatory). `pid` is the
/// HARNESS process (the comm-matched /proc ancestor — the claude/codex/agy
/// process). `win` is the terminal window's Hyprland address and `term_pid`
/// is the TERMINAL process owning that window (the clients-row pid — kitty
/// here, the harness's ancestor); both are best-effort (null when the
/// clients row lacked them) and refresh like `ws` on every hook event so a
/// moved terminal heals — see the protocol contract in lib.rs.
pub fn session_record(
    ws: i64,
    win: Option<&str>,
    status: &str,
    title: &str,
    pid: i64,
    term_pid: Option<i64>,
    kind: &str,
) -> Value {
    json!({"ws": ws, "win": win, "status": status, "kind": kind, "title": title, "pid": pid, "term_pid": term_pid})
}

/// The house table for every human text view: UPPERCASE header row, columns
/// left-aligned to max(header, cells) by CHAR count (EDID strings and
/// workspace names are not ASCII-only), two-space gutters, no trailing
/// whitespace (the last cell pads nothing; shorter cells are padded then the
/// line is trimmed, so an empty last cell can't leave a gutter behind). No
/// rows renders just the header — callers own the no-lonely-header rule
/// (empty collections print nothing at all).
pub fn render_table(headers: &[&str], rows: &[Vec<String>]) -> String {
    let mut widths: Vec<usize> = headers.iter().map(|h| h.chars().count()).collect();
    for r in rows {
        for (w, cell) in widths.iter_mut().zip(r.iter()) {
            *w = (*w).max(cell.chars().count());
        }
    }
    let render_row = |cells: &[&str]| -> String {
        let mut line = String::new();
        for (i, c) in cells.iter().enumerate() {
            if i > 0 {
                line.push_str("  ");
            }
            line.push_str(c);
            if i + 1 < widths.len() {
                line.extend(std::iter::repeat_n(' ', widths[i] - c.chars().count()));
            }
        }
        line.trim_end().to_string()
    };
    let mut out = render_row(headers);
    for r in rows {
        let cells: Vec<&str> = r.iter().map(String::as_str).collect();
        out.push('\n');
        out.push_str(&render_row(&cells));
    }
    out
}

/// "yes" / "" — the table dialect for flags that only matter when set
/// (ACTIVE, FOCUSED, SPECIAL); binary facts use explicit yes/no instead.
pub fn yes(b: bool) -> String {
    if b { "yes".to_string() } else { String::new() }
}

/// "yes" / "no" — for facts where both states are informative (PRESENT,
/// LIVE).
pub fn yes_no(b: bool) -> String {
    (if b { "yes" } else { "no" }).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_aligns_and_never_trails_whitespace() {
        let rows = vec![
            vec!["1".to_string(), "eDP-1".to_string(), String::new()],
            vec!["2".to_string(), "x".to_string(), "yes".to_string()],
        ];
        // header wider than cells (COL), cell wider than header (eDP-1 vs B)
        assert_eq!(
            render_table(&["COL", "B", "FOCUSED"], &rows),
            "COL  B      FOCUSED\n\
             1    eDP-1\n\
             2    x      yes"
        );
        for line in render_table(&["COL", "B", "FOCUSED"], &rows).lines() {
            assert_eq!(line, line.trim_end(), "no trailing whitespace");
        }
        // char-count widths, not byte widths (… is 3 bytes, 1 char)
        let rows = vec![vec!["a…b".to_string(), "y".to_string()]];
        assert_eq!(render_table(&["A", "B"], &rows), "A    B\na…b  y");
        // no rows: just the header (callers own the no-lonely-header rule)
        assert_eq!(render_table(&["A", "B"], &[]), "A  B");
    }

    #[test]
    fn yes_dialects() {
        assert_eq!(yes(true), "yes");
        assert_eq!(yes(false), "");
        assert_eq!(yes_no(true), "yes");
        assert_eq!(yes_no(false), "no");
    }

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
    fn session_key_falls_back_to_conversation_id() {
        // Claude Code / Codex payloads: session_id wins.
        let d: Value =
            serde_json::from_str(r#"{"session_id":"s1","conversationId":"c1"}"#).unwrap();
        assert_eq!(session_key(&d), "s1");
        // agy payloads are protojson camelCase: conversationId only.
        let agy: Value = serde_json::from_str(
            r#"{"conversationId":"ec33ebf9-0cba-4100-8142-c61503f6c587","workspacePaths":["/w"],"modelName":"auto"}"#,
        )
        .unwrap();
        assert_eq!(session_key(&agy), "ec33ebf9-0cba-4100-8142-c61503f6c587");
        // neither -> "" -> hook.rs no-ops (never fabricates a session).
        assert_eq!(session_key(&parse_payload(b"{}")), "");
        assert_eq!(session_key(&parse_payload(b"junk")), "");
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
    fn prompt_title_shape_and_truncation() {
        assert_eq!(
            title_from_prompt("/home/u/dev/battlestation", "fix the widget"),
            "battlestation: fix the widget"
        );
        // multiline prompt: only the first non-blank line, collapsed
        assert_eq!(
            title_from_prompt("/x/proj", "\n\n  first \t line \nsecond line\n"),
            "proj: first line"
        );
        // no usable cwd basename -> the bare prompt line
        assert_eq!(title_from_prompt("", "just a prompt"), "just a prompt");
        assert_eq!(title_from_prompt("/", "root cwd"), "root cwd");
        // no usable prompt -> "" so the precedence chain falls through
        assert_eq!(title_from_prompt("/x/proj", ""), "");
        assert_eq!(title_from_prompt("/x/proj", " \n\t\n"), "");
        // truncation to PROMPT_TITLE_MAX chars, ellipsis in the last slot
        let long = title_from_prompt("/x/proj", &"y".repeat(200));
        assert_eq!(long.chars().count(), PROMPT_TITLE_MAX);
        assert!(long.starts_with("proj: yyy") && long.ends_with('…'));
        // exactly at the budget: untouched, no ellipsis
        let exact = "p".repeat(PROMPT_TITLE_MAX);
        assert_eq!(title_from_prompt("", &exact), exact);
        // multibyte chars: counted as chars, not bytes (no boundary panic)
        let uni = title_from_prompt("", &"héllo wörld ".repeat(10));
        assert_eq!(uni.chars().count(), PROMPT_TITLE_MAX);
    }

    #[test]
    fn title_precedence_chain() {
        assert_eq!(derive_title("ai", "prompt", "old"), "ai");
        assert_eq!(derive_title("", "prompt", "old"), "prompt");
        assert_eq!(derive_title("", "", "old"), "old");
        assert_eq!(derive_title("", "", ""), "");
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
