//! Fixed-point AMBE+2 half-rate port -- see [`super`]'s own doc comment for the numeric convention.
//! The bit-level FEC/whitening/interleave layer (`parse_frame`, `interleaved_to_frame`) is already
//! pure integer/bitwise arithmetic (reused from [`super::super::float::ambe_plus_2`]/
//! [`super::super::float::dstar`] directly, same reasoning as [`super::super::general`]'s own FEC
//! reuse) -- only [`decode`]'s own parameter dequantization genuinely needs a fixed-point port, and
//! it's built almost entirely on [`super::super::general::mbe_speech`]'s shared core.
//!
//! Gated behind the same `ambe_plus_2` feature as the float sibling (patent-clearance scope, see
//! `AMBE_PLUS_2_NOTES.md`).

pub mod decode;
pub mod encode;
mod tables_q16;
