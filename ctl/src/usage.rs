//! `bsctl usage` — Claude plan usage for the Noctalia widget, mirroring
//! claude-usage.sh. The cache/lock protocol is specified in lib.rs ("Usage
//! cache"); output-affecting behavior is script-identical, with one
//! deliberate improvement: the OAuth token is handed to curl as a header on
//! STDIN (`-H @-`) instead of in argv — the sh reference put the bearer
//! token in curl's command line, where /proc/*/cmdline exposes it to every
//! local process for the duration of the request.

use std::env;
use std::ffi::OsString;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::SystemTime;

use serde_json::Value;

use crate::sys;

const TTL_SECS: u64 = 240;
const ENDPOINT: &str = "https://api.anthropic.com/api/oauth/usage";

pub fn run() -> i32 {
    let cache = cache_path();

    // Fast path: a recent reading already exists, so don't touch the network.
    if fresh(&cache) {
        serve(&cache);
        return 0;
    }

    // Serialize refreshes across the per-monitor pollers; a loser waits and
    // serves the winner's result. (The sh reference assumes ~/.cache exists;
    // creating it here just makes a fresh machine work.)
    if let Some(d) = cache.parent() {
        let _ = fs::create_dir_all(d);
    }
    let Ok(lock) = fs::File::create(with_suffix(&cache, ".lock")) else {
        return 0;
    };
    if !sys::flock_exclusive(&lock, true) {
        let _ = sys::flock_exclusive(&lock, false);
        if fresh(&cache) {
            serve(&cache);
        }
        return 0;
    }
    // Won the lock — re-check in case the previous holder just refreshed.
    if fresh(&cache) {
        serve(&cache);
        return 0;
    }

    let Some(tok) = read_token() else { return 0 };
    let Some(body) = fetch(&tok) else { return 0 };
    let Some(out) = transform(&body) else {
        return 0;
    };

    // Atomic cache write; print the reading even if caching fails (script:
    // `printf > tmp && mv`, then an unconditional printf).
    let tmp = with_suffix(&cache, ".tmp");
    let _ = fs::write(&tmp, format!("{out}\n")).and_then(|_| fs::rename(&tmp, &cache));
    println!("{out}");
    0
}

