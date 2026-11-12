// SPDX-License-Identifier: AGPL-3.0-or-later
//! Single-owner lock for one receiver, so the relay and the bench MCP never stream from the same unit at once.
//!
//! One lock file per USB bus and address, `elad-<bus>-<address>.lock`, in a directory both programs can write
//! (`ELAD_LOCK_DIR`, else the user's `XDG_RUNTIME_DIR`, else a private `elad-<uid>` directory in the temp directory; never a directory
//! other users can write to, so nobody else can pre-create or hold the file). The lock is an advisory `flock` on that file, held for
//! as long as the [`DeviceLock`] value lives; the kernel drops it when the process dies, so a crash never leaves a stale lock.
//! The holder writes its pid and name into the file so the refusal can say who has the receiver. libusb's interface claim is a
//! second, kernel-level guard against a program that does not use this lock.

use crate::transport::Transport;
use crate::Error;
use std::fs::{File, OpenOptions, TryLockError};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Why a lock was not taken.
// [@ANCHOR: lock_lock_error_enum]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LockError {
    /// Another process holds the receiver.
    Held {
        /// What the holder wrote: its pid and name, or "unknown" when it wrote nothing readable.
        holder: String,
        /// The lock file.
        path: PathBuf,
    },
    /// The lock file could not be created or locked.
    Io(String),
}

impl std::fmt::Display for LockError {
    // [@ANCHOR: elad_lock_error_display]
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LockError::Held { holder, path } => write!(
                f,
                "the ELAD receiver is in use by {holder} (lock {}); stop that program (for example the relay's ELAD source) or wait, then try again",
                path.display()
            ),
            LockError::Io(m) => write!(f, "could not take the receiver lock: {m}"),
        }
    }
}

impl std::error::Error for LockError {}

/// A held lock on one receiver. Dropping it releases the receiver.
// [@ANCHOR: lock_device_lock_struct]
#[derive(Debug)]
pub struct DeviceLock {
    file: File,
    path: PathBuf,
}

/// The directory lock files go in: `ELAD_LOCK_DIR` if set, else `XDG_RUNTIME_DIR` (private to the user), else `elad-<uid>` in the temp
/// directory (made mode 0700 and checked to be ours by [`DeviceLock::acquire`]). Never `/run/lock`: any local user can write there.
// [@ANCHOR: elad_lock_dir]
pub fn lock_dir() -> PathBuf {
    if let Some(d) = std::env::var_os("ELAD_LOCK_DIR") {
        return PathBuf::from(d);
    }
    if let Some(d) = std::env::var_os("XDG_RUNTIME_DIR") {
        let d = PathBuf::from(d);
        if d.is_dir() {
            return d;
        }
    }
    std::env::temp_dir().join(format!("elad-{}", current_uid().unwrap_or(0)))
}

fn current_uid() -> Option<u32> {
    use std::os::unix::fs::MetadataExt;
    std::fs::metadata("/proc/self").ok().map(|m| m.uid())
}

/// Makes sure a private lock directory is ours: creates it mode 0700 when missing, refuses one owned by someone else or open to
/// group or others. Used only for the default `elad-<uid>` directory.
// [@ANCHOR: elad_lock_ensure_private_dir]
fn ensure_private_dir(dir: &Path) -> Result<(), LockError> {
    use std::os::unix::fs::{DirBuilderExt, MetadataExt};
    let io = |e: std::io::Error| LockError::Io(format!("{}: {e}", dir.display()));
    if let Err(e) = std::fs::DirBuilder::new().mode(0o700).create(dir) {
        if e.kind() != std::io::ErrorKind::AlreadyExists {
            return Err(io(e));
        }
    }
    let m = std::fs::symlink_metadata(dir).map_err(io)?;
    if !m.is_dir() || Some(m.uid()) != current_uid() || m.mode() & 0o077 != 0 {
        return Err(LockError::Io(format!(
            "{} is not a private directory owned by this user; remove it or set ELAD_LOCK_DIR",
            dir.display()
        )));
    }
    Ok(())
}

/// The lock file name for a bus and address.
// [@ANCHOR: elad_lock_file_name]
pub fn lock_file_name(bus: u8, address: u8) -> String {
    format!("elad-{bus}-{address}.lock")
}

impl DeviceLock {
    /// Takes the lock for the receiver at `bus`/`address` in [`lock_dir`], recording `owner` (a short program name).
    // [@ANCHOR: elad_lock_acquire]
    pub fn acquire(bus: u8, address: u8, owner: &str) -> Result<DeviceLock, LockError> {
        let dir = lock_dir();
        if std::env::var_os("ELAD_LOCK_DIR").is_none()
            && std::env::var_os("XDG_RUNTIME_DIR").is_none_or(|d| !Path::new(&d).is_dir())
        {
            ensure_private_dir(&dir)?;
        }
        DeviceLock::acquire_in(&dir, bus, address, owner)
    }

