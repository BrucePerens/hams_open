// SPDX-License-Identifier: LGPL-3.0-or-later
//! Spectral Bridge: an opt-in 16kHz decode extension that extrapolates
//! harmonic amplitudes above the codec's own native ~4kHz ceiling and
//! synthesizes them into the newly-available high-frequency band --
//! Bruce's own explicit design, verbatim: "look at the amplitude of
//! the highest harmonic reproduced by the CODEC, and guess that there
//! would be additional harmonics after it." A real, well-established
//! technique (the same family as AMR-NB->WB, spectral-band-
//! replication) applied to Codec2's own explicit sinusoidal harmonic
//! model rather than to a raw spectral envelope, since Codec2 already
//! reports its own harmonic amplitudes and fundamental directly -- no
//! need to re-infer them the way the original SSB/FM proposal
//! (`SPECTRAL_BRIDGE_AND_COMB_SQUELCH.md`) would have had to.
//!
//! **Why 16kHz, not more 8kHz code.** `Model::new`'s own harmonic-count
//! formula, `l = pi/wo`, means the topmost synthesized harmonic already
//! sits at ~4kHz -- this codec's own Nyquist frequency -- for *every*
//! real `Wo`: confirmed directly, `Wo=W0_MIN` gives `l=80=MAX_AMP`, and
//! `80*50Hz=4000Hz` exactly. There is no unused frequency room to place
//! new harmonics into at 8kHz; anything "after" the last one aliases.
//! This module instead reruns the *same* additive-sinusoid-via-IFFT
//! synthesis `synthesis.rs` already uses (place each harmonic's complex
//! amplitude into its own FFT bin, mirror to a Hermitian-symmetric
//! spectrum, inverse FFT, Parzen-window, overlap-add), at double
//! FFT/sample-rate scale (`FFT_ENC_SB`=1024, `SAMPLE_RATE_SB`=16000).
//! `FFT_ENC_SB` is exactly `2*FFT_ENC`, chosen specifically so the
//! Hz-per-bin resolution is unchanged (8000/512 == 16000/1024 ==
//! 15.625 Hz/bin) -- a harmonic's bin index is then `round(m*wo/fft_r)`
//! with the *same* `fft_r = TAU/FFT_ENC` (512-point) constant
//! `synthesis.rs` already uses, just no longer clamped to `FFT_ENC/2`,
//! so `m > l`'s own bins land correctly in the newly-available upper
//! half without any new scaling constant.
//!
//! **What's reused unchanged vs. new**: harmonics `1..=l` reuse
//! `model.a[m]`/`model.phi[m]` exactly as the normal 8kHz decode
//! already computed them (including that path's own postfilter
//! decision) -- this module never re-derives them. Harmonics
//! `l+1..=l2` (`l2 = min(2*l, MAX_AMP_SB)`) are new: their amplitude
//! comes from `extrapolate_amplitudes` below, and (since extrapolation
//! only ever runs for voiced sub-frames -- see that function's own doc
//! comment) their phase reuses the *same* deterministic voiced
//! excitation-phase formula `synthesis.rs::synthesize_phase` uses for
//! its own unresolved harmonics (`phi0*m`, tracking the same
//! fundamental phase) -- there's no real envelope *phase* information
//! above 4kHz to reuse, only a magnitude estimate, so this module
//! doesn't invent one. There is no unvoiced counterpart to reuse here:
//! an unvoiced sub-frame never reaches this branch at all.
//!
//! **Amplitude extrapolation**: an ordinary least-squares fit of
//! `ln(a[m])` against harmonic frequency over the top `min(l, 16)`
//! known harmonics (closest to, and most representative of, the real
//! high-frequency trend), extrapolated linearly in log-amplitude
//! (equivalently, exponential decay vs. frequency) out to `l2`. A
//! rising fitted trend is clamped flat (`beta.min(0.0)`) rather than
//! extrapolated, since a natural voice spectrum doesn't get *louder*
//! toward its own high-frequency tail and an unclamped rising fit
//! would be a real, audible defect, not a plausible reconstruction.
//!
//! **Fixed-point port**: `extrapolate_amplitudes_fixed`/
//! `SpectralBridgeStateFixed` below, added as a follow-on once this
//! float v1 was validated -- matching this project's own established
//! "float reference first, fixed-point port as an explicit follow-on"
//! discipline (see `codec2_3200::encoder_fixed`'s own doc comment for
//! the precedent). Base-2 log/exp (`fixed_point::log2_q23`/`exp2_q23`)
//! replace the float fit's natural log throughout -- the same linear
//! regression, just scaled by `ln(2)`, which cancels out entirely since
//! both `alpha`/`beta` and their later use in `exp2_q23` stay in the
//! same base-2 domain end to end. The fit's own `x` (harmonic
//! frequency) is kept as a plain integer Hz, not Q23-scaled, so the OLS
//! accumulators (`sum_xx` in particular) can't overflow `i64` -- see
//! `extrapolate_amplitudes_fixed`'s own doc comment.
//!
//! **Simplified relative to the base 8kHz path, and why**: the
//! extrapolated harmonics skip `postfilter_step`'s own voiced/noise
//! reclassification -- there's no real basis for re-deciding
//! voiced/noise on amplitude values this module itself invented, only
//! for the original, really-decoded ones. A real follow-on, not
//! attempted here: perceptual tuning of the extrapolated band (a
//! gentler rolloff, unvoiced noise texture blended in) once this v1's
//! basic approach is validated against real listening, not just the
//! self-consistency checks this module's own tests run -- there is no
//! real captured 16kHz reference to validate against the way every
//! other stage in this codec was, since the extrapolated content is,
//! by construction, this module's own best guess, not a real
//! transmitted or recoverable signal.
//!
//! **Enable/disable switch**: `SpectralBridgeState::enabled`, on by
//! default per Bruce's own explicit direction. When disabled,
//! `synthesize_subframe_sb` still runs (still produces genuine 16kHz
//! output, still reuses harmonics `1..=l` unchanged) but skips harmonic
//! extrapolation entirely (`l2 = l`), leaving the newly-available
//! 4-8kHz band silent -- a real, if less interesting, "upsample with
//! no invented content" mode, not a different code path to maintain.
//! Extrapolation is also always skipped for unvoiced sub-frames even
//! when enabled -- see `extrapolate_amplitudes`'s own doc comment.

