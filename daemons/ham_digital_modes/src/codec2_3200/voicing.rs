// SPDX-License-Identifier: LGPL-3.0-or-later
//! Voiced/unvoiced decision: the classic energy + zero-crossing-rate
//! heuristic (Rabiner & Sambur 1975's baseline endpoint/voicing
//! detector, and the standard first technique in any speech-processing
//! text since) against an adaptively-tracked noise floor.
//!
//! Purely encoder-internal, like `nlp.rs`'s own pitch estimate (see that
//! module's own doc comment, and `mod.rs`'s note on asymmetric
//! interoperability): the transmitted `voiced` bit is just data to a
//! decoder, which has no way to know or care how the encoder derived
//! it. The reference's own voicing decision (`est_voicing_mbe` in
//! `vendor/codec2-mod/src/analysis.c`) is a considerably more elaborate
//! MBE-style spectral-fit-error technique -- not needed here, since this
//! module doesn't need to reproduce its exact decision, only make a
//! reasonable one.

/// Persistent per-encoder voicing-decision state.
pub struct VoicingState {
    /// Adaptively tracked background noise level, dB (updated only on
    /// frames judged unvoiced, the same "don't track during real
    /// speech" idea `synthesis.rs`'s own `bg_est` postfilter constants
    /// are named after, independently applied here on the encoder
    /// side).
    noise_floor_db: f32,
}

impl Default for VoicingState {
    fn default() -> Self {
        VoicingState { noise_floor_db: -20.0 }
    }
}

impl VoicingState {
    pub fn new() -> Self {
        Self::default()
    }
}

/// Fraction of samples where consecutive real samples' sign flips.
/// Periodic (voiced) signals cross zero close to twice per period;
/// broadband noise or fricatives cross far more often.
fn zero_crossing_rate(samples: &[f32]) -> f32 {
    let crossings = samples.windows(2).filter(|w| (w[0] >= 0.0) != (w[1] >= 0.0)).count();
    crossings as f32 / (samples.len() - 1) as f32
}

/// Zero-crossing rate above which a frame is judged unvoiced regardless
/// of energy -- broadband/fricative energy crosses zero often; a clean
/// low-pitched tone crosses twice per period (e.g. ~0.025 at 100Hz/8kHz),
/// so this comfortably separates the two even at high pitch (~0.1 at
/// 400Hz).
const ZCR_THRESH: f32 = 0.15;
/// A frame must exceed the tracked noise floor by this many dB to count
/// as voiced -- keeps steady background noise from getting voiced just
/// because it's briefly quieter/louder than its own recent average.
const MARGIN_DB: f32 = 12.0;
/// Noise-floor EMA update rate (applied only on unvoiced frames).
const NOISE_BETA: f32 = 0.05;

/// Decides whether the newest `N_SAMP` samples (a 10ms sub-frame) are
/// voiced, updating `state`'s noise-floor tracking.
pub fn is_voiced(state: &mut VoicingState, samples: &[f32]) -> bool {
    let energy: f32 = samples.iter().map(|&s| s * s).sum::<f32>() / samples.len() as f32;
    let energy_db = 10.0 * energy.max(1e-9).log10();
    let zcr = zero_crossing_rate(samples);

    let voiced = energy_db > state.noise_floor_db + MARGIN_DB && zcr < ZCR_THRESH;

    if !voiced {
        state.noise_floor_db = state.noise_floor_db * (1.0 - NOISE_BETA) + energy_db * NOISE_BETA;
    }

    voiced
}

#[cfg(test)]
mod tests {
    use super::*;

    fn synthetic_tone(f0_hz: f32, amp: f32, n: usize, sample_rate: f32) -> Vec<f32> {
        (0..n).map(|i| amp * (std::f32::consts::TAU * f0_hz * i as f32 / sample_rate).sin()).collect()
    }

    fn white_noise(amp: f32, n: usize, seed: &mut u32) -> Vec<f32> {
        (0..n)
            .map(|_| {
                *seed = seed.wrapping_mul(1103515245).wrapping_add(12345);
                let u = (*seed >> 8) as f32 / (1u32 << 24) as f32; // [0,1)
                amp * (u * 2.0 - 1.0)
            })
            .collect()
    }

    #[test]
    fn a_clean_low_pitched_tone_is_voiced_once_the_noise_floor_settles() {
        let mut state = VoicingState::new();
        let silence = vec![0.0f32; 80];
        for _ in 0..10 {
            is_voiced(&mut state, &silence);
        }
        let tone = synthetic_tone(150.0, 8000.0, 80, 8000.0);
        assert!(is_voiced(&mut state, &tone), "a clean 150Hz tone at real speech amplitude should be judged voiced");
    }

    #[test]
    fn white_noise_is_not_voiced() {
        let mut state = VoicingState::new();
        let mut seed = 42u32;
        let silence = vec![0.0f32; 80];
        for _ in 0..10 {
            is_voiced(&mut state, &silence);
        }
        let noise = white_noise(8000.0, 80, &mut seed);
        assert!(!is_voiced(&mut state, &noise), "broadband noise should not be judged voiced");
    }

    #[test]
    fn silence_is_not_voiced() {
        let mut state = VoicingState::new();
        let silence = vec![0.0f32; 80];
        for _ in 0..10 {
            assert!(!is_voiced(&mut state, &silence));
        }
    }

    #[test]
    fn a_quiet_tone_well_below_a_loud_settled_noise_floor_is_not_voiced() {
        // The margin-above-noise-floor check should reject a tone that's
        // real and periodic but too quiet relative to a noisier
        // environment to plausibly be speech rather than a residual tone
        // in the background itself.
        let mut state = VoicingState::new();
        let mut seed = 7u32;
        for _ in 0..15 {
            let noise = white_noise(4000.0, 80, &mut seed);
            is_voiced(&mut state, &noise);
        }
        let quiet_tone = synthetic_tone(150.0, 100.0, 80, 8000.0);
        assert!(!is_voiced(&mut state, &quiet_tone), "a tone too quiet relative to the noise floor should not be voiced");
    }
}
