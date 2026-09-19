//! Fixed-point RATET(27)/P25 full-rate port -- see [`super`]'s own doc comment for the numeric
//! convention (Q16.16 unless stated) and tolerance policy. Started with `parameter_encoding` (the
//! fundamental-frequency/voicing-decision dequantization), the first stage of
//! [`super::super::float::ratet27::decode`]'s own decode pipeline -- see that module's doc comment
//! for the full decode order this fixed-point port will eventually mirror stage by stage.
//!
//! The bit-level FEC/interleave/wire-format layer (`ratet27_fec`, `ratet27_frame`,
//! `ratet27_wire_format`, `bit_prioritization`, `modulation`, `ratet27_dtx`,
//! [`super::super::float::dstar`]'s Golay/Hamming via [`super::super::general::fec`]) is already
//! pure integer/bitwise arithmetic and is reused directly from [`super::super::float::ratet27`]
//! rather than duplicated here -- only the parameter dequantization and synthesis math genuinely
//! needs a fixed-point port.

pub mod parameter_encoding;
pub mod prediction;
pub mod reconstruct;
mod reconstruct_tables;