use super::envelope::{synth_k_q23, Model, ModelFixed};
use super::fixed_fft::{fft_fixed, rshift_round_i128, ComplexQ23};
use super::fixed_point::{exp2_q23, f32_to_q_exact_round, log2_q23};
use super::synthesis::{ear_protection, ear_protection_fixed, phase_increment_q32};
use super::trig_fixed::sin_cos_q23;
use super::{FFT_ENC, MAX_AMP, N_SAMP, SAMPLE_RATE};
use rustfft::num_complex::Complex32;
use rustfft::{Fft, FftPlanner};
use std::sync::{Arc, OnceLock};

const FRAC_BITS: u32 = 23;

/// Doubled bandwidth (16kHz) relative to the base codec's own 8kHz.
pub const SAMPLE_RATE_SB: u32 = 2 * SAMPLE_RATE;
/// One 10ms sub-frame at `SAMPLE_RATE_SB` -- still 10ms of real time
/// per sub-frame, just twice the samples (matches the base codec's own
/// `N_SAMP` cadence, not a separate framing decision).
pub const N_SAMP_SB: usize = 2 * N_SAMP;
pub const SAMPLES_PER_FRAME_SB: usize = 2 * N_SAMP_SB;
pub const TW_SB: usize = 2 * super::TW;
/// `2*FFT_ENC`, chosen specifically to keep Hz-per-bin unchanged (see
/// this module's own doc comment) -- not an independent tuning choice.
pub const FFT_ENC_SB: usize = 2 * FFT_ENC;
/// `2*MAX_AMP`: the most harmonics `extrapolate_amplitudes` could ever
/// need room for (`l2 = min(2*l, MAX_AMP_SB)`, and `l` itself is
/// already capped at `MAX_AMP`).
pub const MAX_AMP_SB: usize = 2 * MAX_AMP;

fn make_synthesis_window_sb() -> [f32; SAMPLES_PER_FRAME_SB] {
    let mut pn = [0.0f32; SAMPLES_PER_FRAME_SB];
    let n0 = N_SAMP_SB / 2;
    let n1 = 3 * N_SAMP_SB / 2;
    let inv_2tw = 1.0 / (2.0 * TW_SB as f32);
    for (i, v) in pn
        .iter_mut()
        .enumerate()
        .take((n0 + TW_SB).min(SAMPLES_PER_FRAME_SB))
        .skip(n0.saturating_sub(TW_SB))
    {
        *v = (i as f32 - (n0 - TW_SB) as f32) * inv_2tw;
    }
    for v in pn.iter_mut().take(n1.saturating_sub(TW_SB)).skip(n0 + TW_SB) {
        *v = 1.0;
    }
    for (i, v) in pn
        .iter_mut()
        .enumerate()
        .take((n1 + TW_SB).min(SAMPLES_PER_FRAME_SB))
        .skip(n1.saturating_sub(TW_SB))
    {
        *v = ((n1 + TW_SB) as f32 - i as f32) * inv_2tw;
    }
    pn
}

