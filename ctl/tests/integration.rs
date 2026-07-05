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
        // Fake hyprctl: one client per ancestor pid of the caller, all on
        // ws 42, each with a pid-derived window address (0xaddr<pid>) so
        // the win capture is assertable. Any OTHER query fails (exit 1, no
        // output) so watch's compositor queries take the degraded path
        // (compositor: null) instead of parsing a clients-shaped answer as
        // workspaces.
        let stub = fakebin.join("hyprctl");
        fs::write(
            &stub,
            "#!/bin/sh\n[ \"$1\" = clients ] || exit 1\npid=$PPID\nsep=''\nprintf '['\nwhile [ \"${pid:-0}\" -gt 1 ]; do\n  printf '%s{\"pid\": %s, \"workspace\": {\"id\": 42}, \"address\": \"0xaddr%s\"}' \"$sep\" \"$pid\" \"$pid\"\n  sep=,\n  pid=$(ps -o ppid= -p \"$pid\" 2>/dev/null | tr -d ' ')\ndone\nprintf ']'\n",
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
        self.run.join("battlestation-ws")
    }

    fn cmd(&self) -> Command {
        let mut c = Command::new(BIN);
        c.env("XDG_RUNTIME_DIR", &self.run)
            .env("HOME", &self.home)
            .env("XDG_STATE_HOME", self.root.join("state"))
            // Isolate the usage cache too: without this, the developer
            // machine's real XDG_CACHE_HOME leaks real plan usage into the
            // world (seen live the day usage joined the status schema).
            .env("XDG_CACHE_HOME", self.root.join("cache"))
            .env("PATH", &self.path)
            .env_remove("CLAUDE_WS_DEBUG")
            // Socket discovery must scan THIS env's <run>/hypr, not resolve
            // the developer machine's live instance under the test dir.
            .env_remove("HYPRLAND_INSTANCE_SIGNATURE");
        c
    }

    /// A well-wired Claude hook call (`--kind` is mandatory; the kindless
    /// form is pinned as a no-op in `kindless_hook_is_a_noop`).
    fn hook(&self, verb: &str, payload: &str) -> i32 {
        self.hook_args(&["agents", "set", "--kind", "claude", verb], payload)
    }

    /// `agents get` with extra args, parsed as one JSON value.
    fn agents_get(&self, extra: &[&str]) -> Value {
        let out = self
            .cmd()
            .args(["agents", "get", "--format", "json"])
            .args(extra)
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(0), "agents get must exit 0");
        serde_json::from_slice(&out.stdout).expect("agents get must print one JSON array")
    }

    fn hook_args(&self, args: &[&str], payload: &str) -> i32 {
        let mut child = self
            .cmd()
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        // A hook that never reads stdin (the kindless no-op) may exit
        // before this write lands; EPIPE is that contract working, not a
        // test failure.
        if let Err(e) = child.stdin.take().unwrap().write_all(payload.as_bytes()) {
            assert_eq!(e.kind(), std::io::ErrorKind::BrokenPipe, "{e}");
        }
        child.wait().unwrap().code().unwrap()
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
    // The matched clients row's address is captured alongside ws — the stub
    // derives it from the client pid, which is NOT the harness pid the
    // record carries (the first matching ANCESTOR wins the clients pass).
    let win = rec["win"].as_str().expect("win must be captured");
    assert!(win.starts_with("0xaddr"), "stub-shaped address: {win}");
    // term_pid is the matched clients row's OWN pid (the terminal owning the
    // window). The stub builds each row's address as 0xaddr<that-pid>, so the
    // captured term_pid must equal the pid embedded in win.
    let term_pid = rec["term_pid"].as_i64().expect("term_pid must be captured");
    assert_eq!(
        win,
        format!("0xaddr{term_pid}"),
        "term_pid == the win row's pid"
    );
    // And the published agents row exposes win, pid and term_pid.
    let rows = env.agents_get(&["--session-id", "sess-1"]);
    assert_eq!(rows[0]["win"], json!(win));
    assert_eq!(rows[0]["pid"], json!(pid)); // harness pid present-or-null; here the live ancestor
    assert_eq!(rows[0]["term_pid"], json!(term_pid));
}

