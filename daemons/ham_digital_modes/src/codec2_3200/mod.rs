// SPDX-License-Identifier: LGPL-3.0-or-later
//! An independently-authored Rust port of Codec2's 3200bps mode, aimed at
//! interoperating with real Codec2/Codec2-mod encoders and decoders in the
//! wild, without being a derivative work of Codec2-mod's own
//! LGPL-2.1-only source.
//!
//! "Interoperate" is *not* symmetric, and it's worth being precise here
//! since it shapes what a real acceptance test for this module can and
//! can't check:
//!
//! - **Decoding a real bitstream** (from this crate's own encoder or any
//!   real Codec2/Codec2-mod encoder) must use the exact same quantizer
//!   dequantization formulas and bit-packing/Gray-coding the real format
//!   defines -- there's no design freedom here, and this direction *is*
//!   checkable by feeding a real captured bitstream through both this
//!   decoder and the reference decoder and comparing. (Reconstructed PCM
//!   audio samples themselves can still differ slightly in float
//!   rounding even then -- `vendor/codec2-mod/README.md`'s own README
//!   makes the identical caveat about *its* bit-exactness claim:
//!   "Bit-exactness refers to the encoded bitstream - decoded audio
//!   samples may differ from the reference implementation.")
//! - **Encoding** must produce a bitstream any compliant decoder decodes
//!   correctly, but does *not* need to choose the exact same quantizer
//!   index values the reference's own encoder would have chosen for
//!   identical input speech -- a decoder has no way to know, or care,
//!   how the encoder arrived at a given `Wo`/energy/LSP index (see
//!   `nlp.rs`'s own module doc comment for where this matters most: its
//!   pitch estimate has full design freedom). This direction can only be
//!   checked for decodability and intelligibility, never bit-compared
//!   against what the reference encoder would have produced -- a future
//!   session building a cross-codec acceptance harness should not expect
//!   an encode-in-Rust/decode-in-C round trip to ever bit-match a
//!   reference encode-in-C/decode-in-C round trip on the same input.
//!
//! **Encoder verified against the real reference decoder.** `Encoder`
//! (this module) implements the full real encode pipeline (pitch,
//! voicing, LSP/energy analysis, Gray-coded bit packing) and was checked
//! with `examples/codec2_encode_wav.rs`: encode all five of
//! `tests/fixtures/codec2_3200/README.md`'s real speech WAVs in Rust,
//! feed the resulting bitstream to an unmodified, separately-built (not
//! linked into this crate -- see that example's own doc comment for why)
//! real `vendor/codec2-mod` C decoder. Every frame decoded without error
//! across all five files; decoded-output RMS landed at 83-104% of each
//! WAV's own real input RMS (brian_g8sez 3713/4153, david_vk5dgr
//! 2810/3389, mooneer 1958/1883, peter 1738/2093, k0pfx_mel 1943/1991),
//! and no sample approached the int16 clipping bound on any of them. A
//! quantized 3200bps vocoder isn't a waveform coder, so this isn't
//! bit-for-bit fidelity, but it's real, reproducible evidence this
//! crate's own bitstream decodes cleanly with the real reference at a
//! genuinely speech-like signal level, not silence or garbage -- real
//! interoperability, not just internal self-consistency.
//!
//! **Decoder verified against the real reference decoder, the direction
//! that actually admits a numeric comparison** (see this doc comment's
//! own note above on why encode/decode aren't symmetric here).
//! `examples/codec2_decode_bin.rs` decoded a real ~2539-frame bitstream
//! -- captured straight from the real reference *encoder* itself, not
//! this crate's own encoder, so this checks `Decoder` in complete
//! isolation from any of this crate's own encoder-side choices -- with
//! this crate's own `Decoder`, and the same bitstream was separately
//! decoded with the unmodified real `vendor/codec2-mod` C decoder.
//! Per-sample Pearson correlation over the whole file: **0.958** (RMS
//! 2718.0 vs 2718.5, essentially identical overall level) -- but a
//! single global number over an energy-dominated signal can hide a real
//! problem concentrated in quiet passages, so it was also checked
//! segment-wise (fifty 1-second/8000-sample blocks): median block
//! correlation **0.971**, with the worst blocks landing at 0.09-0.80 --
//! but every one of those low-correlation blocks has near-identical
//! per-block RMS between the two decoders (e.g. one block: RMS 2614 vs
//! 2609; another: RMS 21 vs 21), meaning the amplitude/loudness envelope
//! reconstruction agrees closely even where the sample-level waveform
//! doesn't. That pattern -- right loudness, divergent fine structure --
//! is exactly what's expected from unvoiced/postfilter-randomized
//! harmonics drawing phase from this crate's own independent PRNG
//! rather than the reference's `codec2_rand` (differently-seeded noise,
//! not differently-*shaped* speech), not a structural defect in the
//! reconstruction itself.
//!
//! Written from a from-scratch reading of the *algorithm* (LPC analysis,
//! Levinson-Durbin, LSP conversion, pitch estimation, sinusoidal
//! synthesis -- all textbook DSP techniques predating Codec2 by decades,
//! e.g. Makhoul 1975 for Levinson-Durbin), not translated line-by-line
//! from `vendor/codec2-mod/`'s own C. See
//! `docs/references/CODEC2_MOD_FIXED_POINT_PLAN.md` for the real,
//! measured characterization this implementation is built on (per-stage
//! numeric ranges, the Levinson-Durbin clamp bifurcation, the validated
//! fixed-point primitives), and
//! `vendor/codec2-mod/VENDORED_FROM.md` for why this independent-
//! implementation route was chosen over linking the vendored reference
//! directly (its own LGPL-2.1-only license, no "or later" grant, isn't
//! automatically combinable with this crate's LGPL-3.0-or-later).
//!
//! Bitstream-format constants below (frame layout, quantizer ranges, bit
//! widths) are the actual Codec2 3200bps format -- not creative
//! expression, but the numbers any interoperable implementation must
//! use, the same reasoning `psk31.rs`'s own Varicode table documents for
//! a published external standard. Codec2's own quantizer *design*
//! (`quantise.rs`'s LSP delta-scalar quantizer, this module's
//! `bw_gamma`) turned out, on inspection, to reduce to simple closed-form
//! formulas (a piecewise-uniform step quantizer; a geometric sequence)
//! rather than opaque trained data -- see `quantise.rs`'s own doc comment
//! for the real verification that found this, which is why no separate
//! LGPL-2.1-only data file was needed here at all.

