//! End-to-end tests for `bsctl display` and the ipc module's two transports:
//! a fake `.socket.sock` server (std UnixListener in a thread, logging every
//! request) for the socket path, and the fake-hyprctl-on-PATH pattern for
//! the fallback path (the env's XDG_RUNTIME_DIR holds no socket, so every
//! socket attempt fails by construction). The lid is faked through
//! BSCTL_LID_DIR and clamshell.sh through XDG_CONFIG_HOME; `display scale`'s
//! per-monitor rung state lands inside the same XDG_RUNTIME_DIR, isolated
//! for free.
//!
//! What CANNOT be faked here (left to live review): `reset`'s actual reload
//! effect on the compositor (re-applied monitors.lua, re-enabled outputs)
//! and real clamshell/lid reconciliation — these tests only pin the
//! command SEQUENCE reset emits.

use std::fs;
use std::io::{Read, Write};
use std::os::unix::net::UnixListener;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;

use serde_json::{Value, json};

const BIN: &str = env!("CARGO_BIN_EXE_bsctl");

/// The stable fixture, mirroring the live `j/monitors all` shape: DP-1
/// enabled/dpms-on, DP-2 enabled/dpms-off, eDP-1 disabled (reporting dpms
/// true — the live quirk of disabled panels).
fn monitors_all_fixture() -> String {
    json!([
        {"id": 0, "name": "eDP-1", "description": "LG Display 0x07C6",
         "disabled": true, "dpmsStatus": true, "width": 2880, "height": 1800,
         "refreshRate": 120.001, "x": 0, "y": 0, "scale": 1.25},
        {"id": 1, "name": "DP-1", "description": "HP Inc. OMEN 34c CNC32023FL",
         "disabled": false, "dpmsStatus": true, "width": 3440, "height": 1440,
         "refreshRate": 100.0, "x": 0, "y": 0, "scale": 1.0},
        {"id": 2, "name": "DP-2", "description": "Dell U2720Q ABC123",
         "disabled": false, "dpmsStatus": false, "width": 3840, "height": 2160,
         "refreshRate": 60.0, "x": 3440, "y": 0, "scale": 1.5},
    ])
    .to_string()
}

/// One isolated environment per test: its own XDG_RUNTIME_DIR (so the real
/// compositor socket is unreachable by construction), fake lid dir, fake
/// XDG_CONFIG_HOME (for clamshell.sh), and a PATH-first fake hyprctl that
/// serves fixture files and logs every call to fix/calls.log.
struct TestEnv {
    root: PathBuf,
    run: PathBuf,    // XDG_RUNTIME_DIR
    fix: PathBuf,    // fixtures + call logs
    config: PathBuf, // XDG_CONFIG_HOME
    lid: PathBuf,    // BSCTL_LID_DIR
    path: String,    // fakebin:$PATH
    his: String,     // HYPRLAND_INSTANCE_SIGNATURE handed to the binary
}

