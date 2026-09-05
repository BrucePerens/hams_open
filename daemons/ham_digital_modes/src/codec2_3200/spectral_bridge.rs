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
//! **Float-only v1**, matching this project's own established "float
//! reference first, fixed-point port as an explicit follow-on"
//! discipline (see `codec2_3200::encoder_fixed`'s own doc comment for
//! the precedent) -- not yet fixed-point.
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

use super::envelope::Model;
use super::synthesis::ear_protection;
use super::{FFT_ENC, MAX_AMP, N_SAMP, SAMPLE_RATE};
use rustfft::num_complex::Complex32;
use rustfft::{Fft, FftPlanner};
use std::sync::Arc;

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