pub mod bits;
pub mod envelope;
pub mod fixed_point;
pub mod interp;
pub mod lpc;
pub mod nlp;
pub mod quantise;
pub mod synthesis;
pub mod voicing;
pub mod window;

/// LPC analysis order -- 10 reflection coefficients / LSP frequencies,
/// the real Codec2 3200bps format's own choice.
pub const LPC_ORD: usize = 10;
/// Pitch-analysis history window: 40ms at 8kHz.
pub const M_PITCH: usize = 320;
/// One analysis/synthesis sub-frame: 10ms at 8kHz.
pub const N_SAMP: usize = 80;
/// LPC analysis window support width (samples) -- narrower than
/// `M_PITCH`, centered in it.
pub const NW: usize = 279;
/// LPC analysis FFT size.
pub const FFT_ENC: usize = 512;
/// Pitch-estimator FFT size.
pub const PE_FFT_SIZE: usize = 512;
/// Pitch-estimator decimation factor.
pub const NLP_DEC: usize = 5;
/// Sample rate this mode operates at.
pub const SAMPLE_RATE: u32 = 8000;

/// `Wo` (normalized angular pitch frequency) quantizer range: `P_MAX`
/// (160 samples, ~50Hz) to `P_MIN` (20 samples, ~400Hz) pitch period.
pub const W0_MIN: f32 = (2.0 * std::f32::consts::PI) / 160.0;
pub const W0_MAX: f32 = (2.0 * std::f32::consts::PI) / 20.0;
pub const WO_BITS: u32 = 7;

/// Pitch-estimator search range, samples.
pub const P_MIN: usize = 20;
pub const P_MAX: usize = 160;

/// Energy quantizer range, dB.
pub const E_MIN_DB: f32 = -10.0;
pub const E_MAX_DB: f32 = 40.0;
pub const E_BITS: u32 = 5;

