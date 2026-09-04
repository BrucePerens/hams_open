// SPDX-License-Identifier: LGPL-3.0-or-later
//! The real, forward-looking fixed-point encoder --
//! `docs/references/FIXED_POINT_ENCODER_IMPLEMENTATION_PUNCH_LIST.md`'s
//! own tracked build, one stage at a time, checked against
//! `floating_reference::Encoder` (the original, fully-`f32` pipeline,
//! kept live specifically to serve as that per-frame diff reference --
//! Bruce's own recorded product decision).
//!
//! **Honest current state, not aspirational**: the whole windowing ->
//! `autocorrelate` -> Levinson-Durbin -> `lpc_energy` -> bandwidth-
//! expansion chain (`lpc::autocorrelate_fixed`, `lpc::apply_white_
//! noise_correction_fixed`, `lpc::levinson_durbin_fixed_from_integer_r`,
//! `lpc::lpc_energy_fixed`, `lpc::apply_bw_gamma_fixed`), plus
//! `voicing::is_voiced_fixed`, are genuinely fixed-point today -- no
//! `f32` touches the LPC coefficient or energy estimate anywhere in this
//! struct, arguably the most numerically fragile part of this whole
//! codec. `nlp::nlp` (the pitch estimator) still converts `sn` (this
//! struct's own real `i16`-native sample history -- no `f32` storage
//! here, unlike `floating_reference::Encoder`) to `f32` on the fly,
//! since its own fixed-point FFT hasn't been built yet (a real,
//! separate, much larger piece of work -- see the punch list).
//! `lpc_to_lsp`'s own input boundary (`build_p_q`) and everything after
//! it (`quantise::encode_wo`/`encode_energy`/`encode_lsps_delta_scalar`)
//! hasn't migrated at all. Re-derive this file's own real state directly
//! by reading `encode()` below before trusting this comment -- per this
//! project's own "keep-working-until-actually-done" discipline, a status
//! claim can go stale the moment the next stage lands and this comment
//! isn't updated to match.

use super::{fallback_lsp, lpc, nlp, quantise, voicing, window};
use super::{bits, BYTES_PER_FRAME, E_BITS, M_PITCH, N_SAMP, SAMPLES_PER_FRAME, WO_BITS};

pub struct EncoderFixed {
    /// Raw sample history, `i16`-native -- no `f32` conversion happens
    /// here at all; each not-yet-migrated stage below converts on its
    /// own, at its own call site, so it's visible exactly where the real
    /// float boundary still is.
    sn: [i16; M_PITCH],
    /// `window::make_analysis_window_fixed()`, Q0.30 -- feeds the real,
    /// genuinely fixed-point autocorrelate/Levinson-Durbin chain below.
    window_fixed: [i32; M_PITCH],
    nlp_state: nlp::NlpState,
    voicing_state: voicing::VoicingStateFixed,
}

