// SPDX-License-Identifier: LGPL-3.0-or-later
//! Fixed-point-oriented primitives for this port, built against the
//! real, measured bit-width bounds in
//! `docs/references/CODEC2_MOD_FIXED_POINT_PLAN.md` -- but only for the
//! stages that plan doc's own advisor review confirmed transfer
//! cleanly to *this* port's own structurally-identical code, with an
//! acceptance criterion that doesn't require a product-judgment call
//! first.
//!
//! **Levinson-Durbin's own `|k|>1` clamp-boundary decision (previously
//! deliberately excluded here pending Bruce's own accept-vs-stabilize
//! call) is now made and built**, per Bruce's own explicit direction:
//! match the float reference's clamp behavior as closely as practical
//! and no closer -- accept its real, measured divergence rate rather
//! than adding a stabilization/smoothing step. See `lpc.rs`'s own
//! `levinson_durbin_fixed` for the real, genuinely-integer
//! implementation (not here, since it doesn't use this module's own
//! log-domain LUT shape) and its own doc comment for the real
//! measurement behind its Q8.40 internal format choice.
//!
//! **A real, honest limitation found while landing that work, not yet
//! fixed**: `log2_lut`/`exp2_lut` below are described as "fixed-point-
//! oriented," and their exponent/mantissa split is exact bit
//! manipulation (`to_bits()`/`from_bits()`, free on real fixed-point
//! hardware too), but the interpolation arithmetic itself
//! (`(mantissa - 1.0) * levels as f32`, the table lookup's own linear
//! blend) is genuine `f32` multiply/subtract -- it will not run on
//! genuinely FPU-less hardware (the actual target this whole file
//! exists for: cheap HTs, ESP32-class parts with no good FPU) without
//! either real hardware float support or a software float-emulation
//! library, which defeats the point. This is a real, separate stage
//! needing its own genuine integer conversion (interpret the input as
//! a Q-format integer, extract the exponent via a bit-scan/`clz`
//! instead of IEEE754 bit tricks, interpolate in integer arithmetic) --
//! not attempted in this pass, which was scoped to Levinson-Durbin's
//! own clamp decision specifically. `levinson_durbin_fixed`'s own core
//! recursion, by contrast, is genuine integer arithmetic throughout
//! (`i64`/`i128` only, no `f32` inside the loop) -- the two stages are
//! at different real maturity levels for the no-FPU target, not
//! interchangeably "done."
//!
//! `log2_lut`/`exp2_lut` below are what `quantise::encode_energy`/
//! `decode_energy` AND `synthesis::postfilter_step` actually call now --
//! not a parallel unused implementation sitting next to a plain-float
//! one, the codec's real log-domain primitive at both call sites --
//! replacing each site's own plain-float `log10`/`powf` round trip with
//! the same 8-bit (256-entry), linearly-interpolated log2/exp2 LUT
//! shape the plan doc validated for `aks_to_mag2`'s own `R^(2*BETA)`
//! treatment (`x^k == 2^(k*log2(x))`, computed in base 2 specifically
//! because that's what a real fixed-point target would implement --
//! `frexp`-style exponent extraction is free, only the mantissa's own
//! log2 needs a table). Everything surrounding the LUT call itself (the
//! multiply by 10, the linear quantizer, the EMA) deliberately stays in
//! `f32` here, isolating the log-domain treatment specifically, matching
//! the plan doc's own validation scope for `aks_to_mag2` (Q-format
//! widths for the surrounding fixed-point arithmetic are a separate,
//! later engineering step, not attempted here).

use std::sync::OnceLock;

/// LUT resolution used by the real `log2_lut`/`exp2_lut` below -- 8
/// bits, matching the plan doc's own validated `aks_to_mag2` result
/// (max relative error 8.25e-7 there). Kept as a named constant (not
/// hardcoded into the table size) so the negative-control tests can
/// build a deliberately coarser table with the same generic code path
/// and confirm the choice of resolution is actually load-bearing, not
/// just present.
const LOG2_LUT_BITS: u32 = 8;
const LOG2_LUT_SIZE: usize = (1 << LOG2_LUT_BITS) + 1;

fn log2_lut_table() -> &'static [f32; LOG2_LUT_SIZE] {
    static TABLE: OnceLock<[f32; LOG2_LUT_SIZE]> = OnceLock::new();
    TABLE.get_or_init(|| std::array::from_fn(|i| (1.0 + i as f32 / (1u32 << LOG2_LUT_BITS) as f32).log2()))
}

