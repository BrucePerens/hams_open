// SPDX-License-Identifier: LGPL-3.0-or-later
//! Floating-point inventory for the fixed-point Codec2 3200 path.
//!
//! The run-time encode and decode path has no `f32`/`f64` at all, so it runs on a 32-bit core with
//! no floating-point unit and builds `no_std` (`--no-default-features`). The files under
//! `src/codec2_3200/` still hold the original floating-point reference implementation and some
//! float adapters (used by `codec2_1600`, the reference decoder and the tests) in the same files
//! as the integer code. Every such item is gated with `#[cfg(feature = "std")]`, which removes it
//! from the `no_std` build. This test keeps that honest: every top-level item (fn, struct, impl,
//! const, type, ...) that contains a float token (`f32`, `f64`, a float literal) must be inside
//! `#[cfg(test)]` or `#[cfg(feature = "std")]`. There is no exception list any more (it used to
//! hold 77 entries and could only shrink). `tools/check_codec2_no_std.sh` additionally builds the
//! bare-metal targets, which catches float method calls (`.sqrt()`, `.sin()`) that carry no token.
//!
//! Comments and string literals are stripped before scanning.

use std::path::Path;

const FILES: &[&str] = &[
    "bits", "encoder_fixed", "envelope", "fixed_fft", "fixed_point", "interp", "lpc", "mod", "nlp",
    "quantise", "spectral_bridge", "synthesis", "tables", "trig_fixed", "voicing", "window",
];

/// Removes string literal contents and `//` comments from one line.
fn code_only(line: &str) -> String {
    let mut out = String::new();
    let b: Vec<char> = line.chars().collect();
    let mut i = 0;
    let mut in_str = false;
    while i < b.len() {
        let c = b[i];
        if in_str {
            if c == '\\' {
                i += 2;
                continue;
            }
            if c == '"' {
                in_str = false;
                out.push('"');
            }
        } else if c == '"' {
            in_str = true;
            out.push('"');
        } else if c == '/' && i + 1 < b.len() && b[i + 1] == '/' {
            break;
        } else {
            out.push(c);
        }
        i += 1;
    }
    out
}

/// True if `code` has `f32`, `f64`, or a floating-point literal.
fn has_float_token(code: &str) -> bool {
    let chars: Vec<char> = code.chars().collect();
    let is_word = |c: char| c.is_ascii_alphanumeric() || c == '_';
    let mut i = 0;
    while i < chars.len() {
        if !is_word(chars[i]) {
            i += 1;
            continue;
        }
        // One maximal word (identifier or number), possibly with `1.5` style decimal point.
        let start = i;
        while i < chars.len()
            && (is_word(chars[i])
                || (chars[i] == '.'
                    && chars[start].is_ascii_digit()
                    && i + 1 < chars.len()
                    && chars[i + 1].is_ascii_digit()))
        {
            i += 1;
        }
        let word: String = chars[start..i].iter().collect();
        if word == "f32" || word == "f64" {
            return true;
        }
        if chars[start].is_ascii_digit() {
            let lower = word.to_ascii_lowercase().replace('_', "");
            if lower.starts_with("0x") || lower.starts_with("0b") || lower.starts_with("0o") {
                continue;
            }
            if lower.ends_with("f32") || lower.ends_with("f64") {
                return true;
            }
            const INT_SUFFIXES: [&str; 12] = [
                "usize", "isize", "u128", "i128", "u64", "i64", "u32", "i32", "u16", "i16", "u8", "i8",
            ];
            let base = INT_SUFFIXES
                .iter()
                .find_map(|s| lower.strip_suffix(s))
                .unwrap_or(&lower);
            if base.contains('.') {
                return true;
            }
            // Exponent form: `1e6`, or `1e` followed by a sign (`1e-6` splits at the sign).
            if let Some((mantissa, exponent)) = base.split_once('e') {
                let digits = |t: &str| !t.is_empty() && t.chars().all(|c| c.is_ascii_digit());
                let sign_follows = matches!(chars.get(i), Some('+') | Some('-'));
                if digits(mantissa) && (digits(exponent) || (exponent.is_empty() && sign_follows)) {
                    return true;
                }
            }
        }
    }
    false
}

struct Item {
    key: String,
    gated: bool,
    has_float: bool,
}

