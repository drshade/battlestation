//! `bsctl watch` — long-lived daemon keeping `<state-dir>/.widget.json`
//! current, so the widget FileView-watches ONE file instead of polling
//! anything. Two event sources fold into one output: the agent-state dir
//! (inotify; the sessions section reuses [`poll::poll`] wholesale) and
//! Hyprland's `.socket2.sock` event stream (the compositor section, built
//! fresh per recompute from `j/workspaces` + `j/monitors`). This module only
//! owns *when* to recompute and the single-writer/failover discipline
//! (contract + output schema in lib.rs).
//!
//! Never crash-loops: any transient error (state dir vanishing, inotify fd
//! error, socket2 disconnect on compositor restart) is logged to stderr
//! once, then re-initialized — including the flock, so a wounded winner
//! cleanly hands over to a blocked standby. If socket2 can't connect at all
//! (no Hyprland running), watch still works DEGRADED: agent-state events
//! keep flowing, the compositor section rides the queries' failure path
//! (null) — see [`EventSock::connect`].

use std::convert::Infallible;
use std::ffi::CString;
use std::fs;
use std::io::{self, Read};
use std::os::fd::{AsRawFd, RawFd};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::{ipc, poll, sys};

/// The consolidated widget state file. Dot-prefixed on purpose: `poll`'s
/// dotfile skip and `clear`'s `<sid>.*` sweep can never touch it.
pub const OUTPUT_NAME: &str = ".widget.json";
/// Single-writer exclusive flock; losers BLOCK here (hot standbys).
pub const LOCK_NAME: &str = ".widget.lock";

/// Slow tick: inotify cannot see session pids dying or markers aging past
/// the GC window, so recompute unconditionally this often.
const TICK: Duration = Duration::from_secs(10);
/// After a triggering event, drain further events this long before
/// recomputing once (hooks write session file + marker in one burst).
const COALESCE_MS: i32 = 50;
/// Cap on coalescing rounds so a steady event stream can't starve the write.
const COALESCE_ROUNDS: u32 = 10;

pub fn run() -> i32 {
    let dir = sys::state_dir();
    let proj = poll::projects_dir();
    let mut last_err = String::new();
    loop {
        let e = match session(&dir, &proj) {
            Err(e) => e,
            Ok(never) => match never {},
        };
        // Log once per distinct failure, not once per retry — a permanently
        // broken environment must not fill the widget's stderr at 1 line/s.
        let msg = e.to_string();
        if msg != last_err {
            eprintln!("bsctl watch: {msg}; re-initializing");
            last_err = msg;
        }
        std::thread::sleep(Duration::from_secs(1));
    }
}

/// One lock-acquisition-to-error lifetime: acquire the flock (blocking —
/// this is where standbys park), write the state once, then loop on
/// inotify + the compositor event socket + the slow tick. Only ever returns
/// an error; the caller re-inits — which is also the socket2 reconnect path
/// after a compositor restart (its EOF surfaces as an error here).
fn session(dir: &Path, proj: &Path) -> io::Result<Infallible> {
    fs::create_dir_all(dir)?;
    // Single writer with seamless failover: the winner proceeds; losers
    // block in flock and take over the instant the winner's fd closes
    // (process death included). Reopened on every re-init so a re-created
    // state dir can't leave us holding a lock on a deleted inode while a
    // fresh instance wins the new one.
    let lock = fs::File::create(dir.join(LOCK_NAME))?;
    if !sys::flock_exclusive(&lock, false) {
        return Err(io::Error::last_os_error());
    }
    migrate_legacy_dir(dir);
    let ino = Inotify::new(dir)?;
    // Compositor events; None = degraded mode (no Hyprland), agent-only.
    let mut sock = EventSock::connect();
    // On acquiring the lock (winner or successor): write once immediately.
    let mut prev: Option<String> = None;
    recompute(dir, proj, compositor_state, &mut prev)?;

    let mut deadline = Instant::now() + TICK;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        let ready = wait2(
            ino.fd,
            sock.as_ref().map(|s| s.stream.as_raw_fd()),
            remaining.as_millis().min(i32::MAX as u128) as i32,
        )?;
        let Some((ino_ready, sock_ready)) = ready else {
            // Slow tick (poll(2) timeout expired).
            recompute(dir, proj, compositor_state, &mut prev)?;
            deadline = Instant::now() + TICK;
            continue;
        };
        let mut triggered = false;
        if ino_ready {
            triggered |= ino.drain()?;
        }
        if sock_ready {
            // A socket error here (EOF = compositor restart/exit) propagates
            // to the re-init path, which reconnects — or degrades if the
            // compositor is really gone.
            triggered |= sock.as_mut().unwrap().drain()?;
        }
        if !triggered {
            // Dotfile/debug.log churn or irrelevant compositor events only
            // (incl. our own output writes): no recompute, and the tick
            // deadline keeps running.
            continue;
        }
        // Coalesce the burst (from EITHER source) before recomputing once.
        let mut rounds = 0;
        while rounds < COALESCE_ROUNDS {
            let Some((i, s)) = wait2(
                ino.fd,
                sock.as_ref().map(|s| s.stream.as_raw_fd()),
                COALESCE_MS,
            )?
            else {
                break;
            };
            if i {
                ino.drain()?;
            }
            if s {
                sock.as_mut().unwrap().drain()?;
            }
            rounds += 1;
        }
        recompute(dir, proj, compositor_state, &mut prev)?;
        deadline = Instant::now() + TICK;
    }
}

