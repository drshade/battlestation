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
        self.server_with_kind("claude", block_secs)
    }

    /// [`Self::server`] with an explicit kind — the delivery tests pass the
    /// test process's own comm so identity resolution finds a real session.
    fn server_with_kind(&self, kind: &str, block_secs: u64) -> McpClient {
        let mut child = self
            .cmd()
            .args([
                "mcp",
                "--kind",
                kind,
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
            instructions: String::new(),
        };
        let init = c.request(
            "initialize",
            json!({"protocolVersion": "2025-06-18",
                   "capabilities": {}, "clientInfo": {"name": "test", "version": "0"}}),
        );
        assert_eq!(init["protocolVersion"], "2025-06-18");
        c.instructions = init["instructions"].as_str().unwrap_or("").to_string();
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
    /// The initialize result's instructions — the identity-sentence tests
    /// read it after the handshake dance.
    instructions: String,
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
            "world",
            "whoami"
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
    // Delivery stamp point 1: the block returned the answer to its asker,
    // so delivered_at lands — and the human's table now says so.
    let row: Value =
        serde_json::from_str(&env.asks(&["get", "--id", "1", "--format", "json"])).unwrap();
    assert!(row["delivered_at"].is_f64(), "block return must stamp");
    assert_eq!(row["state"], "answered", "delivery is not a third state");
    let table = env.asks(&["get"]);
    assert!(table.contains("delivered"), "{table}");
}

#[test]
fn get_ask_delivery_stamps_own_session_only() {
    let env = TestEnv::new("deliver-own");
    // Identity fabrication: the server resolves its session by walking its
    // /proc ancestors for comm == kind, then matching that pid against the
    // session files. This test process IS the server's ancestor, so a kind
    // equal to our own comm plus a session file recording our pid gives
    // the server a real identity ("sess-own").
    let comm = fs::read_to_string("/proc/self/comm").unwrap();
    let kind = comm.trim().to_string();
    let ws_dir = env.run.join("battlestation-ws");
    fs::create_dir_all(&ws_dir).unwrap();
    fs::write(
        ws_dir.join("sess-own"),
        json!({"ws": 7, "status": "waiting", "kind": kind, "title": "",
               "pid": std::process::id()})
        .to_string(),
    )
    .unwrap();

    // Two answered asks: one posted by sess-own, one by a stranger.
    env.asks(&[
        "post",
        "--type",
        "question",
        "--title",
        "mine",
        "--session",
        "sess-own",
    ]);
    env.asks(&[
        "post",
        "--type",
        "question",
        "--title",
        "theirs",
        "--session",
        "sess-other",
    ]);
    env.asks(&["answer", "1", "yes"]);
    env.asks(&["answer", "2", "no"]);
    let undelivered = |id: &str| {
        let r: Value =
            serde_json::from_str(&env.asks(&["get", "--id", id, "--format", "json"])).unwrap();
        r["delivered_at"].is_null()
    };
    // CLI writes and reads (the answers + the gets above) never stamp.
    assert!(
        undelivered("1") && undelivered("2"),
        "CLI paths must not stamp"
    );

    let mut c = env.server_with_kind(&kind, 0);
    // A FOREIGN answered ask collected by this session: no stamp.
    let (text, _) = c.call("get_ask", json!({"id": 2}));
    let r: Value = serde_json::from_str(&text).unwrap();
    assert!(r["delivered_at"].is_null(), "foreign peek is not delivery");
    assert!(undelivered("2"));
    // OUR OWN answered ask: stamped, and the response carries the stamp.
    let (text, _) = c.call("get_ask", json!({"id": 1}));
    let r: Value = serde_json::from_str(&text).unwrap();
    assert!(r["delivered_at"].is_f64(), "own collection stamps: {r}");
    let first = r["delivered_at"].as_f64().unwrap();
    // Idempotent: a second collection keeps the FIRST stamp.
    std::thread::sleep(std::time::Duration::from_millis(50));
    let (text, _) = c.call("get_ask", json!({"id": 1}));
    let r: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(
        r["delivered_at"].as_f64().unwrap(),
        first,
        "first delivery wins"
    );
    // Reopen clears the stamp: the next completion is undelivered again.
    env.asks(&["reopen", "1"]);
    assert!(undelivered("1"), "reopen must clear delivered_at");
}

#[test]
fn empty_identity_never_stamps_delivery() {
    let env = TestEnv::new("deliver-empty");
    let mut c = env.server(0); // kind claude, no session files: identity ""
    // The empty==empty accident: an ask with an empty session (this very
    // server posted it, unresolved) collected by the same empty-identity
    // server must NOT read as "own".
    c.call("ask", json!({"title": "anon", "wait_secs": 0}));
    env.asks(&["answer", "1", "ok"]);
    let (text, _) = c.call("get_ask", json!({"id": 1}));
    let r: Value = serde_json::from_str(&text).unwrap();
    assert!(
        r["delivered_at"].is_null(),
        "empty identity must never stamp: {r}"
    );
}

#[test]
fn whoami_and_mine_label_own_asks() {
    let env = TestEnv::new("whoami");
    // The delivery tests' identity fabrication: comm-matched kind + a
    // session file recording this process's pid = a resolved identity.
    let comm = fs::read_to_string("/proc/self/comm").unwrap();
    let kind = comm.trim().to_string();
    let ws_dir = env.run.join("battlestation-ws");
    fs::create_dir_all(&ws_dir).unwrap();
    fs::write(
        ws_dir.join("sess-own"),
        json!({"ws": 7, "win": "0xabc123", "status": "waiting", "kind": kind,
               "title": "", "pid": std::process::id()})
        .to_string(),
    )
    .unwrap();
    // Identity resolved BEFORE spawn -> the handshake instructions name it.
    let mut c = env.server_with_kind(&kind, 0);
    assert!(
        c.instructions.contains("You are session sess-own on ws 7."),
        "{}",
        c.instructions
    );

    // whoami: the resolved identity, win included.
    let (text, is_err) = c.call("whoami", json!({}));
    assert!(!is_err);
    let who: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(who["session"], "sess-own");
    assert_eq!(who["kind"], kind);
    assert_eq!(who["ws"], 7);
    assert_eq!(who["win"], "0xabc123");

    // Three asks: own, foreign, session-less.
    env.asks(&[
        "post",
        "--type",
        "question",
        "--title",
        "mine",
        "--session",
        "sess-own",
    ]);
    env.asks(&[
        "post",
        "--type",
        "question",
        "--title",
        "theirs",
        "--session",
        "s-other",
    ]);
    env.asks(&["post", "--type", "question", "--title", "anon"]);
    let (text, _) = c.call("list_asks", json!({}));
    let rows: Value = serde_json::from_str(&text).unwrap();
    let mine_of = |title: &str| {
        rows.as_array()
            .unwrap()
            .iter()
            .find(|r| r["title"] == title)
            .unwrap()["mine"]
            .clone()
    };
    assert_eq!(mine_of("mine"), true);
    assert_eq!(mine_of("theirs"), false);
    assert_eq!(mine_of("anon"), false, "session-less rows belong to nobody");
    // get_ask carries the label too.
    let (text, _) = c.call("get_ask", json!({"id": 1}));
    let r: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(r["mine"], true);
    let (text, _) = c.call("get_ask", json!({"id": 2}));
    let r: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(r["mine"], false);
}

#[test]
fn whoami_unresolved_is_honest_nulls() {
    let env = TestEnv::new("whoami-null");
    let mut c = env.server(0); // no session files: unresolved forever
    assert!(
        !c.instructions.contains("You are session"),
        "unresolved identity must not be promised at handshake: {}",
        c.instructions
    );
    let (text, is_err) = c.call("whoami", json!({}));
    assert!(!is_err);
    let who: Value = serde_json::from_str(&text).unwrap();
    assert!(who["session"].is_null());
    assert_eq!(who["kind"], "claude");
    assert!(who["ws"].is_null());
    assert!(who["win"].is_null());
    // and every row reads mine: false under an unresolved identity
    env.asks(&["post", "--type", "notify", "--title", "x"]);
    let (text, _) = c.call("list_asks", json!({}));
    let rows: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(rows[0]["mine"], false);
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
fn draft_reply_does_not_release_a_blocked_ask() {
    let env = TestEnv::new("draft-block");
    let mut c = env.server(0);
    let ask_req = c.send_request(
        "tools/call",
        json!({"name": "ask", "arguments": {"title": "Draft me", "wait_secs": 20}}),
    );
    std::thread::sleep(std::time::Duration::from_millis(600));
    // A draft lands (reply, state stays open) — the block must NOT release:
    // prove the server is still parked by pinging and getting the pong as
    // the first line out (several store polls have passed by then).
    env.asks(&["reply", "1", "thinking about it..."]);
    std::thread::sleep(std::time::Duration::from_millis(1200));
    let ping_req = c.send_request("ping", json!({}));
    let line = c.read_line();
    assert_eq!(
        line["id"].as_i64(),
        Some(ping_req),
        "a draft must keep the ask parked (got {line})"
    );
    // Completing is the releasing transition; the collected text is the draft.
    env.asks(&["complete", "1"]);
    let resp = c.read_line();
    assert_eq!(resp["id"].as_i64(), Some(ask_req));
    let text = resp["result"]["content"][0]["text"].as_str().unwrap();
    assert_eq!(text, "The human answered ask #1: thinking about it...");
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
    // The ask outlives the cancelled transport: still open in the store —
    // and no longer blocking (cancellation is one of the clear paths).
    let row: Value =
        serde_json::from_str(&env.asks(&["get", "--id", "1", "--format", "json"])).unwrap();
    assert_eq!(row["state"], "open");
    assert_eq!(row["blocking"], false, "cancel must clear the park marker");
    let _ = ask_req;
}

#[test]
fn parked_ask_reads_blocking_until_the_block_exits() {
    let env = TestEnv::new("blocking-flag");
    let mut c = env.server(10);
    // A concurrent reader mid-park must see blocking:true — then answer,
    // releasing the block (the answered exit path).
    let checker = {
        let cmd_env: Vec<(String, String)> = [
            ("XDG_RUNTIME_DIR", env.run.display().to_string()),
            ("PATH", env.path.clone()),
        ]
        .iter()
        .map(|(k, v)| (k.to_string(), v.clone()))
        .collect();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(800));
            let run = |args: &[&str]| {
                let mut c = Command::new(BIN);
                for (k, v) in &cmd_env {
                    c.env(k, v);
                }
                c.args(args).output().unwrap()
            };
            let out = run(&["asks", "get", "--id", "1", "--format", "json"]);
            let row: Value =
                serde_json::from_str(String::from_utf8_lossy(&out.stdout).trim()).unwrap();
            assert_eq!(row["blocking"], true, "mid-park must read blocking");
            assert!(
                row.get("waiting_pid").is_none(),
                "the pid is internal bookkeeping, never published"
            );
            let out = run(&["asks", "answer", "1", "done"]);
            assert!(out.status.success());
        })
    };
    let (text, is_err) = c.call("ask", json!({"title": "Park me"}));
    checker.join().unwrap();
    assert!(!is_err);
    assert!(text.contains("done"), "{text}");
    // Answered exit cleared the marker.
    let row: Value =
        serde_json::from_str(&env.asks(&["get", "--id", "1", "--format", "json"])).unwrap();
    assert_eq!(row["blocking"], false, "answer must clear the park marker");
    // BLOCKING renders as a table column while parked... and the store
    // itself never leaks the pid through the queue rows either.
    assert!(env.asks(&["get"]).contains("BLOCKING"));
}

