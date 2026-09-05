// SPDX-License-Identifier: LGPL-3.0-or-later
//! Fixed-point non-linear pitch (NLP) estimation -- `EncoderFixed`'s own
//! real production path (`nlp_fixed`, including its own fixed-point
//! radix-2 FFT). The original `f32` version (`nlp`/`NlpState`) moved to
//! `floating_reference::nlp` once this module's own `nlp_fixed` became
//! the only real caller; see that module's own doc comment for the
//! algorithm's own rationale (squares the signal, DC-notches, low-pass
//! decimates, windows, FFTs, then a peak search with sub-multiple
//! correction), still accurate here. This module keeps, and exports via
//! `pub(crate)`, the pieces `floating_reference::nlp`'s own float
//! `decimate`/`dc_notch`/`correct_sub_multiples` still need
//! (`lowpass_coeffs`/`LPF_TAPS`/`NOTCH_A`/`CNLP`/`NDEC`) -- the reverse
//! of the usual direction, because this fixed module's own table
//! builders (`lowpass_coeffs_q23`, `notch_a_q23`, `cnlp_q23`) read them
//! directly, so they can't move out from under those builders. `f0_to_wo`
//! also stays here, shared unchanged by both encoders.

use super::{M_PITCH, NLP_DEC, N_SAMP, PE_FFT_SIZE, P_MAX, P_MIN, SAMPLE_RATE};

/// Decimated buffer length: the full `M_PITCH`-sample pitch-history
/// window, downsampled by `NLP_DEC`. `pub(crate)`: `floating_reference::
/// nlp`'s own `decimate`/`nlp` need this too.
pub(crate) const NDEC: usize = M_PITCH / NLP_DEC;

/// First-order DC-blocking filter: `y[n] = x[n] - x[n-1] + a*y[n-1]`, a
/// standard high-pass structure with its pole at `a` (closer to 1.0 means
/// a lower corner frequency). `0.95` at 8kHz puts the corner around
/// 65Hz, comfortably below `P_MAX`'s own ~50Hz lower pitch bound and high
/// enough to reject squared-signal DC bias -- an independently chosen
/// value, not read from the reference. `pub(crate)`: `floating_reference
/// ::nlp`'s own `dc_notch` needs this too.
pub(crate) const NOTCH_A: f32 = 0.95;

/// Anti-alias low-pass filter length (odd, symmetric) applied before
/// decimating by `NLP_DEC`. `pub(crate)`: `floating_reference::nlp`'s
/// own `decimate` needs this too.
pub(crate) const LPF_TAPS: usize = 25;

/// Windowed-sinc low-pass FIR, unity DC gain, cutoff at `cutoff` cycles
/// per original-rate sample (`0.5 / NLP_DEC` for anti-aliasing ahead of
/// decimation by `NLP_DEC`).
fn design_lowpass(taps: usize, cutoff: f32) -> [f32; LPF_TAPS] {
    let mut h = [0.0f32; LPF_TAPS];
    let center = (taps - 1) as f32 / 2.0;
    let mut sum = 0.0f32;
    for (i, hi) in h.iter_mut().enumerate() {
        let x = i as f32 - center;
        let sinc = if x.abs() < 1e-8 {
            2.0 * cutoff
        } else {
            (2.0 * std::f32::consts::PI * cutoff * x).sin() / (std::f32::consts::PI * x)
        };
        let hann = 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / (taps - 1) as f32).cos();
        *hi = sinc * hann;
        sum += *hi;
    }
    for hi in h.iter_mut() {
        *hi /= sum;
    }
    h
}

/// `pub(crate)`: `floating_reference::nlp`'s own `decimate` needs this
/// too (its float twin of `decimate_fixed` below).
pub(crate) fn lowpass_coeffs() -> &'static [f32; LPF_TAPS] {
    use std::sync::OnceLock;
    static COEFFS: OnceLock<[f32; LPF_TAPS]> = OnceLock::new();
    COEFFS.get_or_init(|| design_lowpass(LPF_TAPS, 0.5 / NLP_DEC as f32))
}

/// Fraction of the global peak magnitude a sub-multiple candidate must
/// clear to be preferred over the raw global peak -- lowered by half
/// when that candidate is close to the previous frame's own estimate
/// (a simple form of pitch tracking/continuity bias). `pub(crate)`:
/// `floating_reference::nlp`'s own `correct_sub_multiples` needs this
/// too.
pub(crate) const CNLP: f32 = 0.3;

/// `f0` (Hz) -> `Wo` (normalized angular pitch frequency), the form
/// `quantise::encode_wo` expects. Shared unchanged by both encoders.
pub fn f0_to_wo(f0: f32) -> f32 {
    std::f32::consts::TAU * f0 / SAMPLE_RATE as f32
}

