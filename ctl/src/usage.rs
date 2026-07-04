//! `bsctl agents usage` — plan usage per harness kind, and the `usage`
//! section of the world. One provider exists today (claude, via the OAuth
//! usage endpoint); adding another is one `PROVIDERS` entry. The
//! cache/lock protocol is specified in lib.rs ("Plan usage"); one
//! deliberate improvement over the sh reference this began as: the OAuth
//! token is handed to curl as a header on STDIN (`-H @-`) instead of in
//! argv — a command-line token is exposed to every local process via
//! /proc/*/cmdline for the duration of the request.

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

/// One provider: how to read a harness kind's plan usage (None = nothing
/// known).
type Reading = fn() -> Option<Value>;

/// The provider table: kind -> how to read its plan usage. A kind with no
/// entry simply never appears in the output; adding a provider (codex, agy
/// — the day they grow usage endpoints) is one entry here.
const PROVIDERS: &[(&str, Reading)] = &[("claude", claude_reading)];

/// The kind-indexed usage object: `{"claude": {sessionPct, ...}}`. Kinds
/// with nothing known (no provider, no cache, no token) are absent — an
/// empty object means "nothing known", never an error.
pub fn snapshot(kind: Option<&str>) -> Value {
    let mut out = serde_json::Map::new();
    for (k, reading) in PROVIDERS {
        if kind.is_none_or(|want| want == *k)
            && let Some(v) = reading()
        {
            out.insert((*k).to_string(), v);
        }
    }
    Value::Object(out)
}

/// `agents usage [--kind] [--format]`: the snapshot as one compact JSON
/// line (`{}` when nothing is known — always valid JSON for consumers) or
/// the house table (nothing at all when empty — no lonely headers).
pub fn get(kind: Option<&str>, json_out: bool) -> i32 {
    let u = snapshot(kind);
    if json_out {
        println!("{u}");
    } else if u.as_object().is_some_and(|m| !m.is_empty()) {
        println!("{}", crate::proto::render_table(&USAGE_HEADERS, &cells(&u)));
    }
    0
}

/// SESSION% / WEEK% are the five-hour and seven-day windows; each RESETS
/// column belongs to the window on its left.
pub const USAGE_HEADERS: [&str; 5] = ["KIND", "SESSION%", "RESETS", "WEEK%", "RESETS"];

/// [`USAGE_HEADERS`]'s cells, one row per kind (the map's order — sorted by
/// kind, deterministic).
pub fn cells(usage: &Value) -> Vec<Vec<String>> {
    usage
        .as_object()
        .map(|m| {
            m.iter()
                .map(|(kind, r)| {
                    vec![
                        kind.clone(),
                        crate::proto::field(r, "sessionPct"),
                        crate::proto::field(r, "sessionResets"),
                        crate::proto::field(r, "weeklyPct"),
                        crate::proto::field(r, "weeklyResets"),
                    ]
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The claude provider: the cached reading, refreshed through the flock
/// when stale. Every failure path serves whatever the cache holds — stale
/// beats absent, the same reasoning that keeps the cache on a non-200 —
/// and leaves the cache mtime old, so the next call retries the refresh.
fn claude_reading() -> Option<Value> {
    let cache = cache_path();

    // Fast path: a recent reading already exists, so don't touch the network.
    if fresh(&cache) {
        return read_cached(&cache);
    }

    // Serialize refreshes across every poller and streamer — exactly one
    // process pays the curl per TTL; a loser waits, then serves whatever
    // the winner cached.
    if let Some(d) = cache.parent() {
        let _ = fs::create_dir_all(d);
    }
    let Ok(lock) = fs::File::create(with_suffix(&cache, ".lock")) else {
        return read_cached(&cache);
    };
    if !sys::flock_exclusive(&lock, true) {
        let _ = sys::flock_exclusive(&lock, false);
        return read_cached(&cache);
    }
    // Won the lock — re-check in case the previous holder just refreshed.
    if fresh(&cache) {
        return read_cached(&cache);
    }

    match read_token()
        .and_then(|tok| fetch(&tok))
        .and_then(|body| transform(&body))
    {
        Some(out) => {
            // Atomic cache write; serve the reading even if caching fails.
            let tmp = with_suffix(&cache, ".tmp");
            let _ = fs::write(&tmp, format!("{out}\n")).and_then(|_| fs::rename(&tmp, &cache));
            serde_json::from_str(&out).ok()
        }
        None => read_cached(&cache),
    }
}

/// The cached reading as a value; missing/unparseable -> nothing known.
fn read_cached(cache: &Path) -> Option<Value> {
    serde_json::from_str(&fs::read_to_string(cache).ok()?).ok()
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
/// statuses all fall back to the stale cache. Bounded by `--max-time 6`:
/// this call sits on the stream engine's tick path and must never hang a
/// subscriber unbounded. The Authorization header travels on curl's stdin
/// (`-H @-`), NEVER in argv (see module header).
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
        Value::Number(n) => Some(crate::proto::round_half_even(n.as_f64()?)),
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
