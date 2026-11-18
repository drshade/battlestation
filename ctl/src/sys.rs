//! OS boundaries: state dir, atomic writes, mtimes, the process ancestor
//! walk and liveness, flock. Platform differences (Linux /proc vs the macOS
//! libproc/confstr APIs) are confined to this file. The compositor boundary lives in [`crate::ipc`]; everything
//! downstream of it takes plain values so it stays testable.

use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::Value;

/// The user runtime dir every bsctl state path hangs off: XDG_RUNTIME_DIR
/// when set and non-empty, else a per-user dir derived WITHOUT the
/// environment, else `/tmp`. The env-free rung is load-bearing: some
/// harnesses spawn MCP servers with a SCRUBBED environment (seen live: Codex
/// 0.142.5 dropped XDG_RUNTIME_DIR, so the server wrote a parallel store
/// under /tmp and read an empty session dir — asks landed where no reader
/// looks). An env-free derivation lets a scrubbed child still converge on
/// the same dirs as everyone else:
/// - Linux: the systemd-standard `/run/user/<uid>`, if it exists.
/// - macOS: the per-user temp dir from `confstr(_CS_DARWIN_USER_TEMP_DIR)`
///   (the `/var/folders/…/T/` that `$TMPDIR` normally names — but read
///   from the system, so a scrubbed `$TMPDIR` changes nothing).
pub fn runtime_dir() -> PathBuf {
    if let Some(v) = env::var_os("XDG_RUNTIME_DIR").filter(|v| !v.is_empty()) {
        return PathBuf::from(v);
    }
    if let Some(d) = os_runtime_dir() {
        return d;
    }
    PathBuf::from("/tmp")
}

#[cfg(target_os = "linux")]
fn os_runtime_dir() -> Option<PathBuf> {
    let run = PathBuf::from(format!("/run/user/{}", unsafe { libc::getuid() }));
    run.is_dir().then_some(run)
}

#[cfg(target_os = "macos")]
fn os_runtime_dir() -> Option<PathBuf> {
    let mut buf = vec![0u8; 1024];
    let n = unsafe {
        libc::confstr(
            libc::_CS_DARWIN_USER_TEMP_DIR,
            buf.as_mut_ptr().cast(),
            buf.len(),
        )
    };
    if n == 0 || n > buf.len() {
        return None;
    }
    buf.truncate(n - 1); // n counts the trailing NUL
    let d = PathBuf::from(std::ffi::OsString::from(
        String::from_utf8(buf).ok()?.trim_end_matches('/'),
    ));
    d.is_dir().then_some(d)
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn os_runtime_dir() -> Option<PathBuf> {
    None
}

/// `<runtime-dir>/battlestation-ws` — the agent-session state dir.
pub fn state_dir() -> PathBuf {
    runtime_dir().join("battlestation-ws")
}

/// Temp-file + rename in the same dir keeps every write atomic for the
/// poller. Temp names are `.<name>.tmp` — the leading dot is what makes the
/// pollers' dotfile skip (and `clear`'s `<sid>.*` sweep) ignore them.
pub fn atomic_write_json(dir: &Path, name: &str, rec: &Value) -> io::Result<()> {
    let tmp = dir.join(format!(".{name}.tmp"));
    fs::write(&tmp, rec.to_string())?;
    fs::rename(&tmp, dir.join(name))
}

/// `os.utime(p)` — bump atime+mtime to now (the marker-refresh fast path).
pub fn touch_now(path: &Path) -> io::Result<()> {
    let now = SystemTime::now();
    let times = fs::FileTimes::new().set_accessed(now).set_modified(now);
    fs::File::options().write(true).open(path)?.set_times(times)
}

/// st_mtime as float seconds since the epoch.
pub fn mtime_f64(md: &fs::Metadata) -> Option<f64> {
    md.modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs_f64())
}

/// `time.time()`.
pub fn now_f64() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

