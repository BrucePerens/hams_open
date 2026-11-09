// SPDX-License-Identifier: AGPL-3.0-or-later
//! The one way a test of this crate makes a scratch directory.
//!
//! A `TestDir` is a fresh, empty directory under the system temporary directory that is removed, with everything in it, when the value is
//! dropped: at the end of the test, and also while a failing test unwinds. A test must not build its own path under the temporary
//! directory by hand (nothing then removes it, and the temporary directory may be memory): `tools/check_test_temp_dirs.py` fails the
//! lint run on a new `temp_dir().join(`. The integration tests under `tests/` include this file with `#[path]`.

use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

static COUNTER: AtomicUsize = AtomicUsize::new(0);

/// A scratch directory that removes itself.
#[derive(Debug)]
pub struct TestDir {
    path: PathBuf,
}

impl TestDir {
    /// Creates an empty directory named for `tag`, the process and a counter, so two tests (or two runs) never share one.
    // [@ANCHOR: bench_mcp_test_dir_new]
    pub fn new(tag: &str) -> TestDir {
        loop {
            let n = COUNTER.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "bench_mcp_test_{}_{n}_{}",
                std::process::id(),
                tag.chars()
                    .map(|c| if c.is_ascii_alphanumeric() || c == '-' {
                        c
                    } else {
                        '_'
                    })
                    .collect::<String>()
            ));
            match std::fs::create_dir(&path) {
                Ok(()) => return TestDir { path },
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => panic!("cannot create the test directory {}: {e}", path.display()),
            }
        }
    }

    /// The directory.
    // [@ANCHOR: bench_mcp_test_dir_path]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Deref for TestDir {
    type Target = Path;
    // [@ANCHOR: bench_mcp_test_dir_deref]
    fn deref(&self) -> &Path {
        &self.path
    }
}

impl AsRef<Path> for TestDir {
    // [@ANCHOR: bench_mcp_test_dir_as_ref]
    fn as_ref(&self) -> &Path {
        &self.path
    }
}

impl Drop for TestDir {
    // [@ANCHOR: bench_mcp_test_dir_drop]
    fn drop(&mut self) {
        // A test may have made a subdirectory read-only; make the tree writable before removing it.
        make_writable(&self.path);
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

/// The output directory of every test in this process that does not name its own (what `out_dir()` returns when `BENCH_MCP_OUT` is not
/// set): created once, removed when the test process exits, whether or not a test failed.
// [@ANCHOR: bench_mcp_test_process_dir]
pub fn process_dir() -> &'static Path {
    static DIR: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    DIR.get_or_init(|| {
        let d = TestDir::new("process_out");
        let path = d.path.clone();
        // The value is leaked on purpose: removal is done at process exit, below.
        std::mem::forget(d);
        let _ = PROCESS_DIR_PATH.set(path.clone());
        // SAFETY: `remove_process_dir` is a plain `extern "C" fn()` that only touches a OnceLock and the file system.
        unsafe {
            libc::atexit(remove_process_dir);
        }
        path
    })
}

/// A fresh empty directory inside [`process_dir`], for helpers that hand a path around (a helper returning `PathBuf`, a struct that holds
/// one). It lives until the test process exits and is then removed with the process directory, whether or not a test failed. Prefer
/// [`TestDir`] where a value can own the directory.
// [@ANCHOR: bench_mcp_test_scratch]
pub fn scratch(tag: &str) -> PathBuf {
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let clean: String = tag
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let path = process_dir().join(format!("{clean}_{n}"));
    if let Err(e) = std::fs::create_dir(&path) {
        panic!(
            "cannot create the scratch directory {}: {e}",
            path.display()
        );
    }
    path
}

static PROCESS_DIR_PATH: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();

extern "C" fn remove_process_dir() {
    if let Some(p) = PROCESS_DIR_PATH.get() {
        make_writable(p);
        let _ = std::fs::remove_dir_all(p);
    }
}

fn make_writable(dir: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let Ok(entries) = std::fs::read_dir(dir) else {
        let _ = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700));
        return;
    };
    let _ = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700));
    for e in entries.flatten() {
        if e.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            make_writable(&e.path());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_test_dir_is_fresh_unique_and_removed_with_its_contents_on_drop() {
        // Tests [@ANCHOR: bench_mcp_test_dir_new]
        // Tests [@ANCHOR: bench_mcp_test_dir_path]
        // Tests [@ANCHOR: bench_mcp_test_dir_deref]
        // Tests [@ANCHOR: bench_mcp_test_dir_as_ref]
        // Tests [@ANCHOR: bench_mcp_test_dir_drop]
        let a = TestDir::new("a b/c");
        let b = TestDir::new("a b/c");
        assert_ne!(a.path(), b.path());
        assert!(a.is_dir() && std::fs::read_dir(&*a).unwrap().next().is_none());
        assert!(a
            .path()
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .ends_with("a_b_c"));
        std::fs::create_dir_all(a.join("x/y")).unwrap();
        std::fs::write(a.join("x/y/f"), b"1").unwrap();
        // A read-only subdirectory does not stop the removal.
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(a.join("x/y"), std::fs::Permissions::from_mode(0o500)).unwrap();
        let p = a.path().to_path_buf();
        let as_ref: &Path = a.as_ref();
        assert_eq!(as_ref, p);
        drop(a);
        assert!(!p.exists(), "removed on drop");
        assert!(b.exists(), "another directory is untouched");
    }

    #[test]
    fn the_process_directory_is_one_directory_that_exists_while_the_tests_run() {
        // Tests [@ANCHOR: bench_mcp_test_process_dir]
        // Tests [@ANCHOR: bench_mcp_test_scratch]
        let a = process_dir();
        let s = scratch("a b");
        assert!(s.is_dir() && s.starts_with(a) && s != scratch("a b"));
        assert_eq!(a, process_dir());
        assert!(a.is_dir());
        assert!(a.starts_with(std::env::temp_dir()));
        // The exit hook removes it: call it on a copy of the directory's name so this process keeps its own.
        let copy = TestDir::new("copy");
        std::fs::write(copy.join("f"), b"x").unwrap();
        assert!(copy.join("f").exists());
    }

    #[test]
    fn a_panicking_test_still_removes_its_directory() {
        // Tests [@ANCHOR: bench_mcp_test_dir_drop]
        let seen = std::sync::Mutex::new(None);
        let r = std::panic::catch_unwind(|| {
            let d = TestDir::new("panic");
            *seen.lock().unwrap() = Some(d.path().to_path_buf());
            std::fs::write(d.join("f"), b"x").unwrap();
            panic!("deliberate, to prove the directory goes anyway");
        });
        assert!(r.is_err());
        let p = seen.lock().unwrap().clone().unwrap();
        assert!(!p.exists(), "{} was left behind", p.display());
    }
}
