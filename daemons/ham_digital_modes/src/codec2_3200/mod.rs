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
//! **Stale as of `lpc::apply_white_noise_correction`'s addition,
//! flagged plainly rather than silently left to look current**: the
//! measurement above predates that function's own real, if small,
//! change to `floating_reference::Encoder`'s LPC estimate (`R[0] *=
//! 1 + 1e-3` before Levinson-Durbin). This crate's own automated tests
//! (round-trip sanity, decoder-vs-reference on a bitstream this crate's
//! own encoder never touches) still pass, but the specific real
//! cross-decoder numbers above have not been re-measured against the
//! corrected encoder -- that check is a manual step (see
//! `examples/codec2_encode_wav.rs`'s own doc comment for why it's kept
//! outside this crate's automated build) that needs re-running before
//! treating this section as current again.
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
pub mod encoder_fixed;
pub mod envelope;
pub mod fixed_fft;
pub mod fixed_point;
pub mod floating_reference;
pub mod interp;
pub mod lpc;
pub mod nlp;
pub mod quantise;
pub mod spectral_bridge;
pub mod synthesis;
pub mod trig_fixed;
pub mod voicing;
pub mod window;

pub use encoder_fixed::EncoderFixed;

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
/// computation). `spectral_bridge` is `pub` so a caller can toggle
/// `.enabled` at any time (on by default) -- see `decode_16k`'s own
/// doc comment; it plays no part in the ordinary `decode()` path at all.
pub struct Decoder {
    prev_wo: f32,
    prev_voiced: bool,
    prev_lsps: [f32; LPC_ORD],
    prev_e: f32,
    synth: synthesis::SynthesisState,
    fft: std::sync::Arc<dyn rustfft::Fft<f32>>,
    pub spectral_bridge: spectral_bridge::SpectralBridgeState,
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
            spectral_bridge: spectral_bridge::SpectralBridgeState::new(),
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
        let wo0 = interp::interp_wo(
            fields.voiced0,
            self.prev_wo,
            self.prev_voiced,
            wo1,
            fields.voiced1,
            W0_MIN,
        );
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

    /// Opt-in 16kHz decode: Spectral Bridge (see `spectral_bridge.rs`'s
    /// own doc comment). Deliberately a *separate* method, not a
    /// parameter on `decode()` -- the ordinary 8kHz path above is
    /// completely untouched by this one (same real per-subframe model
    /// computation, duplicated rather than shared, so there is zero
    /// risk of this new, less-validated path regressing the real,
    /// reference-validated one). Toggle `self.spectral_bridge.enabled`
    /// (on by default) to turn harmonic extrapolation on/off; either
    /// way this always returns genuine 16kHz audio (harmonics `1..=l`
    /// reused unchanged either way, only whether harmonics above `l`
    /// get synthesized differs).
    pub fn decode_16k(
        &mut self,
        bytes: &[u8; BYTES_PER_FRAME],
    ) -> [i16; 2 * spectral_bridge::N_SAMP_SB] {
        let fields = bits::unpack_frame(bytes, WO_BITS, E_BITS);
        let wo1 = quantise::decode_wo(fields.wo_index);
        let e1 = quantise::decode_energy(fields.e_index);
        let lsps1 = quantise::decode_lsps_delta_scalar(&fields.lsp_indexes);

        let voiced0 = interp::interp_voiced(fields.voiced0, self.prev_voiced, fields.voiced1);
        let wo0 = interp::interp_wo(
            fields.voiced0,
            self.prev_wo,
            self.prev_voiced,
            wo1,
            fields.voiced1,
            W0_MIN,
        );
        let e0 = interp::interp_energy(self.prev_e, e1);
        let lsps0 = interp::interpolate_lsp(&self.prev_lsps, &lsps1);

        let mut out = [0i16; 2 * spectral_bridge::N_SAMP_SB];
        let subframes = [(wo0, voiced0, lsps0, e0), (wo1, fields.voiced1, lsps1, e1)];
        for (i, (wo, voiced, lsps, e)) in subframes.into_iter().enumerate() {
            let ak = lpc::lsp_to_lpc(&lsps);
            let mut model = envelope::Model::new(wo, voiced);
            let aw = envelope::compute_harmonic_amplitudes(self.fft.as_ref(), &ak, e, &mut model);
            envelope::apply_first_harmonic_correction(&mut model);
            // Populates model.phi[1..=l] with its own final phase (the
            // spectral bridge synthesis below reuses this unchanged
            // for harmonics 1..=l) -- discards the 8kHz output itself,
            // this method's own caller wants the 16kHz one instead.
            let _sub = self.synth.synthesize_subframe(&mut model, &aw);
            let sub_sb = self.spectral_bridge.synthesize_subframe_sb(&model);
            out[i * spectral_bridge::N_SAMP_SB..(i + 1) * spectral_bridge::N_SAMP_SB]
                .copy_from_slice(&sub_sb);
        }

        self.prev_wo = wo1;
        self.prev_voiced = fields.voiced1;
        self.prev_lsps = lsps1;
        self.prev_e = e1;

        out
    }
}

