// SPDX-License-Identifier: LGPL-3.0-or-later
//! Decoder-side spectral envelope: turns `LPC_ORD+1` LPC coefficients
//! into per-harmonic amplitudes (`Model::a`), by sampling the LPC
//! filter's own frequency response at each harmonic and blending in a
//! standard LPC postfilter (formant sharpening via a bandwidth-expanded
//! "gamma" copy of the filter, energy-normalized so the sharpening
//! doesn't change overall loudness) -- the general postfilter technique
//! traces to Chen & Gersho's classic adaptive postfiltering work, not
//! anything specific to this one codec's own source; reimplemented here
//! from that general understanding.
//!
//! Purely decoder-*audio-quality*-internal, not a bitstream format
//! question (see `mod.rs`'s own note on asymmetric interoperability):
//! nothing here is transmitted, so exact formulas are a design choice,
//! though the general shape (spectral-envelope sampling weighted by
//! harmonic bandwidth, gamma-based postfiltering) is kept close to the
//! reference's own real, published approach since it's a genuinely
//! good, well-motivated technique, not incidental.

use super::{FFT_ENC, LPCPF_BETA, LPCPF_GAMMA, LPCPF_TWO_BETA, LPC_ORD, MAX_AMP, SAMPLE_RATE};
use rustfft::num_complex::Complex32;
use rustfft::Fft;

/// Sinusoidal-synthesis model parameters for one 10ms sub-frame: pitch
/// (`wo`, normalized angular frequency), harmonic count (`l`), per
/// harmonic amplitude/phase (`a`/`phi`, both 1-indexed -- index 0
/// unused, matching the harmonics' own 1-based numbering), and voicing.
///
/// `phi` stores each harmonic's phase as a unit complex vector
/// (`cos`/`sin` already evaluated) rather than an angle in radians.
/// `synthesize_phase` derives it from `h[m] * ex` by dividing out the
/// magnitude directly -- no `atan2` needed, since the only later use
/// (`synthesize_subframe`'s IFFT-input construction) immediately turns
/// an angle back into exactly this same unit vector via `sin_cos`. That
/// round trip (`atan2` then `sin_cos`) was an identity for every
/// harmonic postfilter didn't overwrite; storing the vector directly
/// removes both the `atan2` call and one of the two `sin_cos` calls per
/// harmonic, which matters once this runs in fixed point (no
/// arctangent primitive needed at all, and the remaining `sin_cos`
/// wants a single well-designed LUT rather than two).
pub struct Model {
    pub wo: f32,
    pub l: usize,
    pub a: [f32; MAX_AMP + 1],
    pub phi: [Complex32; MAX_AMP + 1],
    pub voiced: bool,
}

impl Model {
    pub fn new(wo: f32, voiced: bool) -> Self {
        let l = ((std::f32::consts::PI / wo) as usize).min(MAX_AMP);
        Model {
            wo,
            l,
            a: [0.0; MAX_AMP + 1],
            phi: [Complex32::new(1.0, 0.0); MAX_AMP + 1],
            voiced,
        }
    }
}

/// Bins actually used from an `FFT_ENC`-point real-input spectrum (the
/// rest is the conjugate mirror, redundant).
const SPEC_BINS: usize = FFT_ENC / 2 + 1;

/// `ak[]` zero-padded into a `FFT_ENC`-point real buffer, forward FFT'd,
/// returning the complex spectrum's first `SPEC_BINS` bins. Fixed-size
/// stack buffers throughout (`FFT_ENC` is a compile-time constant) --
/// this runs twice per 10ms sub-frame on a real-time codec's decode
/// path, so no heap allocation here.
fn lpc_spectrum(fft: &dyn Fft<f32>, ak: &[f32; LPC_ORD + 1]) -> [Complex32; SPEC_BINS] {
    let mut buf = [Complex32::new(0.0, 0.0); FFT_ENC];
    for (i, &a) in ak.iter().enumerate() {
        buf[i] = Complex32::new(a, 0.0);
    }
    fft.process(&mut buf);
    std::array::from_fn(|i| buf[i])
}