/// Extrapolates harmonic amplitudes above `model.l` -- see this
/// module's own doc comment for the fitting method. Returns the
/// extended amplitude array (indices `1..=l2` populated, matching
/// `model.a`'s own convention of leaving index 0 unused) and `l2`
/// itself, the new harmonic count (`model.l` unchanged if `enabled` is
/// false, `model.voiced` is false, or `model.l` is already at
/// `MAX_AMP_SB/2`, i.e. no room to extrapolate into).
///
/// **Voiced-only, deliberately**: the harmonic-series continuation
/// this function computes is well-justified for voiced speech, which
/// really has that harmonic structure -- unvoiced/fricative sounds
/// (s, sh, f) are physically broadband noise, not harmonics, and
/// extrapolating a tonal series into their own high-frequency "sizzle"
/// region has no real listening validation behind it yet. Left off for
/// unvoiced content in this v1 rather than guessed; a real, separate
/// noise-texture treatment for that case is a genuine follow-on, not
/// attempted here.
pub fn extrapolate_amplitudes(model: &Model, enabled: bool) -> ([f32; MAX_AMP_SB + 1], usize) {
    let l = model.l;
    let mut a_ext = [0.0f32; MAX_AMP_SB + 1];
    a_ext[1..=l].copy_from_slice(&model.a[1..=l]);

    let l2 = if enabled && model.voiced { (2 * l).min(MAX_AMP_SB) } else { l };
    if l2 <= l {
        return (a_ext, l);
    }

    let freq_hz = |m: usize| m as f32 * model.wo * SAMPLE_RATE as f32 / std::f32::consts::TAU;

    let k = l.min(16);
    let mut sum_x = 0.0f64;
    let mut sum_y = 0.0f64;
    let mut sum_xx = 0.0f64;
    let mut sum_xy = 0.0f64;
    let n = k as f64;
    for m in (l - k + 1)..=l {
        let a = model.a[m].max(1e-6);
        let x = freq_hz(m) as f64;
        let y = (a as f64).ln();
        sum_x += x;
        sum_y += y;
        sum_xx += x * x;
        sum_xy += x * y;
    }
    let denom = n * sum_xx - sum_x * sum_x;
    let (alpha, beta) = if denom.abs() > 1e-9 {
        let beta = (n * sum_xy - sum_x * sum_y) / denom;
        let alpha = (sum_y - beta * sum_x) / n;
        (alpha, beta)
    } else {
        (sum_y / n, 0.0)
    };
    // A rising trend is non-physical past the known band -- a natural
    // voice spectrum doesn't get louder toward its own high-frequency
    // tail. Clamp to a flat continuation rather than extrapolate an
    // ever-louder tail.
    let beta = beta.min(0.0);

    #[allow(clippy::needless_range_loop)]
    for m in (l + 1)..=l2 {
        let x = freq_hz(m) as f64;
        let predicted = alpha + beta * x;
        a_ext[m] = predicted.exp() as f32;
    }
    (a_ext, l2)
}

/// Persistent per-decoder Spectral Bridge synthesis state -- parallel
/// to, and independent of, `synthesis::SynthesisState` (the base 8kHz
/// path is untouched by this module entirely). `ex_phase` is updated
/// with the *exact* formula `synthesize_phase` uses (`wo * N_SAMP`,
/// the *original* `N_SAMP`, not `N_SAMP_SB` -- phase is a physical,
/// sample-rate-independent quantity, and both paths advance it once
/// per 10ms sub-frame from the same `Wo`), so harmonics `1..=l`'s own
/// already-decoded phase and this module's own extrapolated harmonics'
/// phase stay consistent with each other from a shared, if
/// independently tracked, phase origin.
pub struct SpectralBridgeState {
    pub enabled: bool,
    sn_: [f32; SAMPLES_PER_FRAME_SB],
    parzen: [f32; SAMPLES_PER_FRAME_SB],
    ex_phase: f32,
    ifft: Arc<dyn Fft<f32>>,
    ifft_buf: [Complex32; FFT_ENC_SB],
}

impl Default for SpectralBridgeState {
    fn default() -> Self {
        let mut planner = FftPlanner::<f32>::new();
        SpectralBridgeState {
            enabled: true,
            sn_: [0.0; SAMPLES_PER_FRAME_SB],
            parzen: make_synthesis_window_sb(),
            ex_phase: 0.0,
            ifft: planner.plan_fft_inverse(FFT_ENC_SB),
            ifft_buf: [Complex32::new(0.0, 0.0); FFT_ENC_SB],
        }
    }
}

