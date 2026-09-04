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
    for (i, v) in pn.iter_mut().enumerate().take((n0 + TW).min(SAMPLES_PER_FRAME)).skip(n0.saturating_sub(TW)) {
        *v = (i as f32 - (n0 - TW) as f32) * inv_2tw;
    }
    for v in pn.iter_mut().take(n1.saturating_sub(TW)).skip(n0 + TW) {
        *v = 1.0;
    }
    for (i, v) in pn.iter_mut().enumerate().take((n1 + TW).min(SAMPLES_PER_FRAME)).skip(n1.saturating_sub(TW)) {
        *v = ((n1 + TW) as f32 - i as f32) * inv_2tw;
    }
    pn
}

/// Simple xorshift PRNG for unvoiced-excitation and postfilter phase
/// randomization -- doesn't need to match the reference's own generator
/// (purely a synthesis-quality detail, not transmitted).
fn next_rand(state: &mut u32) -> f32 {
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
fn synthesize_phase(model: &mut Model, h: &[Complex32; MAX_AMP + 1], ex_phase: &mut f32, rng: &mut u32) {
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
        model.phi[m] = a.im.atan2(a.re + 1e-12);
    }
}

/// Randomizes the phase of harmonics quiet relative to the tracked
/// background-noise level during voiced frames (makes them sound
/// unvoiced/noise-like rather than tonal, closer to real speech's own
/// mixed excitation); tracks that background level from unvoiced
/// frames' own average energy otherwise.
fn postfilter(model: &mut Model, bg_est: &mut f32, rng: &mut u32) {
    let e: f32 = 1e-12 + model.a[1..=model.l].iter().map(|a| a * a).sum::<f32>();
    let e_db = 10.0 * (e / model.l as f32).log10();

    if !model.voiced {
        if e_db < BG_THRESH {
            *bg_est = *bg_est * (1.0 - BG_BETA) + e_db * BG_BETA;
        }
    } else {
        let thresh = 10f32.powf((*bg_est + BG_MARGIN) / 20.0);
        // `model.a` (read) and `model.phi` (written) are sibling fields
        // of the same struct at the same harmonic index -- not a clean
        // fit for `.iter().enumerate()` without destructuring `model`.
        #[allow(clippy::needless_range_loop)]
        for m in 1..=model.l {
            if model.a[m] < thresh {
                model.phi[m] = next_rand(rng);
            }
        }
    }
}

/// Attenuates a whole frame if any sample would exceed a safe int16
/// level -- a defensive measure against bit-error-induced amplitude
/// spikes reaching real ears/speakers, not a normal-operation limiter.
fn ear_protection(samples: &mut [f32]) {
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
            let (s, c) = model.phi[l].sin_cos();
            self.ifft_buf[b] = Complex32::new(model.a[l] * c, model.a[l] * s);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn synthesis_window_peaks_at_1_in_the_middle_and_tapers_toward_0_at_both_ends() {
        let pn = make_synthesis_window();
        assert!((pn[N_SAMP] - 1.0).abs() < 1e-6, "expected peak ~1.0 at the window center, got {}", pn[N_SAMP]);
        assert_eq!(pn[0], 0.0, "the ramp starts exactly at 0 by construction (n0-TW == 0 here)");
        // The ramp only reaches exactly 0 at the one-past-the-end index
        // n1+TW == SAMPLES_PER_FRAME, so the last real sample is one
        // step short of it (1/(2*TW)), not exactly 0.
        let last_step = 1.0 / (2.0 * TW as f32);
        assert!((pn[SAMPLES_PER_FRAME - 1] - last_step).abs() < 1e-6, "expected the last sample one ramp-step above 0 ({last_step}), got {}", pn[SAMPLES_PER_FRAME - 1]);
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
        assert!(max - min > std::f32::consts::TAU * 0.9, "PRNG output didn't spread across the range: min={min} max={max}");
    }
}