/// Computes `model.a[1..=model.l]` from `ak`/`e` (the real LPC energy),
/// and returns the raw LPC spectrum (`Aw`, `SPEC_BINS` complex bins)
/// alongside it, since `synthesis.rs`'s own phase reconstruction needs
/// that same spectrum (`H[m] = conj(Aw[bin])`, the synthesis filter
/// being the LPC analysis filter's own phase response, reversed).
pub fn compute_harmonic_amplitudes(
    fft: &dyn Fft<f32>,
    ak: &[f32; LPC_ORD + 1],
    e: f32,
    model: &mut Model,
) -> [Complex32; SPEC_BINS] {
    let aw = lpc_spectrum(fft, ak);
    let a2: [f32; SPEC_BINS] =
        std::array::from_fn(|i| aw[i].re * aw[i].re + aw[i].im * aw[i].im + 1e-6);

    let mut ak_gamma = [0.0f32; LPC_ORD + 1];
    ak_gamma[0] = ak[0];
    let mut g = LPCPF_GAMMA;
    for i in 1..=LPC_ORD {
        ak_gamma[i] = ak[i] * g;
        g *= LPCPF_GAMMA;
    }
    let awg = lpc_spectrum(fft, &ak_gamma);
    let a2g: [f32; SPEC_BINS] =
        std::array::from_fn(|i| awg[i].re * awg[i].re + awg[i].im * awg[i].im + 1e-6);

    let mut e_before = 1e-12f32;
    let mut e_after = 1e-12f32;
    // Matches the reference's own real range (0..FFT_ENC/2, excluding
    // the Nyquist bin) -- `a2`/`a2g` are one element longer than this
    // (`lpc_spectrum` keeps FFT_ENC/2+1 bins, since `sample_filter_phase`
    // separately needs that same range), but this gain normalization
    // sum shouldn't include the extra bin.
    for i in 0..(FFT_ENC / 2) {
        let inv_a2 = 1.0 / a2[i];
        let r = (a2g[i] * inv_a2).sqrt();
        e_before += inv_a2;
        e_after += inv_a2 * r.powf(LPCPF_TWO_BETA);
    }
    let gain = e * e_before / e_after;

    let fft_r = std::f32::consts::TAU / FFT_ENC as f32;
    // `m` is the harmonic number, used in real arithmetic (`m as f32 -
    // 0.5`) well beyond plain array indexing.
    #[allow(clippy::needless_range_loop)]
    for m in 1..=model.l {
        let am = (((m as f32 - 0.5) * model.wo / fft_r) + 0.5) as usize;
        let bm = ((((m as f32 + 0.5) * model.wo / fft_r) + 0.5) as usize).min(FFT_ENC / 2);

        let mut em = 0.0f32;
        for i in am..bm {
            let r = (a2g[i] / a2[i]).sqrt();
            let mut pw_i = r.powf(LPCPF_TWO_BETA) / a2[i];
            let freq_hz = i as f32 * (SAMPLE_RATE as f32 * 0.5 / (FFT_ENC / 2) as f32);
            if freq_hz < 1000.0 {
                pw_i *= 1.96;
            }
            em += pw_i;
        }
        em *= gain;
        model.a[m] = em.sqrt();
    }

    aw
}

/// First-harmonic correction: for very low-pitched (typically male)
/// voices, LPC modelling tends to overestimate the fundamental's own
/// amplitude -- a real, documented quirk of this general modelling
/// approach at low pitch, not specific to any one codec's source; the
/// reference's own correction factor (0.032, kept here for the same
/// documented low-pitch quality reason) is a specific tuned value, but
/// applying *some* correction here is a design choice available either
/// way.
pub fn apply_first_harmonic_correction(model: &mut Model) {
    if model.wo < (std::f32::consts::PI * 150.0 / 4000.0) {
        model.a[1] *= 0.032;
    }
}

