// SPDX-License-Identifier: AGPL-3.0-or-later
//! `radioctl-protocols`: protocol engines, one per family (Icom CI-V, Kenwood ASCII, ...), and
//! the virtual rigs generated from rig descriptions.
//!
//! Every engine is sans-IO: a pure state machine that takes bytes and a caller-supplied
//! clock and returns bytes and events. No engine touches a socket, a serial port or a runtime,
//! so each can be replayed from a transcript and fuzzed. Only the optional `io` module (feature
//! `tokio-io`, on by default) uses tokio, to run a virtual rig behind an in-memory stream.
//! Bytes are always handled with checked access (`get`, never indexing): reply parsing is the
//! attack surface.

pub mod civ;
pub mod civ_scope;
pub mod conformance;
pub mod elecraft;
pub mod flex;
pub mod kenwood;
pub mod virtual_rig;
pub mod yaesu;
pub mod yaesu_legacy;

#[cfg(feature = "tokio-io")]
pub mod io;
