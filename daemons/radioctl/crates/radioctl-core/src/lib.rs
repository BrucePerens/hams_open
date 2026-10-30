// SPDX-License-Identifier: AGPL-3.0-or-later
//! `radioctl-core`: the typed vocabulary and the rig actor of radioctl.
//!
//! Design rules (see `docs/DESIGN.md`): no global state, no library-wide lock, no runtime of
//! its own by default (the caller supplies a tokio runtime `Handle`), units in type names,
//! protocol engines sans-IO.

pub mod backoff;
pub mod capability;
pub mod description;
pub mod dispatcher;
pub mod engine;
pub mod error;
pub mod event;
pub mod keyer;
pub mod op;
pub mod poll;
pub mod rig;
pub mod rigfile;
pub mod serial;
pub mod tcp;
pub mod transcript;
pub mod transport;
pub mod units;
pub mod usb;