#[test]
fn wait_zero_never_parks_and_timeout_clears() {
    let env = TestEnv::new("no-park");
    // wait_secs 0: posts and returns — never marked.
    let mut c = env.server(0);
    let (text, is_err) = c.call("ask", json!({"title": "Fire and forget", "wait_secs": 0}));
    assert!(!is_err);
    assert!(text.contains("Posted ask #1"), "{text}");
    let row: Value =
        serde_json::from_str(&env.asks(&["get", "--id", "1", "--format", "json"])).unwrap();
    assert_eq!(row["blocking"], false, "wait 0 never parks");
    // A real (short) wait that expires: the timeout exit clears the marker.
    let (text, is_err) = c.call("ask", json!({"title": "Expire me", "wait_secs": 1}));
    assert!(!is_err);
    assert!(text.contains("remains open"), "{text}");
    let row: Value =
        serde_json::from_str(&env.asks(&["get", "--id", "2", "--format", "json"])).unwrap();
    assert_eq!(row["blocking"], false, "timeout must clear the park marker");
}

#[test]
fn wait_expiry_mentions_idle_presence() {
    let env = TestEnv::new("idle-timeout");
    // Fabricate a 5-and-a-bit-minute idle report where presence lives (the
    // asks dir): the timeout text must tell the waiting agent about it.
    let asks_dir = env.run.join("battlestation-asks");
    fs::create_dir_all(&asks_dir).unwrap();
    let since = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs_f64()
        - 330.0;
    fs::write(
        asks_dir.join("presence.json"),
        json!({"state": "idle", "since": since}).to_string(),
    )
    .unwrap();
    let mut c = env.server(0);
    let (text, is_err) = c.call("ask", json!({"title": "Anyone home?", "wait_secs": 1}));
    assert!(!is_err);
    assert!(text.contains("remains open"), "{text}");
    assert!(text.contains("idle 5m"), "{text}");
    // an ACTIVE report earns no mention — no signal beats noise
    fs::write(
        asks_dir.join("presence.json"),
        json!({"state": "active", "since": since}).to_string(),
    )
    .unwrap();
    let (text, _) = c.call("ask", json!({"title": "Again?", "wait_secs": 1}));
    assert!(text.contains("remains open"), "{text}");
    assert!(!text.contains("idle"), "{text}");
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
