// SPDX-License-Identifier: AGPL-3.0-or-later
//! The one way a test of this crate makes a scratch directory: a fresh, empty directory under the temporary directory that is removed,
//! with everything in it, when the value is dropped (also while a failing test unwinds). The test includes this file with `#[path]`;
//! `tools/check_test_temp_dirs.py` fails a new hand-built scratch path in test code.
#![allow(dead_code)]

use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

static COUNTER: AtomicUsize = AtomicUsize::new(0);

pub struct TestDir(PathBuf);

impl TestDir {
    pub fn new(tag: &str) -> TestDir {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        // temp-dir-helper: `TestDir` removes the tree in its Drop, also while a failing test unwinds
        let dir = std::env::temp_dir().join(format!("hdm_{tag}_{}_{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create the test directory");
        TestDir(dir)
    }
}

impl Deref for TestDir {
    type Target = Path;
    fn deref(&self) -> &Path {
        &self.0
    }
}

impl AsRef<Path> for TestDir {
    fn as_ref(&self) -> &Path {
        &self.0
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
