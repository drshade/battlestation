//! `bsctl display` — display state visibility & safe dpms management, the
//! human/diagnostic face of the machine's roughest edge. The contract (and
//! the toggle-only dpms semantics everything here is built around) is in
//! lib.rs ("Display management"); the shell references are displays-on.sh
//! (read-before-toggle + the reset flow) and clamshell.sh (internal-panel
//! detection, lid policy). Per AGENTS.md's recovery boundary those scripts
//! stay authoritative for the AUTOMATED paths (hypridle's after_sleep_cmd,
//! the lid binds) — this module reimplements their logic for humans, it does
//! not replace them.

use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

use serde_json::{Value, json};

use crate::ws::{self, DisplaySel, lua_escape};
use crate::{ipc, proto};

// ---- pure logic (proto.rs-style: deterministic, unit-tested) ---------------

/// One output's table-relevant state, lifted out of a `j/monitors all`
/// entry. Field spellings are the live-verified ones (Hyprland 0.55.4):
/// `disabled` (inverted to `enabled` here), `dpmsStatus`, `width`/`height`/
/// `refreshRate`, `x`/`y`, `scale`, `description`.
pub struct Output {
    pub name: String,
    pub enabled: bool,
    pub dpms: bool,
    pub mode: String, // WxH@Hz
    pub scale: String,
    pub position: String, // XxY
    pub description: String,
}

/// Rows from a `j/monitors all` array, in compositor order.
pub fn outputs_from(monitors: &Value) -> Vec<Output> {
    let Some(arr) = monitors.as_array() else {
        return Vec::new();
    };
    arr.iter()
        .map(|m| {
            let s = |k: &str| {
                m.get(k)
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string()
            };
            let f = |k: &str| m.get(k).and_then(Value::as_f64).unwrap_or(0.0);
            Output {
                name: s("name"),
                enabled: !m.get("disabled").and_then(Value::as_bool).unwrap_or(false),
                dpms: m
                    .get("dpmsStatus")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
                mode: format!(
                    "{}x{}@{}",
                    f("width") as i64,
                    f("height") as i64,
                    trim_num(f("refreshRate"))
                ),
                scale: trim_num(f("scale")),
                position: format!("{}x{}", f("x") as i64, f("y") as i64),
                description: s("description"),
            }
        })
        .collect()
}

