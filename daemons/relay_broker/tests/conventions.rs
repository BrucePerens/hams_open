// SPDX-License-Identifier: AGPL-3.0-or-later
//! Repository conventions as tests (as in radioctl): every Rust file has an SPDX header, and
//! ADR 0090 anchors are in parity between source, tests and the README.
#![allow(clippy::expect_used)]

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

fn crate_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).expect("read_dir") {
        let path = entry.expect("entry").path();
        if path.is_dir() {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if name != "target" && !name.starts_with('.') {
                rust_files(&path, out);
            }
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

fn anchors(files: &[PathBuf], marker: &str) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    for file in files {
        for line in fs::read_to_string(file).expect("read").lines() {
            if let Some(rest) = line.trim_start().strip_prefix(marker) {
                if let Some(id) = rest.trim().strip_suffix(']') {
                    found.insert(id.trim().to_string());
                }
            }
        }
    }
    found
}

#[test]
fn every_rust_file_has_the_spdx_header() {
    let mut files = Vec::new();
    rust_files(&crate_root(), &mut files);
    assert!(files.len() > 10);
    for f in files {
        let first = fs::read_to_string(&f)
            .expect("read")
            .lines()
            .next()
            .unwrap_or("")
            .to_string();
        assert_eq!(
            first,
            "// SPDX-License-Identifier: AGPL-3.0-or-later",
            "{}",
            f.display()
        );
    }
}

#[test]
fn anchors_agree_between_source_tests_and_readme() {
    let mut files = Vec::new();
    rust_files(&crate_root(), &mut files);
    let declared = anchors(&files, &format!("// [@{}: ", "ANCHOR"));
    let tested = anchors(&files, &format!("// Tests [@{}: ", "ANCHOR"));
    let readme = fs::read_to_string(crate_root().join("README.md")).expect("README");
    let cited: BTreeSet<String> = readme
        .lines()
        .filter_map(|l| {
            l.trim_start()
                .strip_prefix(&format!("- [@{}: relay_broker:", "ANCHOR"))
        })
        .filter_map(|rest| rest.split(']').next())
        .map(str::to_string)
        .collect();
    assert!(!declared.is_empty());
    let untested: Vec<_> = declared.difference(&tested).collect();
    assert!(untested.is_empty(), "declared without a test: {untested:?}");
    let orphan: Vec<_> = tested.difference(&declared).collect();
    assert!(orphan.is_empty(), "tests of nothing: {orphan:?}");
    let uncited: Vec<_> = declared.difference(&cited).collect();
    assert!(uncited.is_empty(), "not cited in README: {uncited:?}");
    let stale: Vec<_> = cited.difference(&declared).collect();
    assert!(stale.is_empty(), "README cites a missing anchor: {stale:?}");
}