/// One-time courtesy migration from the pre-rename state dir (`claude-ws`,
/// a sibling of the current one). Strictly the dir is tmpfs and self-heals —
/// every hook recreates its session file in the new dir on its next event,
/// and the old dir dies at reboot — but live sessions would vanish from the
/// widget until that next event. So on acquiring the writer lock, if the old
/// dir still exists and the new dir has no protocol files yet, rename the
/// old dir's protocol files (non-dot, non-debug.log — same filter as the
/// poll pass) across. Best-effort: every failure is ignored, and nothing is
/// ever deleted. Once the new dir has any protocol file the check is a
/// no-op, so re-inits and failovers never re-run the move.
fn migrate_legacy_dir(dir: &Path) {
    migrate_legacy_state_dir();
    let Some(old) = dir.parent().map(|p| p.join("claude-ws")) else {
        return;
    };
    let Ok(old_rd) = fs::read_dir(&old) else {
        return; // no legacy dir (the steady state)
    };
    let new_has_protocol_files = fs::read_dir(dir).is_ok_and(|rd| {
        rd.flatten()
            .any(|e| name_triggers(&e.file_name().to_string_lossy()))
    });
    if new_has_protocol_files {
        return;
    }
    for e in old_rd.flatten() {
        let name = e.file_name();
        if name_triggers(&name.to_string_lossy()) {
            let _ = fs::rename(e.path(), dir.join(&name));
        }
    }
}

/// Companion one-time move for the PERSISTENT state dir: the workspace
/// order file lives under `.../state/battlestation-workspaces/` since the
/// plugin rename (previously `claude-workspaces`, a sibling). Unlike the
/// runtime dir this one survives reboots, so without the move a saved pill
/// order would silently reset. Rename the whole old dir into place, only
/// while the new one doesn't exist yet; best-effort, nothing deleted.
fn migrate_legacy_state_dir() {
    let order = crate::ws::order_file();
    let Some(new_dir) = order.parent() else {
        return;
    };
    let Some(state_root) = new_dir.parent() else {
        return;
    };
    let old_dir = state_root.join("claude-workspaces");
    if old_dir.is_dir() && !new_dir.exists() {
        let _ = fs::rename(&old_dir, new_dir);
    }
}

/// Run the poll pass, fetch fresh compositor state, and rewrite the output
/// file — but only when the serialization changed (a FileView reload on
/// every tick would wake the widget pointlessly), or when the file is
/// missing (manual deletion; the no-change short-circuit would otherwise
/// leave it gone until the state next changes). Content is one compact JSON
/// object + trailing newline (schema in lib.rs): `sessions` is EXACTLY
/// `bsctl poll`'s array; `compositor` is the fetcher's value (null on query
/// failure — the widget keeps its last compositor state). The fetcher is
/// injected so tests never touch a real compositor.
fn recompute(
    dir: &Path,
    proj: &Path,
    fetch_compositor: impl Fn() -> Value,
    prev: &mut Option<String>,
) -> io::Result<()> {
    let out = json!({
        "sessions": Value::Array(poll::poll(sys::now_f64(), dir, proj)),
        "compositor": fetch_compositor(),
    });
    let content = format!("{out}\n");
    if prev.as_deref() != Some(content.as_str()) || !dir.join(OUTPUT_NAME).exists() {
        write_atomic(dir, &content)?;
    }
    *prev = Some(content);
    Ok(())
}

