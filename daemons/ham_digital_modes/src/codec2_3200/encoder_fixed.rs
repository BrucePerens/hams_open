// SPDX-License-Identifier: LGPL-3.0-or-later
//! The real, forward-looking fixed-point encoder --
//! `docs/references/FIXED_POINT_ENCODER_IMPLEMENTATION_PUNCH_LIST.md`'s
//! own tracked build, one stage at a time, checked against
//! `floating_reference::Encoder` (the original, fully-`f32` pipeline,
//! kept live specifically to serve as that per-frame diff reference --
//! Bruce's own recorded product decision).
//!
//! **Honest current state, not aspirational**: most of the pipeline
//! below still delegates to the exact same `f32` functions
//! `floating_reference::Encoder` calls, converting `sn` (this struct's
//! own real `i16`-native sample history -- no `f32` storage here, unlike
//! the reference) to `f32` on the fly at each not-yet-migrated stage's
//! own boundary. Only `voicing::is_voiced_fixed` is genuinely
//! fixed-point today. Re-derive this file's own real state directly by
//! reading `encode()` below before trusting this comment -- per this
//! project's own "keep-working-until-actually-done" discipline, a status
//! claim can go stale the moment the next stage lands and this comment
//! isn't updated to match.

use super::{bw_gamma, fallback_lsp, lpc, nlp, quantise, voicing, window};
use super::{bits, BYTES_PER_FRAME, E_BITS, M_PITCH, N_SAMP, SAMPLES_PER_FRAME, WO_BITS};

pub struct EncoderFixed {
    /// Raw sample history, `i16`-native -- no `f32` conversion happens
    /// here at all; each not-yet-migrated stage below converts on its
    /// own, at its own call site, so it's visible exactly where the real
    /// float boundary still is.
    sn: [i16; M_PITCH],
    /// Still the `f32` window table -- `window::make_analysis_window_
    /// fixed()` exists and is validated (see the punch list), but
    /// nothing downstream is ready to consume a fixed-point windowed
    /// sample yet (`autocorrelate` isn't migrated), so wiring it in here
    /// would just add a pointless fixed-to-float round trip. Swap this
    /// the same commit `autocorrelate`/its normalization boundary lands.
    window: [f32; M_PITCH],
    nlp_state: nlp::NlpState,
    voicing_state: voicing::VoicingStateFixed,
}

impl Default for EncoderFixed {
    fn default() -> Self {
        EncoderFixed {
            sn: [0; M_PITCH],
            window: window::make_analysis_window(),
            nlp_state: nlp::NlpState::new(),
            voicing_state: voicing::VoicingStateFixed::new(),
        }
    }
}

impl EncoderFixed {
    pub fn new() -> Self {
        Self::default()
    }

    fn shift_in(&mut self, new_samples: &[i16]) {
        self.sn.copy_within(N_SAMP.., 0);
        self.sn[M_PITCH - N_SAMP..].copy_from_slice(new_samples);
    }

