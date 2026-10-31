// SPDX-License-Identifier: AGPL-3.0-or-later
//! Repository conventions enforced as tests (ADR 0090 anchors, licensing): every Rust file
//! has an SPDX header, every manifest inherits the AGPL licence, and anchors are in parity
//! between source, tests and the README.
// Integration-test helpers may use expect: a failed read must fail the test loudly.
#![allow(clippy::expect_used)]

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("workspace root")
}

fn files_with_suffix(dir: &Path, suffix: &str, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).expect("read_dir") {
        let path = entry.expect("entry").path();
        if path.is_dir() {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if name != "target" && !name.starts_with('.') {
                files_with_suffix(&path, suffix, out);
            }
        } else if path.to_string_lossy().ends_with(suffix) {
            out.push(path);
        }
    }
}

fn rust_files() -> Vec<PathBuf> {
    let mut out = Vec::new();
    files_with_suffix(&workspace_root().join("crates"), ".rs", &mut out);
    out
}

/// Anchor ids found after `marker` on lines of `files`. The marker is assembled from
/// parts so this file does not declare anchors of its own.
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

fn declared_marker() -> String {
    format!("// [@{}: ", "ANCHOR")
}

fn tests_marker() -> String {
    format!("// Tests [@{}: ", "ANCHOR")
}

#[test]
fn every_rust_file_has_an_spdx_header() {
    for file in rust_files() {
        let text = fs::read_to_string(&file).expect("read");
        let first = text.lines().next().unwrap_or("");
        assert_eq!(
            first,
            "// SPDX-License-Identifier: AGPL-3.0-or-later",
            "{}",
            file.display()
        );
    }
}

#[test]
fn every_manifest_inherits_the_agpl_licence() {
    let root = workspace_root();
    let top = fs::read_to_string(root.join("Cargo.toml")).expect("read");
    assert!(top.contains("license = \"AGPL-3.0-or-later\""));
    let mut manifests = Vec::new();
    files_with_suffix(&root.join("crates"), "Cargo.toml", &mut manifests);
    assert!(manifests.len() >= 3);
    for manifest in manifests {
        let text = fs::read_to_string(&manifest).expect("read");
        assert!(
            text.contains("license.workspace = true"),
            "{}",
            manifest.display()
        );
    }
}

#[test]
fn anchors_are_in_parity_between_source_and_tests() {
    let mut sources = Vec::new();
    files_with_suffix(&workspace_root().join("crates"), ".rs", &mut sources);
    let declared = anchors(&sources, &declared_marker());
    let tested = anchors(&sources, &tests_marker());
    assert!(!declared.is_empty());
    let untested: Vec<_> = declared.difference(&tested).collect();
    let dangling: Vec<_> = tested.difference(&declared).collect();
    assert!(untested.is_empty(), "anchors without a test: {untested:?}");
    assert!(
        dangling.is_empty(),
        "tests citing missing anchors: {dangling:?}"
    );
}

#[test]
fn every_anchor_is_documented_in_the_readme() {
    let mut readme = fs::read_to_string(workspace_root().join("README.md")).expect("readme");
    // A crate may keep its own anchor list in its own README (radioctl-station does).
    if let Ok(entries) = fs::read_dir(workspace_root().join("crates")) {
        for entry in entries.flatten() {
            if let Ok(text) = fs::read_to_string(entry.path().join("README.md")) {
                readme.push_str(&text);
            }
        }
    }
    let declared = anchors(&rust_files(), &declared_marker());
    for id in declared {
        let cite = format!("[@{}: radioctl:{id}]", "ANCHOR");
        assert!(readme.contains(&cite), "README lacks {cite}");
    }
}
