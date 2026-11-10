// SPDX-License-Identifier: AGPL-3.0-or-later
//! The one way a test of this crate makes a scratch directory: a fresh, empty directory that is removed with everything in it when the value
//! is dropped, also while a failing test unwinds.

use std::ops::Deref;
use std::path::{Path, PathBuf};

pub struct TestDir(PathBuf);

impl TestDir {
    pub fn new(tag: &str) -> TestDir {
        // temp-dir-helper: `TestDir` removes the tree in its Drop, also while a failing test unwinds
        let dir = std::env::temp_dir().join(format!("vnwa_open_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        TestDir(dir)
    }
}

impl Deref for TestDir {
    type Target = Path;
    fn deref(&self) -> &Path {
        &self.0
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

impl AsRef<Path> for TestDir {
    fn as_ref(&self) -> &Path {
        &self.0
    }
}

impl AsRef<std::ffi::OsStr> for TestDir {
    fn as_ref(&self) -> &std::ffi::OsStr {
        self.0.as_os_str()
    }
}
