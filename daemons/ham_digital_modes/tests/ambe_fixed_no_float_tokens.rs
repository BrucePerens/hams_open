// SPDX-License-Identifier: LGPL-3.0-or-later
//! Independent enforcement of `ambe::fixed`'s own "no floating point whatsoever" rule, on top of
//! `#![deny(clippy::float_arithmetic)]` in `src/ambe/fixed/mod.rs`. The clippy lint only fires on an
//! actual arithmetic *expression* and only when clippy itself runs (`cargo clippy`, not a plain
//! `cargo build`/`cargo test`); this test instead greps every real line of every `.rs` file under
//! `src/ambe/fixed` for the tokens `f32`/`f64`/a bare float literal, so a future refactor that
//! routes a float through a type alias, a struct field of unclear type, or any other construct the
//! clippy lint doesn't recognize as "arithmetic" still gets caught, and it runs under plain
//! `cargo test` (no separate clippy invocation needed to catch a regression here).
//!
//! Doc comments and ordinary comments are stripped before scanning, since this file (and
//! `ambe::fixed`'s own module docs) legitimately *discuss* `f32`/`f64` in prose -- explaining what's
//! banned necessarily mentions the banned thing.

use std::path::{Path, PathBuf};

fn rust_files_under(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap_or_else(|e| panic!("read_dir {}: {e}", dir.display())) {
        let entry = entry.expect("dir entry");
        let path = entry.path();
        if path.is_dir() {
            rust_files_under(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
}

/// Strips `//`-style line comments (including `///`/`//!` doc comments) from source text -- a
/// simplified strip that doesn't handle `//` appearing inside a string literal, which is fine here
/// since none of `ambe::fixed`'s real code contains a string literal with `//` in it (checked: this
/// test would over-strip in that case, which only makes it more permissive, never less -- so it can
/// only produce a false negative, not a false positive, on code that doesn't exist yet).
fn strip_line_comments(line: &str) -> &str {
    match line.find("//") {
        Some(idx) => &line[..idx],
        None => line,
    }
}

/// A bare, unsuffixed float literal such as `1.0` or `0.5` -- deliberately narrow (a run of digits,
/// a `.`, a run of digits) so it doesn't false-positive on a tuple-index-like `foo.0` (no digits
/// before the identifier boundary) or a version string in a comment (already stripped by the time
/// this runs).
fn contains_bare_float_literal(code: &str) -> bool {
    let bytes = code.as_bytes();
    for (i, &b) in bytes.iter().enumerate() {
        if b == b'.' && i > 0 && i + 1 < bytes.len() {
            let before = bytes[i - 1];
            let after = bytes[i + 1];
            if before.is_ascii_digit() && after.is_ascii_digit() {
                return true;
            }
        }
    }
    false
}

#[test]
fn src_ambe_fixed_contains_no_floating_point_tokens_in_real_code() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/ambe/fixed");
    let mut files = Vec::new();
    rust_files_under(&root, &mut files);
    assert!(!files.is_empty(), "expected to find .rs files under {}", root.display());

    let mut violations = Vec::new();
    for path in &files {
        let content = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
        for (line_no, line) in content.lines().enumerate() {
            let code = strip_line_comments(line);
            if code.contains("f32") || code.contains("f64") || contains_bare_float_literal(code) {
                violations.push(format!("{}:{}: {}", path.display(), line_no + 1, line.trim()));
            }
        }
    }
    assert!(
        violations.is_empty(),
        "src/ambe/fixed must contain zero floating-point tokens in real code, found:\n{}",
        violations.join("\n")
    );
}
