//! `bsctl status` — the full state of the world in one object: the asks
//! queue, displays, the battlespace join, the preferences, the live agent
//! sessions, and the per-kind plan usage. The
//! text form is the at-a-glance human overview (the model's learning
//! surface); the json form is the machine feed, and with `--stream` it is
//! THE subscription the widget lives on (schema in lib.rs). Assembly is
//! pure ([`assemble`]) so the schema is pinned by tests; [`snapshot`] is
//! the thin IO wrapper.

use std::collections::BTreeMap;

use serde_json::{Value, json};

use crate::ws::{BsRow, bs_join, displays_from, human_name, live_ids, pref_rows, pref_rows_json};
use crate::{agents, asks, ipc, presence, proto, sessions, sys, usage, ws};

/// The world, queried live. Session scan first (it sweeps), compositor
/// snapshots second; the map, prefs and asks files are read lock-free
/// (their writers rename atomically). Usage rides the shared flocked
/// cache, so every streamer and poller combined pays at most one fetch per
/// TTL — and the stream's tick is what picks a TTL expiry up.
pub fn snapshot() -> Value {
    let recs = sessions::scan(sys::now_f64(), &sys::state_dir(), &sessions::projects_dir());
    let wsj = ipc::json("workspaces");
    let mons = ipc::json("monitors");
    assemble(
        recs,
        wsj.as_ref(),
        mons.as_ref(),
        &std::fs::read_to_string(ws::map_file()).unwrap_or_default(),
        &ws::load_prefs(),
        usage::snapshot(None),
        asks::rows_json(None),
        presence::human_json(presence::read().as_ref(), sys::now_f64()),
    )
}

/// Pure assembly. Either compositor snapshot missing means `displays` and
/// `workspaces` are `null` — not `[]`, which would read as a true empty
/// world — so a consumer keeps its last state across a compositor restart
/// (the same contract the old widget file had). The prefs section is file
/// truth and always present, but its reality annotations (`present`,
/// `live`) degrade to null without a compositor to ask.
#[allow(clippy::too_many_arguments)]
pub fn assemble(
    recs: Vec<Value>,
    wsj: Option<&Value>,
    mons: Option<&Value>,
    map_content: &str,
    prefs: &BTreeMap<i64, String>,
    usage: Value,
    asks: Vec<Value>,
    human: Value,
) -> Value {
    let comp = match (wsj, mons) {
        (Some(w), Some(m)) => Some((w, m)),
        _ => None,
    };
    let displays = comp.map_or(Value::Null, |(_, m)| Value::Array(displays_json(m)));
    let workspaces = comp.map_or(Value::Null, |(w, m)| {
        Value::Array(workspaces_json(&bs_join(map_content, w, m), prefs))
    });
    let prefs_v = match comp {
        Some((w, m)) => Value::Array(pref_rows_json(&pref_rows(
            prefs,
            &displays_from(m),
            &live_ids(w),
        ))),
        None => Value::Array(
            prefs
                .iter()
                .map(|(id, output)| {
                    json!({"ws": id, "display": output, "present": Value::Null, "live": Value::Null})
                })
                .collect(),
        ),
    };
    json!({
        // File truth like prefs: [] when the queue is empty, never null —
        // an empty queue is not a lost compositor.
        "asks": Value::Array(asks),
        "displays": displays,
        "workspaces": workspaces,
        "prefs": prefs_v,
        "agents": Value::Array(agents::agent_rows(recs, None, None)),
        // Cache truth like prefs is file truth: {} when nothing is known,
        // never null — an unknown usage is not a lost compositor.
        "usage": usage,
        // File truth again: state "unknown" before hypridle's first report,
        // never null (an unwritten file is not a lost compositor).
        "human": human,
    })
}

/// The displays section: the shared 1-based numbering (leftmost first) plus
/// the geometry/focus fields a renderer needs. x/y/activeWs come from the
/// raw monitor entry, looked up by name — [`displays_from`] owns the
/// ordering, the raw entry owns the fields it doesn't carry.
fn displays_json(mons: &Value) -> Vec<Value> {
    displays_from(mons)
        .iter()
        .enumerate()
        .map(|(i, d)| {
            let raw = mons
                .as_array()
                .and_then(|a| {
                    a.iter()
                        .find(|m| m.get("name").and_then(Value::as_str) == Some(d.name.as_str()))
                })
                .cloned()
                .unwrap_or(Value::Null);
            json!({
                "id": i + 1,
                "name": d.name,
                "x": raw.get("x").cloned().unwrap_or(Value::Null),
                "y": raw.get("y").cloned().unwrap_or(Value::Null),
                "focused": d.focused,
                "activeWs": d.active_ws,
                "specialShowing": !raw
                    .pointer("/specialWorkspace/name")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .is_empty(),
            })
        })
        .collect()
}

