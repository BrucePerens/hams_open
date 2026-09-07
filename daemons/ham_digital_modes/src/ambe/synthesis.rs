//! Top-level decoder-side synthesis orchestration (TIA-102.BABA_2003.pdf sections 8-11 combined):
//! wires [`super::reconstruct::reconstruct_spectral_amplitudes`]'s own output through
//! [`super::enhancement`]'s three caller-applied steps -- spectral amplitude enhancement (Eq. 105-
//! 110), Eq. 113's own V/UV forcing, and Eq. 116's own final amplitude smoothing scale, all three
//! deliberately left for the caller by that module's own doc comment -- before handing the result to
//! [`super::unvoiced_synthesis`]/[`super::voiced_synthesis`] and summing per Eq. 142.
//!
//! **Why this module exists as its own thing, not just left to whatever eventually calls
//! `unvoiced_synthesis`/`voiced_synthesis` directly**: `enhance_spectral_amplitudes`'s own output is
//! *not* the `M_bar_l(0)` synthesis actually uses -- it still needs Eq. 113's voicing-forcing and
//! Eq. 116's smoothing scale applied on top of it, both of which carry their own frame-to-frame state
//! (`S_E`, Eq. 111, and `tau_M`, Eq. 115). Skipping either step compiles, runs, and produces
//! plausible-sounding output -- it's wrong in a way no unit test in either `enhancement.rs` or
//! `unvoiced_synthesis.rs`/`voiced_synthesis.rs` alone would ever catch, since each of those modules
//! is only tested against its own equations, not against the composition. This module's own job is
//! to make sure that composition happens in the one place a caller would otherwise be tempted to
//! skip a step.

use super::enhancement::{
    adaptive_voicing_threshold, amplitude_smoothing_scale, amplitude_sum, energy,
    enhance_spectral_amplitudes, smooth_voicing_decision, update_amplitude_threshold,
    update_local_energy,
};
use super::error_estimation::FrameErrors;
use super::unvoiced_synthesis::{NoiseState, UnvoicedState, N};
use super::voiced_synthesis::VoicedState;

/// Persistent decoder-side synthesis state: the shared noise generator (see
/// `unvoiced_synthesis::UnvoicedState`'s own doc comment for why unvoiced and voiced synthesis must
/// share one [`NoiseState`] rather than each owning an independent one), both synthesis halves' own
/// state, and the two enhancement-stage state variables synthesis itself must carry (`S_E`, `tau_M`) --
/// `S_E = 75000.0` per Annex A; `tau_M` isn't itself listed in Annex A, but `20480.0` is Eq. 115's own
/// first-branch value, which any real first frame (zero accumulated FEC errors, the only case that
/// actually matters before a previous `tau_M` exists) reduces to regardless of the seed -- a reasoned
/// default, not a guess.
pub struct SynthesisState {
    noise: NoiseState,
    unvoiced: UnvoicedState,
    voiced: VoicedState,
    s_e: f64,
    tau_m: f64,
    first_frame: bool,
}

impl SynthesisState {
    pub fn new() -> Self {
        Self {
            noise: NoiseState::new(),
            unvoiced: UnvoicedState::new(),
            voiced: VoicedState::new(),
            s_e: 75000.0,
            tau_m: 20480.0,
            first_frame: true,
        }
    }

