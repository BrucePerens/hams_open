// SPDX-License-Identifier: LGPL-3.0-or-later
//! Floating-point inventory for the fixed-point Codec2 3200 path.
//!
//! The goal is a run-time encode and decode path with no `f32`/`f64` at all,
//! so it can run on a 32-bit core with no floating-point unit and be moved
//! to `no_std`. The files under `src/codec2_3200/` still hold the original
//! floating-point reference implementation and a few float adapters that
//! other codec modes use, in the same files as the integer code. This test
//! keeps that honest:
//!
//! * every top-level item (fn, struct, impl, const, type, ...) outside
//!   `#[cfg(test)]` that contains a float token (`f32`, `f64`, a float
//!   literal) must be listed in `ALLOWED` below with the reason;
//! * every `ALLOWED` entry must still exist and still contain a float, so
//!   the list can only shrink;
//! * `bits`, `encoder_fixed`, `fixed_fft`, `trig_fixed`, `voicing` and the
//!   generated `tables` must have no float at all (they have no `ALLOWED`
//!   entries, so any float there fails the first rule).
//!
//! `ALLOWED` is therefore also the precise list of what still stands between
//! this module and a `no_std` build. Categories:
//! * `reference`: the original `f32` implementation, used by
//!   `floating_reference` and the float `Decoder`/`Encoder`, plus the float
//!   constants they share;
//! * `adapter`: an `f32` boundary around an integer core, kept for the
//!   `codec2_1600` mode and the tests (the 3200 encoder and decoder call the
//!   integer core directly);
//! * `generator-input`: a float formula that only the test-only table
//!   generator (`tables_gen.rs`) and the float reference call.
//!
//! Comments and string literals are stripped before scanning.

use std::path::Path;

const FILES: &[&str] = &[
    "bits", "encoder_fixed", "envelope", "fixed_fft", "fixed_point", "interp", "lpc", "mod", "nlp",
    "quantise", "spectral_bridge", "synthesis", "tables", "trig_fixed", "voicing", "window",
];