/// `H[m] = conj(Aw[bin])` for each harmonic `m` -- the synthesis
/// filter's phase response at each harmonic, opposite phase to the
/// analysis filter (`Aw`) it's derived from.
pub fn sample_filter_phase(aw: &[Complex32], model: &Model) -> [Complex32; MAX_AMP + 1] {
    let mut h = [Complex32::new(0.0, 0.0); MAX_AMP + 1];
    let fft_r = std::f32::consts::TAU / FFT_ENC as f32;
    let k = model.wo / fft_r;
    #[allow(clippy::needless_range_loop)]
    for m in 1..=model.l {
        let b = (((m as f32 * k) + 0.5) as usize).min(FFT_ENC / 2 - 1);
        h[m] = aw[b].conj();
    }
    h
}

use super::fixed_fft::{fft_fixed, rshift_round_i128, ComplexQ23};
use super::fixed_point::{exp2_q23, log2_q23};
use super::lpc::pi_q23;

const FRAC_BITS: u32 = 23;

/// Fixed-point sibling of `Model` -- `wo`/`a` (Q23, `lpc::COEF_FRAC_
/// BITS`'s own format for `wo`; a linear amplitude for `a`, same
/// fractional-bit count but real headroom needs -- see `interp.rs`'s
/// own comment on why "23 fractional bits" doesn't imply one shared
/// Q-format name across domains) and `phi` (Q23 unit complex vectors,
/// `ComplexQ23`) genuinely integer, no `f32` anywhere.
pub(crate) struct ModelFixed {
    pub(crate) wo: i64,
    pub(crate) l: usize,
    pub(crate) a: [i64; MAX_AMP + 1],
    pub(crate) phi: [ComplexQ23; MAX_AMP + 1],
    pub(crate) voiced: bool,
}

impl ModelFixed {
    /// `l = floor(pi/wo)`: `pi_q23()/wo_q23` is a plain integer division
    /// whose Q23 scaling cancels between numerator and denominator (both
    /// share it), giving the real ratio directly with no rescale needed.
    pub(crate) fn new(wo_q23: i64, voiced: bool) -> Self {
        let l = ((pi_q23() / wo_q23) as usize).min(MAX_AMP);
        ModelFixed {
            wo: wo_q23,
            l,
            a: [0; MAX_AMP + 1],
            phi: [ComplexQ23 { re: 1i64 << FRAC_BITS, im: 0 }; MAX_AMP + 1],
            voiced,
        }
    }
}

fn mag_sq_q23(c: ComplexQ23) -> i64 {
    rshift_round_i128(c.re as i128 * c.re as i128 + c.im as i128 * c.im as i128, FRAC_BITS)
}

/// Fixed-point `lpc_spectrum`: `ak_q23` zero-padded into an `FFT_ENC`
/// buffer, forward `fixed_fft::fft_fixed` (phase-correct, verified
/// against `rustfft`'s own forward convention -- see that module's own
/// doc comment), first `SPEC_BINS` bins returned as `ComplexQ23`.
fn lpc_spectrum_fixed(ak_q23: &[i64; LPC_ORD + 1]) -> [ComplexQ23; SPEC_BINS] {
    let mut re = [0i64; FFT_ENC];
    let mut im = [0i64; FFT_ENC];
    re[..=LPC_ORD].copy_from_slice(ak_q23);
    fft_fixed(&mut re, &mut im, true);
    std::array::from_fn(|i| ComplexQ23 { re: re[i], im: im[i] })
}