fn w0_min_q23() -> i64 {
    static V: std::sync::OnceLock<i64> = std::sync::OnceLock::new();
    *V.get_or_init(|| fixed_point::f32_to_q_exact_round(W0_MIN, lpc::COEF_FRAC_BITS))
}

fn initial_lsps_q23() -> [i64; LPC_ORD] {
    static V: std::sync::OnceLock<[i64; LPC_ORD]> = std::sync::OnceLock::new();
    *V.get_or_init(|| {
        let lsps = initial_lsps();
        std::array::from_fn(|i| fixed_point::f32_to_q_exact_round(lsps[i], lpc::COEF_FRAC_BITS))
    })
}

/// Fixed-point sibling of `Decoder` -- genuinely integer end to end, no
/// `f32` anywhere except the final `i16` PCM boundary each sub-frame's
/// own `synthesize_subframe_fixed` call already handles. No FFT planner
/// field (unlike `Decoder`'s own `fft: Arc<dyn Fft<f32>>`) since
/// `envelope::compute_harmonic_amplitudes_fixed` calls straight into
/// `fixed_fft::fft_fixed`, no trait object needed.
pub struct DecoderFixed {
    prev_wo: i64,
    prev_voiced: bool,
    prev_lsps: [i64; LPC_ORD],
    prev_e: i64,
    synth: synthesis::SynthesisStateFixed,
}

impl Default for DecoderFixed {
    fn default() -> Self {
        DecoderFixed {
            prev_wo: w0_min_q23(),
            prev_voiced: false,
            prev_lsps: initial_lsps_q23(),
            prev_e: 1i64 << 23,
            synth: synthesis::SynthesisStateFixed::new(),
        }
    }
}

impl DecoderFixed {
    pub fn new() -> Self {
        Self::default()
    }

