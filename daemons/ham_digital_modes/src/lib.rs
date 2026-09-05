// SPDX-License-Identifier: LGPL-3.0-or-later
//! Amateur radio digital mode encode/decode. Moved here from hams_com's
//! proprietary hams_local_relay daemon specifically so decode
//! implementations can depend on GPL-3.0 reference code (e.g. an FT8/WSPR
//! decoder derived from or bound to the WSJT-X lineage) without a
//! license conflict -- an LGPL-3.0 project can incorporate GPL-3.0
//! dependencies; a proprietary/trade-secret one cannot.

pub mod codec2_1600;
pub mod codec2_3200;
pub mod ft8;
pub mod psk31;
pub mod rtty;
pub mod wspr;
pub mod wspr_decode;
pub mod wspr_sync;
