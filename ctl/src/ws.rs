//! `bsctl ws` — the workspace verbs: `focus` and `send` (one selector
//! grammar, mutation split — focus never mutates, send always does), `name`,
//! the battlespace `map` (bs-id -> ws-id), and the sparse workspace->display
//! `prefs` (written only by `prefs add` and `send workspace`, applied by
//! `prefs reconcile` and by the `--stream` engine when an output arrives).
//! The
//! map-file protocol, the battlespace model, the display numbering and the
//! preference protocol are specified in lib.rs ("Workspace display order");
//! the dispatch strings are byte-pinned (Hyprland's Lua parser is picky —
//! these are the known-good forms, the monitor ones discovered by live
//! probing). Exit codes: usage errors 2 via clap; a bs-id off the map's end
//! is a silent exit 1 (keybinds hit it constantly — SUPER+8 with five
//! workspaces — and their stderr goes nowhere useful).

use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::{ipc, proto, sys};

/// `${XDG_STATE_HOME:-$HOME/.local/state}/battlestation-workspaces/map`
/// (an empty env var counts as unset, like the sh `:-` default).
pub fn map_file() -> PathBuf {
    env::var_os("XDG_STATE_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env::var_os("HOME").unwrap_or_default()).join(".local/state")
        })
        .join("battlestation-workspaces/map")
}

/// Serialize every read-modify-write of the map/prefs files: a blocking
/// exclusive flock on `<dir>/.lock` held across load->modify->save, released
/// on drop. READERS take no lock — every write lands by atomic rename (or a
/// single write(2) for the map), so a reader sees old or new bytes, never a
/// torn file; the lock only stops two writers from losing an update to each
/// other. Best-effort: if the lock file can't be created the write proceeds
/// unlocked (state must never be droppable because /run filled up).
fn with_state_lock<T>(f: impl FnOnce() -> T) -> T {
    let lock = map_file().with_file_name(".lock");
    if let Some(dir) = lock.parent() {
        let _ = fs::create_dir_all(dir);
    }
    let held = fs::File::create(&lock)
        .ok()
        .filter(|l| sys::flock_exclusive(l, false));
    let out = f();
    drop(held);
    out
}

// ---- pure logic (proto.rs-style: deterministic, unit-tested) ---------------

/// Live, non-special workspace ids in hyprctl's order — the script's
/// `jq '.[] | select((.name // "") | startswith("special:") | not) | .id'`.
pub fn live_ids(workspaces: &Value) -> Vec<i64> {
    workspaces
        .as_array()
        .map(|a| {
            a.iter()
                .filter(|w| {
                    !w.get("name")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .starts_with("special:")
                })
                .filter_map(|w| w.get("id").and_then(Value::as_i64))
                .collect()
        })
        .unwrap_or_default()
}

/// Resolved display order: preference tokens filtered to live ids (in pref
/// order), then live ids not in the preference, ascending. Tokens match live
/// ids TEXTUALLY (the script's `grep -qxF` / `case " $pref "` membership), so
/// e.g. "07" never matches id 7, and a token repeated in the file repeats in
/// the output. Missing/empty preference -> identity (live ascending).
pub fn resolve(pref: &str, live: &[i64]) -> Vec<i64> {
    let toks: Vec<&str> = pref.split_whitespace().collect();
    let mut out: Vec<i64> = toks
        .iter()
        .filter_map(|t| live.iter().copied().find(|id| id.to_string() == **t))
        .collect();
    let mut rest: Vec<i64> = live
        .iter()
        .copied()
        .filter(|id| !toks.contains(&id.to_string().as_str()))
        .collect();
    rest.sort_unstable();
    out.extend(rest);
    out
}

/// relative's position arithmetic: current position (1-based; unknown -> 1)
/// plus delta, clamped to [1, n]. Callers guarantee n >= 1.
pub fn step(pos: Option<usize>, delta: i64, n: usize) -> usize {
    (pos.unwrap_or(1) as i64 + delta).clamp(1, n as i64) as usize
}

