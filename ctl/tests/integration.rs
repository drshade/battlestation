//! End-to-end tests running the built binary against a temp state dir with
//! fabricated payloads. The hyprctl boundary is faked with a stub script
//! earlier on PATH that reports every ancestor of its caller as a client on
//! workspace 42 (whichever ancestor the hook collected, it matches).
//!
//! Pid conventions: 1 (init) is always alive, 4_000_000 is beyond
//! kernel.pid_max's default and therefore always dead.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, SystemTime};

use serde_json::{Value, json};

const BIN: &str = env!("CARGO_BIN_EXE_bsctl");

/// One isolated environment per test: its own XDG_RUNTIME_DIR, HOME, and a
/// PATH whose first entry holds the fake hyprctl.
struct TestEnv {
    root: PathBuf,
    run: PathBuf,  // XDG_RUNTIME_DIR
    home: PathBuf, // HOME (for ~/.claude/projects in poll GC)
    path: String,  // fakebin:$PATH
}

impl TestEnv {
    fn new(name: &str) -> Self {
        static N: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "bsctl-it-{}-{}-{}",
            std::process::id(),
            name,
            N.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&root);
        let run = root.join("run");
        let home = root.join("home");
        let fakebin = root.join("fakebin");
        for d in [&run, &home, &fakebin] {
            fs::create_dir_all(d).unwrap();
        }
        // Fake hyprctl: one client per ancestor pid of the caller, all on ws 42.
        let stub = fakebin.join("hyprctl");
        fs::write(
            &stub,
            "#!/bin/sh\npid=$PPID\nsep=''\nprintf '['\nwhile [ \"${pid:-0}\" -gt 1 ]; do\n  printf '%s{\"pid\": %s, \"workspace\": {\"id\": 42}}' \"$sep\" \"$pid\"\n  sep=,\n  pid=$(ps -o ppid= -p \"$pid\" 2>/dev/null | tr -d ' ')\ndone\nprintf ']'\n",
        )
        .unwrap();
        let mut perm = fs::metadata(&stub).unwrap().permissions();
        std::os::unix::fs::PermissionsExt::set_mode(&mut perm, 0o755);
        fs::set_permissions(&stub, perm).unwrap();
        let path = format!(
            "{}:{}",
            fakebin.display(),
            std::env::var("PATH").unwrap_or_default()
        );
        TestEnv {
            root,
            run,
            home,
            path,
        }
    }

    fn state_dir(&self) -> PathBuf {
        self.run.join("claude-ws")
    }

    fn cmd(&self) -> Command {
        let mut c = Command::new(BIN);
        c.env("XDG_RUNTIME_DIR", &self.run)
            .env("HOME", &self.home)
            .env("PATH", &self.path)
            .env_remove("CLAUDE_WS_DEBUG");
        c
    }

    fn hook(&self, verb: &str, payload: &str) -> i32 {
        let mut child = self
            .cmd()
            .args(["hook", verb])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(payload.as_bytes())
            .unwrap();
        child.wait().unwrap().code().unwrap()
    }

    fn poll(&self) -> Value {
        let out = self.cmd().arg("poll").output().unwrap();
        assert_eq!(out.status.code(), Some(0), "poll must exit 0");
        serde_json::from_slice(&out.stdout).expect("poll must print one JSON array")
    }

    fn read_json(&self, name: &str) -> Value {
        let p = self.state_dir().join(name);
        serde_json::from_slice(&fs::read(&p).unwrap_or_else(|e| panic!("{p:?}: {e}"))).unwrap()
    }

    fn write_state(&self, name: &str, contents: &str) {
        fs::create_dir_all(self.state_dir()).unwrap();
        fs::write(self.state_dir().join(name), contents).unwrap();
    }

    fn set_mtime(&self, name: &str, when: SystemTime) {
        let times = fs::FileTimes::new().set_accessed(when).set_modified(when);
        fs::File::options()
            .write(true)
            .open(self.state_dir().join(name))
            .unwrap()
            .set_times(times)
            .unwrap();
    }

    /// A transcript with two ai-title lines; the LAST one must win.
    fn make_transcript(&self) -> String {
        let dir = self.root.join("transcripts");
        fs::create_dir_all(&dir).unwrap();
        let p = dir.join("session.jsonl");
        fs::write(
            &p,
            concat!(
                r#"{"type":"user","text":"hi"}"#,
                "\n",
                r#"{"type":"ai-title","aiTitle":"Old title"}"#,
                "\n",
                r#"{"type":"ai-title","aiTitle":"  Fix the\t\twidget   poller "}"#,
                "\n"
            ),
        )
        .unwrap();
        p.to_string_lossy().into_owned()
    }
}