impl Default for EncoderFixed {
    fn default() -> Self {
        EncoderFixed {
            sn: [0; M_PITCH],
            window_fixed: window::make_analysis_window_fixed(),
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

        // Windowing + autocorrelate + Levinson-Durbin: MIGRATED, genuine
        // fixed-point, no f32 anywhere in this block. `wn_q[i] = sn[i] *
        // window_fixed[i]` is naturally Q30 (an integer sample times a
        // Q0.30 coefficient adds no extra fractional bits of its own);
        // `>> 7` brings it to Q8.23, autocorrelate_fixed's own expected
        // input format (see that function's own doc comment for the
        // real measured margin).
        let mut wn_q = [0i32; M_PITCH];
        for ((w, &s), &win) in wn_q
            .iter_mut()
            .zip(self.sn.iter())
            .zip(self.window_fixed.iter())
        {
            *w = ((s as i64 * win as i64) >> 7) as i32;
        }
        let r_q = lpc::autocorrelate_fixed(&wn_q);
        // White noise correction (fixed point) applies only to
        // Levinson-Durbin's own input -- a separate corrected copy, so
        // lpc_energy_fixed below still reports the real, uncorrected
        // signal energy (matching floating_reference::Encoder's own
        // r_for_levinson pattern).
        let mut r_q_for_levinson = r_q;
        lpc::apply_white_noise_correction_fixed(&mut r_q_for_levinson);
        let (_ak, mut a_q23) = lpc::levinson_durbin_fixed_from_integer_r(&r_q_for_levinson);

        // lpc_energy: now fixed-point too -- no separate float windowing/
        // autocorrelate pass needed anymore. Must run before bw_gamma
        // (matches lpc_energy's own doc comment on real ordering), on
        // the pre-expansion a_q23 -- hence taking it before the
        // bandwidth-expansion step below mutates it in place.
        let e = lpc::lpc_energy_fixed(&a_q23, &r_q);

        // bw_gamma: now fixed-point too (apply_bw_gamma_fixed mutates
        // a_q23 in place); lpc_to_lsp still isn't migrated, so convert
        // to f32 at this boundary -- the established "integer core,
        // float boundary" pattern.
        lpc::apply_bw_gamma_fixed(&mut a_q23);
        let ak: lpc::LpcCoeffs = lpc::dequantize_coef_q23(&a_q23);

        // lpc_to_lsp + quantise::encode_energy/encode_lsps_delta_scalar:
        // NOT migrated yet.
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
    use crate::codec2_3200::floating_reference;
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

    /// The real comparison the parallel-encoder architecture exists
    /// for: does `EncoderFixed`'s own bitstream, decoded, sound like
    /// `floating_reference::Encoder`'s does, on the identical real
    /// input? Not bit-exactness (a quantized 3200bps vocoder was never
    /// going to be that fragile a target, and `mod.rs`'s own doc comment
    /// already establishes encoders have real design freedom in how they
    /// arrive at a quantizer index) -- real, measured correlation between
    /// the two decoded waveforms, matching this crate's own established
    /// methodology for comparing two valid encoders/decoders of the same
    /// signal (see `codec2_3200::tests::decoder_matches_the_real_
    /// reference_decoder_on_a_real_captured_synthetic_signal_bitstream`'s
    /// own use of Pearson correlation for exactly this kind of
    /// comparison). This is the first point in `EncoderFixed`'s own
    /// build where such a comparison is meaningful at all -- before the
    /// windowing/autocorrelate/Levinson-Durbin migration, `EncoderFixed`
    /// and `floating_reference::Encoder` ran identical float code for
    /// everything but voicing, so a high correlation would have proven
    /// nothing.
    #[test]
    fn encoder_fixed_produces_highly_correlated_audio_with_the_float_reference_encoder() {
        let mut fixed_encoder = EncoderFixed::new();
        let mut float_encoder = floating_reference::Encoder::new();
        let mut fixed_decoder = Decoder::new();
        let mut float_decoder = Decoder::new();

        let mut fixed_pcm: Vec<i16> = Vec::new();
        let mut float_pcm: Vec<i16> = Vec::new();

        for frame_idx in 0..40 {
            let f0 = 120.0 + 40.0 * (frame_idx as f32 * 0.3).sin();
            let speech = synthetic_speech_frame(f0, frame_idx * SAMPLES_PER_FRAME);
            let fixed_bits = fixed_encoder.encode(&speech);
            let float_bits = float_encoder.encode(&speech);
            fixed_pcm.extend_from_slice(&fixed_decoder.decode(&fixed_bits));
            float_pcm.extend_from_slice(&float_decoder.decode(&float_bits));
        }

        let n = fixed_pcm.len();
        let mean_a: f64 = fixed_pcm.iter().map(|&s| s as f64).sum::<f64>() / n as f64;
        let mean_b: f64 = float_pcm.iter().map(|&s| s as f64).sum::<f64>() / n as f64;
        let mut cov = 0.0f64;
        let mut var_a = 0.0f64;
        let mut var_b = 0.0f64;
        for i in 0..n {
            let da = fixed_pcm[i] as f64 - mean_a;
            let db = float_pcm[i] as f64 - mean_b;
            cov += da * db;
            var_a += da * da;
            var_b += db * db;
        }
        let corr = cov / (var_a * var_b).sqrt();
        println!("EncoderFixed vs floating_reference::Encoder decoded-audio correlation: {corr}");
        assert!(
            corr > 0.99,
            "EncoderFixed's own bitstream diverged from floating_reference::Encoder's on identical input: correlation={corr} (expected > 0.99) -- since both now run the same windowing/autocorrelate/Levinson-Durbin chain (fixed-point vs float), a large drop here would mean a real bug in the fixed-point migration, not just an expected quantizer-index difference"
        );
    }
}
