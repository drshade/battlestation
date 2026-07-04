//! End-to-end tests for `bsctl ws` against a fake hyprctl that serves JSON
//! fixtures and appends every dispatch string to a capture log. The
//! expected dispatch strings are the live-verified Lua forms, byte-pinned;
//! the selector grammar (bs-id/bs-rel/ws-id/display-id/display-name) is
//! exercised through every verb that takes it.

use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

/// One isolated environment per test: its own XDG_STATE_HOME and a PATH whose
/// first entry holds a fake hyprctl wired to per-env fixture files.
struct TestEnv {
    root: PathBuf,
    state: PathBuf, // XDG_STATE_HOME
    fix: PathBuf,   // fixtures + dispatch capture
    path: String,   // fakebin:$PATH
}

impl TestEnv {
    fn new(name: &str) -> Self {
        static N: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "bsctl-ws-{}-{}-{}",
            std::process::id(),
            name,
            N.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&root);
        let state = root.join("state");
        let fix = root.join("fix");
        let fakebin = root.join("fakebin");
        for d in [&state, &fix, &fakebin] {
            fs::create_dir_all(d).unwrap();
        }
        let stub = fakebin.join("hyprctl");
        fs::write(
            &stub,
            format!(
                "#!/bin/sh\ncase \"$1\" in\n  workspaces) cat '{fix}/workspaces.json' ;;\n  activeworkspace) cat '{fix}/activeworkspace.json' ;;\n  monitors) cat '{fix}/monitors.json' ;;\n  dispatch) printf '%s\\n' \"$2\" >> '{fix}/dispatch.log' ;;\nesac\n",
                fix = fix.display()
            ),
        )
        .unwrap();
        let mut perm = fs::metadata(&stub).unwrap().permissions();
        std::os::unix::fs::PermissionsExt::set_mode(&mut perm, 0o755);
        fs::set_permissions(&stub, perm).unwrap();

        // Default fixtures. Workspaces: live ids 2,1,5,3 (hyprctl order), one
        // special workspace (excluded), ws 3 with a null name; 2,1,3 on
        // eDP-1, 5 on DP-1. Monitors: eDP-1 focused showing ws 2, DP-1
        // showing ws 5. Active workspace: 2.
        fs::write(
            fix.join("workspaces.json"),
            r#"[{"id":2,"name":"2","monitor":"eDP-1","windows":2},{"id":1,"name":"1","monitor":"eDP-1","windows":0},{"id":5,"name":"work","monitor":"DP-1","windows":3},{"id":3,"name":null,"monitor":"eDP-1","windows":1},{"id":-99,"name":"special:magic","monitor":"DP-1","windows":1}]"#,
        )
        .unwrap();
        fs::write(
            fix.join("monitors.json"),
            r#"[{"name":"eDP-1","x":0,"y":0,"focused":true,"activeWorkspace":{"id":2}},{"name":"DP-1","x":1920,"y":0,"focused":false,"activeWorkspace":{"id":5}}]"#,
        )
        .unwrap();
        fs::write(fix.join("activeworkspace.json"), r#"{"id":2,"name":"2"}"#).unwrap();

        let path = format!(
            "{}:{}",
            fakebin.display(),
            std::env::var("PATH").unwrap_or_default()
        );
        TestEnv {
            root,
            state,
            fix,
            path,
        }
    }

    fn ws(&self, args: &[&str]) -> std::process::Output {
        Command::new(env!("CARGO_BIN_EXE_bsctl"))
            .arg("ws")
            .args(args)
            .env("XDG_STATE_HOME", &self.state)
            .env("PATH", &self.path)
            // No socket lives under this root, so bsctl's socket-first ipc
            // always falls through to the fake hyprctl above — without this
            // isolation the tests would drive the developer's LIVE
            // compositor through the real request socket.
            .env("XDG_RUNTIME_DIR", self.root.join("run"))
            .output()
            .unwrap()
    }

    fn map_file(&self) -> PathBuf {
        self.state.join("battlestation-workspaces/map")
    }

    fn prefs_file(&self) -> PathBuf {
        self.state.join("battlestation-workspaces/prefs")
    }

    fn write_map(&self, content: &str) {
        fs::create_dir_all(self.state.join("battlestation-workspaces")).unwrap();
        fs::write(self.map_file(), content).unwrap();
    }

