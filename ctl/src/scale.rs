//! `bsctl display scale` — step the focused monitor's scale up/down at
//! runtime, preserving its mode; a verb-for-verb port of display-scale.sh.
//! The rung-index state contract is in lib.rs ("Display scale"); dispatch
//! goes through `ipc::eval` because the Lua config parser makes `hyprctl
//! keyword` a silent no-op (AGENTS.md gotcha).
//!
//! Why a ladder with a persisted INDEX: Hyprland snaps fractional scales to
//! its own 1/120 grid, and the achievable values are irregular per panel —
//! the scale it reports back is NOT the scale that was requested, so
//! deterministic stepping needs the rung index remembered per monitor, not
//! the reported value. The reported scale only seeds the start rung
//! (nearest match) when no index is saved.

use std::env;
use std::fs;
use std::path::PathBuf;

use serde_json::Value;

use crate::ipc;
use crate::proto::round_half_even;
use crate::ws::lua_escape;

/// The scale ladder — rung 0 = native; tune freely (script parity).
pub const LADDER: [f64; 7] = [1.0, 1.25, 1.5, 1.75, 2.0, 2.5, 3.0];

pub enum Action {
    Up,
    Down,
    Reset,
    /// An explicit scale (`--value`). Hyprland snaps whatever we send to its
    /// own 1/120 grid, so the applied value may differ slightly; the saved
    /// rung index becomes the NEAREST rung so later up/down steps stay
    /// deterministic from here.
    Value(f64),
}

// ---- pure logic (proto.rs-style: deterministic, unit-tested) ---------------

/// One monitor's dispatch-relevant state, lifted from a `j/monitors` array
/// (the ENABLED-only view — the script's `hyprctl monitors -j`).
pub struct Focused {
    pub name: String,
    pub mode: String,     // WxH@RR, RR python-rounded — preserved by the dispatch
    pub position: String, // "XxY" — preserved by the dispatch (see eval_cmd)
    pub scale: f64,
}

/// Lift one monitor entry. An entry with missing/mistyped fields is None —
/// the reference python raises there, which lands on the silent-exit-0 path.
fn lift(m: &Value) -> Option<Focused> {
    Some(Focused {
        name: m.get("name")?.as_str()?.to_string(),
        mode: format!(
            "{}x{}@{}",
            num_token(m.get("width"))?,
            num_token(m.get("height"))?,
            round_half_even(m.get("refreshRate")?.as_f64()?),
        ),
        position: format!("{}x{}", num_token(m.get("x"))?, num_token(m.get("y"))?),
        scale: m.get("scale")?.as_f64()?,
    })
}

/// First entry with `focused == true`, or None (script parity: python's
/// loop prints nothing, the sh side exits 0 silently).
pub fn focused_from(monitors: &Value) -> Option<Focused> {
    let m = monitors.as_array()?.iter().find(|m| {
        // Hyprland emits a JSON bool; python's truthiness check reduces to it.
        m.get("focused").and_then(Value::as_bool).unwrap_or(false)
    })?;
    lift(m)
}

/// The named entry, or None. Same silent-tolerance as [`focused_from`] for
/// malformed entries; the CALLER validates the name against the display
/// numbering first (so an unknown name errors loudly there, not here).
pub fn named_from(monitors: &Value, name: &str) -> Option<Focused> {
    let m = monitors
        .as_array()?
        .iter()
        .find(|m| m.get("name").and_then(Value::as_str) == Some(name))?;
    lift(m)
}

