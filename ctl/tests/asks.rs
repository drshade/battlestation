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

    /// Run with a stdin payload (the hook-surface verbs read the event JSON
    /// there); returns (exit code, stdout).
    fn run_stdin(&self, args: &[&str], payload: &str) -> (i32, String) {
        use std::io::Write;
        let mut child = self
            .cmd()
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(payload.as_bytes())
            .unwrap();
        let out = child.wait_with_output().unwrap();
        (
            out.status.code().unwrap(),
            String::from_utf8_lossy(&out.stdout).into_owned(),
        )
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
        "ID  AGE  TYPE      URG   EST  WS  KIND    TITLE              NOTE  STATE  BLOCKING"
    );
    let row = lines.next().unwrap();
    assert!(
        row.starts_with("1   0s   question  high  5m   3   claude  Ship the release?"),
        "unexpected row: {row}"
    );
    // BLOCKING is blank for a CLI-posted ask (nobody is parked on it).
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

    // answer: state flips, text lands, tail keeps it visible — and the
    // answer is NOT delivered (only the asker's own MCP collection stamps
    // that; every CLI path leaves it null)
    let (code, _, _) = env.run(&["asks", "answer", "1", "ship", "it"]);
    assert_eq!(code, 0);
    let q = env.queue_json(&[]);
    assert_eq!(q[0]["state"], "answered");
    assert_eq!(q[0]["answer"], "ship it");
    assert_eq!(q[0]["delivered_at"], Value::Null);
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
fn reply_complete_reopen_decoupling() {
    let env = TestEnv::new("decouple");
    let id = env.post("Needs thought", &["--session", "s-1"]);

    // reply drafts: text lands, state STAYS open (a draft must not release
    // a blocked asker — the MCP loop keys on state)
    let (code, _, _) = env.run(&["asks", "reply", "1", "half", "an", "answer"]);
    assert_eq!(code, 0);
    let q = env.queue_json(&[]);
    assert_eq!(q[0]["answer"], "half an answer");
    assert_eq!(q[0]["state"], "open");

    // reply updates repeatedly; empty reply clears back to null
    let (code, _, _) = env.run(&["asks", "reply", "1", "better", "answer"]);
    assert_eq!(code, 0);
    assert_eq!(env.queue_json(&[])[0]["answer"], "better answer");
    let (code, _, _) = env.run(&["asks", "reply", "1"]);
    assert_eq!(code, 0);
    assert_eq!(env.queue_json(&[])[0]["answer"], Value::Null);

    // complete with NO text: an ack-only completion is legitimate
    let (code, _, _) = env.run(&["asks", "complete", "1"]);
    assert_eq!(code, 0);
    let q = env.queue_json(&[]);
    assert_eq!(q[0]["state"], "answered");
    assert_eq!(q[0]["answer"], Value::Null);
    // complete on a non-open ask: error naming the state
    let (code, _, err) = env.run(&["asks", "complete", "1"]);
    assert_eq!((code, err.contains("answered")), (1, true), "{err}");

    // reopen: back to open, answered_at cleared; a draft set before
    // reopen survives the round-trip
    let (code, _, _) = env.run(&["asks", "reply", "1", "kept", "draft"]);
    assert_eq!(code, 0);
    let (code, _, _) = env.run(&["asks", "reopen", "1"]);
    assert_eq!(code, 0);
    let q = env.queue_json(&[]);
    assert_eq!(q[0]["state"], "open");
    assert_eq!(q[0]["answer"], "kept draft");
    assert_eq!(q[0]["answered_at"], Value::Null);
    // reopen on an open ask: error
    let (code, _, err) = env.run(&["asks", "reopen", "1"]);
    assert_eq!((code, err.contains("open")), (1, true), "{err}");

    // answer still composes reply + complete in one step
    let (code, _, _) = env.run(&["asks", "answer", &id.to_string(), "final"]);
    assert_eq!(code, 0);
    let q = env.queue_json(&[]);
    assert_eq!(q[0]["state"], "answered");
    assert_eq!(q[0]["answer"], "final");

    // reply on an answered ask: text updates, state stays answered
    let (code, _, _) = env.run(&["asks", "reply", "1", "typo", "fixed"]);
    assert_eq!(code, 0);
    let q = env.queue_json(&[]);
    assert_eq!(q[0]["answer"], "typo fixed");
    assert_eq!(q[0]["state"], "answered");

    // reply on a dismissed ask: out of the conversation
    env.run(&["asks", "dismiss", "1"]);
    let (code, _, err) = env.run(&["asks", "reply", "1", "too", "late"]);
    assert_eq!((code, err.contains("dismissed")), (1, true), "{err}");
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

// ---- presence (lives in the asks dir; see the presence contract) ------------

#[test]
fn presence_set_get_roundtrip_and_no_bump() {
    let env = TestEnv::new("presence");
    // before any report: unknown, exit 0 (the desk before hypridle fires)
    let (code, out, _) = env.run(&["presence", "get"]);
    assert_eq!((code, out.trim()), (0, "unknown"));
    let (_, out, _) = env.run(&["presence", "get", "--format", "json"]);
    let v: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["state"], "unknown");
    assert_eq!(v["since"], Value::Null);
    assert_eq!(v["idle_secs"], Value::Null);

    // idle with the listener's threshold: since backdates by --already, so
    // idle_secs never undercounts the quiet time before the fire
    assert_eq!(
        env.run(&["presence", "set", "idle", "--already", "120"]).0,
        0
    );
    let (_, out, _) = env.run(&["presence", "get", "--format", "json"]);
    let first: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(first["state"], "idle");
    let since = first["since"].as_f64().unwrap();
    assert!(first["idle_secs"].as_i64().unwrap() >= 120);

    // the SAME state again must not bump since (idle accumulates across
    // repeated on-timeout fires, --already ignored on re-fires)
    assert_eq!(
        env.run(&["presence", "set", "idle", "--already", "120"]).0,
        0
    );
    let (_, out, _) = env.run(&["presence", "get", "--format", "json"]);
    let again: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(again["since"].as_f64().unwrap(), since, "no-bump rule");

    // a transition takes a fresh since; ACTIVE reads as 0 seconds idle —
    // the direct answer, never null-as-no-data
    assert_eq!(env.run(&["presence", "set", "active"]).0, 0);
    let (_, out, _) = env.run(&["presence", "get", "--format", "json"]);
    let active: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(active["state"], "active");
    assert!(active["since"].as_f64().unwrap() >= since);
    assert_eq!(active["idle_secs"], 0);
    let (_, out, _) = env.run(&["presence", "get"]);
    assert_eq!(out.trim(), "active");

    // clap refuses junk states loudly (this is not the silent hook surface)
    assert_eq!(env.run(&["presence", "set", "asleep"]).0, 2);
}