/// Escape `\` and `"` for embedding in a Lua string — the script's
/// `sed 's/[\\"]/\\&/g'`.
pub fn lua_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if c == '\\' || c == '"' {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// `hl.dsp.focus({ workspace = N })` — byte-identical to the script's.
pub fn focus_cmd(id: i64) -> String {
    format!("hl.dsp.focus({{ workspace = {id} }})")
}

/// `hl.dsp.focus({ monitor = "NAME" })` — the Lua form of legacy
/// `focusmonitor` (verified live: a no-op refocus of the focused monitor
/// replies ok; an unknown name replies `warning: ... monitor not found`).
pub fn focus_monitor_cmd(name: &str) -> String {
    format!(r#"hl.dsp.focus({{ monitor = "{}" }})"#, lua_escape(name))
}

/// `hl.dsp.workspace.move({ workspace = N, monitor = "NAME" })` — the Lua
/// form of legacy `moveworkspacetomonitor` (verified live: a no-op move of a
/// workspace to its own monitor replies ok; omitting `monitor` replies
/// `error: ... 'monitor' is required`).
pub fn move_workspace_cmd(id: i64, monitor: &str) -> String {
    format!(
        r#"hl.dsp.workspace.move({{ workspace = {id}, monitor = "{}" }})"#,
        lua_escape(monitor)
    )
}

/// One enabled output, in display order. Display numbering is a pure remap
/// of `monitors` JSON: enabled outputs sorted by (x, y), 1-based — leftmost
/// is display 1. Like workspace positions, the number is a display-layer
/// concept; output names stay the compositor's.
pub struct Display {
    pub name: String,
    pub focused: bool,
    pub active_ws: Option<i64>,
}

/// Displays from a `monitors` array: disabled outputs dropped, the rest
/// sorted by (x, y). Coordinates compare as f64 (the JSON's native type);
/// ties break by name so the numbering stays deterministic.
pub fn displays_from(monitors: &Value) -> Vec<Display> {
    let Some(arr) = monitors.as_array() else {
        return Vec::new();
    };
    let mut mons: Vec<(f64, f64, Display)> = arr
        .iter()
        .filter(|m| !m.get("disabled").and_then(Value::as_bool).unwrap_or(false))
        .map(|m| {
            let f = |k: &str| m.get(k).and_then(Value::as_f64).unwrap_or(0.0);
            (
                f("x"),
                f("y"),
                Display {
                    name: m
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string(),
                    focused: m.get("focused").and_then(Value::as_bool).unwrap_or(false),
                    active_ws: m
                        .get("activeWorkspace")
                        .and_then(|w| w.get("id"))
                        .and_then(Value::as_i64),
                },
            )
        })
        .collect();
    mons.sort_by(|a, b| {
        a.0.total_cmp(&b.0)
            .then(a.1.total_cmp(&b.1))
            .then_with(|| a.2.name.cmp(&b.2.name))
    });
    mons.into_iter().map(|(_, _, d)| d).collect()
}

/// `hl.dsp.window.move({ workspace = N, follow = B })` — byte-identical.
pub fn move_cmd(id: i64, follow: bool) -> String {
    format!("hl.dsp.window.move({{ workspace = {id}, follow = {follow} }})")
}

/// `hl.dsp.workspace.rename({ workspace = N, name = "..." })` —
/// byte-identical, including the empty-name -> id-number default and the
/// `\`/`"` escaping.
pub fn rename_cmd(id: i64, name: Option<&str>) -> String {
    let name = match name {
        Some(n) if !n.is_empty() => lua_escape(n),
        _ => id.to_string(),
    };
    format!(r#"hl.dsp.workspace.rename({{ workspace = {id}, name = "{name}" }})"#)
}

/// The `order` listing's name lookup — the script's
/// `jq -r '.[] | select(.id==ID) | .name'`: first matching workspace's name;
/// a present entry with a null/missing name prints "null" (jq -r's
/// rendering of null); no matching entry prints "".
pub fn name_of(workspaces: &Value, id: i64) -> String {
    let Some(w) = workspaces.as_array().and_then(|a| {
        a.iter()
            .find(|w| w.get("id").and_then(Value::as_i64) == Some(id))
    }) else {
        return String::new();
    };
    match w.get("name") {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Null) | None => "null".to_string(),
        Some(v) => v.to_string(),
    }
}

// ---- workspace->display preference ------------------------------------------

/// `${XDG_STATE_HOME:-$HOME/.local/state}/battlestation-workspaces/prefs`
/// — one `<id> <output>` pair per line, sorted by id, trailing newline. A
/// sibling of the map file with the same persistence rationale: a
/// preference is user INTENT, and intent outlives boots (workspace ids are
/// stable habits under global numbering).
pub fn prefs_file() -> PathBuf {
    map_file().with_file_name("prefs")
}

/// Preference-file parse. Lines that aren't exactly `<id> <output>` are
/// skipped — a corrupt line loses one preference, never the file; a
/// duplicated id keeps the last line (later = newer intent).
pub fn parse_prefs(content: &str) -> BTreeMap<i64, String> {
    let mut out = BTreeMap::new();
    for line in content.lines() {
        let mut f = line.split_whitespace();
        if let (Some(id), Some(output), None) = (f.next(), f.next(), f.next())
            && let Ok(id) = id.parse::<i64>()
        {
            out.insert(id, output.to_string());
        }
    }
    out
}

/// The inverse of [`parse_prefs`]: one pair per line, sorted by id (the
/// map's order); no preferences serialize to the empty file.
pub fn serialize_prefs(prefs: &BTreeMap<i64, String>) -> String {
    prefs
        .iter()
        .map(|(id, output)| format!("{id} {output}\n"))
        .collect()
}

/// [`parse_prefs`] from a file; missing/unreadable reads as no preferences.
pub fn load_prefs_from(path: &Path) -> BTreeMap<i64, String> {
    parse_prefs(&fs::read_to_string(path).unwrap_or_default())
}

/// Atomic write (temp+rename in the same dir, the crate's pattern);
/// best-effort like every preference write — failures are dropped.
pub fn save_prefs_to(path: &Path, prefs: &BTreeMap<i64, String>) {
    let Some(dir) = path.parent() else { return };
    let _ = fs::create_dir_all(dir);
    let tmp = dir.join(".prefs.tmp");
    if fs::write(&tmp, serialize_prefs(prefs)).is_ok() {
        let _ = fs::rename(&tmp, path);
    }
}

/// [`load_prefs_from`] / [`save_prefs_to`] bound to [`prefs_file`].
pub fn load_prefs() -> BTreeMap<i64, String> {
    load_prefs_from(&prefs_file())
}

fn save_prefs(prefs: &BTreeMap<i64, String>) {
    save_prefs_to(&prefs_file(), prefs)
}

/// Stamp preferences — the write side of the model's one rule: a
/// preference records EXPLICIT user intent, so the stamping call sites are
/// exactly the two homing verbs (`prefs add`, `send workspace`) and nothing
/// else; reorders and bulk operations never stamp, and the stream engine and Hyprland
/// never stamp (evacuations and automatic restores are not intent).
/// Locked: two concurrent stamps must not lose each other's update.
pub fn stamp(pairs: &[(i64, String)]) {
    if pairs.is_empty() {
        return;
    }
    with_state_lock(|| {
        let mut prefs = load_prefs();
        for (id, output) in pairs {
            prefs.insert(*id, output.clone());
        }
        save_prefs(&prefs);
    })
}

/// Bounded settle-and-restore: re-read placement every 200ms (monitor
/// re-application lands async), move each strayed workspace home
/// ([`restore_plan`] against `before`), and stop after a clean pass. Then
/// re-assert what each display SHOWS (its `before_actives` entry), ending on
/// `focused_mon`'s active so the keyboard stays put. Best-effort by design —
/// the caller's primary operation has already succeeded, so failures here
/// never affect its exit code.
pub fn restore_strays(
    before: &[(i64, String)],
    before_actives: &[(String, Option<i64>)],
    focused_mon: &str,
) {
    let mut restored_any = false;
    for _ in 0..5 {
        std::thread::sleep(std::time::Duration::from_millis(200));
        let Some(after) = ipc::json("workspaces").map(|w| monitor_map(&w)) else {
            break;
        };
        let plan = restore_plan(before, &after);
        if plan.is_empty() {
            break;
        }
        restored_any = true;
        for cmd in plan {
            let _ = ipc::dispatch(&cmd);
        }
    }
    if !restored_any {
        return; // nothing strayed: don't churn focus for no reason
    }
    // Unfocused displays first, the focused monitor's own active last.
    for (mon, active) in before_actives.iter().filter(|(m, _)| m != focused_mon) {
        if let Some(id) = active {
            let _ = ipc::dispatch(&focus_cmd(*id));
        }
        let _ = mon;
    }
    if let Some((_, Some(id))) = before_actives.iter().find(|(m, _)| m == focused_mon) {
        let _ = ipc::dispatch(&focus_cmd(*id));
    }
}

/// The apply side, shared by `ws prefs reconcile` (every present output)
/// and the stream engine's monitoradded handling (`only` = the arriving
/// output): move each
/// live workspace that prefers a present output and sits elsewhere, via
/// the bounded settle machinery ([`restore_strays`]). Returns the moves it
/// planned (id -> output) for callers that report, or None when the
/// compositor queries fail — best-effort; the preferences stay put for the
/// next chance.
pub fn apply_preferences(only: Option<&str>) -> Option<Vec<(i64, String)>> {
    let (mons, ws) = (ipc::json("monitors")?, ipc::json("workspaces")?);
    let ds = displays_from(&mons);
    let live = live_ids(&ws);
    // Live workspaces whose preferred output is present (and in scope) —
    // restore_plan then moves only the subset that actually strayed.
    let before: Vec<(i64, String)> = load_prefs()
        .into_iter()
        .filter(|(id, output)| {
            live.contains(id)
                && only.is_none_or(|o| o == output)
                && ds.iter().any(|d| d.name == *output)
        })
        .collect();
    let placements = monitor_map(&ws);
    let moves: Vec<(i64, String)> = before
        .iter()
        .filter(|(id, output)| {
            placements
                .iter()
                .any(|(pid, mon)| pid == id && mon != output)
        })
        .cloned()
        .collect();
    if moves.is_empty() {
        return Some(moves); // nothing strayed: no dispatch, no focus churn
    }
    let focused_mon = ds
        .iter()
        .find(|d| d.focused)
        .map(|d| d.name.clone())
        .unwrap_or_default();
    // What each display should SHOW afterwards: one gaining preferred
    // workspaces shows one of them — its current active if that already
    // prefers it, else the first preferring id (all live by construction,
    // so the focus-a-dead-id-creates-it trap can't spring); every other
    // display keeps its view, and the keyboard ends on the focused display.
    let before_actives: Vec<(String, Option<i64>)> = ds
        .iter()
        .map(|d| {
            let preferring: Vec<i64> = before
                .iter()
                .filter(|(_, o)| *o == d.name)
                .map(|(id, _)| *id)
                .collect();
            let active = match d.active_ws {
                Some(a) if preferring.is_empty() || preferring.contains(&a) => Some(a),
                _ => preferring.first().copied().or(d.active_ws),
            };
            (d.name.clone(), active)
        })
        .collect();
    restore_strays(&before, &before_actives, &focused_mon);
    Some(moves)
}

// ---- selectors ----------------------------------------------------------------

/// A workspace-shaped selector: which workspace a verb should act on.
/// The three coordinate systems are deliberate (nomenclature in lib.rs):
/// `Bs` addresses by battlespace id (1-based position in the resolved map —
/// what the number-row keybinds mean), `BsRel` steps through battlespace
/// order from the active workspace, `Ws` is the raw Hyprland id (stable
/// across reorders — what scripts and prefs mean).
pub enum WsSel {
    Bs(usize),
    BsRel(i64),
    Ws(i64),
}

/// A display-shaped selector: by display-id (1-based, leftmost first — a
/// pure remap of the enabled outputs) or by the compositor's output name.
pub enum DisplaySel {
    Id(usize),
    Name(String),
}

/// `focus`/`send window` accept either shape; the split is what lets clap
/// enforce per-verb selector subsets while the resolution lives here once.
pub enum Target {
    Ws(WsSel),
    Display(DisplaySel),
}

/// Resolve a workspace selector to a real ws-id. Err carries the exit code:
/// a bs-id off the map's end is a SILENT exit 1 (module-header rationale);
/// bs-rel over an empty world is a no-op exit 0 (nothing to step through);
/// a raw ws-id resolves without any query (Hyprland will create it on
/// focus — documented in the CLI help).
pub fn resolve_ws_sel(sel: &WsSel) -> Result<i64, i32> {
    match sel {
        WsSel::Ws(id) => Ok(*id),
        WsSel::Bs(n) => {
            let ws = workspaces_json()?;
            resolve(&read_map(), &live_ids(&ws))
                .get(n - 1)
                .copied()
                .ok_or(1)
        }
        WsSel::BsRel(delta) => {
            let ws = workspaces_json()?;
            let resolved = resolve(&read_map(), &live_ids(&ws));
            if resolved.is_empty() {
                return Err(0);
            }
            let Some(active) = ipc::json("activeworkspace") else {
                eprintln!("bsctl ws: activeworkspace query failed (socket and hyprctl)");
                return Err(1);
            };
            // Position of the active id in the resolved order; an active
            // workspace outside it (e.g. special) defaults to position 1.
            let pos = active
                .get("id")
                .and_then(Value::as_i64)
                .and_then(|cur| resolved.iter().position(|&id| id == cur))
                .map(|i| i + 1);
            Ok(resolved[step(pos, *delta, resolved.len()) - 1])
        }
    }
}

/// Resolve a display selector to its index in `ds`; unknown ids/names error
/// loudly listing the valid numbering (the fix is in the message).
pub fn resolve_display_sel(verb: &str, sel: &DisplaySel, ds: &[Display]) -> Result<usize, i32> {
    match sel {
        DisplaySel::Id(n) => {
            if *n >= 1 && *n <= ds.len() {
                Ok(n - 1)
            } else {
                Err(no_such_display(verb, &n.to_string(), ds))
            }
        }
        DisplaySel::Name(name) => ds
            .iter()
            .position(|d| d.name == *name)
            .ok_or_else(|| no_such_display(verb, name, ds)),
    }
}

// ---- commands ---------------------------------------------------------------

/// Map-file content; missing file reads as empty (identity order).
fn read_map() -> String {
    fs::read_to_string(map_file()).unwrap_or_default()
}

/// `hyprctl workspaces -j` (socket-first via ipc), or the script's set -e
/// death when both paths fail: report and exit 1.
fn workspaces_json() -> Result<Value, i32> {
    ipc::json("workspaces").ok_or_else(|| {
        eprintln!("bsctl ws: workspaces query failed (socket and hyprctl)");
        1
    })
}

/// `hyprctl monitors -j` (socket-first via ipc; without `all`, so disabled
/// outputs never enter the numbering) — same failure contract as
/// [`workspaces_json`].
fn monitors_json() -> Result<Value, i32> {
    ipc::json("monitors").ok_or_else(|| {
        eprintln!("bsctl ws: monitors query failed (socket and hyprctl)");
        1
    })
}

/// Unknown display error: name the valid numbering so the fix is in the
/// message (e.g. `displays: 1 = eDP-1, 2 = DP-1`).
fn no_such_display(verb: &str, wanted: &str, ds: &[Display]) -> i32 {
    let list = ds
        .iter()
        .enumerate()
        .map(|(i, d)| format!("{} = {}", i + 1, d.name))
        .collect::<Vec<_>>()
        .join(", ");
    eprintln!("bsctl ws {verb}: no display {wanted} (displays: {list})");
    1
}

/// The focused display's index in `ds` — exactly one enabled output is
/// focused in a healthy compositor, so None means the query is broken.
fn focused_index(verb: &str, ds: &[Display]) -> Result<usize, i32> {
    ds.iter().position(|d| d.focused).ok_or_else(|| {
        eprintln!("bsctl ws {verb}: no focused display in monitors query");
        1
    })
}

/// `ws focus <selector>` — pure navigation, never mutates: workspace
/// targets focus that workspace (wherever it lives), display targets focus
/// that display (its active workspace).
pub fn focus(target: &Target) -> i32 {
    match target {
        Target::Ws(sel) => match resolve_ws_sel(sel) {
            Ok(id) => ipc::dispatch(&focus_cmd(id)),
            Err(c) => c,
        },
        Target::Display(sel) => {
            let mons = match monitors_json() {
                Ok(v) => v,
                Err(c) => return c,
            };
            let ds = displays_from(&mons);
            match resolve_display_sel("focus", sel, &ds) {
                Ok(i) => ipc::dispatch(&focus_monitor_cmd(&ds[i].name)),
                Err(c) => c,
            }
        }
    }
}

/// `ws send window <selector> [--focus]` — move the active window: to the
/// selected workspace, or (display targets) to that display's ACTIVE
/// workspace. `--focus` follows the window; without it the keyboard stays.
pub fn send_window(target: &Target, focus: bool) -> i32 {
    let id = match target {
        Target::Ws(sel) => match resolve_ws_sel(sel) {
            Ok(id) => id,
            Err(c) => return c,
        },
        Target::Display(sel) => {
            let mons = match monitors_json() {
                Ok(v) => v,
                Err(c) => return c,
            };
            let ds = displays_from(&mons);
            let i = match resolve_display_sel("send window", sel, &ds) {
                Ok(i) => i,
                Err(c) => return c,
            };
            let Some(active) = ds[i].active_ws else {
                eprintln!(
                    "bsctl ws send window: display {} has no active workspace",
                    ds[i].name
                );
                return 1;
            };
            active
        }
    };
    ipc::dispatch(&move_cmd(id, focus))
}

/// `ws send workspace (--display-id|--display-name) [--focus]` — move the
/// ACTIVE workspace to a display, keeping its ws-id. One of the two
/// preference writers: an explicit move is the user re-deciding this
/// workspace's home.
pub fn send_workspace(sel: &DisplaySel, focus: bool) -> i32 {
    let mons = match monitors_json() {
        Ok(v) => v,
        Err(c) => return c,
    };
    let ds = displays_from(&mons);
    let tgt = match resolve_display_sel("send workspace", sel, &ds) {
        Ok(i) => i,
        Err(c) => return c,
    };
    let cur = match focused_index("send workspace", &ds) {
        Ok(i) => i,
        Err(c) => return c,
    };
    if cur == tgt {
        return 0; // already on that display: nothing to move
    }
    let Some(ws_id) = ds[cur].active_ws else {
        eprintln!(
            "bsctl ws send workspace: focused display {} has no active workspace",
            ds[cur].name
        );
        return 1;
    };
    let code = ipc::dispatch(&move_workspace_cmd(ws_id, &ds[tgt].name));
    if code != 0 {
        return code;
    }
    // The move landed: stamp the preference (and the stamp must not depend
    // on the focus pin below succeeding).
    stamp(&[(ws_id, ds[tgt].name.clone())]);
    // Pin focus explicitly rather than trusting the move's inherent focus
    // behavior (version-dependent): --focus lands on the moved workspace,
    // stay re-focuses the source display — both no-op when already true.
    if focus {
        ipc::dispatch(&focus_cmd(ws_id))
    } else {
        ipc::dispatch(&focus_monitor_cmd(&ds[cur].name))
    }
}

/// id -> monitor for every non-special workspace, sorted by id — the
/// placement snapshot [`restore_plan`] diffs.
pub fn monitor_map(workspaces: &Value) -> Vec<(i64, String)> {
    let mut out: Vec<(i64, String)> = workspaces
        .as_array()
        .map(|a| {
            a.iter()
                .filter(|w| {
                    !w.get("name")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .starts_with("special:")
                })
                .filter_map(|w| {
                    Some((
                        w.get("id").and_then(Value::as_i64)?,
                        w.get("monitor").and_then(Value::as_str)?.to_string(),
                    ))
                })
                .collect()
        })
        .unwrap_or_default();
    out.sort_by_key(|(id, _)| *id);
    out
}

/// Move-back commands for every workspace that STRAYED: present in both
/// snapshots but now on a different monitor. Hyprland evacuates a monitor's
/// workspaces when it is re-applied (a scale/geometry change can transiently
/// cycle a neighbor), and never moves them back — this plan does. Workspaces
/// that appeared or vanished between snapshots are left alone.
pub fn restore_plan(before: &[(i64, String)], after: &[(i64, String)]) -> Vec<String> {
    after
        .iter()
        .filter_map(|(id, mon)| {
            let (_, want) = before.iter().find(|(bid, _)| bid == id)?;
            (want != mon).then(|| move_workspace_cmd(*id, want))
        })
        .collect()
}

/// A name/map row filter: everything, one workspace, or one display's
/// workspaces.
pub enum RowFilter {
    All,
    Ws(WsSel),
    Display(DisplaySel),
}

/// One row of the battlespace join: everything the map/name/status views
/// print.
pub struct BsRow {
    pub bs: usize,
    pub ws: i64,
    pub name: String,
    pub display: String,
    pub windows: i64,
    pub active: bool,
}

/// The full battlespace join, pure: map bytes + the two compositor
/// snapshots in, rows in battlespace order out. `active` = some display is
/// showing the workspace. Shared by the ws views and the `status` world
/// snapshot, so the two can never disagree about what a battlespace is.
pub fn bs_join(map_content: &str, ws: &Value, mons: &Value) -> Vec<BsRow> {
    let ds = displays_from(mons);
    let actives: Vec<i64> = ds.iter().filter_map(|d| d.active_ws).collect();
    let placements = monitor_map(ws);
    let windows_of = |id: i64| -> i64 {
        ws.as_array()
            .and_then(|a| {
                a.iter()
                    .find(|w| w.get("id").and_then(Value::as_i64) == Some(id))
            })
            .and_then(|w| w.get("windows"))
            .and_then(Value::as_i64)
            .unwrap_or(0)
    };
    resolve(map_content, &live_ids(ws))
        .into_iter()
        .enumerate()
        .map(|(i, id)| BsRow {
            bs: i + 1,
            ws: id,
            name: name_of(ws, id),
            display: placements
                .iter()
                .find(|(pid, _)| *pid == id)
                .map(|(_, m)| m.clone())
                .unwrap_or_default(),
            windows: windows_of(id),
            active: actives.contains(&id),
        })
        .collect()
}

/// The resolved battlespace join, queried live and filtered. Errors carry
/// the exit code (query failures, unknown display/bs selectors).
fn bs_rows(verb: &str, filter: &RowFilter) -> Result<Vec<BsRow>, i32> {
    let ws = workspaces_json()?;
    let mons = monitors_json()?;
    let ds = displays_from(&mons);
    let only_ws = match filter {
        RowFilter::Ws(sel) => Some(resolve_ws_sel(sel)?),
        _ => None,
    };
    let only_display = match filter {
        RowFilter::Display(sel) => Some(ds[resolve_display_sel(verb, sel, &ds)?].name.clone()),
        _ => None,
    };
    Ok(bs_join(&read_map(), &ws, &mons)
        .into_iter()
        .filter(|r| only_ws.is_none_or(|w| w == r.ws))
        .filter(|r| only_display.as_ref().is_none_or(|od| r.display == *od))
        .collect())
}

/// A workspace's human name, or None for the unnamed default (Hyprland
/// names every workspace its own number until someone renames it).
pub fn human_name(row_name: &str, ws: i64) -> Option<String> {
    (!row_name.is_empty() && row_name != "null" && row_name != ws.to_string())
        .then(|| row_name.to_string())
}

/// `ws name get [filter]` — BS/WS/NAME table over the matched workspaces
/// (NAME empty while a workspace still wears its default number). Empty
/// matches print nothing, not a lonely header.
pub fn name_get(filter: &RowFilter) -> i32 {
    let rows = match bs_rows("name get", filter) {
        Ok(r) => r,
        Err(c) => return c,
    };
    if rows.is_empty() {
        return 0;
    }
    let cells: Vec<Vec<String>> = rows
        .iter()
        .map(|r| {
            vec![
                r.bs.to_string(),
                r.ws.to_string(),
                human_name(&r.name, r.ws).unwrap_or_default(),
            ]
        })
        .collect();
    println!("{}", proto::render_table(&["BS", "WS", "NAME"], &cells));
    0
}

/// `ws name set (--bs-id|--ws-id) --name <name>` — Hyprland can't renumber
/// an id; this only changes the display name (hl.dsp.workspace.rename — the
/// Lua config parser rejects `hyprctl dispatch renameworkspace`).
pub fn name_set(sel: &WsSel, name: &str) -> i32 {
    match resolve_ws_sel(sel) {
        Ok(id) => ipc::dispatch(&rename_cmd(id, Some(name))),
        Err(c) => c,
    }
}

/// `ws name rm [filter]` — reset matched workspaces' names back to their
/// numbers (rename_cmd's empty-name default). All matches are attempted;
/// the exit code is the first failure's.
pub fn name_rm(filter: &RowFilter) -> i32 {
    let rows = match bs_rows("name rm", filter) {
        Ok(r) => r,
        Err(c) => return c,
    };
    let mut code = 0;
    for r in rows {
        let c = ipc::dispatch(&rename_cmd(r.ws, None));
        if code == 0 {
            code = c;
        }
    }
    code
}

/// `ws map get [--display-*] [--format json]` — the resolved battlespace
/// join, one row per battlespace: bs-id, ws-id, name, display, windows,
/// active. THE learning view of the model (bs 3 = "SUPER+3 goes here").
pub fn map_get(filter: &RowFilter, json_out: bool) -> i32 {
    let render = if json_out {
        map_json(filter)
    } else {
        map_text(filter)
    };
    match render {
        Ok(s) => {
            if json_out {
                println!("{s}");
            } else {
                print!("{s}");
            }
            0
        }
        Err(c) => c,
    }
}

/// One `map get` text result — the table (newline-terminated), or "" for
/// no rows (no lonely headers). Shared by the one-shot form and its
/// `--stream` text framing.
pub fn map_text(filter: &RowFilter) -> Result<String, i32> {
    let rows = bs_rows("map get", filter)?;
    if rows.is_empty() {
        return Ok(String::new());
    }
    Ok(proto::render_table(&MAP_HEADERS, &map_cells(&rows)) + "\n")
}

/// The map table's shape, shared with `status`'s workspaces section (which
/// appends a PREF column) so the two views can't drift.
pub const MAP_HEADERS: [&str; 6] = ["BS", "WS", "NAME", "DISPLAY", "WINDOWS", "ACTIVE"];

/// [`MAP_HEADERS`]'s cells for one set of join rows (NAME empty while
/// unnamed, ACTIVE the yes/empty dialect).
pub fn map_cells(rows: &[BsRow]) -> Vec<Vec<String>> {
    rows.iter()
        .map(|r| {
            vec![
                r.bs.to_string(),
                r.ws.to_string(),
                human_name(&r.name, r.ws).unwrap_or_default(),
                r.display.clone(),
                r.windows.to_string(),
                proto::yes(r.active),
            ]
        })
        .collect()
}

/// The map rows as their published JSON shape — shared by `map get
/// --format json` and its `--stream` form.
pub fn map_rows_json(rows: &[BsRow]) -> Vec<Value> {
    rows.iter()
        .map(|r| {
            json!({
                "bs": r.bs,
                "ws": r.ws,
                "name": human_name(&r.name, r.ws),
                "display": r.display,
                "windows": r.windows,
                "active": r.active,
            })
        })
        .collect()
}

/// One `map get --format json` result as its JSON line (the streaming form).
pub fn map_json(filter: &RowFilter) -> Result<String, i32> {
    Ok(Value::Array(map_rows_json(&bs_rows("map get", filter)?)).to_string())
}

/// `ws map set <ws-id>..` — ws-ids in battlespace order, the FULL list
/// (stable denotation: tokens are ws-ids, never positions — a permutation
/// of positions would be relative to the map it replaces and compose
/// confusingly). Bytes are `id id id\n`; the write is locked but lands via
/// one write(2), so readers never tear.
pub fn map_set(ids: &[i64]) -> i32 {
    let p = map_file();
    if let Some(d) = p.parent() {
        let _ = fs::create_dir_all(d);
    }
    let body = ids.iter().map(i64::to_string).collect::<Vec<_>>().join(" ") + "\n";
    with_state_lock(|| match fs::write(&p, body) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("bsctl ws map set: {}: {e}", p.display());
            1
        }
    })
}