#[test]
fn session_write_with_kind_codex() {
    let env = TestEnv::new("session-kind");
    let payload = json!({"session_id": "sess-k"}).to_string();
    assert_eq!(
        env.hook_args(&["agents", "set", "--kind", "codex", "tooling"], &payload),
        0
    );
    let rec = env.read_json("sess-k");
    assert_eq!(rec["kind"], json!("codex"));
    assert_eq!(rec["status"], json!("tooling"));
    // --kind=<k> form writes the same record
    assert_eq!(
        env.hook_args(&["agents", "set", "--kind=gemini", "waiting"], &payload),
        0
    );
    assert_eq!(env.read_json("sess-k")["kind"], json!("gemini"));
    // an empty --kind value is kindless -> no-op: the record is untouched
    assert_eq!(
        env.hook_args(&["agents", "set", "--kind=", "waiting"], &payload),
        0
    );
    assert_eq!(env.read_json("sess-k")["kind"], json!("gemini"));
}

#[test]
fn kindless_hook_is_a_noop() {
    // --kind is MANDATORY: a kindless verb (outdated wiring) must write
    // NOTHING, silently — even with a valid payload and working hyprctl —
    // so the outdated caller's sessions visibly stop updating.
    let env = TestEnv::new("kindless");
    let tp = env.make_transcript();
    let payload = json!({"session_id": "sess-nk", "transcript_path": tp}).to_string();
    for args in [
        &["agents", "set", "waiting"][..],           // no --kind at all
        &["agents", "set", "--kind=", "thinking"],   // empty kind value
        &["agents", "set", "--kind", "", "tooling"], // empty kind token
    ] {
        assert_eq!(env.hook_args(args, &payload), 0, "{args:?}");
    }
    // markers too: agent verbs are equally kind-gated
    let ap = json!({"session_id": "sess-nk", "agent_id": "a1"}).to_string();
    assert_eq!(env.hook_args(&["agents", "set", "subagent-start"], &ap), 0);
    let entries: Vec<_> = fs::read_dir(env.state_dir())
        .map(|rd| rd.flatten().map(|e| e.file_name()).collect())
        .unwrap_or_default();
    assert!(entries.is_empty(), "state dir must stay empty: {entries:?}");
}