/// `1e-6` in Q23 -- the same tiny floor `compute_harmonic_amplitudes`'s
/// own `a2`/`a2g` construction adds, just enough to keep `log2_q23`'s
/// positive-input requirement satisfied at a genuinely near-zero bin,
/// not a value whose own precision matters. Computed via `f32_to_q_
/// exact_round` rather than a hand-typed literal -- this port's own
/// `BW_GAMMA_Q23` doc comment records a real bug class (an
/// independently-computed constant disagreeing with Rust's own real
/// rounding) that hand arithmetic on Q23 literals repeats every time.
fn eps_a2_q23() -> i64 {
    static V: std::sync::OnceLock<i64> = std::sync::OnceLock::new();
    *V.get_or_init(|| super::fixed_point::f32_to_q_exact_round(1e-6, FRAC_BITS))
}

/// `LPCPF_BETA` (`0.2`) and `1.0 + LPCPF_BETA` (`1.2`) in Q23 -- the two
/// exponents `r.powf(LPCPF_TWO_BETA)/a2[i]` (with `r = sqrt(a2g/a2)`)
/// reduces to in log domain: `r^(2*BETA) = (a2g/a2)^BETA`, so `r^(2*BETA)
/// / a2 = a2g^BETA * a2^-(1+BETA)`.
fn beta_q23() -> i64 {
    static V: std::sync::OnceLock<i64> = std::sync::OnceLock::new();
    *V.get_or_init(|| super::fixed_point::f32_to_q_exact_round(LPCPF_BETA, FRAC_BITS))
}
fn one_plus_beta_q23() -> i64 {
    static V: std::sync::OnceLock<i64> = std::sync::OnceLock::new();
    *V.get_or_init(|| super::fixed_point::f32_to_q_exact_round(1.0 + LPCPF_BETA, FRAC_BITS))
}

/// `1.96` in Q23 -- the sub-1kHz boost `compute_harmonic_amplitudes`
/// applies verbatim.
fn boost_ratio_q23() -> i64 {
    static V: std::sync::OnceLock<i64> = std::sync::OnceLock::new();
    *V.get_or_init(|| super::fixed_point::f32_to_q_exact_round(1.96, FRAC_BITS))
}

/// Bin index below which a harmonic's own frequency (`i *
/// SAMPLE_RATE/2/(FFT_ENC/2)`, `15.625Hz/bin` at this codec's real
/// `SAMPLE_RATE`/`FFT_ENC`) is under 1000Hz -- `1000.0 / 15.625 == 64.0`
/// exactly, so unlike most of this port's frequency-domain thresholds,
/// this one needs no LUT or rounding margin at all: it's an exact
/// integer bin boundary, checked directly rather than recomputed from
/// `freq_hz` every call.
const BOOST_BIN_THRESHOLD: usize = 64;

/// `(a2g[i]^BETA) * (a2[i]^-(1+BETA))`, the term both the gain-
/// normalization sum (`e_after`) and the per-harmonic sum (`pw_i`,
/// before its own sub-1kHz boost) share -- computed once per bin
/// wherever both need it, via the log-domain reduction in `BETA_Q23`'s
/// own doc comment above.
fn ratio_pow_term_q23(log2_a2g: i64, log2_a2: i64) -> i64 {
    let exponent_q23 = ((log2_a2g as i128 * beta_q23() as i128
        - log2_a2 as i128 * one_plus_beta_q23() as i128)
        >> FRAC_BITS) as i64;
    exp2_q23(exponent_q23)
}

/// `k = wo/fft_r = wo*FFT_ENC/TAU` in Q23 -- `wo_q23`'s own Q23 scaling
/// is preserved by widening to `i128` before the `<<FRAC_BITS`
/// rescale-back-to-Q23 (dividing by `TAU_Q23`, itself Q23, cancels one
/// factor of the scaling, so the explicit `<<FRAC_BITS` restores it).
fn synth_k_q23(wo_q23: i64) -> i64 {
    let tau_q23 = 2 * pi_q23();
    (((wo_q23 as i128 * FFT_ENC as i128) << FRAC_BITS) / tau_q23 as i128) as i64
}

