//! Hyprland IPC boundary — a direct `.socket.sock` client with a hyprctl
//! fallback. Every compositor-facing call in this crate goes through here;
//! the contract is summarized in lib.rs ("Hyprland IPC") and the wire notes
//! below are what was verified empirically.
//!
//! Wire format (verified against the live compositor, Hyprland 0.55.4,
//! 2026-07-03):
//! - One request per connection to
//!   `$XDG_RUNTIME_DIR/hypr/$HYPRLAND_INSTANCE_SIGNATURE/.socket.sock`:
//!   write the command, read the reply to EOF (the compositor answers the
//!   first message and closes). Shutting down the write side is NOT required
//!   for the reply to arrive on this version, but we do it anyway — it is
//!   the unambiguous "request complete" and costs nothing.
//! - `j/<query>` returns JSON (`j/monitors all`, `j/workspaces`,
//!   `j/activeworkspace`, `j/clients` all verified). A LEADING `/` is
//!   rejected ("unknown request"); a `[[BATCH]]` prefix is accepted but
//!   unnecessary for single requests; without the `j/` prefix the same query
//!   returns human-readable text.
//! - `dispatch <cmd>` is the wire form of `hyprctl dispatch <cmd>` (verified
//!   with a no-op `hl.dsp.focus({ workspace = <active> })` refocus). Success
//!   is the literal reply `ok`; a rejected request replies with an error
//!   string — which hyprctl prints to STDOUT and still exits 0, so a failed
//!   dispatch has never been a non-zero exit through hyprctl either.
//! - `reload` (plain, same ok/error reply shape) is what `hyprctl reload`
//!   sends.
//! - `eval <lua>` is the wire form of `hyprctl eval <lua>` — the runtime
//!   config channel the Lua parser demands (legacy `keyword` is a rejected
//!   no-op). Same ok/error replies (verified with a read-only
//!   `eval return 1+1` -> `ok`, a syntax-error chunk -> `error: ...`, and a
//!   no-op `hl.monitor` re-assert of the disabled internal panel -> `ok`).
//!   An `error:` reply means the chunk failed to parse/run, not that it half
//!   executed, so the hyprctl re-send stays double-fire-safe for the
//!   single-call chunks this crate emits.
//!
//! Fallback policy: every entry point tries the socket first and falls back
//! to spawning hyprctl on ANY socket failure — connect, io, unparseable
//! JSON, non-ok dispatch reply. hyprctl is built with (and always speaks the
//! protocol of) whatever Hyprland version is running, so on a rolling
//! release it is the safety net for compositor protocol drift: if this
//! crate's wire knowledge goes stale, bsctl degrades to exec latency instead
//! of breaking. Re-sending after a non-ok reply is safe: the error reply
//! means the compositor rejected the request rather than executing it.
//!
//! Instance discovery: `$HYPRLAND_INSTANCE_SIGNATURE` when that instance is
//! live, else the newest-mtime LIVE dir under `<runtime>/hypr/` — the
//! restart_crashed_lock.sh walk, so VT/recovery contexts without the env var
//! still find the running instance. Live = the pid on `hyprland.lock`'s first
//! line is a running `Hyprland` (comm-checked, so a recycled pid doesn't
//! count). The env var alone is not trusted: a long-lived process started
//! before a compositor restart (the Codex app-server daemon, parented under
//! systemd --user) keeps the dead instance's signature forever, and its
//! hooks would otherwise query a socket nobody listens on. With no live
//! instance at all the pick is the plain env-then-newest one, so the failure
//! looks exactly as it did before. The hyprctl fallback is pinned to the
//! resolved instance for the same reason.