/// The workspaces section: the battlespace join plus each row's preference
/// (or null — most workspaces have none, by design).
fn workspaces_json(rows: &[BsRow], prefs: &BTreeMap<i64, String>) -> Vec<Value> {
    rows.iter()
        .map(|r| {
            json!({
                "ws": r.ws,
                "bs": r.bs,
                "name": human_name(&r.name, r.ws),
                "display": r.display,
                "windows": r.windows,
                "active": r.active,
                "pref": prefs.get(&r.ws).cloned(),
            })
        })
        .collect()
}

/// The human overview: one table per section (the house style), a blank
/// line between sections. Empty sections are omitted (no lonely headers);
/// a NULL compositor section renders its heading over an honest
/// `(compositor unreachable)` note instead of an empty table.
pub fn render_text(world: &Value) -> String {
    let mut sections: Vec<String> = Vec::new();
    let section = |heading: &str, table: &str| -> String {
        let body: Vec<String> = table.lines().map(|l| format!("  {l}")).collect();
        format!("{heading}\n{}", body.join("\n"))
    };
    let unreachable = |heading: &str| format!("{heading}\n  (compositor unreachable)");

    let num = |v: &Value, k: &str| {
        v.get(k)
            .and_then(Value::as_i64)
            .map(|n| n.to_string())
            .unwrap_or_default()
    };
    let flag = |v: &Value, k: &str| proto::yes(v.get(k).and_then(Value::as_bool) == Some(true));

    // The asks section leads — this is the ATTENTION overview, and who
    // needs you outranks what the desk looks like.
    if let Some(rows) = world.get("asks").and_then(Value::as_array)
        && !rows.is_empty()
    {
        sections.push(section(
            "asks",
            &proto::render_table(&asks::ASK_HEADERS, &asks::ask_cells(rows, sys::now_f64())),
        ));
    }

    match world.get("displays").and_then(Value::as_array) {
        None => sections.push(unreachable("displays")),
        Some(ds) if ds.is_empty() => {}
        Some(ds) => {
            let cells: Vec<Vec<String>> = ds
                .iter()
                .map(|d| {
                    vec![
                        num(d, "id"),
                        proto::field(d, "name"),
                        flag(d, "focused"),
                        num(d, "activeWs"),
                        flag(d, "specialShowing"),
                    ]
                })
                .collect();
            sections.push(section(
                "displays",
                &proto::render_table(&["ID", "NAME", "FOCUSED", "ACTIVE-WS", "SPECIAL"], &cells),
            ));
        }
    }

    match world.get("workspaces").and_then(Value::as_array) {
        None => sections.push(unreachable("workspaces")),
        Some(rows) if rows.is_empty() => {}
        Some(rows) => {
            // The map-get table plus a PREF column (empty when unset — most
            // workspaces have no preference, by design).
            let headers: Vec<&str> = ws::MAP_HEADERS.iter().copied().chain(["PREF"]).collect();
            let cells: Vec<Vec<String>> = rows
                .iter()
                .map(|w| {
                    vec![
                        num(w, "bs"),
                        num(w, "ws"),
                        proto::field(w, "name"),
                        proto::field(w, "display"),
                        num(w, "windows"),
                        flag(w, "active"),
                        proto::field(w, "pref"),
                    ]
                })
                .collect();
            sections.push(section(
                "workspaces",
                &proto::render_table(&headers, &cells),
            ));
        }
    }

    if let Some(agents) = world.get("agents").and_then(Value::as_array)
        && !agents.is_empty()
    {
        sections.push(section(
            "agents",
            &proto::render_table(&agents::AGENT_HEADERS, &agents::agent_cells(agents)),
        ));
    }

    if let Some(u) = world.get("usage")
        && u.as_object().is_some_and(|m| !m.is_empty())
    {
        sections.push(section(
            "usage",
            &proto::render_table(&usage::USAGE_HEADERS, &usage::cells(u)),
        ));
    }

    let waiting: Vec<Vec<String>> = world
        .get("prefs")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter(|p| p.get("present").and_then(Value::as_bool) == Some(false))
                .map(|p| vec![num(p, "ws"), proto::field(p, "display")])
                .collect()
        })
        .unwrap_or_default();
    if !waiting.is_empty() {
        sections.push(section(
            "prefs waiting for absent displays",
            &proto::render_table(&["WS", "DISPLAY"], &waiting),
        ));
    }

    // One line, last — the human knows whether they're at the desk; this
    // section exists to show what the AGENTS are being told. Omitted while
    // unknown (before hypridle's first report there is nothing to show).
    if let Some(h) = world.get("human")
        && h.get("state")
            .and_then(Value::as_str)
            .is_some_and(|s| s != "unknown")
    {
        let state = proto::field(h, "state");
        // Duration only while idle: active is always 0 by contract, and
        // "active 0s" would read as a countdown.
        let line = match h.get("idle_secs").and_then(Value::as_i64) {
            Some(s) if state == "idle" => format!("{state} {}", asks::age(s as f64, 0.0)),
            _ => state,
        };
        sections.push(format!("human\n  {line}"));
    }

    let mut out = sections.join("\n\n");
    out.push('\n');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_usage() -> Value {
        json!({"claude": {"sessionPct": 34, "sessionResets": "2026-07-03T10:00:00Z",
                          "weeklyPct": 62, "weeklyResets": "2026-07-07T00:00:00Z"}})
    }

    fn fixtures() -> (Vec<Value>, Value, Value, BTreeMap<i64, String>) {
        let recs = vec![json!({
            "sid": "s-1", "ws": 3, "status": "thinking", "kind": "claude",
            "title": "Fix the widget", "agents": [{"id": "a1", "type": "fork",
            "description": "d", "started": 1}],
        })];
        let wsj = json!([
            {"id": 1, "name": "corp", "monitor": "DP-1", "windows": 2},
            {"id": 3, "name": "3", "monitor": "eDP-1", "windows": 1},
        ]);
        let mons = json!([
            {"name": "DP-1",  "x": 2304, "y": 0, "focused": true,
             "activeWorkspace": {"id": 1}, "specialWorkspace": {"name": ""}},
            {"name": "eDP-1", "x": 0,    "y": 0, "focused": false,
             "activeWorkspace": {"id": 3}, "specialWorkspace": {"name": ""}},
        ]);
        let mut prefs = BTreeMap::new();
        prefs.insert(3, "eDP-1".to_string());
        prefs.insert(9, "DP-2".to_string()); // dead ws, absent display
        (recs, wsj, mons, prefs)
    }

    /// An ask fixture `created` far in the FUTURE: the age clamp renders
    /// `0s` regardless of when the test runs, keeping text pins
    /// deterministic.
    fn fixture_asks() -> Vec<Value> {
        vec![json!({
            "id": 1, "session": "s-1", "kind": "claude", "ws": 3,
            "type": "question", "title": "Ship it?", "body": "",
            "options": [], "urgency": "high", "estimate_min": 2,
            "note": "", "state": "open", "answer": Value::Null,
            "created": 4e12, "answered_at": Value::Null,
        })]
    }

    fn fixture_human_unknown() -> Value {
        json!({"state": "unknown", "idle_secs": Value::Null})
    }

    #[test]
    fn assemble_joins_all_sections() {
        let (recs, wsj, mons, prefs) = fixtures();
        let w = assemble(
            recs,
            Some(&wsj),
            Some(&mons),
            "",
            &prefs,
            fixture_usage(),
            fixture_asks(),
            json!({"state": "idle", "idle_secs": 300}),
        );
        // human rides the schema verbatim (file truth, shaped by presence)
        assert_eq!(w["human"]["state"], "idle");
        assert_eq!(w["human"]["idle_secs"], 300);
        // asks lead the schema: file truth, [] when empty, never null
        assert_eq!(w["asks"][0]["id"], 1);
        assert_eq!(w["asks"][0]["urgency"], "high");
        // displays numbered leftmost-first: eDP-1 at x=0 is display 1
        assert_eq!(w["displays"][0]["name"], "eDP-1");
        assert_eq!(w["displays"][0]["id"], 1);
        assert_eq!(w["displays"][1]["name"], "DP-1");
        assert_eq!(w["displays"][1]["focused"], true);
        assert_eq!(w["displays"][1]["activeWs"], 1);
        // workspaces in battlespace order (identity map), with pref joined
        assert_eq!(w["workspaces"][0]["ws"], 1);
        assert_eq!(w["workspaces"][0]["bs"], 1);
        assert_eq!(w["workspaces"][0]["name"], "corp");
        assert_eq!(w["workspaces"][0]["pref"], Value::Null);
        assert_eq!(w["workspaces"][1]["ws"], 3);
        assert_eq!(w["workspaces"][1]["name"], Value::Null); // unnamed default
        assert_eq!(w["workspaces"][1]["active"], true);
        assert_eq!(w["workspaces"][1]["pref"], "eDP-1");
        // prefs annotated: ws 3 present+live, ws 9 absent+dead
        assert_eq!(w["prefs"][0]["ws"], 3);
        assert_eq!(w["prefs"][0]["present"], true);
        assert_eq!(w["prefs"][0]["live"], true);
        assert_eq!(w["prefs"][1]["ws"], 9);
        assert_eq!(w["prefs"][1]["present"], false);
        assert_eq!(w["prefs"][1]["live"], false);
        // agents ride the published schema
        assert_eq!(w["agents"][0]["session"], "s-1");
        assert_eq!(w["agents"][0]["subagents"][0]["id"], "a1");
        // usage is passed through kind-indexed; {} would mean nothing known
        assert_eq!(w["usage"]["claude"]["sessionPct"], 34);
    }

    #[test]
    fn assemble_nulls_compositor_sections_when_a_query_fails() {
        let (recs, wsj, _, prefs) = fixtures();
        let w = assemble(
            recs,
            Some(&wsj),
            None,
            "",
            &prefs,
            json!({}),
            vec![],
            fixture_human_unknown(),
        );
        // human is file truth: "unknown" is honest absence, never null
        assert_eq!(w["human"]["state"], "unknown");
        assert_eq!(
            w["asks"],
            json!([]),
            "file truth: [] when empty, never null"
        );
        assert_eq!(w["displays"], Value::Null, "not [] — that would be a lie");
        assert_eq!(w["workspaces"], Value::Null);
        // prefs stay (file truth) with unknowable annotations nulled
        assert_eq!(w["prefs"][0]["ws"], 3);
        assert_eq!(w["prefs"][0]["present"], Value::Null);
        assert_eq!(w["prefs"][0]["live"], Value::Null);
        // agents are file+proc truth, never nulled
        assert_eq!(w["agents"][0]["session"], "s-1");
        // usage is cache truth: {} (nothing known), never null
        assert_eq!(w["usage"], json!({}));
    }

    #[test]
    fn agents_section_equals_agents_get_rows() {
        // The status feed and `agents get` must never drift: same input,
        // same rows.
        let (recs, wsj, mons, prefs) = fixtures();
        let w = assemble(
            recs.clone(),
            Some(&wsj),
            Some(&mons),
            "",
            &prefs,
            json!({}),
            vec![],
            fixture_human_unknown(),
        );
        assert_eq!(
            w["agents"],
            Value::Array(agents::agent_rows(recs, None, None))
        );
    }

    #[test]
    fn text_render_reads_like_the_model() {
        let (recs, wsj, mons, prefs) = fixtures();
        let w = assemble(
            recs,
            Some(&wsj),
            Some(&mons),
            "",
            &prefs,
            fixture_usage(),
            fixture_asks(),
            json!({"state": "idle", "idle_secs": 300}),
        );
        assert_eq!(
            render_text(&w),
            "asks\n\
             \x20 ID  AGE  TYPE      URG   EST  WS  KIND    TITLE     NOTE  STATE  BLOCKING\n\
             \x20 1   0s   question  high  2m   3   claude  Ship it?        open\n\
             \n\
             displays\n\
             \x20 ID  NAME   FOCUSED  ACTIVE-WS  SPECIAL\n\
             \x20 1   eDP-1           3\n\
             \x20 2   DP-1   yes      1\n\
             \n\
             workspaces\n\
             \x20 BS  WS  NAME  DISPLAY  WINDOWS  ACTIVE  PREF\n\
             \x20 1   1   corp  DP-1     2        yes\n\
             \x20 2   3         eDP-1    1        yes     eDP-1\n\
             \n\
             agents\n\
             \x20 KIND    STATUS    WS  SUBAGENTS  TITLE           SESSION\n\
             \x20 claude  thinking  3   1          Fix the widget  s-1\n\
             \n\
             usage\n\
             \x20 KIND    SESSION%  RESETS                WEEK%  RESETS\n\
             \x20 claude  34        2026-07-03T10:00:00Z  62     2026-07-07T00:00:00Z\n\
             \n\
             prefs waiting for absent displays\n\
             \x20 WS  DISPLAY\n\
             \x20 9   DP-2\n\
             \n\
             human\n\
             \x20 idle 5m\n"
        );
        // degraded: honest per-section notes, agents still tabled, and an
        // unknown presence renders NO human section (nothing to show)
        let (recs, ..) = fixtures();
        let w = assemble(
            recs,
            None,
            None,
            "",
            &BTreeMap::new(),
            json!({}),
            vec![],
            fixture_human_unknown(),
        );
        let t = render_text(&w);
        assert!(!t.contains("human"));
        assert!(t.starts_with(
            "displays\n\
             \x20 (compositor unreachable)\n\
             \n\
             workspaces\n\
             \x20 (compositor unreachable)"
        ));
        assert!(t.contains("KIND"));
        assert!(t.contains("claude  thinking"));
    }
}
