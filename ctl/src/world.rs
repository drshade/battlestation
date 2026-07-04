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
use crate::{agents, ipc, sessions, sys, ws};

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

/// The human overview. Sections render only when they have something to
/// say; a null compositor renders as one honest line instead of an empty
/// world.
pub fn render_text(world: &Value) -> String {
    let mut out = String::new();
    match world.get("displays").and_then(Value::as_array) {
        None => out.push_str("compositor unavailable (no display/workspace state)\n"),
        Some(displays) => {
            let ws_rows = world
                .get("workspaces")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            for d in displays {
                let name = d.get("name").and_then(Value::as_str).unwrap_or("?");
                out.push_str(&format!(
                    "display {}  {}{}\n",
                    d.get("id").and_then(Value::as_i64).unwrap_or(0),
                    name,
                    if d.get("focused").and_then(Value::as_bool) == Some(true) {
                        "  (focused)"
                    } else {
                        ""
                    },
                ));
                for w in ws_rows
                    .iter()
                    .filter(|w| w.get("display").and_then(Value::as_str) == Some(name))
                {
                    let windows = w.get("windows").and_then(Value::as_i64).unwrap_or(0);
                    out.push_str(&format!(
                        "  bs {}  ws {}{}  {} window{}{}{}\n",
                        w.get("bs").and_then(Value::as_i64).unwrap_or(0),
                        w.get("ws").and_then(Value::as_i64).unwrap_or(0),
                        w.get("name")
                            .and_then(Value::as_str)
                            .map(|n| format!(" \"{n}\""))
                            .unwrap_or_default(),
                        windows,
                        if windows == 1 { "" } else { "s" },
                        if w.get("active").and_then(Value::as_bool) == Some(true) {
                            "  [active]"
                        } else {
                            ""
                        },
                        w.get("pref")
                            .and_then(Value::as_str)
                            .map(|p| format!("  [prefers {p}]"))
                            .unwrap_or_default(),
                    ));
                }
            }
        }
    }
    let agents = world
        .get("agents")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if !agents.is_empty() {
        out.push_str("agents\n");
        for a in &agents {
            let subs = a
                .get("subagents")
                .and_then(Value::as_array)
                .map_or(0, Vec::len);
            out.push_str(&format!(
                "  {}  {}  ws {}{}{}\n",
                a.get("kind").and_then(Value::as_str).unwrap_or("?"),
                a.get("status").and_then(Value::as_str).unwrap_or("?"),
                a.get("ws").and_then(Value::as_i64).unwrap_or(-1),
                a.get("title")
                    .and_then(Value::as_str)
                    .filter(|t| !t.is_empty())
                    .map(|t| format!("  \"{t}\""))
                    .unwrap_or_default(),
                match subs {
                    0 => String::new(),
                    1 => "  (1 subagent)".to_string(),
                    n => format!("  ({n} subagents)"),
                },
            ));
        }
    }
    let waiting: Vec<&Value> = world
        .get("prefs")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter(|p| p.get("present").and_then(Value::as_bool) == Some(false))
                .collect()
        })
        .unwrap_or_default();
    if !waiting.is_empty() {
        out.push_str("prefs waiting for absent displays\n");
        for p in waiting {
            out.push_str(&format!(
                "  ws {} -> {}\n",
                p.get("ws").and_then(Value::as_i64).unwrap_or(0),
                p.get("display").and_then(Value::as_str).unwrap_or("?"),
            ));
        }
    }
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
            "display 1  eDP-1\n\
             \x20 bs 2  ws 3  1 window  [active]  [prefers eDP-1]\n\
             display 2  DP-1  (focused)\n\
             \x20 bs 1  ws 1 \"corp\"  2 windows  [active]\n\
             agents\n\
             \x20 claude  thinking  ws 3  \"Fix the widget\"  (1 subagent)\n\
             prefs waiting for absent displays\n\
             \x20 ws 9 -> DP-2\n"
        );
        // degraded: one honest line, agents still shown
        let (recs, ..) = fixtures();
        let w = assemble(recs, None, None, "", &BTreeMap::new());
        let t = render_text(&w);
        assert!(t.starts_with("compositor unavailable"));
        assert!(t.contains("claude  thinking"));
    }
}