use std::env;
use std::fs;
use std::io::{self, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::Value;

use crate::sys;

/// The shared resolver ([`sys::runtime_dir`]): env, then /run/user/<uid> —
/// the uid rung was born here as restart_crashed_lock.sh's VT-shell walk
/// and now serves every bsctl state path (scrubbed-env MCP spawns hit it
/// too). A /tmp terminal fallback can't contain a hypr instance dir, so
/// discovery below just finds nothing there — same as no Hyprland.
fn runtime_dir() -> PathBuf {
    sys::runtime_dir()
}

/// Newest-mtime pick among instance-dir candidates — the
/// restart_crashed_lock.sh `ls -t | head -n1` walk. Mtime ties break toward
/// the lexically greater name so the pick stays deterministic (ls -t leaves
/// tie order unspecified; any stable rule works, this one needs no extra
/// state).
pub fn newest_instance(entries: &[(String, f64)]) -> Option<&str> {
    entries
        .iter()
        .max_by(|a, b| a.1.total_cmp(&b.1).then_with(|| a.0.cmp(&b.0)))
        .map(|(name, _)| name.as_str())
}

/// The pure half of [`instance_name`]. `entries` are (name, mtime, live).
/// A live env instance wins; else the newest live one; with nothing live,
/// the env instance (even unlisted) or the newest dir — the pre-liveness
/// pick, so a compositor-less context fails the same way it always did.
pub fn pick_instance<'a>(
    env: Option<&'a str>,
    entries: &'a [(String, f64, bool)],
) -> Option<&'a str> {
    if let Some(his) = env
        && entries.iter().any(|(n, _, live)| *live && n == his)
    {
        return Some(his);
    }
    let live: Vec<(String, f64)> = entries
        .iter()
        .filter(|(_, _, live)| *live)
        .map(|(n, m, _)| (n.clone(), *m))
        .collect();
    if let Some(n) = newest_instance(&live) {
        return entries
            .iter()
            .map(|(name, _, _)| name.as_str())
            .find(|name| *name == n);
    }
    if env.is_some() {
        return env;
    }
    let all: Vec<(String, f64)> = entries.iter().map(|(n, m, _)| (n.clone(), *m)).collect();
    let n = newest_instance(&all)?.to_string();
    entries
        .iter()
        .map(|(name, _, _)| name.as_str())
        .find(|name| *name == n)
}

/// Is the compositor that owns this instance dir still running? The first
/// line of `hyprland.lock` is its pid; a dead or recycled pid (comm is not
/// `Hyprland`) means a stale dir left behind by a crash or restart.
fn instance_live(dir: &Path) -> bool {
    let Ok(lock) = fs::read_to_string(dir.join("hyprland.lock")) else {
        return false;
    };
    let Some(pid) = lock
        .lines()
        .next()
        .and_then(|l| l.trim().parse::<u32>().ok())
    else {
        return false;
    };
    fs::read_to_string(format!("/proc/{pid}/comm")).is_ok_and(|c| c.trim_end() == "Hyprland")
}

/// The instance signature to talk to (see the header's discovery rule).
/// None only when there is no env var and no instance dir at all.
fn instance_name() -> Option<String> {
    let hypr = runtime_dir().join("hypr");
    let env = env::var("HYPRLAND_INSTANCE_SIGNATURE")
        .ok()
        .filter(|v| !v.is_empty());
    let entries: Vec<(String, f64, bool)> = fs::read_dir(&hypr)
        .map(|rd| {
            rd.flatten()
                .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
                .filter_map(|e| {
                    let mtime = sys::mtime_f64(&e.metadata().ok()?)?;
                    let live = instance_live(&e.path());
                    Some((e.file_name().to_string_lossy().into_owned(), mtime, live))
                })
                .collect()
        })
        .unwrap_or_default();
    pick_instance(env.as_deref(), &entries).map(str::to_string)
}

/// The instance dir holding both sockets: `<runtime>/hypr/<instance>/`.
fn instance_dir() -> Option<PathBuf> {
    Some(runtime_dir().join("hypr").join(instance_name()?))
}

/// hyprctl aimed at the same instance the socket path resolved — it reads
/// `$HYPRLAND_INSTANCE_SIGNATURE` too, so an inherited stale value would
/// send the fallback to the same dead compositor.
fn hyprctl() -> Command {
    let mut cmd = Command::new("hyprctl");
    if let Some(his) = instance_name() {
        cmd.env("HYPRLAND_INSTANCE_SIGNATURE", his);
    }
    cmd
}

/// The request socket: `<instance>/.socket.sock`. None when discovery fails —
/// callers then fall straight back to hyprctl.
fn socket_path() -> Option<PathBuf> {
    Some(instance_dir()?.join(".socket.sock"))
}

/// The EVENT socket, `<instance>/.socket2.sock` — same instance dir as the
/// request socket. Protocol (verified live, Hyprland 0.55.4): connect, send
/// nothing, read newline-delimited `EVENT>>DATA` lines forever; the
/// compositor closes the stream only when it exits. The `--stream` engine is the
/// consumer; there is no hyprctl fallback for a *stream*, so None (or a
/// failed connect) means running without compositor events.
pub fn event_socket_path() -> Option<PathBuf> {
    Some(instance_dir()?.join(".socket2.sock"))
}

