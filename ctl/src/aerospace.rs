//! AeroSpace backend (macOS) — the window-manager boundary when bsctl runs
//! under AeroSpace instead of Hyprland. [`crate::ipc`] routes here; this
//! module answers the same queries in the SAME Hyprland-shaped JSON, so the
//! battlespace join, the world, comms labels and the asks watch-check all
//! work unchanged, and translates [`Action`]s into `aerospace` commands.
//!
//! Model mapping (contract in lib.rs, "Window managers"):
//! - ws-id = an AeroSpace workspace whose name is a positive integer.
//!   Declare them (`persistent-workspaces = ["1", …]`); non-numeric
//!   workspaces are invisible to bsctl — there is no stable id to give them.
//! - Workspace display names are bsctl-owned ([`ws::names_file`]):
//!   AeroSpace has no rename, a workspace's name is its id.
//! - Displays: `monitor-id` is AeroSpace's left-to-right numbering, so the
//!   synthesized `x` is `monitor-id - 1` — an ORDINAL, not pixels (AeroSpace
//!   reports no geometry). Duplicate monitor names get a ` #<monitor-id>`
//!   suffix so names stay unique, which display addressing requires.
//! - Clients: one row per window; `address` is the AeroSpace window-id,
//!   `pid` the app's pid. kitty runs ONE process for all its windows on
//!   macOS, so a pid match is ambiguous — [`client_for_pids`] narrows it
//!   with kitty's own window id (see there).
//!
//! The CLI is reached through PATH (`aerospace`), like hyprctl, so tests
//! substitute a fake. One query round (workspaces + windows + monitors +
//! focused monitor) is memoized for 250ms per process: the world asks for
//! `workspaces` then `monitors` back to back, and each round is four CLI
//! calls. Every [`act`] drops the memo.

use std::collections::{BTreeMap, HashSet};
use std::env;
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::ws::{self, Action, WsToken};

const WS_FMT: &str =
    "%{workspace} %{workspace-is-visible} %{workspace-is-focused} %{monitor-id} %{monitor-name}";
const WIN_FMT: &str = "%{window-id} %{app-pid} %{app-name} %{workspace}";
const MON_FMT: &str = "%{monitor-id} %{monitor-name}";
const MEMO: Duration = Duration::from_millis(250);

/// One raw query round, as the CLI returned it.
#[derive(Clone)]
pub struct Snap {
    pub workspaces: Value,
    pub windows: Value,
    pub monitors: Value,
    pub focused_monitor: Option<i64>,
}

static CACHE: Mutex<Option<(Instant, Snap)>> = Mutex::new(None);

fn cli_json(args: &[&str]) -> Option<Value> {
    let out = Command::new("aerospace")
        .args(args)
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    serde_json::from_slice(&out.stdout).ok()
}

/// Run an aerospace command for its effect; stderr passes through (the
/// CLI's own messages are the useful diagnostics), 127 when missing.
fn cli_run(args: &[&str]) -> i32 {
    match Command::new("aerospace")
        .args(args)
        .stdout(Stdio::null())
        .status()
    {
        Ok(s) => s.code().unwrap_or(1),
        Err(_) => {
            eprintln!("bsctl: aerospace not found");
            127
        }
    }
}

/// The memoized query round; None when AeroSpace isn't reachable.
pub fn snap() -> Option<Snap> {
    let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((at, s)) = cache.as_ref()
        && at.elapsed() < MEMO
    {
        return Some(s.clone());
    }
    let s = Snap {
        workspaces: cli_json(&["list-workspaces", "--all", "--json", "--format", WS_FMT])?,
        windows: cli_json(&["list-windows", "--all", "--json", "--format", WIN_FMT])?,
        monitors: cli_json(&["list-monitors", "--json", "--format", MON_FMT])?,
        focused_monitor: cli_json(&["list-monitors", "--focused", "--json", "--format", MON_FMT])
            .as_ref()
            .and_then(|v| v.as_array()?.first().map(|m| int(m, "monitor-id")))
            .flatten(),
    };
    *cache = Some((Instant::now(), s.clone()));
    Some(s)
}

fn invalidate() {
    *CACHE.lock().unwrap_or_else(|e| e.into_inner()) = None;
}

// ---- tolerant field readers (numbers or numeric strings; bools or "true") ---

