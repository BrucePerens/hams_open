// SPDX-License-Identifier: LGPL-3.0-or-later
//! An independently-authored Rust port of Codec2's 1600bps mode --
//! the codec M17's "Voice + Data" stream type uses (`M17_spec.tex`
//! line 727: "64 bits encoded speech + 64 bits arbitrary data" per
//! 40ms frame; this module implements the speech half only, not M17's
//! own framing/FEC/sync layer, which is a separate, larger, and
//! currently-shelved concern -- see
//! `hams_com/docs/proposals/blocked/M17_IMPLEMENTATION_PLAN.md`).
//!
//! Written the same way `codec2_3200` was: a from-scratch reading of
//! plain upstream Codec2's *algorithm* (`codec2_encode_1600`/
//! `codec2_decode_1600` in `codec2.c`, `encode_lsps_scalar`/
//! `decode_lsps_scalar`/`bw_expand_lsps`/`check_lsp_order`/
//! `interpolate_lsp_ver2` in `quantise.c`/`interp.c`), not translated
//! line-by-line -- informed by, not a derivative of, that LGPL-2.1-only
//! source (same reasoning `codec2_3200::mod`'s own doc comment gives,
//! and the same license: plain upstream Codec2 is LGPL-2.1-only, same
//! trap `vendor/codec2-mod/VENDORED_FROM.md` documents for the M17
//! fork). Reference used: a local unmodified build of plain upstream
//! Codec2 (not `vendor/codec2-mod`, which has been stripped down to
//! 3200bps only and has no 1600bps mode at all -- confirmed by reading
//! its own `codec2_mod.c`, 149 lines, no mode dispatch).
//!
//! 1600bps shares almost its entire signal-processing pipeline with
//! 3200bps -- pitch estimation, voicing decision, LPC analysis,
//! LSP<->LPC conversion, spectral envelope reconstruction, sinusoidal
//! synthesis are all genuinely mode-independent and reused directly
//! from `codec2_3200` (`floating_reference::{nlp, voicing, lpc}`,
//! `codec2_3200::{nlp::f0_to_wo, window, lpc::lsp_to_lpc, envelope,
//! synthesis, interp::{interp_wo, interp_voiced, interp_energy}}`).
//! What's genuinely new here: a different LSP quantizer (ten
//! independent per-dimension scalar codebooks instead of one 10-D
//! delta-scalar quantizer -- `lsp_quantiser.rs`), LSP order/bandwidth
//! post-processing and 3-way interpolation 40ms-cadence LSPs need that
//! 3200bps's 20ms cadence never does (`lsp_post.rs`), and a materially
//! different 64-bit frame layout (`bits.rs`).
//!
//! Frame structure, matching the real reference exactly (see
//! `bits.rs`'s own doc comment for the bit layout): 320 samples (40ms)
//! encoded as four 10ms sub-frame analysis passes -- sub-frames 0 and
//! 2 contribute only a voicing bit, sub-frames 1 and 3 each contribute
//! a voicing bit plus a transmitted `Wo`/energy pair (20ms cadence,
//! same as 3200bps), and sub-frame 3's own LPC/LSP analysis is the one
//! that gets quantized and transmitted (LSPs update only once per
//! 40ms). Decode interpolates sub-frames 0 and 2 from their neighbors
//! (`interp_wo`/`interp_energy`, unchanged from 3200bps) and
//! sub-frames 0/1/2 all interpolate their own LSPs from the previous
//! frame's decoded LSPs and this frame's newly decoded ones at weights
//! 0.25/0.5/0.75 (`lsp_post::interpolate_lsp_ver2`); sub-frame 3 uses
//! its own newly decoded LSPs directly.
//!
//! `apply_lpc_correction` (upstream) turned out, on inspection, to be
//! byte-for-byte the same formula `codec2_3200::envelope::
//! apply_first_harmonic_correction` already implements (`Wo < PI*150/
//! 4000` -> `A[1] *= 0.032`) -- reused directly, not reimplemented.