/// The compositor section, built fresh from `j/workspaces` + `j/monitors`
/// (~1ms socket queries; the hyprctl fallback inside [`ipc::json`] covers
/// wire drift). Null when either query fails outright — degraded mode or a
/// mid-restart compositor; the widget keeps its last state. Workspaces
/// whose name starts with `special:` are excluded (the bar never renders
/// them — same rule as `bsctl ws`); a monitor's `specialShowing` covers the
/// scratchpad-visible case instead.
fn compositor_state() -> Value {
    let (Some(ws), Some(mons)) = (ipc::json("workspaces"), ipc::json("monitors")) else {
        return Value::Null;
    };
    let (Some(ws), Some(mons)) = (ws.as_array(), mons.as_array()) else {
        return Value::Null;
    };
    let workspaces: Vec<Value> = ws
        .iter()
        .filter(|w| {
            !w.get("name")
                .and_then(Value::as_str)
                .unwrap_or("")
                .starts_with("special:")
        })
        .map(|w| {
            json!({
                "id": w.get("id").cloned().unwrap_or(Value::Null),
                "name": w.get("name").and_then(Value::as_str).unwrap_or(""),
                "monitor": w.get("monitor").and_then(Value::as_str).unwrap_or(""),
                "windows": w.get("windows").and_then(Value::as_i64).unwrap_or(0),
            })
        })
        .collect();
    let monitors: Vec<Value> = mons
        .iter()
        .map(|m| {
            json!({
                "name": m.get("name").and_then(Value::as_str).unwrap_or(""),
                "x": m.get("x").cloned().unwrap_or(Value::Null),
                "y": m.get("y").cloned().unwrap_or(Value::Null),
                "focused": m.get("focused").and_then(Value::as_bool).unwrap_or(false),
                "activeWs": m
                    .pointer("/activeWorkspace/id")
                    .cloned()
                    .unwrap_or(Value::Null),
                "specialShowing": !m
                    .pointer("/specialWorkspace/name")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .is_empty(),
            })
        })
        .collect();
    json!({ "workspaces": workspaces, "monitors": monitors })
}

/// Temp file + rename in the same dir (the crate's atomic-write pattern);
/// the temp name is dot-prefixed like every other writer's, so poll/clear
/// skip it and our own event filter ignores it.
fn write_atomic(dir: &Path, content: &str) -> io::Result<()> {
    let tmp = dir.join(format!(".{OUTPUT_NAME}.tmp"));
    fs::write(&tmp, content)?;
    fs::rename(&tmp, dir.join(OUTPUT_NAME))
}

/// Should an event on this dir entry trigger a recompute? Exactly the names
/// the poll pass reads: dotfiles (our own output + temp writes, hook temp
/// files) and debug.log can never change the output — and reacting to our
/// own `.widget.json` rename would self-trigger forever. An empty name is
/// an event about the watch dir itself, not an entry.
pub fn name_triggers(name: &str) -> bool {
    !name.is_empty() && !name.starts_with('.') && name != "debug.log"
}

/// Parse a raw inotify read(2) buffer into (mask, name) pairs. Wire layout
/// per inotify(7): `{wd: i32, mask: u32, cookie: u32, len: u32}` followed by
/// `len` bytes of NUL-padded name (len == 0: no name). Fields are read
/// byte-wise so buffer alignment never matters; a truncated tail is dropped.
pub fn parse_events(buf: &[u8]) -> Vec<(u32, String)> {
    const HDR: usize = 16;
    let mut out = Vec::new();
    let mut off = 0;
    while off + HDR <= buf.len() {
        let mask = u32::from_ne_bytes(buf[off + 4..off + 8].try_into().unwrap());
        let len = u32::from_ne_bytes(buf[off + 12..off + 16].try_into().unwrap()) as usize;
        if off + HDR + len > buf.len() {
            break;
        }
        let name = &buf[off + HDR..off + HDR + len];
        let name = name.split(|&b| b == 0).next().unwrap_or(&[]);
        out.push((mask, String::from_utf8_lossy(name).into_owned()));
        off += HDR + len;
    }
    out
}

