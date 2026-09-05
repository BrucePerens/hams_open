// SPDX-License-Identifier: LGPL-3.0-or-later
//! Sinusoidal synthesis: reconstructs a 10ms sub-frame of audio from a
//! `Model` (harmonic amplitudes/phases from `envelope.rs`) via the
//! standard technique for this family of vocoders -- McAulay & Quatieri
//! 1986's sinusoidal speech model, synthesized here through a sparse
//! per-harmonic spectrum, an inverse FFT, and Parzen-windowed
//! overlap-add (an efficient way to sum many individually-phased
//! sinusoids without one oscillator per harmonic) -- general published
//! technique, reimplemented from that understanding, not this specific
//! codec's own source. Plus: zero-order-hold phase tracking for voiced
//! excitation (advance one phase per frame from `Wo`, sample each
//! harmonic's phase off it, filtered by the LPC synthesis filter's own
//! phase response), a postfilter that randomizes weak harmonics' phase
//! during voiced frames (quietened noise-like components in an
//! otherwise-tonal frame sound better randomized than tonal), and a
//! peak-limiter as a defensive measure against bit-error-induced level
//! spikes.

use super::envelope::Model;
use super::{BG_BETA, BG_MARGIN, BG_THRESH, FFT_ENC, MAX_AMP, N_SAMP, SAMPLES_PER_FRAME, TW};
use rustfft::num_complex::Complex32;
use rustfft::{Fft, FftPlanner};
use std::sync::Arc;

fn make_synthesis_window() -> [f32; SAMPLES_PER_FRAME] {
    let mut pn = [0.0f32; SAMPLES_PER_FRAME];
    let n0 = N_SAMP / 2;
    let n1 = 3 * N_SAMP / 2;
    let inv_2tw = 1.0 / (2.0 * TW as f32);
    for (i, v) in pn
        .iter_mut()
        .enumerate()
        .take((n0 + TW).min(SAMPLES_PER_FRAME))
        .skip(n0.saturating_sub(TW))
    {
        *v = (i as f32 - (n0 - TW) as f32) * inv_2tw;
    }
    for v in pn.iter_mut().take(n1.saturating_sub(TW)).skip(n0 + TW) {
        *v = 1.0;
    }
    for (i, v) in pn
        .iter_mut()
        .enumerate()
        .take((n1 + TW).min(SAMPLES_PER_FRAME))
        .skip(n1.saturating_sub(TW))
    {
        *v = ((n1 + TW) as f32 - i as f32) * inv_2tw;
    }
    pn
}

/// Simple xorshift PRNG for unvoiced-excitation and postfilter phase
/// randomization -- doesn't need to match the reference's own generator
/// (purely a synthesis-quality detail, not transmitted).
pub(crate) fn next_rand(state: &mut u32) -> f32 {
    *state ^= *state << 13;
    *state ^= *state >> 17;
    *state ^= *state << 5;
    (*state >> 8) as f32 / (1u32 << 24) as f32 * std::f32::consts::TAU
}

/// Advances the voiced-excitation phase track by one frame and samples
/// each harmonic's phase through the LPC synthesis filter (`h`, from
/// `envelope::sample_filter_phase`) -- voiced harmonics phase-lock to a
/// single tracked fundamental phase (zero-order-hold pitch synthesis);
/// unvoiced harmonics get independent random phase.
fn synthesize_phase(
    model: &mut Model,
    h: &[Complex32; MAX_AMP + 1],
    ex_phase: &mut f32,
    rng: &mut u32,
) {
    *ex_phase += model.wo * N_SAMP as f32;
    *ex_phase -= std::f32::consts::TAU * (*ex_phase / std::f32::consts::TAU + 0.5).floor();
    let phi0 = *ex_phase;

    // `m` is the harmonic number (used in `phi0 * m` below, not just an
    // array index) shared across `h` and `model.phi` -- not a clean fit
    // for `.iter().enumerate()`.
    #[allow(clippy::needless_range_loop)]
    for m in 1..=model.l {
        let ex = if model.voiced {
            let (s, c) = (phi0 * m as f32).sin_cos();
            Complex32::new(c, s)
        } else {
            let phi = next_rand(rng);
            let (s, c) = phi.sin_cos();
            Complex32::new(c, s)
        };
        let a = h[m] * ex;
        // Unit vector in the direction of `a`, replacing the old
        // `a.im.atan2(a.re + 1e-12)` angle extraction -- see `Model::phi`'s
        // own doc comment. The `1e-12` floor plays the same role the old
        // `atan2` epsilon did: guard the degenerate near-zero-magnitude
        // case rather than dividing by (near) zero.
        let mag = (a.re * a.re + a.im * a.im).sqrt().max(1e-12);
        model.phi[m] = Complex32::new(a.re / mag, a.im / mag);
    }
}