    /// Same real frame structure as `Decoder::decode` (see that
    /// function's own doc comment) -- this is `DecoderFixed`'s own
    /// mirror, genuinely fixed-point end to end.
    pub fn decode(&mut self, bytes: &[u8; BYTES_PER_FRAME]) -> [i16; SAMPLES_PER_FRAME] {
        let fields = bits::unpack_frame(bytes, WO_BITS, E_BITS);
        let wo1 = quantise::decode_wo_fixed(fields.wo_index);
        let e1 = quantise::decode_energy_fixed(fields.e_index);
        let lsps1 = quantise::decode_lsps_delta_scalar_fixed(&fields.lsp_indexes);

        let voiced0 = interp::interp_voiced(fields.voiced0, self.prev_voiced, fields.voiced1);
        let wo0 = interp::interp_wo_fixed(
            fields.voiced0,
            self.prev_wo,
            self.prev_voiced,
            wo1,
            fields.voiced1,
            w0_min_q23(),
        );
        let e0 = interp::interp_energy_fixed(self.prev_e, e1);
        let lsps0 = interp::interpolate_lsp_fixed(&self.prev_lsps, &lsps1);

        let mut out = [0i16; SAMPLES_PER_FRAME];
        let subframes = [(wo0, voiced0, lsps0, e0), (wo1, fields.voiced1, lsps1, e1)];
        for (i, (wo, voiced, lsps, e)) in subframes.into_iter().enumerate() {
            let ak = lpc::lsp_to_lpc_fixed(&lsps);
            let mut model = envelope::ModelFixed::new(wo, voiced);
            let aw = envelope::compute_harmonic_amplitudes_fixed(&ak, e, &mut model);
            envelope::apply_first_harmonic_correction_fixed(&mut model);
            let sub = self.synth.synthesize_subframe_fixed(&mut model, &aw);
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
    use super::floating_reference::Encoder;
    use super::*;

    fn synthetic_speech_frame(f0: f32, t0: usize) -> [i16; SAMPLES_PER_FRAME] {
        std::array::from_fn(|i| {
            let t = (t0 + i) as f32 / SAMPLE_RATE as f32;
            let v = 8000.0 * (std::f32::consts::TAU * f0 * t).sin()
                + 3000.0 * (std::f32::consts::TAU * 2.0 * f0 * t).sin();
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

    /// `decode_16k`'s own real, end-to-end design invariant: with
    /// Spectral Bridge *disabled*, harmonics `1..=l` are reused
    /// unchanged from the base decode -- so decimating its 16kHz output
    /// by 2 (safe here specifically because there is no >4kHz content
    /// to alias down when disabled) should closely match `decode()`'s
    /// own real output for the *same* bitstream. Uses the same real
    /// captured synthetic-signal fixture the base decoder's own
    /// reference test uses, so this doesn't need a new fixture.
    ///
    /// Samples the *odd* 16kHz indices (`skip(1).step_by(2)`), not the
    /// even ones -- measured directly (a small standalone diagnostic
    /// comparing both phases sample-by-sample) that the odd phase lines
    /// up with the base decoder's own output almost exactly (within a
    /// few counts, ordinary float-rounding noise from the larger IFFT)
    /// while the even phase does not. A one-sample framing offset
    /// between the two overlap-add buffers' own "carried tail" vs "new"
    /// halves, not a defect in either decoder's own output.
    #[test]
    fn decode_16k_with_spectral_bridge_disabled_matches_the_base_8khz_decoder_when_decimated() {
        let bits_path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/codec2_3200/synthetic_c_encoded_bits.bin"
        );
        let bits_data = std::fs::read(bits_path).unwrap_or_else(|e| panic!("{bits_path}: {e}"));
        let n_frames = bits_data.len() / BYTES_PER_FRAME;
        assert!(n_frames > 150, "expected the real captured fixture corpus, got {n_frames} frames");

        let mut decoder_8k = Decoder::new();
        let mut decoder_16k = Decoder::new();
        decoder_16k.spectral_bridge.enabled = false;

        let mut pcm_8k: Vec<i16> = Vec::with_capacity(n_frames * SAMPLES_PER_FRAME);
        let mut pcm_16k_decimated: Vec<i16> = Vec::with_capacity(n_frames * SAMPLES_PER_FRAME);
        for f in 0..n_frames {
            let frame: [u8; BYTES_PER_FRAME] =
                bits_data[f * BYTES_PER_FRAME..(f + 1) * BYTES_PER_FRAME].try_into().unwrap();
            pcm_8k.extend_from_slice(&decoder_8k.decode(&frame));
            let out_16k = decoder_16k.decode_16k(&frame);
            pcm_16k_decimated.extend(out_16k.iter().skip(1).step_by(2).copied());
        }

        assert_eq!(pcm_8k.len(), pcm_16k_decimated.len());
        let n = pcm_8k.len();
        let mean_a: f64 = pcm_8k.iter().map(|&s| s as f64).sum::<f64>() / n as f64;
        let mean_b: f64 = pcm_16k_decimated.iter().map(|&s| s as f64).sum::<f64>() / n as f64;
        let mut cov = 0.0f64;
        let mut var_a = 0.0f64;
        let mut var_b = 0.0f64;
        for i in 0..n {
            let da = pcm_8k[i] as f64 - mean_a;
            let db = pcm_16k_decimated[i] as f64 - mean_b;
            cov += da * db;
            var_a += da * da;
            var_b += db * db;
        }
        let corr = cov / (var_a * var_b).sqrt();
        println!("decode_16k (disabled, decimated) vs decode(): correlation={corr}");
        assert!(
            corr > 0.99,
            "decode_16k's own reused-harmonics content diverged from the base decoder: correlation={corr} (expected > 0.99)"
        );
    }

    /// The one measurement the disabled-path test above can't make:
    /// with Spectral Bridge *enabled* (the real default), does the
    /// newly-available 4-8kHz band actually carry extrapolated energy,
    /// and is that energy bounded relative to the real 0-4kHz band
    /// (not a runaway fit)? Every other Spectral Bridge test either
    /// disables extrapolation or checks the amplitude array directly,
    /// never the resulting 16kHz audio itself -- this closes that gap.
    #[test]
    fn decode_16k_with_spectral_bridge_enabled_places_bounded_energy_in_the_new_4_to_8khz_band() {
        let bits_path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/codec2_3200/synthetic_c_encoded_bits.bin"
        );
        let bits_data = std::fs::read(bits_path).unwrap_or_else(|e| panic!("{bits_path}: {e}"));
        let n_frames = bits_data.len() / BYTES_PER_FRAME;
        assert!(n_frames > 150, "expected the real captured fixture corpus, got {n_frames} frames");

        let mut decoder_16k = Decoder::new();
        assert!(decoder_16k.spectral_bridge.enabled, "Spectral Bridge should be on by default");

        let mut pcm_16k: Vec<f32> = Vec::with_capacity(n_frames * 2 * spectral_bridge::N_SAMP_SB);
        for f in 0..n_frames {
            let frame: [u8; BYTES_PER_FRAME] =
                bits_data[f * BYTES_PER_FRAME..(f + 1) * BYTES_PER_FRAME].try_into().unwrap();
            let out_16k = decoder_16k.decode_16k(&frame);
            pcm_16k.extend(out_16k.iter().map(|&s| s as f32));
        }

        // Non-overlapping 1024-sample (64ms @ 16kHz) DFT windows -- bin
        // k covers k*(16000/1024)=15.625Hz, so bins 0..256 are 0-4kHz
        // and 256..512 are the new 4-8kHz band (Nyquist at bin 512).
        const WIN: usize = 1024;
        let mut planner = rustfft::FftPlanner::<f32>::new();
        let fft = planner.plan_fft_forward(WIN);
        let mut low_energy = 0.0f64;
        let mut high_energy = 0.0f64;
        let mut windows = 0usize;
        for chunk in pcm_16k.chunks_exact(WIN) {
            let mut buf: Vec<rustfft::num_complex::Complex32> =
                chunk.iter().map(|&s| rustfft::num_complex::Complex32::new(s, 0.0)).collect();
            fft.process(&mut buf);
            for (k, c) in buf.iter().enumerate().take(WIN / 2) {
                let e = (c.norm() as f64).powi(2);
                if k < WIN / 4 {
                    low_energy += e;
                } else {
                    high_energy += e;
                }
            }
            windows += 1;
        }
        assert!(windows > 50, "expected enough 1024-sample windows to be meaningful, got {windows}");

        println!("decode_16k (enabled): low(0-4kHz) energy={low_energy:e}, high(4-8kHz) energy={high_energy:e}, ratio={:e}", high_energy / low_energy);
        assert!(
            high_energy > low_energy * 1e-4,
            "extrapolated 4-8kHz band carries ~no energy (high={high_energy:e}, low={low_energy:e}) -- Spectral Bridge looks like a no-op on this fixture"
        );
        assert!(
            high_energy < low_energy,
            "extrapolated 4-8kHz band ({high_energy:e}) exceeds the real 0-4kHz band ({low_energy:e}) -- the amplitude fit may be running away despite its beta.min(0.0) clamp"
        );
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
        let bits_path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/codec2_3200/synthetic_c_encoded_bits.bin"
        );
        let pcm_path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/codec2_3200/synthetic_c_decoded_pcm.bin"
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
            let frame: [u8; BYTES_PER_FRAME] = bits_data
                [f * BYTES_PER_FRAME..(f + 1) * BYTES_PER_FRAME]
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
        assert!(corr > 0.99, "decoder output diverged from the real reference decoder on the same real captured bitstream: correlation={corr} (expected > 0.99, measured 0.9987 when this test was written)");
    }

    /// `DecoderFixed`'s own acceptance bar -- same real captured
    /// bitstream/PCM fixture, same correlation threshold as `Decoder`'s
    /// own test above, plus an RMS comparison the correlation check
    /// alone wouldn't catch: correlation is scale-invariant, so a
    /// uniform gain bug (e.g. a missing or extra `1/FFT_ENC` somewhere
    /// in the fixed-point synthesis chain) would pass a correlation-only
    /// check and only show up as implausibly loud or quiet audio.
    #[test]
    fn decoder_fixed_matches_the_real_reference_decoder_on_a_real_captured_synthetic_signal_bitstream(
    ) {
        let bits_path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/codec2_3200/synthetic_c_encoded_bits.bin"
        );
        let pcm_path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/codec2_3200/synthetic_c_decoded_pcm.bin"
        );

        let bits_data = std::fs::read(bits_path).unwrap_or_else(|e| panic!("{bits_path}: {e}"));
        let pcm_data = std::fs::read(pcm_path).unwrap_or_else(|e| panic!("{pcm_path}: {e}"));
        let n_frames = bits_data.len() / BYTES_PER_FRAME;
        assert!(
            n_frames > 150,
            "expected the real captured fixture corpus, got {n_frames} frames"
        );

        let mut decoder = DecoderFixed::new();
        let mut fixed_pcm: Vec<i16> = Vec::with_capacity(n_frames * SAMPLES_PER_FRAME);
        for f in 0..n_frames {
            let frame: [u8; BYTES_PER_FRAME] = bits_data
                [f * BYTES_PER_FRAME..(f + 1) * BYTES_PER_FRAME]
                .try_into()
                .unwrap();
            fixed_pcm.extend_from_slice(&decoder.decode(&frame));
        }
        let ref_pcm: Vec<i16> = pcm_data
            .chunks_exact(2)
            .map(|b| i16::from_le_bytes([b[0], b[1]]))
            .collect();

        let n = fixed_pcm.len();
        let mean_a: f64 = fixed_pcm.iter().map(|&s| s as f64).sum::<f64>() / n as f64;
        let mean_b: f64 = ref_pcm.iter().map(|&s| s as f64).sum::<f64>() / n as f64;
        let mut cov = 0.0f64;
        let mut var_a = 0.0f64;
        let mut var_b = 0.0f64;
        for i in 0..n {
            let da = fixed_pcm[i] as f64 - mean_a;
            let db = ref_pcm[i] as f64 - mean_b;
            cov += da * db;
            var_a += da * da;
            var_b += db * db;
        }
        let corr = cov / (var_a * var_b).sqrt();
        let rms_a = (var_a / n as f64 + mean_a * mean_a).sqrt();
        let rms_b = (var_b / n as f64 + mean_b * mean_b).sqrt();
        let rms_ratio = rms_a / rms_b;
        println!("DecoderFixed vs reference: correlation={corr}, rms_fixed={rms_a}, rms_reference={rms_b}, ratio={rms_ratio}");
        // Measured 1.0005 (essentially exact scale match -- no missing
        // or extra FFT_ENC factor anywhere in the fixed-point synthesis
        // chain) and correlation 0.9964 when this test was written;
        // real margin either side, not a loosened guess.
        assert!(
            (0.9..1.1).contains(&rms_ratio),
            "DecoderFixed's own RMS ({rms_a}) diverged from the reference's ({rms_b}), ratio={rms_ratio} -- looks like a gain/scale bug, not fixed-point rounding noise"
        );
        assert!(corr > 0.99, "DecoderFixed diverged from the real reference decoder on the same real captured bitstream: correlation={corr} (expected > 0.99, measured 0.9964 when this test was written)");
    }
}