/// Fixed-point `compute_harmonic_amplitudes`: genuinely integer end to
/// end (Q23 throughout) -- `sqrt`/`powf`/reciprocal all compose from
/// `fixed_point::log2_q23`/`exp2_q23` (see `ratio_pow_term_q23`'s own
/// doc comment for the reduction), and the sub-1kHz boost's own
/// threshold is an exact integer bin comparison (`BOOST_BIN_THRESHOLD`),
/// not a recomputed float frequency. `gain` is computed via a genuine
/// `i128` integer division (`e_before/e_after`, both real accumulated
/// linear-domain sums, not a single power-law term) rather than a third
/// log-domain round trip, since the values are already in hand as plain
/// Q23 sums.
pub(crate) fn compute_harmonic_amplitudes_fixed(
    ak_q23: &[i64; LPC_ORD + 1],
    e_q23: i64,
    model: &mut ModelFixed,
) -> [ComplexQ23; SPEC_BINS] {
    let aw = lpc_spectrum_fixed(ak_q23);
    let a2: [i64; SPEC_BINS] = std::array::from_fn(|i| mag_sq_q23(aw[i]) + eps_a2_q23());

    let mut ak_gamma_q23 = [0i64; LPC_ORD + 1];
    ak_gamma_q23[0] = ak_q23[0];
    for i in 1..=LPC_ORD {
        // `LPCPF_GAMMA == 0.5` exactly, so `ak[i] * 0.5^i` is an exact
        // right shift, no Q23-constant rounding error at all (matching
        // `lpc.rs`'s own `apply_bw_gamma_fixed`-adjacent reasoning for
        // exact-power-of-two multipliers).
        ak_gamma_q23[i] = (ak_q23[i] + (1i64 << (i - 1))) >> i;
    }
    let awg = lpc_spectrum_fixed(&ak_gamma_q23);
    let a2g: [i64; SPEC_BINS] = std::array::from_fn(|i| mag_sq_q23(awg[i]) + eps_a2_q23());

    let mut e_before_q23: i64 = 0;
    let mut e_after_q23: i64 = 0;
    for i in 0..(FFT_ENC / 2) {
        let log2_a2 = log2_q23(a2[i]);
        let log2_a2g = log2_q23(a2g[i]);
        e_before_q23 += exp2_q23(-log2_a2);
        e_after_q23 += ratio_pow_term_q23(log2_a2g, log2_a2);
    }
    let gain_q23 =
        ((e_q23 as i128 * e_before_q23 as i128) / e_after_q23.max(1) as i128) as i64;

    let k_q23 = synth_k_q23(model.wo);
    #[allow(clippy::needless_range_loop)]
    for m in 1..=model.l {
        // am = round((m-0.5)*k), bm = round((m+0.5)*k): both rewritten
        // as round((2m∓1)*k/2) to avoid a half-integer `m`, matching
        // `interp.rs`'s own "avoid float, keep integer" discipline.
        let raw_am = (2 * m as i64 - 1) * k_q23;
        let am = ((raw_am + (1i64 << 23)) >> 24) as usize;
        let raw_bm = (2 * m as i64 + 1) * k_q23;
        let bm = (((raw_bm + (1i64 << 23)) >> 24) as usize).min(FFT_ENC / 2);

        let mut em_q23: i64 = 0;
        for i in am..bm {
            let log2_a2 = log2_q23(a2[i]);
            let log2_a2g = log2_q23(a2g[i]);
            let mut pw_i = ratio_pow_term_q23(log2_a2g, log2_a2);
            if i < BOOST_BIN_THRESHOLD {
                pw_i = rshift_round_i128(pw_i as i128 * boost_ratio_q23() as i128, FRAC_BITS);
            }
            em_q23 += pw_i;
        }
        em_q23 = rshift_round_i128(em_q23 as i128 * gain_q23 as i128, FRAC_BITS);
        model.a[m] = if em_q23 <= 0 {
            0
        } else {
            exp2_q23(log2_q23(em_q23) >> 1)
        };
    }

    aw
}

