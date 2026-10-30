// SPDX-License-Identifier: AGPL-3.0-or-later
//! `radioctl-core`: the typed vocabulary and (from the actor PR on) the rig actor of radioctl.
//!
//! Design rules (see `docs/DESIGN.md`): no global state, no library-wide lock, no runtime of
//! its own by default, units in type names, protocol engines sans-IO.

pub mod capability;
pub mod description;
pub mod engine;
pub mod error;
pub mod op;
pub mod units;
