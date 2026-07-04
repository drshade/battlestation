//! End-to-end tests for `bsctl asks` against an isolated runtime dir. The
//! asks verbs are pure file protocol (no compositor), so the fake hyprctl
//! here just fails every query — which also pins the degraded-mode shape of
//! `status` carrying the asks section.

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use serde_json::Value;

const BIN: &str = env!("CARGO_BIN_EXE_bsctl");

/// One isolated environment per test: its own XDG_RUNTIME_DIR (the asks
/// store), XDG_STATE_HOME (map/prefs, which `status` reads), HOME (no
/// credentials -> usage stays {} and off the network), and a PATH whose
/// fake hyprctl always fails (no live compositor can leak in).
struct TestEnv {
    root: PathBuf,
    run: PathBuf,
    path: String,
}

impl TestEnv {
    fn new(name: &str) -> Self {
        static N: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "bsctl-asks-{}-{}-{}",
            std::process::id(),
            name,
            N.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&root);
        let run = root.join("run");
        let fakebin = root.join("fakebin");
        for d in [&run, &root.join("state"), &root.join("home"), &fakebin] {
            fs::create_dir_all(d).unwrap();
        }
        let stub = fakebin.join("hyprctl");
        fs::write(&stub, "#!/bin/sh\nexit 1\n").unwrap();
        let mut perm = fs::metadata(&stub).unwrap().permissions();
        std::os::unix::fs::PermissionsExt::set_mode(&mut perm, 0o755);
        fs::set_permissions(&stub, perm).unwrap();
        let path = format!(
            "{}:{}",
            fakebin.display(),
            std::env::var("PATH").unwrap_or_default()
        );
        TestEnv { root, run, path }
    }

    fn cmd(&self) -> Command {
        let mut c = Command::new(BIN);
        c.env("XDG_RUNTIME_DIR", &self.run)
            .env("XDG_STATE_HOME", self.root.join("state"))
            .env("HOME", self.root.join("home"))
            .env("XDG_CACHE_HOME", self.root.join("cache"))
            .env("PATH", &self.path)
            .env_remove("HYPRLAND_INSTANCE_SIGNATURE");
        c
    }

    /// Run and return (exit code, stdout, stderr).
    fn run(&self, args: &[&str]) -> (i32, String, String) {
        let out = self.cmd().args(args).output().unwrap();
        (
            out.status.code().unwrap(),
            String::from_utf8_lossy(&out.stdout).into_owned(),
            String::from_utf8_lossy(&out.stderr).into_owned(),
        )
    }

    /// Post a minimal ask, returning its id.
    fn post(&self, title: &str, extra: &[&str]) -> i64 {
        let (code, out, err) = self.run(
            &[
                &["asks", "post", "--type", "question", "--title", title],
                extra,
            ]
            .concat(),
        );
        assert_eq!(code, 0, "post failed: {err}");
        out.trim()
            .strip_prefix("ask ")
            .unwrap_or_else(|| panic!("post must print `ask <id>`, got {out:?}"))
            .parse()
            .unwrap()
    }

    fn queue_json(&self, extra: &[&str]) -> Value {
        let (code, out, _) = self.run(&[&["asks", "get", "--format", "json"], extra].concat());
        assert_eq!(code, 0);
        serde_json::from_str(&out).unwrap()
    }
}

impl Drop for TestEnv {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn post_get_answer_roundtrip() {
    let env = TestEnv::new("roundtrip");
    // empty queue: text prints nothing, json prints []
    let (code, out, _) = env.run(&["asks", "get"]);
    assert_eq!((code, out.as_str()), (0, ""));
    assert_eq!(env.queue_json(&[]), serde_json::json!([]));

    let id = env.post(
        "Ship the release?",
        &[
            "--body",
            "v2 is green, tag it?",
            "--option",
            "ship",
            "--option",
            "hold",
            "--urgency",
            "high",
            "--estimate-min",
            "5",
            "--kind",
            "claude",
            "--session",
            "sess-1",
            "--ws",
            "3",
        ],
    );
    assert_eq!(id, 1);

    // table form: headers + the row, urgency/est/note dialects
    let (_, table, _) = env.run(&["asks", "get"]);
    let mut lines = table.lines();
    assert_eq!(
        lines.next().unwrap(),
        "ID  AGE  TYPE      URG   EST  WS  KIND    TITLE              NOTE  STATE"
    );
    let row = lines.next().unwrap();
    assert!(
        row.starts_with("1   0s   question  high  5m   3   claude  Ship the release?"),
        "unexpected row: {row}"
    );
    assert!(row.ends_with("open"));