/// Which compositor events warrant a recompute: anything that can change
/// what the widget renders — workspace existence/name/monitor/active state,
/// monitor set/focus, window counts (occupancy = `windows` per workspace,
/// which open/close/movewindow cover), the scratchpad, and config reloads
/// (monitor layout may change). Deliberately NOT here: `windowtitle*` and
/// `activewindow*` — nothing rendered depends on titles or the focused
/// window, and they are by far the noisiest events. Unknown events: ignore.
pub fn event_triggers(line: &str) -> bool {
    const RELEVANT: &[&str] = &[
        "workspace",
        "workspacev2",
        "createworkspace",
        "createworkspacev2",
        "destroyworkspace",
        "destroyworkspacev2",
        "moveworkspace",
        "moveworkspacev2",
        "renameworkspace",
        "focusedmon",
        "focusedmonv2",
        "monitoradded",
        "monitoraddedv2",
        "monitorremoved",
        "monitorremovedv2",
        "openwindow",
        "closewindow",
        "movewindow",
        "movewindowv2",
        "activespecial",
        "activespecialv2",
        "configreloaded",
    ];
    let name = line.split_once(">>").map_or(line, |(n, _)| n);
    RELEVANT.contains(&name)
}

/// poll(2) on the inotify fd plus (optionally) the event socket:
/// Ok(None) = timeout hit, Ok(Some((inotify_ready, socket_ready)))
/// otherwise. EINTR retries (conservatively with the full timeout —
/// precision on a coalesce window or the slow tick doesn't matter).
fn wait2(
    ino_fd: RawFd,
    sock_fd: Option<RawFd>,
    timeout_ms: i32,
) -> io::Result<Option<(bool, bool)>> {
    let mut pfds = [
        libc::pollfd {
            fd: ino_fd,
            events: libc::POLLIN,
            revents: 0,
        },
        libc::pollfd {
            // poll(2) ignores negative fds — the degraded-mode slot.
            fd: sock_fd.unwrap_or(-1),
            events: libc::POLLIN,
            revents: 0,
        },
    ];
    loop {
        let r = unsafe { libc::poll(pfds.as_mut_ptr(), pfds.len() as libc::nfds_t, timeout_ms) };
        if r == 0 {
            return Ok(None);
        }
        if r > 0 {
            // POLLHUP/POLLERR count as readable: the next read returns the
            // remaining data then EOF/error, which is the re-init signal.
            let ready =
                |p: &libc::pollfd| p.revents & (libc::POLLIN | libc::POLLHUP | libc::POLLERR) != 0;
            return Ok(Some((ready(&pfds[0]), ready(&pfds[1]))));
        }
        let e = io::Error::last_os_error();
        if e.kind() != io::ErrorKind::Interrupted {
            return Err(e);
        }
    }
}

/// A connected `.socket2.sock` event stream: nonblocking, with a carry
/// buffer for the partial line a read(2) may end on.
struct EventSock {
    stream: UnixStream,
    pending: Vec<u8>,
}

impl EventSock {
    /// None = degraded mode: no instance dir or the compositor isn't
    /// accepting — watch keeps running on agent state alone (the compositor
    /// section then rides `compositor_state`'s own failure path to null).
    /// Retried naturally on every re-init, so a compositor that appears
    /// later is picked up after the next transient error; the common
    /// restart case goes through the disconnect-EOF -> re-init path anyway.
    fn connect() -> Option<Self> {
        let stream = UnixStream::connect(ipc::event_socket_path()?).ok()?;
        stream.set_nonblocking(true).ok()?;
        Some(EventSock {
            stream,
            pending: Vec::new(),
        })
    }