impl TestEnv {
    fn new(name: &str) -> Self {
        static N: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "bsctl-display-{}-{}-{}",
            std::process::id(),
            name,
            N.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&root);
        let run = root.join("run");
        let fix = root.join("fix");
        let config = root.join("config");
        let lid = root.join("lid");
        let fakebin = root.join("fakebin");
        for d in [&run, &fix, &config, &fakebin] {
            fs::create_dir_all(d).unwrap();
        }
        // Fake hyprctl: log every call, serve the monitors fixtures, record
        // dispatches. A dispatch also promotes fix/monitors.after.json to
        // fix/monitors.json when present — the "dpms toggle worked" seam the
        // reset test uses so its retry loop converges.
        let stub = fakebin.join("hyprctl");
        fs::write(
            &stub,
            format!(
                "#!/bin/sh\n\
                 echo \"hyprctl $*\" >> '{fix}/calls.log'\n\
                 case \"$1\" in\n\
                   monitors)\n\
                     if [ \"$2\" = all ]; then cat '{fix}/monitors_all.json'; else cat '{fix}/monitors.json'; fi ;;\n\
                   dispatch)\n\
                     printf '%s\\n' \"$2\" >> '{fix}/dispatch.log'\n\
                     if [ -f '{fix}/monitors.after.json' ]; then mv '{fix}/monitors.after.json' '{fix}/monitors.json'; fi ;;\n\
                   eval)\n\
                     printf '%s\\n' \"$2\" >> '{fix}/eval.log' ;;\n\
                 esac\n",
                fix = fix.display()
            ),
        )
        .unwrap();
        let mut perm = fs::metadata(&stub).unwrap().permissions();
        std::os::unix::fs::PermissionsExt::set_mode(&mut perm, 0o755);
        fs::set_permissions(&stub, perm).unwrap();
        fs::write(fix.join("monitors_all.json"), monitors_all_fixture()).unwrap();
        let path = format!(
            "{}:{}",
            fakebin.display(),
            std::env::var("PATH").unwrap_or_default()
        );
        TestEnv {
            root,
            run,
            fix,
            config,
            lid,
            path,
            his: "testinst".to_string(),
        }
    }

    /// Fake request-socket server for instance `inst`: logs each request to
    /// fix/socket.log and answers from the map (unknown requests get the
    /// compositor's real "unknown request"). The thread leaks per test —
    /// fine, the test process is short-lived.
    fn start_socket(&self, inst: &str, responses: &[(&str, &str)]) {
        let dir = self.run.join("hypr").join(inst);
        fs::create_dir_all(&dir).unwrap();
        let listener = UnixListener::bind(dir.join(".socket.sock")).unwrap();
        let log = self.fix.join("socket.log");
        let responses: Vec<(String, String)> = responses
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut s) = stream else { break };
                let mut req = Vec::new();
                if s.read_to_end(&mut req).is_err() {
                    continue;
                }
                let req = String::from_utf8_lossy(&req).into_owned();
                let mut f = fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&log)
                    .unwrap();
                writeln!(f, "{req}").unwrap();
                let reply = responses
                    .iter()
                    .find(|(k, _)| *k == req)
                    .map(|(_, v)| v.clone())
                    .unwrap_or_else(|| "unknown request".to_string());
                let _ = s.write_all(reply.as_bytes());
            }
        });
    }

    fn set_lid(&self, state: &str) {
        let d = self.lid.join("LID0");
        fs::create_dir_all(&d).unwrap();
        fs::write(d.join("state"), format!("state:      {state}\n")).unwrap();
    }

    fn display(&self, args: &[&str]) -> std::process::Output {
        Command::new(BIN)
            .arg("display")
            .args(args)
            .env("XDG_RUNTIME_DIR", &self.run)
            .env("PATH", &self.path)
            .env("XDG_CONFIG_HOME", &self.config)
            .env("BSCTL_LID_DIR", &self.lid)
            .env("HYPRLAND_INSTANCE_SIGNATURE", &self.his)
            .output()
            .unwrap()
    }

    fn log(&self, name: &str) -> Vec<String> {
        fs::read_to_string(self.fix.join(name))
            .unwrap_or_default()
            .lines()
            .map(str::to_owned)
            .collect()
    }
}