/// Pure decision logic behind `postfilter` below -- no RNG/phi
/// mutation, and the two log-domain operations (`e_db`, `thresh`) are
/// parameterized (`log2`/`exp2`) rather than hardcoded, so
/// `fixed_point.rs`'s own tests can run the *identical* control flow
/// with the real `fixed_point::log2_lut`/`exp2_lut` swapped out for
/// plain `f32::log2`/`f32::exp2` (mathematically the same operations
/// `postfilter`'s own pre-LUT code used, just expressed in base 2
/// instead of base 10 -- `10*log10(x) == 10*log2(x)/LOG2_10`,
/// `10^y == 2^(y*LOG2_10)`) and compare real per-harmonic decisions
/// against each other over a real temporal replay, the same validation
/// shape `CODEC2_MOD_FIXED_POINT_PLAN.md` used for this exact stage
/// (145,576 real per-harmonic decisions, zero mismatches there).
/// Returns the updated `bg_est` and, for a voiced frame, which
/// harmonics `1..=l` should get their phase randomized (unvoiced
/// frames return an all-`false` array -- there's nothing to randomize
/// on that branch, only `bg_est` updates).
pub(crate) fn postfilter_step<L: Fn(f32) -> f32, E: Fn(f32) -> f32>(
    voiced: bool,
    l: usize,
    a: &[f32; MAX_AMP + 1],
    bg_est: f32,
    log2: L,
    exp2: E,
) -> (f32, [bool; MAX_AMP + 1]) {
    let e: f32 = 1e-12 + a[1..=l].iter().map(|v| v * v).sum::<f32>();
    let e_db = 10.0 * (log2(e / l as f32) / std::f32::consts::LOG2_10);

    let mut decisions = [false; MAX_AMP + 1];
    let mut new_bg_est = bg_est;
    if !voiced {
        if e_db < BG_THRESH {
            new_bg_est = bg_est * (1.0 - BG_BETA) + e_db * BG_BETA;
        }
    } else {
        let thresh = exp2((bg_est + BG_MARGIN) / 20.0 * std::f32::consts::LOG2_10);
        for (m, &am) in a.iter().enumerate().take(l + 1).skip(1) {
            decisions[m] = am < thresh;
        }
    }
    (new_bg_est, decisions)
}

/// Randomizes the phase of harmonics quiet relative to the tracked
/// background-noise level during voiced frames (makes them sound
/// unvoiced/noise-like rather than tonal, closer to real speech's own
/// mixed excitation); tracks that background level from unvoiced
/// frames' own average energy otherwise. See `postfilter_step`'s own
/// doc comment for the log-domain LUT this calls into and how it's
/// validated.
fn postfilter(model: &mut Model, bg_est: &mut f32, rng: &mut u32) {
    let (new_bg_est, decisions) = postfilter_step(
        model.voiced,
        model.l,
        &model.a,
        *bg_est,
        super::fixed_point::log2_lut,
        super::fixed_point::exp2_lut,
    );
    *bg_est = new_bg_est;
    if model.voiced {
        // `decisions` (read) and `model.phi` (written) are independent
        // arrays at the same harmonic index -- not a clean fit for
        // `.iter().enumerate()`.
        #[allow(clippy::needless_range_loop)]
        for m in 1..=model.l {
            if decisions[m] {
                let phi = next_rand(rng);
                let (s, c) = phi.sin_cos();
                model.phi[m] = Complex32::new(c, s);
            }
        }
    }
}