    /// As [`DeviceLock::acquire`] with the directory given.
    // [@ANCHOR: elad_lock_acquire_in]
    pub fn acquire_in(
        dir: &Path,
        bus: u8,
        address: u8,
        owner: &str,
    ) -> Result<DeviceLock, LockError> {
        let path = dir.join(lock_file_name(bus, address));
        // Not truncated on open: the holder's note must survive until we know we own the lock.
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)
            .map_err(|e| LockError::Io(format!("{}: {e}", path.display())))?;
        match file.try_lock() {
            Ok(()) => {}
            Err(TryLockError::WouldBlock) => {
                return Err(LockError::Held {
                    holder: read_holder(&mut file),
                    path,
                })
            }
            Err(TryLockError::Error(e)) => {
                return Err(LockError::Io(format!("{}: {e}", path.display())))
            }
        }
        let note = format!("pid {} ({owner})", std::process::id());
        file.set_len(0)
            .and_then(|()| file.seek(SeekFrom::Start(0)).map(|_| ()))
            .and_then(|()| file.write_all(note.as_bytes()))
            .map_err(|e| LockError::Io(format!("{}: {e}", path.display())))?;
        Ok(DeviceLock { file, path })
    }

    /// The lock file.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for DeviceLock {
    fn drop(&mut self) {
        let _ = self.file.unlock();
    }
}

// [@ANCHOR: elad_lock_read_holder]
fn read_holder(file: &mut File) -> String {
    let mut s = String::new();
    let ok = file.seek(SeekFrom::Start(0)).is_ok() && file.read_to_string(&mut s).is_ok();
    let s = s.trim();
    if ok && !s.is_empty() && s.len() < 200 {
        s.to_string()
    } else {
        "another program".to_string()
    }
}

/// A transport that owns the receiver's lock: the lock lives exactly as long as the transport, so a `Radio` built on it keeps the
/// receiver to itself until it is dropped.
// [@ANCHOR: lock_locked_struct]
pub struct Locked<T: Transport> {
    inner: T,
    _lock: DeviceLock,
}

impl<T: Transport> Locked<T> {
    /// Ties `lock` to `inner`.
    // [@ANCHOR: elad_locked_new]
    pub fn new(inner: T, lock: DeviceLock) -> Locked<T> {
        Locked { inner, _lock: lock }
    }
}

impl<T: Transport> Transport for Locked<T> {
    // [@ANCHOR: elad_locked_control_in]
    fn control_in(
        &mut self,
        request: u8,
        value: u16,
        index: u16,
        buf: &mut [u8],
    ) -> Result<usize, Error> {
        self.inner.control_in(request, value, index, buf)
    }
    // [@ANCHOR: elad_locked_control_out]
    fn control_out(
        &mut self,
        request: u8,
        value: u16,
        index: u16,
        data: &[u8],
    ) -> Result<usize, Error> {
        self.inner.control_out(request, value, index, data)
    }
    // [@ANCHOR: elad_locked_bulk_in]
    fn bulk_in(&mut self, buf: &mut [u8], timeout: Duration) -> Result<usize, Error> {
        self.inner.bulk_in(buf, timeout)
    }
}

#[cfg(test)]
mod tests {
    // Tests [@ANCHOR: lock_lock_error_enum]
    // Tests [@ANCHOR: lock_device_lock_struct]
    // Tests [@ANCHOR: lock_locked_struct]
    use super::*;

    /// A scratch directory that removes itself when dropped, also while a failing test unwinds.
    struct Scratch(PathBuf);