impl Drop for TestEnv {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

const MON_ALL_REQ: &str = "j/monitors all";
const TOGGLE_DP1: &str = r#"dispatch hl.dsp.dpms({ monitor = "DP-1" })"#;
const TOGGLE_DP2: &str = r#"dispatch hl.dsp.dpms({ monitor = "DP-2" })"#;

// ---- status ------------------------------------------------------------------

#[test]
fn status_via_socket_disabled_panel_closed_lid_is_clean() {
    // The machine's real resting state: lid closed, internal panel disabled.
    let env = TestEnv::new("status-socket");
    env.set_lid("closed");
    let fixture = monitors_all_fixture();
    env.start_socket("testinst", &[(MON_ALL_REQ, &fixture)]);

    let out = env.display(&["status"]);
    assert_eq!(out.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("OUTPUT"), "{stdout}");
    assert!(stdout.contains("eDP-1   no       on"), "{stdout}");
    assert!(stdout.contains("DP-1    yes      on"), "{stdout}");
    assert!(stdout.contains("lid: closed"), "{stdout}");
    // DP-2 is dpms-off so warnings exist — but NOT for the disabled panel
    assert!(!stdout.contains("internal panel"), "{stdout}");
    // served by the socket, hyprctl never spawned
    assert_eq!(env.log("socket.log"), vec![MON_ALL_REQ]);
    assert!(env.log("calls.log").is_empty());
}

#[test]
fn status_warnings_via_fallback() {
    // No socket in this env's XDG_RUNTIME_DIR -> every request falls back to
    // the fake hyprctl. Fixture: internal panel enabled + lid closed, and
    // mixed dpms (DP-1 on, DP-2 off).
    let env = TestEnv::new("status-fallback");
    env.set_lid("closed");
    fs::write(
        env.fix.join("monitors_all.json"),
        json!([
            {"name": "eDP-1", "disabled": false, "dpmsStatus": true,
             "width": 2880, "height": 1800, "refreshRate": 120.0, "x": 0, "y": 0, "scale": 1.25},
            {"name": "DP-1", "disabled": false, "dpmsStatus": true,
             "width": 3440, "height": 1440, "refreshRate": 100.0, "x": 0, "y": 0, "scale": 1.0},
            {"name": "DP-2", "disabled": false, "dpmsStatus": false,
             "width": 3840, "height": 2160, "refreshRate": 60.0, "x": 3440, "y": 0, "scale": 1.5},
        ])
        .to_string(),
    )
    .unwrap();

    let out = env.display(&["status"]);
    assert_eq!(out.status.code(), Some(0), "warnings must not change exit");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains(
            "WARNING: internal panel eDP-1 is enabled while the lid is closed — run: bsctl display reset (or clamshell.sh on)"
        ),
        "{stdout}"
    );
    assert!(stdout.contains("dpms state is mixed"), "{stdout}");
    assert!(
        stdout.contains("DP-2 is enabled but dpms is off — run: bsctl display on DP-2"),
        "{stdout}"
    );
    assert_eq!(env.log("calls.log"), vec!["hyprctl monitors all -j"]);
}

#[test]
fn status_json_emits_raw_structured_form() {
    let env = TestEnv::new("status-json");
    env.set_lid("closed");
    let fixture = monitors_all_fixture();
    env.start_socket("testinst", &[(MON_ALL_REQ, &fixture)]);

    let out = env.display(&["status", "--json"]);
    assert_eq!(out.status.code(), Some(0));
    let v: Value = serde_json::from_slice(&out.stdout).expect("must be one JSON doc");
    assert_eq!(v["lid"], json!("closed"));
    assert_eq!(
        v["monitors"],
        serde_json::from_str::<Value>(&fixture).unwrap()
    );
    let warns = v["warnings"].as_array().unwrap();
    assert_eq!(warns.len(), 2, "mixed + DP-2 off: {warns:?}");
}

#[test]
fn status_without_lid_dir_omits_lid_line() {
    // BSCTL_LID_DIR points at a nonexistent dir = no lid device = desktop.
    let env = TestEnv::new("status-desktop");
    let fixture = monitors_all_fixture();
    env.start_socket("testinst", &[(MON_ALL_REQ, &fixture)]);
    let out = env.display(&["status"]);
    assert_eq!(out.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!stdout.contains("lid:"), "{stdout}");
}

// ---- on / off ------------------------------------------------------------------

#[test]
fn on_off_read_before_toggle_via_socket() {
    let env = TestEnv::new("on-off-socket");
    let fixture = monitors_all_fixture();
    env.start_socket(
        "testinst",
        &[
            (MON_ALL_REQ, &fixture),
            (TOGGLE_DP1, "ok"),
            (TOGGLE_DP2, "ok"),
        ],
    );

    // already in the desired state: note + exit 0, NO dispatch
    let out = env.display(&["on", "DP-1"]);
    assert_eq!(out.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&out.stdout).contains("already on"));
    assert_eq!(env.log("socket.log"), vec![MON_ALL_REQ]);

    // differs: exactly one TABLE-FORM toggle for that one output
    let out = env.display(&["off", "DP-1"]);
    assert_eq!(out.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&out.stdout).contains("toggled off"));
    let out = env.display(&["on", "DP-2"]);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(
        env.log("socket.log"),
        vec![
            MON_ALL_REQ,
            MON_ALL_REQ,
            TOGGLE_DP1,
            MON_ALL_REQ,
            TOGGLE_DP2
        ]
    );
    // everything went over the socket; the fake hyprctl was never consulted
    assert!(env.log("calls.log").is_empty());
}

