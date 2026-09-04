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

pub mod lpc;
pub mod nlp;
pub mod quantise;
pub mod synthesis;
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
