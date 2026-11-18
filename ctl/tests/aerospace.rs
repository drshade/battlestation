//! The AeroSpace backend end to end: the real binary under
//! `BSCTL_WM=aerospace`, against a fake `aerospace` CLI on PATH that serves
//! canned `list-*` JSON and logs every other invocation. Runs on every
//! platform — the backend is plain CLI calls, nothing macOS-specific.

use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};

use serde_json::{Value, json};

const BIN: &str = env!("CARGO_BIN_EXE_bsctl");

struct TestEnv {
    root: PathBuf,
    run: PathBuf,
    fix: PathBuf,
    path: String,
}

impl TestEnv {
    /// `windows` may reference `{PID}` — replaced by this test process's
    /// pid, which IS an ancestor of every hook the test spawns.
    fn new(name: &str, windows: Value) -> Self {
        static N: AtomicUsize = AtomicUsize::new(0);
        let root = PathBuf::from("/tmp").join(format!(
            "bsctl-aero-{}-{}-{}",
            std::process::id(),
            name,
            N.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&root);
        let (run, fix, fakebin) = (root.join("run"), root.join("fix"), root.join("fakebin"));
        for d in [
            &run,
            &fix,
            &fakebin,
            &root.join("home"),
            &root.join("state"),
        ] {
            fs::create_dir_all(d).unwrap();
        }
        let windows = windows
            .to_string()
            .replace("\"{PID}\"", &std::process::id().to_string());
        fs::write(
            fix.join("workspaces"),
            json!([
                {"workspace": "1", "workspace-is-visible": true, "workspace-is-focused": true, "monitor-id": 1, "monitor-name": "Built-in Retina Display"},
                {"workspace": "2", "workspace-is-visible": false, "workspace-is-focused": false, "monitor-id": 1, "monitor-name": "Built-in Retina Display"},
                {"workspace": "3", "workspace-is-visible": true, "workspace-is-focused": false, "monitor-id": 2, "monitor-name": "LG UltraFine"},
                {"workspace": "M", "workspace-is-visible": false, "workspace-is-focused": false, "monitor-id": 1, "monitor-name": "Built-in Retina Display"}
            ])
            .to_string(),
        )
        .unwrap();
        fs::write(fix.join("windows"), windows).unwrap();
        fs::write(
            fix.join("monitors"),
            json!([
                {"monitor-id": 1, "monitor-name": "Built-in Retina Display"},
                {"monitor-id": 2, "monitor-name": "LG UltraFine"}
            ])
            .to_string(),
        )
        .unwrap();
        fs::write(
            fix.join("focused-monitor"),
            json!([{"monitor-id": 1, "monitor-name": "Built-in Retina Display"}]).to_string(),
        )
        .unwrap();
        let stub = fakebin.join("aerospace");
        fs::write(
            &stub,
            format!(
                r#"#!/bin/sh
f='{fix}'
case "$1" in
  list-workspaces) cat "$f/workspaces" ;;
  list-windows) cat "$f/windows" ;;
  list-monitors) case " $* " in *" --focused "*) cat "$f/focused-monitor" ;; *) cat "$f/monitors" ;; esac ;;
  subscribe) exit 1 ;;
  *) printf '%s\n' "$*" >> "$f/actions.log" ;;
esac
"#,
                fix = fix.display()
            ),
        )
        .unwrap();
        let sec = fakebin.join("security");
        fs::write(&sec, "#!/bin/sh\nexit 44\n").unwrap();
        for p in [&stub, &sec] {
            let mut perm = fs::metadata(p).unwrap().permissions();
            std::os::unix::fs::PermissionsExt::set_mode(&mut perm, 0o755);
            fs::set_permissions(p, perm).unwrap();
        }
        let path = format!(
            "{}:{}",
            fakebin.display(),
            std::env::var("PATH").unwrap_or_default()
        );
        TestEnv {
            root,
            run,
            fix,
            path,
        }
    }

    fn cmd(&self) -> Command {
        let mut c = Command::new(BIN);
        c.env("BSCTL_WM", "aerospace")
            .env("XDG_RUNTIME_DIR", &self.run)
            .env("HOME", self.root.join("home"))
            .env("XDG_STATE_HOME", self.root.join("state"))
            .env("XDG_CACHE_HOME", self.root.join("cache"))
            .env("PATH", &self.path)
            .env_remove("KITTY_LISTEN_ON")
            .env_remove("KITTY_WINDOW_ID");
        c
    }

    fn run(&self, args: &[&str]) -> std::process::Output {
        self.cmd().args(args).output().unwrap()
    }

    fn status(&self) -> Value {
        let out = self.run(&["status", "--format", "json"]);
        assert_eq!(out.status.code(), Some(0));
        serde_json::from_slice(&out.stdout).unwrap()
    }

    fn actions(&self) -> Vec<String> {
        fs::read_to_string(self.fix.join("actions.log"))
            .unwrap_or_default()
            .lines()
            .map(str::to_string)
            .collect()
    }

    fn hook(&self, verb: &str, payload: &str) -> i32 {
        let mut child = self
            .cmd()
            .args(["agents", "set", "--kind", "claude", verb])
            .stdin(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(payload.as_bytes())
            .unwrap();
        child.wait().unwrap().code().unwrap_or(-1)
    }
}