impl SpectralBridgeState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Synthesizes one 10ms (`N_SAMP_SB`-sample) sub-frame at
    /// `SAMPLE_RATE_SB`, given the *already-decoded* base `model`
    /// (unchanged, same object the normal 8kHz decode produced).
    pub fn synthesize_subframe_sb(&mut self, model: &Model) -> [i16; N_SAMP_SB] {
        let (a_ext, l2) = extrapolate_amplitudes(model, self.enabled);

        self.ex_phase += model.wo * N_SAMP as f32;
        self.ex_phase -=
            std::f32::consts::TAU * (self.ex_phase / std::f32::consts::TAU + 0.5).floor();
        let phi0 = self.ex_phase;

        self.sn_.copy_within(N_SAMP_SB.., 0);
        self.sn_[N_SAMP_SB - 1] = 0.0;

        for c in self.ifft_buf.iter_mut() {
            *c = Complex32::new(0.0, 0.0);
        }
        let fft_r = std::f32::consts::TAU / FFT_ENC as f32;
        #[allow(clippy::needless_range_loop)]
        for m in 1..=l2 {
            let b = (((m as f32 * model.wo / fft_r) + 0.5) as usize).min(FFT_ENC_SB / 2 - 1);
            self.ifft_buf[b] = if m <= model.l {
                // Reuse the original decode's own already-filtered,
                // already-postfiltered phase/amplitude unchanged.
                model.phi[m] * model.a[m]
            } else {
                // extrapolate_amplitudes only ever returns l2 > model.l
                // (reaching this branch at all) when model.voiced is
                // true -- see its own doc comment. No real envelope
                // phase exists above 4kHz, only the deterministic
                // voiced excitation phase (same formula as the base
                // path's own synthesize_phase).
                debug_assert!(model.voiced, "extrapolated harmonics require a voiced model");
                let (s, c) = (phi0 * m as f32).sin_cos();
                Complex32::new(c, s) * a_ext[m]
            };
        }
        for k in 1..(FFT_ENC_SB / 2) {
            self.ifft_buf[FFT_ENC_SB - k] = self.ifft_buf[k].conj();
        }

        self.ifft.process(&mut self.ifft_buf);

        #[allow(clippy::needless_range_loop)]
        for i in 0..(N_SAMP_SB - 1) {
            self.sn_[i] += self.ifft_buf[FFT_ENC_SB - N_SAMP_SB + 1 + i].re * self.parzen[i];
        }
        #[allow(clippy::needless_range_loop)]
        for j in 0..(N_SAMP_SB + 1) {
            let idx = N_SAMP_SB - 1 + j;
            if idx < SAMPLES_PER_FRAME_SB {
                self.sn_[idx] = self.ifft_buf[j].re * self.parzen[N_SAMP_SB - 1 + j];
            }
        }

        let mut out: [f32; N_SAMP_SB] = std::array::from_fn(|i| self.sn_[i]);
        ear_protection(&mut out);

        std::array::from_fn(|i| out[i].clamp(-32767.0, 32767.0) as i16)
    }
}

fn hz_per_rad_q23() -> i64 {
    static V: OnceLock<i64> = OnceLock::new();
    *V.get_or_init(|| f32_to_q_exact_round(SAMPLE_RATE as f32 / std::f32::consts::TAU, FRAC_BITS))
}

/// `freq_hz(m)` (see the float `extrapolate_amplitudes`'s own closure)
/// as a plain integer Hz value, deliberately *not* Q23-scaled --
/// `extrapolate_amplitudes_fixed`'s OLS fit uses this as its own `x`,
/// and keeping it a small plain integer (at most `SAMPLE_RATE_SB/2`)
/// rather than Q23-scaled keeps every accumulator (`sum_xx` most of
/// all) comfortably within `i64` with no widening needed there.
fn freq_hz_fixed(m: usize, wo_q23: i64) -> i64 {
    let mw_q23 = m as i64 * wo_q23;
    let freq_hz_q23 = rshift_round_i128(mw_q23 as i128 * hz_per_rad_q23() as i128, FRAC_BITS);
    rshift_round_i128(freq_hz_q23 as i128, FRAC_BITS)
}

/// Round-to-nearest division for the OLS fit's own two divisions
/// (`beta`, `alpha`) -- same shape `lpc.rs`'s own `div_round_i128`
/// establishes, duplicated locally rather than threaded through as
/// `pub(crate)` (matching this project's own precedent for small,
/// self-contained per-module helpers, e.g. `codec2_1600::fallback_lsp`).
/// `d` is always positive here: an OLS `denom` is a variance sum
/// (`n*sum_xx - sum_x^2 >= 0` by Cauchy-Schwarz) and `n` is a plain
/// positive harmonic count.
fn div_round_i128(n: i128, d: i128) -> i64 {
    debug_assert!(d > 0, "div_round_i128: divisor must be positive, got {d}");
    let half = d / 2;
    (if n >= 0 { (n + half) / d } else { (n - half) / d }) as i64
}