// ---------------------------------------------------------------------
// Fixed-point pitch estimator.
//
// Same algorithmic shape as `floating_reference::nlp::nlp` -- DC-notch,
// low-pass decimate, Hann window, FFT, peak search, sub-multiple
// correction -- computed entirely in scaled integer arithmetic. The key
// property that makes this tractable: `power[]` is used only for
// *ordinal* comparisons (a bounded-range peak search, and `lmax` vs
// `CNLP*gmax`/neighbor-bin threshold tests inside `correct_sub_
// multiples`) -- no absolute magnitude ever escapes this module except
// the one scalar `f0` (Hz) returned at the very end. That means any
// integer scaling applied uniformly to every bin (as every stage below
// is) is free: it cancels in every comparison. The real constraint is
// therefore keeping competing peaks distinguishable at realistic signal
// ratios, not tracking absolute magnitude to some fixed precision
// target -- and `i64`/`i128` throughout gives far more headroom than
// that needs (a direct measurement, on a wide synthetic
// f0/harmonic/noise sweep, of every comparison this module makes found
// the tightest realistic margins around -20dB to -30dB; `i64`/`i128`
// preserve well over 60 bits of dynamic range end to end, so no
// block-floating-point rescaling is needed to stay well clear of that
// margin).
//
// That headroom claim is real, but it's a *per-stage* one, not a
// license to narrow freely: the front end (DC-notch, decimate, window)
// and the FFT butterflies each stay bounded well under `i64::MAX` given
// this module's real signal magnitudes, so `rshift_round_i128`'s
// narrowing back to `i64` is safe there. `power[]` and `gmax`
// themselves are a different story -- they reach real magnitudes up to
// ~1e24 at full-scale `i16` amplitude, and anything derived from them by
// multiplying by a Q23 constant (`correct_sub_multiples_fixed`'s own
// `CNLP * gmax` threshold, specifically) can exceed `i64::MAX` on its
// own. That case genuinely needs `rshift_round_i128_wide` (full `i128`
// result, no narrowing) -- an earlier version of this module used the
// narrowing shift there instead and silently wrapped at real speech
// amplitude, caught only by a dedicated full-scale test
// (`nlp_fixed_agrees_with_the_float_reference_at_full_scale_amplitude`),
// not by anything at this file's own more modest `REALISTIC_AMP` test
// scale. Read `rshift_round_i128`'s own doc comment before adding a new
// caller to either shift helper.
//
// Twiddle-factor sign convention (forward vs inverse DFT) doesn't
// matter: the FFT input here is always real (imaginary part 0), so the
// output is Hermitian-symmetric regardless of sign convention, and only
// the magnitude spectrum is ever used.
const NLP_FRAC_BITS: u32 = 23;

/// One-time float->Q23 conversion for table/constant construction (not a
/// per-sample conversion) -- same convention `lpc.rs`'s own
/// `acos_lut_table_q23`/`f32_to_q` use for building fixed tables from a
/// float formula.
fn f32_to_q23(x: f32) -> i64 {
    (x as f64 * (1i64 << NLP_FRAC_BITS) as f64).round() as i64
}

/// Round-to-nearest right shift (matches `lpc.rs`'s own `rshift_round`).
fn rshift_round(x: i64, n: u32) -> i64 {
    (x + (1i64 << (n - 1))) >> n
}

/// Same rounding shift, for an `i128` accumulator too wide for `i64` --
/// narrowing to `i64` at the end, so only for a call site that has
/// already confirmed its own result fits (`decimate_fixed`'s FIR
/// accumulator, `fft_fixed`'s butterfly products -- both bounded well
/// under `i64::MAX` by construction, so the `debug_assert` below
/// documents that invariant rather than guarding a live risk at either
/// of today's two call sites). It exists to catch the *next* caller
/// that doesn't hold the same bound: an earlier version of
/// `correct_sub_multiples_fixed` used this function for its own `CNLP *
/// gmax` threshold and silently wrapped, because `power[]`/`gmax` reach
/// real magnitudes -- up to ~1e24 at full-scale `i16` amplitude -- large
/// enough that a Q23-scaled product of `CNLP` and `gmax` exceeds
/// `i64::MAX` well before this shift even runs (see
/// `rshift_round_i128_wide` below, used there instead now, and
/// `nlp_fixed_agrees_with_the_float_reference_at_full_scale_amplitude`,
/// which regresses on this exact bug if the narrowing version is used
/// there again).
fn rshift_round_i128(x: i128, n: u32) -> i64 {
    let shifted = (x + (1i128 << (n - 1))) >> n;
    debug_assert!(
        shifted >= i64::MIN as i128 && shifted <= i64::MAX as i128,
        "rshift_round_i128: result {shifted} doesn't fit i64 -- this call site needs \
         rshift_round_i128_wide instead"
    );
    shifted as i64
}

/// Same rounding shift, but keeps the full `i128` width -- for a call
/// site (like `correct_sub_multiples_fixed`'s own `CNLP * gmax`
/// threshold) whose result can itself exceed `i64::MAX` at real,
/// full-scale signal amplitudes, unlike `rshift_round_i128`'s other
/// callers.
fn rshift_round_i128_wide(x: i128, n: u32) -> i128 {
    (x + (1i128 << (n - 1))) >> n
}

fn notch_a_q23() -> i64 {
    static V: std::sync::OnceLock<i64> = std::sync::OnceLock::new();
    *V.get_or_init(|| f32_to_q23(NOTCH_A))
}

fn cnlp_q23() -> i64 {
    static V: std::sync::OnceLock<i64> = std::sync::OnceLock::new();
    *V.get_or_init(|| f32_to_q23(CNLP))
}

fn lowpass_coeffs_q23() -> &'static [i64; LPF_TAPS] {
    static TABLE: std::sync::OnceLock<[i64; LPF_TAPS]> = std::sync::OnceLock::new();
    TABLE.get_or_init(|| {
        let h = design_lowpass(LPF_TAPS, 0.5 / NLP_DEC as f32);
        std::array::from_fn(|i| f32_to_q23(h[i]))
    })
}