/// Attenuates a whole frame if any sample would exceed a safe int16
/// level -- a defensive measure against bit-error-induced amplitude
/// spikes reaching real ears/speakers, not a normal-operation limiter.
pub(crate) fn ear_protection(samples: &mut [f32]) {
    let max_abs = samples.iter().fold(0.0f32, |m, &s| m.max(s.abs()));
    if max_abs <= 30000.0 {
        return;
    }
    let over = max_abs / 30000.0;
    let gain = 1.0 / (over * over);
    for s in samples.iter_mut() {
        *s *= gain;
    }
}

/// Persistent per-decoder synthesis state.
pub struct SynthesisState {
    /// `SAMPLES_PER_FRAME`-sample overlap-add buffer: the newest
    /// `N_SAMP` samples are finished output, the rest carries the
    /// windowed tail of the previous frame's own IDFT waiting to be
    /// summed into the next one.
    sn_: [f32; SAMPLES_PER_FRAME],
    parzen: [f32; SAMPLES_PER_FRAME],
    ex_phase: f32,
    bg_est: f32,
    rng: u32,
    ifft: Arc<dyn Fft<f32>>,
    /// `FFT_ENC` is a compile-time constant, and this buffer is reused
    /// every call -- a fixed-size stack array, not a `Vec`, since this
    /// runs on a real-time codec's per-10ms-sub-frame decode path.
    ifft_buf: [Complex32; FFT_ENC],
}

impl Default for SynthesisState {
    fn default() -> Self {
        let mut planner = FftPlanner::<f32>::new();
        SynthesisState {
            sn_: [0.0; SAMPLES_PER_FRAME],
            parzen: make_synthesis_window(),
            ex_phase: 0.0,
            bg_est: 0.0,
            rng: 0xC0FFEE,
            ifft: planner.plan_fft_inverse(FFT_ENC),
            ifft_buf: [Complex32::new(0.0, 0.0); FFT_ENC],
        }
    }
}

impl SynthesisState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Synthesizes one `N_SAMP`-sample (10ms) sub-frame from `model`
    /// (its amplitudes/phases already filled in by `envelope.rs`) and
    /// `h` (the LPC synthesis filter's own phase response, for voiced
    /// excitation phase tracking).
    pub fn synthesize_subframe(&mut self, model: &mut Model, aw: &[Complex32]) -> [i16; N_SAMP] {
        let h = super::envelope::sample_filter_phase(aw, model);
        synthesize_phase(model, &h, &mut self.ex_phase, &mut self.rng);
        postfilter(model, &mut self.bg_est, &mut self.rng);

        // Shift the overlap-add buffer: the previous frame's own
        // "new" half becomes this frame's "carried-over tail" half.
        self.sn_.copy_within(N_SAMP.., 0);
        self.sn_[N_SAMP - 1] = 0.0;

        for c in self.ifft_buf.iter_mut() {
            *c = Complex32::new(0.0, 0.0);
        }
        let fft_r = std::f32::consts::TAU / FFT_ENC as f32;
        for l in 1..=model.l {
            let b = (((l as f32 * model.wo / fft_r) + 0.5) as usize).min(FFT_ENC / 2 - 1);
            self.ifft_buf[b] = model.phi[l] * model.a[l];
        }
        // Mirror into a full Hermitian-symmetric spectrum so the
        // inverse FFT's output is real (up to float rounding): bin 0
        // and the Nyquist bin have no imaginary part by construction
        // above (harmonics never land exactly there in practice), and
        // every other populated bin's conjugate goes in its mirror.
        for k in 1..(FFT_ENC / 2) {
            self.ifft_buf[FFT_ENC - k] = self.ifft_buf[k].conj();
        }

        self.ifft.process(&mut self.ifft_buf);

        // Three arrays (`sn_`, `ifft_buf`, `parzen`), each at its own
        // offset from the loop index -- not a clean fit for
        // `.enumerate()`. `sw_` (the IDFT's real time-domain output) is
        // `ifft_buf[..].re` read directly, no separate buffer needed.
        #[allow(clippy::needless_range_loop)]
        for i in 0..(N_SAMP - 1) {
            self.sn_[i] += self.ifft_buf[FFT_ENC - N_SAMP + 1 + i].re * self.parzen[i];
        }
        #[allow(clippy::needless_range_loop)]
        for j in 0..(N_SAMP + 1) {
            let idx = N_SAMP - 1 + j;
            if idx < SAMPLES_PER_FRAME {
                self.sn_[idx] = self.ifft_buf[j].re * self.parzen[N_SAMP - 1 + j];
            }
        }

        let mut out: [f32; N_SAMP] = std::array::from_fn(|i| self.sn_[i]);
        ear_protection(&mut out);

        std::array::from_fn(|i| out[i].clamp(-32767.0, 32767.0) as i16)
    }
}