impl Drop for TestEnv {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

// ---- hook: session write ---------------------------------------------------

#[test]
fn session_write_with_title_and_injected_ws() {
    let env = TestEnv::new("session-write");
    let tp = env.make_transcript();
    let payload = json!({"session_id": "sess-1", "transcript_path": tp}).to_string();
    assert_eq!(env.hook("thinking", &payload), 0);
    let rec = env.read_json("sess-1");
    assert_eq!(rec["ws"], json!(42)); // injected by the fake hyprctl
    assert_eq!(rec["status"], json!("thinking"));
    assert_eq!(rec["kind"], json!("claude"));
    assert_eq!(rec["title"], json!("Fix the widget poller")); // last line, collapsed
    let pid = rec["pid"].as_i64().expect("pid must be an int");
    assert!(
        Path::new(&format!("/proc/{pid}")).exists(),
        "pid must be a live ancestor"
    );
}

#[test]
fn session_write_without_hyprctl_writes_nothing() {
    let env = TestEnv::new("no-hyprctl");
    // PATH with no hyprctl at all: spawning it fails -> silent exit 0.
    let mut child = Command::new(BIN)
        .args(["hook", "thinking"])
        .env("XDG_RUNTIME_DIR", &env.run)
        .env("PATH", env.root.join("nowhere").display().to_string())
        .stdin(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(br#"{"session_id":"sess-x"}"#)
        .unwrap();
    assert_eq!(child.wait().unwrap().code(), Some(0));
    assert!(!env.state_dir().join("sess-x").exists());
}

#[test]
fn bad_stdin_is_tolerated() {
    let env = TestEnv::new("bad-stdin");
    // No session_id -> silent no-op: bad JSON, empty stdin, and payloads
    // lacking the field must all exit 0 WITHOUT writing. (A "default" sid
    // fallback here once fabricated a phantom session file whose pid was the
    // live claude ancestor — unsweepable until that process died.)
    for payload in [
        "this is { not json",
        "",
        "{}",
        r#"{"transcript_path":"/nope"}"#,
    ] {
        assert_eq!(env.hook("waiting", payload), 0, "{payload:?}");
        assert_eq!(env.hook("tooling", payload), 0, "{payload:?}");
    }
    let entries: Vec<_> = fs::read_dir(env.state_dir())
        .map(|rd| rd.flatten().map(|e| e.file_name()).collect())
        .unwrap_or_default();
    assert!(entries.is_empty(), "state dir must stay empty: {entries:?}");
    // Unknown verbs are silently ignored.
    assert_eq!(env.hook("explode", "{}"), 0);
}

// ---- hook: agent markers ---------------------------------------------------

#[test]
fn agent_start_meta_json_fallback_and_stop() {
    let env = TestEnv::new("agent-meta");
    let tp = env.make_transcript();
    // meta.json lives at dirname(transcript)/<sid>/subagents/agent-<aid>.meta.json
    let meta_dir = Path::new(&tp).parent().unwrap().join("s1/subagents");
    fs::create_dir_all(&meta_dir).unwrap();
    fs::write(
        meta_dir.join("agent-a1.meta.json"),
        r#"{"description": "Search the tree", "agentType": "Explore"}"#,
    )
    .unwrap();

    let payload = json!({"session_id": "s1", "agent_id": "a1", "transcript_path": tp}).to_string();
    assert_eq!(env.hook("agent-start", &payload), 0);
    let m = env.read_json("s1.a1");
    assert_eq!(
        m,
        json!({"type": "Explore", "description": "Search the tree"})
    );

    // agent-stop removes exactly that marker
    assert_eq!(env.hook("agent-stop", &payload), 0);
    assert!(!env.state_dir().join("s1.a1").exists());
}

#[test]
fn agent_start_payload_fields_win_and_missing_id_is_noop() {
    let env = TestEnv::new("agent-payload");
    let payload = json!({
        "session_id": "s2", "agent_id": "b2",
        "agent_type": "fork", "description": "Do a thing"
    })
    .to_string();
    assert_eq!(env.hook("agent-start", &payload), 0);
    assert_eq!(
        env.read_json("s2.b2"),
        json!({"type": "fork", "description": "Do a thing"})
    );
    // No agent_id -> silent no-op
    assert_eq!(env.hook("agent-start", r#"{"session_id":"s2"}"#), 0);
    assert_eq!(env.hook("agent-stop", r#"{"session_id":"s2"}"#), 0);
    assert!(env.state_dir().join("s2.b2").exists());
}

#[test]
fn tool_verb_refreshes_existing_marker_and_recreates_missing() {
    let env = TestEnv::new("marker-refresh");
    let tp = env.make_transcript();
    let meta_dir = Path::new(&tp).parent().unwrap().join("s3/subagents");
    fs::create_dir_all(&meta_dir).unwrap();
    fs::write(
        meta_dir.join("agent-c3.meta.json"),
        r#"{"description": "From meta", "agentType": "general"}"#,
    )
    .unwrap();

    // Existing marker: content stays, mtime bumps.
    env.write_state("s3.c3", r#"{"type": "orig", "description": "orig"}"#);
    let old = SystemTime::now() - Duration::from_secs(3600);
    env.set_mtime("s3.c3", old);
    let payload = json!({"session_id": "s3", "agent_id": "c3", "transcript_path": tp}).to_string();
    assert_eq!(env.hook("tooling", &payload), 0);
    let mtime = fs::metadata(env.state_dir().join("s3.c3"))
        .unwrap()
        .modified()
        .unwrap();
    assert!(
        mtime > old + Duration::from_secs(3000),
        "mtime must be bumped"
    );
    assert_eq!(
        env.read_json("s3.c3"),
        json!({"type": "orig", "description": "orig"}),
        "refresh must not rewrite content"
    );

    // Missing marker: recreated from meta.json (description never from payload).
    fs::remove_file(env.state_dir().join("s3.c3")).unwrap();
    assert_eq!(env.hook("tooling", &payload), 0);
    assert_eq!(
        env.read_json("s3.c3"),
        json!({"type": "general", "description": "From meta"})
    );
}

// ---- hook: clear -------------------------------------------------------------

#[test]
fn clear_sweeps_session_and_markers_only() {
    let env = TestEnv::new("clear");
    env.write_state(
        "s4",
        r#"{"ws":1,"status":"waiting","kind":"claude","title":"","pid":1}"#,
    );
    env.write_state("s4.a", r#"{"type":"x","description":"y"}"#);
    env.write_state("s4.b", r#"{"type":"x","description":"y"}"#);
    env.write_state(
        "other",
        r#"{"ws":2,"status":"waiting","kind":"claude","title":"","pid":1}"#,
    );
    env.write_state("other.a", r#"{"type":"x","description":"y"}"#);
    assert_eq!(env.hook("clear", r#"{"session_id":"s4"}"#), 0);
    assert!(!env.state_dir().join("s4").exists());
    assert!(!env.state_dir().join("s4.a").exists());
    assert!(!env.state_dir().join("s4.b").exists());
    assert!(env.state_dir().join("other").exists());
    assert!(env.state_dir().join("other.a").exists());
}

// ---- poll --------------------------------------------------------------------

#[test]
fn poll_sweeps_dead_orphans_and_stale_markers() {
    let env = TestEnv::new("poll");
    let now = SystemTime::now();
    let stale = now - Duration::from_secs(40 * 60);

    // live session (pid 1 = init, always alive) with two agents
    env.write_state(
        "live",
        r#"{"ws":3,"status":"tooling","kind":"claude","title":"T","pid":1}"#,
    );
    env.write_state("live.a1", r#"{"type":"explore","description":"d1"}"#);
    env.write_state("live.a2", r#"{"type":"fork","description":"d2"}"#);
    env.set_mtime("live.a2", now - Duration::from_secs(10)); // older -> sorts first

    // stale marker, no transcript anywhere under HOME -> GC'd
    env.write_state("live.gone", r#"{"type":"x","description":"leaked"}"#);
    env.set_mtime("live.gone", stale);

    // stale marker rescued by a FRESH subagent transcript under
    // ~/.claude/projects/*/live/subagents/agent-rescued.jsonl
    env.write_state(
        "live.rescued",
        r#"{"type":"y","description":"still going"}"#,
    );
    env.set_mtime("live.rescued", stale);
    let tdir = env.home.join(".claude/projects/some-proj/live/subagents");
    fs::create_dir_all(&tdir).unwrap();
    fs::write(tdir.join("agent-rescued.jsonl"), "{}\n").unwrap();

    // dead session (pid 4000000 > default pid_max) + its marker -> swept
    env.write_state(
        "dead",
        r#"{"ws":9,"status":"waiting","kind":"claude","title":"","pid":4000000}"#,
    );
    env.write_state("dead.z", r#"{"type":"x","description":"x"}"#);

    // unparseable session file -> swept
    env.write_state("legacy", "3\twaiting\told-format");

    // orphan marker (no session file) -> swept
    env.write_state("ghost.q", r#"{"type":"x","description":"x"}"#);

    // dotfiles + debug.log ignored
    env.write_state(".live.tmp", "{}");
    env.write_state("debug.log", "=== noise");

    let out = env.poll();
    let arr = out.as_array().unwrap();
    assert_eq!(arr.len(), 1, "only the live session survives: {out}");
    let s = &arr[0];
    assert_eq!(s["sid"], json!("live"));
    assert_eq!(s["ws"], json!(3));
    assert_eq!(s["status"], json!("tooling"));
    assert_eq!(s["kind"], json!("claude"));
    assert_eq!(s["title"], json!("T"));
    let agents = s["agents"].as_array().unwrap();
    let ids: Vec<&str> = agents.iter().map(|a| a["id"].as_str().unwrap()).collect();
    // rescued has the oldest mtime, then a2, then a1 — sorted by (started, id)
    assert_eq!(ids, vec!["rescued", "a2", "a1"]);
    for a in agents {
        assert!(a["started"].is_i64());
        assert!(a["type"].is_string() && a["description"].is_string());
    }

    let sd = env.state_dir();
    assert!(!sd.join("dead").exists(), "dead-pid session swept");
    assert!(!sd.join("dead.z").exists(), "dead session's marker swept");
    assert!(!sd.join("legacy").exists(), "unparseable session swept");
    assert!(!sd.join("ghost.q").exists(), "orphan marker swept");
    assert!(!sd.join("live.gone").exists(), "stale marker GC'd");
    assert!(
        sd.join("live.rescued").exists(),
        "fresh transcript rescues marker"
    );
    assert!(sd.join(".live.tmp").exists(), "dotfiles untouched");
    assert!(sd.join("debug.log").exists(), "debug.log untouched");
}

#[test]
fn poll_empty_or_missing_dir_prints_empty_array() {
    let env = TestEnv::new("poll-empty");
    // state dir doesn't even exist yet
    assert_eq!(env.poll(), json!([]));
}

// ---- injected-ws write path (the factored hyprctl seam) ----------------------

#[test]
fn write_session_unit_seam() {
    let env = TestEnv::new("write-seam");
    fs::create_dir_all(env.state_dir()).unwrap();
    bsctl::hook::write_session(&env.state_dir(), "seam", "waiting", "A title", 7, 1).unwrap();
    assert_eq!(
        env.read_json("seam"),
        json!({"ws": 7, "status": "waiting", "kind": "claude", "title": "A title", "pid": 1})
    );
    // and the poller picks it straight up (pid 1 alive)
    let out = bsctl::poll::poll(
        bsctl_now(),
        &env.state_dir(),
        &env.home.join(".claude/projects"),
    );
    assert_eq!(out.len(), 1);
    assert_eq!(out[0]["ws"], json!(7));
}

fn bsctl_now() -> f64 {
    SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs_f64()
}

// ---- watch ---------------------------------------------------------------
// The daemon side: spawn the built binary's `watch` against the TestEnv
// state dir, poke the dir, and assert `.widget.json` CONVERGES (bounded
// waits on conditions, not fixed sleeps) to what `poll` would emit.

/// A spawned `bsctl watch`, killed on drop so a panicking test never leaks
/// a daemon holding the lock.
struct Watcher(std::process::Child);

impl Watcher {
    fn spawn(env: &TestEnv) -> Self {
        Watcher(
            env.cmd()
                .arg("watch")
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap(),
        )
    }
    fn kill(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

impl Drop for Watcher {
    fn drop(&mut self) {
        self.kill();
    }
}

/// Poll `cond` every 10ms for up to 5s; panic with `what` on timeout.
fn wait_for(mut cond: impl FnMut() -> bool, what: &str) {
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while std::time::Instant::now() < deadline {
        if cond() {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("timed out waiting for {what}");
}

/// Parsed `.widget.json`, or None while missing/in-flight.
fn widget_json(env: &TestEnv) -> Option<Value> {
    let b = fs::read(env.state_dir().join(".widget.json")).ok()?;
    serde_json::from_slice(&b).ok()
}

#[test]
fn watch_converges_to_poll_output() {
    let env = TestEnv::new("watch-converge");
    let _w = Watcher::spawn(&env);

    // On acquiring the lock the daemon writes once immediately, even with
    // an empty (freshly created) state dir.
    wait_for(
        || widget_json(&env) == Some(json!([])),
        "initial empty .widget.json",
    );
    let raw = fs::read(env.state_dir().join(".widget.json")).unwrap();
    assert_eq!(raw, b"[]\n", "exactly poll's array + trailing newline");

    // A session file appearing (what a hook write looks like) must show up.
    env.write_state(
        "w1",
        r#"{"ws":5,"status":"thinking","kind":"claude","title":"T","pid":1}"#,
    );
    wait_for(
        || widget_json(&env).is_some_and(|v| v[0]["sid"] == json!("w1")),
        "session w1 in .widget.json",
    );

    // A marker touched in -> agents list converges; and the whole file must
    // equal a fresh `poll` over the same dir.
    env.write_state("w1.a1", r#"{"type":"explore","description":"d"}"#);
    wait_for(
        || {
            widget_json(&env)
                .is_some_and(|v| v[0]["agents"].as_array().is_some_and(|a| a.len() == 1))
        },
        "marker w1.a1 in .widget.json",
    );
    assert_eq!(widget_json(&env).unwrap(), env.poll());

    // Marker removed -> agents empty; session removed -> back to [].
    fs::remove_file(env.state_dir().join("w1.a1")).unwrap();
    wait_for(
        || widget_json(&env).is_some_and(|v| v[0]["agents"] == json!([])),
        "marker removal reflected",
    );
    fs::remove_file(env.state_dir().join("w1")).unwrap();
    wait_for(
        || widget_json(&env) == Some(json!([])),
        "session removal reflected",
    );
}

#[test]
fn watch_ignores_dotfiles_and_unchanged_output() {
    let env = TestEnv::new("watch-dotfiles");
    let _w = Watcher::spawn(&env);
    wait_for(|| widget_json(&env).is_some(), "initial .widget.json");

    // Plant a sentinel mtime on the output, then churn only names the
    // filter must ignore. A rewrite would clobber the sentinel, so a
    // surviving sentinel proves no rewrite happened (the one spot a bounded
    // sleep is unavoidable — we're asserting an absence).
    let past = SystemTime::now() - Duration::from_secs(1000);
    env.set_mtime(".widget.json", past);
    env.write_state(".scratch.tmp", "{}");
    env.write_state("debug.log", "=== noise");
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(
        fs::metadata(env.state_dir().join(".widget.json"))
            .unwrap()
            .modified()
            .unwrap(),
        past,
        "dotfile/debug.log events must not rewrite the output"
    );
}

#[test]
fn watch_failover_between_two_instances() {
    let env = TestEnv::new("watch-failover");
    let mut a = Watcher::spawn(&env);
    // A wins the lock (proven by the file appearing) ...
    wait_for(|| widget_json(&env).is_some(), "winner's first write");
    // ... so B can only block in flock as the standby.
    let _b = Watcher::spawn(&env);

    env.write_state(
        "f1",
        r#"{"ws":1,"status":"waiting","kind":"claude","title":"","pid":1}"#,
    );
    wait_for(
        || widget_json(&env).is_some_and(|v| v[0]["sid"] == json!("f1")),
        "winner tracking state",
    );

    // Kill the winner; the standby must acquire the lock and take over —
    // only B is alive to observe this write.
    a.kill();
    env.write_state(
        "f2",
        r#"{"ws":2,"status":"tooling","kind":"claude","title":"","pid":1}"#,
    );
    wait_for(
        || {
            widget_json(&env).is_some_and(|v| {
                v.as_array()
                    .is_some_and(|arr| arr.iter().any(|s| s["sid"] == json!("f2")))
            })
        },
        "survivor taking over the file",
    );
}

#[test]
fn tool_verb_heals_empty_marker_fields_from_meta() {
    // SubagentStart can fire before the agent's meta.json exists (live race,
    // 2026-07-03): the marker is created with empty fields. A later tool-call
    // refresh must refill them from the by-then-written meta.json.
    let env = TestEnv::new("marker-heal");
    let tp = env.make_transcript();

    // agent-start with a bare payload and NO meta.json yet -> empty fields
    let start = json!({"session_id": "s9", "agent_id": "h1", "transcript_path": tp}).to_string();
    assert_eq!(env.hook("agent-start", &start), 0);
    assert_eq!(
        env.read_json("s9.h1"),
        json!({"type": "", "description": ""})
    );

    // meta.json appears afterwards
    let meta_dir = Path::new(&tp).parent().unwrap().join("s9/subagents");
    fs::create_dir_all(&meta_dir).unwrap();
    fs::write(
        meta_dir.join("agent-h1.meta.json"),
        r#"{"description": "Late meta", "agentType": "general-purpose"}"#,
    )
    .unwrap();

    // a tool-call refresh heals both fields (payload agent_type wins for type)
    let refresh = json!({
        "session_id": "s9", "agent_id": "h1",
        "agent_type": "general-purpose", "transcript_path": tp
    })
    .to_string();
    assert_eq!(env.hook("tooling", &refresh), 0);
    assert_eq!(
        env.read_json("s9.h1"),
        json!({"type": "general-purpose", "description": "Late meta"})
    );

    // healed markers are left alone by further refreshes (touch only)
    fs::write(
        meta_dir.join("agent-h1.meta.json"),
        r#"{"description": "Changed later", "agentType": "other"}"#,
    )
    .unwrap();
    assert_eq!(env.hook("tooling", &refresh), 0);
    assert_eq!(
        env.read_json("s9.h1"),
        json!({"type": "general-purpose", "description": "Late meta"})
    );
}
