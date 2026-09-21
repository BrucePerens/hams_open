// SPDX-License-Identifier: LGPL-3.0-or-later
//! Amateur radio digital mode encode/decode. Moved here from hams_com's
//! proprietary hams_local_relay daemon specifically so decode
//! implementations can depend on GPL-3.0 reference code (e.g. an FT8/WSPR
//! decoder derived from or bound to the WSJT-X lineage) without a
//! license conflict -- an LGPL-3.0 project can incorporate GPL-3.0
//! dependencies; a proprietary/trade-secret one cannot.

// `no_std` support: with `--no-default-features` only the fixed-point Codec2 3200 path is built
// (core only, no heap, no floating point). Everything else needs `std`. See docs/CODEC2_NO_STD.md.
#![cfg_attr(not(feature = "std"), no_std)]

// AMBE codec family: ambe::float::<mode> (tia_102_baba, dstar, ambe_plus_2, general) and
// ambe::fixed::<mode>. AMBE+2 half-rate (ambe::float::ambe_plus_2 / ambe::fixed::ambe_plus_2) is
// gated off by default, pending patent clearance for any real deployment use -- see
// src/ambe/float/ambe_plus_2/mod.rs's own doc comment and
// src/ambe/float/ambe_plus_2/AMBE_PLUS_2_NOTES.md for the authorization history and scope.
// Build/test with `cargo build/test --features ambe_plus_2`.
#[cfg(feature = "std")]
pub mod ambe;
pub mod codec2_1600;
pub mod codec2_3200;
#[cfg(feature = "std")]
pub mod dstar;
#[cfg(feature = "std")]
pub mod ft8;
#[cfg(feature = "std")]
pub mod psk31;
#[cfg(feature = "std")]
pub mod rtty;
#[cfg(feature = "std")]
pub mod timing_characterizer;
#[cfg(feature = "std")]
pub mod wspr;
#[cfg(feature = "std")]
pub mod wspr_decode;
#[cfg(feature = "std")]
pub mod wspr_sync;