impl Drop for TestEnv {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn two_windows() -> Value {
    json!([
        {"window-id": 4242, "app-pid": "{PID}", "app-name": "kitty", "workspace": "2"},
        {"window-id": 77, "app-pid": 1, "app-name": "Safari", "workspace": "1"}
    ])
}

#[test]
fn status_world_is_built_from_aerospace() {
    let env = TestEnv::new("world", two_windows());
    let w = env.status();
    assert_eq!(
        w["workspaces"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["ws"].as_i64().unwrap())
            .collect::<Vec<_>>(),
        vec![1, 2, 3],
        "numeric workspaces only, battlespace order = ascending by default"
    );
    assert_eq!(w["workspaces"][1]["windows"], 1);
    assert_eq!(w["workspaces"][2]["display"], "LG UltraFine");
    assert_eq!(w["workspaces"][0]["active"], true);
    assert_eq!(w["displays"][0]["name"], "Built-in Retina Display");
    assert_eq!(w["displays"][0]["focused"], true);
    assert_eq!(w["displays"][1]["activeWs"], 3);
}

#[test]
fn ws_verbs_translate_to_aerospace_commands() {
    let env = TestEnv::new("verbs", two_windows());
    assert_eq!(
        env.run(&["ws", "focus", "--bs-id", "2"]).status.code(),
        Some(0)
    );
    assert_eq!(
        env.run(&["ws", "send", "window", "--bs-id", "3", "--focus"])
            .status
            .code(),
        Some(0)
    );
    assert_eq!(
        env.run(&["ws", "focus", "--display-id", "2"]).status.code(),
        Some(0)
    );
    assert_eq!(
        env.actions(),
        vec![
            "workspace 2",
            "move-node-to-workspace --focus-follows-window 3",
            "focus-monitor 2",
        ]
    );
}

#[test]
fn renames_are_bsctl_owned_and_show_in_the_world() {
    let env = TestEnv::new("names", two_windows());
    assert_eq!(
        env.run(&["ws", "name", "set", "--ws-id", "2", "--name", "web ui"])
            .status
            .code(),
        Some(0)
    );
    assert!(env.actions().is_empty(), "AeroSpace has no rename to call");
    assert_eq!(env.status()["workspaces"][1]["name"], "web ui");
    assert_eq!(
        env.run(&["ws", "name", "rm", "--ws-id", "2"]).status.code(),
        Some(0)
    );
    assert_eq!(env.status()["workspaces"][1]["name"], Value::Null);
}

#[test]
fn hook_places_the_session_on_its_windows_workspace() {
    let env = TestEnv::new("hook", two_windows());
    let payload = json!({"session_id": "sess-a"}).to_string();
    assert_eq!(env.hook("thinking", &payload), 0);
    let rec: Value =
        serde_json::from_slice(&fs::read(env.run.join("battlestation-ws/sess-a")).unwrap())
            .unwrap();
    assert_eq!(rec["ws"], 2);
    assert_eq!(rec["win"], "4242");
    assert_eq!(rec["status"], "thinking");
    // Jump focuses the exact window.
    assert_eq!(
        env.run(&["ws", "focus", "--session", "sess-a"])
            .status
            .code(),
        Some(0)
    );
    assert_eq!(env.actions(), vec!["focus --window-id 4242"]);
}

#[test]
fn hyprland_only_verbs_refuse_under_aerospace() {
    let env = TestEnv::new("refuse", two_windows());
    let out = env.run(&["display", "set", "scale", "--up"]);
    assert_ne!(out.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&out.stderr).contains("needs Hyprland"));
}

#[test]
fn hook_records_the_kitty_route_privately_and_rows_carry_the_switchboard_name() {
    let env = TestEnv::new("route", two_windows());
    let mut child = env
        .cmd()
        .args(["agents", "set", "--kind", "claude", "waiting"])
        .env("KITTY_LISTEN_ON", "unix:/tmp/kitty-route-test")
        .env("KITTY_WINDOW_ID", "5")
        .stdin(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(json!({"session_id": "sess-k"}).to_string().as_bytes())
        .unwrap();
    assert_eq!(child.wait().unwrap().code(), Some(0));
    let rec: Value =
        serde_json::from_slice(&fs::read(env.run.join("battlestation-ws/sess-k")).unwrap())
            .unwrap();
    assert_eq!(
        rec["kitty"],
        json!({"to": "unix:/tmp/kitty-route-test", "window": 5})
    );
    // Name the session on the Switchboard; agent rows now carry it.
    let comms = env.run.join("battlestation-comms");
    fs::create_dir_all(&comms).unwrap();
    fs::write(
        comms.join("comms.json"),
        json!({"next_id": 1, "names": {"sess-k": "web-ui"}, "links": [], "messages": []})
            .to_string(),
    )
    .unwrap();
    let out = env.run(&["agents", "get", "--format", "json"]);
    let rows: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(rows[0]["name"], "web-ui");
    assert!(rows[0].get("kitty").is_none(), "the route is private");
}