/// `ws map reset` — truncate IN PLACE (resolve() then falls back to
/// identity order). Not in the design sketch, but without it the only route
/// back to identity is hand-editing the file, which the
/// everything-through-bsctl rule exists to prevent.
pub fn map_reset() -> i32 {
    let p = map_file();
    if let Some(d) = p.parent() {
        let _ = fs::create_dir_all(d);
    }
    with_state_lock(|| match fs::write(&p, "") {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("bsctl ws map reset: {}: {e}", p.display());
            1
        }
    })
}

/// The preferences with reality annotations, pure: `(ws, display, present,
/// live)` per entry — present = the output is enabled now, live = the
/// workspace still exists. Shared by `prefs get` and the `status` world
/// snapshot.
pub fn pref_rows(
    prefs: &BTreeMap<i64, String>,
    ds: &[Display],
    live: &[i64],
) -> Vec<(i64, String, bool, bool)> {
    prefs
        .iter()
        .map(|(id, output)| {
            (
                *id,
                output.clone(),
                ds.iter().any(|d| d.name == *output),
                live.contains(id),
            )
        })
        .collect()
}

/// The pref rows as their published JSON shape.
pub fn pref_rows_json(rows: &[(i64, String, bool, bool)]) -> Vec<Value> {
    rows.iter()
        .map(|(id, output, present, alive)| {
            json!({"ws": id, "display": output, "present": present, "live": alive})
        })
        .collect()
}