    /// Same real frame structure as `floating_reference::Encoder::
    /// encode` (see that function's own doc comment) -- this is the one
    /// place in the whole crate where migrated and not-yet-migrated
    /// stages sit side by side, so it doubles as the punch list's own
    /// real, checkable status: every `as f32` conversion below marks a
    /// stage still delegating to the float reference.
    pub fn encode(&mut self, speech: &[i16; SAMPLES_PER_FRAME]) -> [u8; BYTES_PER_FRAME] {
        self.shift_in(&speech[..N_SAMP]);

        // nlp::nlp: NOT migrated (needs a real fixed-point FFT, its own
        // separate pass per the punch list) -- converts to f32 here,
        // just for this call.
        let sn_f32: [f32; M_PITCH] = std::array::from_fn(|i| self.sn[i] as f32);
        nlp::nlp(&mut self.nlp_state, &sn_f32);
        // voicing::is_voiced_fixed: migrated, real i16 in, no
        // conversion at all.
        let voiced0 = voicing::is_voiced_fixed(
            &mut self.voicing_state,
            &self.sn[M_PITCH - N_SAMP..],
        );

        self.shift_in(&speech[N_SAMP..]);
        let sn_f32: [f32; M_PITCH] = std::array::from_fn(|i| self.sn[i] as f32);
        let f0 = nlp::nlp(&mut self.nlp_state, &sn_f32);
        let voiced1 = voicing::is_voiced_fixed(
            &mut self.voicing_state,
            &self.sn[M_PITCH - N_SAMP..],
        );

        // quantise::encode_wo: NOT migrated.
        let wo_index = quantise::encode_wo(nlp::f0_to_wo(f0));

        // Windowing + autocorrelate + levinson_durbin + lpc_energy +
        // bw_gamma + lpc_to_lsp + quantise::encode_energy/
        // encode_lsps_delta_scalar: NONE of these are migrated yet --
        // autocorrelate's own normalization boundary is the real gate
        // (see the punch list), so this whole block still runs the
        // identical float path `floating_reference::Encoder` does,
        // converting from `sn_f32` above.
        let mut windowed = [0.0f32; M_PITCH];
        for ((w, &s), &win) in windowed
            .iter_mut()
            .zip(sn_f32.iter())
            .zip(self.window.iter())
        {
            *w = s * win;
        }
        let r = lpc::autocorrelate(&windowed);
        // White noise correction (lpc::apply_white_noise_correction) --
        // a separate copy so lpc_energy still reports the real,
        // uncorrected signal energy.
        let mut r_for_levinson = r;
        lpc::apply_white_noise_correction(&mut r_for_levinson);
        let mut ak = lpc::levinson_durbin(&r_for_levinson);
        let e = lpc::lpc_energy(&ak, &r);
        for (i, a) in ak.iter_mut().enumerate() {
            *a *= bw_gamma(i);
        }
        let lsp = lpc::lpc_to_lsp(&ak).unwrap_or_else(fallback_lsp);

        let e_index = quantise::encode_energy(e);
        let lsp_indexes = quantise::encode_lsps_delta_scalar(&lsp);

        let fields = bits::FrameFields {
            voiced0,
            voiced1,
            wo_index,
            e_index,
            lsp_indexes,
        };
        bits::pack_frame(&fields, WO_BITS, E_BITS)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec2_3200::Decoder;

    fn synthetic_speech_frame(f0: f32, t0: usize) -> [i16; SAMPLES_PER_FRAME] {
        std::array::from_fn(|i| {
            let t = (t0 + i) as f32 / super::super::SAMPLE_RATE as f32;
            let v = 8000.0 * (std::f32::consts::TAU * f0 * t).sin()
                + 3000.0 * (std::f32::consts::TAU * 2.0 * f0 * t).sin();
            v as i16
        })
    }

    /// Not a claim of correctness -- `EncoderFixed` is still mostly the
    /// float pipeline under the hood (see this file's own doc comment).
    /// This is the same basic "produces finite, reasonably-scaled,
    /// non-degenerate audio and doesn't panic" sanity check
    /// `floating_reference`'s own round-trip test runs, kept passing
    /// here as each stage migrates so a regression is caught immediately
    /// rather than discovered later.
    #[test]
    fn encode_decode_round_trip_produces_finite_reasonably_scaled_audio() {
        let mut encoder = EncoderFixed::new();
        let mut decoder = Decoder::new();
        let mut max_abs = 0i32;
        let mut sumsq = 0.0f64;
        let mut n_samples = 0u64;

        for frame_idx in 0..40 {
            let f0 = 120.0 + 40.0 * (frame_idx as f32 * 0.3).sin();
            let speech = synthetic_speech_frame(f0, frame_idx * SAMPLES_PER_FRAME);
            let bits = encoder.encode(&speech);
            let out = decoder.decode(&bits);
            for &s in &out {
                max_abs = max_abs.max(s.abs() as i32);
                sumsq += (s as f64) * (s as f64);
                n_samples += 1;
            }
        }

        let rms = (sumsq / n_samples as f64).sqrt();
        assert!(rms > 50.0, "decoded audio looks like silence, RMS={rms}");
        assert!(rms < 20000.0, "decoded audio implausibly loud, RMS={rms}");
        assert!(max_abs > 0, "decoded audio is all zero");
    }
}