    // json form: the full record in resolved order
    let q = env.queue_json(&[]);
    assert_eq!(q[0]["id"], 1);
    assert_eq!(q[0]["options"], serde_json::json!(["ship", "hold"]));
    assert_eq!(q[0]["estimate_min"], 5);
    assert_eq!(q[0]["answer"], Value::Null);

    // detail form: field-per-line
    let (_, detail, _) = env.run(&["asks", "get", "--id", "1"]);
    assert!(detail.contains("title    Ship the release?"), "{detail}");
    assert!(detail.contains("options  ship | hold"), "{detail}");
    assert!(detail.contains("state    open"), "{detail}");

    // answer: state flips, text lands, tail keeps it visible
    let (code, _, _) = env.run(&["asks", "answer", "1", "ship", "it"]);
    assert_eq!(code, 0);
    let q = env.queue_json(&[]);
    assert_eq!(q[0]["state"], "answered");
    assert_eq!(q[0]["answer"], "ship it");
    // answering again: error naming the state
    let (code, _, err) = env.run(&["asks", "answer", "1", "again"]);
    assert_eq!(code, 1);
    assert!(err.contains("answered"), "{err}");
    // unknown id: error
    let (code, _, err) = env.run(&["asks", "answer", "99", "x"]);
    assert_eq!(code, 1);
    assert!(err.contains("no ask 99"), "{err}");
}

#[test]
fn note_update_dismiss_semantics() {
    let env = TestEnv::new("verbs");
    env.post("a", &["--kind", "claude", "--session", "s-1"]);
    env.post("b", &["--kind", "codex", "--session", "s-2"]);

    // note: set then clear; the human's field
    assert_eq!(env.run(&["asks", "note", "1", "working", "on", "it"]).0, 0);
    assert_eq!(env.queue_json(&[])[0]["note"], "working on it");
    assert_eq!(env.run(&["asks", "note", "1"]).0, 0);
    assert_eq!(env.queue_json(&[])[0]["note"], "");

    // update: the agent-owned pair only; open asks only
    assert_eq!(
        env.run(&[
            "asks",
            "update",
            "2",
            "--urgency",
            "high",
            "--estimate-min",
            "1"
        ])
        .0,
        0
    );
    let q = env.queue_json(&[]);
    assert_eq!(q[1]["urgency"], "high");
    assert_eq!(q[1]["estimate_min"], 1);
    // at least one field required: clap usage error
    assert_eq!(env.run(&["asks", "update", "2"]).0, 2);

    // session filter narrows the queue
    let q = env.queue_json(&["--session", "s-2"]);
    assert_eq!(q.as_array().unwrap().len(), 1);
    assert_eq!(q[0]["id"], 2);

    // dismiss: gone from the queue, still visible via --id (any state)
    assert_eq!(env.run(&["asks", "dismiss", "1"]).0, 0);
    let q = env.queue_json(&[]);
    assert_eq!(q.as_array().unwrap().len(), 1);
    let (code, detail, _) = env.run(&["asks", "get", "--id", "1"]);
    assert_eq!(code, 0);
    assert!(detail.contains("state    dismissed"), "{detail}");
    // note/update on a dismissed ask: refused
    assert_eq!(env.run(&["asks", "note", "1", "x"]).0, 1);
    assert_eq!(env.run(&["asks", "update", "1", "--urgency", "low"]).0, 1);
    // dismiss is idempotent, unknown ids included
    assert_eq!(env.run(&["asks", "dismiss", "1"]).0, 0);
    assert_eq!(env.run(&["asks", "dismiss", "99"]).0, 0);
}

#[test]
fn human_order_overrides_fifo_for_open_asks() {
    let env = TestEnv::new("order");
    for t in ["a", "b", "c"] {
        env.post(t, &[]);
    }
    let ids = |q: &Value| -> Vec<i64> {
        q.as_array()
            .unwrap()
            .iter()
            .map(|r| r["id"].as_i64().unwrap())
            .collect()
    };
    // FIFO by default
    assert_eq!(ids(&env.queue_json(&[])), vec![1, 2, 3]);
    // the human reorders; unlisted asks keep FIFO after the listed ones
    assert_eq!(env.run(&["asks", "order", "set", "3", "1"]).0, 0);
    let (_, raw, _) = env.run(&["asks", "order", "get"]);
    assert_eq!(raw, "3 1\n");
    assert_eq!(ids(&env.queue_json(&[])), vec![3, 1, 2]);
    // answered asks fall to the tail even when listed
    assert_eq!(env.run(&["asks", "answer", "3", "done"]).0, 0);
    assert_eq!(ids(&env.queue_json(&[])), vec![1, 2, 3]);
}

#[test]
fn status_carries_the_asks_section() {
    let env = TestEnv::new("status");
    // empty queue: [] in json (file truth, never null), no text section
    let (_, out, _) = env.run(&["status", "--format", "json"]);
    let w: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(w["asks"], serde_json::json!([]));
    assert_eq!(w["displays"], Value::Null); // no compositor in this env
    let (_, text, _) = env.run(&["status"]);
    assert!(!text.contains("asks\n"), "no lonely asks section: {text}");

    env.post("Ship it?", &["--kind", "claude", "--ws", "3"]);
    let (_, out, _) = env.run(&["status", "--format", "json"]);
    let w: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(w["asks"][0]["title"], "Ship it?");
    // the text overview leads with the asks table
    let (_, text, _) = env.run(&["status"]);
    assert!(text.starts_with("asks\n  ID  AGE"), "{text}");
}

#[test]
fn concurrent_posts_get_distinct_ids() {
    let env = TestEnv::new("flock");
    // Eight posters racing on one store: the write lock must serialize the
    // read-modify-writes so every id is distinct and none is lost.
    let children: Vec<_> = (0..8)
        .map(|i| {
            env.cmd()
                .args([
                    "asks",
                    "post",
                    "--type",
                    "notify",
                    "--title",
                    &format!("t{i}"),
                ])
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
                .unwrap()
        })
        .collect();
    let mut ids: Vec<i64> = children
        .into_iter()
        .map(|c| {
            let out = c.wait_with_output().unwrap();
            assert_eq!(out.status.code(), Some(0));
            String::from_utf8_lossy(&out.stdout)
                .trim()
                .strip_prefix("ask ")
                .unwrap()
                .parse()
                .unwrap()
        })
        .collect();
    ids.sort_unstable();
    assert_eq!(ids, (1..=8).collect::<Vec<i64>>());
    assert_eq!(env.queue_json(&[]).as_array().unwrap().len(), 8);
}

// ---- --stream --------------------------------------------------------------

/// A spawned streaming query: the child plus a channel of its parsed
/// emissions. Killed on drop so a panicking test never leaks a subscriber.
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