/// Two decimals, trailing zeros dropped: 1.00 -> "1", 100.001 -> "100",
/// 1.25 -> "1.25". Enough for refresh rates and 1/120-grid scales.
pub fn trim_num(v: f64) -> String {
    let s = format!("{v:.2}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// Char-safe truncation with a `…` suffix — descriptions are EDID strings of
/// arbitrary length and the table caps them so line width stays sane.
pub fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let cut: String = s.chars().take(max.saturating_sub(1)).collect();
    format!("{}…", cut.trim_end())
}

/// clamshell.sh's connector-class rule: eDP/LVDS/DSI-prefixed connectors are
/// built-in panels, everything else (DP/HDMI/...) is external. Needs no
/// per-machine output name and is inert on desktops.
pub fn is_internal(name: &str) -> bool {
    ["eDP", "LVDS", "DSI"].iter().any(|p| name.starts_with(p))
}

/// The aligned status table (header + one row per output; description last
/// so its 48-char truncation caps the line). `ids` maps output name ->
/// display-id; a disabled output has no number (ids only count what's
/// plugged in and enabled) and shows `-`.
pub fn format_table(outs: &[Output], ids: &[(String, usize)]) -> String {
    const HDR: [&str; 8] = [
        "ID",
        "OUTPUT",
        "ENABLED",
        "DPMS",
        "MODE",
        "SCALE",
        "POSITION",
        "DESCRIPTION",
    ];
    let rows: Vec<Vec<String>> = outs
        .iter()
        .map(|o| {
            vec![
                ids.iter()
                    .find(|(n, _)| *n == o.name)
                    .map(|(_, i)| i.to_string())
                    .unwrap_or_else(|| "-".to_string()),
                o.name.clone(),
                proto::yes_no(o.enabled),
                (if o.dpms { "on" } else { "off" }).to_string(),
                o.mode.clone(),
                o.scale.clone(),
                o.position.clone(),
                truncate(&o.description, 48),
            ]
        })
        .collect();
    proto::render_table(&HDR, &rows)
}

/// Consistency warnings, each carrying its remedy. `lid_closed` is None on
/// desktops (no lid device). Exit status is unaffected — status is a report.
pub fn warnings(outs: &[Output], lid_closed: Option<bool>) -> Vec<String> {
    let mut w = Vec::new();
    if lid_closed == Some(true) {
        for o in outs.iter().filter(|o| o.enabled && is_internal(&o.name)) {
            w.push(format!(
                "WARNING: internal panel {} is enabled while the lid is closed — run: bsctl display reset (or clamshell.sh on)",
                o.name
            ));
        }
    }
    let enabled: Vec<&Output> = outs.iter().filter(|o| o.enabled).collect();
    if enabled.iter().any(|o| o.dpms) && enabled.iter().any(|o| !o.dpms) {
        w.push(
            "WARNING: dpms state is mixed across enabled outputs — run: bsctl display reset (or displays-on.sh)"
                .to_string(),
        );
    }
    for o in enabled.iter().filter(|o| !o.dpms) {
        w.push(format!(
            "WARNING: {} is enabled but dpms is off — run: bsctl display set dpms --display-name {} --on (or displays-on.sh)",
            o.name, o.name
        ));
    }
    w
}

/// on/off decision for one output. The dpms dispatcher is TOGGLE-ONLY
/// (AGENTS.md gotcha) — neither wire form can "set on" — so safe targeting
/// is read-before-toggle: toggle only when the current state differs from
/// the desired one. A disabled output is refused outright: its dpmsStatus is
/// meaningless (a lid-disabled panel reports dpms ON — verified live) and
/// dpms/eval calls no-op on disabled outputs, so acting on the reading could
/// only mislead.
pub enum Dpms {
    AlreadyThere,
    Toggle,
    Disabled,
}

pub fn dpms_decision(enabled: bool, dpms: bool, want_on: bool) -> Dpms {
    if !enabled {
        Dpms::Disabled
    } else if dpms == want_on {
        Dpms::AlreadyThere
    } else {
        Dpms::Toggle
    }
}

/// The TABLE-FORM dpms toggle — targets exactly one monitor. Never emit the
/// string form (`hl.dsp.dpms("on")`): it toggles EVERY monitor at once.
pub fn dpms_toggle_cmd(name: &str) -> String {
    format!(r#"hl.dsp.dpms({{ monitor = "{}" }})"#, lua_escape(name))
}

/// Pure half of the lid probe: the state-file contents under
/// `/proc/acpi/button/lid/*/state`. No lid device at all -> None (desktop;
/// the status report omits its lid line); any device reading "closed" counts
/// as closed (clamshell.sh's `grep -q closed`).
pub fn lid_from_states(states: &[String]) -> Option<bool> {
    if states.is_empty() {
        None
    } else {
        Some(states.iter().any(|s| s.contains("closed")))
    }
}

// ---- OS boundaries -----------------------------------------------------------

/// Lid state via /proc. `BSCTL_LID_DIR` overrides the base dir — the same
/// redirect seam PATH gives the fake-hyprctl tests, since /proc/acpi cannot
/// be faked any other way.
fn lid_closed() -> Option<bool> {
    let dir = env::var_os("BSCTL_LID_DIR")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/proc/acpi/button/lid"));
    let states: Vec<String> = fs::read_dir(dir)
        .ok()?
        .flatten()
        .filter_map(|e| fs::read_to_string(e.path().join("state")).ok())
        .collect();
    lid_from_states(&states)
}

/// `j/monitors all` — `all` so disabled outputs are still listed (the whole
/// point of the status view, and what lets `on`/`off` refuse them).
fn monitors_all() -> Result<Value, i32> {
    ipc::json("monitors all").ok_or_else(|| {
        eprintln!("bsctl display: monitors query failed (socket and hyprctl)");
        1
    })
}

/// `${XDG_CONFIG_HOME:-$HOME/.config}` (empty counts as unset).
fn config_home() -> PathBuf {
    env::var_os("XDG_CONFIG_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env::var_os("HOME").unwrap_or_default()).join(".config"))
}

// ---- commands ---------------------------------------------------------------

/// The display-id numbering over a `monitors all` array: enabled outputs
/// by (x, y), name -> 1-based id (the same numbering `ws` uses — one remap,
/// two faces).
fn id_map(mons: &Value) -> Vec<(String, usize)> {
    ws::displays_from(mons)
        .into_iter()
        .enumerate()
        .map(|(i, d)| (d.name, i + 1))
        .collect()
}

/// Resolve a display selector against the FULL output set (`monitors all`):
/// ids number the enabled outputs (the shared remap), names may also hit a
/// DISABLED output — `set dpms` wants that so its is-disabled refusal (with
/// remedy) can fire instead of a know-nothing "unknown output".
fn resolve_output<'o>(
    verb: &str,
    sel: &DisplaySel,
    outs: &'o [Output],
    ids: &[(String, usize)],
) -> Result<&'o Output, i32> {
    let name = match sel {
        DisplaySel::Name(n) => n.clone(),
        DisplaySel::Id(n) => match ids.iter().find(|(_, i)| i == n) {
            Some((name, _)) => name.clone(),
            None => {
                let list = ids
                    .iter()
                    .map(|(name, i)| format!("{i} = {name}"))
                    .collect::<Vec<_>>()
                    .join(", ");
                eprintln!("bsctl display {verb}: no display {n} (displays: {list})");
                return Err(1);
            }
        },
    };
    outs.iter().find(|o| o.name == name).ok_or_else(|| {
        let names: Vec<&str> = outs.iter().map(|o| o.name.as_str()).collect();
        eprintln!(
            "bsctl display {verb}: unknown output {name} (valid: {})",
            names.join(", ")
        );
        1
    })
}

/// One `display get --format json` result as its JSON line — the raw
/// structured form: the unmodified monitor objects (narrowed by the
/// selector) plus the derived lid + warnings. Shared by the one-shot json
/// path and its `--stream` form.
pub fn get_json(sel: Option<&DisplaySel>) -> Result<String, i32> {
    let mons = monitors_all()?;
    let outs = outputs_from(&mons);
    let ids = id_map(&mons);
    let only = match sel {
        Some(sel) => Some(resolve_output("get", sel, &outs, &ids)?.name.clone()),
        None => None,
    };
    let lid = lid_closed();
    let warns = warnings(&outs, lid);
    let lid_v = match lid {
        Some(true) => json!("closed"),
        Some(false) => json!("open"),
        None => Value::Null,
    };
    let mons_v = match &only {
        None => mons,
        Some(name) => Value::Array(
            mons.as_array()
                .map(|a| {
                    a.iter()
                        .filter(|m| m.get("name").and_then(Value::as_str) == Some(name))
                        .cloned()
                        .collect()
                })
                .unwrap_or_default(),
        ),
    };
    Ok(json!({"lid": lid_v, "monitors": mons_v, "warnings": warns}).to_string())
}

/// `bsctl display get [selector] [--format json]` — the report. Warnings
/// never change the exit status; only failing to query the compositor at
/// all (or an unknown selector) exits non-zero. A selector narrows the
/// TABLE (and the json's monitors) to one output; the lid line and the
/// warnings stay — they are the report's value and cost nothing.
pub fn get(sel: Option<&DisplaySel>, json_out: bool) -> i32 {
    let render = if json_out {
        get_json(sel)
    } else {
        get_text(sel)
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

/// One `display get` text result — table + lid line + warnings,
/// newline-terminated. Shared by the one-shot form and its `--stream` text
/// framing.
pub fn get_text(sel: Option<&DisplaySel>) -> Result<String, i32> {
    let mons = monitors_all()?;
    let outs = outputs_from(&mons);
    let ids = id_map(&mons);
    let only = match sel {
        Some(sel) => Some(resolve_output("get", sel, &outs, &ids)?.name.clone()),
        None => None,
    };
    let lid = lid_closed();
    let warns = warnings(&outs, lid);
    let shown: Vec<Output> = match &only {
        None => outs,
        Some(name) => outs.into_iter().filter(|o| o.name == *name).collect(),
    };
    let mut s = format_table(&shown, &ids) + "\n";
    if let Some(closed) = lid {
        s += &format!("lid: {}\n", if closed { "closed" } else { "open" });
    }
    for w in &warns {
        s += &format!("{w}\n");
    }
    Ok(s)
}

/// `bsctl display set dpms <selector> (--on|--off)` — safe dpms targeting
/// per lib.rs: read the output's current dpmsStatus from `j/monitors all`,
/// toggle (table form) only if it differs. Idempotent by construction.
pub fn set_dpms(sel: &DisplaySel, want_on: bool) -> i32 {
    let mons = match monitors_all() {
        Ok(v) => v,
        Err(c) => return c,
    };
    let outs = outputs_from(&mons);
    let ids = id_map(&mons);
    let o = match resolve_output("set dpms", sel, &outs, &ids) {
        Ok(o) => o,
        Err(c) => return c,
    };
    let name = o.name.clone();
    let name = name.as_str();
    let want = if want_on { "on" } else { "off" };
    match dpms_decision(o.enabled, o.dpms, want_on) {
        Dpms::Disabled => {
            eprintln!(
                "bsctl display: {name} is disabled — dpms would no-op; run: bsctl display reset (or clamshell.sh off for an internal panel) to re-enable"
            );
            1
        }
        Dpms::AlreadyThere => {
            println!("{name}: dpms already {want}");
            0
        }
        Dpms::Toggle => {
            let code = ipc::dispatch(&dpms_toggle_cmd(name));
            if code == 0 {
                println!("{name}: dpms toggled {want}");
            }
            code
        }
    }
}

/// `bsctl display reset` — the displays-on.sh reset flow: reload (re-applies
/// monitors.lua: native modes, re-enables wrongly disabled outputs), then
/// reconcile the lid, then dpms-on every enabled output. Each step reported;
/// a failed step aborts (the script's set -e).
pub fn reset() -> i32 {
    println!("reload: re-applying hyprland config");
    let code = ipc::reload();
    if code != 0 {
        eprintln!("bsctl display reset: reload failed (exit {code})");
        return code;
    }

    // clamshell.sh stays the OWNER of the lid/panel policy (AGENTS.md
    // recovery boundary) — shell out to it exactly like displays-on.sh does.
    // reload re-enables the internal panel regardless of lid; this puts it
    // back in step (no-op on desktops / lid open).
    //
    // Reconcile-and-VERIFY, bounded: reload's monitor re-application lands
    // asynchronously, so a single reconcile can lose the race — observed live
    // 2026-07-03: the re-enable arrived AFTER clamshell auto ran, leaving the
    // internal panel on with the lid shut (this tool's own status WARNING
    // caught it). Re-read the state and re-assert until stable, like the dpms
    // loop below.
    println!("lid: reconciling (clamshell.sh auto)");
    let script = config_home().join("hypr/scripts/clamshell.sh");
    for pass in 0..10 {
        match Command::new("sh").arg(&script).arg("auto").status() {
            Ok(s) if s.success() => {}
            Ok(s) => {
                let code = s.code().unwrap_or(1);
                eprintln!("bsctl display reset: clamshell.sh auto failed (exit {code})");
                return code;
            }
            Err(e) => {
                eprintln!("bsctl display reset: cannot run {}: {e}", script.display());
                return 1;
            }
        }
        // Lid open / desktop / unreadable lid: nothing to verify — one
        // reconcile was already a no-op, don't add sleeps or queries.
        if lid_closed() != Some(true) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(400));
        // Settled = no internal panel enabled while the lid is closed.
        let settled = match ipc::json("monitors") {
            Some(mons) => !outputs_from(&mons)
                .iter()
                .any(|o| o.enabled && is_internal(&o.name)),
            None => true, // no data: nothing further we can do
        };
        if settled {
            break;
        }
        if pass == 9 {
            eprintln!(
                "bsctl display reset: internal panel still enabled with the lid closed after 10 reconciles"
            );
            return 1;
        }
    }

    dpms_on_all()
}

/// displays-on.sh's dpms_on_all: `monitors` WITHOUT `all` lists only enabled
/// outputs, so a lid-disabled internal panel is correctly left alone. Toggle
/// only the outputs reading dpms off (the toggle-only gotcha), then re-read —
/// right after resume the report can be transient — up to 10 passes, 0.4s
/// apart. Never blanks an on display: each pass acts only on outputs
/// currently reading off. (The sh reference gives up SILENTLY with exit 0
/// after 10 passes; as the human-facing tool this reports the give-up and
/// exits 1 — the one deliberate divergence.)
fn dpms_on_all() -> i32 {
    for pass in 0..10 {
        let Some(mons) = ipc::json("monitors") else {
            eprintln!("bsctl display: monitors query failed (socket and hyprctl)");
            return 1;
        };
        let off: Vec<String> = outputs_from(&mons)
            .into_iter()
            .filter(|o| !o.dpms)
            .map(|o| o.name)
            .collect();
        if off.is_empty() {
            println!(
                "dpms: all enabled outputs on{}",
                if pass > 0 {
                    format!(
                        " (after {pass} toggle pass{})",
                        if pass > 1 { "es" } else { "" }
                    )
                } else {
                    String::new()
                }
            );
            return 0;
        }
        for name in &off {
            println!("dpms: toggling {name} on");
            ipc::dispatch(&dpms_toggle_cmd(name));
        }
        std::thread::sleep(std::time::Duration::from_millis(400));
    }
    eprintln!("bsctl display: gave up after 10 passes — some outputs still report dpms off");
    1
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Mirrors the live `j/monitors all` shape: external DP-1 enabled/on,
    /// internal eDP-1 disabled (reporting dpms true — the live quirk).
    fn mons_fixture() -> Value {
        json!([
            {"id": 0, "name": "eDP-1", "description": "LG Display 0x07C6",
             "disabled": true, "dpmsStatus": true, "width": 2880, "height": 1800,
             "refreshRate": 120.00100, "x": 0, "y": 0, "scale": 1.25},
            {"id": 1, "name": "DP-1", "description": "HP Inc. OMEN 34c CNC32023FL",
             "disabled": false, "dpmsStatus": true, "width": 3440, "height": 1440,
             "refreshRate": 100.0, "x": 0, "y": 0, "scale": 1.00},
        ])
    }

    #[test]
    fn outputs_parse_live_shape() {
        let outs = outputs_from(&mons_fixture());
        assert_eq!(outs.len(), 2);
        let edp = &outs[0];
        assert_eq!(edp.name, "eDP-1");
        assert!(!edp.enabled && edp.dpms);
        assert_eq!(edp.mode, "2880x1800@120");
        assert_eq!(edp.scale, "1.25");
        assert_eq!(edp.position, "0x0");
        let dp = &outs[1];
        assert!(dp.enabled && dp.dpms);
        assert_eq!(dp.mode, "3440x1440@100");
        assert_eq!(dp.scale, "1");
        // non-array / missing fields tolerated
        assert!(outputs_from(&json!("nope")).is_empty());
        let bare = outputs_from(&json!([{}]));
        assert_eq!(bare[0].name, "");
        assert!(bare[0].enabled); // missing `disabled` reads as enabled
        assert!(!bare[0].dpms);
    }

    #[test]
    fn trim_num_two_decimals_no_trailing_zeros() {
        assert_eq!(trim_num(1.0), "1");
        assert_eq!(trim_num(1.25), "1.25");
        assert_eq!(trim_num(100.001), "100");
        assert_eq!(trim_num(120.001), "120");
        assert_eq!(trim_num(59.999), "60"); // rounds, then trims
        assert_eq!(trim_num(1.5666), "1.57");
    }

    #[test]
    fn truncate_is_char_safe() {
        assert_eq!(truncate("short", 10), "short");
        assert_eq!(truncate("exactly-ten", 11), "exactly-ten");
        assert_eq!(truncate("a very long description", 10), "a very lo…");
        // multi-byte chars must not split
        assert_eq!(truncate("ééééé", 3), "éé…");
        // trailing space before the ellipsis is trimmed
        assert_eq!(truncate("ab cdef", 4), "ab…");
    }

    #[test]
    fn internal_panel_rule_matches_clamshell() {
        for n in ["eDP-1", "eDP-2", "LVDS-1", "DSI-1"] {
            assert!(is_internal(n), "{n}");
        }
        for n in ["DP-1", "HDMI-A-1", "DVI-D-1", "VGA-1", ""] {
            assert!(!is_internal(n), "{n}");
        }
    }

    #[test]
    fn table_is_aligned_and_truncated() {
        let outs = outputs_from(&mons_fixture());
        // ids number the ENABLED outputs only; a disabled panel shows "-"
        let ids = vec![("DP-1".to_string(), 1)];
        let t = format_table(&outs, &ids);
        assert_eq!(
            t,
            "ID  OUTPUT  ENABLED  DPMS  MODE           SCALE  POSITION  DESCRIPTION\n\
             -   eDP-1   no       on    2880x1800@120  1.25   0x0       LG Display 0x07C6\n\
             1   DP-1    yes      on    3440x1440@100  1      0x0       HP Inc. OMEN 34c CNC32023FL"
        );
        // a long description is capped at 48 chars + ellipsis
        let long = outputs_from(&json!([{"name": "DP-9", "description": "x".repeat(80)}]));
        let row = format_table(&long, &[]).lines().last().unwrap().to_string();
        assert!(row.ends_with(&format!("{}…", "x".repeat(47))));
    }

    #[test]
    fn no_warnings_when_state_is_consistent() {
        // the machine's actual current state: lid closed, panel disabled
        let outs = outputs_from(&mons_fixture());
        assert!(warnings(&outs, Some(true)).is_empty());
        assert!(warnings(&outs, Some(false)).is_empty());
        assert!(warnings(&outs, None).is_empty());
    }

    #[test]
    fn warns_internal_panel_enabled_while_lid_closed() {
        let outs = outputs_from(&json!([
            {"name": "eDP-1", "disabled": false, "dpmsStatus": true},
            {"name": "DP-1", "disabled": false, "dpmsStatus": true},
        ]));
        let w = warnings(&outs, Some(true));
        assert_eq!(w.len(), 1);
        assert!(w[0].contains("internal panel eDP-1"), "{w:?}");
        assert!(w[0].contains("bsctl display reset"), "{w:?}");
        // same outputs, lid open or absent: fine
        assert!(warnings(&outs, Some(false)).is_empty());
        assert!(warnings(&outs, None).is_empty());
    }

    #[test]
    fn warns_mixed_and_off_dpms_on_enabled_outputs_only() {
        let outs = outputs_from(&json!([
            {"name": "DP-1", "disabled": false, "dpmsStatus": true},
            {"name": "DP-2", "disabled": false, "dpmsStatus": false},
            {"name": "eDP-1", "disabled": true, "dpmsStatus": false},
        ]));
        let w = warnings(&outs, None);
        assert_eq!(w.len(), 2, "{w:?}");
        assert!(w[0].contains("mixed"), "{w:?}");
        assert!(w[1].contains("DP-2 is enabled but dpms is off"), "{w:?}");
        assert!(
            w[1].contains("bsctl display set dpms --display-name DP-2 --on"),
            "{w:?}"
        );
        // all enabled outputs off: not mixed, one warning per output
        let outs = outputs_from(&json!([
            {"name": "DP-1", "disabled": false, "dpmsStatus": false},
            {"name": "DP-2", "disabled": false, "dpmsStatus": false},
        ]));
        let w = warnings(&outs, None);
        assert_eq!(w.len(), 2, "{w:?}");
        assert!(w.iter().all(|l| l.contains("enabled but dpms is off")));
        // a disabled output's dpms reading triggers nothing
        let outs = outputs_from(&json!([
            {"name": "DP-1", "disabled": false, "dpmsStatus": true},
            {"name": "eDP-1", "disabled": true, "dpmsStatus": false},
        ]));
        assert!(warnings(&outs, None).is_empty());
    }

    #[test]
    fn dpms_decision_read_before_toggle() {
        assert!(matches!(
            dpms_decision(true, true, true),
            Dpms::AlreadyThere
        ));
        assert!(matches!(
            dpms_decision(true, false, false),
            Dpms::AlreadyThere
        ));
        assert!(matches!(dpms_decision(true, false, true), Dpms::Toggle));
        assert!(matches!(dpms_decision(true, true, false), Dpms::Toggle));
        // disabled outputs are refused regardless of the reading
        assert!(matches!(dpms_decision(false, true, true), Dpms::Disabled));
        assert!(matches!(dpms_decision(false, false, false), Dpms::Disabled));
    }

    #[test]
    fn dpms_cmd_is_table_form_only() {
        assert_eq!(
            dpms_toggle_cmd("eDP-1"),
            r#"hl.dsp.dpms({ monitor = "eDP-1" })"#
        );
        // hostile names can't break out of the Lua string
        assert_eq!(
            dpms_toggle_cmd(r#"x"break"#),
            r#"hl.dsp.dpms({ monitor = "x\"break" })"#
        );
    }

    #[test]
    fn lid_states_desktop_open_closed() {
        assert_eq!(lid_from_states(&[]), None); // no lid device -> desktop
        let open = "state:      open\n".to_string();
        let closed = "state:      closed\n".to_string();
        assert_eq!(lid_from_states(std::slice::from_ref(&open)), Some(false));
        assert_eq!(lid_from_states(std::slice::from_ref(&closed)), Some(true));
        // any closed lid counts (clamshell.sh's grep over all devices)
        assert_eq!(lid_from_states(&[open, closed]), Some(true));
    }
}