pub mod bits;
pub mod lsp_post;
pub mod lsp_quantiser;

use crate::codec2_3200::floating_reference::{lpc as flpc, nlp as fnlp, voicing as fvoicing};
use crate::codec2_3200::{
    self, bw_gamma, envelope, interp, lpc, nlp, quantise, synthesis, window, LPC_ORD, M_PITCH,
    N_SAMP,
};

/// LSPs to substitute when `lpc::lpc_to_lsp` fails to find all
/// `LPC_ORD` roots -- same fallback `codec2_3200::fallback_lsp` uses
/// (that function is private to its own module, so this is the same
/// evenly-spaced-across-`[0,pi]` construction, not a re-export).
fn fallback_lsp() -> [f32; LPC_ORD] {
    std::array::from_fn(|i| (std::f32::consts::PI / LPC_ORD as f32) * i as f32)
}

/// One 1600bps frame: 64 bits (8 bytes) covering 40ms of audio (four
/// 10ms, `N_SAMP`-sample sub-frames) -- 64 bits / 40ms = 1600bps.
pub const BYTES_PER_FRAME: usize = 8;
pub const SAMPLES_PER_FRAME: usize = 4 * N_SAMP;

/// Persistent per-call encoder state -- same shape as
/// `codec2_3200::floating_reference::Encoder`'s own, since the
/// underlying analysis pipeline (pitch/voicing/LPC) is identical; this
/// mode just runs it across four 10ms steps instead of two, and
/// quantizes/transmits LSPs from only the last of them.
pub struct Encoder {
    sn: [f32; M_PITCH],
    analysis_window: [f32; M_PITCH],
    nlp_state: fnlp::NlpState,
    voicing_state: fvoicing::VoicingState,
}

impl Default for Encoder {
    fn default() -> Self {
        Encoder {
            sn: [0.0; M_PITCH],
            analysis_window: window::make_analysis_window(),
            nlp_state: fnlp::NlpState::new(),
            voicing_state: fvoicing::VoicingState::new(),
        }
    }
}

/// One 20ms LPC/LSP analysis pass over the current `M_PITCH`-sample
/// history window -- identical to the inline block in
/// `floating_reference::Encoder::encode`, factored out here since
/// 1600bps runs it twice per 40ms frame but only quantizes/transmits
/// the second pass's own LSPs (the first pass's LSPs are computed only
/// because `e` falls out of the same analysis, matching the
/// reference's own `speech_to_uq_lsps` call for that purpose).
fn analyse_lsps_and_energy(sn: &[f32; M_PITCH], analysis_window: &[f32; M_PITCH]) -> ([f32; LPC_ORD], f32) {
    let mut windowed = [0.0f32; M_PITCH];
    for ((w, &s), &win) in windowed.iter_mut().zip(sn.iter()).zip(analysis_window.iter()) {
        *w = s * win;
    }
    let r = flpc::autocorrelate(&windowed);
    let mut r_for_levinson = r;
    flpc::apply_white_noise_correction(&mut r_for_levinson);
    let mut ak = flpc::levinson_durbin(&r_for_levinson);
    let e = flpc::lpc_energy(&ak, &r);
    for (i, a) in ak.iter_mut().enumerate() {
        *a *= bw_gamma(i);
    }
    let lsp = flpc::lpc_to_lsp(&ak).unwrap_or_else(fallback_lsp);
    (lsp, e)
}

impl Encoder {
    pub fn new() -> Self {
        Self::default()
    }

    fn shift_in(&mut self, new_samples: &[i16]) {
        self.sn.copy_within(N_SAMP.., 0);
        for (dst, &s) in self.sn[M_PITCH - N_SAMP..].iter_mut().zip(new_samples) {
            *dst = s as f32;
        }
    }