#[test]
fn unknown_output_errors_listing_valid_names() {
    let env = TestEnv::new("unknown-output");
    let fixture = monitors_all_fixture();
    env.start_socket("testinst", &[(MON_ALL_REQ, &fixture)]);
    let out = env.display(&["on", "HDMI-9"]);
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("unknown output HDMI-9"), "{err}");
    assert!(err.contains("eDP-1, DP-1, DP-2"), "{err}");
    assert_eq!(env.log("socket.log"), vec![MON_ALL_REQ]); // no dispatch
}

#[test]
fn disabled_output_is_refused_with_remedy() {
    // A disabled panel reports dpmsStatus true (live quirk) — trusting it
    // would make `on eDP-1` claim success while doing nothing.
    let env = TestEnv::new("disabled-refused");
    let fixture = monitors_all_fixture();
    env.start_socket("testinst", &[(MON_ALL_REQ, &fixture)]);
    for args in [&["on", "eDP-1"][..], &["off", "eDP-1"]] {
        let out = env.display(args);
        assert_eq!(out.status.code(), Some(1), "{args:?}");
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(err.contains("eDP-1 is disabled"), "{err}");
        assert!(err.contains("bsctl display reset"), "{err}");
    }
    assert_eq!(env.log("socket.log"), vec![MON_ALL_REQ, MON_ALL_REQ]);
}

#[test]
fn non_ok_dispatch_reply_falls_back_to_hyprctl() {
    // The socket answers the query but REJECTS the dispatch -> bsctl must
    // re-send it through hyprctl (protocol-drift safety net; a non-ok reply
    // means nothing executed, so the re-send cannot double-toggle).
    let env = TestEnv::new("dispatch-fallback");
    let fixture = monitors_all_fixture();
    env.start_socket(
        "testinst",
        &[(MON_ALL_REQ, &fixture), (TOGGLE_DP1, "error: nope")],
    );
    let out = env.display(&["off", "DP-1"]);
    assert_eq!(out.status.code(), Some(0)); // fake hyprctl exits 0
    assert_eq!(
        env.log("dispatch.log"),
        vec![r#"hl.dsp.dpms({ monitor = "DP-1" })"#]
    );
    assert_eq!(env.log("socket.log"), vec![MON_ALL_REQ, TOGGLE_DP1]);
}

// ---- reset ---------------------------------------------------------------------

#[test]
fn reset_runs_reload_then_clamshell_then_dpms_on() {
    // Fallback path end to end (no socket). The enabled-only `monitors` view
    // starts with DP-1 dpms off; the fake's dispatch side effect flips it on,
    // so the bounded retry loop converges on pass 2.
    let env = TestEnv::new("reset");
    fs::write(
        env.fix.join("monitors.json"),
        json!([{"name": "DP-1", "disabled": false, "dpmsStatus": false}]).to_string(),
    )
    .unwrap();
    fs::write(
        env.fix.join("monitors.after.json"),
        json!([{"name": "DP-1", "disabled": false, "dpmsStatus": true}]).to_string(),
    )
    .unwrap();
    let scripts = env.config.join("hypr/scripts");
    fs::create_dir_all(&scripts).unwrap();
    fs::write(
        scripts.join("clamshell.sh"),
        format!(
            "#!/bin/sh\necho \"clamshell $*\" >> '{}/calls.log'\n",
            env.fix.display()
        ),
    )
    .unwrap();

    let out = env.display(&["reset"]);
    assert_eq!(out.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&out.stdout);
    for step in [
        "reload: re-applying hyprland config",
        "lid: reconciling (clamshell.sh auto)",
        "dpms: toggling DP-1 on",
        "dpms: all enabled outputs on",
    ] {
        assert!(stdout.contains(step), "missing {step:?} in {stdout}");
    }
    // exact sequence: reload -> clamshell auto -> read -> toggle -> re-read
    assert_eq!(
        env.log("calls.log"),
        vec![
            "hyprctl reload",
            "clamshell auto",
            "hyprctl monitors -j",
            r#"hyprctl dispatch hl.dsp.dpms({ monitor = "DP-1" })"#,
            "hyprctl monitors -j",
        ]
    );
}

#[test]
fn reset_aborts_when_clamshell_fails() {
    let env = TestEnv::new("reset-clamshell-fail");
    fs::write(
        env.fix.join("monitors.json"),
        json!([{"name": "DP-1", "disabled": false, "dpmsStatus": true}]).to_string(),
    )
    .unwrap();
    let scripts = env.config.join("hypr/scripts");
    fs::create_dir_all(&scripts).unwrap();
    fs::write(scripts.join("clamshell.sh"), "#!/bin/sh\nexit 3\n").unwrap();

    let out = env.display(&["reset"]);
    assert_eq!(out.status.code(), Some(3)); // step's code propagates, like set -e
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("clamshell.sh auto failed"), "{err}");
    // never reached the dpms pass
    assert_eq!(env.log("calls.log"), vec!["hyprctl reload"]);
}