fn hann_window_q23() -> &'static [i64; NDEC] {
    static TABLE: std::sync::OnceLock<[i64; NDEC]> = std::sync::OnceLock::new();
    TABLE.get_or_init(|| {
        std::array::from_fn(|i| {
            let hann = 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / (NDEC - 1) as f32).cos();
            f32_to_q23(hann)
        })
    })
}

/// `(cos(2*pi*k/PE_FFT_SIZE), sin(2*pi*k/PE_FFT_SIZE))` for `k` in
/// `0..PE_FFT_SIZE/2` -- the only twiddle angles a radix-2 FFT of this
/// size ever needs (`j*step` below always lands in this range).
fn fft_twiddles_q23() -> &'static [(i64, i64); PE_FFT_SIZE / 2] {
    static TABLE: std::sync::OnceLock<[(i64, i64); PE_FFT_SIZE / 2]> = std::sync::OnceLock::new();
    TABLE.get_or_init(|| {
        std::array::from_fn(|k| {
            let theta = std::f32::consts::TAU * k as f32 / PE_FFT_SIZE as f32;
            (f32_to_q23(theta.cos()), f32_to_q23(theta.sin()))
        })
    })
}

fn fft_bit_reverse_table() -> &'static [usize; PE_FFT_SIZE] {
    static TABLE: std::sync::OnceLock<[usize; PE_FFT_SIZE]> = std::sync::OnceLock::new();
    TABLE.get_or_init(|| {
        let bits = PE_FFT_SIZE.trailing_zeros();
        std::array::from_fn(|i| ((i as u32).reverse_bits() >> (32 - bits)) as usize)
    })
}

/// Persistent per-encoder pitch-estimator state -- `EncoderFixed`'s own
/// production path. `floating_reference::nlp::NlpState` is the
/// independent float-only twin (no shared fields; this struct needs no
/// FFT planner at all, since `fft_fixed` below is a from-scratch radix-2
/// implementation, not `rustfft`).
pub struct NlpStateFixed {
    /// Squared, DC-notched signal history, one entry per original-rate
    /// sample across the `M_PITCH`-sample analysis window -- plain
    /// integer domain, no fractional scaling (see `dc_notch_fixed`'s own
    /// doc comment).
    sq_fixed: [i64; M_PITCH],
    /// DC-notch filter memory (previous input, previous output).
    mem_x_fixed: i64,
    mem_y_fixed: i64,
    /// Previous frame's estimated fundamental, stored directly as a bin
    /// index (the only form `correct_sub_multiples_fixed` needs it in)
    /// rather than re-deriving it from Hz every call.
    prev_f0_bin_fixed: usize,
}

impl Default for NlpStateFixed {
    fn default() -> Self {
        NlpStateFixed {
            sq_fixed: [0; M_PITCH],
            mem_x_fixed: 0,
            mem_y_fixed: 0,
            // Same 100Hz initial guess `floating_reference::nlp::
            // NlpState`'s own `prev_f0` uses, converted to this state's
            // own bin-index form via the identical formula `nlp_fixed`
            // itself uses (`prev_f0 / bin_to_hz`) -- keeps the two
            // implementations' first-frame continuity bias aligned
            // instead of silently starting from bin 0.
            prev_f0_bin_fixed: (100.0
                / (SAMPLE_RATE as f32 / (PE_FFT_SIZE * NLP_DEC) as f32))
                as usize,
        }
    }
}

impl NlpStateFixed {
    pub fn new() -> Self {
        Self::default()
    }
}

/// Fixed-point twin of `floating_reference::nlp::dc_notch` -- same
/// first-order structure, `x`/`mem_x`/`mem_y`/the return value all share
/// one plain integer domain (squared-sample magnitude, no fractional
/// scaling); only `NOTCH_A` itself is a Q23 dimensionless gain, descaled
/// back out after the multiply. `a * *mem_y` fits `i64` directly without
/// widening: `a` is bounded by `2^23` and `mem_y` by a real squared
/// `i16` sample's own range (up to ~1.07e9), so the product is
/// comfortably under `i64::MAX`.
fn dc_notch_fixed(x: i64, mem_x: &mut i64, mem_y: &mut i64) -> i64 {
    let a = notch_a_q23();
    let ay = rshift_round(a * *mem_y, NLP_FRAC_BITS);
    let y = x - *mem_x + ay;
    *mem_x = x;
    *mem_y = y;
    y
}

/// Fixed-point twin of `floating_reference::nlp::decimate` -- same
/// windowed-sinc FIR, coefficients quantized Q23, accumulated in `i128`
/// (safe headroom: worst case `LPF_TAPS` terms of `2^23 * ~1.07e9` each
/// is still two orders of magnitude under `i64::MAX`, but `i128` costs
/// nothing here and matches this crate's own established
/// multiply-accumulate convention).
fn decimate_fixed(sq: &[i64; M_PITCH]) -> [i64; NDEC] {
    let h = lowpass_coeffs_q23();
    let half = (LPF_TAPS as isize - 1) / 2;
    let mut out = [0i64; NDEC];
    for (k, out_k) in out.iter_mut().enumerate() {
        let center = (k * NLP_DEC) as isize;
        let mut acc: i128 = 0;
        for (t, &coeff) in h.iter().enumerate() {
            let idx = center + t as isize - half;
            let idx = idx.clamp(0, M_PITCH as isize - 1) as usize;
            acc += coeff as i128 * sq[idx] as i128;
        }
        *out_k = rshift_round_i128(acc, NLP_FRAC_BITS);
    }
    out
}