/// `prefs get`'s annotated rows, queried live and filtered. Empty
/// preferences short-circuit to no rows without touching the compositor.
fn prefs_rows_live(sel: Option<&WsSel>) -> Result<Vec<(i64, String, bool, bool)>, i32> {
    let prefs = load_prefs();
    if prefs.is_empty() {
        return Ok(Vec::new());
    }
    let only = match sel {
        Some(s) => Some(resolve_ws_sel(s)?),
        None => None,
    };
    let mons = monitors_json()?;
    let ws = workspaces_json()?;
    let rows = pref_rows(&prefs, &displays_from(&mons), &live_ids(&ws));
    Ok(rows
        .into_iter()
        .filter(|(id, ..)| only.is_none_or(|o| o == *id))
        .collect())
}

/// One `prefs get --format json` result as its JSON line (the streaming
/// form).
pub fn prefs_json(sel: Option<&WsSel>) -> Result<String, i32> {
    Ok(Value::Array(pref_rows_json(&prefs_rows_live(sel)?)).to_string())
}

/// `ws prefs get [--bs-id|--ws-id] [--format json]` — the preferences with
/// reality annotations: the output's presence among the enabled outputs,
/// and whether the workspace still exists. Empty preferences print nothing
/// (and query nothing).
pub fn prefs_get(sel: Option<&WsSel>, json_out: bool) -> i32 {
    let render = if json_out {
        prefs_json(sel)
    } else {
        prefs_text(sel)
    };
    match render {
        Ok(s) => {
            if json_out {
                println!("{s}");
            } else {
                print!("{s}");
            }
            0
        }
        Err(c) => c,
    }
}