#[test]
fn status_and_stream_carry_presence() {
    let env = TestEnv::new("presence-world");
    // unknown before the first report — and NO text section for it
    let (_, out, _) = env.run(&["status", "--format", "json"]);
    let w: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(w["human"]["state"], "unknown");
    let (_, text, _) = env.run(&["status"]);
    assert!(!text.contains("human"), "{text}");

    // a presence flip from another process wakes the status stream (the
    // report lives in the watched asks dir — no new machinery)
    let s = Streamer::spawn(&env, &["status", "--format", "json", "--stream"]);
    assert_eq!(s.next("initial world")["human"]["state"], "unknown");
    assert_eq!(env.run(&["presence", "set", "idle"]).0, 0);
    let v = s.converge("idle in the stream", |v| v["human"]["state"] == "idle");
    assert!(v["human"]["idle_secs"].as_i64().unwrap() >= 0);

    // and the text overview now shows the one-line human section
    let (_, text, _) = env.run(&["status"]);
    assert!(text.contains("human\n  idle"), "{text}");
}

#[test]
fn inbox_delivers_once_and_stamps() {
    let env = TestEnv::new("inbox");
    let mine = env.post("Pick a color", &["--session", "s-me", "--option", "red"]);
    let theirs = env.post("Foreign ask", &["--session", "s-other"]);
    let open_mine = env.post("Still open", &["--session", "s-me"]);
    assert_eq!(env.run(&["asks", "answer", &mine.to_string(), "red"]).0, 0);
    assert_eq!(
        env.run(&["asks", "answer", &theirs.to_string(), "blue"]).0,
        0
    );

    // No session resolvable: silent exit 0, nothing printed, nothing stamped.
    let (code, out) = env.run_stdin(&["asks", "inbox"], "{}");
    assert_eq!((code, out.as_str()), (0, ""));
    let (code, out) = env.run_stdin(&["asks", "inbox"], "not json");
    assert_eq!((code, out.as_str()), (0, ""));

    // The session's answered, undelivered ask is delivered: the envelope
    // carries the [asks] block, and delivered_at lands in the store.
    let payload = r#"{"session_id": "s-me"}"#;
    let (code, out) = env.run_stdin(&["asks", "inbox"], payload);
    assert_eq!(code, 0);
    let v: Value = serde_json::from_str(&out).expect("hookSpecificOutput envelope");
    assert_eq!(v["hookSpecificOutput"]["hookEventName"], "UserPromptSubmit");
    let ctx = v["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .unwrap();
    assert!(ctx.contains(&format!("Your ask #{mine}")), "{ctx}");
    assert!(ctx.contains("was answered: \"red\""), "{ctx}");
    assert!(ctx.contains("no need to re-raise"), "{ctx}");
    // the open ask and the foreign ask are NOT in the block
    assert!(!ctx.contains(&format!("#{open_mine}")), "{ctx}");
    assert!(!ctx.contains("blue"), "{ctx}");
    let (_, detail, _) = env.run(&["asks", "get", "--id", &mine.to_string()]);
    assert!(detail.contains("state    delivered"), "{detail}");
    // the foreign session's ask stays undelivered
    let (_, detail, _) = env.run(&["asks", "get", "--id", &theirs.to_string()]);
    assert!(detail.contains("state    answered"), "{detail}");

    // Second call: already delivered, nothing to say.
    let (code, out) = env.run_stdin(&["asks", "inbox"], payload);
    assert_eq!((code, out.as_str()), (0, ""));

    // The --session-id override outranks the payload (agents-set precedence)
    // and delivers the foreign session's answer to ITS owner.
    let (code, out) = env.run_stdin(
        &["asks", "inbox", "--session-id", "s-other"],
        r#"{"session_id": "s-me"}"#,
    );
    assert_eq!(code, 0);
    assert!(out.contains("blue"), "{out}");
    // A valueless --session-id swallows only its slot: payload key rules,
    // and s-me has nothing left -> silence (hook tolerance, exit 0).
    let (code, out) = env.run_stdin(&["asks", "inbox", "--session-id"], payload);
    assert_eq!((code, out.as_str()), (0, ""));

    // An ack-only completion (no reply text) delivers as an acknowledgment.
    let ack = env.post("Ack me", &["--session", "s-me"]);
    assert_eq!(env.run(&["asks", "complete", &ack.to_string()]).0, 0);
    let (_, out) = env.run_stdin(&["asks", "inbox"], payload);
    assert!(out.contains("completed without reply text"), "{out}");
}
