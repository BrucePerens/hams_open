// SPDX-License-Identifier: LGPL-3.0-or-later
//! Test-only generator for `tables.rs` (the committed, float-free constant
//! tables and scalars the fixed-point codec path uses at run time).
//!
//! Before this existed every table below was built lazily, on first use,
//! with `f32`/`f64` maths (`sin`, `cos`, `log2`, `acos`, ...) behind an
//! `OnceLock`. That pulled a floating-point maths library into the run-time
//! path, cost roughly 25 million instructions on the first frame of a
//! 32-bit core with no floating-point unit, and made a `no_std` build
//! impossible. Now the values are computed here, once, by exactly the same
//! formulas, written into `tables.rs`, and checked in. The drift test below
//! regenerates every value and fails if the committed file differs, so the
//! file cannot silently rot.
//!
//! Regenerate after changing a formula:
//! `cargo test --release --features ambe_plus_2 --lib tables_gen::regenerate
//! -- --ignored` (writes `src/codec2_3200/tables.rs`).
//!
//! The formulas use the host's `f32` maths library; a platform whose `cosf`
//! rounds differently in the last place would make the drift test fail
//! there even though nothing is wrong. The committed values are the source
//! of truth at run time, so that only matters when regenerating.

use super::lpc::COEF_FRAC_BITS;
use super::spectral_bridge::SAMPLES_PER_FRAME_SB;
use super::*;
use std::f32::consts::{LOG2_10, PI, TAU};

fn q(y: f32, frac_bits: u32) -> i64 {
    fixed_point::f32_to_q_exact_round(y, frac_bits)
}

/// The `f32 -> Q` conversions the individual modules used for their tables
/// (`(x as f64 * 2^bits).round()`), kept separate from `q` because the two
/// differ in how they round a few ties.
fn q_f64(x: f32, frac_bits: u32) -> i64 {
    (x as f64 * (1i64 << frac_bits) as f64).round() as i64
}

const LOG2_LUT_BITS: u32 = 8;
const LOG2_LUT_SIZE: usize = (1 << LOG2_LUT_BITS) + 1;
const TRIG_LUT_BITS: u32 = 12;
const TRIG_LUT_SIZE: usize = (1 << TRIG_LUT_BITS) + 1;