/// In-place iterative radix-2 decimation-in-time FFT over `PE_FFT_SIZE`
/// integer samples. No per-stage rescaling: worst-case bin magnitude
/// growth across all `log2(PE_FFT_SIZE)` stages is bounded by
/// `PE_FFT_SIZE` itself (Parseval's theorem's own energy-conservation
/// bound), so an input bounded by ~1.07e9 (a real squared `i16` sample,
/// windowed) stays orders of magnitude under `i64::MAX` throughout --
/// the block-floating-point rescaling a genuinely bit-width-constrained
/// (16- or 32-bit) fixed-point FFT would need is simply unnecessary at
/// `i64`/`i128` width, and skipping it avoids the extra rounding error
/// per-stage rescaling would otherwise cost.
fn fft_fixed(re: &mut [i64; PE_FFT_SIZE], im: &mut [i64; PE_FFT_SIZE]) {
    let bitrev = fft_bit_reverse_table();
    for (i, &j) in bitrev.iter().enumerate() {
        if j > i {
            re.swap(i, j);
            im.swap(i, j);
        }
    }

    let twiddles = fft_twiddles_q23();
    let mut len = 2usize;
    while len <= PE_FFT_SIZE {
        let half = len / 2;
        let step = PE_FFT_SIZE / len;
        let mut i = 0;
        while i < PE_FFT_SIZE {
            for j in 0..half {
                let (wr, wi) = twiddles[j * step];
                let br = re[i + j + half];
                let bi = im[i + j + half];
                let vr = rshift_round_i128(
                    wr as i128 * br as i128 - wi as i128 * bi as i128,
                    NLP_FRAC_BITS,
                );
                let vi = rshift_round_i128(
                    wr as i128 * bi as i128 + wi as i128 * br as i128,
                    NLP_FRAC_BITS,
                );
                let ar = re[i + j];
                let ai = im[i + j];
                re[i + j] = ar + vr;
                im[i + j] = ai + vi;
                re[i + j + half] = ar - vr;
                im[i + j + half] = ai - vi;
            }
            i += len;
        }
        len *= 2;
    }
}

/// Fixed-point twin of `floating_reference::nlp::correct_sub_multiples`,
/// operating on the `i128` power spectrum directly -- same comparisons,
/// same structure. `0.8`/`1.2` become exact integer ratios (`8/10`,
/// `12/10`): for the small bin indices this ever runs on (`b` is at most
/// `hi/2`, itself at most `PE_FFT_SIZE * NLP_DEC / P_MIN`), truncating
/// integer division and the float version's own `(0.8 * b as f32) as
/// usize` truncation agree exactly (checked directly across the real
/// range, see
/// `sub_multiple_bounds_match_the_float_formula_across_the_real_bin_range`).
fn correct_sub_multiples_fixed(
    power: &[i128],
    gmax: i128,
    gmax_bin: usize,
    prev_f0_bin: usize,
    min_bin: usize,
) -> usize {
    let mut cmax_bin = gmax_bin;
    let mut mult = 2usize;
    let cnlp = cnlp_q23();
    while gmax_bin / mult >= min_bin {
        let b = gmax_bin / mult;
        let bmin = ((8 * b) / 10).max(min_bin);
        let bmax = ((12 * b) / 10).min(power.len() - 1);

        // `gmax` reaches real magnitudes (~1e24 at full-scale `i16`
        // amplitude) large enough that `CNLP * gmax` itself can exceed
        // `i64::MAX` well before any shift -- stays `i128` end to end,
        // unlike `decimate_fixed`/`fft_fixed`'s own accumulators, which
        // are bounded well under `i64::MAX` by construction.
        let base_thresh = rshift_round_i128_wide(cnlp as i128 * gmax, NLP_FRAC_BITS);
        let thresh = if prev_f0_bin > bmin && prev_f0_bin < bmax {
            base_thresh / 2
        } else {
            base_thresh
        };

        let mut lmax: i128 = 0;
        let mut lmax_bin = bmin;
        for (bin, &p) in power.iter().enumerate().take(bmax + 1).skip(bmin) {
            if p > lmax {
                lmax = p;
                lmax_bin = bin;
            }
        }

        if lmax > thresh
            && lmax_bin > 0
            && lmax_bin < power.len() - 1
            && lmax > power[lmax_bin - 1]
            && lmax > power[lmax_bin + 1]
        {
            cmax_bin = lmax_bin;
        }
        mult += 1;
    }
    cmax_bin
}