    impl std::ops::Deref for Scratch {
        type Target = Path;
        fn deref(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn dir(tag: &str) -> Scratch {
        static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        // temp-dir-helper: the Drop of Scratch removes it
        let d =
            std::env::temp_dir().join(format!("elad-lock-test-{tag}-{}-{n}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        Scratch(d)
    }

    #[test]
    fn a_second_acquire_is_refused_naming_the_holder_and_release_frees_it() {
        // Tests [@ANCHOR: elad_lock_acquire_in]
        // Tests [@ANCHOR: elad_lock_read_holder]
        // Tests [@ANCHOR: elad_lock_error_display]
        let d = dir("held");
        let first = DeviceLock::acquire_in(&d, 1, 8, "relay").unwrap();
        let err = DeviceLock::acquire_in(&d, 1, 8, "bench_mcp").unwrap_err();
        let LockError::Held { holder, path } = &err else {
            panic!("expected Held, got {err:?}");
        };
        assert!(holder.contains("relay") && holder.contains(&std::process::id().to_string()));
        assert_eq!(path, first.path());
        let text = err.to_string();
        assert!(
            text.contains("in use by pid") && text.contains("relay"),
            "{text}"
        );
        // The refused attempt must not have overwritten the holder's note.
        assert!(std::fs::read_to_string(first.path())
            .unwrap()
            .contains("relay"));
        drop(first);
        assert!(DeviceLock::acquire_in(&d, 1, 8, "bench_mcp").is_ok());
    }

    #[test]
    fn different_receivers_lock_independently() {
        // Tests [@ANCHOR: elad_lock_file_name]
        let d = dir("two");
        let _a = DeviceLock::acquire_in(&d, 1, 8, "a").unwrap();
        let _b = DeviceLock::acquire_in(&d, 1, 9, "b").unwrap();
        assert_eq!(lock_file_name(1, 8), "elad-1-8.lock");
        assert_ne!(lock_file_name(1, 8), lock_file_name(2, 8));
    }

    #[test]
    fn an_unwritable_directory_is_an_io_error_not_a_held_lock() {
        // Tests [@ANCHOR: elad_lock_acquire_in]
        let err =
            DeviceLock::acquire_in(Path::new("/nonexistent-elad-dir"), 1, 8, "x").unwrap_err();
        assert!(matches!(err, LockError::Io(_)));
        assert!(err.to_string().contains("could not take"));
    }

    #[test]
    fn the_directory_comes_from_the_environment_then_the_runtime_dir_then_a_private_temp_dir() {
        // Tests [@ANCHOR: elad_lock_dir]
        // Tests [@ANCHOR: elad_lock_acquire]
        let d = dir("env");
        std::env::set_var("ELAD_LOCK_DIR", d.as_os_str());
        assert_eq!(lock_dir(), d.to_path_buf());
        let l = DeviceLock::acquire(3, 4, "t").unwrap();
        assert_eq!(l.path(), d.join("elad-3-4.lock"));
        drop(l);
        std::env::remove_var("ELAD_LOCK_DIR");
        let fallback = lock_dir();
        assert_ne!(fallback, Path::new("/run/lock"));
        assert_ne!(fallback, std::env::temp_dir());
    }

    #[test]
    fn the_default_directory_is_private_and_a_foreign_or_open_one_is_refused() {
        // Tests [@ANCHOR: elad_lock_ensure_private_dir]
        // Tests [@ANCHOR: elad_lock_dir]
        use std::os::unix::fs::PermissionsExt;
        let base = dir("priv");
        let mine = base.join("elad-test");
        ensure_private_dir(&mine).unwrap();
        ensure_private_dir(&mine).unwrap();
        assert_eq!(
            std::fs::metadata(&mine).unwrap().permissions().mode() & 0o777,
            0o700
        );
        std::fs::set_permissions(&mine, std::fs::Permissions::from_mode(0o755)).unwrap();
        let err = ensure_private_dir(&mine).unwrap_err();
        assert!(err.to_string().contains("not a private directory"), "{err}");
        let link = base.join("link");
        std::os::unix::fs::symlink(&mine, &link).unwrap();
        assert!(ensure_private_dir(&link).is_err());
    }

    #[test]
    fn a_locked_transport_keeps_the_lock_until_it_is_dropped_and_passes_requests_through() {
        // Tests [@ANCHOR: elad_locked_new]
        // Tests [@ANCHOR: elad_locked_control_in]
        // Tests [@ANCHOR: elad_locked_control_out]
        // Tests [@ANCHOR: elad_locked_bulk_in]
        use crate::transport::{FakeTransport, Op};
        let d = dir("locked");
        let fake = FakeTransport::new();
        fake.reply(0xFF, 0, vec![3, 0]);
        fake.queue_bulk(Ok(vec![1, 2, 3]));
        let mut t = Locked::new(
            fake.clone(),
            DeviceLock::acquire_in(&d, 5, 6, "bench").unwrap(),
        );
        assert!(matches!(
            DeviceLock::acquire_in(&d, 5, 6, "relay"),
            Err(LockError::Held { .. })
        ));
        let mut b = [0u8; 2];
        assert_eq!(t.control_in(0xFF, 0, 0, &mut b).unwrap(), 2);
        assert_eq!(b, [3, 0]);
        assert_eq!(t.control_out(0xF2, 1, 2, &[9]).unwrap(), 1);
        let mut buf = [0u8; 8];
        assert_eq!(t.bulk_in(&mut buf, Duration::from_millis(1)).unwrap(), 3);
        assert_eq!(fake.ops().len(), 2);
        assert!(matches!(fake.ops()[1], Op::Out { request: 0xF2, .. }));
        drop(t);
        assert!(DeviceLock::acquire_in(&d, 5, 6, "relay").is_ok());
    }
}
