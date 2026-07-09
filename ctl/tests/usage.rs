//! End-to-end tests for `bsctl agents usage` with a fake curl (captures
//! argv and stdin, serves a canned `body\n<code>` response), a fake
//! `codex app-server` (serves canned JSON-RPC lines over stdio), and fake
//! credentials — the network never enters the picture.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, SystemTime};

use serde_json::{Value, json};

const TOKEN: &str = "sk-test-oauth-token-12345";

/// One isolated environment per test: its own XDG_CACHE_HOME, a HOME holding
/// fake credentials, and a PATH whose first entry holds the fake curl.
struct TestEnv {
    root: PathBuf,
    cache_dir: PathBuf, // XDG_CACHE_HOME
    home: PathBuf,      // HOME (for ~/.claude/.credentials.json)
    fix: PathBuf,       // canned response + capture logs
    path: String,       // fakebin:$PATH
}

impl TestEnv {
    fn new(name: &str) -> Self {
        static N: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "bsctl-usage-{}-{}-{}",
            std::process::id(),
            name,
            N.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&root);
        let cache_dir = root.join("cache");
        let home = root.join("home");
        let fix = root.join("fix");
        let fakebin = root.join("fakebin");
        for d in [&cache_dir, &home.join(".claude"), &fix, &fakebin] {
            fs::create_dir_all(d).unwrap();
        }
        fs::write(
            home.join(".claude/.credentials.json"),
            json!({"claudeAiOauth": {"accessToken": TOKEN}}).to_string(),
        )
        .unwrap();
        // Fake curl: log argv and stdin, optionally stall (lock contention
        // test), then serve the canned response. Calls are counted in a
        // separate file because argv itself contains a newline (the -w
        // format), so argv.log lines != invocations.
        let stub = fakebin.join("curl");
        fs::write(
            &stub,
            format!(
                "#!/bin/sh\necho call >> '{fix}/curl-calls.log'\nprintf '%s\\n' \"$*\" >> '{fix}/curl-argv.log'\ncat >> '{fix}/curl-stdin.log'\n[ -f '{fix}/slow' ] && sleep 1\ncat '{fix}/response'\n",
                fix = fix.display()
            ),
        )
        .unwrap();
        let mut perm = fs::metadata(&stub).unwrap().permissions();
        std::os::unix::fs::PermissionsExt::set_mode(&mut perm, 0o755);
        fs::set_permissions(&stub, perm).unwrap();
        // Fake `codex app-server`: drains stdin in the background (the real
        // server doesn't wait for EOF before responding, so blocking on a
        // full read here would deadlock against bsctl, which holds stdin
        // open across the whole exchange), then — if a canned response was
        // staged — prints it. `codex-hold` makes it linger like the real
        // persistent server (bsctl must kill it); with neither file staged
        // it exits immediately, which is the default for every test that
        // never touches codex at all (auth.json absent -> never even spawned).
        let codex_stub = fakebin.join("codex");
        fs::write(
            &codex_stub,
            format!(
                "#!/bin/sh\necho call >> '{fix}/codex-calls.log'\ncat > '{fix}/codex-stdin.log' &\n[ -f '{fix}/codex-response' ] && cat '{fix}/codex-response'\n[ -f '{fix}/codex-hold' ] && sleep 100\n",
                fix = fix.display()
            ),
        )
        .unwrap();
        let mut perm = fs::metadata(&codex_stub).unwrap().permissions();
        std::os::unix::fs::PermissionsExt::set_mode(&mut perm, 0o755);
        fs::set_permissions(&codex_stub, perm).unwrap();
        let path = format!(
            "{}:{}",
            fakebin.display(),
            std::env::var("PATH").unwrap_or_default()
        );
        TestEnv {
            root,
            cache_dir,
            home,
            fix,
            path,
        }
    }

    /// Stages `~/.codex/auth.json` — the signal `codex_reading` gates on
    /// before ever spawning the fake `codex app-server`. Tests that don't
    /// call this never invoke it at all, by design.
    fn enable_codex(&self) {
        fs::create_dir_all(self.home.join(".codex")).unwrap();
        fs::write(self.home.join(".codex/auth.json"), "{}").unwrap();
    }

    /// Stages the fake app-server's canned stdout. `hold` mirrors the real
    /// server never exiting on its own (bsctl's kill()/reap must do the
    /// work); a response with no `id:2` line and `hold: false` is how the
    /// "app-server answered but never got to the rate-limits request"
    /// case is exercised without waiting out the real timeout.
    fn codex_respond(&self, lines: &[&str], hold: bool) {
        fs::write(self.fix.join("codex-response"), lines.join("\n") + "\n").unwrap();
        if hold {
            fs::write(self.fix.join("codex-hold"), "").unwrap();
        }
    }

    fn codex_cache(&self) -> PathBuf {
        self.cache_dir.join("codex-usage.json")
    }

    fn codex_calls(&self) -> usize {
        fs::read_to_string(self.fix.join("codex-calls.log"))
            .map(|s| s.lines().count())
            .unwrap_or(0)
    }

    /// `agents usage --format json` — the default form the tests exercise.
    fn cmd(&self) -> Command {
        let mut c = self.cmd_args(&["agents", "usage", "--format", "json"]);
        c.stdout(std::process::Stdio::piped());
        c
    }

    fn cmd_args(&self, args: &[&str]) -> Command {
        let mut c = Command::new(env!("CARGO_BIN_EXE_bsctl"));
        c.args(args)
            .env("XDG_CACHE_HOME", &self.cache_dir)
            .env("HOME", &self.home)
            .env("PATH", &self.path);
        c
    }

    fn run(&self) -> std::process::Output {
        self.cmd().output().unwrap()
    }

    fn run_args(&self, args: &[&str]) -> std::process::Output {
        self.cmd_args(args).output().unwrap()
    }

    fn cache(&self) -> PathBuf {
        self.cache_dir.join("claude-usage.json")
    }

    fn respond(&self, body: &str, status: &str) {
        fs::write(self.fix.join("response"), format!("{body}\n{status}")).unwrap();
    }

    fn write_cache(&self, content: &str, age: Duration) {
        self.write_cache_at(&self.cache(), content, age);
    }

    /// A stale (> TTL) codex cache — set up so a test can assert a failed
    /// refresh serves it back unchanged, same contract as claude's non-200
    /// tests.
    fn write_stale_codex_cache(&self, content: &str) {
        self.write_cache_at(&self.codex_cache(), content, Duration::from_secs(300));
    }

    fn write_cache_at(&self, path: &Path, content: &str, age: Duration) {
        fs::write(path, content).unwrap();
        let when = SystemTime::now() - age;
        let times = fs::FileTimes::new().set_accessed(when).set_modified(when);
        fs::File::options()
            .write(true)
            .open(path)
            .unwrap()
            .set_times(times)
            .unwrap();
    }

    fn curl_calls(&self) -> usize {
        fs::read_to_string(self.fix.join("curl-calls.log"))
            .map(|s| s.lines().count())
            .unwrap_or(0)
    }
}