fn int(v: &Value, k: &str) -> Option<i64> {
    match v.get(k)? {
        Value::Number(n) => n.as_i64(),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

fn flag(v: &Value, k: &str) -> bool {
    match v.get(k) {
        Some(Value::Bool(b)) => *b,
        Some(Value::String(s)) => s == "true",
        _ => false,
    }
}

fn text(v: &Value, k: &str) -> String {
    match v.get(k) {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => n.to_string(),
        _ => String::new(),
    }
}

/// A workspace name that is a ws-id: a positive integer, nothing else.
fn ws_id(name: &str) -> Option<i64> {
    name.parse::<i64>().ok().filter(|&id| id >= 1)
}

// ---- pure translators (unit-tested) -----------------------------------------

/// monitor-id -> unique display name (duplicates suffixed ` #<id>`).
pub fn monitor_names(monitors: &Value) -> BTreeMap<i64, String> {
    let rows: Vec<(i64, String)> = monitors
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|m| Some((int(m, "monitor-id")?, text(m, "monitor-name"))))
                .collect()
        })
        .unwrap_or_default();
    let mut out = BTreeMap::new();
    for (id, name) in &rows {
        let dup = rows.iter().filter(|(_, n)| n == name).count() > 1;
        out.insert(
            *id,
            if dup {
                format!("{name} #{id}")
            } else {
                name.clone()
            },
        );
    }
    out
}

/// Hyprland-shaped `workspaces`: `[{id, name, monitor, windows}]` for the
/// numeric workspaces, named from bsctl's names file (default: the id,
/// exactly as Hyprland names an unnamed workspace).
pub fn workspaces_from(s: &Snap, names: &BTreeMap<i64, String>) -> Value {
    let mons = monitor_names(&s.monitors);
    let mut counts: BTreeMap<i64, i64> = BTreeMap::new();
    for w in s.windows.as_array().into_iter().flatten() {
        if let Some(id) = ws_id(&text(w, "workspace")) {
            *counts.entry(id).or_default() += 1;
        }
    }
    let mut seen = HashSet::new();
    let rows: Vec<Value> = s
        .workspaces
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|w| {
            let id = ws_id(&text(w, "workspace"))?;
            seen.insert(id).then_some(())?;
            let monitor = int(w, "monitor-id")
                .and_then(|m| mons.get(&m).cloned())
                .unwrap_or_else(|| text(w, "monitor-name"));
            Some(json!({
                "id": id,
                "name": names.get(&id).cloned().unwrap_or_else(|| id.to_string()),
                "monitor": monitor,
                "windows": counts.get(&id).copied().unwrap_or(0),
            }))
        })
        .collect();
    Value::Array(rows)
}

/// Hyprland-shaped `monitors`: `[{name, id, x, y, focused,
/// activeWorkspace: {id}, disabled}]` — x is the ordinal (module header).
pub fn monitors_from(s: &Snap) -> Value {
    let mons = monitor_names(&s.monitors);
    let visible = |mid: i64| -> Option<i64> {
        s.workspaces.as_array()?.iter().find_map(|w| {
            (int(w, "monitor-id") == Some(mid) && flag(w, "workspace-is-visible"))
                .then(|| ws_id(&text(w, "workspace")))
                .flatten()
        })
    };
    Value::Array(
        mons.iter()
            .map(|(&mid, name)| {
                json!({
                    "id": mid,
                    "name": name,
                    "x": mid - 1,
                    "y": 0,
                    "focused": s.focused_monitor == Some(mid),
                    "activeWorkspace": {"id": visible(mid)},
                    "disabled": false,
                })
            })
            .collect(),
    )
}

/// Hyprland-shaped `clients`: `[{pid, address, workspace: {id}}]`, numeric
/// workspaces only (a window on a non-numeric workspace has no ws-id).
pub fn clients_from(s: &Snap) -> Value {
    Value::Array(
        s.windows
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|w| {
                let id = ws_id(&text(w, "workspace"))?;
                Some(json!({
                    "pid": int(w, "app-pid"),
                    "address": text(w, "window-id"),
                    "workspace": {"id": id},
                }))
            })
            .collect(),
    )
}