/// One `prefs get` text result — the table (newline-terminated), or "" for
/// no preferences. Shared by the one-shot form and its `--stream` text
/// framing.
pub fn prefs_text(sel: Option<&WsSel>) -> Result<String, i32> {
    let rows = prefs_rows_live(sel)?;
    if rows.is_empty() {
        return Ok(String::new());
    }
    let cells: Vec<Vec<String>> = rows
        .iter()
        .map(|(id, output, present, alive)| {
            vec![
                id.to_string(),
                output.clone(),
                proto::yes_no(*present),
                proto::yes_no(*alive),
            ]
        })
        .collect();
    Ok(proto::render_table(&["WS", "DISPLAY", "PRESENT", "LIVE"], &cells) + "\n")
}

/// `ws prefs add (--bs-id|--ws-id) (--display-id|--display-name)` — the
/// explicit preference writer. `--display-name` is accepted VERBATIM even
/// when absent — pre-declaring a home for the dock display is the feature's
/// main move; `--display-id` must resolve (an id only numbers what's
/// plugged in).
pub fn prefs_add(sel: &WsSel, display: &DisplaySel) -> i32 {
    let id = match resolve_ws_sel(sel) {
        Ok(id) => id,
        Err(c) => return c,
    };
    let output = match display {
        DisplaySel::Name(name) => name.clone(),
        DisplaySel::Id(_) => {
            let mons = match monitors_json() {
                Ok(v) => v,
                Err(c) => return c,
            };
            let ds = displays_from(&mons);
            match resolve_display_sel("prefs add", display, &ds) {
                Ok(i) => ds[i].name.clone(),
                Err(c) => return c,
            }
        }
    };
    stamp(&[(id, output)]);
    0
}