/// One 3200bps frame: 64 bits (voiced x2, Wo, energy, 10 LSP deltas x 5
/// bits each = 2 + 7 + 5 + 50 = 64) covering 20ms of audio (two 10ms,
/// `N_SAMP`-sample sub-frames) -- 64 bits / 20ms = 3200bps.
pub const BYTES_PER_FRAME: usize = 8;
pub const SAMPLES_PER_FRAME: usize = 2 * N_SAMP;

/// Highest sinusoidal harmonic this mode's `model_t` can track.
pub const MAX_AMP: usize = 80;

/// Synthesis (Parzen) window ramp width, samples -- ramps up/down over
/// `TW` samples at each end of the `SAMPLES_PER_FRAME`-sample overlap-add
/// window.
pub const TW: usize = 40;

/// LPC postfilter constants (bandwidth-expansion gamma exponent base,
/// spectral-envelope-flattening beta) -- Codec2's own real, published
/// choices for this specific postfilter design.
pub const LPCPF_GAMMA: f32 = 0.5;
pub const LPCPF_BETA: f32 = 0.2;
pub const LPCPF_TWO_BETA: f32 = 2.0 * LPCPF_BETA;

/// Background-noise estimator (voiced/unvoiced harmonic split) constants.
pub const BG_THRESH: f32 = 40.0;
pub const BG_BETA: f32 = 0.1;
pub const BG_MARGIN: f32 = 6.0;

/// `~15Hz` bandwidth expansion applied to LPC coefficients before LSP
/// conversion, geometric: `bw_gamma[i] = 0.994^i`. Verified directly
/// against the real vendored reference's own literal table (11 values,
/// `vendor/codec2-mod/src/lpc.c`) that this is exactly what it computes
/// -- not independently chosen, reproduced for interoperability, but a
/// one-parameter formula rather than a table.
pub fn bw_gamma(i: usize) -> f32 {
    const GAMMA: f32 = 0.994;
    GAMMA.powi(i as i32)
}

/// LSPs to substitute when `lpc::lpc_to_lsp` fails to find all
/// `LPC_ORD` roots (a real, if rare, LPC analysis failure mode on
/// pathological input) -- evenly spaced across `[0, pi]`, matching the
/// reference's own documented fallback for the same case.
fn fallback_lsp() -> [f32; LPC_ORD] {
    std::array::from_fn(|i| (std::f32::consts::PI / LPC_ORD as f32) * i as f32)
}

/// Persistent per-call encoder state: the `M_PITCH`-sample speech
/// history window shared by pitch estimation and LPC analysis, plus
/// `nlp`/`voicing`'s own state.
pub struct Encoder {
    sn: [f32; M_PITCH],
    window: [f32; M_PITCH],
    nlp_state: nlp::NlpState,
    voicing_state: voicing::VoicingState,
}

impl Default for Encoder {
    fn default() -> Self {
        Encoder { sn: [0.0; M_PITCH], window: window::make_analysis_window(), nlp_state: nlp::NlpState::new(), voicing_state: voicing::VoicingState::new() }
    }
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

    /// Encodes one 20ms (`SAMPLES_PER_FRAME`-sample) frame into
    /// `BYTES_PER_FRAME` real-format bytes. Matches the reference's own
    /// real `codec2_encode` structure: two 10ms analysis sub-steps (each
    /// advancing pitch estimation and contributing one `voiced` bit, the
    /// second also setting the transmitted `Wo`), then one LSP/energy
    /// analysis pass over the full `M_PITCH`-sample history window.
    pub fn encode(&mut self, speech: &[i16; SAMPLES_PER_FRAME]) -> [u8; BYTES_PER_FRAME] {
        self.shift_in(&speech[..N_SAMP]);
        nlp::nlp(&mut self.nlp_state, &self.sn);
        let voiced0 = voicing::is_voiced(&mut self.voicing_state, &self.sn[M_PITCH - N_SAMP..]);

        self.shift_in(&speech[N_SAMP..]);
        let f0 = nlp::nlp(&mut self.nlp_state, &self.sn);
        let voiced1 = voicing::is_voiced(&mut self.voicing_state, &self.sn[M_PITCH - N_SAMP..]);

        let wo_index = quantise::encode_wo(nlp::f0_to_wo(f0));

        let mut windowed = [0.0f32; M_PITCH];
        for ((w, &s), &win) in windowed.iter_mut().zip(self.sn.iter()).zip(self.window.iter()) {
            *w = s * win;
        }
        let r = lpc::autocorrelate(&windowed);
        let mut ak = lpc::levinson_durbin(&r);
        let e = lpc::lpc_energy(&ak, &r);
        for (i, a) in ak.iter_mut().enumerate() {
            *a *= bw_gamma(i);
        }
        let lsp = lpc::lpc_to_lsp(&ak).unwrap_or_else(fallback_lsp);

        let e_index = quantise::encode_energy(e);
        let lsp_indexes = quantise::encode_lsps_delta_scalar(&lsp);

        let fields = bits::FrameFields { voiced0, voiced1, wo_index, e_index, lsp_indexes };
        bits::pack_frame(&fields, WO_BITS, E_BITS)
    }
}