// ---- scale ---------------------------------------------------------------------

/// The enabled-only `j/monitors` view scale reads (the script's `hyprctl
/// monitors -j`): DP-1 focused, at the given reported scale.
fn scale_mons_fixture(scale: f64) -> String {
    json!([
        {"id": 1, "name": "DP-1", "description": "HP Inc. OMEN 34c CNC32023FL",
         "focused": true, "disabled": false, "dpmsStatus": true,
         "width": 3440, "height": 1440, "refreshRate": 100.0,
         "x": 0, "y": 0, "scale": scale},
    ])
    .to_string()
}

const MON_REQ: &str = "j/monitors";

/// The socket wire form of the scale dispatch for DP-1 — byte-identical to
/// what display-scale.sh hands `hyprctl eval`.
fn scale_eval_req(luascale: &str) -> String {
    format!(
        r#"eval hl.monitor({{ output = "DP-1", mode = "3440x1440@100", position = "auto", scale = {luascale} }})"#
    )
}

impl TestEnv {
    fn scale_state(&self) -> PathBuf {
        self.run.join("hypr-display-scale.DP-1")
    }
}

#[test]
fn scale_up_from_nothing_seeds_nearest_rung_via_socket() {
    // No state file: the reported scale 1.0 seeds rung 0, up -> rung 1.
    let env = TestEnv::new("scale-up-fresh");
    let fixture = scale_mons_fixture(1.0);
    let eval = scale_eval_req("1.25000");
    env.start_socket("testinst", &[(MON_REQ, &fixture), (&eval, "ok")]);

    let out = env.display(&["scale", "up"]);
    assert_eq!(out.status.code(), Some(0));
    assert!(out.stdout.is_empty(), "script parity: silent on success");
    assert_eq!(env.log("socket.log"), vec![MON_REQ.to_string(), eval]);
    assert!(env.log("calls.log").is_empty());
    // the new rung index is persisted, index + newline
    assert_eq!(fs::read(env.scale_state()).unwrap(), b"1\n");
}

#[test]
fn scale_up_prefers_saved_index_over_reported_scale() {
    // Saved rung 3 disagrees with the reported scale 1.0 — the index wins
    // (Hyprland's grid snap makes the reported value untrustworthy).
    let env = TestEnv::new("scale-saved-index");
    fs::write(env.scale_state(), "3\n").unwrap();
    let fixture = scale_mons_fixture(1.0);
    let eval = scale_eval_req("2.00000");
    env.start_socket("testinst", &[(MON_REQ, &fixture), (&eval, "ok")]);

    let out = env.display(&["scale", "up"]);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(env.log("socket.log"), vec![MON_REQ.to_string(), eval]);
    assert_eq!(fs::read(env.scale_state()).unwrap(), b"4\n");
}

#[test]
fn scale_clamps_at_both_ends() {
    // Top rung + up: stays 3.00000, index stays 6 — and still dispatches.
    let env = TestEnv::new("scale-clamp-top");
    fs::write(env.scale_state(), "6\n").unwrap();
    let fixture = scale_mons_fixture(3.0);
    let eval = scale_eval_req("3.00000");
    env.start_socket("testinst", &[(MON_REQ, &fixture), (&eval, "ok")]);
    let out = env.display(&["scale", "up"]);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(env.log("socket.log"), vec![MON_REQ.to_string(), eval]);
    assert_eq!(fs::read(env.scale_state()).unwrap(), b"6\n");

    // Bottom rung + down, seeded from the reported scale (no state file).
    let env = TestEnv::new("scale-clamp-bottom");
    let fixture = scale_mons_fixture(1.0);
    let eval = scale_eval_req("1.00000");
    env.start_socket("testinst", &[(MON_REQ, &fixture), (&eval, "ok")]);
    let out = env.display(&["scale", "down"]);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(env.log("socket.log"), vec![MON_REQ.to_string(), eval]);
    assert_eq!(fs::read(env.scale_state()).unwrap(), b"0\n");
}

