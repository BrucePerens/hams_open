//! AMBE+2 half-rate (the DMR / Yaesu System Fusion / P25 Phase 2 generation, TIA-102.BABA-1's own
//! 2009 addendum to the base P25 IMBE standard `super::tia_102_baba` implements): a 72-bit frame every
//! 20ms (3600 total / 2450 speech / 1150 FEC bps -- DVSI's own USB-3000 Manual lists this as
//! `PKT_RATET` Rate Index 33 "APCO Project 25 half-rate with FEC" and Rate Index 34 "APCO Project
//! 25 half-rate with No FEC", `0x21`/`0x22`; see
//! `docs/references/AMBE_CHIP_VALIDATION_FINDINGS.md`'s newest section for how those indices were
//! found and used to test this module against the real chip).
//!
//! # Gated behind the `ambe_plus_2` Cargo feature, off by default
//!
//! AMBE+2 is covered by 12 specific patents named in the TIA-102.BABA-1 addendum itself. This
//! module exists **only** for internal testing against real DVSI chip hardware -- confirming or
//! refuting the hypothesis that the chip's own "P25" configurations are actually running an
//! AMBE+2-family algorithm, not the published, patent-clear IMBE algorithm `super::tia_102_baba`
//! implements. It is not enabled by default, not exported for any deployment use, and real
//! deployment would need the patent-clearance question resolved separately (see
//! `AMBE_PLUS_2_NOTES.md`'s own dated sections for the authorization history). Build/test it with
//! `cargo build --features ambe_plus_2` / `cargo test --features ambe_plus_2`.
//!
//! # Frame structure: identical to `super::dstar`'s own frame layer, different tables
//!
//! Traced directly from mbelib's real `ambe3600x2450.c`/`ambe3600x2450_const.h`
//! (<https://github.com/szechyjs/mbelib>, ISC-licensed): **the 72-bit FEC/whitening frame layer
//! here is bit-for-bit identical to D-STAR's own** (`mbe_eccAmbe3600x2450C0`'s `in[j] =
//! ambe_fr[0][j+1]` is the same spare-bit convention as `mbe_eccAmbe3600x2400C0`;
//! `mbe_demodulateAmbe3600x2450Data`'s pseudo-random generator, seeded from `ambe_fr[0][23..12]`
//! with the same `173*p+13849 mod 65536` recurrence, is the same construction
//! `ambe_dstar::whitening` already implements and tests). Rather than reimplement it, this module
//! reuses `super::dstar`'s frame layer directly -- see the re-exports below -- exactly the
//! same "reuse, don't reimplement" precedent `ambe_dstar` itself set by reusing
//! `super::general::fec::golay_encode`/`golay_decode`.
//!
//! Four sub-blocks, `C0 || C1 || C2 || C3` for `24 + 23 + 11 + 14 = 72` bits: `C0` (a `[23,12]`
//! Golay codeword plus 1 spare LSB), `C1` (a second Golay codeword, whitened), `C2` (11
//! unprotected bits), `C3` (14 unprotected bits). Golay-correcting both yields `12 + 12 + 11 + 14 =
//! 49` decoded data bits, numbered `d[0..49)` in that concatenation order.
//!
//! # The 49 decoded bits -> nine parameters (`b0..b8`)
//!
//! This scatter is genuinely different from D-STAR's own (traced directly from mbelib's real
//! `mbe_decodeAmbe2450Parms`, which builds each `bN` via explicit `ambe_d[i]<<k` shifts over its
//! own 49-element `ambe_d` array -- `ambe_d[i]` here is this module's `d[i]`, 0-indexed from the
//! array's own start, unlike `ambe_dstar`'s `d[a..b)` MSB-relative notation):
//!
//! | Parameter | Bits | Meaning | Source bits (direct `d[]` indices) |
//! |---|---|---|---|
//! | `b0` | 7 | Pitch index -- [`tables::W0_TABLE`]/[`tables::L_TABLE`] (120 real codes; 120-127 reserved, see below) | `d[0..4)`, `d[37..40)` |
//! | `b1` | 5 | Voicing pattern -- [`tables::VUV`] | `d[4..8)`, `d[35]` |
//! | `b2` | 5 | Gain delta `Δγ` -- [`tables::DG`] | `d[8..12)`, `d[36]` |
//! | `b3` | 9 | PRBA gains for harmonics 2-4 -- [`tables::PRBA24`] | `d[12..20)`, `d[40]` |
//! | `b4` | 7 | PRBA gains for harmonics 5-8 -- [`tables::PRBA58`] | `d[20..24)`, `d[41..44)` |
//! | `b5` | 5 | Higher-order coefficients, block 1 -- [`tables::HOC_B5`] | `d[24..28)`, `d[44]` |
//! | `b6` | 4 | Higher-order coefficients, block 2 -- [`tables::HOC_B6`] | `d[28..31)`, `d[45]` |
//! | `b7` | 4 | Higher-order coefficients, block 3 -- [`tables::HOC_B7`] | `d[31..34)`, `d[46]` |
//! | `b8` | 3 | Higher-order coefficients, block 4 -- [`tables::HOC_B8`] | `d[34]`, `d[47..49)` |
//!
//! This scatter is a genuine bijection over all 49 bits of `d[]` (unlike D-STAR's own, which
//! leaves `d[24]` -- `C2`'s own first bit -- completely unread): every index 0..49 is used by
//! exactly one parameter, exactly once (`decode.rs`'s own tests check this directly).
//!
//! `b0` is a 7-bit field (0..=127) but the pitch table (Annex A) only defines 120 real codes
//! (0..=119); mbelib's real decode logic treats 120-127 as special frame types, reused here as
//! [`decode::FrameKind`]: 120-123 = erasure, 124-125 = silence (fixed `L=14`, `w0=2*pi/32`),
//! 126-127 = tone (a special pure-tone encoding with its own dedicated parameter table, Annex J --
//! **not implemented here**, stubbed as [`decode::FrameKind::Tone`] with the raw frame data
//! preserved, since decoding it needs Annex J's own formula-driven/tabulated `f0`/`l1`/`l2` lookup
//! that `AMBE_PLUS_2_NOTES.md` already has recorded but this pass didn't wire up -- a real,
//! disclosed gap, not a silent skip).
//!
//! # Scope, stated honestly
//!
//! Like `ambe_dstar`, this module recovers real semantic parameters (harmonic count, fundamental
//! frequency, per-harmonic voicing, reconstructed spectral amplitudes `Ml`) for validating against
//! the real chip's own bitstream -- not a full audio-synthesis stage. The encoder
//! ([`encode`]/[`quantize`]) performs the real inverse: nearest-codeword/nearest-scalar
//! quantization into `b0..b8`, not full PCM analysis (no pitch estimation from audio) -- the same
//! scope boundary `ambe_dstar::encode` already draws.

pub mod decode;
pub mod encode;
pub mod encoder;
pub mod interleave;
pub mod quantize;
pub mod synthesis;
pub mod tables;

/// Total frame size: `24 + 23 + 11 + 14`, identical to `ambe_dstar::FRAME_BITS`.
pub const FRAME_BITS: usize = 72;
/// Real decoded parameter bits after Golay correction: `12 + 12 + 11 + 14`.
pub const DECODED_BITS: usize = 49;

// The FEC/whitening frame layer is bit-for-bit identical to ambe_dstar's own (see this module's
// doc comment) -- reused directly rather than reimplemented, the same precedent ambe_dstar itself
// set reusing ambe::fec.
pub use crate::ambe::float::dstar::decode::{parse_frame, ParsedFrame};
pub use crate::ambe::float::dstar::encode::build_frame;
pub use crate::ambe::float::dstar::encode::build_frame as build_frame_from_d;
pub use crate::ambe::float::dstar::whiten_c1;