/// Fixed-point sibling of `extrapolate_amplitudes` -- see this module's
/// own doc comment for the base-2-log reasoning. `y` (log2-amplitude)
/// stays Q23 throughout (`log2_q23`'s own output format), so `beta`
/// comes out of the fit already in "Q23 log2-amplitude per Hz" units:
/// multiplying it by a plain integer Hz `x` again needs no further
/// rescale to get a Q23 result, and neither does adding it to `alpha`
/// (also Q23) -- unlike a Q23*Q23 product, there's no `FRAC_BITS`
/// shift anywhere in this function.
pub(crate) fn extrapolate_amplitudes_fixed(
    model: &ModelFixed,
    enabled: bool,
) -> ([i64; MAX_AMP_SB + 1], usize) {
    let l = model.l;
    let mut a_ext = [0i64; MAX_AMP_SB + 1];
    a_ext[1..=l].copy_from_slice(&model.a[1..=l]);

    let l2 = if enabled && model.voiced { (2 * l).min(MAX_AMP_SB) } else { l };
    if l2 <= l {
        return (a_ext, l);
    }

    let k = l.min(16);
    let mut sum_x: i64 = 0;
    let mut sum_y: i64 = 0;
    let mut sum_xx: i64 = 0;
    let mut sum_xy: i64 = 0;
    for m in (l - k + 1)..=l {
        let a_q23 = model.a[m].max(1);
        let x = freq_hz_fixed(m, model.wo);
        let y = log2_q23(a_q23);
        sum_x += x;
        sum_y += y;
        sum_xx += x * x;
        sum_xy += x * y;
    }
    let n = k as i64;
    let denom = n * sum_xx - sum_x * sum_x;
    let (alpha_q23, beta_q23_per_hz) = if denom != 0 {
        let beta = div_round_i128((n * sum_xy - sum_x * sum_y) as i128, denom as i128);
        let alpha = div_round_i128((sum_y - beta * sum_x) as i128, n as i128);
        (alpha, beta)
    } else {
        (div_round_i128(sum_y as i128, n as i128), 0)
    };
    // Same non-rising clamp the float version applies, for the same
    // reason (see extrapolate_amplitudes's own doc comment).
    let beta_q23_per_hz = beta_q23_per_hz.min(0);

    #[allow(clippy::needless_range_loop)]
    for m in (l + 1)..=l2 {
        let x = freq_hz_fixed(m, model.wo);
        let delta_q23 = beta_q23_per_hz as i128 * x as i128;
        debug_assert!(
            delta_q23 >= i64::MIN as i128 && delta_q23 <= i64::MAX as i128,
            "extrapolate_amplitudes_fixed: beta*x={delta_q23} doesn't fit i64"
        );
        let predicted_q23 = alpha_q23 + delta_q23 as i64;
        a_ext[m] = exp2_q23(predicted_q23);
    }
    (a_ext, l2)
}

fn parzen_window_sb_q23() -> &'static [i64; SAMPLES_PER_FRAME_SB] {
    static V: OnceLock<[i64; SAMPLES_PER_FRAME_SB]> = OnceLock::new();
    V.get_or_init(|| {
        let pn = make_synthesis_window_sb();
        std::array::from_fn(|i| f32_to_q_exact_round(pn[i], FRAC_BITS))
    })
}

/// Fixed-point sibling of `SpectralBridgeState` -- genuinely integer
/// end to end (no `f32` until the final `i16` PCM conversion, same
/// "integer core, float boundary" pattern `DecoderFixed` establishes
/// elsewhere in this crate). No `rng`/unvoiced excitation-phase branch
/// here at all, matching `synthesize_subframe_sb`'s own (post-review)
/// shape -- extrapolation only ever runs voiced.
pub(crate) struct SpectralBridgeStateFixed {
    pub(crate) enabled: bool,
    sn_: [i64; SAMPLES_PER_FRAME_SB],
    parzen: [i64; SAMPLES_PER_FRAME_SB],
    ex_phase_q32: u32,
    ifft_re: [i64; FFT_ENC_SB],
    ifft_im: [i64; FFT_ENC_SB],
}

impl Default for SpectralBridgeStateFixed {
    fn default() -> Self {
        SpectralBridgeStateFixed {
            enabled: true,
            sn_: [0; SAMPLES_PER_FRAME_SB],
            parzen: *parzen_window_sb_q23(),
            ex_phase_q32: 0,
            ifft_re: [0; FFT_ENC_SB],
            ifft_im: [0; FFT_ENC_SB],
        }
    }
}