/// `ws prefs rm (--bs-id|--ws-id|--all)` (clap enforces exactly one, so a
/// None selector MEANS --all): drop preferences. Removing an id that has no
/// preference is a quiet success — idempotent; `--all` writes the empty
/// file (atomically, like every preference write) rather than deleting it.
pub fn prefs_rm(sel: Option<&WsSel>) -> i32 {
    let Some(sel) = sel else {
        with_state_lock(|| save_prefs(&BTreeMap::new()));
        return 0;
    };
    let id = match resolve_ws_sel(sel) {
        Ok(id) => id,
        Err(c) => return c,
    };
    with_state_lock(|| {
        let mut prefs = load_prefs();
        if prefs.remove(&id).is_some() {
            save_prefs(&prefs);
        }
    });
    0
}

/// `ws prefs reconcile`: apply the preferences — move every live workspace
/// that prefers a present output and sits elsewhere, printing each move.
/// The manual counterpart of the stream engine's monitoradded apply, for
/// when nothing was streaming at replug time.
pub fn reconcile() -> i32 {
    match apply_preferences(None) {
        None => {
            eprintln!("bsctl ws prefs reconcile: compositor query failed (socket and hyprctl)");
            1
        }
        Some(moves) => {
            for (id, output) in moves {
                println!("ws {id} -> {output}");
            }
            0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ws_fixture() -> Value {
        json!([
            {"id": 2, "name": "2"},
            {"id": 1, "name": "1"},
            {"id": 5, "name": "work"},
            {"id": 3, "name": null},
            {"id": -99, "name": "special:magic"},
        ])
    }

    #[test]
    fn live_ids_excludes_special_keeps_hyprctl_order() {
        assert_eq!(live_ids(&ws_fixture()), vec![2, 1, 5, 3]);
        // missing name -> "" -> not special
        assert_eq!(live_ids(&json!([{"id": 7}])), vec![7]);
        assert_eq!(live_ids(&json!("not an array")), Vec::<i64>::new());
    }

    #[test]
    fn resolve_pref_first_then_newcomers_ascending() {
        let live = [2, 1, 5, 3];
        assert_eq!(resolve("3 1", &live), vec![3, 1, 2, 5]);
        // dead pref ids are skipped, order of the rest preserved
        assert_eq!(resolve("9 3 1", &live), vec![3, 1, 2, 5]);
        // empty / missing preference -> identity (ascending)
        assert_eq!(resolve("", &live), vec![1, 2, 3, 5]);
        assert_eq!(resolve(" \n", &live), vec![1, 2, 3, 5]);
        // full preference -> exactly it
        assert_eq!(resolve("5 3 2 1", &live), vec![5, 3, 2, 1]);
    }

    #[test]
    fn resolve_matches_tokens_textually() {
        // "07" is not the token "7" (grep -xF semantics)
        assert_eq!(resolve("07", &[7]), vec![7]); // skipped, then newcomer
        // a repeated token repeats in the output, and still blocks the
        // newcomer pass (case " $pref " membership)
        assert_eq!(resolve("3 3", &[3, 1]), vec![3, 3, 1]);
    }

    #[test]
    fn step_clamps_and_defaults() {
        assert_eq!(step(Some(2), 1, 4), 3);
        assert_eq!(step(Some(2), -1, 4), 1);
        assert_eq!(step(Some(4), 1, 4), 4); // clamped high
        assert_eq!(step(Some(1), -1, 4), 1); // clamped low
        assert_eq!(step(None, 1, 4), 2); // unknown position acts as 1
        assert_eq!(step(None, -1, 4), 1);
    }

    #[test]
    fn dispatch_strings_are_byte_identical_to_the_script() {
        assert_eq!(focus_cmd(3), "hl.dsp.focus({ workspace = 3 })");
        assert_eq!(
            move_cmd(1, false),
            "hl.dsp.window.move({ workspace = 1, follow = false })"
        );
        assert_eq!(
            move_cmd(5, true),
            "hl.dsp.window.move({ workspace = 5, follow = true })"
        );
        assert_eq!(
            rename_cmd(3, Some("plain name")),
            r#"hl.dsp.workspace.rename({ workspace = 3, name = "plain name" })"#
        );
        // empty/missing name -> the id number
        assert_eq!(
            rename_cmd(3, None),
            r#"hl.dsp.workspace.rename({ workspace = 3, name = "3" })"#
        );
        assert_eq!(
            rename_cmd(3, Some("")),
            r#"hl.dsp.workspace.rename({ workspace = 3, name = "3" })"#
        );
        // quotes and backslashes escaped like the sed (verified against the
        // script byte-for-byte, incl. this exact input)
        assert_eq!(
            rename_cmd(7, Some(r#"quo"te \back\ "x""#)),
            r#"hl.dsp.workspace.rename({ workspace = 7, name = "quo\"te \\back\\ \"x\"" })"#
        );
    }

    #[test]
    fn lua_escape_only_backslash_and_quote() {
        assert_eq!(lua_escape(""), "");
        assert_eq!(lua_escape("plain"), "plain");
        assert_eq!(lua_escape(r#"a"b"#), r#"a\"b"#);
        assert_eq!(lua_escape(r"a\b"), r"a\\b");
        assert_eq!(lua_escape("tab\t'nl'\n"), "tab\t'nl'\n"); // untouched
    }

    #[test]
    fn name_lookup_mirrors_jq_r() {
        let ws = ws_fixture();
        assert_eq!(name_of(&ws, 5), "work");
        assert_eq!(name_of(&ws, 3), "null"); // null name -> jq -r "null"
        assert_eq!(name_of(&json!([{"id": 4}]), 4), "null"); // missing key too
        assert_eq!(name_of(&ws, 99), ""); // no entry -> empty capture
    }

    #[test]
    fn displays_numbered_left_to_right() {
        let mons = json!([
            {"name": "DP-1",  "x": 2304, "y": 0, "focused": true,  "activeWorkspace": {"id": 11}},
            {"name": "eDP-1", "x": 0,    "y": 0, "focused": false, "activeWorkspace": {"id": 3}},
            {"name": "HDMI-A-1", "x": 500, "y": 0, "disabled": true},
        ]);
        let ds = displays_from(&mons);
        assert_eq!(
            ds.iter().map(|d| d.name.as_str()).collect::<Vec<_>>(),
            vec!["eDP-1", "DP-1"] // by x, disabled dropped
        );
        assert!(!ds[0].focused);
        assert!(ds[1].focused);
        assert_eq!(ds[0].active_ws, Some(3));
        assert_eq!(ds[1].active_ws, Some(11));
        // ties on (x, y) break by name, deterministically
        let tie = json!([{"name": "b", "x": 0, "y": 0}, {"name": "a", "x": 0, "y": 0}]);
        let ds = displays_from(&tie);
        assert_eq!(ds[0].name, "a");
        assert_eq!(displays_from(&json!("junk")).len(), 0);
    }

    #[test]
    fn display_dispatch_strings_match_verified_forms() {
        // Pinned byte-for-byte to the live-verified Lua forms (see each
        // constructor's doc comment for the verification story).
        assert_eq!(
            focus_monitor_cmd("DP-1"),
            r#"hl.dsp.focus({ monitor = "DP-1" })"#
        );
        assert_eq!(
            move_workspace_cmd(3, "DP-1"),
            r#"hl.dsp.workspace.move({ workspace = 3, monitor = "DP-1" })"#
        );
        // names ride through lua_escape like rename's
        assert_eq!(
            focus_monitor_cmd(r#"we"ird"#),
            r#"hl.dsp.focus({ monitor = "we\"ird" })"#
        );
    }

    #[test]
    fn monitor_map_and_restore_plan() {
        let mk = |pairs: &[(i64, &str)]| -> Vec<(i64, String)> {
            pairs.iter().map(|(i, m)| (*i, m.to_string())).collect()
        };
        let ws = json!([
            {"id": 3, "name": "three", "monitor": "DP-1"},
            {"id": 1, "name": "one", "monitor": "eDP-1"},
            {"id": -99, "name": "special:magic", "monitor": "DP-1"},
        ]);
        assert_eq!(monitor_map(&ws), mk(&[(1, "eDP-1"), (3, "DP-1")]));

        // strays go home; ids that appeared/vanished are left alone
        let before = mk(&[(1, "eDP-1"), (3, "DP-1"), (9, "DP-1")]);
        let after = mk(&[(1, "eDP-1"), (3, "eDP-1"), (12, "eDP-1")]);
        assert_eq!(
            restore_plan(&before, &after),
            vec![r#"hl.dsp.workspace.move({ workspace = 3, monitor = "DP-1" })"#.to_string()]
        );
        assert!(restore_plan(&before, &before).is_empty());
    }

    #[test]
    fn prefs_file_roundtrip_and_junk_tolerance() {
        let mut prefs = BTreeMap::new();
        prefs.insert(9, "DP-2".to_string());
        prefs.insert(11, "DP-2".to_string());
        prefs.insert(-3, "eDP-1".to_string());
        let text = serialize_prefs(&prefs);
        assert_eq!(text, "-3 eDP-1\n9 DP-2\n11 DP-2\n"); // sorted by id
        assert_eq!(parse_prefs(&text), prefs);
        // junk lines are skipped, never fatal; a duplicated id keeps the
        // LAST line (later = newer intent)
        assert_eq!(
            parse_prefs("9 DP-2\nnot a pref\n9 eDP-1\n12\n13 DP-1 extra\n\n"),
            BTreeMap::from([(9, "eDP-1".to_string())])
        );
        assert!(parse_prefs("").is_empty());
        assert!(serialize_prefs(&BTreeMap::new()).is_empty());
    }

    #[test]
    fn pref_io_missing_file_and_empty_write() {
        let dir = std::env::temp_dir().join(format!("bsctl-ws-pref-ut-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let path = dir.join("prefs");
        assert!(load_prefs_from(&path).is_empty()); // no file = no prefs
        let prefs = BTreeMap::from([(9, "DP-2".to_string())]);
        save_prefs_to(&path, &prefs); // creates the dir
        assert_eq!(load_prefs_from(&path), prefs);
        // prefs rm --all writes the EMPTY file; the file stays
        save_prefs_to(&path, &BTreeMap::new());
        assert_eq!(fs::read_to_string(&path).unwrap(), "");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn prefs_file_is_the_map_files_sibling() {
        // Can't mutate the env safely in tests; just check the name contract.
        assert_eq!(prefs_file().parent(), map_file().parent());
        assert!(prefs_file().ends_with("battlestation-workspaces/prefs"));
        assert!(map_file().ends_with("battlestation-workspaces/map"));
    }

    #[test]
    fn human_name_hides_the_default_number() {
        assert_eq!(human_name("work", 5), Some("work".to_string()));
        assert_eq!(human_name("5", 5), None); // Hyprland's unnamed default
        assert_eq!(human_name("null", 3), None); // name_of's null rendering
        assert_eq!(human_name("", 9), None);
        assert_eq!(human_name("7", 5), Some("7".to_string())); // a REAL name "7"
    }
}