/// A raw JSON number as python `print` renders it — ints stay bare
/// ("2880"), floats keep their shortest repr. Hyprland emits ints for
/// width/height; passing the token through beats casting.
fn num_token(v: Option<&Value>) -> Option<String> {
    match v? {
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

/// Nearest ladder index to a reported scale — python
/// `min(range(len(ladder)), key=lambda i: abs(ladder[i] - scale))`: min is
/// stable, so ties break toward the LOWER rung.
pub fn nearest_rung(scale: f64) -> usize {
    let mut best = 0;
    for (i, rung) in LADDER.iter().enumerate().skip(1) {
        if (rung - scale).abs() < (LADDER[best] - scale).abs() {
            best = i;
        }
    }
    best
}

/// One step from the saved index (or the nearest rung when none), clamped to
/// the ladder — the python `idx += ...; idx = max(0, min(len - 1, idx))`.
/// A saved index is NOT pre-clamped: 99 + up clamps to the top rung, exactly
/// like the reference.
pub fn stepped(saved: Option<i64>, scale: f64, delta: i64) -> usize {
    let start = saved.unwrap_or(nearest_rung(scale) as i64);
    (start + delta).clamp(0, LADDER.len() as i64 - 1) as usize
}

/// State-file parse. The sh reference reads via command substitution
/// (trailing newlines stripped): "" -> no saved index (seed from the
/// reported scale), otherwise python `int()` — whitespace-tolerant, signs
/// ok, anything else raises (-> [`Saved::Invalid`], an error here as there).
pub enum Saved {
    None,
    Idx(i64),
    Invalid,
}

pub fn parse_saved(content: Option<&str>) -> Saved {
    let Some(c) = content else { return Saved::None };
    let c = c.trim_end_matches('\n');
    if c.is_empty() {
        return Saved::None;
    }
    match c.trim().parse::<i64>() {
        Ok(i) => Saved::Idx(i),
        Err(_) => Saved::Invalid,
    }
}

/// A ladder value as the reference prints it into the Lua command —
/// python `f"{x:.5f}"` ("1.25000"); the dispatch strings stay byte-identical.
pub fn format_scale(v: f64) -> String {
    format!("{v:.5}")
}

/// The eval chunk: mode AND position preserved, scale a bare Lua number
/// ("1.25000") for ladder rungs or the quoted Lua STRING `"auto"` for reset —
/// the quoting is the semantic.
///
/// DELIBERATE DIVERGENCE from the sh reference: the script sent
/// `position = "auto"`, written in the single-monitor era where auto was
/// harmless. With a second monitor attached, re-declaring an output with
/// auto position makes Hyprland RE-PLACE it (appending to the right of the
/// others) and migrate its workspaces — a scale keystroke rearranged the
/// desk (seen live 2026-07-03). Pinning the monitor's current "XxY" keeps
/// scaling a scale-only operation. (Adjacent auto-positioned monitors may
/// still re-flow because the logical size changed — that part is inherent.)
pub fn eval_cmd(name: &str, mode: &str, position: &str, luascale: &str) -> String {
    format!(
        r#"hl.monitor({{ output = "{}", mode = "{}", position = "{}", scale = {} }})"#,
        lua_escape(name),
        mode,
        position,
        luascale
    )
}

/// A monitor's logical right edge: `x + width/scale`.
pub fn logical_right(mons: &Value, name: &str) -> Option<f64> {
    let m = mons
        .as_array()?
        .iter()
        .find(|m| m.get("name").and_then(Value::as_str) == Some(name))?;
    let x = m.get("x")?.as_f64()?;
    let w = m.get("width")?.as_f64()?;
    let s = m.get("scale")?.as_f64()?;
    (s > 0.0).then(|| x + w / s)
}

/// Repositioning evals for the monitors to the RIGHT of a rescaled one.
/// A logical-width change leaves a pointer-blocking gap (or an overlap)
/// between the scaled monitor and anything positioned past its old right
/// edge — seen live as "the mouse can't cross monitors" after a scale reset
/// landed on a different auto scale. Each right-neighbor's DESIRED position
/// preserves its pre-scale offset from the scaled monitor's right edge:
/// `new_right + (pre_x - old_right)` — computed from the PRE-eval snapshot,
/// and dispatched only where post-eval reality disagrees. Absolute targets,
/// not a shift: Hyprland re-flows auto-positioned neighbors ITSELF on some
/// runtime changes but not others (both observed live), and a relative delta
/// double-applies in the first case. X axis only; vertical arrangements are
/// untouched.
pub fn reflow_plan(
    pre_mons: &Value,
    post_mons: &Value,
    scaled: &str,
    old_right: f64,
    new_right: f64,
) -> Vec<String> {
    let (Some(pre), Some(post)) = (pre_mons.as_array(), post_mons.as_array()) else {
        return Vec::new();
    };
    pre.iter()
        .filter(|m| m.get("name").and_then(Value::as_str) != Some(scaled))
        .filter(|m| !m.get("disabled").and_then(Value::as_bool).unwrap_or(false))
        .filter_map(|m| {
            let name = m.get("name")?.as_str()?;
            let pre_x = m.get("x")?.as_f64()?;
            if pre_x < old_right - 0.5 {
                return None; // left of (or under) the scaled monitor: untouched
            }
            let desired_x = (new_right + (pre_x - old_right)).round() as i64;
            // Re-declare from POST-eval reality (mode/scale/y may have
            // settled); skip when Hyprland already put it where we want it.
            let p = post
                .iter()
                .find(|p| p.get("name").and_then(Value::as_str) == Some(name))?;
            let post_x = p.get("x")?.as_f64()?;
            if (post_x - desired_x as f64).abs() <= 0.5 {
                return None;
            }
            let mode = format!(
                "{}x{}@{}",
                num_token(p.get("width"))?,
                num_token(p.get("height"))?,
                round_half_even(p.get("refreshRate")?.as_f64()?),
            );
            let pos = format!("{}x{}", desired_x, p.get("y")?.as_f64()?.round() as i64);
            let scale = format_scale(p.get("scale")?.as_f64()?);
            Some(eval_cmd(name, &mode, &pos, &scale))
        })
        .collect()
}

/// `${XDG_RUNTIME_DIR:-/tmp}/hypr-display-scale.<name>` (empty env counts
/// as unset, like the sh `:-` default).
pub fn state_path(name: &str) -> PathBuf {
    env::var_os("XDG_RUNTIME_DIR")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join(format!("hypr-display-scale.{name}"))
}

// ---- command ----------------------------------------------------------------

/// `bsctl display set scale [selector] (--up|--down|--reset|--value <v>)`.
/// No selector targets the FOCUSED monitor (what the zoom keybinds mean).
/// Ordering matches the script: state first (write the new index / rm the
/// file), dispatch last — so the exit code is the dispatch's, and a
/// rejected dispatch still leaves the stepped index persisted (script
/// parity).
pub fn run(action: Action, sel: Option<&crate::ws::DisplaySel>) -> i32 {
    let Some(mons) = ipc::json("monitors") else {
        eprintln!("bsctl display scale: monitors query failed (socket and hyprctl)");
        return 1;
    };
    let f = match sel {
        None => match focused_from(&mons) {
            Some(f) => f,
            None => return 0, // no focused monitor: silent no-op (script parity)
        },
        Some(sel) => {
            // Validate against the display numbering first so unknown
            // ids/names error loudly with the valid list.
            let ds = crate::ws::displays_from(&mons);
            let i = match crate::ws::resolve_display_sel("set scale", sel, &ds) {
                Ok(i) => i,
                Err(c) => return c,
            };
            match named_from(&mons, &ds[i].name) {
                Some(f) => f,
                None => return 0, // malformed entry: the silent no-op path
            }
        }
    };
    let state = state_path(&f.name);
    let luascale = match action {
        Action::Reset => {
            let _ = fs::remove_file(&state); // rm -f
            r#""auto""#.to_string()
        }
        Action::Value(v) => {
            if !(v.is_finite() && v > 0.0) {
                eprintln!("bsctl display set scale: --value must be a positive scale (got {v})");
                return 1;
            }
            // Persist the NEAREST rung so later --up/--down step
            // deterministically from wherever --value landed.
            if let Err(e) = fs::write(&state, format!("{}\n", nearest_rung(v))) {
                eprintln!("bsctl display set scale: {}: {e}", state.display());
                return 1;
            }
            format_scale(v)
        }
        Action::Up | Action::Down => {
            let delta = if matches!(action, Action::Up) { 1 } else { -1 };
            // cat failure of any kind reads as "no saved index" (the
            // script's `cat ... 2>/dev/null || echo ""`).
            let saved = match parse_saved(fs::read_to_string(&state).ok().as_deref()) {
                Saved::None => None,
                Saved::Idx(i) => Some(i),
                Saved::Invalid => {
                    eprintln!(
                        "bsctl display scale: unreadable rung index in {} — remove it (or run: bsctl display scale reset)",
                        state.display()
                    );
                    return 1; // the reference dies on int(garbage) too
                }
            };
            let idx = stepped(saved, f.scale, delta);
            // `echo "$1" > "$state"` — index + newline, byte-compatible.
            if let Err(e) = fs::write(&state, format!("{idx}\n")) {
                eprintln!("bsctl display scale: {}: {e}", state.display());
                return 1;
            }
            format_scale(LADDER[idx])
        }
    };
    // Snapshot workspace placement + what each display shows BEFORE the eval:
    // a scale change re-flows auto-positioned neighbors, and a re-applied
    // monitor gets its workspaces evacuated to the focused display with no
    // return path (seen live: scaling the laptop pulled every external
    // workspace onto it). The pinned position above protects the scaled
    // monitor itself; the restore below protects its neighbors.
    let before_ws = ipc::json("workspaces").map(|w| crate::ws::monitor_map(&w));
    let before_actives: Vec<(String, Option<i64>)> = crate::ws::displays_from(&mons)
        .iter()
        .map(|d| (d.name.clone(), d.active_ws))
        .collect();
    let old_right = logical_right(&mons, &f.name);

    let code = ipc::eval(&eval_cmd(&f.name, &f.mode, &f.position, &luascale));
    if code != 0 {
        return code;
    }

    // Close the gap the scale just opened: re-read reality (works for
    // "auto" too — we can't predict what scale it resolves to) and put
    // right-hand neighbors at their desired absolute positions. The
    // neighbor re-declarations may evacuate THEIR workspaces;
    // restore_strays below catches that too.
    std::thread::sleep(std::time::Duration::from_millis(200));
    if let (Some(old), Some(after_mons)) = (old_right, ipc::json("monitors"))
        && let Some(new) = logical_right(&after_mons, &f.name)
    {
        for cmd in reflow_plan(&mons, &after_mons, &f.name, old, new) {
            let _ = ipc::eval(&cmd);
        }
    }

    if let Some(before) = before_ws {
        crate::ws::restore_strays(&before, &before_actives, &f.name);
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// The live `j/monitors` (enabled-only) shape: DP-1 focused at native.
    fn mons_fixture() -> Value {
        json!([
            {"id": 1, "name": "DP-1", "description": "HP Inc. OMEN 34c CNC32023FL",
             "focused": true, "disabled": false, "width": 3440, "height": 1440,
             "x": 2304, "y": 0, "refreshRate": 100.0, "scale": 1.0},
        ])
    }

    #[test]
    fn focused_extraction_and_mode_rounding() {
        let f = focused_from(&mons_fixture()).unwrap();
        assert_eq!(f.name, "DP-1");
        assert_eq!(f.mode, "3440x1440@100");
        assert_eq!(f.position, "2304x0");
        assert_eq!(f.scale, 1.0);
        // the internal panel's live 120.001 rounds like python round()
        let f = focused_from(&json!([
            {"name": "eDP-1", "focused": true, "width": 2880, "height": 1800,
             "x": 0, "y": 0, "refreshRate": 120.001, "scale": 1.25}
        ]))
        .unwrap();
        assert_eq!(f.mode, "2880x1800@120");
        assert_eq!(f.position, "0x0");
        // half-rates go to even (python banker's rounding)
        let f = focused_from(&json!([
            {"name": "X", "focused": true, "width": 1, "height": 1,
             "x": 0, "y": 0, "refreshRate": 59.5, "scale": 1.0}
        ]))
        .unwrap();
        assert_eq!(f.mode, "1x1@60");
        // missing x/y = malformed monitors data -> None (silent no-op path)
        assert!(
            focused_from(&json!([
                {"name": "X", "focused": true, "width": 1, "height": 1,
                 "refreshRate": 60.0, "scale": 1.0}
            ]))
            .is_none()
        );
    }

    #[test]
    fn focused_missing_or_malformed_is_none() {
        // no focused entry (the script's silent-exit-0 path)
        assert!(focused_from(&json!([{"name": "DP-1", "focused": false}])).is_none());
        assert!(focused_from(&json!([])).is_none());
        assert!(focused_from(&json!("nope")).is_none());
        // focused but a required field missing/mistyped: python raises ->
        // same silent path
        assert!(focused_from(&json!([{"name": "DP-1", "focused": true}])).is_none());
        assert!(
            focused_from(&json!([
                {"name": "DP-1", "focused": true, "width": "3440", "height": 1440,
                 "refreshRate": 100.0, "scale": 1.0}
            ]))
            .is_none()
        );
    }

    #[test]
    fn nearest_rung_snaps_and_ties_break_low() {
        assert_eq!(nearest_rung(1.0), 0);
        assert_eq!(nearest_rung(3.0), 6);
        // Hyprland's 1/120-grid snap reports values off the ladder
        assert_eq!(nearest_rung(1.241_666), 1); // snapped "1.25"
        assert_eq!(nearest_rung(1.733_333), 3); // snapped "1.75"
        assert_eq!(nearest_rung(0.5), 0); // below the ladder
        assert_eq!(nearest_rung(9.9), 6); // above the ladder
        // exact midpoints: python min() keeps the FIRST minimum (lower rung)
        assert_eq!(nearest_rung(1.125), 0);
        assert_eq!(nearest_rung(2.75), 5);
    }

    #[test]
    fn stepped_prefers_saved_and_clamps() {
        // no saved index: seed from the reported scale
        assert_eq!(stepped(None, 1.0, 1), 1);
        assert_eq!(stepped(None, 1.0, -1), 0); // bottom clamp
        assert_eq!(stepped(None, 3.0, 1), 6); // top clamp
        // saved index wins over a disagreeing reported scale
        assert_eq!(stepped(Some(3), 1.0, 1), 4);
        assert_eq!(stepped(Some(3), 1.0, -1), 2);
        // saved is not pre-clamped; the result is
        assert_eq!(stepped(Some(99), 1.0, 1), 6);
        assert_eq!(stepped(Some(-5), 3.0, -1), 0);
    }

    #[test]
    fn saved_parse_mirrors_the_sh_and_python() {
        assert!(matches!(parse_saved(None), Saved::None)); // no file
        assert!(matches!(parse_saved(Some("")), Saved::None));
        assert!(matches!(parse_saved(Some("\n")), Saved::None)); // echo ""
        assert!(matches!(parse_saved(Some("3\n")), Saved::Idx(3)));
        assert!(matches!(parse_saved(Some("3")), Saved::Idx(3)));
        assert!(matches!(parse_saved(Some(" 3 \n")), Saved::Idx(3))); // int() strips
        assert!(matches!(parse_saved(Some("+2\n")), Saved::Idx(2)));
        assert!(matches!(parse_saved(Some("-1\n")), Saved::Idx(-1)));
        assert!(matches!(parse_saved(Some("junk\n")), Saved::Invalid));
        assert!(matches!(parse_saved(Some("1.5\n")), Saved::Invalid)); // int("1.5") raises
        assert!(matches!(parse_saved(Some(" \n")), Saved::Invalid)); // int(" ") raises
    }

    #[test]
    fn scale_formatting_is_pythons_5f() {
        let formatted: Vec<String> = LADDER.iter().map(|&v| format_scale(v)).collect();
        assert_eq!(
            formatted,
            [
                "1.00000", "1.25000", "1.50000", "1.75000", "2.00000", "2.50000", "3.00000"
            ]
        );
    }

    #[test]
    fn eval_chunk_pins_mode_and_position() {
        // Position is the monitor's CURRENT "XxY", never "auto" — the one
        // deliberate divergence from the sh reference (see eval_cmd's doc).
        assert_eq!(
            eval_cmd("DP-1", "3440x1440@100", "2304x0", "1.25000"),
            r#"hl.monitor({ output = "DP-1", mode = "3440x1440@100", position = "2304x0", scale = 1.25000 })"#
        );
        // reset: the quoted Lua STRING "auto" for the SCALE, not a bare word
        assert_eq!(
            eval_cmd("DP-1", "3440x1440@100", "0x0", r#""auto""#),
            r#"hl.monitor({ output = "DP-1", mode = "3440x1440@100", position = "0x0", scale = "auto" })"#
        );
        // hostile names can't break out of the Lua string
        assert_eq!(
            eval_cmd(r#"x"break"#, "1x1@1", "0x0", "1.00000"),
            r#"hl.monitor({ output = "x\"break", mode = "1x1@1", position = "0x0", scale = 1.00000 })"#
        );
    }

    #[test]
    fn reflow_targets_absolute_positions() {
        let pre = json!([
            {"name": "eDP-1", "focused": true, "disabled": false, "x": 0, "y": 0,
             "width": 2880, "height": 1800, "refreshRate": 120.0, "scale": 1.25},
            {"name": "DP-1", "focused": false, "disabled": false, "x": 2304, "y": 0,
             "width": 3440, "height": 1440, "refreshRate": 100.0, "scale": 1.0},
            {"name": "LEFT-1", "focused": false, "disabled": false, "x": -1920, "y": 0,
             "width": 1920, "height": 1080, "refreshRate": 60.0, "scale": 1.0},
            {"name": "DEAD-1", "focused": false, "disabled": true, "x": 9000, "y": 0,
             "width": 1920, "height": 1080, "refreshRate": 60.0, "scale": 1.0},
        ]);
        // eDP-1 rescaled 1.25 -> 1.8: right edge 2304 -> 1600. Hyprland did
        // NOT move DP-1 (still at its pre position): plan puts it at 1600.
        let post_unmoved = {
            let mut v = pre.clone();
            v[0]["scale"] = json!(1.8);
            v
        };
        assert_eq!(logical_right(&post_unmoved, "eDP-1"), Some(1600.0));
        assert_eq!(
            reflow_plan(&pre, &post_unmoved, "eDP-1", 2304.0, 1600.0),
            vec![
                r#"hl.monitor({ output = "DP-1", mode = "3440x1440@100", position = "1600x0", scale = 1.00000 })"#.to_string()
            ]
        );
        // Hyprland ALREADY re-flowed DP-1 to 1600 itself (both behaviors
        // observed live): desired == reality -> no dispatch, no double-shift.
        let mut post_moved = post_unmoved.clone();
        post_moved[1]["x"] = json!(1600);
        assert!(reflow_plan(&pre, &post_moved, "eDP-1", 2304.0, 1600.0).is_empty());
        // A pre-existing deliberate gap is preserved, not closed.
        let mut pre_gap = pre.clone();
        pre_gap[1]["x"] = json!(2504); // 200 past the old right edge
        let mut post_gap = pre_gap.clone();
        post_gap[0]["scale"] = json!(1.8);
        assert_eq!(
            reflow_plan(&pre_gap, &post_gap, "eDP-1", 2304.0, 1600.0),
            vec![
                r#"hl.monitor({ output = "DP-1", mode = "3440x1440@100", position = "1800x0", scale = 1.00000 })"#.to_string()
            ]
        );
        // No change in width -> desired == pre == post -> empty plan.
        assert!(reflow_plan(&pre, &pre, "eDP-1", 2304.0, 2304.0).is_empty());
    }

    #[test]
    fn state_path_shape() {
        // Can't mutate the env safely in tests; just check the name contract.
        assert!(
            state_path("DP-1")
                .to_string_lossy()
                .ends_with("/hypr-display-scale.DP-1")
        );
    }
}
