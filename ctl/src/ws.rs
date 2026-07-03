//! `bsctl ws` — workspace display-order commands, mirroring ws.sh
//! verb-for-verb. The order-file protocol and the display-position model are
//! specified in lib.rs ("Workspace display order"); this module keeps the
//! script's observable behavior exactly: same dispatch strings (Hyprland's
//! Lua parser is picky — these are the known-good forms), same file bytes,
//! same exit codes (usage errors 2 via clap; position-off-the-end 1, the sh
//! `[ -n "$id" ] && dispatch` leftover status).

use std::env;
use std::fs;
use std::io::Write;
use std::path::PathBuf;

use serde_json::Value;

use crate::sys;

/// `${XDG_STATE_HOME:-$HOME/.local/state}/claude-workspaces/order` (an empty
/// env var counts as unset, like the sh `:-` default).
pub fn order_file() -> PathBuf {
    env::var_os("XDG_STATE_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env::var_os("HOME").unwrap_or_default()).join(".local/state")
        })
        .join("claude-workspaces/order")
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

// ---- commands ---------------------------------------------------------------

/// Preference file content; missing file reads as empty (identity order).
fn read_pref() -> String {
    fs::read_to_string(order_file()).unwrap_or_default()
}

/// `hyprctl workspaces -j`, or the script's set -e death when hyprctl/JSON
/// fails: report and exit 1.
fn workspaces_json() -> Result<Value, i32> {
    sys::hyprctl_json(&["workspaces", "-j"]).ok_or_else(|| {
        eprintln!("bsctl ws: hyprctl workspaces -j failed");
        1
    })
}

pub fn goto(pos: usize) -> i32 {
    let ws = match workspaces_json() {
        Ok(v) => v,
        Err(c) => return c,
    };
    match resolve(&read_pref(), &live_ids(&ws)).get(pos - 1) {
        Some(id) => sys::hyprctl_dispatch(&focus_cmd(*id)),
        None => 1, // position off the end: no dispatch, exit 1 (script parity)
    }
}

pub fn movewindow(pos: usize, follow: bool) -> i32 {
    let ws = match workspaces_json() {
        Ok(v) => v,
        Err(c) => return c,
    };
    match resolve(&read_pref(), &live_ids(&ws)).get(pos - 1) {
        Some(id) => sys::hyprctl_dispatch(&move_cmd(*id, follow)),
        None => 1,
    }
}

pub fn relative(delta: i64, mov: bool) -> i32 {
    let ws = match workspaces_json() {
        Ok(v) => v,
        Err(c) => return c,
    };
    let resolved = resolve(&read_pref(), &live_ids(&ws));
    if resolved.is_empty() {
        return 0;
    }
    // Current position of `hyprctl activeworkspace -j`'s id in the resolved
    // order; a workspace outside the order (e.g. special) defaults to 1.
    let Some(active) = sys::hyprctl_json(&["activeworkspace", "-j"]) else {
        eprintln!("bsctl ws: hyprctl activeworkspace -j failed");
        return 1;
    };
    let pos = active
        .get("id")
        .and_then(Value::as_i64)
        .and_then(|cur| resolved.iter().position(|&id| id == cur))
        .map(|i| i + 1);
    let id = resolved[step(pos, delta, resolved.len()) - 1];
    let cmd = if mov {
        move_cmd(id, true)
    } else {
        focus_cmd(id)
    };
    sys::hyprctl_dispatch(&cmd)
}

pub fn set(ids: &[i64]) -> i32 {
    let p = order_file();
    if let Some(d) = p.parent() {
        let _ = fs::create_dir_all(d);
    }
    // `printf '%s\n' "$*"`: ids space-joined + trailing newline. The bar
    // plugin FileView-watches and parses this exact format — keep the bytes.
    let body = ids.iter().map(i64::to_string).collect::<Vec<_>>().join(" ") + "\n";
    match fs::write(&p, body) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("bsctl ws set: {}: {e}", p.display());
            1
        }
    }
}

pub fn reset() -> i32 {
    // Empty (not delete) the preference so resolve() falls back to identity
    // order. Truncating IN PLACE is what makes the bar plugin's FileView
    // watch fire, so the pills re-render to 1,2,3,... immediately.
    let p = order_file();
    if let Some(d) = p.parent() {
        let _ = fs::create_dir_all(d);
    }
    match fs::write(&p, "") {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("bsctl ws reset: {}: {e}", p.display());
            1
        }
    }
}

pub fn get() -> i32 {
    // `cat "$ORDER_FILE" 2>/dev/null || true` — raw bytes, silent if missing.
    if let Ok(b) = fs::read(order_file()) {
        let _ = std::io::stdout().write_all(&b);
    }
    0
}

pub fn rename(id: i64, name: Option<&str>) -> i32 {
    // Hyprland can't renumber an id; this only changes the display name. The
    // Lua config parser rejects `hyprctl dispatch renameworkspace`, so the
    // rename goes through hl.dsp.workspace.rename (same as focus/move).
    sys::hyprctl_dispatch(&rename_cmd(id, name))
}

pub fn order() -> i32 {
    let ws = match workspaces_json() {
        Ok(v) => v,
        Err(c) => return c,
    };
    for (i, id) in resolve(&read_pref(), &live_ids(&ws)).iter().enumerate() {
        println!("{} -> ws {} ({})", i + 1, id, name_of(&ws, *id));
    }
    0
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
}
