//! End-to-end tests for `bsctl usage` with a fake curl (captures argv and
//! stdin, serves a canned `body\n<code>` response) and fake credentials —
//! the network never enters the picture.

use std::fs;
use std::path::PathBuf;
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

    fn cmd(&self) -> Command {
        let mut c = Command::new(env!("CARGO_BIN_EXE_bsctl"));
        c.arg("usage")
            .env("XDG_CACHE_HOME", &self.cache_dir)
            .env("HOME", &self.home)
            .env("PATH", &self.path);
        c
    }

    fn run(&self) -> std::process::Output {
        self.cmd().output().unwrap()
    }

    fn cache(&self) -> PathBuf {
        self.cache_dir.join("claude-usage.json")
    }

    fn respond(&self, body: &str, status: &str) {
        fs::write(self.fix.join("response"), format!("{body}\n{status}")).unwrap();
    }

    fn write_cache(&self, content: &str, age: Duration) {
        fs::write(self.cache(), content).unwrap();
        let when = SystemTime::now() - age;
        let times = fs::FileTimes::new().set_accessed(when).set_modified(when);
        fs::File::options()
            .write(true)
            .open(self.cache())
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

fn expected_json() -> Value {
    json!({
        "sessionPct": 34,
        "sessionResets": "2026-07-03T10:00:00Z",
        "weeklyPct": 62,
        "weeklyResets": "2026-07-07T00:00:00Z",
    })
}

// ---- cache behavior ----------------------------------------------------------

#[test]
fn fresh_cache_is_served_without_touching_the_network() {
    let env = TestEnv::new("fresh");
    env.write_cache("{\"sessionPct\":11}\n", Duration::from_secs(10));
    let out = env.run();
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(out.stdout, b"{\"sessionPct\":11}\n");
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
    // cache now holds exactly what was printed
    assert_eq!(fs::read(env.cache()).unwrap(), out.stdout);
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
fn non_200_prints_nothing_and_keeps_the_stale_cache() {
    let env = TestEnv::new("non-200");
    env.write_cache("{\"sessionPct\":11}\n", Duration::from_secs(300));
    env.respond(r#"{"error": "rate limited"}"#, "429");
    let out = env.run();
    assert_eq!(out.status.code(), Some(0));
    assert!(out.stdout.is_empty() && out.stderr.is_empty());
    assert_eq!(
        fs::read(env.cache()).unwrap(),
        b"{\"sessionPct\":11}\n",
        "a failed refresh must never clobber the last good value"
    );
}

#[test]
fn malformed_200_body_prints_nothing_and_keeps_the_cache() {
    let env = TestEnv::new("malformed");
    env.write_cache("{\"sessionPct\":11}\n", Duration::from_secs(300));
    for body in ["not json at all", r#"{"five_hour": 3, "seven_day": {}}"#] {
        env.respond(body, "200");
        let out = env.run();
        assert_eq!(out.status.code(), Some(0));
        assert!(out.stdout.is_empty() && out.stderr.is_empty());
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
    assert!(out.stdout.is_empty() && out.stderr.is_empty());
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