/// `${XDG_CACHE_HOME:-$HOME/.cache}/claude-usage.json` (empty env = unset).
fn cache_path() -> PathBuf {
    env::var_os("XDG_CACHE_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env::var_os("HOME").unwrap_or_default()).join(".cache"))
        .join("claude-usage.json")
}

/// `$cache.lock` / `$cache.tmp` — suffix appended to the full name.
fn with_suffix(p: &Path, suffix: &str) -> PathBuf {
    let mut s = OsString::from(p.as_os_str());
    s.push(suffix);
    PathBuf::from(s)
}

/// Cache mtime within TTL — the script's `date +%s - stat -c %Y < ttl` in
/// whole seconds; a future mtime counts as fresh (negative difference).
fn fresh(cache: &Path) -> bool {
    let Ok(mtime) = fs::metadata(cache).and_then(|m| m.modified()) else {
        return false;
    };
    match SystemTime::now().duration_since(mtime) {
        Ok(age) => age.as_secs() < TTL_SECS,
        Err(_) => true,
    }
}

/// `cat "$cache"` — raw bytes (the cached line already ends in \n).
fn serve(cache: &Path) {
    if let Ok(b) = fs::read(cache) {
        let _ = std::io::stdout().write_all(&b);
    }
}

/// Token from `~/.claude/.credentials.json`; any failure (missing HOME or
/// file, bad JSON, wrong shape, empty token) -> None -> silent exit 0.
fn read_token() -> Option<String> {
    let home = env::var_os("HOME").filter(|v| !v.is_empty())?;
    let raw = fs::read_to_string(Path::new(&home).join(".claude/.credentials.json")).ok()?;
    token_from(&raw)
}

/// `.claudeAiOauth.accessToken`, non-empty.
pub fn token_from(credentials: &str) -> Option<String> {
    let v: Value = serde_json::from_str(credentials).ok()?;
    match v.get("claudeAiOauth")?.get("accessToken")? {
        Value::String(s) if !s.is_empty() => Some(s.clone()),
        _ => None,
    }
}

/// curl boundary (faked in tests with a stub on PATH). Returns the response
/// BODY on a clean 200, None otherwise — curl errors, timeouts and non-200
/// statuses all keep the cache and print nothing. The Authorization header
/// travels on curl's stdin (`-H @-`), NEVER in argv (see module header).
fn fetch(token: &str) -> Option<Vec<u8>> {
    let mut child = Command::new("curl")
        .args([
            "-s",
            "-w",
            "\n%{http_code}",
            "--max-time",
            "6",
            ENDPOINT,
            "-H",
            "@-",
            "-H",
            "anthropic-beta: oauth-2025-04-20",
            "-H",
            "anthropic-version: 2023-06-01",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    child
        .stdin
        .take()?
        .write_all(format!("Authorization: Bearer {token}\n").as_bytes())
        .ok()?;
    let out = child.wait_with_output().ok()?;
    // `-w '\n%{http_code}'`: status is everything after the last newline.
    let split = out.stdout.iter().rposition(|&b| b == b'\n')?;
    (&out.stdout[split + 1..] == b"200").then(|| out.stdout[..split].to_vec())
}

/// The script's python transform: five_hour and seven_day must both be
/// objects; pct = python `round(utilization or 0)`, resets pass through
/// unless falsy (-> ""). Any input python's round() would raise on yields
/// None, and the caller prints nothing (keeping the cache) — same as the
/// reference's dying python leaving `$out` empty.
pub fn transform(body: &[u8]) -> Option<String> {
    let d: Value = serde_json::from_slice(body).ok()?;
    let fh = d.get("five_hour")?.as_object()?;
    let sd = d.get("seven_day")?.as_object()?;
    let out = serde_json::json!({
        "sessionPct": pct(fh.get("utilization"))?,
        "sessionResets": resets(fh.get("resets_at")),
        "weeklyPct": pct(sd.get("utilization"))?,
        "weeklyResets": resets(sd.get("resets_at")),
    });
    Some(out.to_string())
}

/// `round(v or 0)`: python-falsy -> 0, numbers python-3-rounded (half to
/// even), True -> 1; anything else raises in the reference -> None.
fn pct(v: Option<&Value>) -> Option<i64> {
    let v = v.unwrap_or(&Value::Null);
    if falsy(v) {
        return Some(0);
    }
    match v {
        Value::Bool(true) => Some(1),
        Value::Number(n) => Some(round_half_even(n.as_f64()?)),
        _ => None,
    }
}

/// `v or ""`: falsy -> "", otherwise the JSON value unchanged (json.dumps
/// re-emits whatever type it was).
fn resets(v: Option<&Value>) -> Value {
    match v {
        Some(v) if !falsy(v) => v.clone(),
        _ => Value::String(String::new()),
    }
}

/// python truthiness over JSON values.
fn falsy(v: &Value) -> bool {
    match v {
        Value::Null | Value::Bool(false) => true,
        Value::Bool(true) => false,
        Value::Number(n) => n.as_f64() == Some(0.0),
        Value::String(s) => s.is_empty(),
        Value::Array(a) => a.is_empty(),
        Value::Object(o) => o.is_empty(),
    }
}

/// python-3 `round()`: exact halves go to the even integer (Rust's
/// f64::round goes away from zero instead).
fn round_half_even(f: f64) -> i64 {
    let floor = f.floor();
    if f - floor == 0.5 {
        let below = floor as i64;
        if below % 2 == 0 { below } else { below + 1 }
    } else {
        f.round() as i64
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn token_extraction() {
        assert_eq!(
            token_from(r#"{"claudeAiOauth": {"accessToken": "sk-x"}}"#),
            Some("sk-x".to_string())
        );
        assert_eq!(
            token_from(r#"{"claudeAiOauth": {"accessToken": ""}}"#),
            None
        );
        assert_eq!(token_from(r#"{"claudeAiOauth": {}}"#), None);
        assert_eq!(token_from("{}"), None);
        assert_eq!(token_from("not json"), None);
    }

    #[test]
    fn transform_happy_path() {
        let body = json!({
            "five_hour": {"utilization": 34.2, "resets_at": "2026-07-03T10:00:00Z"},
            "seven_day": {"utilization": 61.7, "resets_at": "2026-07-07T00:00:00Z"},
        });
        let out: Value =
            serde_json::from_str(&transform(body.to_string().as_bytes()).unwrap()).unwrap();
        assert_eq!(
            out,
            json!({
                "sessionPct": 34,
                "sessionResets": "2026-07-03T10:00:00Z",
                "weeklyPct": 62,
                "weeklyResets": "2026-07-07T00:00:00Z",
            })
        );
    }

    #[test]
    fn transform_null_and_missing_fields_default() {
        let body = json!({
            "five_hour": {"utilization": null, "resets_at": null},
            "seven_day": {},
        });
        let out: Value =
            serde_json::from_str(&transform(body.to_string().as_bytes()).unwrap()).unwrap();
        assert_eq!(
            out,
            json!({"sessionPct": 0, "sessionResets": "", "weeklyPct": 0, "weeklyResets": ""})
        );
    }

    #[test]
    fn transform_rejects_bad_shapes() {
        assert_eq!(transform(b"not json"), None);
        assert_eq!(transform(br#"{"five_hour": 3, "seven_day": {}}"#), None);
        assert_eq!(transform(br#"{"seven_day": {}}"#), None);
        // a utilization round() would raise on kills the whole output
        assert_eq!(
            transform(br#"{"five_hour": {"utilization": "50"}, "seven_day": {}}"#),
            None
        );
    }

    #[test]
    fn rounding_is_pythons_half_to_even() {
        assert_eq!(round_half_even(34.2), 34);
        assert_eq!(round_half_even(34.6), 35);
        assert_eq!(round_half_even(62.5), 62); // python round(62.5) == 62
        assert_eq!(round_half_even(63.5), 64);
        assert_eq!(round_half_even(-2.5), -2);
        assert_eq!(round_half_even(-1.5), -2);
        assert_eq!(round_half_even(0.0), 0);
        assert_eq!(round_half_even(100.0), 100);
    }

    #[test]
    fn python_falsy_values() {
        for v in [
            json!(null),
            json!(false),
            json!(0),
            json!(0.0),
            json!(""),
            json!([]),
            json!({}),
        ] {
            assert!(falsy(&v), "{v} must be falsy");
        }
        for v in [
            json!(true),
            json!(1),
            json!("x"),
            json!([0]),
            json!({"a": 1}),
        ] {
            assert!(!falsy(&v), "{v} must be truthy");
        }
    }
}