/// Fixed-point twin of `floating_reference::nlp::nlp`. `sn` is this
/// crate's own real `i16`-native sample history (`EncoderFixed`'s own
/// `sn` field) -- no `f32` conversion anywhere in this call. Returns
/// `f0` (Hz) as `f32` only at this one final boundary (the "integer
/// core, float boundary" pattern already established throughout this
/// crate's fixed-point migration), so `f0_to_wo`/`quantise::encode_wo`
/// downstream need no changes at all.
pub fn nlp_fixed(state: &mut NlpStateFixed, sn: &[i16; M_PITCH]) -> f32 {
    let start = M_PITCH - N_SAMP;
    for (sn_i, sq_i) in sn[start..].iter().zip(state.sq_fixed[start..].iter_mut()) {
        let s = *sn_i as i64;
        let squared = s * s;
        let notch = dc_notch_fixed(squared, &mut state.mem_x_fixed, &mut state.mem_y_fixed);
        *sq_i = notch + 1;
    }

    let decimated = decimate_fixed(&state.sq_fixed);

    state.sq_fixed.copy_within(N_SAMP..M_PITCH, 0);

    let mut re = [0i64; PE_FFT_SIZE];
    let mut im = [0i64; PE_FFT_SIZE];
    let hann = hann_window_q23();
    for (i, &d) in decimated.iter().enumerate() {
        re[i] = rshift_round(d * hann[i], NLP_FRAC_BITS);
    }

    fft_fixed(&mut re, &mut im);

    const HALF: usize = PE_FFT_SIZE / 2 + 1;
    let power: [i128; HALF] = std::array::from_fn(|i| {
        re[i] as i128 * re[i] as i128 + im[i] as i128 * im[i] as i128
    });

    let bin_to_hz = SAMPLE_RATE as f32 / (PE_FFT_SIZE * NLP_DEC) as f32;
    let lo = (PE_FFT_SIZE * NLP_DEC / P_MAX).max(1);
    let hi = (PE_FFT_SIZE * NLP_DEC / P_MIN).min(HALF - 1);

    let mut gmax: i128 = 0;
    let mut gmax_bin = lo;
    for (bin, &p) in power.iter().enumerate().take(hi + 1).skip(lo) {
        if p > gmax {
            gmax = p;
            gmax_bin = bin;
        }
    }

    let prev_f0_bin = state.prev_f0_bin_fixed;
    let cmax_bin = correct_sub_multiples_fixed(&power, gmax, gmax_bin, prev_f0_bin, lo);

    let f0 = cmax_bin as f32 * bin_to_hz;
    state.prev_f0_bin_fixed = cmax_bin;
    f0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec2_3200::floating_reference::nlp::tests::{
        estimate_synthetic_pitch, estimate_synthetic_pitch_at_amp, REALISTIC_AMP,
    };
    use crate::codec2_3200::floating_reference::nlp::{correct_sub_multiples, dc_notch, decimate, nlp, NlpState};
    use crate::codec2_3200::{W0_MAX, W0_MIN};
    use rustfft::num_complex::Complex32;
    use rustfft::FftPlanner;

    /// Same synthesis as `floating_reference::nlp::tests::
    /// estimate_synthetic_pitch`, but through `nlp_fixed` on real
    /// `i16`-native samples (clamped/rounded from the same float
    /// synthesis, matching how `EncoderFixed` actually receives speech)
    /// instead of `nlp`'s own `f32` path.
    fn estimate_synthetic_pitch_fixed(f0_hz: f32, harmonic_amps: &[f32]) -> f32 {
        estimate_synthetic_pitch_fixed_at_amp(f0_hz, harmonic_amps, REALISTIC_AMP)
    }

    /// `amp_scale` lets callers push the synthesized signal toward
    /// full-scale `i16` amplitude -- real headroom to exercise, since
    /// `power[]`/`gmax` grow with the *fourth* power of sample amplitude
    /// (squared once for `sq[]`, squared again for FFT power), so a
    /// margin measured at a modest test amplitude does not by itself
    /// establish there's no overflow at real full-scale speech.
    fn estimate_synthetic_pitch_fixed_at_amp(
        f0_hz: f32,
        harmonic_amps: &[f32],
        amp_scale: f32,
    ) -> f32 {
        let mut state = NlpStateFixed::new();
        let mut phase = 0.0f32;
        let mut history = [0i16; M_PITCH];
        let mut last_f0 = 0.0f32;

        let n_frames = (M_PITCH / N_SAMP) + 6;
        for _ in 0..n_frames {
            history.copy_within(N_SAMP..M_PITCH, 0);
            for s in history.iter_mut().skip(M_PITCH - N_SAMP) {
                let mut v = 0.0f32;
                for (h, &amp) in harmonic_amps.iter().enumerate() {
                    v += amp * (phase * (h + 1) as f32).sin();
                }
                *s = (v * amp_scale).round().clamp(i16::MIN as f32, i16::MAX as f32) as i16;
                phase += std::f32::consts::TAU * f0_hz / SAMPLE_RATE as f32;
            }
            last_f0 = nlp_fixed(&mut state, &history);
        }
        last_f0
    }

    /// `nlp_fixed`'s own version of `floating_reference::nlp::tests::
    /// finds_the_fundamental_of_a_synthetic_voiced_like_signal_across_
    /// the_valid_pitch_range` -- same synthetic signals, same tolerance,
    /// checked independently against the true `f0` (not just against
    /// `nlp`'s own float estimate) so a bug shared by both
    /// implementations wouldn't hide behind mutual agreement.
    #[test]
    fn nlp_fixed_finds_the_fundamental_of_a_synthetic_voiced_like_signal_across_the_valid_pitch_range(
    ) {
        for &f0 in &[70.0f32, 110.0, 150.0, 200.0, 250.0, 320.0] {
            let amps = [1.0, 0.6, 0.3, 0.15];
            let est = estimate_synthetic_pitch_fixed(f0, &amps);
            let rel_err = (est - f0).abs() / f0;
            assert!(
                rel_err < 0.05,
                "f0={f0}Hz estimated as {est}Hz, {}% error",
                rel_err * 100.0
            );
        }
    }

    /// The real acceptance bar for this migration (matching every other
    /// stage's own validation discipline): `nlp_fixed` must agree with
    /// the already-tested `nlp` closely enough that they'd select the
    /// same or an adjacent `Wo` quantizer level on real signals, across
    /// a wide sweep of f0/harmonic content -- not bit-exact agreement,
    /// since `correct_sub_multiples`'s own local-peak test is a strict
    /// inequality that both implementations can land on either side of
    /// at a genuine near-tie (see
    /// `correct_sub_multiples_fixed_rejects_an_exact_tie_the_same_way_
    /// the_float_version_does` below for why that's expected, not a
    /// bug).
    #[test]
    fn nlp_fixed_agrees_with_the_float_reference_across_a_wide_synthetic_sweep() {
        // Same range the original float-only test above already
        // validates `nlp()` itself against (70-320Hz) -- a pure,
        // harmonic-free tone right at the pitch range's own edge (e.g.
        // 380Hz, close to `P_MIN`'s ~400Hz limit) is a case where even
        // `nlp()` alone is only marginally accurate (checked directly:
        // it estimates such a tone at 393.75Hz, already a real 3.6%
        // error against the true 380Hz) -- not a fair basis for an
        // agreement bound between two *already only approximately
        // accurate* estimates, and not representative of real voiced
        // speech, which always carries harmonics.
        let f0_sweep = [70.0f32, 90.0, 110.0, 150.0, 200.0, 250.0, 320.0];
        let harmonic_sets: &[&[f32]] = &[&[1.0, 0.6, 0.3, 0.15], &[0.3, 1.0, 0.5, 0.2, 0.1]];
        let mut max_rel_err = 0.0f32;
        for &f0 in &f0_sweep {
            for amps in harmonic_sets {
                let est_float = estimate_synthetic_pitch(f0, amps);
                let est_fixed = estimate_synthetic_pitch_fixed(f0, amps);
                let rel_err = (est_fixed - est_float).abs() / est_float.max(1.0);
                max_rel_err = max_rel_err.max(rel_err);
                assert!(
                    rel_err < 0.05,
                    "f0={f0}Hz amps={amps:?}: float={est_float}Hz fixed={est_fixed}Hz, {}% disagreement",
                    rel_err * 100.0
                );
            }
        }
        // Not a hard assertion, just keeps the real measured margin
        // visible in test output (`cargo test -- --nocapture`) for
        // anyone tightening the tolerance above later.
        println!("nlp_fixed_agrees_with_the_float_reference: max_rel_err={max_rel_err:.5}");
    }

    /// Same agreement check, but at real full-scale `i16` amplitude
    /// (~30000, clipping on the loudest harmonic set, matching real
    /// clamped speech) rather than the more modest `REALISTIC_AMP`
    /// (8000) every other test in this file uses. `power[]`/`gmax` grow
    /// with the *fourth* power of sample amplitude (squared for `sq[]`,
    /// squared again for FFT power), so this is a real, necessary check,
    /// not a duplicate of the sweep above at a different number -- an
    /// earlier version of `correct_sub_multiples_fixed` silently
    /// overflowed its own `i64` threshold computation well before this
    /// amplitude, wrapping to a wrong (possibly negative) threshold with
    /// no panic in either debug or release, undetected by every test at
    /// `REALISTIC_AMP`'s more modest scale.
    #[test]
    fn nlp_fixed_agrees_with_the_float_reference_at_full_scale_amplitude() {
        let full_scale = 30_000.0f32;
        for &f0 in &[90.0f32, 150.0, 250.0] {
            for amps in [&[1.0, 0.6, 0.3, 0.15][..], &[0.3, 1.0, 0.5, 0.2, 0.1][..]] {
                let est_float = estimate_synthetic_pitch_at_amp(f0, amps, full_scale);
                let est_fixed = estimate_synthetic_pitch_fixed_at_amp(f0, amps, full_scale);
                let rel_err = (est_fixed - est_float).abs() / est_float.max(1.0);
                assert!(
                    rel_err < 0.05,
                    "full-scale f0={f0}Hz amps={amps:?}: float={est_float}Hz fixed={est_fixed}Hz, {}% disagreement",
                    rel_err * 100.0
                );
            }
        }
    }

    /// An all-silence frame (`sn` all zero) is the one case the `+1`
    /// bias in `nlp_fixed`/`nlp` exists for (keeping an all-zero FFT
    /// input from being pathological) -- exercised directly here rather
    /// than left implicit, since nothing else in this file's sweep ever
    /// produces a literal all-zero frame. Not a claim that silence
    /// produces a *meaningful* `f0` (neither implementation claims
    /// that) -- just that both stay finite and don't panic or diverge
    /// wildly from each other on this real, reachable input.
    #[test]
    fn nlp_fixed_matches_nlp_on_silence_without_panicking() {
        let mut state_f = NlpState::new();
        let mut state_i = NlpStateFixed::new();
        let silence_f = [0.0f32; M_PITCH];
        let silence_i = [0i16; M_PITCH];
        let mut last_f = 0.0f32;
        let mut last_i = 0.0f32;
        for _ in 0..10 {
            last_f = nlp(&mut state_f, &silence_f);
            last_i = nlp_fixed(&mut state_i, &silence_i);
        }
        assert!(last_f.is_finite(), "nlp produced a non-finite f0 on silence");
        assert!(
            last_i.is_finite(),
            "nlp_fixed produced a non-finite f0 on silence"
        );
    }

    /// Direct precision check on the pieces that carry real dynamic
    /// range: `dc_notch_fixed` and `decimate_fixed` against their float
    /// twins, on the same realistic-amplitude input, before the FFT
    /// itself can compound any of their error. Per this crate's own
    /// established "front-end first, then the FFT" migration order.
    #[test]
    fn dc_notch_fixed_matches_the_float_dc_notch_on_realistic_amplitude_input() {
        let mut mem_x = 0.0f32;
        let mut mem_y = 0.0f32;
        let mut mem_x_fixed = 0i64;
        let mut mem_y_fixed = 0i64;
        let mut seed = 42u32;
        // `squared` (always non-negative, `s*s`) carries a large mean
        // component of its own -- the notch's *job* is to reject that,
        // so its own output naturally settles to a scale far smaller
        // than `squared` itself. Comparing against `squared` (as an
        // earlier version of this test did) manufactures a spuriously
        // huge "relative error" out of two genuinely close numbers that
        // both happen to be small; the real reference scale is the
        // notch output's own typical post-transient magnitude, tracked
        // directly below rather than assumed.
        let mut max_abs_err = 0.0f32;
        let mut max_abs_y = 0.0f32;
        for i in 0..2000 {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            let s = ((seed >> 16) as i16 as f32 / 32768.0 * REALISTIC_AMP).round();
            let squared = s * s;
            let y = dc_notch(squared, &mut mem_x, &mut mem_y);
            let y_fixed = dc_notch_fixed(squared as i64, &mut mem_x_fixed, &mut mem_y_fixed);
            if i > 100 {
                max_abs_err = max_abs_err.max((y_fixed as f32 - y).abs());
                max_abs_y = max_abs_y.max(y.abs());
            }
        }
        let rel_err = max_abs_err / max_abs_y.max(1.0);
        assert!(
            rel_err < 0.01,
            "dc_notch_fixed diverged from dc_notch: max_abs_err={max_abs_err} vs max_abs_y={max_abs_y} ({}% )",
            rel_err * 100.0
        );
    }

    #[test]
    fn decimate_fixed_matches_the_float_decimate_on_realistic_amplitude_input() {
        let mut seed = 7u32;
        let mut sq = [0.0f32; M_PITCH];
        let mut sq_fixed = [0i64; M_PITCH];
        for i in 0..M_PITCH {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            let v = ((seed >> 16) as i16 as f32 / 32768.0 * REALISTIC_AMP * REALISTIC_AMP).round();
            sq[i] = v;
            sq_fixed[i] = v as i64;
        }
        let out = decimate(&sq);
        let out_fixed = decimate_fixed(&sq_fixed);
        let max_abs_in = sq.iter().fold(0.0f32, |m, &v| m.max(v.abs())).max(1.0);
        for i in 0..NDEC {
            let rel_err = (out_fixed[i] as f32 - out[i]).abs() / max_abs_in;
            assert!(
                rel_err < 1e-4,
                "decimate_fixed[{i}] diverged: float={} fixed={} rel_err={rel_err}",
                out[i],
                out_fixed[i]
            );
        }
    }

    /// `correct_sub_multiples`'s own local-peak test (`lmax >
    /// power[lmax_bin ± 1]`) is a strict inequality -- at an exact tie
    /// between adjacent bins, float and fixed-point can legitimately
    /// disagree about which side of `>` the comparison lands on (a
    /// rounding-direction difference between the two implementations,
    /// not a bug in either). This test documents and pins that expected
    /// behavior directly, the same way the LSP quantizer's own exact-tie
    /// case is pinned elsewhere in this crate, rather than leaving it as
    /// an unstated assumption.
    #[test]
    fn correct_sub_multiples_fixed_matches_correct_sub_multiples_away_from_exact_ties() {
        let true_bin = 40usize;
        let lo = 16usize;
        let len = 256usize;

        let mut power_f = vec![0.01f32; len];
        power_f[true_bin * 2] = 1.0;
        power_f[true_bin] = 0.5;
        let cmax_f = correct_sub_multiples(&power_f, 1.0, true_bin * 2, 0, lo);

        let scale = 1_000_000i128;
        let power_i: Vec<i128> = power_f
            .iter()
            .map(|&p| (p as f64 * scale as f64) as i128)
            .collect();
        let cmax_i = correct_sub_multiples_fixed(&power_i, scale, true_bin * 2, 0, lo);

        assert_eq!(cmax_f, true_bin);
        assert_eq!(cmax_i, true_bin);
        assert_eq!(cmax_f, cmax_i);
    }

    /// An exact power tie at the bin `correct_sub_multiples`'s local-peak
    /// test compares against is a real, reachable case (not just a
    /// theoretical corner) -- documents that both implementations use
    /// the same strict-`>` rule and so make the *same* choice (reject
    /// the tied candidate) when the tie is bit-for-bit exact in each
    /// implementation's own domain, which is the only guarantee either
    /// one actually makes.
    #[test]
    fn correct_sub_multiples_fixed_rejects_an_exact_tie_the_same_way_the_float_version_does() {
        let true_bin = 40usize;
        let lo = 16usize;
        let len = 256usize;

        let mut power_f = vec![0.01f32; len];
        power_f[true_bin * 2] = 1.0;
        power_f[true_bin] = 0.5;
        power_f[true_bin - 1] = 0.5; // exact tie with the candidate itself
        let cmax_f = correct_sub_multiples(&power_f, 1.0, true_bin * 2, 0, lo);

        let power_i: Vec<i128> = power_f
            .iter()
            .map(|&p| (p as f64 * 1_000_000.0) as i128)
            .collect();
        let cmax_i = correct_sub_multiples_fixed(&power_i, 1_000_000, true_bin * 2, 0, lo);

        // Neither implementation's strict `>` accepts a tied neighbor as
        // a local peak, so both fall back to leaving the raw global peak
        // uncorrected here.
        assert_eq!(cmax_f, true_bin * 2);
        assert_eq!(cmax_i, true_bin * 2);
    }

    /// `correct_sub_multiples_fixed`'s integer `(8*b)/10`/`(12*b)/10`
    /// bounds must agree with the float version's own `(0.8*b as f32)
    /// as usize`/`(1.2*b as f32) as usize` truncation across every `b`
    /// this module can actually produce (`b <= gmax_bin/2`, and
    /// `gmax_bin` is itself bounded by `hi`, this crate's own real
    /// P_MIN/P_MAX-derived search range) -- checked directly rather than
    /// assumed.
    #[test]
    fn sub_multiple_bounds_match_the_float_formula_across_the_real_bin_range() {
        let hi = (PE_FFT_SIZE * NLP_DEC / P_MIN).min(PE_FFT_SIZE / 2);
        for b in 0..=hi {
            let bmin_f = (0.8 * b as f32) as usize;
            let bmax_f = (1.2 * b as f32) as usize;
            let bmin_i = (8 * b) / 10;
            let bmax_i = (12 * b) / 10;
            assert_eq!(bmin_f, bmin_i, "bmin mismatch at b={b}");
            assert_eq!(bmax_f, bmax_i, "bmax mismatch at b={b}");
        }
    }

    /// `fft_fixed` against `rustfft`'s own float FFT on the same real
    /// input, comparing the resulting *power spectra* (magnitude only,
    /// per this module's own established "ordinal comparisons only"
    /// design, not real/imaginary parts directly, which depend on a sign
    /// convention this module deliberately leaves unfixed).
    #[test]
    fn fft_fixed_power_spectrum_matches_a_float_fft_on_a_multi_tone_input() {
        // Tones at exact bin frequencies of this *same* `PE_FFT_SIZE`-
        // point transform (not a shorter, zero-padded block) so both
        // implementations should land clean, unleaked peaks at exactly
        // these bins -- a real, checkable ground truth rather than an
        // approximate one.
        let tones: &[(usize, f32)] = &[(30, 1.0), (70, 0.5), (190, 0.25)];
        let mut seed = 99u32;
        let mut input_f = [0.0f32; PE_FFT_SIZE];
        let mut re = [0i64; PE_FFT_SIZE];
        for i in 0..PE_FFT_SIZE {
            let mut v = 0.0f32;
            for &(bin, amp) in tones {
                v += amp
                    * (std::f32::consts::TAU * bin as f32 * i as f32 / PE_FFT_SIZE as f32).sin();
            }
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            v += 0.02 * ((seed >> 16) as i16 as f32 / 32768.0);
            let scaled = v * 1.0e9;
            input_f[i] = scaled;
            re[i] = scaled.round() as i64;
        }
        let mut im = [0i64; PE_FFT_SIZE];
        fft_fixed(&mut re, &mut im);

        let mut planner = FftPlanner::<f32>::new();
        let fft = planner.plan_fft_forward(PE_FFT_SIZE);
        let mut buf: Vec<Complex32> = input_f.iter().map(|&x| Complex32::new(x, 0.0)).collect();
        fft.process(&mut buf);

        const HALF: usize = PE_FFT_SIZE / 2 + 1;
        let power_fixed: Vec<i128> = (0..HALF)
            .map(|i| re[i] as i128 * re[i] as i128 + im[i] as i128 * im[i] as i128)
            .collect();
        let power_float: Vec<f32> = (0..HALF)
            .map(|i| buf[i].re * buf[i].re + buf[i].im * buf[i].im)
            .collect();

        let gmax_float = power_float.iter().cloned().fold(0.0f32, f32::max);
        let gmax_fixed = power_fixed.iter().cloned().fold(0i128, i128::max);
        let gmax_bin_float = power_float.iter().position(|&p| p == gmax_float).unwrap();
        let gmax_bin_fixed = power_fixed.iter().position(|&p| p == gmax_fixed).unwrap();
        assert_eq!(
            gmax_bin_float, 30,
            "float FFT's own peak landed off the expected bin"
        );
        assert_eq!(
            gmax_bin_float, gmax_bin_fixed,
            "fft_fixed's own global peak bin disagrees with the float FFT's"
        );

        // Confirm the two weaker tones are also each the loudest bin in
        // a narrow window around their own known frequency, for both
        // implementations -- pins the whole spectrum shape, not just
        // its single global peak.
        for &(bin, _) in &tones[1..] {
            let window = bin - 3..=bin + 3;
            let local_float = window
                .clone()
                .max_by(|&a, &b| power_float[a].partial_cmp(&power_float[b]).unwrap())
                .unwrap();
            let local_fixed = window.clone().max_by_key(|&b| power_fixed[b]).unwrap();
            assert_eq!(local_float, bin, "float FFT's tone at bin {bin} was off-peak");
            assert_eq!(local_fixed, bin, "fft_fixed's tone at bin {bin} was off-peak");
        }
    }

    #[test]
    fn f0_to_wo_matches_the_format_wo_range() {
        assert!((f0_to_wo(50.0) - W0_MIN).abs() < 1e-3);
        assert!((f0_to_wo(400.0) - W0_MAX).abs() < 1e-3);
    }
}