impl SpectralBridgeStateFixed {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Fixed-point `synthesize_subframe_sb`. `k_q23 = synth_k_q23(model.wo)`
    /// is the *same* bin-index scaling `synthesize_subframe_fixed`
    /// already uses (built from the original `FFT_ENC`=512, not
    /// `FFT_ENC_SB` -- see this module's own doc comment on why the bin
    /// formula stays anchored to the original constant), just no longer
    /// clamped to `FFT_ENC/2`.
    pub(crate) fn synthesize_subframe_sb_fixed(&mut self, model: &ModelFixed) -> [i16; N_SAMP_SB] {
        let (a_ext, l2) = extrapolate_amplitudes_fixed(model, self.enabled);

        let increment = phase_increment_q32(model.wo);
        self.ex_phase_q32 = self.ex_phase_q32.wrapping_add(increment);
        let phi0_q32 = self.ex_phase_q32;

        self.sn_.copy_within(N_SAMP_SB.., 0);
        self.sn_[N_SAMP_SB - 1] = 0;

        for i in 0..FFT_ENC_SB {
            self.ifft_re[i] = 0;
            self.ifft_im[i] = 0;
        }

        let k_q23 = synth_k_q23(model.wo);
        #[allow(clippy::needless_range_loop)]
        for m in 1..=l2 {
            let raw = m as i64 * k_q23;
            let b = (((raw + (1i64 << 22)) >> 23) as usize).min(FFT_ENC_SB / 2 - 1);
            let bin = if m <= model.l {
                model.phi[m].mul(ComplexQ23 { re: model.a[m], im: 0 })
            } else {
                debug_assert!(model.voiced, "extrapolated harmonics require a voiced model");
                let angle_m_q32 = ((phi0_q32 as u64 * m as u64) & 0xFFFF_FFFF) as u32;
                sin_cos_q23(angle_m_q32).mul(ComplexQ23 { re: a_ext[m], im: 0 })
            };
            self.ifft_re[b] = bin.re;
            self.ifft_im[b] = bin.im;
        }
        for k in 1..(FFT_ENC_SB / 2) {
            self.ifft_re[FFT_ENC_SB - k] = self.ifft_re[k];
            self.ifft_im[FFT_ENC_SB - k] = -self.ifft_im[k];
        }

        fft_fixed(&mut self.ifft_re, &mut self.ifft_im, false);

        #[allow(clippy::needless_range_loop)]
        for i in 0..(N_SAMP_SB - 1) {
            let re = self.ifft_re[FFT_ENC_SB - N_SAMP_SB + 1 + i];
            self.sn_[i] += ((re as i128 * self.parzen[i] as i128) >> FRAC_BITS) as i64;
        }
        #[allow(clippy::needless_range_loop)]
        for j in 0..(N_SAMP_SB + 1) {
            let idx = N_SAMP_SB - 1 + j;
            if idx < SAMPLES_PER_FRAME_SB {
                let re = self.ifft_re[j];
                self.sn_[idx] = ((re as i128 * self.parzen[idx] as i128) >> FRAC_BITS) as i64;
            }
        }

        let mut out: [i64; N_SAMP_SB] = std::array::from_fn(|i| self.sn_[i]);
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

    fn synthetic_model(wo: f32, voiced: bool, tilt_db_per_khz: f32) -> Model {
        let mut model = Model::new(wo, voiced);
        let freq_hz = |m: usize| m as f32 * wo * SAMPLE_RATE as f32 / std::f32::consts::TAU;
        for m in 1..=model.l {
            let db = -tilt_db_per_khz * (freq_hz(m) / 1000.0);
            model.a[m] = 1000.0 * 10f32.powf(db / 20.0);
            model.phi[m] = Complex32::new(1.0, 0.0);
        }
        model
    }

    /// Fixed-point sibling of `synthetic_model` -- same tilt formula,
    /// same construction, so `extrapolate_amplitudes_fixed`'s own
    /// output can be cross-checked directly against
    /// `extrapolate_amplitudes`'s float output on matching input.
    fn synthetic_model_fixed(wo: f32, voiced: bool, tilt_db_per_khz: f32) -> ModelFixed {
        let wo_q23 = f32_to_q_exact_round(wo, FRAC_BITS);
        let mut model = ModelFixed::new(wo_q23, voiced);
        let freq_hz = |m: usize| m as f32 * wo * SAMPLE_RATE as f32 / std::f32::consts::TAU;
        for m in 1..=model.l {
            let db = -tilt_db_per_khz * (freq_hz(m) / 1000.0);
            let amp = 1000.0 * 10f32.powf(db / 20.0);
            model.a[m] = f32_to_q_exact_round(amp, FRAC_BITS);
            model.phi[m] = ComplexQ23 { re: 1i64 << FRAC_BITS, im: 0 };
        }
        model
    }

    /// Cross-validates `extrapolate_amplitudes_fixed` against the float
    /// `extrapolate_amplitudes` on matching input -- same discipline
    /// `codec2_1600::lsp_quantiser`'s own fixed-vs-float cross-check
    /// uses (measure the real max error, then set the tolerance with
    /// real margin, never guessed).
    #[test]
    fn extrapolate_amplitudes_fixed_matches_the_float_version_within_tolerance() {
        let wo = 180.0f32.to_radians().max(super::super::W0_MIN);
        let model_f = synthetic_model(wo, true, 9.0);
        let model_x = synthetic_model_fixed(wo, true, 9.0);
        assert_eq!(model_f.l, model_x.l, "float/fixed Model::new disagree on l for this wo");

        let (a_ext_f, l2_f) = extrapolate_amplitudes(&model_f, true);
        let (a_ext_x, l2_x) = extrapolate_amplitudes_fixed(&model_x, true);
        assert_eq!(l2_f, l2_x, "float/fixed extrapolation disagree on l2");

        let mut max_rel_err = 0.0f32;
        #[allow(clippy::needless_range_loop)]
        for m in (model_f.l + 1)..=l2_f {
            let got = a_ext_x[m] as f32 / (1i64 << FRAC_BITS) as f32;
            let want = a_ext_f[m];
            max_rel_err = max_rel_err.max((got - want).abs() / want.max(1e-6));
        }
        println!("extrapolate_amplitudes_fixed vs float: max_rel_err={max_rel_err}");
        assert!(
            max_rel_err < 1e-4,
            "extrapolate_amplitudes_fixed diverged from the float version by {max_rel_err} (relative, measured 6.6e-7 when this test was written)"
        );
    }

    #[test]
    fn extrapolated_amplitudes_continue_a_declining_trend_without_exceeding_the_known_top() {
        let model = synthetic_model(super::super::W0_MIN, true, 12.0);
        let (a_ext, l2) = extrapolate_amplitudes(&model, true);
        assert!(l2 > model.l, "expected extrapolation to add harmonics, l={}, l2={l2}", model.l);
        assert_eq!(l2, (2 * model.l).min(MAX_AMP_SB));
        // A real declining trend should extrapolate to a value no
        // louder than the last known harmonic, and never negative/NaN.
        let last_known = model.a[model.l];
        #[allow(clippy::needless_range_loop)]
        for m in (model.l + 1)..=l2 {
            assert!(a_ext[m] >= 0.0, "negative extrapolated amplitude at m={m}");
            assert!(a_ext[m].is_finite(), "non-finite extrapolated amplitude at m={m}");
            assert!(
                a_ext[m] <= last_known * 1.01,
                "extrapolated amplitude at m={m} ({}) exceeds the last known harmonic ({last_known}) -- a declining trend should never get louder",
                a_ext[m]
            );
        }
    }

    #[test]
    fn extrapolation_never_extends_past_max_amp_sb() {
        // Wo=W0_MIN gives l=MAX_AMP already -- 2*l would overflow
        // MAX_AMP_SB's own array bound if not clamped.
        let model = synthetic_model(super::super::W0_MIN, true, 6.0);
        let (_a_ext, l2) = extrapolate_amplitudes(&model, true);
        assert!(l2 <= MAX_AMP_SB, "l2={l2} exceeds MAX_AMP_SB={MAX_AMP_SB}");
    }

    /// Fixed-point sibling of `extrapolation_never_extends_past_max_amp_sb`
    /// -- the same overflow edge (`l=MAX_AMP` so `2*l=MAX_AMP_SB` exactly)
    /// exercised through `extrapolate_amplitudes_fixed`. If the
    /// `.min(MAX_AMP_SB)` clamp there were ever dropped, a release build
    /// would index `a_ext[MAX_AMP_SB+1]` out of bounds.
    #[test]
    fn extrapolation_fixed_never_extends_past_max_amp_sb() {
        // `ModelFixed::new`'s own Q23-quantized `wo` can land `l` one
        // harmonic below the float path's exact `l=MAX_AMP` at
        // `Wo=W0_MIN` (a known, already-documented rounding-boundary
        // discrepancy -- see `envelope.rs`'s own cross-check test), so
        // this sets `l` directly rather than relying on `Wo` alone to
        // reach the real overflow edge: at `l=MAX_AMP`, `2*l=MAX_AMP_SB`
        // exactly, the boundary the `.min(MAX_AMP_SB)` clamp exists for.
        let wo_q23 = f32_to_q_exact_round(super::super::W0_MIN, FRAC_BITS);
        let mut model = ModelFixed::new(wo_q23, true);
        model.l = MAX_AMP;
        let (_a_ext, l2) = extrapolate_amplitudes_fixed(&model, true);
        assert_eq!(l2, MAX_AMP_SB, "expected extrapolation to reach exactly MAX_AMP_SB at l=MAX_AMP");
        assert!(l2 <= MAX_AMP_SB, "l2={l2} exceeds MAX_AMP_SB={MAX_AMP_SB}");
    }

    #[test]
    fn a_flat_spectrum_extrapolates_flat_not_rising() {
        // Zero tilt: the fitted trend should come out ~flat (beta~0),
        // not accidentally rising from floating-point noise in the
        // least-squares fit.
        let model = synthetic_model(150.0f32.to_radians().max(super::super::W0_MIN), true, 0.0);
        let (a_ext, l2) = extrapolate_amplitudes(&model, true);
        let last_known = model.a[model.l];
        #[allow(clippy::needless_range_loop)]
        for m in (model.l + 1)..=l2 {
            assert!(
                a_ext[m] <= last_known * 1.05,
                "flat input extrapolated to a rising trend at m={m}: {} vs known {last_known}",
                a_ext[m]
            );
        }
    }

    #[test]
    fn disabled_spectral_bridge_leaves_l2_at_the_original_harmonic_count() {
        let model = synthetic_model(200.0f32.to_radians().max(super::super::W0_MIN), true, 6.0);
        let (_a_ext, l2) = extrapolate_amplitudes(&model, false);
        assert_eq!(l2, model.l, "disabled Spectral Bridge should not add any harmonics");
    }

    #[test]
    fn unvoiced_subframes_never_extrapolate_even_when_enabled() {
        let model = synthetic_model(200.0f32.to_radians().max(super::super::W0_MIN), false, 6.0);
        let (_a_ext, l2) = extrapolate_amplitudes(&model, true);
        assert_eq!(l2, model.l, "unvoiced sub-frames must not extrapolate, even when enabled=true");
    }

    #[test]
    fn synthesize_subframe_sb_produces_finite_reasonably_scaled_audio_enabled_and_disabled() {
        for enabled in [true, false] {
            let mut sb = SpectralBridgeState::new();
            sb.enabled = enabled;
            let mut sumsq = 0.0f64;
            let mut n_samples = 0u64;
            let mut max_abs = 0i32;
            for frame_idx in 0..40 {
                let wo = super::super::W0_MIN + 0.001 * (frame_idx as f32 * 0.3).sin().abs();
                let model = synthetic_model(wo, frame_idx % 3 != 0, 9.0);
                let out = sb.synthesize_subframe_sb(&model);
                for &s in &out {
                    sumsq += (s as f64) * (s as f64);
                    max_abs = max_abs.max(s.abs() as i32);
                    n_samples += 1;
                }
            }
            let rms = (sumsq / n_samples as f64).sqrt();
            assert!(rms.is_finite() && rms >= 0.0, "enabled={enabled}: non-finite/negative RMS={rms}");
            assert!(max_abs < 32768, "enabled={enabled}: clipped at the i16 boundary");
        }
    }

    #[test]
    fn synthesize_subframe_sb_fixed_produces_finite_reasonably_scaled_audio_enabled_and_disabled() {
        for enabled in [true, false] {
            let mut sb = SpectralBridgeStateFixed::new();
            sb.enabled = enabled;
            let mut sumsq = 0.0f64;
            let mut n_samples = 0u64;
            let mut max_abs = 0i32;
            for frame_idx in 0..40 {
                let wo = super::super::W0_MIN + 0.001 * (frame_idx as f32 * 0.3).sin().abs();
                let model = synthetic_model_fixed(wo, frame_idx % 3 != 0, 9.0);
                let out = sb.synthesize_subframe_sb_fixed(&model);
                for &s in &out {
                    sumsq += (s as f64) * (s as f64);
                    max_abs = max_abs.max(s.abs() as i32);
                    n_samples += 1;
                }
            }
            let rms = (sumsq / n_samples as f64).sqrt();
            assert!(rms.is_finite() && rms >= 0.0, "enabled={enabled}: non-finite/negative RMS={rms}");
            assert!(max_abs < 32768, "enabled={enabled}: clipped at the i16 boundary");
        }
    }

    #[test]
    fn harmonics_up_to_l_are_bit_identical_to_the_base_decode_when_extrapolation_is_disabled() {
        // With extrapolation disabled, the only content placed into the
        // bigger spectrum is exactly model.phi[m]*model.a[m] for
        // m=1..=l -- the same values the base 8kHz decode already
        // computed. This doesn't check the full 16kHz waveform (no
        // real reference exists to compare against), but it does
        // directly check the one real invariant this module's own
        // design promises: it must not alter the base harmonics at all.
        let model = synthetic_model(180.0f32.to_radians().max(super::super::W0_MIN), true, 9.0);
        let (a_ext, l2) = extrapolate_amplitudes(&model, false);
        assert_eq!(l2, model.l);
        #[allow(clippy::needless_range_loop)]
        for m in 1..=model.l {
            assert_eq!(a_ext[m], model.a[m], "harmonic {m} was altered when extrapolation is disabled");
        }
    }
}