    fn next(&self, what: &str) -> Value {
        self.lines
            .recv_timeout(Duration::from_secs(5))
            .unwrap_or_else(|_| panic!("timed out waiting for {what}"))
    }

    fn converge(&self, what: &str, pred: impl Fn(&Value) -> bool) -> Value {
        for _ in 0..20 {
            let v = self.next(what);
            if pred(&v) {
                return v;
            }
        }
        panic!("stream never converged: {what}");
    }
}

impl Drop for Streamer {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
fn asks_stream_wakes_on_posts_from_another_process() {
    let env = TestEnv::new("stream");
    let s = Streamer::spawn(&env, &["asks", "get", "--format", "json", "--stream"]);
    assert_eq!(s.next("initial empty queue"), serde_json::json!([]));

    // a post from ANOTHER process must wake the stream (the asks-dir
    // inotify watch), and the emission is the resolved queue
    env.post("wake up", &["--kind", "claude"]);
    let v = s.converge("posted ask in the stream", |v| v[0]["id"] == 1);
    assert_eq!(v[0]["title"], "wake up");

    // an answer wakes it again with the new state
    assert_eq!(env.run(&["asks", "answer", "1", "ok"]).0, 0);
    s.converge("answered ask in the stream", |v| {
        v[0]["state"] == "answered"
    });

    // the status stream carries the same rows
    let w = Streamer::spawn(&env, &["status", "--format", "json", "--stream"]);
    let first = w.next("initial world");
    assert_eq!(first["asks"][0]["state"], "answered");
}

#[test]
fn stream_refuses_id_detail() {
    let env = TestEnv::new("stream-id");
    let (code, _, err) = env.run(&["asks", "get", "--id", "1", "--stream"]);
    assert_eq!(code, 2, "streaming a single record is a usage error");
    assert!(err.contains("--id"), "{err}");
}
