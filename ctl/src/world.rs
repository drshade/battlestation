//! `bsctl status` — the full state of the world in one object: displays,
//! the battlespace join, the preferences, and the live agent sessions. The
//! text form is the at-a-glance human overview (the model's learning
//! surface); the json form is the machine feed, and with `--stream` it is
//! THE subscription the widget lives on (schema in lib.rs). Assembly is
//! pure ([`assemble`]) so the schema is pinned by tests; [`snapshot`] is
//! the thin IO wrapper.

use std::collections::BTreeMap;

use serde_json::{Value, json};

use crate::ws::{BsRow, bs_join, displays_from, human_name, live_ids, pref_rows, pref_rows_json};
use crate::{agents, ipc, proto, sessions, sys, ws};

/// The world, queried live. Session scan first (it sweeps), compositor
/// snapshots second; the map and prefs files are read lock-free (their
/// writers rename atomically).
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
    )
}

/// Pure assembly. Either compositor snapshot missing means `displays` and
/// `workspaces` are `null` — not `[]`, which would read as a true empty
/// world — so a consumer keeps its last state across a compositor restart
/// (the same contract the old widget file had). The prefs section is file
/// truth and always present, but its reality annotations (`present`,
/// `live`) degrade to null without a compositor to ask.
pub fn assemble(
    recs: Vec<Value>,
    wsj: Option<&Value>,
    mons: Option<&Value>,
    map_content: &str,
    prefs: &BTreeMap<i64, String>,
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
        "displays": displays,
        "workspaces": workspaces,
        "prefs": prefs_v,
        "agents": Value::Array(agents::agent_rows(recs, None, None)),
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

    let mut out = sections.join("\n\n");
    out.push('\n');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn assemble_joins_all_sections() {
        let (recs, wsj, mons, prefs) = fixtures();
        let w = assemble(recs, Some(&wsj), Some(&mons), "", &prefs);
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
    }

    #[test]
    fn assemble_nulls_compositor_sections_when_a_query_fails() {
        let (recs, wsj, _, prefs) = fixtures();
        let w = assemble(recs, Some(&wsj), None, "", &prefs);
        assert_eq!(w["displays"], Value::Null, "not [] — that would be a lie");
        assert_eq!(w["workspaces"], Value::Null);
        // prefs stay (file truth) with unknowable annotations nulled
        assert_eq!(w["prefs"][0]["ws"], 3);
        assert_eq!(w["prefs"][0]["present"], Value::Null);
        assert_eq!(w["prefs"][0]["live"], Value::Null);
        // agents are file+proc truth, never nulled
        assert_eq!(w["agents"][0]["session"], "s-1");
    }

    #[test]
    fn agents_section_equals_agents_get_rows() {
        // The status feed and `agents get` must never drift: same input,
        // same rows.
        let (recs, wsj, mons, prefs) = fixtures();
        let w = assemble(recs.clone(), Some(&wsj), Some(&mons), "", &prefs);
        assert_eq!(
            w["agents"],
            Value::Array(agents::agent_rows(recs, None, None))
        );
    }

    #[test]
    fn text_render_reads_like_the_model() {
        let (recs, wsj, mons, prefs) = fixtures();
        let w = assemble(recs, Some(&wsj), Some(&mons), "", &prefs);
        assert_eq!(
            render_text(&w),
            "displays\n\
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
             prefs waiting for absent displays\n\
             \x20 WS  DISPLAY\n\
             \x20 9   DP-2\n"
        );
        // degraded: honest per-section notes, agents still tabled
        let (recs, ..) = fixtures();
        let w = assemble(recs, None, None, "", &BTreeMap::new());
        let t = render_text(&w);
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