use super::envelope::{synth_k_q23, ModelFixed};
use super::fixed_fft::{fft_fixed, ComplexQ23};
use super::fixed_point::{exp2_q23, log2_q23};
use super::trig_fixed::sin_cos_q23;

const FRAC_BITS: u32 = 23;

/// `wo`'s own per-sample-to-per-subframe phase increment, in a `u32`
/// "turns" representation (see `trig_fixed.rs`'s own doc comment) --
/// `wo_q23` and `pi_q23()` share Q23 scaling, so it cancels in the
/// division; the final cast to `u32` keeps only the fractional-turn
/// part (the whole-turn count is discarded, correctly, since only the
/// angle mod one turn ever matters downstream).
fn phase_increment_q32(wo_q23: i64) -> u32 {
    let tau_q23 = 2 * super::lpc::pi_q23();
    let scaled = (wo_q23 as i128 * N_SAMP as i128) << 32;
    let half_denom = tau_q23 as i128 / 2;
    (((scaled + half_denom) / tau_q23 as i128) & 0xFFFF_FFFF) as u32
}

/// Same xorshift state update as `next_rand` -- the two decoders hold
/// independent `rng` state (never shared), so what matters for the
/// eventual decoded-audio comparison is that this draws exactly the
/// same *number* of random values, at the same points in the
/// algorithm, as the float path (see `postfilter_fixed`'s own call
/// site) -- not that the two produce numerically identical angles.
/// Returns the raw post-step state directly as a `u32` turns angle: no
/// scaling needed at all, unlike the float version's own `radians`
/// conversion, since a raw xorshift word is already uniform over its
/// full range.
fn next_rand_fixed(state: &mut u32) -> u32 {
    *state ^= *state << 13;
    *state ^= *state >> 17;
    *state ^= *state << 5;
    *state
}

/// Fixed-point `synthesize_phase`: `h`/`model.phi` both `ComplexQ23`,
/// `ex_phase`/random angles both `u32` turns. The unit-vector
/// normalization (`a / |a|`) composes `log2_q23`/`exp2_q23` for
/// `1/sqrt(mag_sq)` (`exp2(-0.5*log2(mag_sq))`) rather than a fixed-
/// point square root primitive -- same log-domain-reciprocal-sqrt
/// trick `envelope.rs`'s own gain normalization uses.
fn synthesize_phase_fixed(
    model: &mut ModelFixed,
    h: &[ComplexQ23; MAX_AMP + 1],
    ex_phase_q32: &mut u32,
    rng: &mut u32,
) {
    let increment = phase_increment_q32(model.wo);
    *ex_phase_q32 = ex_phase_q32.wrapping_add(increment);
    let phi0_q32 = *ex_phase_q32;

    #[allow(clippy::needless_range_loop)]
    for m in 1..=model.l {
        let ex = if model.voiced {
            // See trig_fixed.rs's own doc comment: multiplying the
            // already-wrapped fractional angle by `m`, widened through
            // u64 then truncated back to u32, gives exactly the m-th
            // harmonic's own angle mod one turn.
            let angle_m_q32 = ((phi0_q32 as u64 * m as u64) & 0xFFFF_FFFF) as u32;
            sin_cos_q23(angle_m_q32)
        } else {
            sin_cos_q23(next_rand_fixed(rng))
        };
        let a = h[m].mul(ex);
        let mag_sq = ((a.re as i128 * a.re as i128 + a.im as i128 * a.im as i128) >> FRAC_BITS)
            .max(1) as i64;
        let inv_mag = exp2_q23(-(log2_q23(mag_sq) >> 1));
        model.phi[m] = a.mul(ComplexQ23 { re: inv_mag, im: 0 });
    }
}

