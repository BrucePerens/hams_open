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
//! interoperability, not just internal self-consistency. **The decoder
//! half of this codec (LSP-to-LPC, spectral envelope, phase/sinusoidal
//! synthesis, postfilter, IDFT/overlap-add) is not yet implemented** --
//! `synthesis.rs` is still a stub.
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