#[test]
fn scale_reset_deletes_state_and_evals_auto_string() {
    let env = TestEnv::new("scale-reset");
    fs::write(env.scale_state(), "2\n").unwrap();
    let fixture = scale_mons_fixture(1.5);
    // scale = "auto" — the QUOTED Lua string, not a bare number
    let eval = scale_eval_req(r#""auto""#);
    env.start_socket("testinst", &[(MON_REQ, &fixture), (&eval, "ok")]);

    let out = env.display(&["scale", "reset"]);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(env.log("socket.log"), vec![MON_REQ.to_string(), eval]);
    assert!(!env.scale_state().exists(), "reset must rm the state file");
    // reset with no state file is equally fine (rm -f)
    let out = env.display(&["scale", "reset"]);
    assert_eq!(out.status.code(), Some(0));
}

#[test]
fn scale_no_focused_monitor_is_silent_exit_0() {
    let env = TestEnv::new("scale-unfocused");
    let fixture = json!([
        {"id": 1, "name": "DP-1", "focused": false, "width": 3440,
         "height": 1440, "refreshRate": 100.0, "scale": 1.0},
    ])
    .to_string();
    env.start_socket("testinst", &[(MON_REQ, &fixture)]);

    let out = env.display(&["scale", "up"]);
    assert_eq!(out.status.code(), Some(0));
    assert!(out.stdout.is_empty() && out.stderr.is_empty());
    // queried, but never dispatched and never wrote state
    assert_eq!(env.log("socket.log"), vec![MON_REQ]);
    assert!(!env.scale_state().exists());
}

#[test]
fn scale_falls_back_to_hyprctl() {
    // No socket in this env -> both the query and the eval go through the
    // fake hyprctl, with the script's exact argv shape.
    let env = TestEnv::new("scale-fallback");
    fs::write(env.fix.join("monitors.json"), scale_mons_fixture(1.0)).unwrap();

    let out = env.display(&["scale", "up"]);
    assert_eq!(out.status.code(), Some(0));
    let eval = r#"hl.monitor({ output = "DP-1", mode = "3440x1440@100", position = "auto", scale = 1.25000 })"#;
    assert_eq!(
        env.log("calls.log"),
        vec![
            "hyprctl monitors -j".to_string(),
            format!("hyprctl eval {eval}")
        ]
    );
    assert_eq!(env.log("eval.log"), vec![eval]);
    assert_eq!(fs::read(env.scale_state()).unwrap(), b"1\n");
}

#[test]
fn scale_garbage_state_file_errors() {
    // The sh reference dies on int(garbage) too; ours says why.
    let env = TestEnv::new("scale-garbage-state");
    fs::write(env.scale_state(), "not-a-rung\n").unwrap();
    let fixture = scale_mons_fixture(1.0);
    env.start_socket("testinst", &[(MON_REQ, &fixture)]);

    let out = env.display(&["scale", "up"]);
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("unreadable rung index"), "{err}");
    assert!(err.contains("hypr-display-scale.DP-1"), "{err}");
    // no dispatch, state left for inspection
    assert_eq!(env.log("socket.log"), vec![MON_REQ]);
    assert_eq!(fs::read(env.scale_state()).unwrap(), b"not-a-rung\n");
}

// ---- instance discovery -----------------------------------------------------------

#[test]
fn discovery_without_signature_picks_newest_instance_dir() {
    // Empty HYPRLAND_INSTANCE_SIGNATURE counts as unset -> newest-mtime dir
    // under $XDG_RUNTIME_DIR/hypr wins. The older dir sorts lexically LAST,
    // so a lexical pick would miss the socket and leak to hyprctl — the
    // empty calls.log proves mtime ordering did the work.
    let mut env = TestEnv::new("discovery");
    env.his = String::new();
    fs::create_dir_all(env.run.join("hypr/zzz_stale_instance")).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(30)); // mtime tick
    let fixture = monitors_all_fixture();
    env.start_socket("aaa_live_instance", &[(MON_ALL_REQ, &fixture)]);

    let out = env.display(&["status"]);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(env.log("socket.log"), vec![MON_ALL_REQ]);
    assert!(env.log("calls.log").is_empty());
}