    fn write_prefs(&self, content: &str) {
        fs::create_dir_all(self.state.join("battlestation-workspaces")).unwrap();
        fs::write(self.prefs_file(), content).unwrap();
    }

    fn set_active(&self, id: i64) {
        fs::write(
            self.fix.join("activeworkspace.json"),
            format!(r#"{{"id":{id}}}"#),
        )
        .unwrap();
    }

    fn dispatches(&self) -> Vec<String> {
        fs::read_to_string(self.fix.join("dispatch.log"))
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

// ---- focus ---------------------------------------------------------------------

#[test]
fn focus_bs_id_maps_through_the_map_file() {
    let env = TestEnv::new("focus-bs");
    env.write_map("3 1\n"); // resolved: 3 1 2 5
    assert_eq!(env.ws(&["focus", "--bs-id", "1"]).status.code(), Some(0));
    assert_eq!(env.ws(&["focus", "--bs-id", "3"]).status.code(), Some(0));
    assert_eq!(
        env.dispatches(),
        vec![
            "hl.dsp.focus({ workspace = 3 })",
            "hl.dsp.focus({ workspace = 2 })",
        ]
    );
}

#[test]
fn focus_bs_id_without_map_file_is_identity_ascending() {
    let env = TestEnv::new("focus-identity");
    assert_eq!(env.ws(&["focus", "--bs-id", "4"]).status.code(), Some(0)); // 1 2 3 5 -> 5
    assert_eq!(env.dispatches(), vec!["hl.dsp.focus({ workspace = 5 })"]);
}

#[test]
fn focus_bs_id_off_the_end_dispatches_nothing_and_exits_1() {
    let env = TestEnv::new("focus-oob");
    let out = env.ws(&["focus", "--bs-id", "99"]);
    assert_eq!(out.status.code(), Some(1)); // silent: keybinds hit this constantly
    assert!(out.stdout.is_empty() && out.stderr.is_empty());
    assert!(env.dispatches().is_empty());
}

#[test]
fn focus_session_dispatches_window_or_workspace() {
    let env = TestEnv::new("focus-session");
    // Session state lives under XDG_RUNTIME_DIR/battlestation-ws (the
    // agents protocol); focus --session reads the record directly.
    let dir = env.root.join("run/battlestation-ws");
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("sess-win"),
        r#"{"ws": 5, "win": "0xfeed", "status": "waiting", "kind": "claude", "pid": 1}"#,
    )
    .unwrap();
    fs::write(
        dir.join("sess-nowin"),
        r#"{"ws": 5, "win": null, "status": "waiting", "kind": "claude", "pid": 1}"#,
    )
    .unwrap();
    assert_eq!(
        env.ws(&["focus", "--session", "sess-win"]).status.code(),
        Some(0)
    );
    assert_eq!(
        env.ws(&["focus", "--session", "sess-nowin"]).status.code(),
        Some(0)
    );
    assert_eq!(
        env.dispatches(),
        vec![
            r#"hl.dsp.focus({ window = "address:0xfeed" })"#,
            "hl.dsp.focus({ workspace = 5 })",
        ]
    );
    // Unknown session: loud error, no dispatch (a caller bug, not a
    // keybind grazing the map's edge).
    let out = env.ws(&["focus", "--session", "no-such"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(!out.stderr.is_empty());
    // Path-shaped sids are hostile input, refused before any file IO.
    let out = env.ws(&["focus", "--session", "../escape"]);
    assert_eq!(out.status.code(), Some(1));
    // And the selector group stays exactly-one: session + ws-id is misuse.
    let out = env.ws(&["focus", "--session", "s", "--ws-id", "3"]);
    assert_eq!(out.status.code(), Some(2));
}

#[test]
fn focus_ws_id_dispatches_without_resolving() {
    let env = TestEnv::new("focus-ws-id");
    // A raw ws-id needs no map and no live workspace — Hyprland creates it.
    assert_eq!(env.ws(&["focus", "--ws-id", "42"]).status.code(), Some(0));
    assert_eq!(env.ws(&["focus", "--ws-id", "-3"]).status.code(), Some(0));
    assert_eq!(
        env.dispatches(),
        vec![
            "hl.dsp.focus({ workspace = 42 })",
            "hl.dsp.focus({ workspace = -3 })",
        ]
    );
}

#[test]
fn focus_display_selectors() {
    let env = TestEnv::new("focus-display");
    assert_eq!(
        env.ws(&["focus", "--display-id", "2"]).status.code(),
        Some(0)
    );
    assert_eq!(
        env.ws(&["focus", "--display-name", "eDP-1"]).status.code(),
        Some(0)
    );
    assert_eq!(
        env.dispatches(),
        vec![
            r#"hl.dsp.focus({ monitor = "DP-1" })"#,
            r#"hl.dsp.focus({ monitor = "eDP-1" })"#,
        ]
    );
    // unknown id/name: loud error listing the valid numbering, no dispatch
    for args in [
        &["focus", "--display-id", "9"][..],
        &["focus", "--display-name", "HDMI-9"],
    ] {
        let out = env.ws(args);
        assert_eq!(out.status.code(), Some(1), "{args:?}");
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(err.contains("1 = eDP-1, 2 = DP-1"), "{err}");
    }
    assert_eq!(env.dispatches().len(), 2);
}

#[test]
fn focus_bs_rel_steps_and_clamps_in_map_order() {
    let env = TestEnv::new("focus-rel");
    env.write_map("3 1\n"); // resolved: 3 1 2 5; active 2 = pos 3
    assert_eq!(env.ws(&["focus", "--bs-rel", "+1"]).status.code(), Some(0)); // -> 5
    assert_eq!(env.ws(&["focus", "--bs-rel", "-1"]).status.code(), Some(0)); // -> 1
    env.set_active(5); // pos 4: +1 clamps at the end
    assert_eq!(env.ws(&["focus", "--bs-rel", "+1"]).status.code(), Some(0));
    env.set_active(3); // pos 1: -1 clamps at the start
    assert_eq!(env.ws(&["focus", "--bs-rel", "-1"]).status.code(), Some(0));
    env.set_active(-99); // active not in resolved -> position defaults to 1
    assert_eq!(env.ws(&["focus", "--bs-rel", "+1"]).status.code(), Some(0));
    assert_eq!(
        env.dispatches(),
        vec![
            "hl.dsp.focus({ workspace = 5 })",
            "hl.dsp.focus({ workspace = 1 })",
            "hl.dsp.focus({ workspace = 5 })",
            "hl.dsp.focus({ workspace = 3 })",
            "hl.dsp.focus({ workspace = 1 })",
        ]
    );
}

// ---- send window ---------------------------------------------------------------

#[test]
fn send_window_by_bs_and_ws_with_and_without_focus() {
    let env = TestEnv::new("send-window");
    env.write_map("3 1\n"); // resolved: 3 1 2 5
    assert_eq!(
        env.ws(&["send", "window", "--bs-id", "2"]).status.code(),
        Some(0)
    );
    assert_eq!(
        env.ws(&["send", "window", "--bs-id", "2", "--focus"])
            .status
            .code(),
        Some(0)
    );
    assert_eq!(
        env.ws(&["send", "window", "--ws-id", "7"]).status.code(),
        Some(0)
    );
    let out = env.ws(&["send", "window", "--bs-id", "99"]);
    assert_eq!(out.status.code(), Some(1)); // off the end: silent, no dispatch
    assert!(out.stdout.is_empty() && out.stderr.is_empty());
    assert_eq!(
        env.dispatches(),
        vec![
            "hl.dsp.window.move({ workspace = 1, follow = false })",
            "hl.dsp.window.move({ workspace = 1, follow = true })",
            "hl.dsp.window.move({ workspace = 7, follow = false })",
        ]
    );
}

#[test]
fn send_window_to_display_targets_its_active_workspace() {
    let env = TestEnv::new("send-window-display");
    assert_eq!(
        env.ws(&["send", "window", "--display-id", "2"])
            .status
            .code(),
        Some(0)
    );
    assert_eq!(
        env.ws(&["send", "window", "--display-name", "DP-1", "--focus"])
            .status
            .code(),
        Some(0)
    );
    // DP-1's active workspace is 5
    assert_eq!(
        env.dispatches(),
        vec![
            "hl.dsp.window.move({ workspace = 5, follow = false })",
            "hl.dsp.window.move({ workspace = 5, follow = true })",
        ]
    );
}

#[test]
fn send_window_bs_rel_steps_from_the_active_workspace() {
    let env = TestEnv::new("send-window-rel");
    env.write_map("3 1\n"); // resolved: 3 1 2 5; active 2 = pos 3
    assert_eq!(
        env.ws(&["send", "window", "--bs-rel", "+1", "--focus"])
            .status
            .code(),
        Some(0)
    );
    assert_eq!(
        env.dispatches(),
        vec!["hl.dsp.window.move({ workspace = 5, follow = true })"]
    );
}

// ---- send workspace --------------------------------------------------------------

#[test]
fn send_workspace_moves_stamps_pref_and_pins_focus() {
    let env = TestEnv::new("send-workspace");
    // focused eDP-1 shows ws 2; send it to display 2 (DP-1), stay behind
    assert_eq!(
        env.ws(&["send", "workspace", "--display-id", "2"])
            .status
            .code(),
        Some(0)
    );
    assert_eq!(
        env.dispatches(),
        vec![
            r#"hl.dsp.workspace.move({ workspace = 2, monitor = "DP-1" })"#,
            r#"hl.dsp.focus({ monitor = "eDP-1" })"#, // stay: source display re-focused
        ]
    );
    // the move stamped the preference: an explicit move re-decides the home
    assert_eq!(fs::read_to_string(env.prefs_file()).unwrap(), "2 DP-1\n");
}

#[test]
fn send_workspace_focus_follows_and_self_send_noops() {
    let env = TestEnv::new("send-workspace-focus");
    assert_eq!(
        env.ws(&["send", "workspace", "--display-name", "DP-1", "--focus"])
            .status
            .code(),
        Some(0)
    );
    assert_eq!(
        env.dispatches(),
        vec![
            r#"hl.dsp.workspace.move({ workspace = 2, monitor = "DP-1" })"#,
            "hl.dsp.focus({ workspace = 2 })", // --focus: follow the workspace
        ]
    );
    // sending to the display it's already on: nothing to do, no stamp
    let before = env.dispatches().len();
    assert_eq!(
        env.ws(&["send", "workspace", "--display-id", "1"])
            .status
            .code(),
        Some(0)
    );
    assert_eq!(env.dispatches().len(), before);
}

// ---- map ----------------------------------------------------------------------

#[test]
fn map_set_writes_ws_ids_in_bs_order() {
    let env = TestEnv::new("map-set");
    assert_eq!(
        env.ws(&["map", "set", "4", "2", "9"]).status.code(),
        Some(0)
    );
    // exact bytes: ws-ids space-joined + trailing newline
    assert_eq!(fs::read(env.map_file()).unwrap(), b"4 2 9\n");
    assert_eq!(env.ws(&["map", "set", "7"]).status.code(), Some(0));
    assert_eq!(fs::read(env.map_file()).unwrap(), b"7\n");
}

#[test]
fn map_set_requires_at_least_one_id() {
    let env = TestEnv::new("map-set-empty");
    let out = env.ws(&["map", "set"]);
    assert_eq!(out.status.code(), Some(2)); // usage error
    assert!(!out.stderr.is_empty());
    assert!(!env.map_file().exists());
}

#[test]
fn map_reset_truncates_in_place() {
    let env = TestEnv::new("map-reset");
    env.write_map("3 1\n");
    assert_eq!(env.ws(&["map", "reset"]).status.code(), Some(0));
    assert_eq!(fs::read(env.map_file()).unwrap(), b"");
    // works with no prior file too
    let env2 = TestEnv::new("map-reset-fresh");
    assert_eq!(env2.ws(&["map", "reset"]).status.code(), Some(0));
    assert_eq!(fs::read(env2.map_file()).unwrap(), b"");
}

#[test]
fn map_get_prints_the_resolved_join() {
    let env = TestEnv::new("map-get");
    env.write_map("9 3 1\n"); // 9 is not live -> skipped; resolved: 3 1 2 5
    let out = env.ws(&["map", "get"]);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "BS  WS  NAME  DISPLAY  WINDOWS  ACTIVE\n\
         1   3         eDP-1    1\n\
         2   1         eDP-1    0\n\
         3   2         eDP-1    2        yes\n\
         4   5   work  DP-1     3        yes\n"
    );
    // filtered to one display
    let out = env.ws(&["map", "get", "--display-name", "DP-1"]);
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "BS  WS  NAME  DISPLAY  WINDOWS  ACTIVE\n\
         4   5   work  DP-1     3        yes\n"
    );
}

#[test]
fn map_get_json_is_machine_shaped() {
    let env = TestEnv::new("map-get-json");
    env.write_map("3 1\n");
    let out = env.ws(&["map", "get", "--format", "json"]);
    assert_eq!(out.status.code(), Some(0));
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let rows = v.as_array().unwrap();
    assert_eq!(rows.len(), 4);
    assert_eq!(rows[0]["bs"], 1);
    assert_eq!(rows[0]["ws"], 3);
    assert_eq!(rows[0]["name"], serde_json::Value::Null); // null name = unnamed
    assert_eq!(rows[0]["display"], "eDP-1");
    assert_eq!(rows[3]["name"], "work");
    assert_eq!(rows[2]["active"], true);
    assert_eq!(rows[1]["active"], false);
}

// ---- name ---------------------------------------------------------------------

#[test]
fn name_set_escapes_for_the_lua_string() {
    let env = TestEnv::new("name-set");
    assert_eq!(
        env.ws(&["name", "set", "--ws-id", "3", "--name", "plain name"])
            .status
            .code(),
        Some(0)
    );
    assert_eq!(
        env.ws(&[
            "name",
            "set",
            "--ws-id",
            "7",
            "--name",
            r#"quo"te \back\ "x""#
        ])
        .status
        .code(),
        Some(0)
    );
    // --bs-id resolves through the map first (identity here: bs 4 -> ws 5)
    assert_eq!(
        env.ws(&["name", "set", "--bs-id", "4", "--name", "web"])
            .status
            .code(),
        Some(0)
    );
    assert_eq!(
        env.dispatches(),
        vec![
            r#"hl.dsp.workspace.rename({ workspace = 3, name = "plain name" })"#,
            r#"hl.dsp.workspace.rename({ workspace = 7, name = "quo\"te \\back\\ \"x\"" })"#,
            r#"hl.dsp.workspace.rename({ workspace = 5, name = "web" })"#,
        ]
    );
}

#[test]
fn name_rm_resets_to_the_number() {
    let env = TestEnv::new("name-rm");
    assert_eq!(
        env.ws(&["name", "rm", "--ws-id", "3"]).status.code(),
        Some(0)
    );
    assert_eq!(
        env.dispatches(),
        vec![r#"hl.dsp.workspace.rename({ workspace = 3, name = "3" })"#]
    );
    // rm with a display filter resets every workspace on it
    let env = TestEnv::new("name-rm-display");
    assert_eq!(
        env.ws(&["name", "rm", "--display-name", "DP-1"])
            .status
            .code(),
        Some(0)
    );
    assert_eq!(
        env.dispatches(),
        vec![r#"hl.dsp.workspace.rename({ workspace = 5, name = "5" })"#]
    );
}

#[test]
fn name_get_lists_bs_ws_and_names() {
    let env = TestEnv::new("name-get");
    let out = env.ws(&["name", "get"]);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "BS  WS  NAME\n\
         1   1\n\
         2   2\n\
         3   3\n\
         4   5   work\n"
    );
    let out = env.ws(&["name", "get", "--ws-id", "5"]);
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "BS  WS  NAME\n\
         4   5   work\n"
    );
}

// ---- prefs --------------------------------------------------------------------

#[test]
fn prefs_add_by_name_verbatim_and_by_id_resolved() {
    let env = TestEnv::new("prefs-add");
    // an ABSENT output by name: accepted verbatim (pre-declaring a home)
    assert_eq!(
        env.ws(&["prefs", "add", "--ws-id", "9", "--display-name", "DP-2"])
            .status
            .code(),
        Some(0)
    );
    // a display-id resolves through the live numbering
    assert_eq!(
        env.ws(&["prefs", "add", "--ws-id", "5", "--display-id", "2"])
            .status
            .code(),
        Some(0)
    );
    // a bs-id resolves through the map (identity: bs 3 -> ws 3)
    assert_eq!(
        env.ws(&["prefs", "add", "--bs-id", "3", "--display-name", "eDP-1"])
            .status
            .code(),
        Some(0)
    );
    assert_eq!(
        fs::read_to_string(env.prefs_file()).unwrap(),
        "3 eDP-1\n5 DP-1\n9 DP-2\n"
    );
    // an unknown display-id errors listing the numbering, writes nothing
    let out = env.ws(&["prefs", "add", "--ws-id", "5", "--display-id", "9"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("1 = eDP-1, 2 = DP-1"),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn prefs_get_annotates_and_filters() {
    let env = TestEnv::new("prefs-get");
    env.write_prefs("5 DP-1\n9 DP-2\n");
    let out = env.ws(&["prefs", "get"]);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "WS  DISPLAY  PRESENT  LIVE\n\
         5   DP-1     yes      yes\n\
         9   DP-2     no       no\n"
    );
    let out = env.ws(&["prefs", "get", "--ws-id", "5"]);
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "WS  DISPLAY  PRESENT  LIVE\n\
         5   DP-1     yes      yes\n"
    );
    // json form
    let out = env.ws(&["prefs", "get", "--format", "json"]);
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v[0]["ws"], 5);
    assert_eq!(v[0]["present"], true);
    assert_eq!(v[1]["display"], "DP-2");
    assert_eq!(v[1]["live"], false);
    // empty prefs: silent text, [] json
    let env2 = TestEnv::new("prefs-get-empty");
    let out = env2.ws(&["prefs", "get"]);
    assert!(out.stdout.is_empty());
    let out = env2.ws(&["prefs", "get", "--format", "json"]);
    assert_eq!(out.stdout, b"[]\n");
}

#[test]
fn prefs_rm_one_or_all() {
    let env = TestEnv::new("prefs-rm");
    env.write_prefs("5 DP-1\n9 DP-2\n");
    assert_eq!(
        env.ws(&["prefs", "rm", "--ws-id", "5"]).status.code(),
        Some(0)
    );
    assert_eq!(fs::read_to_string(env.prefs_file()).unwrap(), "9 DP-2\n");
    // removing an id with no preference: quiet success (idempotent)
    assert_eq!(
        env.ws(&["prefs", "rm", "--ws-id", "5"]).status.code(),
        Some(0)
    );
    assert_eq!(env.ws(&["prefs", "rm", "--all"]).status.code(), Some(0));
    assert_eq!(fs::read_to_string(env.prefs_file()).unwrap(), "");
}

#[test]
fn prefs_reconcile_moves_strays_home() {
    let env = TestEnv::new("prefs-reconcile");
    // ws 5 sits on DP-1 but prefers eDP-1 (present): reconcile moves it.
    env.write_prefs("5 eDP-1\n");
    let out = env.ws(&["prefs", "reconcile"]);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "ws 5 -> eDP-1\n");
    // The settle loop re-reads a STATIC fixture, so the move repeats up to
    // its bounded 5 passes — assert the dispatch happened, not how often.
    let moves = env.dispatches();
    assert!(
        moves
            .iter()
            .any(|d| d == r#"hl.dsp.workspace.move({ workspace = 5, monitor = "eDP-1" })"#),
        "{moves:?}"
    );
    // nothing astray: no dispatch at all
    let env2 = TestEnv::new("prefs-reconcile-clean");
    env2.write_prefs("5 DP-1\n");
    let out = env2.ws(&["prefs", "reconcile"]);
    assert_eq!(out.status.code(), Some(0));
    assert!(out.stdout.is_empty());
    assert!(env2.dispatches().is_empty());
}

// ---- argument validation (this is the human-facing CLI) ---------------------------

#[test]
fn usage_errors_exit_2() {
    let env = TestEnv::new("usage-errors");
    for args in [
        &["focus"][..],                                 // a selector is required
        &["focus", "--bs-id", "0"],                     // bs-ids are 1-based
        &["focus", "--bs-id", "two"],                   // not a number
        &["focus", "--bs-id", "1", "--ws-id", "2"],     // exclusive
        &["send", "window"],                            // a selector is required
        &["send", "workspace", "--ws-id", "5"],         // workspace sends take displays only
        &["bogus"],                                     // unknown subcommand
        &["name", "set", "--ws-id", "3"],               // missing --name
        &["map", "set", "x"],                           // ids must be integers
        &["prefs", "add", "--ws-id", "5"],              // missing display selector
        &["prefs", "rm"],                               // selector or --all required
        &["send", "window", "--bs-id", "2", "--folow"], // typo'd flag
    ] {
        let out = env.ws(args);
        assert_eq!(out.status.code(), Some(2), "ws {args:?} must exit 2");
        assert!(!out.stderr.is_empty(), "ws {args:?} must explain on stderr");
    }
    assert!(env.dispatches().is_empty());
}