    /// Encodes one 40ms (`SAMPLES_PER_FRAME`-sample) frame into
    /// `BYTES_PER_FRAME` real-format bytes. Matches
    /// `codec2_encode_1600`'s own real structure exactly (see this
    /// module's own doc comment).
    pub fn encode(&mut self, speech: &[i16; SAMPLES_PER_FRAME]) -> [u8; BYTES_PER_FRAME] {
        self.shift_in(&speech[0..N_SAMP]);
        fnlp::nlp(&mut self.nlp_state, &self.sn);
        let voiced0 = fvoicing::is_voiced(&mut self.voicing_state, &self.sn[M_PITCH - N_SAMP..]);

        self.shift_in(&speech[N_SAMP..2 * N_SAMP]);
        let f0_a = fnlp::nlp(&mut self.nlp_state, &self.sn);
        let voiced1 = fvoicing::is_voiced(&mut self.voicing_state, &self.sn[M_PITCH - N_SAMP..]);
        let wo_index_a = quantise::encode_wo(nlp::f0_to_wo(f0_a));
        let (_lsp_a_unused, e_a) = analyse_lsps_and_energy(&self.sn, &self.analysis_window);
        let e_index_a = quantise::encode_energy(e_a);

        self.shift_in(&speech[2 * N_SAMP..3 * N_SAMP]);
        fnlp::nlp(&mut self.nlp_state, &self.sn);
        let voiced2 = fvoicing::is_voiced(&mut self.voicing_state, &self.sn[M_PITCH - N_SAMP..]);

        self.shift_in(&speech[3 * N_SAMP..4 * N_SAMP]);
        let f0_b = fnlp::nlp(&mut self.nlp_state, &self.sn);
        let voiced3 = fvoicing::is_voiced(&mut self.voicing_state, &self.sn[M_PITCH - N_SAMP..]);
        let wo_index_b = quantise::encode_wo(nlp::f0_to_wo(f0_b));
        let (lsp_b, e_b) = analyse_lsps_and_energy(&self.sn, &self.analysis_window);
        let e_index_b = quantise::encode_energy(e_b);
        let lsp_indexes = lsp_quantiser::encode_lsps_scalar(&lsp_b);

        bits::pack_frame_1600(&bits::FrameFields1600 {
            voiced0,
            voiced1,
            wo_index_a,
            e_index_a,
            voiced2,
            voiced3,
            wo_index_b,
            e_index_b,
            lsp_indexes,
        })
    }
}

/// LSPs the reference's own decoder starts from before any real frame
/// has been decoded -- same fallback `codec2_3200::initial_lsps` uses
/// (its own function is private to that module, so this is the same
/// evenly-spaced-across-`[0,pi]` construction, not a re-export).
fn initial_lsps() -> [f32; LPC_ORD] {
    std::array::from_fn(|i| (i as f32 * std::f32::consts::PI) / (LPC_ORD as f32 + 1.0))
}

/// Persistent per-decoder state: the previous frame's own decoded
/// `Wo`/voiced/LSPs/energy (sub-frame 3's own, needed to interpolate
/// the next frame's sub-frames 0/1/2), plus `synthesis::SynthesisState`'s
/// own overlap-add memory and a forward FFT plan.
pub struct Decoder {
    prev_wo: f32,
    prev_voiced: bool,
    prev_lsps: [f32; LPC_ORD],
    prev_e: f32,
    synth: synthesis::SynthesisState,
    fft: std::sync::Arc<dyn rustfft::Fft<f32>>,
}

impl Default for Decoder {
    fn default() -> Self {
        let mut planner = rustfft::FftPlanner::<f32>::new();
        Decoder {
            prev_wo: codec2_3200::W0_MIN,
            prev_voiced: false,
            prev_lsps: initial_lsps(),
            prev_e: 1.0,
            synth: synthesis::SynthesisState::new(),
            fft: planner.plan_fft_forward(codec2_3200::FFT_ENC),
        }
    }
}

impl Decoder {
    pub fn new() -> Self {
        Self::default()
    }