/// Walk the process ancestor chain from this process: returns (all pids up
/// to init, the nearest ancestor whose process name matches the harness
/// `kind`). The harness process is assumed to be named after its kind —
/// verified live for both current kinds (`claude` → comm `claude`; `codex` →
/// comm `codex`, checked against a running `codex exec`, 2026-07-03; same
/// names on macOS, where both ship as native binaries). The name is the
/// kernel's 15-char truncation on both platforms (Linux `comm`, macOS
/// `pbi_comm`), so a kind longer than 15 chars matches on its truncation;
/// anything looser (prefix match) would self-match other same-named tooling
/// (this binary's own name is `bsctl`; the sh reference's was
/// `claude-ws-statu`).
pub fn ancestor_chain(kind: &str) -> (Vec<i64>, Option<i64>) {
    let mut pids = Vec::new();
    let mut harness = None;
    let mut pid = std::process::id() as i64;
    while pid > 1 {
        pids.push(pid);
        let info = proc_info(pid);
        if harness.is_none()
            && info
                .as_ref()
                .is_some_and(|(_, comm)| comm_matches_kind(comm, kind))
        {
            harness = Some(pid);
        }
        match info {
            Some((p, _)) if p != pid => pid = p,
            _ => break,
        }
    }
    (pids, harness)
}

/// [`ancestor_chain`]'s name rule: exact match, or — for a kind longer than
/// the kernel's 15-char name limit — its truncation (exactly 15 chars AND a
/// prefix of the kind; shorter prefixes are different names).
fn comm_matches_kind(comm: &str, kind: &str) -> bool {
    comm == kind || (comm.len() == 15 && kind.len() > 15 && kind.starts_with(comm))
}

/// (ppid, process name) of a live pid; None when the pid is gone or
/// unreadable.
///
/// Linux: PPID = field 4 of /proc/<pid>/stat (the comm field (2) is
/// parenthesised and may contain spaces or parens, so parse from after the
/// LAST ')'); the name is /proc/<pid>/comm.
#[cfg(target_os = "linux")]
pub fn proc_info(pid: i64) -> Option<(i64, String)> {
    let stat = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let rest = &stat[stat.rfind(')')? + 1..];
    let ppid = rest.split_whitespace().nth(1)?.parse().ok()?;
    let comm = fs::read_to_string(format!("/proc/{pid}/comm")).ok()?;
    Some((ppid, comm.trim_end_matches('\n').to_string()))
}

/// macOS: one `proc_pidinfo(PROC_PIDT_SHORTBSDINFO)` call yields both the
/// ppid and `pbsi_comm` (the executable name, NUL-terminated, ≤ 15 chars).
/// The SHORT flavor on purpose: the full `PROC_PIDTBSDINFO` is refused for
/// another user's process, and every Terminal.app session has the
/// root-owned `login` in its ancestry — the walk stopped there, one step
/// short of the terminal, so no Terminal.app session was ever placed
/// (found live, macOS 27). libc doesn't bind the struct; it is
/// `struct proc_bsdshortinfo` from <sys/proc_info.h> (64 bytes).
#[cfg(target_os = "macos")]
pub fn proc_info(pid: i64) -> Option<(i64, String)> {
    #[repr(C)]
    struct ProcBsdShortInfo {
        pid: u32,
        ppid: u32,
        pgid: u32,
        status: u32,
        comm: [libc::c_char; 16], // MAXCOMLEN
        flags: u32,
        uid: u32,
        gid: u32,
        ruid: u32,
        rgid: u32,
        svuid: u32,
        svgid: u32,
        rfu: u32,
    }
    const PROC_PIDT_SHORTBSDINFO: libc::c_int = 13;
    let mut info: ProcBsdShortInfo = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of::<ProcBsdShortInfo>() as libc::c_int;
    let n = unsafe {
        libc::proc_pidinfo(
            pid as libc::c_int,
            PROC_PIDT_SHORTBSDINFO,
            0,
            (&mut info as *mut ProcBsdShortInfo).cast(),
            size,
        )
    };
    if n != size {
        return None;
    }
    let comm: Vec<u8> = info
        .comm
        .iter()
        .take_while(|&&c| c != 0)
        .map(|&c| c as u8)
        .collect();
    Some((info.ppid as i64, String::from_utf8_lossy(&comm).into_owned()))
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub fn proc_info(_pid: i64) -> Option<(i64, String)> {
    None
}

/// Is this pid a live process? `kill(pid, 0)`: success, or EPERM (alive
/// but owned by someone else), means alive. Portable across Linux and
/// macOS, unlike the old `/proc/<pid>` existence test. Non-positive pids
/// are never alive (kill(0|-n) would address process GROUPS).
pub fn pid_alive(pid: i64) -> bool {
    if pid <= 0 || pid > i64::from(libc::pid_t::MAX) {
        return false;
    }
    if unsafe { libc::kill(pid as libc::pid_t, 0) } == 0 {
        return true;
    }
    io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

/// flock(2) on an open file — the usage cache's cross-process serialization
/// (sh reference: flock(1) on fd 9). Blocking mode retries on EINTR, like
/// flock(1); the lock is released when `f` closes (process exit).
pub fn flock_exclusive(f: &fs::File, nonblocking: bool) -> bool {
    use std::os::fd::AsRawFd;
    let op = libc::LOCK_EX | if nonblocking { libc::LOCK_NB } else { 0 };
    loop {
        if unsafe { libc::flock(f.as_raw_fd(), op) } == 0 {
            return true;
        }
        if io::Error::last_os_error().kind() != io::ErrorKind::Interrupted {
            return false;
        }
    }
}

/// UTC "%F %T" for debug.log lines. (The sh reference logs local time via
/// date(1); UTC here keeps the binary dependency-free. debug.log is
/// diagnostics, not protocol — the poll side skips it by name.)
pub fn debug_ts() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0) as i64;
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    let (y, m, d) = civil_from_days(days);
    format!(
        "{y:04}-{m:02}-{d:02} {:02}:{:02}:{:02}",
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60
    )
}

