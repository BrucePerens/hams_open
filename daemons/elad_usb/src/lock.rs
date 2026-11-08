// SPDX-License-Identifier: AGPL-3.0-or-later
//! Single-owner lock for one receiver, so the relay and the bench MCP never stream from the same unit at once.
//!
//! One lock file per USB bus and address, `elad-<bus>-<address>.lock`, in a directory both programs can write
//! (`ELAD_LOCK_DIR`, else `/run/lock`, else the system temp directory). The lock is an advisory `flock` on that file, held for
//! as long as the [`DeviceLock`] value lives; the kernel drops it when the process dies, so a crash never leaves a stale lock.
//! The holder writes its pid and name into the file so the refusal can say who has the receiver. libusb's interface claim is a
//! second, kernel-level guard against a program that does not use this lock.

use std::fs::{File, OpenOptions, TryLockError};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

/// Why a lock was not taken.
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
#[derive(Debug)]
pub struct DeviceLock {
    file: File,
    path: PathBuf,
}

/// The directory lock files go in: `ELAD_LOCK_DIR` if set, else `/run/lock` if it exists, else the temp directory.
// [@ANCHOR: elad_lock_dir]
pub fn lock_dir() -> PathBuf {
    if let Some(d) = std::env::var_os("ELAD_LOCK_DIR") {
        return PathBuf::from(d);
    }
    let run_lock = Path::new("/run/lock");
    if run_lock.is_dir() {
        return run_lock.to_path_buf();
    }
    std::env::temp_dir()
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
        DeviceLock::acquire_in(&lock_dir(), bus, address, owner)
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

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("elad-lock-test-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap_or(());
        d
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
    fn the_directory_comes_from_the_environment_then_run_lock_then_temp() {
        // Tests [@ANCHOR: elad_lock_dir]
        // Tests [@ANCHOR: elad_lock_acquire]
        let d = dir("env");
        std::env::set_var("ELAD_LOCK_DIR", &d);
        assert_eq!(lock_dir(), d);
        let l = DeviceLock::acquire(3, 4, "t").unwrap();
        assert_eq!(l.path(), d.join("elad-3-4.lock"));
        drop(l);
        std::env::remove_var("ELAD_LOCK_DIR");
        let fallback = lock_dir();
        assert!(fallback == Path::new("/run/lock") || fallback == std::env::temp_dir());
    }
}