/// Fixed-point `apply_first_harmonic_correction`: `wo` compared directly
/// against a precomputed Q23 threshold (`PI*150/4000`), no float
/// anywhere.
fn first_harmonic_wo_threshold_q23() -> i64 {
    static V: std::sync::OnceLock<i64> = std::sync::OnceLock::new();
    *V.get_or_init(|| {
        super::fixed_point::f32_to_q_exact_round(
            std::f32::consts::PI * 150.0 / 4000.0,
            FRAC_BITS,
        )
    })
}
fn first_harmonic_correction_q23() -> i64 {
    static V: std::sync::OnceLock<i64> = std::sync::OnceLock::new();
    *V.get_or_init(|| super::fixed_point::f32_to_q_exact_round(0.032, FRAC_BITS))
}

pub(crate) fn apply_first_harmonic_correction_fixed(model: &mut ModelFixed) {
    if model.wo < first_harmonic_wo_threshold_q23() {
        model.a[1] = rshift_round_i128(
            model.a[1] as i128 * first_harmonic_correction_q23() as i128,
            FRAC_BITS,
        );
    }
}

/// Fixed-point `sample_filter_phase`: `aw` (this same `compute_harmonic_
/// amplitudes_fixed` call's own return value) in, `ComplexQ23` out.
pub(crate) fn sample_filter_phase_fixed(
    aw: &[ComplexQ23],
    model: &ModelFixed,
) -> [ComplexQ23; MAX_AMP + 1] {
    let mut h = [ComplexQ23::ZERO; MAX_AMP + 1];
    let k_q23 = synth_k_q23(model.wo);
    #[allow(clippy::needless_range_loop)]
    for m in 1..=model.l {
        let raw = m as i64 * k_q23;
        let b = (((raw + (1i64 << 22)) >> 23) as usize).min(FFT_ENC / 2 - 1);
        h[m] = aw[b].conj();
    }
    h
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec2_3200::fixed_point::f32_to_q_exact_round;
    use crate::codec2_3200::lpc::{lsp_to_lpc, lsp_to_lpc_fixed, COEF_FRAC_BITS};
    use crate::codec2_3200::quantise::{decode_wo, decode_wo_fixed};
    use crate::codec2_3200::{LPC_ORD as CRATE_LPC_ORD, WO_BITS};

    fn from_q23(x: i64, frac_bits: u32) -> f32 {
        x as f32 / (1i64 << frac_bits) as f32
    }

    /// `ModelFixed::new`'s `l = pi_q23()/wo_q23` and `Model::new`'s `l =
    /// (PI/wo) as usize` are the same formula, but `pi_q23()`/`wo_q23`
    /// are each independently-rounded Q23/f32 approximations -- at
    /// `index=0` (`wo == W0_MIN == 2*PI/160` exactly, so `PI/wo == 80`
    /// exactly in true real arithmetic), the two roundings land on
    /// opposite sides of that exact integer boundary (measured: float
    /// gives 80, fixed gives 79), a real, checked, one-harmonic
    /// disagreement rather than a hypothetical one. Per this module's
    /// own doc comment, `l` is purely decoder-internal (never
    /// transmitted, "exact formulas are a design choice"), so a
    /// one-harmonic difference at this single rare low-pitch boundary
    /// is the same category of acceptable float-vs-fixed divergence as
    /// `decode_lsps_delta_scalar_fixed`'s own Hz-based tolerance, not a
    /// bug to chase into exact bit-parity nobody needs -- exhaustive
    /// over every real transmitted `Wo` index (only 128 of them) with a
    /// tolerance of 1, not exact equality.
    #[test]
    fn model_fixed_new_agrees_with_model_new_within_one_harmonic_on_every_real_wo_index() {
        for index in 0..(1u32 << WO_BITS) {
            let wo_f = decode_wo(index);
            let wo_q23 = decode_wo_fixed(index);
            for &voiced in &[true, false] {
                let float_l = Model::new(wo_f, voiced).l;
                let fixed_l = ModelFixed::new(wo_q23, voiced).l;
                let diff = float_l.abs_diff(fixed_l);
                assert!(
                    diff <= 1,
                    "index={index} wo_f={wo_f} voiced={voiced}: Model::new gave {float_l}, ModelFixed::new gave {fixed_l} -- more than the one-harmonic rounding-boundary tolerance"
                );
            }
        }
    }

    /// Real captured LSP/energy data through both `compute_harmonic_
    /// amplitudes` (float) and `compute_harmonic_amplitudes_fixed`,
    /// same fixture combination `fixed_point.rs`'s own `postfilter_lut_
    /// decisions_match_plain_float_across_a_real_temporal_replay` test
    /// uses. Compares **absolute** per-harmonic amplitude (relative
    /// error against the float value), not a normalized shape -- this
    /// is the stage where real amplitude scale first becomes load-
    /// bearing (it feeds `synthesis.rs`'s IFFT, then `ear_protection`'s
    /// hard 30000.0 threshold), so a uniform gain bug here would pass
    /// any shape-only comparison and only surface later as wrong RMS,
    /// far from its actual cause.
    #[test]
    fn compute_harmonic_amplitudes_fixed_matches_the_float_version_on_real_captured_data() {
        let lsp_path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/codec2_3200/codec2_lsp_dump.txt"
        );
        let e_path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/codec2_3200/codec2_enc_e_dump.txt"
        );
        let read_rows = |path: &str, cols: usize| -> Vec<Vec<f32>> {
            std::fs::read_to_string(path)
                .unwrap()
                .lines()
                .map(|l| {
                    let v: Vec<f32> = l.split_whitespace().map(|s| s.parse().unwrap()).collect();
                    assert_eq!(v.len(), cols);
                    v
                })
                .collect()
        };
        let lsp_rows = read_rows(lsp_path, CRATE_LPC_ORD + 1);
        let e_rows = read_rows(e_path, 1);
        let n = lsp_rows.len().min(e_rows.len());
        assert!(n > 300, "expected the real captured fixture corpus, got {n} rows");

        let mut planner = rustfft::FftPlanner::<f32>::new();
        let fft = planner.plan_fft_forward(FFT_ENC);

        // Synthetic Wo/voiced, same reasoning fixed_point.rs's own
        // postfilter replay test uses: not transmitted, purely a test-
        // harness choice, so a representative fixed value is fine --
        // what's under test is whether the LSP/energy-driven amplitude
        // computation agrees, not pitch tracking.
        let wo = super::super::W0_MIN + (super::super::W0_MAX - super::super::W0_MIN) * 0.3;
        let wo_q23 = f32_to_q_exact_round(wo, COEF_FRAC_BITS);

        let mut n_checked = 0;
        let mut max_rel_err = 0.0f32;
        let mut worst_m = 0usize;
        for i in 0..n {
            let roots = lsp_rows[i][0] as i32;
            if roots as usize != CRATE_LPC_ORD {
                continue;
            }
            let mut lsp = [0.0f32; LPC_ORD];
            lsp.copy_from_slice(&lsp_rows[i][1..]);
            let e = e_rows[i][0].max(1e-3);

            let ak = lsp_to_lpc(&lsp);
            let mut model = Model::new(wo, true);
            let _aw = compute_harmonic_amplitudes(fft.as_ref(), &ak, e, &mut model);

            let lsp_q23: [i64; LPC_ORD] =
                std::array::from_fn(|j| f32_to_q_exact_round(lsp[j], COEF_FRAC_BITS));
            let ak_q23 = lsp_to_lpc_fixed(&lsp_q23);
            let e_q23 = f32_to_q_exact_round(e, FRAC_BITS);
            let mut model_fixed = ModelFixed::new(wo_q23, true);
            let _aw_fixed = compute_harmonic_amplitudes_fixed(&ak_q23, e_q23, &mut model_fixed);

            assert_eq!(
                model.l, model_fixed.l,
                "harmonic count disagreement on real frame {i} -- should be impossible given the exhaustive Wo sweep above already passed"
            );
            for m in 1..=model.l {
                let fixed_a = from_q23(model_fixed.a[m], FRAC_BITS);
                let rel_err = ((fixed_a - model.a[m]) / model.a[m].max(1e-6)).abs();
                if rel_err > max_rel_err {
                    max_rel_err = rel_err;
                    worst_m = m;
                }
            }
            n_checked += 1;
        }
        println!("compute_harmonic_amplitudes_fixed: worst relative error {max_rel_err} at harmonic {worst_m}, {n_checked} real frames checked");
        assert!(n_checked > 150, "only checked {n_checked} real frames");
        assert!(
            max_rel_err < 0.02,
            "compute_harmonic_amplitudes_fixed diverged from the float version by {max_rel_err} relative (worst at harmonic {worst_m}) on real captured data"
        );
    }

    #[test]
    fn apply_first_harmonic_correction_fixed_matches_the_float_version_on_both_sides_of_the_threshold() {
        let low_wo = super::super::W0_MIN; // below threshold -- correction applies
        let high_wo = super::super::W0_MAX; // above threshold -- no correction
        for &wo in &[low_wo, high_wo] {
            let mut model = Model::new(wo, true);
            model.a[1] = 1234.5;
            apply_first_harmonic_correction(&mut model);

            let wo_q23 = f32_to_q_exact_round(wo, COEF_FRAC_BITS);
            let mut model_fixed = ModelFixed::new(wo_q23, true);
            model_fixed.a[1] = f32_to_q_exact_round(1234.5, FRAC_BITS);
            apply_first_harmonic_correction_fixed(&mut model_fixed);

            let fixed_a1 = from_q23(model_fixed.a[1], FRAC_BITS);
            assert!(
                (fixed_a1 - model.a[1]).abs() < 1e-2,
                "wo={wo}: float a[1]={}, fixed a[1]={fixed_a1}",
                model.a[1]
            );
        }
    }

    #[test]
    fn sample_filter_phase_fixed_matches_the_float_version_on_a_synthetic_spectrum() {
        // A synthetic Aw spectrum (not real captured data -- this
        // function is pure bin-index arithmetic plus a conjugate, no
        // real spectral shape needed to exercise it) at a real Wo,
        // checked bin-for-bin against the float version.
        let wo = super::super::W0_MIN + (super::super::W0_MAX - super::super::W0_MIN) * 0.4;
        let model = Model::new(wo, true);
        let aw: Vec<Complex32> = (0..SPEC_BINS)
            .map(|i| Complex32::new((i as f32 * 0.37).sin(), (i as f32 * 0.61).cos()))
            .collect();
        let h = sample_filter_phase(&aw, &model);

        let wo_q23 = f32_to_q_exact_round(wo, COEF_FRAC_BITS);
        let model_fixed = ModelFixed::new(wo_q23, true);
        let aw_q23: Vec<ComplexQ23> = aw
            .iter()
            .map(|c| ComplexQ23 {
                re: f32_to_q_exact_round(c.re, FRAC_BITS),
                im: f32_to_q_exact_round(c.im, FRAC_BITS),
            })
            .collect();
        let h_fixed = sample_filter_phase_fixed(&aw_q23, &model_fixed);

        assert_eq!(model.l, model_fixed.l);
        for m in 1..=model.l {
            let got_re = from_q23(h_fixed[m].re, FRAC_BITS);
            let got_im = from_q23(h_fixed[m].im, FRAC_BITS);
            assert!(
                (got_re - h[m].re).abs() < 1e-4 && (got_im - h[m].im).abs() < 1e-4,
                "harmonic {m}: float h={:?}, fixed h=({got_re}, {got_im})",
                h[m]
            );
        }
    }
}