fn exp2_lut_table() -> &'static [f32; LOG2_LUT_SIZE] {
    static TABLE: OnceLock<[f32; LOG2_LUT_SIZE]> = OnceLock::new();
    TABLE.get_or_init(|| std::array::from_fn(|i| (i as f32 / (1u32 << LOG2_LUT_BITS) as f32).exp2()))
}

/// `log2(x)` via IEEE754 exponent/mantissa split (exact, free -- just
/// the bit pattern) plus a linearly-interpolated table lookup for the
/// mantissa's own log2 -- the real fixed-point-friendly approximation
/// shape (`frexpf` + LUT) the plan doc validated, not a float shortcut.
/// `bits`/`table` are parameterized so the negative-control test below
/// can exercise the exact same interpolation code at a deliberately
/// coarser resolution.
fn log2_lut_generic(x: f32, bits: u32, table: &[f32]) -> f32 {
    debug_assert!(x > 0.0, "log2_lut_generic: x must be positive, got {x}");
    let levels = 1u32 << bits;
    let raw = x.to_bits();
    let exponent = ((raw >> 23) & 0xFF) as i32 - 127;
    let mantissa = f32::from_bits((raw & 0x007F_FFFF) | 0x3F80_0000); // [1.0, 2.0)
    let scaled = (mantissa - 1.0) * levels as f32;
    let idx = (scaled as usize).min(levels as usize - 1);
    let frac = scaled - idx as f32;
    exponent as f32 + table[idx] + frac * (table[idx + 1] - table[idx])
}

/// `2^y` via integer/fractional split (`2^y = 2^floor(y) * 2^frac`) plus
/// a linearly-interpolated table lookup for `2^frac` -- the inverse of
/// `log2_lut_generic`, same real fixed-point-friendly shape.
fn exp2_lut_generic(y: f32, bits: u32, table: &[f32]) -> f32 {
    let levels = 1u32 << bits;
    let floor_y = y.floor();
    let frac = y - floor_y;
    let scaled = frac * levels as f32;
    let idx = (scaled as usize).min(levels as usize - 1);
    let t = scaled - idx as f32;
    let mantissa = table[idx] + t * (table[idx + 1] - table[idx]);
    mantissa * 2f32.powi(floor_y as i32)
}

/// `pub(crate)`: `quantise::encode_energy` calls this directly (see
/// that function) -- this is the actual implementation the codec uses,
/// not a parallel unused sibling.
pub(crate) fn log2_lut(x: f32) -> f32 {
    log2_lut_generic(x, LOG2_LUT_BITS, log2_lut_table())
}

/// `pub(crate)`: `quantise::decode_energy` calls this directly.
pub(crate) fn exp2_lut(y: f32) -> f32 {
    exp2_lut_generic(y, LOG2_LUT_BITS, exp2_lut_table())
}

#[cfg(test)]
mod tests {
    use super::*;

    macro_rules! fixture {
        ($name:literal) => {
            concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/codec2_3200/", $name)
        };
    }

    fn read_dump(path: &str, cols: usize) -> Vec<Vec<f32>> {
        std::fs::read_to_string(path)
            .unwrap_or_else(|e| panic!("{path}: {e}"))
            .lines()
            .map(|line| {
                let v: Vec<f32> = line.split_whitespace().map(|s| s.parse().unwrap()).collect();
                assert_eq!(v.len(), cols, "line has {} fields, expected {cols}", v.len());
                v
            })
            .collect()
    }

    #[test]
    fn log2_lut_and_exp2_lut_are_real_inverses_across_a_wide_dynamic_range() {
        // Sweeps many decades (matching the real E_MIN_DB..E_MAX_DB dB
        // span converted to linear, 1e-1..1e4, with real margin either
        // side) -- confirms the two LUTs are actually inverses of each
        // other, not just individually plausible.
        let mut x = 1e-3f32;
        let mut max_rel_err = 0.0f32;
        while x < 1e6 {
            let y = log2_lut(x);
            let back = exp2_lut(y);
            let rel_err = ((back - x) / x).abs();
            max_rel_err = max_rel_err.max(rel_err);
            x *= 1.0173; // dense, irrational-ish multiplicative step
        }
        assert!(max_rel_err < 1e-4, "log2_lut/exp2_lut round trip relative error too large: {max_rel_err}");
    }

    /// Independent plain-float reference (`log10`/`powf`, no LUT at
    /// all) for what `quantise::encode_energy` computed *before* this
    /// module existed -- deliberately NOT calling
    /// `quantise::encode_energy` itself, since that function now calls
    /// straight into `log2_lut` (see this module's own doc comment):
    /// comparing the LUT against itself via that indirection would be
    /// circular and prove nothing.
    fn reference_e_db(e_linear: f32) -> f32 {
        10.0 * e_linear.max(1e-12).log10()
    }