fn bg_thresh_q23() -> i64 {
    static V: std::sync::OnceLock<i64> = std::sync::OnceLock::new();
    *V.get_or_init(|| super::fixed_point::f32_to_q_exact_round(BG_THRESH, FRAC_BITS))
}
fn bg_beta_q23() -> i64 {
    static V: std::sync::OnceLock<i64> = std::sync::OnceLock::new();
    *V.get_or_init(|| super::fixed_point::f32_to_q_exact_round(BG_BETA, FRAC_BITS))
}
fn one_minus_bg_beta_q23() -> i64 {
    static V: std::sync::OnceLock<i64> = std::sync::OnceLock::new();
    *V.get_or_init(|| super::fixed_point::f32_to_q_exact_round(1.0 - BG_BETA, FRAC_BITS))
}
fn bg_margin_q23() -> i64 {
    static V: std::sync::OnceLock<i64> = std::sync::OnceLock::new();
    *V.get_or_init(|| super::fixed_point::f32_to_q_exact_round(BG_MARGIN, FRAC_BITS))
}
/// `10.0 / LOG2_10` in Q23 -- `e_db = 10*log2(x)/LOG2_10` becomes
/// `log2_q23(x) * this constant`, rescaled.
fn ten_over_log2_10_q23() -> i64 {
    static V: std::sync::OnceLock<i64> = std::sync::OnceLock::new();
    *V.get_or_init(|| {
        super::fixed_point::f32_to_q_exact_round(10.0 / std::f32::consts::LOG2_10, FRAC_BITS)
    })
}
/// `LOG2_10 / 20.0` in Q23 -- `thresh = exp2((bg+MARGIN)/20*LOG2_10)`
/// becomes `exp2_q23((bg_q23+margin_q23) * this constant)`, rescaled.
fn log2_10_over_20_q23() -> i64 {
    static V: std::sync::OnceLock<i64> = std::sync::OnceLock::new();
    *V.get_or_init(|| {
        super::fixed_point::f32_to_q_exact_round(std::f32::consts::LOG2_10 / 20.0, FRAC_BITS)
    })
}

/// Fixed-point `postfilter_step`: `a`/`bg_est`/return value all Q23,
/// `log2_q23`/`exp2_q23` used directly (no parameterization -- unlike
/// the float version, there's no equivalent "plain float in the same
/// shape" comparison to make against a genuinely-integer function).
pub(crate) fn postfilter_step_fixed(
    voiced: bool,
    l: usize,
    a: &[i64; MAX_AMP + 1],
    bg_est: i64,
) -> (i64, [bool; MAX_AMP + 1]) {
    let mut e_q23: i64 = 0;
    for &av in &a[1..=l] {
        e_q23 += ((av as i128 * av as i128) >> FRAC_BITS) as i64;
    }
    let e_over_l_q23 = (e_q23 + l as i64 / 2) / l as i64;
    let e_db_q23 =
        ((log2_q23(e_over_l_q23.max(1)) as i128 * ten_over_log2_10_q23() as i128) >> FRAC_BITS)
            as i64;

    let mut decisions = [false; MAX_AMP + 1];
    let mut new_bg_est = bg_est;
    if !voiced {
        if e_db_q23 < bg_thresh_q23() {
            new_bg_est = ((bg_est as i128 * one_minus_bg_beta_q23() as i128
                + e_db_q23 as i128 * bg_beta_q23() as i128)
                >> FRAC_BITS) as i64;
        }
    } else {
        let y_q23 = (((bg_est + bg_margin_q23()) as i128 * log2_10_over_20_q23() as i128) >> FRAC_BITS) as i64;
        let thresh = exp2_q23(y_q23);
        for (m, &am) in a.iter().enumerate().take(l + 1).skip(1) {
            decisions[m] = am < thresh;
        }
    }
    (new_bg_est, decisions)
}

