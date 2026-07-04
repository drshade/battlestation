//! The streaming engine behind every `--stream` query: re-emit the query's
//! full result to stdout (one line per emission) whenever it changes.
//! Subscribers spawn `bsctl <query> --format json --stream` and read lines
//! — nothing watches bsctl's state files but bsctl (contract in lib.rs).
//! Three wake sources fold into one re-evaluation: inotify on the runtime
//! agent-state dir AND on the persistent map/prefs dir, Hyprland's
//! `.socket2.sock` event stream, and a slow tick (session pids dying and
//! markers aging are invisible to inotify). Emissions are deduped on the
//! serialized result, so a subscriber is never woken for nothing.
//!
//! ONE deliberate exception to the engine's read-only role: on
//! `monitoraddedv2` the arriving output's workspace->display preferences
//! are applied ([`crate::ws::apply_preferences`], contract in lib.rs).
//! Several subscribers may be streaming at once, so a single APPLIER is
//! elected with a nonblocking flock — see `Applier` below. Removals need
//! nothing: preferences are stamped at intent time, never at teardown time,
//! so there is no race against Hyprland's own evacuation.
//!
//! Never crash-loops: any transient error (state dir vanishing, inotify fd
//! error, socket2 disconnect on compositor restart) is logged to stderr
//! once, then re-initialized. If socket2 can't connect at all (no
//! Hyprland running), the stream still works DEGRADED: file events keep
//! flowing, compositor-derived sections ride the queries' failure path
//! (null). A closed stdout (the subscriber went away) is the one CLEAN
//! exit: streaming to nobody is done, not broken.

use std::ffi::CString;
use std::fs;
use std::io::{self, Read, Write};
use std::os::fd::{AsRawFd, RawFd};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::{Duration, Instant};

use crate::{ipc, sys, ws};

/// Slow tick: inotify cannot see session pids dying or markers aging past
/// the GC window, so re-evaluate unconditionally this often.
const TICK: Duration = Duration::from_secs(10);
/// After a triggering event, drain further events this long before
/// re-evaluating once (hooks write session file + marker in one burst).
const COALESCE_MS: i32 = 50;
/// Cap on coalescing rounds so a steady event stream can't starve emission.
const COALESCE_ROUNDS: u32 = 10;

/// The single-applier election. Every streaming process may see a
/// `monitoraddedv2`, but only one may dispatch the preference apply — two
/// bars mean two subscribers, and a double apply would double the dispatch
/// and the focus churn. The first process to see an arrival wins a
/// NONBLOCKING flock on `<state-dir>/.apply.lock` and keeps it for its
/// lifetime; losers skip, knowing a winner exists. When the winner dies the
/// kernel drops its lock, so the next arrival event elects a survivor.
struct Applier {
    lock: Option<fs::File>,
}

impl Applier {
    fn new() -> Self {
        Applier { lock: None }
    }

    /// Apply the arriving outputs' preferences iff this process is (or just
    /// became) the elected applier. Best-effort: a failed apply leaves the
    /// preferences in place for `ws prefs reconcile`.
    fn apply(&mut self, added: &[String], dir: &Path) {
        if added.is_empty() {
            return;
        }
        if self.lock.is_none()
            && let Ok(f) = fs::File::create(dir.join(".apply.lock"))
            && sys::flock_exclusive(&f, true)
        {
            self.lock = Some(f);
        }
        if self.lock.is_none() {
            return; // another subscriber holds the apply role
        }
        for mon in added {
            let _ = ws::apply_preferences(Some(mon));
        }
    }
}

/// Emission dedupe: has the serialized result changed since the last
/// emission? (True for the very first result — `prev` starts None.)
pub fn changed(prev: &mut Option<String>, cur: &str) -> bool {
    if prev.as_deref() == Some(cur) {
        return false;
    }
    *prev = Some(cur.to_string());
    true
}

/// Drive `query` forever: emit its result now, then whenever it changes.
/// An `Err` from the FIRST evaluation aborts with that exit code (a bad
/// selector must fail loudly at spawn time, not stream nothing); later
/// `Err`s skip the emission (mid-stream flux resolves by the next event).
/// Exits 0 when stdout closes — the subscriber is done, not broken.
pub fn run(mut query: impl FnMut() -> Result<String, i32>) -> i32 {
    let mut prev: Option<String> = None;
    match query() {
        Err(code) => return code,
        Ok(cur) => {
            if emit(&mut prev, &cur).is_err() {
                return 0; // subscriber already gone
            }
        }
    }
    let mut applier = Applier::new();
    let mut last_err = String::new();
    loop {
        match session(&mut query, &mut prev, &mut applier) {
            Ok(()) => return 0, // stdout closed: clean finish
            Err(e) => {
                // Log once per distinct failure, not once per retry — a
                // permanently broken environment must not fill the
                // subscriber's stderr at 1 line/s.
                let msg = e.to_string();
                if msg != last_err {
                    eprintln!("bsctl --stream: {msg}; re-initializing");
                    last_err = msg;
                }
                std::thread::sleep(Duration::from_secs(1));
            }
        }
    }
}