    #[test]
    fn the_8_bit_log2_lut_reproduces_the_plain_float_log10_quantizer_decision_on_real_encoder_side_data_with_zero_index_mismatches() {
        // Real ENCODER-side e (the actual encode_energy() call-site
        // argument, captured directly -- see quantise.rs's own test of
        // the same fixture for why that distinction matters).
        let e_path = fixture!("codec2_enc_e_dump.txt");
        let es = read_dump(e_path, 1);
        assert!(es.len() > 300, "expected the real captured fixture corpus, got {} rows", es.len());

        let mut mismatches = 0;
        for row in &es {
            let e = row[0];
            let plain_idx = super::super::quantise::quantize_linear(reference_e_db(e), super::super::E_MIN_DB, super::super::E_MAX_DB, super::super::E_BITS);
            let lut_e_db = 10.0 * (log2_lut(e.max(1e-12)) / std::f32::consts::LOG2_10);
            let lut_idx = super::super::quantise::quantize_linear(lut_e_db, super::super::E_MIN_DB, super::super::E_MAX_DB, super::super::E_BITS);
            if plain_idx != lut_idx {
                mismatches += 1;
            }
        }
        assert_eq!(mismatches, 0, "LUT-based log2 diverged from plain log10 on {mismatches}/{} real frames -- the plan doc's own validated result for this LUT design is zero mismatches", es.len());
    }

    #[test]
    fn a_deliberately_coarse_4_bit_lut_produces_real_index_mismatches_confirming_the_test_above_is_not_vacuous() {
        // Negative control, same methodology the plan doc itself used
        // for aks_to_mag2: rerun the real fixture corpus through the
        // exact same interpolation code at a much coarser resolution
        // and confirm it actually degrades -- if it didn't, the
        // zero-mismatches result above wouldn't be evidence the 8-bit
        // resolution matters, just that log2/exp2-LUT-shaped code
        // happens to always match float here regardless of table size.
        const COARSE_BITS: u32 = 4;
        let coarse_log2: Vec<f32> = (0..=(1u32 << COARSE_BITS)).map(|i| (1.0 + i as f32 / (1u32 << COARSE_BITS) as f32).log2()).collect();

        let e_path = fixture!("codec2_enc_e_dump.txt");
        let es = read_dump(e_path, 1);

        let mut mismatches = 0;
        for row in &es {
            let e = row[0].max(1e-12);
            let e_db_coarse = 10.0 * (log2_lut_generic(e, COARSE_BITS, &coarse_log2) / std::f32::consts::LOG2_10);
            let coarse_idx = super::super::quantise::quantize_linear(e_db_coarse, super::super::E_MIN_DB, super::super::E_MAX_DB, super::super::E_BITS);
            let plain_idx = super::super::quantise::quantize_linear(reference_e_db(e), super::super::E_MIN_DB, super::super::E_MAX_DB, super::super::E_BITS);
            if coarse_idx != plain_idx {
                mismatches += 1;
            }
        }
        assert!(mismatches > 0, "expected the deliberately coarse 4-bit LUT to produce at least one real index mismatch against the plain float quantizer -- got zero, which would mean the 8-bit result above isn't evidence of anything");
    }

    #[test]
    fn encode_energy_and_decode_energy_now_run_through_the_lut_and_still_round_trip_within_one_quantizer_step() {
        // Closes the loop advisor flagged: after quantise::encode_energy/
        // decode_energy were switched to call straight into this
        // module's LUT, does a real captured e still round-trip through
        // the *actual, now-LUT-backed* encode_energy/decode_energy to
        // within the same one-quantizer-step bound the plain-float
        // version always met? (quantise.rs's own
        // energy_quantizer_matches_real_encoder_side_data_within_the_real_5_bit_step
        // test already asserts this same bound against the same
        // fixture -- this test exists so that guarantee is visible from
        // this module too, right next to the LUT it now depends on.)
        let e_path = fixture!("codec2_enc_e_dump.txt");
        let es = read_dump(e_path, 1);
        let step_db = (super::super::E_MAX_DB - super::super::E_MIN_DB) / (1 << super::super::E_BITS) as f32;

        for row in &es {
            let e = row[0];
            let e_db = reference_e_db(e);
            if !(super::super::E_MIN_DB..=super::super::E_MAX_DB).contains(&e_db) {
                continue; // real, designed clamp region -- see quantise.rs's own test for why
            }
            let idx = super::super::quantise::encode_energy(e);
            let back = super::super::quantise::decode_energy(idx);
            let back_db = reference_e_db(back);
            assert!((back_db - e_db).abs() <= step_db, "LUT-backed round trip for e={e} (db={e_db}) landed at {back} (db={back_db}), step {step_db}dB");
        }
    }

