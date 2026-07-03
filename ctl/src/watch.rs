//! `bsctl watch` — long-lived daemon keeping `<state-dir>/.widget.json`
//! equal to `bsctl poll`'s output, so the widget FileView-watches ONE file
//! instead of running the poll on a timer. The poll pass itself is reused
//! wholesale ([`poll::poll`]); this module only owns *when* to run it and
//! the single-writer/failover discipline (contract in lib.rs).
//!
//! Never crash-loops: any transient error (state dir vanishing, inotify fd
//! error) is logged to stderr once, then re-initialized — including the
//! flock, so a wounded winner cleanly hands over to a blocked standby.

use std::convert::Infallible;
use std::ffi::CString;
use std::fs;
use std::io;
use std::os::fd::RawFd;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::{poll, sys};

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
/// inotify + the slow tick. Only ever returns an error; the caller re-inits.
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
    let ino = Inotify::new(dir)?;
    // On acquiring the lock (winner or successor): write once immediately.
    let mut prev: Option<String> = None;
    recompute(dir, proj, &mut prev)?;

    let mut deadline = Instant::now() + TICK;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if !ino.wait(remaining.as_millis().min(i32::MAX as u128) as i32)? {
            // Slow tick (poll(2) timeout expired).
            recompute(dir, proj, &mut prev)?;
            deadline = Instant::now() + TICK;
            continue;
        }
        if !ino.drain()? {
            // Dotfile/debug.log churn only (incl. our own output writes):
            // no recompute, and the tick deadline keeps running.
            continue;
        }
        // Coalesce the burst before recomputing once.
        let mut rounds = 0;
        while rounds < COALESCE_ROUNDS && ino.wait(COALESCE_MS)? {
            ino.drain()?;
            rounds += 1;
        }
        recompute(dir, proj, &mut prev)?;
        deadline = Instant::now() + TICK;
    }
}

/// Run the poll pass and rewrite the output file — but only when the
/// serialization changed (a FileView reload on every tick would wake the
/// widget pointlessly), or when the file is missing (manual deletion; the
/// no-change short-circuit would otherwise leave it gone until the state
/// next changes). Content is EXACTLY `bsctl poll`'s stdout: the compact
/// JSON array + trailing newline.
fn recompute(dir: &Path, proj: &Path, prev: &mut Option<String>) -> io::Result<()> {
    let content = format!("{}\n", Value::Array(poll::poll(sys::now_f64(), dir, proj)));
    if prev.as_deref() != Some(content.as_str()) || !dir.join(OUTPUT_NAME).exists() {
        write_atomic(dir, &content)?;
    }
    *prev = Some(content);
    Ok(())
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

    /// poll(2) on the fd: Ok(true) = readable, Ok(false) = timeout hit.
    /// EINTR retries (conservatively with the full timeout — precision on a
    /// coalesce window or the slow tick doesn't matter).
    fn wait(&self, timeout_ms: i32) -> io::Result<bool> {
        let mut pfd = libc::pollfd {
            fd: self.fd,
            events: libc::POLLIN,
            revents: 0,
        };
        loop {
            let r = unsafe { libc::poll(&mut pfd, 1, timeout_ms) };
            if r >= 0 {
                return Ok(r > 0);
            }
            let e = io::Error::last_os_error();
            if e.kind() != io::ErrorKind::Interrupted {
                return Err(e);
            }
        }
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

        let mut prev = None;
        recompute(&dir, &proj, &mut prev).unwrap();
        assert_eq!(fs::read(&out).unwrap(), b"[]\n"); // poll's array + newline

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
        recompute(&dir, &proj, &mut prev).unwrap();
        assert_eq!(
            fs::metadata(&out).unwrap().modified().unwrap(),
            past,
            "no-change recompute must not rewrite"
        );

        // Missing output file -> rewritten even though the content matches.
        fs::remove_file(&out).unwrap();
        recompute(&dir, &proj, &mut prev).unwrap();
        assert_eq!(fs::read(&out).unwrap(), b"[]\n");

        // A state change -> rewritten with the new content.
        fs::write(
            dir.join("s1"),
            r#"{"ws":7,"status":"waiting","kind":"claude","title":"T","pid":1}"#,
        )
        .unwrap();
        recompute(&dir, &proj, &mut prev).unwrap();
        let v: Value = serde_json::from_slice(&fs::read(&out).unwrap()).unwrap();
        assert_eq!(v[0]["sid"], "s1");
        assert_eq!(v[0]["ws"], 7);

        let _ = fs::remove_dir_all(&dir);
    }
}