/// One init-to-error lifetime: watch both state dirs + socket2 + the slow
/// tick, re-evaluating and emitting on change. Ok(()) = stdout closed (the
/// clean finish, surfaced as BrokenPipe by [`emit`]); Err = transient, the
/// caller re-inits — which is also the socket2 reconnect path after a
/// compositor restart.
fn session(
    query: &mut impl FnMut() -> Result<String, i32>,
    prev: &mut Option<String>,
    applier: &mut Applier,
) -> io::Result<()> {
    let dir = sys::state_dir();
    fs::create_dir_all(&dir)?;
    let files_dir = ws::map_file()
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_default();
    fs::create_dir_all(&files_dir)?;
    // One inotify fd, two watches: the runtime agent-state dir and the
    // persistent map/prefs dir. One trigger filter serves both — each dir's
    // protocol files are exactly its non-dot entries.
    let ino = Inotify::new(&[&dir, &files_dir])?;
    // Compositor events; None = degraded mode (no Hyprland), files-only.
    let mut sock = EventSock::connect();
    // Re-init entry: state may have moved while we were broken (dedupe
    // makes this free when it didn't).
    let done = maybe_emit(query, prev)?;
    if done {
        return Ok(());
    }

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
            if maybe_emit(query, prev)? {
                return Ok(());
            }
            deadline = Instant::now() + TICK;
            continue;
        };
        let mut triggered = false;
        let mut plug = Drained::default();
        if ino_ready {
            triggered |= ino.drain()?;
        }
        if sock_ready {
            // A socket error here (EOF = compositor restart/exit) propagates
            // to the re-init path, which reconnects — or degrades if the
            // compositor is really gone.
            plug.merge(sock.as_mut().unwrap().drain()?);
            triggered |= plug.triggered;
        }
        if !triggered {
            // Dotfile churn or irrelevant compositor events only: no
            // re-evaluation, and the tick deadline keeps running.
            continue;
        }
        // Coalesce the burst (from EITHER source) before re-evaluating once.
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
                plug.merge(sock.as_mut().unwrap().drain()?);
            }
            rounds += 1;
        }
        if maybe_emit(query, prev)? {
            return Ok(());
        }
        // Apply the arriving outputs' preferences AFTER emitting, so the
        // subscriber sees the world before the moves land, then again as
        // they do (the apply's own move events wake the loop). This is the
        // engine's ONLY dispatch — everywhere else it is a passive mirror
        // (scope rule in lib.rs) — and it is single-elected: see [`Applier`].
        applier.apply(&plug.added, &dir);
        deadline = Instant::now() + TICK;
    }
}

/// Re-evaluate and emit if changed. Ok(true) = stdout closed (clean
/// finish); a query error mid-stream skips the emission (the state is in
/// flux — e.g. a filtered display mid-replug — and the next event
/// re-evaluates); any other write error is transient.
fn maybe_emit(
    query: &mut impl FnMut() -> Result<String, i32>,
    prev: &mut Option<String>,
) -> io::Result<bool> {
    let Ok(cur) = query() else {
        return Ok(false);
    };
    match emit(prev, &cur) {
        Ok(()) => Ok(false),
        Err(e) if e.kind() == io::ErrorKind::BrokenPipe => Ok(true),
        Err(e) => Err(e),
    }
}

/// Write one emission line + flush, deduped via [`changed`].
fn emit(prev: &mut Option<String>, cur: &str) -> io::Result<()> {
    if !changed(prev, cur) {
        return Ok(());
    }
    let mut out = io::stdout().lock();
    writeln!(out, "{cur}")?;
    out.flush()
}

/// Should an event on this dir entry trigger a re-evaluation? Non-dot,
/// non-debug.log names are exactly the protocol files (session files and
/// markers in the runtime dir; `map` and `prefs` in the persistent dir) —
/// dotfiles are writers' in-flight temp files and locks, and debug.log is
/// diagnostics. An empty name is an event about a watched dir itself, not
/// an entry.
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

/// The arriving output's name from a `monitoraddedv2>>id,name,description`
/// line — the only plug event the engine acts on (a removal needs nothing:
/// preferences are stamped at intent time, and Hyprland's evacuation is
/// left alone). V2 only, because Hyprland emits the legacy
/// `monitoradded>>name` alongside and classifying both would apply the
/// preferences twice. Only the description can contain commas, so the
/// payload splits into at most 3 fields and the name is the second; a
/// malformed payload (missing or empty name) is ignored.
pub fn added_monitor(line: &str) -> Option<String> {
    let payload = line.strip_prefix("monitoraddedv2>>")?;
    let mut fields = payload.splitn(3, ',');
    let _id = fields.next()?;
    let name = fields.next()?;
    (!name.is_empty()).then(|| name.to_string())
}

/// What one socket drain produced: whether any complete line warrants a
/// recompute, plus the outputs that arrived ([`added_monitor`], in arrival
/// order). The session loop merges drains across the coalesce window so a
/// burst's arrivals are applied exactly once, after coalescing.
#[derive(Default)]
pub struct Drained {
    pub triggered: bool,
    pub added: Vec<String>,
}