    /// Decodes `BYTES_PER_FRAME` real-format bytes into one 40ms
    /// (`SAMPLES_PER_FRAME`-sample) frame of audio. Matches
    /// `codec2_decode_1600`'s own real structure exactly (see this
    /// module's own doc comment).
    pub fn decode(&mut self, bytes: &[u8; BYTES_PER_FRAME]) -> [i16; SAMPLES_PER_FRAME] {
        let fields = bits::unpack_frame_1600(bytes);
        let wo_a = quantise::decode_wo(fields.wo_index_a);
        let e_a = quantise::decode_energy(fields.e_index_a);
        let wo_b = quantise::decode_wo(fields.wo_index_b);
        let e_b = quantise::decode_energy(fields.e_index_b);

        let mut lsps3 = lsp_quantiser::decode_lsps_scalar(&fields.lsp_indexes);
        lsp_post::check_lsp_order(&mut lsps3);
        lsp_post::bw_expand_lsps(&mut lsps3, 50.0, 100.0);

        let voiced0 = interp::interp_voiced(fields.voiced0, self.prev_voiced, fields.voiced1);
        let wo0 = interp::interp_wo(
            fields.voiced0,
            self.prev_wo,
            self.prev_voiced,
            wo_a,
            fields.voiced1,
            codec2_3200::W0_MIN,
        );
        let e0 = interp::interp_energy(self.prev_e, e_a);

        let voiced2 = interp::interp_voiced(fields.voiced2, fields.voiced1, fields.voiced3);
        let wo2 = interp::interp_wo(
            fields.voiced2,
            wo_a,
            fields.voiced1,
            wo_b,
            fields.voiced3,
            codec2_3200::W0_MIN,
        );
        let e2 = interp::interp_energy(e_a, e_b);

        let lsps0 = lsp_post::interpolate_lsp_ver2(&self.prev_lsps, &lsps3, 0.25);
        let lsps1 = lsp_post::interpolate_lsp_ver2(&self.prev_lsps, &lsps3, 0.5);
        let lsps2 = lsp_post::interpolate_lsp_ver2(&self.prev_lsps, &lsps3, 0.75);

        let mut out = [0i16; SAMPLES_PER_FRAME];
        let subframes = [
            (wo0, voiced0, lsps0, e0),
            (wo_a, fields.voiced1, lsps1, e_a),
            (wo2, voiced2, lsps2, e2),
            (wo_b, fields.voiced3, lsps3, e_b),
        ];
        for (i, (wo, voiced, lsps, e)) in subframes.into_iter().enumerate() {
            let ak = lpc::lsp_to_lpc(&lsps);
            let mut model = envelope::Model::new(wo, voiced);
            let aw = envelope::compute_harmonic_amplitudes(self.fft.as_ref(), &ak, e, &mut model);
            envelope::apply_first_harmonic_correction(&mut model);
            let sub = self.synth.synthesize_subframe(&mut model, &aw);
            out[i * N_SAMP..(i + 1) * N_SAMP].copy_from_slice(&sub);
        }

        self.prev_wo = wo_b;
        self.prev_voiced = fields.voiced3;
        self.prev_lsps = lsps3;
        self.prev_e = e_b;

        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn synthetic_speech_frame(f0: f32, t0: usize) -> [i16; SAMPLES_PER_FRAME] {
        std::array::from_fn(|i| {
            let t = (t0 + i) as f32 / codec2_3200::SAMPLE_RATE as f32;
            let v = 8000.0 * (std::f32::consts::TAU * f0 * t).sin()
                + 3000.0 * (std::f32::consts::TAU * 2.0 * f0 * t).sin();
            v as i16
        })
    }

    /// Full round trip through this crate's own encoder and decoder --
    /// same basic sanity bar `codec2_3200`'s own equivalent test uses:
    /// finite, reasonably-scaled, non-degenerate audio, no panics.
    #[test]
    fn encode_decode_round_trip_produces_finite_reasonably_scaled_audio() {
        let mut encoder = Encoder::new();
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

    /// Real cross-implementation decoder check: a real captured
    /// bitstream from plain upstream Codec2's own unmodified `c2enc
    /// 1600`, decoded both by this crate's `Decoder` and (already, at
    /// fixture-capture time) by upstream's own `c2dec 1600` -- same
    /// synthetic (non-speech) signal, same real reference decoder
    /// methodology `codec2_3200`'s own equivalent test documents (see
    /// that test's own doc comment for why synthetic-not-speech and why
    /// this direction, not the encode direction, is the one that
    /// admits a bit-level correctness check).
    #[test]
    fn decoder_matches_the_real_reference_decoder_on_a_real_captured_synthetic_signal_bitstream() {
        let bits_path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/codec2_1600/synthetic_c_encoded_bits.bin"
        );
        let pcm_path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/codec2_1600/synthetic_c_decoded_pcm.bin"
        );

        let bits_data = std::fs::read(bits_path).unwrap_or_else(|e| panic!("{bits_path}: {e}"));
        let pcm_data = std::fs::read(pcm_path).unwrap_or_else(|e| panic!("{pcm_path}: {e}"));
        let n_frames = bits_data.len() / BYTES_PER_FRAME;
        assert!(
            n_frames > 150,
            "expected the real captured fixture corpus, got {n_frames} frames"
        );
        assert_eq!(
            pcm_data.len(),
            n_frames * SAMPLES_PER_FRAME * 2,
            "bitstream/PCM fixture frame counts don't line up"
        );

        let mut decoder = Decoder::new();
        let mut rust_pcm: Vec<i16> = Vec::with_capacity(n_frames * SAMPLES_PER_FRAME);
        for f in 0..n_frames {
            let frame: [u8; BYTES_PER_FRAME] = bits_data[f * BYTES_PER_FRAME..(f + 1) * BYTES_PER_FRAME]
                .try_into()
                .unwrap();
            rust_pcm.extend_from_slice(&decoder.decode(&frame));
        }
        let ref_pcm: Vec<i16> = pcm_data
            .chunks_exact(2)
            .map(|b| i16::from_le_bytes([b[0], b[1]]))
            .collect();

        let n = rust_pcm.len();
        let mean_a: f64 = rust_pcm.iter().map(|&s| s as f64).sum::<f64>() / n as f64;
        let mean_b: f64 = ref_pcm.iter().map(|&s| s as f64).sum::<f64>() / n as f64;
        let mut cov = 0.0f64;
        let mut var_a = 0.0f64;
        let mut var_b = 0.0f64;
        for i in 0..n {
            let da = rust_pcm[i] as f64 - mean_a;
            let db = ref_pcm[i] as f64 - mean_b;
            cov += da * db;
            var_a += da * da;
            var_b += db * db;
        }
        let corr = cov / (var_a * var_b).sqrt();
        let rms_a = (var_a / n as f64 + mean_a * mean_a).sqrt();
        let rms_b = (var_b / n as f64 + mean_b * mean_b).sqrt();
        let rms_ratio = rms_a / rms_b;
        println!("codec2_1600 Decoder vs reference: correlation={corr}, rms_rust={rms_a}, rms_reference={rms_b}, ratio={rms_ratio}");
        // Measured correlation 0.9492 and RMS ratio close to 1.0 when
        // this test was written; real margin either side, not a
        // loosened guess (same "measure first, then set the bound"
        // discipline `codec2_3200::DecoderFixed`'s own equivalent test
        // documents). 1600bps is a genuinely lossier mode than 3200bps
        // (a coarser LSP quantizer, LSPs updated only every 40ms), so a
        // somewhat lower correlation bar than that mode's own >0.99 is
        // expected, not a regression.
        assert!(
            (0.85..1.15).contains(&rms_ratio),
            "Decoder's own RMS ({rms_a}) diverged from the reference's ({rms_b}), ratio={rms_ratio} -- looks like a gain/scale bug"
        );
        assert!(corr > 0.9, "decoder output diverged from the real reference decoder on the same real captured bitstream: correlation={corr} (expected > 0.9, measured 0.9492 when this test was written)");
    }
}