    /// Read everything available (call only after poll said readable);
    /// Ok(true) when any COMPLETE line is a relevant event. EOF is an error
    /// on purpose — a closed event stream means the compositor went away,
    /// and the caller's re-init is the reconnect path.
    fn drain(&mut self) -> io::Result<bool> {
        let mut buf = [0u8; 4096];
        loop {
            match self.stream.read(&mut buf) {
                Ok(0) => return Err(io::Error::other("event socket closed (compositor gone)")),
                Ok(n) => self.pending.extend_from_slice(&buf[..n]),
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(e),
            }
        }
        let mut triggered = false;
        while let Some(nl) = self.pending.iter().position(|&b| b == b'\n') {
            let line: Vec<u8> = self.pending.drain(..=nl).collect();
            triggered |= event_triggers(&String::from_utf8_lossy(&line[..nl]));
        }
        Ok(triggered)
    }
}

/// Thin RAII wrapper over an inotify fd watching ONE directory.
struct Inotify {
    fd: RawFd,
}

impl Inotify {
    fn new(dir: &Path) -> io::Result<Self> {
        let fd = unsafe { libc::inotify_init1(libc::IN_CLOEXEC) };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        let ino = Inotify { fd }; // Drop closes the fd on the error paths below
        let cpath = CString::new(dir.as_os_str().as_bytes())
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "NUL in state dir path"))?;
        let mask = libc::IN_CREATE
            | libc::IN_MOVED_TO
            | libc::IN_DELETE
            | libc::IN_ATTRIB
            | libc::IN_MODIFY
            | libc::IN_CLOSE_WRITE;
        if unsafe { libc::inotify_add_watch(ino.fd, cpath.as_ptr(), mask) } < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(ino)
    }

    /// One read(2)'s worth of queued events (call only after `wait` said
    /// readable); Ok(true) when any of them warrants a recompute. Watch
    /// invalidation (dir deleted/unmounted) and queue overflow surface as
    /// errors so the caller re-initializes rather than trusting a blind spot.
    fn drain(&self) -> io::Result<bool> {
        let mut buf = [0u8; 4096];
        let n = loop {
            let n = unsafe { libc::read(self.fd, buf.as_mut_ptr().cast(), buf.len()) };
            if n >= 0 {
                break n as usize;
            }
            let e = io::Error::last_os_error();
            if e.kind() != io::ErrorKind::Interrupted {
                return Err(e);
            }
        };
        let mut triggered = false;
        for (mask, name) in parse_events(&buf[..n]) {
            if mask & (libc::IN_IGNORED | libc::IN_Q_OVERFLOW | libc::IN_UNMOUNT) != 0 {
                return Err(io::Error::other("inotify watch invalidated"));
            }
            triggered |= name_triggers(&name);
        }
        Ok(triggered)
    }
}