/// One request per connection: write the command, shutdown the write side,
/// read the reply to EOF.
fn socket_request(cmd: &str) -> io::Result<Vec<u8>> {
    let path = socket_path().ok_or_else(|| io::Error::from(io::ErrorKind::NotFound))?;
    let mut s = UnixStream::connect(path)?;
    s.write_all(cmd.as_bytes())?;
    let _ = s.shutdown(std::net::Shutdown::Write); // reply arrives regardless (header)
    let mut buf = Vec::new();
    s.read_to_end(&mut buf)?;
    Ok(buf)
}

/// JSON query — socket `j/<query>` first, then `hyprctl <query...> -j`.
/// None on total failure (both paths); like the old sys::hyprctl_json, hook
/// callers stay silent on None while CLI callers report and exit non-zero.
pub fn json(query: &str) -> Option<Value> {
    if let Ok(buf) = socket_request(&format!("j/{query}"))
        && let Ok(v) = serde_json::from_slice(&buf)
    {
        return Some(v);
    }
    let out = hyprctl()
        .args(query.split_whitespace())
        .arg("-j")
        .output()
        .ok()?;
    serde_json::from_slice(&out.stdout).ok()
}

/// `hyprctl dispatch <cmd>` equivalent (the Lua `hl.dsp.*` strings).
pub fn dispatch(cmd: &str) -> i32 {
    ok_command(&["dispatch", cmd])
}

/// `hyprctl reload` equivalent — `bsctl display reset`'s first step.
pub fn reload() -> i32 {
    ok_command(&["reload"])
}

/// `hyprctl eval <lua>` equivalent — runtime config through the Lua API
/// (`hl.monitor` and friends). Ok/error replies like dispatch; see the
/// header for why the fallback re-send cannot double-apply.
pub fn eval(lua: &str) -> i32 {
    ok_command(&["eval", lua])
}

/// An ok/error command: the wire form is the argv words space-joined,
/// success the literal reply "ok". Anything else falls back to spawning
/// hyprctl with the same words (safe re-send, per the header). The spawn
/// keeps the old sys::hyprctl_dispatch contract byte-for-byte: stdout
/// (hyprctl's ok/error text) discarded, stderr passed through, exit code
/// propagated, 127 when hyprctl is missing.
fn ok_command(words: &[&str]) -> i32 {
    if let Ok(buf) = socket_request(&words.join(" "))
        && buf.trim_ascii() == b"ok"
    {
        return 0;
    }
    match hyprctl().args(words).stdout(Stdio::null()).status() {
        Ok(s) => s.code().unwrap_or(1),
        Err(_) => {
            eprintln!("bsctl: hyprctl not found");
            127 // what sh reports for a missing command
        }
    }
}

/// >>> compositor boundary for the hook — needs a live query. <<<
///
/// (workspace id, window address, terminal pid) of the FIRST client whose
/// pid is in `pids` (the hook caller's ancestor set) — the session's
/// terminal window. None on ANY failure — socket and hyprctl both
/// unreachable, bad JSON, no matching client, or a matching client without
/// a usable workspace id — and the hook then exits 0 without writing,
/// exactly like the sh reference. The address and terminal pid are
/// best-effort (None when the row lacks them): the workspace decides
/// whether a session exists at all, the address upgrades focus-by-session
/// from workspace to window, and the terminal pid is the matched client's
/// OWN pid — the process owning the window (kitty here), a descendant of
/// which is the harness. Integration tests fake this with a stub hyprctl
/// on PATH (their XDG_RUNTIME_DIR holds no socket, so the fallback runs).
pub fn client_for_pids(pids: &[i64]) -> Option<(i64, Option<String>, Option<i64>)> {
    client_of(&json("clients")?, pids)
}

