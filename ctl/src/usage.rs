//! `bsctl agents usage` — plan usage per harness kind, and the `usage`
//! section of the world. Providers: claude (the OAuth usage endpoint) and
//! codex (the rate-limit records Codex writes into its own session logs —
//! no network, no token); adding another is one `PROVIDERS` entry. The
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
const PROVIDERS: &[(&str, Reading)] = &[("claude", claude_reading), ("codex", codex_reading)];

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
                    // Percentages render explicitly: proto::field treats 0
                    // as empty, and 0% is a real reading.
                    let pct = |k: &str| {
                        r.get(k)
                            .and_then(Value::as_i64)
                            .map_or(String::new(), |n| n.to_string())
                    };
                    vec![
                        kind.clone(),
                        pct("sessionPct"),
                        crate::proto::field(r, "sessionResets"),
                        pct("weeklyPct"),
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

// ---- codex: rate limits from Codex's own session logs ------------------------
//
// Codex records the account's rate limits in every session log
// (`$CODEX_HOME/sessions/YYYY/MM/DD/rollout-*.jsonl`, CODEX_HOME defaulting
// to ~/.codex) as `token_count` events: `{"timestamp", "payload": {"type":
// "token_count", "rate_limits": {"primary"|"secondary": {"used_percent",
// "window_minutes", "resets_at" (epoch s)}}}}` (observed on Codex 0.170).
// The newest record is the freshest view of the account, so the reading is
// as current as the last Codex turn — `asOf` says when that was. Windows map
// by LENGTH (<= 1 day -> session, >= 7 days -> weekly), not by the
// primary/secondary label; a window whose reset time has passed reads 0%
// (it rolled over since the record was written).

/// How many of the newest session logs to search, and how much of each
/// file's tail: records repeat every turn, so the end of a recent log holds
/// one; logs can run to many MB.
const CODEX_FILES: usize = 6;
const CODEX_TAIL: u64 = 512 * 1024;

fn codex_reading() -> Option<Value> {
    let home = env::var_os("CODEX_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env::var_os("HOME").unwrap_or_default()).join(".codex"));
    let now = sys::now_f64() as i64;
    newest_rollouts(&home.join("sessions"), CODEX_FILES)
        .iter()
        .find_map(|p| last_rate_limits(p))
        .and_then(|(line, ts)| codex_from_record(&line, ts.as_deref(), now))
}

/// The `n` newest `rollout-*.jsonl` files, walking the date-named dirs
/// newest-first (zero-padded names sort chronologically) and stopping once
/// enough are found — never a full-tree scan.
fn newest_rollouts(sessions: &Path, n: usize) -> Vec<PathBuf> {
    fn sorted_desc(d: &Path) -> Vec<PathBuf> {
        let mut v: Vec<PathBuf> = fs::read_dir(d)
            .map(|rd| rd.flatten().map(|e| e.path()).collect())
            .unwrap_or_default();
        v.sort();
        v.reverse();
        v
    }
    let mut out = Vec::new();
    for y in sorted_desc(sessions).into_iter().filter(|p| p.is_dir()) {
        for m in sorted_desc(&y).into_iter().filter(|p| p.is_dir()) {
            for d in sorted_desc(&m).into_iter().filter(|p| p.is_dir()) {
                let mut files: Vec<(SystemTime, PathBuf)> = sorted_desc(&d)
                    .into_iter()
                    .filter(|p| {
                        p.file_name()
                            .and_then(|n| n.to_str())
                            .is_some_and(|n| n.starts_with("rollout-") && n.ends_with(".jsonl"))
                    })
                    .filter_map(|p| Some((fs::metadata(&p).ok()?.modified().ok()?, p)))
                    .collect();
                files.sort_by(|a, b| b.0.cmp(&a.0));
                out.extend(files.into_iter().map(|(_, p)| p));
                if out.len() >= n {
                    out.truncate(n);
                    return out;
                }
            }
        }
    }
    out
}

/// The last line in the file's tail that carries `"rate_limits"`, with that
/// line's `timestamp`.
fn last_rate_limits(path: &Path) -> Option<(String, Option<String>)> {
    use std::io::{Read, Seek, SeekFrom};
    let mut f = fs::File::open(path).ok()?;
    let len = f.metadata().ok()?.len();
    f.seek(SeekFrom::Start(len.saturating_sub(CODEX_TAIL)))
        .ok()?;
    let mut buf = Vec::new();
    f.read_to_end(&mut buf).ok()?;
    let text = String::from_utf8_lossy(&buf);
    text.lines()
        .rev()
        .find(|l| l.contains("\"rate_limits\""))
        .and_then(|l| {
            let v: Value = serde_json::from_str(l).ok()?;
            let ts = v
                .get("timestamp")
                .and_then(Value::as_str)
                .map(str::to_string);
            Some((l.to_string(), ts))
        })
}

/// One rate-limit record -> the shared usage shape (`sessionPct`,
/// `sessionResets`, `weeklyPct`, `weeklyResets`) plus `asOf`. None unless
/// both windows are present.
pub fn codex_from_record(line: &str, as_of: Option<&str>, now: i64) -> Option<Value> {
    let v: Value = serde_json::from_str(line).ok()?;
    let rl = v
        .pointer("/payload/rate_limits")
        .or_else(|| v.get("rate_limits"))?;
    let windows: Vec<&Value> = ["primary", "secondary"]
        .iter()
        .filter_map(|k| rl.get(*k).filter(|w| w.is_object()))
        .collect();
    let pick = |short: bool| {
        windows.iter().find(|w| {
            let mins = w
                .get("window_minutes")
                .and_then(Value::as_f64)
                .unwrap_or(0.0);
            if short {
                mins > 0.0 && mins <= 1440.0
            } else {
                mins >= 10080.0
            }
        })
    };
    let read = |w: &&Value| -> (i64, String) {
        let resets = w.get("resets_at").and_then(Value::as_i64).unwrap_or(0);
        let pct = w.get("used_percent").and_then(Value::as_f64).unwrap_or(0.0);
        let pct = if resets > 0 && resets <= now {
            0.0
        } else {
            pct
        };
        (
            pct.round() as i64,
            if resets > 0 {
                sys::iso_utc(resets)
            } else {
                String::new()
            },
        )
    };
    let (sp, sr) = read(pick(true)?);
    let (wp, wr) = read(pick(false)?);
    let mut out = serde_json::json!({
        "sessionPct": sp, "sessionResets": sr, "weeklyPct": wp, "weeklyResets": wr,
    });
    if let Some(ts) = as_of {
        out["asOf"] = Value::String(ts.to_string());
    }
    Some(out)
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

/// Token from `~/.claude/.credentials.json`, else (macOS) the login
/// Keychain; any failure (missing HOME or file, no Keychain item, bad JSON,
/// wrong shape, empty token) -> None -> silent exit 0.
fn read_token() -> Option<String> {
    let from_file = env::var_os("HOME")
        .filter(|v| !v.is_empty())
        .and_then(|home| {
            fs::read_to_string(Path::new(&home).join(".claude/.credentials.json")).ok()
        })
        .and_then(|raw| token_from(&raw));
    from_file.or_else(keychain_credentials)
}

/// Claude Code on macOS keeps the same credentials JSON in the login
/// Keychain (generic password, service `Claude Code-credentials`; verified
/// on macOS 27 — no credentials file is written there). `security … -w`
/// prints the secret on STDOUT, so the token never touches argv. Reached
/// through PATH like curl, so tests stub it and can never read a real
/// developer Keychain.
#[cfg(target_os = "macos")]
fn keychain_credentials() -> Option<String> {
    let out = Command::new("security")
        .args([
            "find-generic-password",
            "-s",
            "Claude Code-credentials",
            "-w",
        ])
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    token_from(String::from_utf8_lossy(&out.stdout).trim())
}

#[cfg(not(target_os = "macos"))]
fn keychain_credentials() -> Option<String> {
    None
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
    let mut out = serde_json::json!({
        "sessionPct": pct(fh.get("utilization"))?,
        "sessionResets": resets(fh.get("resets_at")),
        "weeklyPct": pct(sd.get("utilization"))?,
        "weeklyResets": resets(sd.get("resets_at")),
    });
    // Optional extras — absent unless the account has them, so consumers
    // that only know the four core keys are unaffected.
    let scoped = scoped_limits(&d);
    if !scoped.is_empty() {
        out["scoped"] = Value::Array(scoped);
    }
    if let Some(m) = monthly_extra(&d) {
        for (k, v) in m {
            out[k] = v;
        }
    }
    Some(out.to_string())
}

/// Per-model weekly limits: the `limits` entries of kind `weekly_scoped`,
/// labelled by the model's display name -> `[{label, pct, resets}]`.
fn scoped_limits(d: &Value) -> Vec<Value> {
    d.get("limits")
        .and_then(Value::as_array)
        .map(|ls| {
            ls.iter()
                .filter(|l| l.get("kind").and_then(Value::as_str) == Some("weekly_scoped"))
                .filter_map(|l| {
                    let label = l
                        .pointer("/scope/model/display_name")
                        .and_then(Value::as_str)
                        .filter(|s| !s.is_empty())?;
                    Some(serde_json::json!({
                        "label": label,
                        "pct": pct(l.get("percent"))?,
                        "resets": resets(l.get("resets_at")),
                    }))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The monthly cap on pay-as-you-go credits beyond the plan (`extra_usage`),
/// only while it is enabled: `monthlyPct`, plus `monthlySpent` /
/// `monthlyLimit` as decimal strings in `monthlyCurrency` (the endpoint
/// gives minor units with `decimal_places`).
fn monthly_extra(d: &Value) -> Option<Vec<(&'static str, Value)>> {
    let e = d.get("extra_usage")?;
    if e.get("is_enabled").and_then(Value::as_bool) != Some(true) {
        return None;
    }
    let limit = e.get("monthly_limit").and_then(Value::as_f64)?;
    let used = e.get("used_credits").and_then(Value::as_f64).unwrap_or(0.0);
    let places = e
        .get("decimal_places")
        .and_then(Value::as_i64)
        .unwrap_or(2)
        .clamp(0, 6) as i32;
    let scale = 10f64.powi(places);
    let p = match e.get("utilization").and_then(Value::as_f64) {
        Some(u) => u,
        None if limit > 0.0 => used / limit * 100.0,
        None => 0.0,
    };
    let money = |minor: f64| format!("{:.*}", places as usize, minor / scale);
    Some(vec![
        ("monthlyPct", Value::from(crate::proto::round_half_even(p))),
        ("monthlySpent", Value::from(money(used))),
        ("monthlyLimit", Value::from(money(limit))),
        (
            "monthlyCurrency",
            Value::from(e.get("currency").and_then(Value::as_str).unwrap_or("USD")),
        ),
    ])
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

    #[test]
    fn claude_extras_are_optional_and_parsed() {
        let base = serde_json::json!({
            "five_hour": {"utilization": 25.0, "resets_at": "2026-11-18T23:30:00Z"},
            "seven_day": {"utilization": 11.0, "resets_at": "2026-11-25T00:00:00Z"},
        });
        let plain: Value =
            serde_json::from_str(&transform(base.to_string().as_bytes()).unwrap()).unwrap();
        assert!(plain.get("scoped").is_none() && plain.get("monthlyPct").is_none());

        let mut rich = base.clone();
        rich["limits"] = serde_json::json!([
            {"kind": "session", "percent": 25},
            {"kind": "weekly_scoped", "percent": 15, "resets_at": "2026-11-25T00:00:00Z",
             "scope": {"model": {"display_name": "Fable"}}},
            {"kind": "weekly_scoped", "percent": 3, "scope": {"model": {"display_name": null}}}
        ]);
        rich["extra_usage"] = serde_json::json!({"is_enabled": true, "monthly_limit": 5000,
            "used_credits": 1250, "utilization": null, "currency": "USD", "decimal_places": 2});
        let v: Value =
            serde_json::from_str(&transform(rich.to_string().as_bytes()).unwrap()).unwrap();
        assert_eq!(
            v["scoped"],
            serde_json::json!([{"label": "Fable", "pct": 15, "resets": "2026-11-25T00:00:00Z"}])
        );
        assert_eq!(v["monthlyPct"], 25);
        assert_eq!(v["monthlySpent"], "12.50");
        assert_eq!(v["monthlyLimit"], "50.00");

        // disabled extra usage (the out_of_credits case) stays absent
        rich["extra_usage"]["is_enabled"] = Value::Bool(false);
        let off: Value =
            serde_json::from_str(&transform(rich.to_string().as_bytes()).unwrap()).unwrap();
        assert!(off.get("monthlyPct").is_none());
    }

    fn rl(primary: (f64, i64, i64), secondary: (f64, i64, i64)) -> String {
        serde_json::json!({"timestamp": "2026-11-18T18:46:18Z", "type": "event_msg",
            "payload": {"type": "token_count", "rate_limits": {
                "primary": {"used_percent": primary.0, "window_minutes": primary.1, "resets_at": primary.2},
                "secondary": {"used_percent": secondary.0, "window_minutes": secondary.1, "resets_at": secondary.2}}}})
        .to_string()
    }

    #[test]
    fn codex_record_maps_windows_by_length_and_rolls_over() {
        let now = 1_795_000_000;
        let line = rl((12.4, 300, now + 600), (8.0, 10080, now + 86_400));
        let v = codex_from_record(&line, Some("2026-11-18T18:46:18Z"), now).unwrap();
        assert_eq!(v["sessionPct"], 12);
        assert_eq!(v["weeklyPct"], 8);
        assert_eq!(v["sessionResets"], crate::sys::iso_utc(now + 600));
        assert_eq!(v["asOf"], "2026-11-18T18:46:18Z");
        // swapped labels still map by window length
        let swapped = rl((8.0, 10080, now + 86_400), (12.4, 300, now + 600));
        assert_eq!(
            codex_from_record(&swapped, None, now).unwrap()["sessionPct"],
            12
        );
        // a window whose reset has passed has rolled over: 0%
        let stale = rl((90.0, 300, now - 1), (8.0, 10080, now + 86_400));
        assert_eq!(
            codex_from_record(&stale, None, now).unwrap()["sessionPct"],
            0
        );
        // missing a window -> nothing known
        assert_eq!(
            codex_from_record(
                r#"{"payload":{"rate_limits":{"primary":{"window_minutes":300}}}}"#,
                None,
                now
            ),
            None
        );
        assert_eq!(codex_from_record("not json", None, now), None);
    }

    #[test]
    fn codex_reads_the_newest_log_tail() {
        let root =
            std::path::PathBuf::from("/tmp").join(format!("bsctl-codex-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let old = root.join("sessions/2026/11/17");
        let new = root.join("sessions/2026/11/18");
        fs::create_dir_all(&old).unwrap();
        fs::create_dir_all(&new).unwrap();
        let far = 4_000_000_000;
        fs::write(
            old.join("rollout-a.jsonl"),
            rl((50.0, 300, far), (50.0, 10080, far)),
        )
        .unwrap();
        // the newest log has an unrelated last line after its record
        fs::write(
            new.join("rollout-b.jsonl"),
            format!(
                "{}\n{{\"type\":\"other\"}}\n",
                rl((7.0, 300, far), (3.0, 10080, far))
            ),
        )
        .unwrap();
        // a newer log without any record yet must not hide the older ones
        fs::write(new.join("rollout-c.jsonl"), "{\"type\":\"session_meta\"}\n").unwrap();
        let files = newest_rollouts(&root.join("sessions"), 6);
        assert_eq!(files.len(), 3);
        assert!(
            files[2].ends_with("2026/11/17/rollout-a.jsonl"),
            "older days come last"
        );
        let (line, _) = files.iter().find_map(|p| last_rate_limits(p)).unwrap();
        assert_eq!(codex_from_record(&line, None, 0).unwrap()["sessionPct"], 7);
        let _ = fs::remove_dir_all(&root);
    }

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
