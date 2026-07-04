//! End-to-end tests for `bsctl mcp` — the binary spawned with piped stdio,
//! driven by scripted JSON-RPC lines. Isolated env: no session files exist,
//! so identity resolution degrades to empty session / null ws (the expected
//! headless case); the fake hyprctl fails every query, pinning `world`'s
//! degraded shape.

use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};

use serde_json::{Value, json};

const BIN: &str = env!("CARGO_BIN_EXE_bsctl");

struct TestEnv {
    root: PathBuf,
    run: PathBuf,
    path: String,
}

impl TestEnv {
    fn new(name: &str) -> Self {
        static N: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "bsctl-mcp-{}-{}-{}",
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

    /// Spawn the MCP server and run the initialize dance.
    fn server(&self, block_secs: u64) -> McpClient {
        let mut child = self
            .cmd()
            .args([
                "mcp",
                "--kind",
                "claude",
                "--block-secs",
                &block_secs.to_string(),
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let stdin = child.stdin.take().unwrap();
        let stdout = BufReader::new(child.stdout.take().unwrap());
        let mut c = McpClient {
            child,
            stdin,
            stdout,
            next_id: 1,
        };
        let init = c.request(
            "initialize",
            json!({"protocolVersion": "2025-06-18",
                   "capabilities": {}, "clientInfo": {"name": "test", "version": "0"}}),
        );
        assert_eq!(init["protocolVersion"], "2025-06-18");
        c.notify("notifications/initialized", json!({}));
        c
    }

    /// Run a one-shot asks CLI verb in this env.
    fn asks(&self, args: &[&str]) -> String {
        let out = self.cmd().arg("asks").args(args).output().unwrap();
        assert!(out.status.success(), "asks {args:?} failed");
        String::from_utf8(out.stdout).unwrap()
    }
}

impl Drop for TestEnv {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

struct McpClient {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next_id: i64,
}

impl McpClient {
    /// Send one request, read lines until ITS response arrives (progress
    /// notifications in between are collected and ignored here).
    fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.next_id;
        self.next_id += 1;
        let line =
            json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}).to_string();
        writeln!(self.stdin, "{line}").unwrap();
        loop {
            let mut buf = String::new();
            assert!(
                self.stdout.read_line(&mut buf).unwrap() > 0,
                "server closed before responding to {method}"
            );
            let v: Value = serde_json::from_str(&buf).unwrap();
            if v.get("id").and_then(Value::as_i64) == Some(id) {
                assert!(v.get("error").is_none(), "{method} errored: {}", v["error"]);
                return v["result"].clone();
            }
        }
    }

    fn notify(&mut self, method: &str, params: Value) {
        let line = json!({"jsonrpc": "2.0", "method": method, "params": params}).to_string();
        writeln!(self.stdin, "{line}").unwrap();
    }

    /// Fire a request WITHOUT waiting for its response (the mid-block
    /// tests drive send and read separately). Returns the request id.
    fn send_request(&mut self, method: &str, params: Value) -> i64 {
        let id = self.next_id;
        self.next_id += 1;
        let line =
            json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}).to_string();
        writeln!(self.stdin, "{line}").unwrap();
        id
    }

    /// Read exactly one server line.
    fn read_line(&mut self) -> Value {
        let mut buf = String::new();
        assert!(
            self.stdout.read_line(&mut buf).unwrap() > 0,
            "server closed unexpectedly"
        );
        serde_json::from_str(&buf).unwrap()
    }

    /// tools/call sugar: returns (text, isError).
    fn call(&mut self, name: &str, args: Value) -> (String, bool) {
        let r = self.request("tools/call", json!({"name": name, "arguments": args}));
        let text = r["content"][0]["text"].as_str().unwrap_or("").to_string();
        (text, r["isError"].as_bool().unwrap_or(false))
    }
}

impl Drop for McpClient {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
fn handshake_tools_and_the_norm() {
    let env = TestEnv::new("handshake");
    let mut c = env.server(0);
    let tools = c.request("tools/list", json!({}));
    let names: Vec<&str> = tools["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        [
            "ask",
            "notify",
            "list_asks",
            "get_ask",
            "update_ask",
            "world"
        ]
    );
    for t in tools["tools"].as_array().unwrap() {
        assert!(!t["description"].as_str().unwrap().trim().is_empty());
    }
    // the norm reaches the model through ask's description
    let ask_desc = tools["tools"][0]["description"].as_str().unwrap();
    assert!(ask_desc.contains("MUST post an ask"));
    // ping works
    let pong = c.request("ping", json!({}));
    assert_eq!(pong, json!({}));
}