impl Drained {
    fn merge(&mut self, other: Drained) {
        self.triggered |= other.triggered;
        self.added.extend(other.added);
    }
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
    /// `triggered` when any COMPLETE line is a relevant event, plus the
    /// arriving outputs among them ([`added_monitor`]). EOF is an error
    /// on purpose — a closed event stream means the compositor went away,
    /// and the caller's re-init is the reconnect path.
    fn drain(&mut self) -> io::Result<Drained> {
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
        let mut d = Drained::default();
        while let Some(nl) = self.pending.iter().position(|&b| b == b'\n') {
            let line: Vec<u8> = self.pending.drain(..=nl).collect();
            let line = String::from_utf8_lossy(&line[..nl]);
            d.triggered |= event_triggers(&line);
            if let Some(name) = added_monitor(&line) {
                d.added.push(name);
            }
        }
        Ok(d)
    }
}

/// Thin RAII wrapper over ONE inotify fd watching every given directory
/// (one watch descriptor per dir; [`parse_events`] reads only mask + name,
/// so the shared [`name_triggers`] filter serves all of them).
struct Inotify {
    fd: RawFd,
}

impl Inotify {
    fn new(dirs: &[&Path]) -> io::Result<Self> {
        let fd = unsafe { libc::inotify_init1(libc::IN_CLOEXEC) };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        let ino = Inotify { fd }; // Drop closes the fd on the error paths below
        let mask = libc::IN_CREATE
            | libc::IN_MOVED_TO
            | libc::IN_DELETE
            | libc::IN_ATTRIB
            | libc::IN_MODIFY
            | libc::IN_CLOSE_WRITE;
        for dir in dirs {
            let cpath = CString::new(dir.as_os_str().as_bytes()).map_err(|_| {
                io::Error::new(io::ErrorKind::InvalidInput, "NUL in state dir path")
            })?;
            if unsafe { libc::inotify_add_watch(ino.fd, cpath.as_ptr(), mask) } < 0 {
                return Err(io::Error::last_os_error());
            }
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
        buf.extend(ev(libc::IN_MOVED_TO, ".map.tmp"));
        buf.extend(ev(libc::IN_IGNORED, "")); // dir-level event, no name
        assert_eq!(
            parse_events(&buf),
            vec![
                (libc::IN_CREATE, "sid-1".to_string()),
                (libc::IN_MOVED_TO, ".map.tmp".to_string()),
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
    fn trigger_filter_covers_both_watched_dirs() {
        // runtime dir: session files and markers trigger
        assert!(name_triggers("0198f2-uuid")); // session file
        assert!(name_triggers("0198f2-uuid.9d0aa1")); // marker
        // persistent dir: the map and prefs files trigger
        assert!(name_triggers("map"));
        assert!(name_triggers("prefs"));
        // dotfiles are temp writes and locks; debug.log is diagnostics
        assert!(!name_triggers(".sid.tmp")); // hook temp
        assert!(!name_triggers(".map.tmp")); // map temp
        assert!(!name_triggers(".prefs.tmp")); // prefs temp
        assert!(!name_triggers(".lock")); // state-file write lock
        assert!(!name_triggers(".apply.lock")); // applier election
        assert!(!name_triggers("debug.log"));
        assert!(!name_triggers("")); // event about a watched dir itself
    }

    #[test]
    fn emission_dedupes_on_serialized_result() {
        let mut prev = None;
        assert!(changed(&mut prev, "a"), "first result always emits");
        assert!(!changed(&mut prev, "a"), "same result never re-emits");
        assert!(changed(&mut prev, "b"), "a change emits");
        assert!(!changed(&mut prev, "b"));
        assert!(changed(&mut prev, "a"), "a change BACK emits too");
    }

    #[test]
    fn added_monitor_parses_v2_only() {
        // v2 lines yield the name; the description may contain commas.
        assert_eq!(
            added_monitor("monitoraddedv2>>1,DP-2,Dell Inc. U2720Q"),
            Some("DP-2".to_string())
        );
        assert_eq!(
            added_monitor("monitoraddedv2>>1,DP-2,Dell Inc, U2720Q, rev A"),
            Some("DP-2".to_string())
        );
        // missing description still parses (splitn tolerates 2 fields)
        assert_eq!(
            added_monitor("monitoraddedv2>>1,eDP-1"),
            Some("eDP-1".to_string())
        );
        // the legacy line is DELIBERATELY ignored: Hyprland sends it
        // alongside v2 and classifying both would apply preferences twice —
        // and removals need nothing from watch at all.
        assert_eq!(added_monitor("monitoradded>>DP-2"), None);
        assert_eq!(added_monitor("monitorremovedv2>>1,DP-2,desc"), None);
        // malformed payloads and unrelated/junk lines are ignored
        assert_eq!(added_monitor("monitoraddedv2>>"), None);
        assert_eq!(added_monitor("monitoraddedv2>>1,"), None);
        assert_eq!(added_monitor("workspacev2>>3,name"), None);
        assert_eq!(added_monitor("not an event line"), None);
        assert_eq!(added_monitor(""), None);
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