/// The pure half of [`client_for_pids`], on an already-fetched clients
/// array.
pub fn client_of(clients: &Value, pids: &[i64]) -> Option<(i64, Option<String>, Option<i64>)> {
    let set: std::collections::HashSet<i64> = pids.iter().copied().collect();
    // First match decides; if ITS workspace.id is unusable the reference
    // python raises and prints nothing (no fallthrough to later clients).
    let c = clients.as_array()?.iter().find(|c| {
        c.get("pid")
            .and_then(Value::as_i64)
            .is_some_and(|p| set.contains(&p))
    })?;
    let ws = match c.get("workspace")?.get("id")? {
        Value::Number(n) => n.as_i64(),
        Value::String(s) => s.trim().parse().ok(), // int("3") succeeds in the reference
        _ => None,
    }?;
    let win = c
        .get("address")
        .and_then(Value::as_str)
        .filter(|a| !a.is_empty())
        .map(str::to_string);
    // The matched client's own pid: the terminal owning the window.
    let term_pid = c.get("pid").and_then(Value::as_i64);
    Some((ws, win, term_pid))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn e(pairs: &[(&str, f64)]) -> Vec<(String, f64)> {
        pairs.iter().map(|(n, m)| (n.to_string(), *m)).collect()
    }

    #[test]
    fn newest_instance_picks_max_mtime() {
        let entries = e(&[("old_1", 100.0), ("new_9", 300.0), ("mid_5", 200.0)]);
        assert_eq!(newest_instance(&entries), Some("new_9"));
        assert_eq!(newest_instance(&[]), None);
    }

    #[test]
    fn newest_instance_breaks_mtime_ties_lexically() {
        let entries = e(&[("bbb", 100.0), ("aaa", 100.0)]);
        assert_eq!(newest_instance(&entries), Some("bbb"));
        let entries = e(&[("aaa", 100.0), ("bbb", 100.0)]);
        assert_eq!(newest_instance(&entries), Some("bbb"));
    }

    fn l(rows: &[(&str, f64, bool)]) -> Vec<(String, f64, bool)> {
        rows.iter()
            .map(|(n, m, live)| (n.to_string(), *m, *live))
            .collect()
    }

    #[test]
    fn pick_instance_trusts_a_live_env_signature() {
        let entries = l(&[("env_1", 100.0, true), ("new_2", 300.0, true)]);
        assert_eq!(pick_instance(Some("env_1"), &entries), Some("env_1"));
    }

    #[test]
    fn pick_instance_skips_a_dead_env_signature() {
        // A daemon started before a compositor restart: its env names the
        // dead instance, whose dir (and socket files) are still on disk.
        let entries = l(&[("dead_1", 500.0, false), ("live_2", 300.0, true)]);
        assert_eq!(pick_instance(Some("dead_1"), &entries), Some("live_2"));
        assert_eq!(pick_instance(Some("gone_0"), &entries), Some("live_2"));
    }

    #[test]
    fn pick_instance_prefers_live_over_newer_dead() {
        let entries = l(&[("dead_9", 900.0, false), ("live_1", 100.0, true)]);
        assert_eq!(pick_instance(None, &entries), Some("live_1"));
    }

    #[test]
    fn pick_instance_without_live_keeps_the_old_pick() {
        let entries = l(&[("old_1", 100.0, false), ("new_2", 300.0, false)]);
        assert_eq!(pick_instance(Some("old_1"), &entries), Some("old_1"));
        assert_eq!(pick_instance(Some("unlisted"), &entries), Some("unlisted"));
        assert_eq!(pick_instance(None, &entries), Some("new_2"));
        assert_eq!(pick_instance(None, &[]), None);
    }

    #[test]
    fn client_of_first_match_decides() {
        let clients = json!([
            {"pid": 10, "workspace": {"id": 3}, "address": "0xaaa"},
            {"pid": 20, "workspace": {"id": 7}, "address": "0xbbb"},
        ]);
        // array order wins; the matched row's address AND its own pid (the
        // terminal owning the window) ride along
        assert_eq!(
            client_of(&clients, &[20, 10]),
            Some((3, Some("0xaaa".to_string()), Some(10)))
        );
        assert_eq!(
            client_of(&clients, &[20]),
            Some((7, Some("0xbbb".to_string()), Some(20)))
        );
        assert_eq!(client_of(&clients, &[99]), None);
    }

    #[test]
    fn client_of_id_and_address_shapes() {
        // string ids parse (int("3") in the reference); junk on the FIRST
        // match is None, with no fallthrough to later clients. A missing or
        // empty address is None — the workspace still resolves (the address
        // is an upgrade, never a requirement). The terminal pid is the
        // matched row's pid (always present here since we matched on it).
        let clients = json!([
            {"pid": 1, "workspace": {"id": " 3 "}},
            {"pid": 2, "workspace": {"id": true}, "address": "0xccc"},
            {"pid": 3, "workspace": {"id": 5}, "address": ""},
        ]);
        assert_eq!(client_of(&clients, &[1]), Some((3, None, Some(1))));
        assert_eq!(client_of(&clients, &[2, 3]), None);
        assert_eq!(client_of(&clients, &[3]), Some((5, None, Some(3))));
        assert_eq!(client_of(&json!("not an array"), &[1]), None);
        assert_eq!(client_of(&json!([{"pid": 1}]), &[1]), None);
    }
}