#[test]
fn ask_posts_store_first_and_reports_open_on_no_block() {
    let env = TestEnv::new("ask-noblock");
    let mut c = env.server(0);
    let (text, is_err) = c.call(
        "ask",
        json!({"title": "Pick a color", "body": "red or blue?",
               "options": ["red", "blue"], "urgency": "high", "estimate_min": 2}),
    );
    assert!(!is_err);
    // --block-secs 0 defaults omitted wait_secs to fire-and-forget
    assert!(text.contains("Posted ask #1"), "{text}");
    // the store has it, with identity degraded to empty session
    let row: Value =
        serde_json::from_str(&env.asks(&["get", "--id", "1", "--format", "json"])).unwrap();
    assert_eq!(row["title"], "Pick a color");
    assert_eq!(row["type"], "question");
    assert_eq!(row["urgency"], "high");
    assert_eq!(row["estimate_min"], 2);
    assert_eq!(row["kind"], "claude");
    assert_eq!(row["session"], "");
    assert_eq!(row["state"], "open");
}

#[test]
fn ask_fast_path_returns_the_answer_mid_block() {
    let env = TestEnv::new("ask-block");
    let mut c = env.server(10);
    // answer from "another terminal" shortly after the ask posts
    let answerer = {
        let cmd_env: Vec<(String, String)> = [
            ("XDG_RUNTIME_DIR", env.run.display().to_string()),
            ("PATH", env.path.clone()),
        ]
        .iter()
        .map(|(k, v)| (k.to_string(), v.clone()))
        .collect();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(800));
            let mut c = Command::new(BIN);
            for (k, v) in &cmd_env {
                c.env(k, v);
            }
            let out = c
                .args(["asks", "answer", "1", "go", "with", "blue"])
                .output()
                .unwrap();
            assert!(out.status.success());
        })
    };
    let (text, is_err) = c.call("ask", json!({"title": "Pick a color"}));
    answerer.join().unwrap();
    assert!(!is_err);
    assert_eq!(text, "The human answered ask #1: go with blue");
}

#[test]
fn dismissal_mid_block_says_proceed_on_judgment() {
    let env = TestEnv::new("ask-dismiss");
    let mut c = env.server(10);
    let dismisser = {
        let run = env.run.display().to_string();
        let path = env.path.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(800));
            let out = Command::new(BIN)
                .env("XDG_RUNTIME_DIR", run)
                .env("PATH", path)
                .args(["asks", "dismiss", "1"])
                .output()
                .unwrap();
            assert!(out.status.success());
        })
    };
    let (text, is_err) = c.call("ask", json!({"title": "Really?"}));
    dismisser.join().unwrap();
    assert!(!is_err);
    assert!(text.contains("dismissed ask #1"), "{text}");
    assert!(text.contains("do not re-post"), "{text}");
}

#[test]
fn wait_secs_zero_overrides_the_server_default() {
    // Server default is a 30s block — an explicit wait_secs 0 must win and
    // return immediately with the collect-later text, not the timeout text.
    let env = TestEnv::new("wait-zero");
    let mut c = env.server(30);
    let started = std::time::Instant::now();
    let (text, is_err) = c.call("ask", json!({"title": "No rush", "wait_secs": 0}));
    assert!(!is_err);
    assert!(
        started.elapsed() < std::time::Duration::from_secs(5),
        "must not block"
    );
    assert!(text.contains("Posted ask #1"), "{text}");
    assert!(text.contains("not waiting"), "{text}");
    assert!(text.contains("get_ask"), "{text}");
    let row: Value =
        serde_json::from_str(&env.asks(&["get", "--id", "1", "--format", "json"])).unwrap();
    assert_eq!(row["state"], "open");
}

#[test]
fn ping_mid_block_is_ponged_while_the_ask_stays_parked() {
    let env = TestEnv::new("ping-block");
    let mut c = env.server(0); // default fire-and-forget; wait_secs opts in
    let ask_req = c.send_request(
        "tools/call",
        json!({"name": "ask", "arguments": {"title": "Long one", "wait_secs": 20}}),
    );
    std::thread::sleep(std::time::Duration::from_millis(600)); // block established
    let ping_req = c.send_request("ping", json!({}));
    // The pong arrives FIRST — the ask call is still parked.
    let pong = c.read_line();
    assert_eq!(pong["id"].as_i64(), Some(ping_req));
    assert_eq!(pong["result"], json!({}));
    // Now answer from outside; the parked call returns with the answer.
    env.asks(&["answer", "1", "proceed"]);
    let resp = c.read_line();
    assert_eq!(resp["id"].as_i64(), Some(ask_req));
    let text = resp["result"]["content"][0]["text"].as_str().unwrap();
    assert_eq!(text, "The human answered ask #1: proceed");
}