/// Splits a source file into its top-level items. An item runs from its
/// first non-attribute line until brackets balance and a line ends in `;`
/// or `}`.
fn parse_items(src: &str) -> Vec<Item> {
    let lines: Vec<&str> = src.lines().collect();
    let mut items = Vec::new();
    let mut attrs_gate = false;
    let mut i = 0;
    while i < lines.len() {
        let code = code_only(lines[i]);
        let t = code.trim();
        if t.is_empty() {
            i += 1;
            continue;
        }
        if t.starts_with("#![") {
            i += 1;
            continue;
        }
        if t.starts_with("#[") {
            // (Raw line: `code_only` blanks string literals such as "std".)
            if t.contains("cfg(test)") || lines[i].contains("cfg(feature = \"std\")") {
                attrs_gate = true;
            }
            i += 1;
            continue;
        }
        // Item header: `pub(crate) const fn foo`, `impl<T> X for Y {`, `struct S`, ...
        let mut words = t.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).filter(|w| !w.is_empty());
        let mut kind = String::new();
        let mut name = String::new();
        let mut all_words = Vec::new();
        for w in words.by_ref() {
            all_words.push(w.to_string());
        }
        let mut idx = 0;
        while idx < all_words.len() {
            let w = all_words[idx].as_str();
            if matches!(w, "pub" | "crate" | "super" | "in" | "unsafe" | "async" | "extern" | "self")
                || (w == "const" && all_words.get(idx + 1).map(String::as_str) == Some("fn"))
            {
                idx += 1;
                continue;
            }
            kind = w.to_string();
            break;
        }
        if kind == "impl" {
            let head = t.split('{').next().unwrap_or("").trim();
            let head = head.trim_start_matches("pub ").trim_start_matches("impl").trim();
            let head = if let Some(rest) = head.strip_prefix('<') {
                rest.split_once('>').map_or(rest, |x| x.1).trim()
            } else {
                head
            };
            name = head.trim_end_matches("where").trim().to_string();
        } else if let Some(n) = all_words.get(idx + 1) {
            name = n.clone();
        }
        let mut depth: i32 = 0;
        let mut has_float = false;
        let mut j = i;
        while j < lines.len() {
            let c = code_only(lines[j]);
            if has_float_token(&c) {
                has_float = true;
            }
            for ch in c.chars() {
                match ch {
                    '{' | '[' | '(' => depth += 1,
                    '}' | ']' | ')' => depth -= 1,
                    _ => {}
                }
            }
            let end = c.trim_end();
            if depth <= 0 && (end.ends_with(';') || end.ends_with('}')) {
                break;
            }
            j += 1;
        }
        items.push(Item { key: format!("{kind} {name}"), gated: attrs_gate, has_float });
        attrs_gate = false;
        i = j + 1;
    }
    items
}

fn read_items(file: &str) -> Vec<Item> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/codec2_3200").join(format!("{file}.rs"));
    let src = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    parse_items(&src)
}

#[test]
fn every_float_in_the_codec2_3200_fixed_files_is_in_the_inventory() {
    let mut unexpected: Vec<String> = Vec::new();
    for file in FILES {
        for item in read_items(file) {
            if item.gated || !item.has_float {
                continue;
            }
            unexpected.push(format!("{file}.rs: {}", item.key));
        }
    }
    assert!(
        unexpected.is_empty(),
        "floating point outside #[cfg(test)] and #[cfg(feature = \"std\")]; the run-time fixed \
         path must stay float-free and no_std. Add the std gate (or make the item integer). \
         Offending items:\n{}",
        unexpected.join("\n")
    );
}

#[test]
fn the_scanner_itself_recognizes_floats_and_ignores_integers() {
    assert!(has_float_token("let x: f32 = 1;"));
    assert!(has_float_token("let y = 0.5;"));
    assert!(has_float_token("let z = 1e-6;"));
    assert!(has_float_token("let w = 3f64;"));
    assert!(!has_float_token("let a = 0x1e5;"));
    assert!(!has_float_token("let b = t.0 + 12;"));
    assert!(!has_float_token("let c = 1u64 << 23;"));
    assert!(!has_float_token(&code_only("let s = \"1.5 f32\"; // 2.5 f64")));
}