/// `(file, "kind name", category)`.
const ALLOWED: &[(&str, &str, &str)] = &[
    ("envelope", "struct Model", "reference"),
    ("envelope", "impl Model", "reference"),
    ("envelope", "fn lpc_spectrum", "reference"),
    ("envelope", "fn compute_harmonic_amplitudes", "reference"),
    ("envelope", "fn apply_first_harmonic_correction", "reference"),
    ("envelope", "fn sample_filter_phase", "reference"),
    ("fixed_point", "fn log2_lut_generic_fixed", "adapter"),
    ("fixed_point", "fn f32_to_q_exact_round", "adapter"),
    ("fixed_point", "fn exp2_lut_generic_fixed", "adapter"),
    ("fixed_point", "fn log2_lut", "adapter"),
    ("fixed_point", "fn exp2_lut", "adapter"),
    ("interp", "fn interp_wo", "reference"),
    ("interp", "fn interp_energy", "reference"),
    ("interp", "fn interpolate_lsp", "reference"),
    ("lpc", "type Autocorr", "reference"),
    ("lpc", "type LpcCoeffs", "reference"),
    ("lpc", "fn levinson_durbin_fixed", "adapter"),
    ("lpc", "fn levinson_durbin_fixed_core", "adapter"),
    ("lpc", "fn dequantize_coef_q23", "adapter"),
    ("lpc", "fn f32_to_q", "adapter"),
    ("lpc", "fn f32_to_q64", "adapter"),
    ("lpc", "fn cheb_poly_eval_fixed_core", "adapter"),
    ("lpc", "const LSP_SEARCH_STEP", "adapter"),
    ("lpc", "fn find_next_root_from_q23", "adapter"),
    ("lpc", "fn acos_lut_fixed", "adapter"),
    ("lpc", "fn lpc_to_lsp_from_integer_ak", "adapter"),
    ("lpc", "fn poly_mul_fixed", "reference"),
    ("lpc", "fn build_half_poly", "reference"),
    ("lpc", "fn lsp_to_lpc", "reference"),
    ("lpc", "fn lpc_energy_fixed", "adapter"),
    ("nlp", "const NOTCH_A", "generator-input"),
    ("nlp", "fn design_lowpass", "generator-input"),
    ("nlp", "fn lowpass_coeffs", "reference"),
    ("nlp", "const CNLP", "generator-input"),
    ("nlp", "fn f0_to_wo", "adapter"),
    ("nlp", "fn nlp_fixed", "adapter"),
    ("quantise", "fn encode_wo", "reference"),
    ("quantise", "fn decode_wo", "reference"),
    ("quantise", "fn encode_energy", "adapter"),
    ("quantise", "fn decode_energy", "reference"),
    ("quantise", "fn quantize_linear", "reference"),
    ("quantise", "fn dequantize_linear", "reference"),
    ("quantise", "struct LspDim", "reference"),
    ("quantise", "const LSP_DIMS", "reference"),
    ("quantise", "fn lsp_dim_value_hz", "reference"),
    ("quantise", "fn decode_lsps_delta_scalar", "reference"),
    ("quantise", "fn encode_lsps_delta_scalar_fixed", "adapter"),
    ("spectral_bridge", "fn make_synthesis_window_sb", "generator-input"),
    ("spectral_bridge", "fn extrapolate_amplitudes", "reference"),
    ("spectral_bridge", "struct SpectralBridgeState", "reference"),
    ("spectral_bridge", "impl Default for SpectralBridgeState", "reference"),
    ("spectral_bridge", "impl SpectralBridgeState", "reference"),
    ("synthesis", "fn make_synthesis_window", "generator-input"),
    ("synthesis", "fn next_rand", "reference"),
    ("synthesis", "fn synthesize_phase", "reference"),
    ("synthesis", "fn postfilter_step", "reference"),
    ("synthesis", "fn postfilter", "reference"),
    ("synthesis", "fn ear_protection", "reference"),
    ("synthesis", "struct SynthesisState", "reference"),
    ("synthesis", "impl Default for SynthesisState", "reference"),
    ("synthesis", "impl SynthesisState", "reference"),
    ("window", "fn make_analysis_window", "generator-input"),
    ("mod", "const W0_MIN", "generator-input"),
    ("mod", "const W0_MAX", "generator-input"),
    ("mod", "const E_MIN_DB", "generator-input"),
    ("mod", "const E_MAX_DB", "generator-input"),
    ("mod", "const LPCPF_GAMMA", "reference"),
    ("mod", "const LPCPF_BETA", "reference"),
    ("mod", "const LPCPF_TWO_BETA", "reference"),
    ("mod", "const BG_THRESH", "generator-input"),
    ("mod", "const BG_BETA", "generator-input"),
    ("mod", "const BG_MARGIN", "generator-input"),
    ("mod", "fn bw_gamma", "reference"),
    ("mod", "fn fallback_lsp", "reference"),
    ("mod", "fn initial_lsps", "generator-input"),
    ("mod", "struct Decoder", "reference"),
    ("mod", "impl Default for Decoder", "reference"),
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
            if t.contains("cfg(test)") {
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
    let mut unexpected = Vec::new();
    for file in FILES {
        for item in read_items(file) {
            if item.gated || !item.has_float {
                continue;
            }
            if !ALLOWED.iter().any(|(f, k, _)| f == file && *k == item.key) {
                unexpected.push(format!("{file}.rs: {}", item.key));
            }
        }
    }
    assert!(
        unexpected.is_empty(),
        "floating point outside #[cfg(test)] and outside the documented inventory in \
         tests/codec2_3200_fixed_no_float_tokens.rs; the run-time fixed path must stay float-free. \
         Offending items:\n{}",
        unexpected.join("\n")
    );
}

#[test]
fn the_float_inventory_has_no_stale_entries() {
    let mut stale = Vec::new();
    for (file, key, _) in ALLOWED {
        let found = read_items(file)
            .into_iter()
            .any(|it| !it.gated && it.has_float && it.key == *key);
        if !found {
            stale.push(format!("{file}.rs: {key}"));
        }
    }
    assert!(
        stale.is_empty(),
        "these ALLOWED entries no longer exist or no longer contain a float; delete them:\n{}",
        stale.join("\n")
    );
}

#[test]
fn the_run_time_encode_and_decode_files_have_no_float_at_all() {
    // These files are the integer core. None of their items may appear in ALLOWED.
    for file in ["bits", "encoder_fixed", "fixed_fft", "trig_fixed", "voicing", "tables"] {
        assert!(
            !ALLOWED.iter().any(|(f, _, _)| *f == file),
            "{file}.rs must be entirely float-free outside #[cfg(test)]"
        );
    }
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
