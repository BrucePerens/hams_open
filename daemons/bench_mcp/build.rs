// SPDX-License-Identifier: AGPL-3.0-or-later
//! Embeds every file under `manual/` in the binary: writes `$OUT_DIR/manual_files.rs`, a table of
//! (path relative to `manual/`, file text) built with `include_str!`, so the installed server needs
//! no files and a file on disk cannot be missing from it.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

fn walk(dir: &Path, root: &Path, out: &mut Vec<(String, PathBuf)>) {
    let mut entries: Vec<PathBuf> = std::fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("manual: {}: {e}", dir.display()))
        .map(|e| e.expect("manual: directory entry").path())
        .collect();
    entries.sort();
    for p in entries {
        if p.is_dir() {
            walk(&p, root, out);
        } else {
            let rel = p
                .strip_prefix(root)
                .expect("manual: path under the manual directory")
                .to_string_lossy()
                .replace('\\', "/");
            out.push((rel, p));
        }
    }
}

fn main() {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let root = manifest.join("manual");
    println!("cargo:rerun-if-changed=manual");
    println!("cargo:rerun-if-changed=build.rs");
    let mut files = Vec::new();
    walk(&root, &root, &mut files);
    let mut src = String::from("pub const FILES: &[(&str, &str)] = &[\n");
    for (rel, abs) in &files {
        writeln!(
            src,
            "    ({rel:?}, include_str!({:?})),",
            abs.to_string_lossy()
        )
        .unwrap();
    }
    src.push_str("];\n");
    let out = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR")).join("manual_files.rs");
    std::fs::write(&out, src).unwrap_or_else(|e| panic!("manual: {}: {e}", out.display()));
}