    #[test]
    fn postfilter_lut_decisions_match_plain_float_across_a_real_temporal_replay() {
        // Same validation shape CODEC2_MOD_FIXED_POINT_PLAN.md used for
        // this exact stage (bg_est is a real stateful EMA carrying
        // across frames, so a faithful test needs a real temporal
        // replay, not independent per-frame samples): run two parallel
        // postfilter_step state machines -- one plain float, one this
        // module's LUT -- forward through an identical real sequence of
        // (voiced, l, a[]), and check every real per-harmonic threshold
        // decision matches.
        //
        // The per-harmonic amplitudes (`a[]`) driving this come from
        // real captured data (this port's own already-validated
        // lsp_to_lpc + compute_harmonic_amplitudes run on real captured
        // lsp[]/e[] fixture rows) -- that's what determines whether the
        // log-domain LUT approximation ever changes a real decision, so
        // it has to be real. `voiced`/`Wo` only select which branch
        // runs and how many harmonics exist; a representative synthetic
        // sweep (long voiced/unvoiced RUNS, not frame-by-frame
        // alternation, so bg_est's own EMA has real stretches to settle
        // against before being tested) is fine there -- the same
        // encoder/decoder-internal-design-freedom reasoning `nlp.rs`'s
        // own module doc comment uses for a transmitted value, applied
        // here to a purely decoder-internal test-harness choice instead.
        let lsp_rows = read_dump(fixture!("codec2_lsp_dump.txt"), super::super::LPC_ORD + 1);
        let e_rows = read_dump(fixture!("codec2_enc_e_dump.txt"), 1);
        let n = lsp_rows.len().min(e_rows.len());
        assert!(n > 300, "expected the real captured fixture corpus, got {n} rows");

        let fft = {
            let mut planner = rustfft::FftPlanner::<f32>::new();
            planner.plan_fft_forward(super::super::FFT_ENC)
        };

        let mut plain_bg = 0.0f32;
        let mut lut_bg = 0.0f32;
        let mut decisions_checked = 0usize;
        let mut decision_mismatches = 0usize;
        let mut max_bg_drift_db = 0.0f32;

        for i in 0..n {
            let roots = lsp_rows[i][0] as i32;
            if roots as usize != super::super::LPC_ORD {
                continue; // real, rare LSP root-finding failure -- skip, same as quantise.rs's own test
            }
            let mut lsp = [0.0f32; super::super::LPC_ORD];
            lsp.copy_from_slice(&lsp_rows[i][1..]);
            let e = e_rows[i][0];

            // 30-frame runs: long enough for bg_est's BG_BETA=0.1 EMA
            // to settle meaningfully within each unvoiced run before
            // the following voiced run's decisions get checked against it.
            let voiced = (i / 30) % 2 == 1;
            let wo = super::super::W0_MIN + (super::super::W0_MAX - super::super::W0_MIN) * 0.3;

            let ak = super::super::lpc::lsp_to_lpc(&lsp);
            let mut model = super::super::envelope::Model::new(wo, voiced);
            let _aw = super::super::envelope::compute_harmonic_amplitudes(fft.as_ref(), &ak, e, &mut model);
            super::super::envelope::apply_first_harmonic_correction(&mut model);

            let (new_plain_bg, plain_decisions) = super::super::synthesis::postfilter_step(model.voiced, model.l, &model.a, plain_bg, f32::log2, f32::exp2);
            let (new_lut_bg, lut_decisions) = super::super::synthesis::postfilter_step(model.voiced, model.l, &model.a, lut_bg, log2_lut, exp2_lut);

            max_bg_drift_db = max_bg_drift_db.max((new_plain_bg - new_lut_bg).abs());
            plain_bg = new_plain_bg;
            lut_bg = new_lut_bg;

            if voiced {
                for m in 1..=model.l {
                    decisions_checked += 1;
                    if plain_decisions[m] != lut_decisions[m] {
                        decision_mismatches += 1;
                    }
                }
            }
        }

        assert!(decisions_checked > 1000, "expected a real number of per-harmonic decisions checked across the replay, got {decisions_checked}");
        assert_eq!(decision_mismatches, 0, "{decision_mismatches}/{decisions_checked} real per-harmonic postfilter decisions diverged between the LUT and plain float across the temporal replay");
        assert!(max_bg_drift_db < 1e-3, "bg_est drifted {max_bg_drift_db}dB between the LUT and plain-float state machines over the replay -- too large for an 8-bit LUT");
    }
}
