//! End-to-end tests for `bsctl ws` against a fake hyprctl that serves JSON
//! fixtures and appends every dispatch string to a capture log — the same
//! harness the ws.sh parity check used, so the expected dispatch strings
//! below are the script's verbatim output for identical inputs.

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
                "#!/bin/sh\ncase \"$1\" in\n  workspaces) cat '{fix}/workspaces.json' ;;\n  activeworkspace) cat '{fix}/activeworkspace.json' ;;\n  dispatch) printf '%s\\n' \"$2\" >> '{fix}/dispatch.log' ;;\nesac\n",
                fix = fix.display()
            ),
        )
        .unwrap();
        let mut perm = fs::metadata(&stub).unwrap().permissions();
        std::os::unix::fs::PermissionsExt::set_mode(&mut perm, 0o755);
        fs::set_permissions(&stub, perm).unwrap();

        // Default fixture: live ids 2,1,5,3 (hyprctl order), one special
        // workspace (excluded), ws 3 with a null name; active ws 2.
        fs::write(
            fix.join("workspaces.json"),
            r#"[{"id":2,"name":"2"},{"id":1,"name":"1"},{"id":5,"name":"work"},{"id":3,"name":null},{"id":-99,"name":"special:magic"}]"#,
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

    fn order_file(&self) -> PathBuf {
        self.state.join("battlestation-workspaces/order")
    }

    fn write_order(&self, content: &str) {
        fs::create_dir_all(self.state.join("battlestation-workspaces")).unwrap();
        fs::write(self.order_file(), content).unwrap();
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

// ---- goto / movewindow -------------------------------------------------------

#[test]
fn goto_maps_display_position_through_the_order_file() {
    let env = TestEnv::new("goto");
    env.write_order("3 1\n"); // resolved: 3 1 2 5
    assert_eq!(env.ws(&["goto", "1"]).status.code(), Some(0));
    assert_eq!(env.ws(&["goto", "3"]).status.code(), Some(0));
    assert_eq!(
        env.dispatches(),
        vec![
            "hl.dsp.focus({ workspace = 3 })",
            "hl.dsp.focus({ workspace = 2 })",
        ]
    );
}

#[test]
fn goto_without_order_file_is_identity_ascending() {
    let env = TestEnv::new("goto-identity");
    assert_eq!(env.ws(&["goto", "4"]).status.code(), Some(0)); // 1 2 3 5 -> 5
    assert_eq!(env.dispatches(), vec!["hl.dsp.focus({ workspace = 5 })"]);
}

#[test]
fn goto_off_the_end_dispatches_nothing_and_exits_1() {
    let env = TestEnv::new("goto-oob");
    let out = env.ws(&["goto", "99"]);
    assert_eq!(out.status.code(), Some(1)); // script parity (verified)
    assert!(out.stdout.is_empty() && out.stderr.is_empty());
    assert!(env.dispatches().is_empty());
}

#[test]
fn movewindow_with_and_without_follow() {
    let env = TestEnv::new("movewindow");
    env.write_order("3 1\n");
    assert_eq!(env.ws(&["movewindow", "2"]).status.code(), Some(0));
    assert_eq!(
        env.ws(&["movewindow", "2", "--follow"]).status.code(),
        Some(0)
    );
    assert_eq!(env.ws(&["movewindow", "99"]).status.code(), Some(1));
    assert_eq!(
        env.dispatches(),
        vec![
            "hl.dsp.window.move({ workspace = 1, follow = false })",
            "hl.dsp.window.move({ workspace = 1, follow = true })",
        ]
    );
}

// ---- relative ------------------------------------------------------------------

#[test]
fn relative_steps_and_clamps_in_display_order() {
    let env = TestEnv::new("relative");
    env.write_order("3 1\n"); // resolved: 3 1 2 5; active 2 = pos 3
    assert_eq!(env.ws(&["relative", "next"]).status.code(), Some(0)); // -> 5
    assert_eq!(env.ws(&["relative", "prev"]).status.code(), Some(0)); // -> 1
    assert_eq!(
        env.ws(&["relative", "next", "--move"]).status.code(),
        Some(0)
    );
    env.set_active(5); // pos 4: next clamps at the end
    assert_eq!(env.ws(&["relative", "next"]).status.code(), Some(0));
    env.set_active(3); // pos 1: prev clamps at the start
    assert_eq!(env.ws(&["relative", "prev"]).status.code(), Some(0));
    env.set_active(-99); // active not in resolved -> position defaults to 1
    assert_eq!(env.ws(&["relative", "next"]).status.code(), Some(0));
    assert_eq!(
        env.dispatches(),
        vec![
            "hl.dsp.focus({ workspace = 5 })",
            "hl.dsp.focus({ workspace = 1 })",
            "hl.dsp.window.move({ workspace = 5, follow = true })",
            "hl.dsp.focus({ workspace = 5 })",
            "hl.dsp.focus({ workspace = 3 })",
            "hl.dsp.focus({ workspace = 1 })",
        ]
    );
}

// ---- set / reset / get -----------------------------------------------------------

#[test]
fn set_writes_space_joined_ids_with_trailing_newline() {
    let env = TestEnv::new("set");
    assert_eq!(env.ws(&["set", "4", "2", "9"]).status.code(), Some(0));
    // exact bytes: the bar plugin FileView-parses this format
    assert_eq!(fs::read(env.order_file()).unwrap(), b"4 2 9\n");
    assert_eq!(env.ws(&["set", "7"]).status.code(), Some(0));
    assert_eq!(fs::read(env.order_file()).unwrap(), b"7\n");
}

#[test]
fn set_requires_at_least_one_id() {
    let env = TestEnv::new("set-empty");
    let out = env.ws(&["set"]);
    assert_eq!(out.status.code(), Some(2)); // usage error, like the script
    assert!(!out.stderr.is_empty());
    assert!(!env.order_file().exists());
}

#[test]
fn reset_truncates_in_place() {
    let env = TestEnv::new("reset");
    env.write_order("3 1\n");
    assert_eq!(env.ws(&["reset"]).status.code(), Some(0));
    // still exists (a delete would not fire the plugin's FileView watch), empty
    assert_eq!(fs::read(env.order_file()).unwrap(), b"");
    // works with no prior file too
    let env2 = TestEnv::new("reset-fresh");
    assert_eq!(env2.ws(&["reset"]).status.code(), Some(0));
    assert_eq!(fs::read(env2.order_file()).unwrap(), b"");
}

#[test]
fn get_prints_raw_file_or_nothing() {
    let env = TestEnv::new("get");
    let out = env.ws(&["get"]);
    assert_eq!(out.status.code(), Some(0)); // missing file: silent success
    assert!(out.stdout.is_empty());
    env.write_order("3 1\n");
    let out = env.ws(&["get"]);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(out.stdout, b"3 1\n");
}

// ---- rename --------------------------------------------------------------------

#[test]
fn rename_escapes_for_the_lua_string() {
    let env = TestEnv::new("rename");
    assert_eq!(
        env.ws(&["rename", "3", "plain name"]).status.code(),
        Some(0)
    );
    assert_eq!(env.ws(&["rename", "3"]).status.code(), Some(0)); // omitted
    assert_eq!(env.ws(&["rename", "3", ""]).status.code(), Some(0)); // empty
    assert_eq!(
        env.ws(&["rename", "7", r#"quo"te \back\ "x""#])
            .status
            .code(),
        Some(0)
    );
    assert_eq!(
        env.dispatches(),
        vec![
            r#"hl.dsp.workspace.rename({ workspace = 3, name = "plain name" })"#,
            r#"hl.dsp.workspace.rename({ workspace = 3, name = "3" })"#,
            r#"hl.dsp.workspace.rename({ workspace = 3, name = "3" })"#,
            r#"hl.dsp.workspace.rename({ workspace = 7, name = "quo\"te \\back\\ \"x\"" })"#,
        ]
    );
}

// ---- order (debug listing) -------------------------------------------------------

#[test]
fn order_lists_pos_id_name_including_dead_pref_ids_skipped() {
    let env = TestEnv::new("order");
    env.write_order("9 3 1\n"); // 9 is not live -> skipped
    let out = env.ws(&["order"]);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "1 -> ws 3 (null)\n2 -> ws 1 (1)\n3 -> ws 2 (2)\n4 -> ws 5 (work)\n"
    );
}

// ---- argument validation (this is the human-facing CLI) ---------------------------

#[test]
fn usage_errors_exit_2() {
    let env = TestEnv::new("usage-errors");
    for args in [
        &["goto"][..],                   // missing position
        &["goto", "0"],                  // positions are 1-based
        &["goto", "two"],                // not a number
        &["relative", "up"],             // not next|prev
        &["bogus"],                      // unknown subcommand
        &["rename"],                     // missing id
        &["set", "x"],                   // ids must be integers
        &["movewindow", "2", "--folow"], // typo'd flag
    ] {
        let out = env.ws(args);
        assert_eq!(out.status.code(), Some(2), "ws {args:?} must exit 2");
        assert!(!out.stderr.is_empty(), "ws {args:?} must explain on stderr");
    }
    assert!(env.dispatches().is_empty());
}