impl Drop for Inotify {
    fn drop(&mut self) {
        unsafe { libc::close(self.fd) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Encode one wire event the way the kernel does (name NUL-padded).
    fn ev(mask: u32, name: &str) -> Vec<u8> {
        let mut b = Vec::new();
        b.extend_from_slice(&1i32.to_ne_bytes()); // wd
        b.extend_from_slice(&mask.to_ne_bytes());
        b.extend_from_slice(&0u32.to_ne_bytes()); // cookie
        let padded = if name.is_empty() {
            0
        } else {
            (name.len() / 4 + 1) * 4
        };
        b.extend_from_slice(&(padded as u32).to_ne_bytes());
        b.extend_from_slice(name.as_bytes());
        b.resize(16 + padded, 0);
        b
    }

    #[test]
    fn parses_event_stream() {
        let mut buf = ev(libc::IN_CREATE, "sid-1");
        buf.extend(ev(libc::IN_MOVED_TO, ".widget.json"));
        buf.extend(ev(libc::IN_IGNORED, "")); // dir-level event, no name
        assert_eq!(
            parse_events(&buf),
            vec![
                (libc::IN_CREATE, "sid-1".to_string()),
                (libc::IN_MOVED_TO, ".widget.json".to_string()),
                (libc::IN_IGNORED, String::new()),
            ]
        );
        assert_eq!(parse_events(&[]), vec![]);
        // Truncated tail (short read can't split an event, but the parser
        // must never panic on garbage): the partial record is dropped.
        let cut = buf.len() - 4;
        assert_eq!(parse_events(&buf[..cut]).len(), 2);
    }

    #[test]
    fn trigger_filter_matches_polls_skip_rule() {
        assert!(name_triggers("0198f2-uuid")); // session file
        assert!(name_triggers("0198f2-uuid.9d0aa1")); // marker
        assert!(!name_triggers(".widget.json")); // our own output
        assert!(!name_triggers("..widget.json.tmp")); // our own temp
        assert!(!name_triggers(".sid.tmp")); // hook temp
        assert!(!name_triggers(".widget.lock"));
        assert!(!name_triggers("debug.log"));
        assert!(!name_triggers("")); // event about the dir itself
    }

    #[test]
    fn recompute_writes_only_on_change() {
        let dir = std::env::temp_dir().join(format!("bsctl-watch-ut-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let proj = dir.join("no-projects");
        let out = dir.join(OUTPUT_NAME);
        let no_comp = || Value::Null; // degraded-mode fetcher

        let mut prev = None;
        recompute(&dir, &proj, no_comp, &mut prev).unwrap();
        // The object schema: poll's array nested under "sessions",
        // compositor null in degraded mode, trailing newline.
        assert_eq!(
            fs::read(&out).unwrap(),
            b"{\"compositor\":null,\"sessions\":[]}\n"
        );

        // Unchanged output + file still present -> the write is skipped.
        // Provable without sleeping: a skipped write can't restore mtime, so
        // plant a sentinel mtime and check it survives.
        let past = std::time::SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000);
        let times = fs::FileTimes::new().set_accessed(past).set_modified(past);
        fs::File::options()
            .write(true)
            .open(&out)
            .unwrap()
            .set_times(times)
            .unwrap();
        recompute(&dir, &proj, no_comp, &mut prev).unwrap();
        assert_eq!(
            fs::metadata(&out).unwrap().modified().unwrap(),
            past,
            "no-change recompute must not rewrite"
        );

        // Missing output file -> rewritten even though the content matches.
        fs::remove_file(&out).unwrap();
        recompute(&dir, &proj, no_comp, &mut prev).unwrap();
        assert_eq!(
            fs::read(&out).unwrap(),
            b"{\"compositor\":null,\"sessions\":[]}\n"
        );

        // A state change -> rewritten with the new content.
        fs::write(
            dir.join("s1"),
            r#"{"ws":7,"status":"waiting","kind":"claude","title":"T","pid":1}"#,
        )
        .unwrap();
        recompute(&dir, &proj, no_comp, &mut prev).unwrap();
        let v: Value = serde_json::from_slice(&fs::read(&out).unwrap()).unwrap();
        assert_eq!(v["sessions"][0]["sid"], "s1");
        assert_eq!(v["sessions"][0]["ws"], 7);

        // A compositor change alone -> rewritten too.
        let comp = json!({"workspaces": [], "monitors": []});
        recompute(&dir, &proj, || comp.clone(), &mut prev).unwrap();
        let v: Value = serde_json::from_slice(&fs::read(&out).unwrap()).unwrap();
        assert_eq!(v["compositor"], comp);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn event_filter_relevant_vs_noise() {
        // Everything the recompute set covers, with and without payload.
        for ev in [
            "workspacev2>>3,name",
            "workspace>>3",
            "createworkspacev2>>11,11",
            "destroyworkspacev2>>11,11",
            "moveworkspacev2>>3,name,DP-1",
            "renameworkspace>>3,newname",
            "focusedmonv2>>DP-1,3",
            "monitoraddedv2>>1,DP-2,desc",
            "monitorremovedv2>>1,DP-2,desc",
            "openwindow>>abc123,3,kitty,fish",
            "closewindow>>abc123",
            "movewindowv2>>abc123,3,name",
            "activespecial>>special:magic,DP-1",
            "configreloaded>>",
        ] {
            assert!(event_triggers(ev), "{ev} must trigger");
        }
        // The documented noise + unknown events are ignored.
        for ev in [
            "windowtitle>>abc123",
            "windowtitlev2>>abc123,new title",
            "activewindow>>kitty,fish",
            "activewindowv2>>abc123",
            "openlayer>>noctalia-bar",
            "somefutureevent>>data",
            "not an event line",
            "",
        ] {
            assert!(!event_triggers(ev), "{ev} must not trigger");
        }
    }
}
