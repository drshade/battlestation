//! The directory watcher behind the `--stream` engine: one pollable fd that
//! reads ready when any watched dir changes, plus a `drain` that says
//! whether the change warrants a re-evaluation.
//!
//! - Linux: inotify, with per-entry names, so the shared
//!   [`crate::stream::name_triggers`] filter skips writers' dotfile temps.
//! - macOS: kqueue `EVFILT_VNODE` on each dir's fd. A directory's
//!   `NOTE_WRITE` fires when an entry is added, removed or renamed — which
//!   covers every bsctl writer, since all of them publish by atomic rename.
//!   kqueue carries no entry names, so dotfile churn also triggers; the
//!   engine's emission dedupe makes that free. In-place changes — marker
//!   mtime refreshes, or a human editing `map`/`prefs` with an editor that
//!   writes in place — are invisible here and ride the engine's slow tick.
//!
//! Both surface watch invalidation (dir deleted/renamed/unmounted, inotify
//! queue overflow) as an error, so the engine re-initializes rather than
//! trusting a blind spot.

use std::io;
use std::os::fd::RawFd;
use std::path::Path;

#[cfg(target_os = "linux")]
pub use linux::Watcher;
#[cfg(target_os = "macos")]
pub use macos::Watcher;

#[cfg(target_os = "linux")]
mod linux {
    use super::*;
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;

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

    /// Thin RAII wrapper over ONE inotify fd watching every given directory
    /// (one watch descriptor per dir; [`parse_events`] reads only mask + name,
    /// so the shared [`crate::stream::name_triggers`] filter serves all of them).
    pub struct Watcher {
        fd: RawFd,
    }

    impl Watcher {
        pub fn new(dirs: &[&Path]) -> io::Result<Self> {
            let fd = unsafe { libc::inotify_init1(libc::IN_CLOEXEC) };
            if fd < 0 {
                return Err(io::Error::last_os_error());
            }
            let ino = Watcher { fd }; // Drop closes the fd on the error paths below
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

        pub fn fd(&self) -> RawFd {
            self.fd
        }

        /// One read(2)'s worth of queued events (call only after `wait` said
        /// readable); Ok(true) when any of them warrants a recompute. Watch
        /// invalidation (dir deleted/unmounted) and queue overflow surface as
        /// errors so the caller re-initializes rather than trusting a blind spot.
        pub fn drain(&self) -> io::Result<bool> {
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
                triggered |= crate::stream::name_triggers(&name);
            }
            Ok(triggered)
        }
    }

    impl Drop for Watcher {
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
    }
}

#[cfg(target_os = "macos")]
mod macos {
    use super::*;
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;

    /// The vnode events that count as "this dir changed" (WRITE: an entry
    /// was added/removed/renamed; EXTEND/ATTRIB: belt and braces) vs "this
    /// watch is dead" (the dir itself deleted/renamed/revoked).
    const CHANGED: u32 = libc::NOTE_WRITE | libc::NOTE_EXTEND | libc::NOTE_ATTRIB;
    const INVALID: u32 = libc::NOTE_DELETE | libc::NOTE_RENAME | libc::NOTE_REVOKE;

    /// One kqueue (pollable: it reads ready while events are queued) plus
    /// one `O_EVTONLY` fd per watched dir, all closed on drop.
    pub struct Watcher {
        kq: RawFd,
        dirs: Vec<RawFd>,
    }

    impl Watcher {
        pub fn new(dirs: &[&Path]) -> io::Result<Self> {
            let kq = unsafe { libc::kqueue() };
            if kq < 0 {
                return Err(io::Error::last_os_error());
            }
            unsafe { libc::fcntl(kq, libc::F_SETFD, libc::FD_CLOEXEC) };
            let mut w = Watcher {
                kq,
                dirs: Vec::new(),
            }; // Drop closes everything on the error paths below
            let mut changes = Vec::new();
            for dir in dirs {
                let cpath = CString::new(dir.as_os_str().as_bytes()).map_err(|_| {
                    io::Error::new(io::ErrorKind::InvalidInput, "NUL in state dir path")
                })?;
                let fd = unsafe { libc::open(cpath.as_ptr(), libc::O_EVTONLY | libc::O_CLOEXEC) };
                if fd < 0 {
                    return Err(io::Error::last_os_error());
                }
                w.dirs.push(fd);
                changes.push(libc::kevent {
                    ident: fd as libc::uintptr_t,
                    filter: libc::EVFILT_VNODE,
                    flags: libc::EV_ADD | libc::EV_CLEAR,
                    fflags: CHANGED | INVALID,
                    data: 0,
                    udata: std::ptr::null_mut(),
                });
            }
            let r = unsafe {
                libc::kevent(
                    w.kq,
                    changes.as_ptr(),
                    changes.len() as libc::c_int,
                    std::ptr::null_mut(),
                    0,
                    std::ptr::null(),
                )
            };
            if r < 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(w)
        }

        pub fn fd(&self) -> RawFd {
            self.kq
        }

        /// Collect everything queued without blocking; Ok(true) when any
        /// dir changed. An invalidated watch is an error (re-init).
        pub fn drain(&self) -> io::Result<bool> {
            let zero = libc::timespec {
                tv_sec: 0,
                tv_nsec: 0,
            };
            let mut evs: [libc::kevent; 32] = unsafe { std::mem::zeroed() };
            let mut triggered = false;
            loop {
                let n = unsafe {
                    libc::kevent(
                        self.kq,
                        std::ptr::null(),
                        0,
                        evs.as_mut_ptr(),
                        evs.len() as libc::c_int,
                        &zero,
                    )
                };
                if n < 0 {
                    let e = io::Error::last_os_error();
                    if e.kind() == io::ErrorKind::Interrupted {
                        continue;
                    }
                    return Err(e);
                }
                for ev in &evs[..n as usize] {
                    if ev.fflags & INVALID != 0 {
                        return Err(io::Error::other("kqueue watch invalidated"));
                    }
                    triggered |= ev.fflags & CHANGED != 0;
                }
                if (n as usize) < evs.len() {
                    return Ok(triggered);
                }
            }
        }
    }

    impl Drop for Watcher {
        fn drop(&mut self) {
            for &fd in &self.dirs {
                unsafe { libc::close(fd) };
            }
            unsafe { libc::close(self.kq) };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Wait (bounded) for the watcher fd to read ready.
    fn ready(w: &Watcher, ms: i32) -> bool {
        let mut p = libc::pollfd {
            fd: w.fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        unsafe { libc::poll(&mut p, 1, ms) > 0 }
    }

    #[test]
    fn atomic_publish_into_watched_dir_triggers() {
        let d = std::env::temp_dir().join(format!("bsctl-watch-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let w = Watcher::new(&[&d]).unwrap();
        assert!(!ready(&w, 0), "a quiet dir must not read ready");
        std::fs::write(d.join(".x.tmp"), "{}").unwrap();
        std::fs::rename(d.join(".x.tmp"), d.join("x")).unwrap();
        assert!(ready(&w, 2000), "an atomic publish must wake the watcher");
        assert!(w.drain().unwrap());
        assert!(!ready(&w, 0), "a drained watcher reads quiet again");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn removing_a_watched_dir_invalidates() {
        let d = std::env::temp_dir().join(format!("bsctl-watch-rm-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let w = Watcher::new(&[&d]).unwrap();
        std::fs::remove_dir_all(&d).unwrap();
        assert!(ready(&w, 2000));
        assert!(w.drain().is_err(), "a dead watch must force re-init");
    }
}