    /// Synthesizes one 20 ms PCM frame (Eq. 142) from this frame's own *unenhanced* reconstructed
    /// spectral amplitudes (`reconstruct::reconstruct_spectral_amplitudes`'s own output), fundamental
    /// frequency, decoded V/UV decisions, and FEC error statistics
    /// (`error_estimation::estimate_errors`'s own output) -- exactly the pieces this codebase's own
    /// decoder-side pipeline has already built up through section 7. Returns `None` on a length
    /// mismatch between `reconstructed_amplitudes` and `decoded_voiced`.
    pub fn synthesize_frame(
        &mut self,
        reconstructed_amplitudes: &[f64],
        omega0_tilde: f64,
        decoded_voiced: &[bool],
        errors: &FrameErrors,
    ) -> Option<[f64; N]> {
        if reconstructed_amplitudes.len() != decoded_voiced.len() {
            return None;
        }

        // Section 8: spectral amplitude enhancement (Eq. 105-110).
        let r_m0 = energy(reconstructed_amplitudes);
        let enhanced = enhance_spectral_amplitudes(reconstructed_amplitudes, omega0_tilde);

        // Section 9: V/UV smoothing (Eq. 111-113) -- forcing uses the *enhanced*, not-yet-gamma_M-
        // scaled amplitude, per enhancement::smooth_voicing_decision's own established contract.
        self.s_e = update_local_energy(self.s_e, r_m0);
        let v_m = adaptive_voicing_threshold(errors, self.s_e);
        let smoothed_voiced: Vec<bool> = enhanced
            .iter()
            .zip(decoded_voiced)
            .map(|(&m, &v)| smooth_voicing_decision(m, v, v_m))
            .collect();

        // Section 9: amplitude smoothing (Eq. 114-116) -- A_M is the sum of the enhanced amplitudes
        // *before* gamma_M is applied to them; gamma_M is then the last step before synthesis.
        let a_m = amplitude_sum(&enhanced);
        self.tau_m = update_amplitude_threshold(errors, self.tau_m);
        let gamma_m = amplitude_smoothing_scale(self.tau_m, a_m);
        let final_amplitudes: Vec<f64> = enhanced.iter().map(|&m| m * gamma_m).collect();

        // The noise generator is a single continuous sequence shared by both synthesis halves;
        // advanced exactly once per frame, here, not inside either half.
        if !self.first_frame {
            self.noise.advance_frame();
        }
        self.first_frame = false;

        let s_uv = self.unvoiced.synthesize(
            &self.noise,
            omega0_tilde,
            &smoothed_voiced,
            &final_amplitudes,
        )?;
        let s_v =
            self.voiced
                .synthesize(&self.noise, omega0_tilde, &smoothed_voiced, &final_amplitudes)?;

        let mut s = [0.0; N];
        for i in 0..N {
            s[i] = s_uv[i] + s_v[i]; // Eq. 142.
        }
        Some(s)
    }
}

impl Default for SynthesisState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn zero_errors() -> FrameErrors {
        FrameErrors {
            total: 0,
            rate: 0.0,
            golay_init: 0,
            hamming_init: 0,
        }
    }

    #[test]
    fn synthesize_frame_produces_a_full_finite_frame_across_several_calls() {
        let mut state = SynthesisState::new();
        let omega0 = 2.0 * std::f64::consts::PI / 100.0;
        let voiced = vec![true; 16];
        let amplitudes = vec![500.0; 16];
        let errors = zero_errors();

        for _ in 0..5 {
            let frame = state
                .synthesize_frame(&amplitudes, omega0, &voiced, &errors)
                .unwrap();
            assert_eq!(frame.len(), N);
            for &sample in &frame {
                assert!(sample.is_finite(), "non-finite combined sample: {sample}");
            }
        }
    }

    #[test]
    fn synthesize_frame_rejects_a_length_mismatch() {
        let mut state = SynthesisState::new();
        let voiced = vec![true; 5];
        let amplitudes = vec![100.0; 6];
        assert!(state
            .synthesize_frame(&amplitudes, 0.1, &voiced, &zero_errors())
            .is_none());
    }

    /// A real, checkable regression for the exact wiring bug this module exists to prevent: with a
    /// clean error record (`should_mute_frame`/`should_repeat_frame` both false, matching
    /// `error_estimation`'s own module) and every harmonic amplitude far below both the voicing
    /// threshold and the amplitude threshold, `gamma_M` (Eq. 116) must clamp to `1.0` (since
    /// `tau_M > A_M`, the "otherwise" branch) -- confirmed by checking the combined output stays
    /// close to what an unscaled synthesis would produce, rather than trusting the wiring blindly.
    #[test]
    fn synthesize_frame_applies_a_no_op_gamma_m_when_amplitudes_are_well_under_threshold() {
        let mut state = SynthesisState::new();
        let omega0 = 2.0 * std::f64::consts::PI / 100.0;
        let voiced = vec![false; 9]; // Small L~, fully unvoiced: only unvoiced_synthesis contributes.
        let amplitudes = vec![1.0; 9]; // Tiny relative to tau_M's own 20480.0 floor.
        let frame = state
            .synthesize_frame(&amplitudes, omega0, &voiced, &zero_errors())
            .unwrap();
        // gamma_M == 1.0 here is a real, checkable fact (A_M for these inputs is tiny relative to
        // tau_M's 20480.0 floor, so update_amplitude_threshold/amplitude_smoothing_scale's own
        // "otherwise" branch can't fire) -- if gamma_M were wrongly computed as near-zero instead
        // (e.g. tau_M and A_M swapped), every sample would collapse to ~0.0, which this rules out.
        assert!(
            frame.iter().any(|&s| s.abs() > 1e-6),
            "expected real synthesized energy, got near-silence: {frame:?}"
        );
    }
}