fn postfilter_fixed(model: &mut ModelFixed, bg_est: &mut i64, rng: &mut u32) {
    let (new_bg_est, decisions) = postfilter_step_fixed(model.voiced, model.l, &model.a, *bg_est);
    *bg_est = new_bg_est;
    if model.voiced {
        #[allow(clippy::needless_range_loop)]
        for m in 1..=model.l {
            if decisions[m] {
                model.phi[m] = sin_cos_q23(next_rand_fixed(rng));
            }
        }
    }
}

fn ear_protection_thresh_q23() -> i64 {
    static V: std::sync::OnceLock<i64> = std::sync::OnceLock::new();
    *V.get_or_init(|| super::fixed_point::f32_to_q_exact_round(30000.0, FRAC_BITS))
}

/// Fixed-point `ear_protection`: `gain = (thresh/max_abs)^2` computed
/// via one direct `i128` division (both squares fit `i128` easily at
/// these magnitudes), not a log2/exp2 round trip -- exact, and this is
/// the one path whose entire job is bounding amplitude, not a place to
/// spend extra LUT-interpolation error (same reasoning `envelope.rs`'s
/// own `gain_q23` uses division instead of a third log-domain
/// composition).
fn ear_protection_fixed(samples: &mut [i64]) {
    let max_abs = samples.iter().fold(0i64, |m, &s| m.max(s.abs()));
    let thresh = ear_protection_thresh_q23();
    if max_abs <= thresh {
        return;
    }
    let thresh_sq = thresh as i128 * thresh as i128;
    let max_abs_sq = max_abs as i128 * max_abs as i128;
    for s in samples.iter_mut() {
        *s = ((*s as i128 * thresh_sq) / max_abs_sq) as i64;
    }
}

/// Fixed-point sibling of `SynthesisState` -- genuinely integer end to
/// end. `ifft` is `fixed_fft::fft_fixed` (`forward: false`, matching
/// `rustfft`'s own unnormalized inverse convention, verified against it
/// directly in `fixed_fft.rs`'s own tests) rather than an `Arc<dyn
/// Fft<f32>>`, so there's no trait object or planner here at all.
pub(crate) struct SynthesisStateFixed {
    sn_: [i64; SAMPLES_PER_FRAME],
    parzen: [i64; SAMPLES_PER_FRAME],
    ex_phase: u32,
    bg_est: i64,
    rng: u32,
    ifft_re: [i64; FFT_ENC],
    ifft_im: [i64; FFT_ENC],
}

fn parzen_window_q23() -> &'static [i64; SAMPLES_PER_FRAME] {
    static V: std::sync::OnceLock<[i64; SAMPLES_PER_FRAME]> = std::sync::OnceLock::new();
    V.get_or_init(|| {
        let pn = make_synthesis_window();
        std::array::from_fn(|i| super::fixed_point::f32_to_q_exact_round(pn[i], FRAC_BITS))
    })
}

impl Default for SynthesisStateFixed {
    fn default() -> Self {
        SynthesisStateFixed {
            sn_: [0; SAMPLES_PER_FRAME],
            parzen: *parzen_window_q23(),
            ex_phase: 0,
            bg_est: 0,
            rng: 0xC0FFEE,
            ifft_re: [0; FFT_ENC],
            ifft_im: [0; FFT_ENC],
        }
    }
}

