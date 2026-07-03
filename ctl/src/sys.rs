//! OS boundaries: state dir, atomic writes, mtimes, the /proc ancestor walk,
//! flock. The compositor boundary lives in [`crate::ipc`]; everything
//! downstream of it takes plain values so it stays testable.

use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::Value;

/// `${XDG_RUNTIME_DIR:-/tmp}/battlestation-ws` (an empty env var counts as
/// unset, like the sh `:-` default and python's `get(...) or "/tmp"`).
pub fn state_dir() -> PathBuf {
    env::var_os("XDG_RUNTIME_DIR")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("battlestation-ws")
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

/// Walk the /proc ancestor chain from this process: returns (all pids up to
/// init, the nearest ancestor whose comm matches the harness `kind`). The
/// harness process is assumed to be named after its kind — verified live for
/// both current kinds (`claude` → comm `claude`; `codex` → comm `codex`,
/// checked against a running `codex exec`, 2026-07-03). comm is the kernel's
/// 15-char truncation, so a kind longer than 15 chars matches on its
/// truncation; anything looser (prefix match) would self-match other
/// same-named tooling (this binary's own comm is `bsctl`; the sh reference's
/// was `claude-ws-statu`).
pub fn ancestor_chain(kind: &str) -> (Vec<i64>, Option<i64>) {
    let mut pids = Vec::new();
    let mut harness = None;
    let mut pid = std::process::id() as i64;
    while pid > 1 {
        pids.push(pid);
        if harness.is_none()
            && fs::read_to_string(format!("/proc/{pid}/comm"))
                .is_ok_and(|c| comm_matches_kind(c.trim_end_matches('\n'), kind))
        {
            harness = Some(pid);
        }
        match ppid_of(pid) {
            Some(p) if p != pid => pid = p,
            _ => break,
        }
    }
    (pids, harness)
}

/// [`ancestor_chain`]'s comm rule: exact match, or — for a kind longer than
/// comm's 15-char limit — the kernel's truncation of it (exactly 15 chars
/// AND a prefix of the kind; shorter prefixes are different names).
fn comm_matches_kind(comm: &str, kind: &str) -> bool {
    comm == kind || (comm.len() == 15 && kind.len() > 15 && kind.starts_with(comm))
}

/// PPID = field 4 of /proc/<pid>/stat. The comm field (2) is parenthesised
/// and may contain spaces or parens, so parse from after the LAST ')'.
fn ppid_of(pid: i64) -> Option<i64> {
    let stat = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let rest = &stat[stat.rfind(')')? + 1..];
    rest.split_whitespace().nth(1)?.parse().ok()
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
        let comm = std::fs::read_to_string(format!("/proc/{parent}/comm")).unwrap();
        let comm = comm.trim_end_matches('\n');
        let (_, found) = ancestor_chain(comm);
        assert_eq!(found, Some(parent));
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