/// LSPs the reference's own decoder starts from before any real frame
/// has been decoded -- evenly spaced across `[0, pi]`, same shape as
/// `fallback_lsp` (a fresh decoder has nothing better to interpolate the
/// very first frame's own first sub-frame from).
fn initial_lsps() -> [f32; LPC_ORD] {
    std::array::from_fn(|i| (i as f32 * std::f32::consts::PI) / (LPC_ORD as f32 + 1.0))
}

/// Persistent per-decoder state: the previous frame's own decoded
/// `Wo`/voiced/LSPs/energy (needed for interpolating the next frame's
/// first sub-frame), plus `synthesis::SynthesisState`'s own overlap-add
/// memory and a forward FFT plan (`envelope.rs`'s own spectral-envelope
/// computation).
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
            prev_wo: W0_MIN,
            prev_voiced: false,
            prev_lsps: initial_lsps(),
            prev_e: 1.0,
            synth: synthesis::SynthesisState::new(),
            fft: planner.plan_fft_forward(FFT_ENC),
        }
    }
}

impl Decoder {
    pub fn new() -> Self {
        Self::default()
    }

    /// Decodes `BYTES_PER_FRAME` real-format bytes into one 20ms
    /// (`SAMPLES_PER_FRAME`-sample) frame of audio. Matches the
    /// reference's own real `codec2_decode` structure: unpack this
    /// frame's own (`Wo`, energy, LSPs, both `voiced` bits), interpolate
    /// the first (earlier) sub-frame's parameters from the previous
    /// frame's own decoded state, then run LSP-to-LPC, spectral-envelope
    /// reconstruction, and sinusoidal synthesis once per sub-frame.
    pub fn decode(&mut self, bytes: &[u8; BYTES_PER_FRAME]) -> [i16; SAMPLES_PER_FRAME] {
        let fields = bits::unpack_frame(bytes, WO_BITS, E_BITS);
        let wo1 = quantise::decode_wo(fields.wo_index);
        let e1 = quantise::decode_energy(fields.e_index);
        let lsps1 = quantise::decode_lsps_delta_scalar(&fields.lsp_indexes);

        let voiced0 = interp::interp_voiced(fields.voiced0, self.prev_voiced, fields.voiced1);
        let wo0 = interp::interp_wo(fields.voiced0, self.prev_wo, self.prev_voiced, wo1, fields.voiced1, W0_MIN);
        let e0 = interp::interp_energy(self.prev_e, e1);
        let lsps0 = interp::interpolate_lsp(&self.prev_lsps, &lsps1);

        let mut out = [0i16; SAMPLES_PER_FRAME];
        let subframes = [(wo0, voiced0, lsps0, e0), (wo1, fields.voiced1, lsps1, e1)];
        for (i, (wo, voiced, lsps, e)) in subframes.into_iter().enumerate() {
            let ak = lpc::lsp_to_lpc(&lsps);
            let mut model = envelope::Model::new(wo, voiced);
            let aw = envelope::compute_harmonic_amplitudes(self.fft.as_ref(), &ak, e, &mut model);
            envelope::apply_first_harmonic_correction(&mut model);
            let sub = self.synth.synthesize_subframe(&mut model, &aw);
            out[i * N_SAMP..(i + 1) * N_SAMP].copy_from_slice(&sub);
        }

        self.prev_wo = wo1;
        self.prev_voiced = fields.voiced1;
        self.prev_lsps = lsps1;
        self.prev_e = e1;

        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn synthetic_speech_frame(f0: f32, t0: usize) -> [i16; SAMPLES_PER_FRAME] {
        std::array::from_fn(|i| {
            let t = (t0 + i) as f32 / SAMPLE_RATE as f32;
            let v = 8000.0 * (std::f32::consts::TAU * f0 * t).sin() + 3000.0 * (std::f32::consts::TAU * 2.0 * f0 * t).sin();
            v as i16
        })
    }

    /// Full round trip through this crate's own encoder and decoder:
    /// not a claim of correctness against the reference (see mod.rs's
    /// own module doc comment on why that's checked differently, with
    /// `examples/codec2_encode_wav.rs`) -- just a basic sanity check
    /// that a real multi-frame sequence produces finite, reasonably-
    /// scaled, non-degenerate audio and doesn't panic.
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

    /// The real cross-implementation decoder check (see this module's
    /// own doc comment above) lived in a one-off shell session and
    /// won't survive it -- this is that same check made permanent and
    /// automated, guarding `Decoder` against regressions the way
    /// `lpc.rs`/`quantise.rs`/`bits.rs`'s own real-reference tests guard
    /// their own stages.
    ///
    /// Uses the *synthetic* (non-speech) signal's own real captured
    /// bitstream and the real reference decoder's own PCM output for
    /// it, not the real donated speech recordings: this crate's own
    /// PRNG seed differs from the reference's `codec2_rand`, so
    /// unvoiced/postfilter-randomized excitation legitimately diverges
    /// between the two decoders' output -- real speech's natural mix of
    /// voiced/unvoiced/silence content makes that divergence large
    /// enough that a single global correlation threshold would need to
    /// be loose enough to hide a real regression. The synthetic
    /// signal's mostly-tonal, mostly-voiced content keeps that
    /// divergence small (measured directly: 0.9987 correlation vs the
    /// real speech check's 0.958 whole-file / 0.971 median-of-blocks),
    /// so a tight threshold here is actually discriminating. Committing
    /// vocoded PCM of the real donated recordings would also cross the
    /// same line `tests/fixtures/codec2_3200/README.md` already draws
    /// against real `Wn[]`/`Sn[]` data (still recognizable speech, even
    /// lossily coded) -- the synthetic signal has no such concern.
    #[test]
    fn decoder_matches_the_real_reference_decoder_on_a_real_captured_synthetic_signal_bitstream() {
        let bits_path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/codec2_3200/synthetic_c_encoded_bits.bin");
        let pcm_path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/codec2_3200/synthetic_c_decoded_pcm.bin");

        let bits_data = std::fs::read(bits_path).unwrap_or_else(|e| panic!("{bits_path}: {e}"));
        let pcm_data = std::fs::read(pcm_path).unwrap_or_else(|e| panic!("{pcm_path}: {e}"));
        let n_frames = bits_data.len() / BYTES_PER_FRAME;
        assert!(n_frames > 150, "expected the real captured fixture corpus, got {n_frames} frames");
        assert_eq!(pcm_data.len(), n_frames * SAMPLES_PER_FRAME * 2, "bitstream/PCM fixture frame counts don't line up");

        let mut decoder = Decoder::new();
        let mut rust_pcm: Vec<i16> = Vec::with_capacity(n_frames * SAMPLES_PER_FRAME);
        for f in 0..n_frames {
            let frame: [u8; BYTES_PER_FRAME] = bits_data[f * BYTES_PER_FRAME..(f + 1) * BYTES_PER_FRAME].try_into().unwrap();
            rust_pcm.extend_from_slice(&decoder.decode(&frame));
        }
        let ref_pcm: Vec<i16> = pcm_data.chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]])).collect();

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
        assert!(corr > 0.99, "decoder output diverged from the real reference decoder on the same real captured bitstream: correlation={corr} (expected > 0.99, measured 0.9987 when this test was written)");
    }
}