/// Days-since-epoch -> (year, month, day). Howard Hinnant's civil_from_days.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn civil_dates() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(19_723), (2024, 1, 1)); // date -ud @1704067200
        assert_eq!(civil_from_days(1_751_414_400 / 86_400), (2025, 7, 2));
        assert_eq!(civil_from_days(11_016), (2000, 2, 29)); // leap day
    }

    #[test]
    fn state_dir_shape() {
        // Can't mutate the env safely in tests; just check the suffix contract.
        assert!(state_dir().ends_with("battlestation-ws"));
    }

    #[test]
    fn ancestors_include_self_and_parent() {
        let (pids, _harness) = ancestor_chain("claude");
        assert_eq!(pids.first().copied(), Some(std::process::id() as i64));
        assert!(pids.contains(&(std::os::unix::process::parent_id() as i64)));
    }

    #[test]
    fn ancestor_kind_match_is_exact_with_truncation() {
        // The test binary's own comm is the crate's test-bin name, never a
        // harness kind — an unrelated kind must find no harness ancestor...
        let (_, none) = ancestor_chain("no-such-harness-kind");
        assert_eq!(none, None);
        // ...while a kind that IS an ancestor's comm matches: walk our real
        // chain and reuse the first ancestor's comm as the kind.
        let parent = std::os::unix::process::parent_id() as i64;
        let (_, comm) = proc_info(parent).unwrap();
        let (_, found) = ancestor_chain(&comm);
        assert_eq!(found, Some(parent));
    }

    #[test]
    fn ancestor_walk_reaches_init() {
        // Regression: the walk must not stop at a process owned by another
        // user (macOS Terminal.app sessions descend from root's `login`).
        // Reaching init means the last pid walked is a child of pid 1.
        let (pids, _) = ancestor_chain("no-such-harness-kind");
        let last = *pids.last().unwrap();
        assert_eq!(proc_info(last).map(|(ppid, _)| ppid), Some(1), "chain {pids:?}");
    }

    #[test]
    fn pid_liveness() {
        assert!(pid_alive(std::process::id() as i64));
        assert!(pid_alive(1)); // init/launchd: alive, EPERM for non-root
        assert!(!pid_alive(0));
        assert!(!pid_alive(-5));
        assert!(!pid_alive(i64::MAX));
    }

    #[test]
    fn runtime_dir_is_absolute() {
        assert!(runtime_dir().is_absolute());
    }

    #[test]
    fn comm_kind_match_is_exact_with_truncation() {
        assert!(comm_matches_kind("claude", "claude"));
        assert!(comm_matches_kind("codex", "codex"));
        // no prefix matching for short kinds (claude-* tooling must not match)
        assert!(!comm_matches_kind("claude-ws-statu", "claude"));
        assert!(!comm_matches_kind("clau", "claude"));
        // 15-char truncation rule: a >15-char kind matches its truncated comm
        let long = "a-very-long-harness-name";
        assert!(comm_matches_kind(&long[..15], long));
        assert!(!comm_matches_kind(&long[..14], long)); // not a comm truncation
        assert!(!comm_matches_kind("a-very-long-hXr", long)); // 15 chars, wrong
    }
}