impl SynthesisStateFixed {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Fixed-point `synthesize_subframe`. `aw`/`model` from `envelope::
    /// compute_harmonic_amplitudes_fixed`/`ModelFixed`.
    pub(crate) fn synthesize_subframe_fixed(
        &mut self,
        model: &mut ModelFixed,
        aw: &[ComplexQ23],
    ) -> [i16; N_SAMP] {
        let h = super::envelope::sample_filter_phase_fixed(aw, model);
        synthesize_phase_fixed(model, &h, &mut self.ex_phase, &mut self.rng);
        postfilter_fixed(model, &mut self.bg_est, &mut self.rng);

        self.sn_.copy_within(N_SAMP.., 0);
        self.sn_[N_SAMP - 1] = 0;

        for i in 0..FFT_ENC {
            self.ifft_re[i] = 0;
            self.ifft_im[i] = 0;
        }
        let k_q23 = synth_k_q23(model.wo);
        for l in 1..=model.l {
            let raw = l as i64 * k_q23;
            let b = (((raw + (1i64 << 22)) >> 23) as usize).min(FFT_ENC / 2 - 1);
            let bin = model.phi[l].mul(ComplexQ23 { re: model.a[l], im: 0 });
            self.ifft_re[b] = bin.re;
            self.ifft_im[b] = bin.im;
        }
        for k in 1..(FFT_ENC / 2) {
            self.ifft_re[FFT_ENC - k] = self.ifft_re[k];
            self.ifft_im[FFT_ENC - k] = -self.ifft_im[k];
        }

        fft_fixed(&mut self.ifft_re, &mut self.ifft_im, false);

        #[allow(clippy::needless_range_loop)]
        for i in 0..(N_SAMP - 1) {
            let re = self.ifft_re[FFT_ENC - N_SAMP + 1 + i];
            self.sn_[i] += ((re as i128 * self.parzen[i] as i128) >> FRAC_BITS) as i64;
        }
        #[allow(clippy::needless_range_loop)]
        for j in 0..(N_SAMP + 1) {
            let idx = N_SAMP - 1 + j;
            if idx < SAMPLES_PER_FRAME {
                let re = self.ifft_re[j];
                self.sn_[idx] = ((re as i128 * self.parzen[idx] as i128) >> FRAC_BITS) as i64;
            }
        }

        let mut out: [i64; N_SAMP] = std::array::from_fn(|i| self.sn_[i]);
        ear_protection_fixed(&mut out);

        std::array::from_fn(|i| {
            let sample_f = out[i] as f32 / (1i64 << FRAC_BITS) as f32;
            sample_f.clamp(-32767.0, 32767.0) as i16
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn synthesis_window_peaks_at_1_in_the_middle_and_tapers_toward_0_at_both_ends() {
        let pn = make_synthesis_window();
        assert!(
            (pn[N_SAMP] - 1.0).abs() < 1e-6,
            "expected peak ~1.0 at the window center, got {}",
            pn[N_SAMP]
        );
        assert_eq!(
            pn[0], 0.0,
            "the ramp starts exactly at 0 by construction (n0-TW == 0 here)"
        );
        // The ramp only reaches exactly 0 at the one-past-the-end index
        // n1+TW == SAMPLES_PER_FRAME, so the last real sample is one
        // step short of it (1/(2*TW)), not exactly 0.
        let last_step = 1.0 / (2.0 * TW as f32);
        assert!(
            (pn[SAMPLES_PER_FRAME - 1] - last_step).abs() < 1e-6,
            "expected the last sample one ramp-step above 0 ({last_step}), got {}",
            pn[SAMPLES_PER_FRAME - 1]
        );
    }

    #[test]
    fn next_rand_produces_values_spread_across_the_full_tau_range() {
        let mut state = 12345u32;
        let mut min = f32::MAX;
        let mut max = f32::MIN;
        for _ in 0..1000 {
            let v = next_rand(&mut state);
            min = min.min(v);
            max = max.max(v);
            assert!((0.0..std::f32::consts::TAU).contains(&v));
        }
        assert!(
            max - min > std::f32::consts::TAU * 0.9,
            "PRNG output didn't spread across the range: min={min} max={max}"
        );
    }

    /// Fixed-point sibling of `fixed_point.rs`'s own `postfilter_lut_
    /// decisions_match_plain_float_across_a_real_temporal_replay` --
    /// same real fixture, same voiced/unvoiced run structure, but
    /// comparing `postfilter_step_fixed` (genuinely Q23 throughout)
    /// against `postfilter_step` (still called with the LUT closures,
    /// matching that other test's own real production call site)
    /// instead of against plain float. `bg_est` is a stateful EMA
    /// carried across frames -- a single-frame test can't see a sign or
    /// rescale bug in the EMA update itself, only a real multi-frame
    /// replay comparing its own drift can.
    #[test]
    fn postfilter_step_fixed_matches_postfilter_step_across_a_real_temporal_replay() {
        use crate::codec2_3200::envelope::{
            apply_first_harmonic_correction, apply_first_harmonic_correction_fixed,
            compute_harmonic_amplitudes, compute_harmonic_amplitudes_fixed, Model, ModelFixed,
        };
        use crate::codec2_3200::fixed_point::f32_to_q_exact_round;
        use crate::codec2_3200::lpc::{lsp_to_lpc, lsp_to_lpc_fixed, COEF_FRAC_BITS};
        use crate::codec2_3200::{LPC_ORD, W0_MAX, W0_MIN};

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
        let lsp_rows = read_rows(
            concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/codec2_3200/codec2_lsp_dump.txt"),
            LPC_ORD + 1,
        );
        let e_rows = read_rows(
            concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/codec2_3200/codec2_enc_e_dump.txt"),
            1,
        );
        let n = lsp_rows.len().min(e_rows.len());
        assert!(n > 300, "expected the real captured fixture corpus, got {n} rows");

        let mut planner = rustfft::FftPlanner::<f32>::new();
        let fft = planner.plan_fft_forward(FFT_ENC);
        let wo = W0_MIN + (W0_MAX - W0_MIN) * 0.3;
        let wo_q23 = f32_to_q_exact_round(wo, COEF_FRAC_BITS);

        let mut plain_bg = 0.0f32;
        let mut fixed_bg_q23 = 0i64;
        let mut decisions_checked = 0usize;
        let mut decision_mismatches = 0usize;
        let mut max_bg_drift_db = 0.0f32;

        for i in 0..n {
            let roots = lsp_rows[i][0] as i32;
            if roots as usize != LPC_ORD {
                continue;
            }
            let mut lsp = [0.0f32; LPC_ORD];
            lsp.copy_from_slice(&lsp_rows[i][1..]);
            let e = e_rows[i][0].max(1e-3);
            let voiced = (i / 30) % 2 == 1;

            let ak = lsp_to_lpc(&lsp);
            let mut model = Model::new(wo, voiced);
            let _aw = compute_harmonic_amplitudes(fft.as_ref(), &ak, e, &mut model);
            apply_first_harmonic_correction(&mut model);

            let lsp_q23: [i64; LPC_ORD] =
                std::array::from_fn(|j| f32_to_q_exact_round(lsp[j], COEF_FRAC_BITS));
            let ak_q23 = lsp_to_lpc_fixed(&lsp_q23);
            let e_q23 = f32_to_q_exact_round(e, FRAC_BITS);
            let mut model_fixed = ModelFixed::new(wo_q23, voiced);
            let _aw_fixed = compute_harmonic_amplitudes_fixed(&ak_q23, e_q23, &mut model_fixed);
            apply_first_harmonic_correction_fixed(&mut model_fixed);

            let (new_plain_bg, plain_decisions) =
                postfilter_step(model.voiced, model.l, &model.a, plain_bg, super::super::fixed_point::log2_lut, super::super::fixed_point::exp2_lut);
            let (new_fixed_bg_q23, fixed_decisions) =
                postfilter_step_fixed(model_fixed.voiced, model_fixed.l, &model_fixed.a, fixed_bg_q23);

            let fixed_bg_db = new_fixed_bg_q23 as f32 / (1i64 << FRAC_BITS) as f32;
            max_bg_drift_db = max_bg_drift_db.max((new_plain_bg - fixed_bg_db).abs());
            plain_bg = new_plain_bg;
            fixed_bg_q23 = new_fixed_bg_q23;

            if voiced {
                let l = model.l.min(model_fixed.l);
                for m in 1..=l {
                    decisions_checked += 1;
                    if plain_decisions[m] != fixed_decisions[m] {
                        decision_mismatches += 1;
                    }
                }
            }
        }

        println!("postfilter_step_fixed: {decision_mismatches}/{decisions_checked} decision mismatches, max bg_est drift {max_bg_drift_db}dB");
        assert!(decisions_checked > 1000, "expected a real number of per-harmonic decisions checked, got {decisions_checked}");
        assert!(
            (decision_mismatches as f64 / decisions_checked as f64) < 0.01,
            "{decision_mismatches}/{decisions_checked} real per-harmonic postfilter decisions diverged between fixed and float across the temporal replay"
        );
        assert!(
            max_bg_drift_db < 0.5,
            "bg_est drifted {max_bg_drift_db}dB between fixed and float over the replay"
        );
    }
}