#[test]
fn session_id_override_and_agents_get_filters() {
    let env = TestEnv::new("sid-override");
    // The argv override writes the record under ITS id even when the
    // payload carries none (the scripting/testing seam; harness hooks keep
    // delivering ids inside the event JSON).
    assert_eq!(
        env.hook_args(
            &[
                "agents",
                "set",
                "--kind",
                "codex",
                "waiting",
                "--session-id",
                "ovr-1"
            ],
            "{}",
        ),
        0
    );
    let rec = env.read_json("ovr-1");
    assert_eq!(rec["status"], json!("waiting"));
    assert_eq!(rec["kind"], json!("codex"));
    // ...and outranks a payload session_id when both are present.
    let payload = json!({"session_id": "payload-sid"}).to_string();
    assert_eq!(
        env.hook_args(
            &[
                "agents",
                "set",
                "--kind",
                "codex",
                "thinking",
                "--session-id=ovr-1"
            ],
            &payload,
        ),
        0
    );
    assert_eq!(env.read_json("ovr-1")["status"], json!("thinking"));
    assert!(!env.state_dir().join("payload-sid").exists());

    // agents get: the NEW schema (session/subagents), filterable.
    let all = env.agents_get(&[]);
    assert_eq!(all.as_array().unwrap().len(), 1);
    assert_eq!(all[0]["session"], json!("ovr-1"));
    assert_eq!(all[0]["kind"], json!("codex"));
    assert_eq!(all[0]["subagents"], json!([]));
    assert!(
        all[0].get("sid").is_none(),
        "the old sid spelling must be gone from agents get"
    );
    assert!(
        env.agents_get(&["--kind", "claude"])
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        env.agents_get(&["--session-id", "ovr-1"])
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn codex_title_derives_from_prompt_and_sticks() {
    // Codex transcripts carry no ai-title records; UserPromptSubmit carries
    // `prompt` + `cwd` instead. The derived title must land in the session
    // file and then STICK across later events that carry nothing.
    let env = TestEnv::new("codex-title");
    let prompt_ev = json!({
        "session_id": "cx-1",
        "cwd": "/home/u/dev/myproj",
        "prompt": "Fix the flaky test\nsecond line is ignored"
    })
    .to_string();
    assert_eq!(
        env.hook_args(
            &["agents", "set", "--kind", "codex", "thinking"],
            &prompt_ev
        ),
        0
    );
    let rec = env.read_json("cx-1");
    assert_eq!(rec["kind"], json!("codex"));
    assert_eq!(rec["title"], json!("myproj: Fix the flaky test"));

    // A following tool event (no prompt, no transcript title) keeps it.
    let tool_ev = json!({"session_id": "cx-1"}).to_string();
    assert_eq!(
        env.hook_args(&["agents", "set", "--kind", "codex", "tooling"], &tool_ev),
        0
    );
    let rec = env.read_json("cx-1");
    assert_eq!(rec["status"], json!("tooling"));
    assert_eq!(rec["title"], json!("myproj: Fix the flaky test"), "sticky");

    // A new prompt re-derives (payload beats sticky).
    let prompt2 = json!({
        "session_id": "cx-1", "cwd": "/home/u/dev/myproj", "prompt": "Now refactor"
    })
    .to_string();
    assert_eq!(
        env.hook_args(&["agents", "set", "--kind", "codex", "thinking"], &prompt2),
        0
    );
    assert_eq!(
        env.read_json("cx-1")["title"],
        json!("myproj: Now refactor")
    );
}

#[test]
fn transcript_title_beats_prompt_title() {
    // Claude events can carry BOTH a transcript ai-title and (hypothetically)
    // a prompt; the transcript scan wins the precedence chain.
    let env = TestEnv::new("title-precedence");
    let tp = env.make_transcript();
    let payload = json!({
        "session_id": "pr-1", "transcript_path": tp,
        "cwd": "/x/proj", "prompt": "loser prompt"
    })
    .to_string();
    assert_eq!(env.hook("thinking", &payload), 0);
    assert_eq!(
        env.read_json("pr-1")["title"],
        json!("Fix the widget poller")
    );
}

#[test]
fn session_write_without_hyprctl_writes_nothing() {
    let env = TestEnv::new("no-hyprctl");
    // PATH with no hyprctl at all: spawning it fails -> silent exit 0.
    let mut child = Command::new(BIN)
        .args(["agents", "set", "--kind", "claude", "thinking"])
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
    assert_eq!(env.hook("subagent-start", &payload), 0);
    let m = env.read_json("s1.a1");
    assert_eq!(
        m,
        json!({"type": "Explore", "description": "Search the tree"})
    );

    // subagent-stop removes exactly that marker
    assert_eq!(env.hook("subagent-stop", &payload), 0);
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
    assert_eq!(env.hook("subagent-start", &payload), 0);
    assert_eq!(
        env.read_json("s2.b2"),
        json!({"type": "fork", "description": "Do a thing"})
    );
    // No agent_id -> silent no-op
    assert_eq!(env.hook("subagent-start", r#"{"session_id":"s2"}"#), 0);
    assert_eq!(env.hook("subagent-stop", r#"{"session_id":"s2"}"#), 0);
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

// ---- agents get: the sweeping read ------------------------------------------

#[test]
fn agents_get_sweeps_dead_orphans_and_stale_markers() {
    let env = TestEnv::new("sweep");
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

    let out = env.agents_get(&[]);
    let arr = out.as_array().unwrap();
    assert_eq!(arr.len(), 1, "only the live session survives: {out}");
    let s = &arr[0];
    assert_eq!(s["session"], json!("live"));
    assert_eq!(s["ws"], json!(3));
    assert_eq!(s["status"], json!("tooling"));
    assert_eq!(s["kind"], json!("claude"));
    assert_eq!(s["title"], json!("T"));
    let agents = s["subagents"].as_array().unwrap();
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
fn agents_get_empty_or_missing_dir_prints_empty_array() {
    let env = TestEnv::new("get-empty");
    // state dir doesn't even exist yet
    assert_eq!(env.agents_get(&[]), json!([]));
}

// ---- injected-ws write path (the factored hyprctl seam) ----------------------

#[test]
fn write_session_unit_seam() {
    let env = TestEnv::new("write-seam");
    fs::create_dir_all(env.state_dir()).unwrap();
    bsctl::agents::write_session(
        &env.state_dir(),
        "seam",
        "waiting",
        "A title",
        7,
        Some("0xseam"),
        1,
        Some(4321),
        "claude",
    )
    .unwrap();
    assert_eq!(
        env.read_json("seam"),
        json!({"ws": 7, "win": "0xseam", "status": "waiting", "kind": "claude",
               "title": "A title", "pid": 1, "term_pid": 4321})
    );
    // and the scan picks it straight up (pid 1 alive)
    let out = bsctl::sessions::scan(
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

// ---- --stream ------------------------------------------------------------
// The engine side: spawn the built binary's streaming form against the
// TestEnv and assert the emission stream CONVERGES (bounded channel waits,
// never unbounded reads). `status --format json --stream` is the widget's
// subscription; `agents get --stream` proves the per-query forms share the
// engine.

/// A spawned streaming query: the child plus a channel of its parsed
/// emissions (a reader thread pumps stdout lines). Killed on drop so a
/// panicking test never leaks a subscriber.
struct Streamer {
    child: std::process::Child,
    lines: std::sync::mpsc::Receiver<Value>,
}

impl Streamer {
    fn spawn(env: &TestEnv, args: &[&str]) -> Self {
        let mut child = env
            .cmd()
            .args(args)
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let stdout = child.stdout.take().unwrap();
        let (tx, lines) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            use std::io::BufRead;
            for line in std::io::BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                let Ok(v) = serde_json::from_str::<Value>(&line) else {
                    break;
                };
                if tx.send(v).is_err() {
                    break;
                }
            }
        });
        Streamer { child, lines }
    }

    /// The next emission, bounded; panics with `what` on timeout.
    fn next(&self, what: &str) -> Value {
        self.lines
            .recv_timeout(Duration::from_secs(5))
            .unwrap_or_else(|_| panic!("timed out waiting for {what}"))
    }

    /// Pull emissions until one satisfies `pred` (a burst may land as one
    /// or several emissions — coalescing is a latency knob, not a schema
    /// promise); bounded per pull, so a stalled stream panics with `what`.
    fn converge(&self, what: &str, pred: impl Fn(&Value) -> bool) -> Value {
        for _ in 0..20 {
            let v = self.next(what);
            if pred(&v) {
                return v;
            }
        }
        panic!("stream never converged: {what}");
    }

    /// Assert NO emission arrives within `ms` — the absence proof (the one
    /// spot a bounded wait is unavoidable).
    fn expect_silence(&self, ms: u64, what: &str) {
        if let Ok(v) = self.lines.recv_timeout(Duration::from_millis(ms)) {
            panic!("unexpected emission during {what}: {v}");
        }
    }
}

impl Drop for Streamer {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
fn stream_status_converges_and_matches_agents_get() {
    let env = TestEnv::new("stream-converge");
    let s = Streamer::spawn(&env, &["status", "--format", "json", "--stream"]);

    // First emission is immediate, even with a freshly created state dir.
    // No Hyprland in this env: the compositor sections are null (NOT [] —
    // a consumer keeps its last state), prefs/agents honestly empty.
    let first = s.next("initial world");
    assert_eq!(first["displays"], Value::Null);
    assert_eq!(first["workspaces"], Value::Null);
    assert_eq!(first["prefs"], json!([]));
    assert_eq!(first["agents"], json!([]));
    // no credentials in this env: usage is {} (nothing known), never null —
    // and read without touching the network (no token, no curl).
    assert_eq!(first["usage"], json!({}));

    // A session file appearing (what a hook write looks like) must emit.
    env.write_state(
        "w1",
        r#"{"ws":5,"status":"thinking","kind":"claude","title":"T","pid":1}"#,
    );
    let v = s.converge("session w1 in the stream", |v| {
        v["agents"][0]["session"] == json!("w1")
    });
    assert_eq!(v["agents"][0]["status"], json!("thinking"));

    // A marker touched in -> subagents converge; and the agents section
    // must equal a fresh `agents get` over the same dir.
    env.write_state("w1.a1", r#"{"type":"explore","description":"d"}"#);
    let v = s.converge("marker w1.a1 in the stream", |v| {
        v["agents"][0]["subagents"]
            .as_array()
            .is_some_and(|a| a.len() == 1)
    });
    assert_eq!(v["agents"], env.agents_get(&[]));

    // Marker removed -> subagents empty; session removed -> back to [].
    fs::remove_file(env.state_dir().join("w1.a1")).unwrap();
    s.converge("marker removal reflected", |v| {
        v["agents"][0]["subagents"] == json!([])
    });
    fs::remove_file(env.state_dir().join("w1")).unwrap();
    s.converge("session removal reflected", |v| v["agents"] == json!([]));
}

#[test]
fn stream_emits_nothing_for_dotfiles_or_noise() {
    let env = TestEnv::new("stream-dotfiles");
    let s = Streamer::spawn(&env, &["status", "--format", "json", "--stream"]);
    s.next("initial world");

    // Churn only names the trigger filter must ignore: no emission may
    // arrive (dedupe would also stop a re-evaluated identical world, but
    // the filter stops the wake-up itself).
    env.write_state(".scratch.tmp", "{}");
    env.write_state("debug.log", "=== noise");
    s.expect_silence(300, "dotfile/debug.log churn");
}

#[test]
fn two_streamers_serve_two_subscribers() {
    // No election gates the OUTPUT anymore (that died with .widget.json):
    // every subscriber gets the full stream. (The flock election gates only
    // the preference-apply dispatch, which this env never triggers.)
    let env = TestEnv::new("stream-two");
    let a = Streamer::spawn(&env, &["status", "--format", "json", "--stream"]);
    let b = Streamer::spawn(&env, &["status", "--format", "json", "--stream"]);
    a.next("first subscriber's initial world");
    b.next("second subscriber's initial world");
    env.write_state(
        "t1",
        r#"{"ws":1,"status":"waiting","kind":"claude","title":"","pid":1}"#,
    );
    for (name, s) in [("a", &a), ("b", &b)] {
        s.converge(&format!("subscriber {name} seeing t1"), |v| {
            v["agents"][0]["session"] == json!("t1")
        });
    }
}

#[test]
fn agents_get_stream_shares_the_engine() {
    let env = TestEnv::new("stream-agents");
    let s = Streamer::spawn(&env, &["agents", "get", "--format", "json", "--stream"]);
    assert_eq!(s.next("initial agents"), json!([]));
    env.write_state(
        "g1",
        r#"{"ws":2,"status":"tooling","kind":"codex","title":"","pid":1}"#,
    );
    let v = s.converge("g1 in the agents stream", |v| {
        v[0]["session"] == json!("g1")
    });
    assert_eq!(v[0]["kind"], json!("codex"));
}

#[test]
fn text_stream_frames_with_blank_lines_when_piped() {
    // --stream is orthogonal to --format: a text stream through a pipe
    // (stdout here is a pipe by construction — the tty clear-redraw path
    // needs a pty and stays untested live) renders the same tables as the
    // one-shot form, emissions separated by one blank line, the first
    // unseparated.
    let env = TestEnv::new("stream-text");
    env.write_state(
        "t1",
        r#"{"ws":3,"status":"waiting","kind":"claude","title":"","pid":1}"#,
    );
    let mut child = env
        .cmd()
        .args(["agents", "get", "--stream"]) // default --format text
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let stdout = child.stdout.take().unwrap();
    let (tx, lines) = std::sync::mpsc::channel::<String>();
    std::thread::spawn(move || {
        use std::io::BufRead;
        for line in std::io::BufReader::new(stdout).lines() {
            let Ok(line) = line else { break };
            if tx.send(line).is_err() {
                break;
            }
        }
    });
    let next = |what: &str| -> String {
        lines
            .recv_timeout(Duration::from_secs(5))
            .unwrap_or_else(|_| panic!("timed out waiting for {what}"))
    };

    // First emission: the table, unseparated — header then t1's row.
    assert!(
        next("first header").starts_with("KIND"),
        "first emission must open with the table header"
    );
    assert!(next("t1 row").starts_with("claude"));

    // A second session -> a new emission: exactly one blank separator,
    // then the re-rendered table.
    env.write_state(
        "t2",
        r#"{"ws":4,"status":"tooling","kind":"codex","title":"","pid":1}"#,
    );
    assert_eq!(next("separator"), "", "emissions must be blank-separated");
    assert!(next("second header").starts_with("KIND"));

    let _ = child.kill();
    let _ = child.wait();
}

// ---- --stream: compositor events (fake socket2) ------------------------------
// A fake Hyprland instance dir under <run>/hypr/<name>/ with BOTH sockets:
// `.socket.sock` answers `j/workspaces` / `j/monitors` from mutable canned
// JSON (one request per connection, like the real wire), `.socket2.sock`
// accepts the streaming engine and lets the test push `EVENT>>DATA` lines.

struct FakeHypr {
    ws_json: std::sync::Arc<std::sync::Mutex<String>>,
    mon_json: std::sync::Arc<std::sync::Mutex<String>>,
    events: std::os::unix::net::UnixListener,
}

impl FakeHypr {
    fn start(env: &TestEnv) -> Self {
        use std::io::Read;
        let inst = env.run.join("hypr/fake-instance");
        fs::create_dir_all(&inst).unwrap();
        let ws_json = std::sync::Arc::new(std::sync::Mutex::new(String::from("[]")));
        let mon_json = std::sync::Arc::new(std::sync::Mutex::new(String::from("[]")));
        let req = std::os::unix::net::UnixListener::bind(inst.join(".socket.sock")).unwrap();
        let (ws, mon) = (ws_json.clone(), mon_json.clone());
        // Request server: read to EOF (the client shuts down its write
        // side), answer, close. The thread parks in accept at test end and
        // dies with the process.
        std::thread::spawn(move || {
            for stream in req.incoming() {
                let Ok(mut s) = stream else { break };
                let mut cmd = String::new();
                if s.read_to_string(&mut cmd).is_err() {
                    continue;
                }
                let reply = match cmd.as_str() {
                    "j/workspaces" => ws.lock().unwrap().clone(),
                    "j/monitors" => mon.lock().unwrap().clone(),
                    _ => String::from("unknown request"),
                };
                let _ = s.write_all(reply.as_bytes());
            }
        });
        let events = std::os::unix::net::UnixListener::bind(inst.join(".socket2.sock")).unwrap();
        FakeHypr {
            ws_json,
            mon_json,
            events,
        }
    }

    fn set_state(&self, workspaces: &Value, monitors: &Value) {
        *self.ws_json.lock().unwrap() = workspaces.to_string();
        *self.mon_json.lock().unwrap() = monitors.to_string();
    }

    /// The engine's socket2 connection (bounded, so a subscriber that
    /// never connects fails the test instead of hanging it).
    fn accept_event_client(&self) -> std::os::unix::net::UnixStream {
        self.events.set_nonblocking(true).unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        loop {
            match self.events.accept() {
                Ok((s, _)) => {
                    s.set_nonblocking(false).unwrap();
                    return s;
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    assert!(
                        std::time::Instant::now() < deadline,
                        "the stream never connected to .socket2.sock"
                    );
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(e) => panic!("accept: {e}"),
            }
        }
    }
}

/// Raw hyprctl-shaped compositor JSON (extra fields included, to prove the
/// projection) for two states A and B; only B has workspace 7 on DP-2.
fn fake_state_a() -> (Value, Value) {
    (
        json!([
            {"id": 7, "name": "seven", "monitor": "DP-1", "monitorID": 0,
             "windows": 2, "hasfullscreen": false},
            {"id": -98, "name": "special:magic", "monitor": "DP-1",
             "monitorID": 0, "windows": 1, "hasfullscreen": false},
        ]),
        json!([
            {"name": "DP-1", "x": 0, "y": 0, "focused": true, "scale": 1.0,
             "activeWorkspace": {"id": 7, "name": "seven"},
             "specialWorkspace": {"id": 0, "name": ""}},
        ]),
    )
}

fn fake_state_b() -> (Value, Value) {
    (
        json!([
            {"id": 7, "name": "seven", "monitor": "DP-2", "monitorID": 1,
             "windows": 2, "hasfullscreen": false},
        ]),
        json!([
            {"name": "DP-1", "x": 0, "y": 0, "focused": false, "scale": 1.0,
             "activeWorkspace": {"id": 1, "name": "1"},
             "specialWorkspace": {"id": -99, "name": "special:magic"}},
            {"name": "DP-2", "x": 2304, "y": 0, "focused": true, "scale": 1.0,
             "activeWorkspace": {"id": 7, "name": "seven"},
             "specialWorkspace": {"id": 0, "name": ""}},
        ]),
    )
}

#[test]
fn stream_socket2_event_reemits_the_world() {
    let env = TestEnv::new("stream-socket2");
    let fake = FakeHypr::start(&env);
    let (ws_a, mon_a) = fake_state_a();
    fake.set_state(&ws_a, &mon_a);
    let s = Streamer::spawn(&env, &["status", "--format", "json", "--stream"]);
    let mut conn = fake.accept_event_client();

    // The first emission already carries the world PROJECTED to the schema:
    // special:* excluded, the battlespace join applied, extra fields gone.
    let first = s.next("initial world with compositor");
    assert_eq!(
        first["displays"],
        json!([{"id": 1, "name": "DP-1", "x": 0, "y": 0, "focused": true,
                "activeWs": 7, "specialShowing": false}])
    );
    assert_eq!(
        first["workspaces"],
        json!([{"ws": 7, "bs": 1, "name": "seven", "display": "DP-1",
                "windows": 2, "active": true, "pref": null}])
    );

    // Serve fresh state, then push a relevant event: the engine must
    // re-query and emit state B — no subscriber/timer involvement anywhere.
    let (ws_b, mon_b) = fake_state_b();
    fake.set_state(&ws_b, &mon_b);
    conn.write_all(b"moveworkspacev2>>7,seven,DP-2\n").unwrap();
    let v = s.converge("moveworkspace event re-emitting the world", |v| {
        v["workspaces"][0]["display"] == json!("DP-2")
    });
    assert_eq!(
        v["displays"],
        json!([{"id": 1, "name": "DP-1", "x": 0, "y": 0, "focused": false,
                "activeWs": 1, "specialShowing": true},
               {"id": 2, "name": "DP-2", "x": 2304, "y": 0, "focused": true,
                "activeWs": 7, "specialShowing": false}])
    );
    assert_eq!(v["agents"], json!([]));

    // Only noise on the socket -> no emission (relevance filter, then
    // dedupe as the second line of defense).
    conn.write_all(
        b"windowtitle>>555abc\nwindowtitlev2>>555abc,New Title\nactivewindow>>kitty,fish\n",
    )
    .unwrap();
    s.expect_silence(300, "irrelevant socket2 events");
}

#[test]
fn stream_reemits_on_map_file_edit() {
    // The engine's SECOND inotify watch: a `ws map set` from another
    // process lands in the persistent dir and must re-emit the world with
    // the new battlespace order — the widget's pill-drag path end to end.
    let env = TestEnv::new("stream-map-edit");
    let fake = FakeHypr::start(&env);
    fake.set_state(
        &json!([
            {"id": 7, "name": "seven", "monitor": "DP-1", "windows": 1},
            {"id": 9, "name": "nine", "monitor": "DP-1", "windows": 0},
        ]),
        &json!([
            {"name": "DP-1", "x": 0, "y": 0, "focused": true,
             "activeWorkspace": {"id": 7, "name": "seven"},
             "specialWorkspace": {"id": 0, "name": ""}},
        ]),
    );
    let s = Streamer::spawn(&env, &["status", "--format", "json", "--stream"]);
    let first = s.next("initial world");
    assert_eq!(first["workspaces"][0]["ws"], json!(7), "identity order");

    let code = env
        .cmd()
        .args(["ws", "map", "set", "9", "7"])
        .status()
        .unwrap();
    assert!(code.success());
    s.converge("map edit re-emitting the world", |v| {
        v["workspaces"][0]["ws"] == json!(9) && v["workspaces"][1]["ws"] == json!(7)
    });
}

#[test]
fn tool_verb_heals_empty_marker_fields_from_meta() {
    // SubagentStart can fire before the agent's meta.json exists (live race,
    // 2026-07-03): the marker is created with empty fields. A later tool-call
    // refresh must refill them from the by-then-written meta.json.
    let env = TestEnv::new("marker-heal");
    let tp = env.make_transcript();

    // subagent-start with a bare payload and NO meta.json yet -> empty fields
    let start = json!({"session_id": "s9", "agent_id": "h1", "transcript_path": tp}).to_string();
    assert_eq!(env.hook("subagent-start", &start), 0);
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