/// Hyprland-shaped `activeworkspace`: `{id}` of the focused numeric
/// workspace (None when the focused one isn't numeric).
pub fn active_from(s: &Snap) -> Option<Value> {
    let id = s.workspaces.as_array()?.iter().find_map(|w| {
        flag(w, "workspace-is-focused")
            .then(|| ws_id(&text(w, "workspace")))
            .flatten()
    })?;
    Some(json!({ "id": id }))
}

/// The aerospace argv for an action (None = nothing to run: renames are
/// bsctl-owned, unsupported actions are refused by [`act`]). Monitors are
/// addressed by AeroSpace monitor-id, resolved from our unique names.
pub fn action_argv(a: &Action, s: Option<&Snap>) -> Result<Option<Vec<String>>, String> {
    let ws_arg = |t: &WsToken| match t {
        WsToken::Id(id) => Ok(id.to_string()),
        WsToken::Name(n) => Err(format!(
            "named workspace {n:?} has no AeroSpace form (bsctl uses numeric workspaces)"
        )),
    };
    let monitor_id = |name: &str| -> Result<String, String> {
        let s = s.ok_or("AeroSpace is not reachable")?;
        monitor_names(&s.monitors)
            .into_iter()
            .find(|(_, n)| n == name)
            .map(|(id, _)| id.to_string())
            .ok_or_else(|| format!("no display named {name:?}"))
    };
    let v = |xs: &[&str]| Some(xs.iter().map(|x| x.to_string()).collect());
    Ok(match a {
        Action::Focus(t) => v(&["workspace", &ws_arg(t)?]),
        Action::FocusMonitor(m) => v(&["focus-monitor", &monitor_id(m)?]),
        Action::FocusWindow(addr) => v(&["focus", "--window-id", addr]),
        Action::MoveWindow(t, true) => v(&[
            "move-node-to-workspace",
            "--focus-follows-window",
            &ws_arg(t)?,
        ]),
        Action::MoveWindow(t, false) => v(&["move-node-to-workspace", &ws_arg(t)?]),
        Action::MoveWorkspace(id, m) => v(&[
            "move-workspace-to-monitor",
            "--workspace",
            &id.to_string(),
            &monitor_id(m)?,
        ]),
        Action::Rename(..) => None,
    })
}

// ---- the ipc surface -----------------------------------------------------------

/// [`crate::ipc::json`] under AeroSpace: the Hyprland query names this crate
/// uses, answered in Hyprland's shape. Unknown queries -> None.
pub fn json(query: &str) -> Option<Value> {
    let s = snap()?;
    match query.trim() {
        "workspaces" => Some(workspaces_from(&s, &ws::load_names())),
        "monitors" | "monitors all" => Some(monitors_from(&s)),
        "clients" => Some(clients_from(&s)),
        "activeworkspace" => active_from(&s),
        _ => None,
    }
}

/// [`crate::ipc::act`] under AeroSpace.
pub fn act(a: &Action) -> i32 {
    if let Action::Rename(id, name) = a {
        return ws::set_name(*id, name.as_deref());
    }
    let s = snap();
    let code = match action_argv(a, s.as_ref()) {
        Ok(Some(argv)) => {
            let refs: Vec<&str> = argv.iter().map(String::as_str).collect();
            cli_run(&refs)
        }
        Ok(None) => 0,
        Err(msg) => {
            eprintln!("bsctl ws: {msg}");
            1
        }
    };
    invalidate();
    code
}

/// [`crate::ipc::client_for_pids`] under AeroSpace: the FIRST window (in
/// AeroSpace's order) whose app pid is in the ancestor set — unless the
/// caller runs inside kitty, which on macOS serves every window from one
/// process: then the window is pinned through kitty itself, using the
/// `KITTY_LISTEN_ON` + `KITTY_WINDOW_ID` that kitty exports into every
/// child (the hook inherits them from the harness).
pub fn client_for_pids(pids: &[i64]) -> Option<(i64, Option<String>, Option<i64>)> {
    let s = snap()?;
    let clients = clients_from(&s);
    let set: HashSet<i64> = pids.iter().copied().collect();
    let matches: Vec<&Value> = clients
        .as_array()?
        .iter()
        .filter(|c| {
            c.get("pid")
                .and_then(Value::as_i64)
                .is_some_and(|p| set.contains(&p))
        })
        .collect();
    let pick = if matches.len() > 1 {
        kitty_platform_window()
            .and_then(|pw| {
                matches
                    .iter()
                    .find(|c| c.get("address").and_then(Value::as_str) == Some(pw.as_str()))
                    .copied()
            })
            .or_else(|| matches.first().copied())
    } else {
        matches.first().copied()
    }?;
    let ws = pick.pointer("/workspace/id").and_then(Value::as_i64)?;
    let win = pick
        .get("address")
        .and_then(Value::as_str)
        .filter(|a| !a.is_empty())
        .map(str::to_string);
    Some((ws, win, pick.get("pid").and_then(Value::as_i64)))
}

