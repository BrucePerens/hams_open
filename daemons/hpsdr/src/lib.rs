// SPDX-License-Identifier: AGPL-3.0-or-later
//! An OpenHPSDR (ANAN, Hermes) client: discovery, Protocol 1 and Protocol 2, receive I/Q for several
//! DDC receivers, hardware status, and a transmit path behind a gate that defaults closed.
//!
//! Design rules:
//! * **Sans-IO parsers.** [`p1`] (and [`p2`]) only turn settings into bytes and bytes into values.
//! * **Transmit fails closed.** [`gate::TxGate`] is closed unless the caller opens it for a bounded
//!   lease. The receive builders have no transmit parameters; the transmit builders refuse with
//!   [`error::HpsdrError::GateClosed`]. See `hams_com/docs/proposals/RELAY_SDR_RADIOCTL_ROADMAP.md` 2a.
//! * **Tokio only, no runtime of our own.** Anything that opens a socket or spawns a task takes a
//!   [`tokio::runtime::Handle`].
//! * **Everything is `verified=unverified`** until run against a real radio.
#![doc = "Modules: [`error`], [`gate`], [`iq`] stream interface, [`types`], [`p1`] Protocol 1 wire format, [`discovery`], [`session`] receive session, [`emulator`] loopback radio, [`session_p2`] Protocol 2 receive session, [`emulator_p2`] Protocol 2 loopback radio."]

pub mod discovery;
pub mod emulator;
pub mod emulator_p2;
pub mod error;
pub mod gate;
pub mod iq;
pub mod p1;
pub mod p2;
pub mod session;
pub mod session_p2;
pub mod types;