#[test]
fn cancellation_mid_block_is_honored_and_the_ask_survives() {
    let env = TestEnv::new("cancel-block");
    let mut c = env.server(0);
    let ask_req = c.send_request(
        "tools/call",
        json!({"name": "ask", "arguments": {"title": "Never mind", "wait_secs": 20}}),
    );
    std::thread::sleep(std::time::Duration::from_millis(600));
    c.notify("notifications/cancelled", json!({"requestId": ask_req}));
    // The server is back in its main loop: a ping answers, and the FIRST
    // line out is the pong — a cancelled request gets no response, ever.
    std::thread::sleep(std::time::Duration::from_millis(600));
    let ping_req = c.send_request("ping", json!({}));
    let line = c.read_line();
    assert_eq!(
        line["id"].as_i64(),
        Some(ping_req),
        "nothing may be emitted for the cancelled ask call (got {line})"
    );
    // The ask outlives the cancelled transport: still open in the store.
    let row: Value =
        serde_json::from_str(&env.asks(&["get", "--id", "1", "--format", "json"])).unwrap();
    assert_eq!(row["state"], "open");
    let _ = ask_req;
}

#[test]
fn get_ask_collects_a_late_answer() {
    let env = TestEnv::new("late-answer");
    let mut c = env.server(0);
    let (text, _) = c.call("ask", json!({"title": "Later question"}));
    assert!(text.contains("Posted ask #1"), "{text}");
    env.asks(&["answer", "1", "yes,", "ship", "it"]);
    let (text, is_err) = c.call("get_ask", json!({"id": 1}));
    assert!(!is_err);
    let rec: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(rec["state"], "answered");
    assert_eq!(rec["answer"], "yes, ship it");
    // unknown id is a tool-level error
    let (text, is_err) = c.call("get_ask", json!({"id": 99}));
    assert!(is_err);
    assert!(text.contains("no ask 99"));
}

#[test]
fn notify_returns_immediately_and_update_enforces_ownership() {
    let env = TestEnv::new("notify-update");
    let mut c = env.server(0);
    let (text, is_err) = c.call(
        "notify",
        json!({"title": "Review the diff", "type": "review", "estimate_min": 10}),
    );
    assert!(!is_err);
    assert!(text.contains("Posted ask #1"));
    let row: Value =
        serde_json::from_str(&env.asks(&["get", "--id", "1", "--format", "json"])).unwrap();
    assert_eq!(row["type"], "review");
    // a foreign-session ask (posted via CLI with an explicit session)
    env.asks(&[
        "post",
        "--type",
        "question",
        "--title",
        "someone else's",
        "--session",
        "s-other",
    ]);
    // own ask (session "" == our degraded identity): update ok
    let (_, is_err) = c.call("update_ask", json!({"id": 1, "urgency": "high"}));
    assert!(!is_err);
    let row: Value =
        serde_json::from_str(&env.asks(&["get", "--id", "1", "--format", "json"])).unwrap();
    assert_eq!(row["urgency"], "high");
    // foreign ask: refused
    let (text, is_err) = c.call("update_ask", json!({"id": 2, "urgency": "low"}));
    assert!(is_err);
    assert!(text.contains("not posted by this session"), "{text}");
    // nothing-to-update is a tool error, not a store write
    let (text, is_err) = c.call("update_ask", json!({"id": 1}));
    assert!(is_err);
    assert!(text.contains("nothing to update"));
}

#[test]
fn list_asks_and_world_return_parseable_json() {
    let env = TestEnv::new("reads");
    env.asks(&["post", "--type", "notify", "--title", "pre-existing"]);
    let mut c = env.server(0);
    let (text, is_err) = c.call("list_asks", json!({}));
    assert!(!is_err);
    let rows: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(rows.as_array().unwrap().len(), 1);
    assert_eq!(rows[0]["title"], "pre-existing");
    let (text, is_err) = c.call("world", json!({}));
    assert!(!is_err);
    let w: Value = serde_json::from_str(&text).unwrap();
    // degraded compositor -> null sections; file-truth sections present
    assert!(w["displays"].is_null());
    assert!(w["asks"].is_array());
    assert!(w["agents"].is_array());
    // unknown tool is a tool-level error
    let (text, is_err) = c.call("frobnicate", json!({}));
    assert!(is_err);
    assert!(text.contains("unknown tool"));
}