/// This process's kitty OS window, as its platform window id (the macOS
/// window number AeroSpace calls window-id). None outside kitty or when
/// kitty's remote control isn't listening.
fn kitty_platform_window() -> Option<String> {
    let sock = env::var("KITTY_LISTEN_ON").ok().filter(|s| !s.is_empty())?;
    let kwin: i64 = env::var("KITTY_WINDOW_ID").ok()?.parse().ok()?;
    let out = Command::new("kitty")
        .args(["@", "--to", &sock, "ls"])
        .stderr(Stdio::null())
        .output()
        .ok()?;
    platform_window_of(&serde_json::from_slice(&out.stdout).ok()?, kwin)
}

/// From `kitty @ ls` output: the `platform_window_id` of the OS window that
/// contains kitty window `kwin`.
pub fn platform_window_of(ls: &Value, kwin: i64) -> Option<String> {
    ls.as_array()?.iter().find_map(|osw| {
        let has = osw.get("tabs")?.as_array()?.iter().any(|t| {
            t.get("windows")
                .and_then(Value::as_array)
                .is_some_and(|ws| {
                    ws.iter()
                        .any(|w| w.get("id").and_then(Value::as_i64) == Some(kwin))
                })
        });
        has.then(|| text(osw, "platform_window_id"))
            .filter(|s| !s.is_empty())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap_fixture() -> Snap {
        Snap {
            workspaces: json!([
                {"workspace": "1", "workspace-is-visible": true, "workspace-is-focused": true, "monitor-id": 1, "monitor-name": "Built-in Retina Display"},
                {"workspace": "2", "workspace-is-visible": false, "workspace-is-focused": false, "monitor-id": 1, "monitor-name": "Built-in Retina Display"},
                {"workspace": "M", "workspace-is-visible": false, "workspace-is-focused": false, "monitor-id": 1, "monitor-name": "Built-in Retina Display"},
                {"workspace": "4", "workspace-is-visible": true, "workspace-is-focused": false, "monitor-id": 2, "monitor-name": "LG UltraFine"}
            ]),
            windows: json!([
                {"window-id": 101, "app-pid": 500, "app-name": "kitty", "workspace": "1"},
                {"window-id": 102, "app-pid": 500, "app-name": "kitty", "workspace": "4"},
                {"window-id": 103, "app-pid": 600, "app-name": "Safari", "workspace": "1"},
                {"window-id": 104, "app-pid": 700, "app-name": "Notes", "workspace": "M"}
            ]),
            monitors: json!([
                {"monitor-id": 1, "monitor-name": "Built-in Retina Display"},
                {"monitor-id": 2, "monitor-name": "LG UltraFine"}
            ]),
            focused_monitor: Some(1),
        }
    }

    #[test]
    fn workspaces_are_numeric_counted_and_named() {
        let names: BTreeMap<i64, String> = [(4, "infra".to_string())].into();
        let w = workspaces_from(&snap_fixture(), &names);
        assert_eq!(
            w,
            json!([
                {"id": 1, "name": "1", "monitor": "Built-in Retina Display", "windows": 2},
                {"id": 2, "name": "2", "monitor": "Built-in Retina Display", "windows": 0},
                {"id": 4, "name": "infra", "monitor": "LG UltraFine", "windows": 1}
            ])
        );
    }

    #[test]
    fn monitors_carry_ordinal_x_focus_and_visible_workspace() {
        let m = monitors_from(&snap_fixture());
        assert_eq!(m[0]["name"], "Built-in Retina Display");
        assert_eq!(m[0]["x"], 0);
        assert_eq!(m[0]["focused"], true);
        assert_eq!(m[0]["activeWorkspace"]["id"], 1);
        assert_eq!(m[1]["x"], 1);
        assert_eq!(m[1]["focused"], false);
        assert_eq!(m[1]["activeWorkspace"]["id"], 4);
        // the downstream display numbering agrees with AeroSpace's order
        let ds = ws::displays_from(&m);
        assert_eq!(ds[0].name, "Built-in Retina Display");
        assert_eq!(ds[1].active_ws, Some(4));
    }

    #[test]
    fn duplicate_monitor_names_are_made_unique() {
        let n = monitor_names(&json!([
            {"monitor-id": 1, "monitor-name": "DELL U2720Q"},
            {"monitor-id": 2, "monitor-name": "DELL U2720Q"},
            {"monitor-id": 3, "monitor-name": "Built-in"}
        ]));
        assert_eq!(n[&1], "DELL U2720Q #1");
        assert_eq!(n[&2], "DELL U2720Q #2");
        assert_eq!(n[&3], "Built-in");
    }

    #[test]
    fn clients_and_active_workspace() {
        let s = snap_fixture();
        let c = clients_from(&s);
        assert_eq!(
            c.as_array().unwrap().len(),
            3,
            "the window on M has no ws-id"
        );
        assert_eq!(
            c[0],
            json!({"pid": 500, "address": "101", "workspace": {"id": 1}})
        );
        assert_eq!(active_from(&s), Some(json!({"id": 1})));
        // the shared pure matcher picks the first ancestor-matching window
        assert_eq!(
            crate::ipc::client_of(&c, &[999, 500]),
            Some((1, Some("101".into()), Some(500)))
        );
    }

    #[test]
    fn string_typed_fields_are_tolerated() {
        let s = Snap {
            workspaces: json!([{"workspace": "3", "workspace-is-visible": "true", "workspace-is-focused": "true", "monitor-id": "1", "monitor-name": "A"}]),
            windows: json!([{"window-id": "9", "app-pid": "42", "workspace": "3"}]),
            monitors: json!([{"monitor-id": "1", "monitor-name": "A"}]),
            focused_monitor: Some(1),
        };
        assert_eq!(workspaces_from(&s, &BTreeMap::new())[0]["windows"], 1);
        assert_eq!(monitors_from(&s)[0]["activeWorkspace"]["id"], 3);
        assert_eq!(clients_from(&s)[0]["pid"], 42);
    }

    #[test]
    fn actions_translate_to_aerospace_argv() {
        let s = snap_fixture();
        let argv = |a: Action| action_argv(&a, Some(&s)).unwrap().unwrap().join(" ");
        assert_eq!(argv(Action::Focus(WsToken::Id(3))), "workspace 3");
        assert_eq!(
            argv(Action::MoveWindow(WsToken::Id(2), true)),
            "move-node-to-workspace --focus-follows-window 2"
        );
        assert_eq!(
            argv(Action::MoveWindow(WsToken::Id(2), false)),
            "move-node-to-workspace 2"
        );
        assert_eq!(
            argv(Action::FocusMonitor("LG UltraFine".into())),
            "focus-monitor 2"
        );
        assert_eq!(
            argv(Action::MoveWorkspace(4, "Built-in Retina Display".into())),
            "move-workspace-to-monitor --workspace 4 1"
        );
        assert_eq!(
            argv(Action::FocusWindow("101".into())),
            "focus --window-id 101"
        );
        assert_eq!(action_argv(&Action::Rename(1, None), Some(&s)), Ok(None));
        assert!(action_argv(&Action::Focus(WsToken::Name("x".into())), Some(&s)).is_err());
        assert!(action_argv(&Action::FocusMonitor("nope".into()), Some(&s)).is_err());
    }

    #[test]
    fn kitty_ls_resolves_the_os_window_of_a_kitty_window() {
        let ls = json!([
            {"id": 1, "platform_window_id": 8801, "tabs": [{"windows": [{"id": 1}, {"id": 2}]}]},
            {"id": 2, "platform_window_id": 8802, "tabs": [{"windows": [{"id": 3}]}, {"windows": [{"id": 7}]}]}
        ]);
        assert_eq!(platform_window_of(&ls, 2).as_deref(), Some("8801"));
        assert_eq!(platform_window_of(&ls, 7).as_deref(), Some("8802"));
        assert_eq!(platform_window_of(&ls, 99), None);
    }
}