impl Drop for TestEnv {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn usage_body() -> String {
    json!({
        "five_hour": {"utilization": 34.2, "resets_at": "2026-07-03T10:00:00Z"},
        "seven_day": {"utilization": 61.7, "resets_at": "2026-07-07T00:00:00Z"},
    })
    .to_string()
}

fn expected_reading() -> Value {
    json!({
        "sessionPct": 34,
        "sessionResets": "2026-07-03T10:00:00Z",
        "weeklyPct": 62,
        "weeklyResets": "2026-07-07T00:00:00Z",
    })
}

/// The published shape: readings indexed by harness kind.
fn expected_json() -> Value {
    json!({ "claude": expected_reading() })
}

// ---- cache behavior ----------------------------------------------------------

#[test]
fn fresh_cache_is_served_without_touching_the_network() {
    let env = TestEnv::new("fresh");
    env.write_cache("{\"sessionPct\":11}\n", Duration::from_secs(10));
    let out = env.run();
    assert_eq!(out.status.code(), Some(0));
    let printed: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(printed, json!({"claude": {"sessionPct": 11}}));
    assert_eq!(env.curl_calls(), 0, "fresh cache must skip curl entirely");
}

#[test]
fn expired_cache_refreshes_prints_and_rewrites_atomically() {
    let env = TestEnv::new("expired");
    env.write_cache("{\"sessionPct\":11}\n", Duration::from_secs(300)); // > ttl 240
    env.respond(&usage_body(), "200");
    let out = env.run();
    assert_eq!(out.status.code(), Some(0));
    let printed: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(printed, expected_json());
    // the cache holds the bare reading (the kind indexing is output shape,
    // not cache shape)
    let cached: Value = serde_json::from_slice(&fs::read(env.cache()).unwrap()).unwrap();
    assert_eq!(cached, expected_reading());
    assert_eq!(env.curl_calls(), 1);
}

#[test]
fn no_cache_at_all_fetches_and_creates_it() {
    let env = TestEnv::new("cold");
    env.respond(&usage_body(), "200");
    let out = env.run();
    assert_eq!(out.status.code(), Some(0));
    let printed: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(printed, expected_json());
    assert!(env.cache().is_file());
}

#[test]
fn non_200_serves_the_stale_cache_and_keeps_it() {
    // Stale beats absent: a rate-limited refresh serves the last good
    // value (and leaves the cache mtime old, so the next call retries).
    let env = TestEnv::new("non-200");
    env.write_cache("{\"sessionPct\":11}\n", Duration::from_secs(300));
    env.respond(r#"{"error": "rate limited"}"#, "429");
    let out = env.run();
    assert_eq!(out.status.code(), Some(0));
    let printed: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(printed, json!({"claude": {"sessionPct": 11}}));
    assert_eq!(
        fs::read(env.cache()).unwrap(),
        b"{\"sessionPct\":11}\n",
        "a failed refresh must never clobber the last good value"
    );
}

#[test]
fn malformed_200_body_serves_the_stale_cache() {
    let env = TestEnv::new("malformed");
    env.write_cache("{\"sessionPct\":11}\n", Duration::from_secs(300));
    for body in ["not json at all", r#"{"five_hour": 3, "seven_day": {}}"#] {
        env.respond(body, "200");
        let out = env.run();
        assert_eq!(out.status.code(), Some(0));
        let printed: Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(printed, json!({"claude": {"sessionPct": 11}}));
        assert_eq!(fs::read(env.cache()).unwrap(), b"{\"sessionPct\":11}\n");
    }
}

// ---- credentials & token hygiene ----------------------------------------------

#[test]
fn missing_credentials_exit_0_silently_without_curl() {
    let env = TestEnv::new("no-creds");
    fs::remove_file(env.home.join(".claude/.credentials.json")).unwrap();
    env.respond(&usage_body(), "200");
    let out = env.run();
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(out.stdout, b"{}\n", "nothing known is {{}} — valid JSON");
    assert!(out.stderr.is_empty());
    assert_eq!(env.curl_calls(), 0);
    assert!(!env.cache().exists());
}

#[test]
fn token_travels_on_stdin_never_in_argv() {
    let env = TestEnv::new("token-hygiene");
    env.respond(&usage_body(), "200");
    assert_eq!(env.run().status.code(), Some(0));
    let argv = fs::read_to_string(env.fix.join("curl-argv.log")).unwrap();
    assert!(
        !argv.contains(TOKEN),
        "the token must never be visible in /proc/*/cmdline: {argv}"
    );
    assert!(argv.contains("-H @-"), "curl must read a header from stdin");
    let stdin = fs::read_to_string(env.fix.join("curl-stdin.log")).unwrap();
    assert_eq!(stdin, format!("Authorization: Bearer {TOKEN}\n"));
}

// ---- lock serialization ---------------------------------------------------------

#[test]
fn concurrent_pollers_share_one_fetch() {
    let env = TestEnv::new("lock");
    env.respond(&usage_body(), "200");
    fs::write(env.fix.join("slow"), "").unwrap(); // curl stalls 1s
    let a = env
        .cmd()
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let b = env
        .cmd()
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let oa = a.wait_with_output().unwrap();
    let ob = b.wait_with_output().unwrap();
    assert_eq!(oa.status.code(), Some(0));
    assert_eq!(ob.status.code(), Some(0));
    assert_eq!(
        env.curl_calls(),
        1,
        "the loser must serve the winner's cache"
    );
    assert_eq!(oa.stdout, ob.stdout, "both pollers see the same reading");
    let printed: Value = serde_json::from_slice(&oa.stdout).unwrap();
    assert_eq!(printed, expected_json());
}

// ---- kind filter & text form ---------------------------------------------------

#[test]
fn kind_filter_and_text_table() {
    let env = TestEnv::new("kind-text");
    env.write_cache(
        "{\"sessionPct\":34,\"sessionResets\":\"2026-07-03T10:00:00Z\",\"weeklyPct\":62,\"weeklyResets\":\"2026-07-07T00:00:00Z\"}\n",
        Duration::from_secs(10),
    );
    // --kind claude passes; an unknown kind filters to nothing known
    let out = env.run_args(&["agents", "usage", "--kind", "claude", "--format", "json"]);
    let printed: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(printed, expected_json());
    let out = env.run_args(&["agents", "usage", "--kind", "codex", "--format", "json"]);
    assert_eq!(out.stdout, b"{}\n");
    // text: the house table; empty result prints nothing (no lonely header)
    let out = env.run_args(&["agents", "usage"]);
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "KIND    SESSION%  RESETS                WEEK%  RESETS\n\
         claude  34        2026-07-03T10:00:00Z  62     2026-07-07T00:00:00Z\n"
    );
    let out = env.run_args(&["agents", "usage", "--kind", "codex"]);
    assert!(out.stdout.is_empty());
}

// ---- codex provider ---------------------------------------------------------

fn codex_rate_limits_line() -> String {
    json!({
        "id": 2,
        "result": {
            "rateLimits": {
                "primary": {"usedPercent": 1, "resetsAt": 1_783_385_189_i64},
                "secondary": {"usedPercent": 4, "resetsAt": 1_783_437_715_i64},
            }
        }
    })
    .to_string()
}

fn expected_codex_reading() -> Value {
    json!({
        "sessionPct": 1,
        "sessionResets": "2026-07-07T00:46:29Z",
        "weeklyPct": 4,
        "weeklyResets": "2026-07-07T15:21:55Z",
    })
}

#[test]
fn missing_codex_auth_never_spawns_the_app_server() {
    let env = TestEnv::new("codex-no-auth");
    // enable_codex() deliberately not called: no ~/.codex/auth.json.
    env.codex_respond(&[&codex_rate_limits_line()], true);
    let out = env.run_args(&["agents", "usage", "--kind", "codex", "--format", "json"]);
    assert_eq!(out.stdout, b"{}\n");
    assert_eq!(env.codex_calls(), 0, "no auth.json must mean no spawn at all");
    assert!(!env.codex_cache().exists());
}

#[test]
fn codex_happy_path_parses_ratelimits_and_caches() {
    let env = TestEnv::new("codex-happy");
    env.enable_codex();
    // A realistic transcript: the id:1 initialize ack and an unsolicited
    // notification both precede id:2, and must be skipped rather than
    // mistaken for it.
    env.codex_respond(
        &[
            r#"{"id":1,"result":{"userAgent":"bsctl/test"}}"#,
            r#"{"method":"remoteControl/status/changed","params":{"status":"disabled"}}"#,
            &codex_rate_limits_line(),
        ],
        true, // hold: mirrors the real server never exiting on its own
    );
    let out = env.run_args(&["agents", "usage", "--kind", "codex", "--format", "json"]);
    assert_eq!(out.status.code(), Some(0));
    let printed: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(printed, json!({"codex": expected_codex_reading()}));
    let cached: Value = serde_json::from_slice(&fs::read(env.codex_cache()).unwrap()).unwrap();
    assert_eq!(cached, expected_codex_reading());
    assert_eq!(env.codex_calls(), 1);
}

#[test]
fn codex_fresh_cache_is_served_without_spawning_app_server() {
    let env = TestEnv::new("codex-fresh");
    env.enable_codex();
    fs::write(env.codex_cache(), "{\"sessionPct\":9}\n").unwrap();
    let out = env.run_args(&["agents", "usage", "--kind", "codex", "--format", "json"]);
    let printed: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(printed, json!({"codex": {"sessionPct": 9}}));
    assert_eq!(env.codex_calls(), 0);
}

#[test]
fn codex_response_without_id2_serves_the_stale_cache() {
    // The app-server answered (and this run's calls prove it was spawned)
    // but never got to (or never sent) the rate-limits result -- same
    // "stale beats absent" contract as a non-200 from claude's endpoint.
    let env = TestEnv::new("codex-no-id2");
    env.enable_codex();
    env.write_stale_codex_cache("{\"sessionPct\":9}\n");
    env.codex_respond(&[r#"{"id":1,"result":{"userAgent":"bsctl/test"}}"#], false);
    let out = env.run_args(&["agents", "usage", "--kind", "codex", "--format", "json"]);
    let printed: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(printed, json!({"codex": {"sessionPct": 9}}));
    assert_eq!(
        fs::read(env.codex_cache()).unwrap(),
        b"{\"sessionPct\":9}\n",
        "a failed refresh must never clobber the last good value"
    );
    assert_eq!(env.codex_calls(), 1);
}