pub(super) enum Value {
    Scalar(&'static str, &'static str, i64),
    Array(&'static str, &'static str, &'static str, Vec<i64>),
    Pairs(&'static str, &'static str, Vec<(i64, i64)>),
}

pub(super) fn all_values() -> Vec<Value> {
    let mut v = Vec::new();
    let s = |v: &mut Vec<Value>, name, doc, val| v.push(Value::Scalar(name, doc, val));

    // ---- scalars: envelope ----
    s(&mut v, "ENVELOPE_EPS_A2_Q23", "1e-6, Q23", q(1e-6, 23));
    s(&mut v, "ENVELOPE_BETA_Q23", "LPCPF_BETA, Q23", q(LPCPF_BETA, 23));
    s(&mut v, "ENVELOPE_ONE_PLUS_BETA_Q23", "1 + LPCPF_BETA, Q23", q(1.0 + LPCPF_BETA, 23));
    s(&mut v, "ENVELOPE_BOOST_RATIO_Q23", "1.96, Q23", q(1.96, 23));
    s(
        &mut v,
        "ENVELOPE_FIRST_HARMONIC_WO_THRESHOLD_Q23",
        "PI * 150 / 4000, Q23",
        q(PI * 150.0 / 4000.0, 23),
    );
    s(&mut v, "ENVELOPE_FIRST_HARMONIC_CORRECTION_Q23", "0.032, Q23", q(0.032, 23));
    // ---- scalars: synthesis ----
    s(&mut v, "SYNTH_BG_THRESH_Q23", "BG_THRESH, Q23", q(BG_THRESH, 23));
    s(&mut v, "SYNTH_BG_BETA_Q23", "BG_BETA, Q23", q(BG_BETA, 23));
    s(&mut v, "SYNTH_ONE_MINUS_BG_BETA_Q23", "1 - BG_BETA, Q23", q(1.0 - BG_BETA, 23));
    s(&mut v, "SYNTH_BG_MARGIN_Q23", "BG_MARGIN, Q23", q(BG_MARGIN, 23));
    s(&mut v, "SYNTH_TEN_OVER_LOG2_10_Q23", "10 / log2(10), Q23", q(10.0 / LOG2_10, 23));
    s(&mut v, "SYNTH_LOG2_10_OVER_20_Q23", "log2(10) / 20, Q23", q(LOG2_10 / 20.0, 23));
    s(&mut v, "SYNTH_EAR_PROTECTION_THRESH_Q23", "30000, Q23", q(30000.0, 23));
    // ---- scalars: quantiser ----
    s(&mut v, "QUANT_W0_MIN_Q23", "W0_MIN, Q23", q(W0_MIN, COEF_FRAC_BITS));
    s(
        &mut v,
        "QUANT_W0_STEP_Q23",
        "(W0_MAX - W0_MIN) / 2^WO_BITS, Q23",
        q((W0_MAX - W0_MIN) / (1u32 << WO_BITS) as f32, COEF_FRAC_BITS),
    );
    s(
        &mut v,
        "QUANT_ENERGY_Y_MIN_Q23",
        "E_MIN_DB / 10 * log2(10), Q23",
        q(E_MIN_DB / 10.0 * LOG2_10, 23),
    );
    s(
        &mut v,
        "QUANT_ENERGY_Y_STEP_Q23",
        "(E_MAX_DB - E_MIN_DB) / 2^E_BITS / 10 * log2(10), Q23",
        q((E_MAX_DB - E_MIN_DB) / (1u32 << E_BITS) as f32 / 10.0 * LOG2_10, 23),
    );
    s(&mut v, "QUANT_HZ_PER_RAD_Q16", "4000 / PI, Q16", q(4000.0 / PI, 16));
    s(&mut v, "QUANT_RAD_PER_HZ_Q23", "PI / 4000, Q23", q(PI / 4000.0, COEF_FRAC_BITS));
    // ---- scalars: nlp ----
    s(&mut v, "NLP_NOTCH_A_Q23", "NOTCH_A, Q23", q_f64(nlp::NOTCH_A, 23));
    s(&mut v, "NLP_CNLP_Q23", "CNLP, Q23", q_f64(nlp::CNLP, 23));
    // ---- scalars: misc ----
    s(&mut v, "LPC_PI_Q23", "PI, Q23", q(PI, COEF_FRAC_BITS));
    s(&mut v, "MOD_W0_MIN_Q23", "W0_MIN, Q23", q(W0_MIN, COEF_FRAC_BITS));
    s(&mut v, "SB_HZ_PER_RAD_Q23", "SAMPLE_RATE / TAU, Q23", q(SAMPLE_RATE as f32 / TAU, 23));

    // ---- arrays ----
    let lpf = nlp::design_lowpass(nlp::LPF_TAPS, 0.5 / NLP_DEC as f32);
    v.push(Value::Array(
        "NLP_LOWPASS_Q23",
        "i64",
        "Windowed-sinc decimation filter, Q23.",
        lpf.iter().map(|&x| q_f64(x, 23)).collect(),
    ));
    v.push(Value::Array(
        "NLP_HANN_Q23",
        "i64",
        "Hann window over the decimated block, Q23.",
        (0..nlp::NDEC)
            .map(|i| q_f64(0.5 - 0.5 * (TAU * i as f32 / (nlp::NDEC - 1) as f32).cos(), 23))
            .collect(),
    ));
    v.push(Value::Pairs(
        "NLP_TWIDDLES_Q23",
        "Pitch-estimator FFT twiddles `(cos, sin)` of `TAU * k / 512`, Q23 (computed in f32, so they differ by a last bit from the general FFT's table).",
        (0..PE_FFT_SIZE / 2)
            .map(|k| {
                let theta = TAU * k as f32 / PE_FFT_SIZE as f32;
                (q_f64(theta.cos(), 23), q_f64(theta.sin(), 23))
            })
            .collect(),
    ));
    v.push(Value::Array(
        "WINDOW_ANALYSIS_Q30",
        "i32",
        "Analysis window, Q30.",
        window::make_analysis_window()
            .iter()
            .map(|&w| (w as f64 * (1i64 << 30) as f64).round())
            .map(|x| x as i64)
            .collect(),
    ));
    let log2_f: Vec<f32> = (0..LOG2_LUT_SIZE)
        .map(|i| (1.0 + i as f32 / (1u32 << LOG2_LUT_BITS) as f32).log2())
        .collect();
    let exp2_f: Vec<f32> = (0..LOG2_LUT_SIZE)
        .map(|i| (i as f32 / (1u32 << LOG2_LUT_BITS) as f32).exp2())
        .collect();
    v.push(Value::Array(
        "LOG2_LUT_Q23",
        "i32",
        "log2(1 + i/256), Q23.",
        log2_f.iter().map(|&f| (f * (1i64 << 23) as f32).round() as i64).collect(),
    ));
    v.push(Value::Array(
        "EXP2_LUT_FRAC_Q23",
        "i32",
        "2^(i/256) - 1, Q23.",
        exp2_f.iter().map(|&f| ((f - 1.0) * (1i64 << 23) as f32).round() as i64).collect(),
    ));
    let trig = |f: fn(f32) -> f32| -> Vec<i64> {
        let levels = 1u32 << TRIG_LUT_BITS;
        let mut t: Vec<i64> = (0..TRIG_LUT_SIZE)
            .map(|i| {
                let angle = i as f32 / levels as f32 * TAU;
                (f(angle) as f64 * (1i64 << 23) as f64).round() as i64
            })
            .collect();
        t[levels as usize] = t[0];
        t
    };
    v.push(Value::Array("TRIG_COS_Q23", "i32", "cos(TAU * i / 4096), Q23.", trig(f32::cos)));
    v.push(Value::Array("TRIG_SIN_Q23", "i32", "sin(TAU * i / 4096), Q23.", trig(f32::sin)));
    let acos_lut: Vec<i64> = (0..TRIG_LUT_SIZE)
        .map(|i| q_f64((i as f32 / (1u32 << TRIG_LUT_BITS) as f32).acos(), COEF_FRAC_BITS))
        .collect();
    v.push(Value::Array("LPC_ACOS_LUT_Q23", "i32", "acos(i / 4096), Q23.", acos_lut));
    let cos_lut: Vec<i64> = (0..TRIG_LUT_SIZE)
        .map(|i| q_f64((i as f32 / (1u32 << TRIG_LUT_BITS) as f32 * PI).cos(), COEF_FRAC_BITS))
        .collect();
    v.push(Value::Array("LPC_COS_LUT_Q23", "i32", "cos(PI * i / 4096), Q23.", cos_lut));
    v.push(Value::Array(
        "SYNTH_PARZEN_Q23",
        "i64",
        "Synthesis overlap-add window, Q23.",
        synthesis::make_synthesis_window().iter().map(|&p| q(p, 23)).collect(),
    ));
    v.push(Value::Array(
        "SB_PARZEN_Q23",
        "i64",
        "Spectral-bridge overlap-add window, Q23.",
        spectral_bridge::make_synthesis_window_sb().iter().map(|&p| q(p, 23)).collect::<Vec<_>>(),
    ));
    debug_assert_eq!(SAMPLES_PER_FRAME_SB, 320);
    v.push(Value::Array(
        "NLP_BIN_WO_INDEX",
        "u8",
        "Transmitted `Wo` index for each pitch-estimator bin (fundamental `bin * 3.125` Hz): the float `encode_wo(f0_to_wo(f0))` path evaluated once per bin.",
        (0..=PE_FFT_SIZE / 2)
            .map(|bin| {
                let bin_to_hz = SAMPLE_RATE as f32 / (PE_FFT_SIZE * NLP_DEC) as f32;
                quantise::encode_wo(nlp::f0_to_wo(bin as f32 * bin_to_hz)) as i64
            })
            .collect(),
    ));
    let lsps = initial_lsps();
    v.push(Value::Array(
        "MOD_INITIAL_LSPS_Q23",
        "i64",
        "Decoder's initial line spectral pairs, Q23.",
        lsps.iter().map(|&l| q(l, COEF_FRAC_BITS)).collect(),
    ));
    v.push(Value::Array(
        "MOD_FALLBACK_LSP_Q23",
        "i64",
        "Evenly spaced fallback line spectral pairs used when root finding fails, Q23.",
        (0..LPC_ORD)
            .map(|i| q((PI / LPC_ORD as f32) * i as f32, COEF_FRAC_BITS))
            .collect(),
    ));
    v
}

pub(super) fn render() -> String {
    use std::fmt::Write;
    let mut out = String::new();
    out.push_str(
        "// SPDX-License-Identifier: LGPL-3.0-or-later\n\
         //! GENERATED FILE -- do not edit by hand. Produced by `tables_gen.rs`\n\
         //! (`cargo test --release --features ambe_plus_2 --lib tables_gen::regenerate -- --ignored`);\n\
         //! `tables_gen::tests::committed_tables_match_the_generator` fails if this drifts.\n\
         //! Contains no floating point: every value is an integer constant.\n\n",
    );
    // The 16 kHz spectral-bridge tables are only compiled when that feature is on.
    let gate = |name: &str| {
        if name.starts_with("SB_") {
            "#[cfg(feature = \"codec2_16k_bridge\")]\n"
        } else {
            ""
        }
    };
    for val in all_values() {
        match val {
            Value::Scalar(name, doc, x) => {
                writeln!(out, "/// {doc}\n{}pub(crate) const {name}: i64 = {x};", gate(name)).unwrap();
            }
            Value::Array(name, ty, doc, xs) => {
                writeln!(out, "/// {doc}\n{}pub(crate) static {name}: [{ty}; {}] = [", gate(name), xs.len()).unwrap();
                for chunk in xs.chunks(8) {
                    let line: Vec<String> = chunk.iter().map(|x| x.to_string()).collect();
                    writeln!(out, "    {},", line.join(", ")).unwrap();
                }
                out.push_str("];\n");
            }
            Value::Pairs(name, doc, xs) => {
                writeln!(out, "/// {doc}\n{}pub(crate) static {name}: [(i64, i64); {}] = [", gate(name), xs.len()).unwrap();
                for chunk in xs.chunks(4) {
                    let line: Vec<String> = chunk.iter().map(|(a, b)| format!("({a}, {b})")).collect();
                    writeln!(out, "    {},", line.join(", ")).unwrap();
                }
                out.push_str("];\n");
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "writes src/codec2_3200/tables.rs; run explicitly after changing a formula"]
    fn regenerate() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/src/codec2_3200/tables.rs");
        std::fs::write(path, render()).expect("write tables.rs");
    }

    #[test]
    fn committed_tables_match_the_generator() {
        let committed = include_str!("tables.rs");
        assert!(
            committed == render(),
            "src/codec2_3200/tables.rs has drifted from tables_gen.rs; regenerate it with \
             `cargo test --release --features ambe_plus_2 --lib tables_gen::tests::regenerate -- --ignored`"
        );
    }
}
