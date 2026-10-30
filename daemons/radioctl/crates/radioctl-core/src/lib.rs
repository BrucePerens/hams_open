// SPDX-License-Identifier: AGPL-3.0-or-later
//! `radioctl-core`: the typed vocabulary and (from PR 3 on) the rig actor of radioctl.
//!
//! Design rules (see `docs/DESIGN.md`): no global state, no library-wide lock, no runtime of
//! its own by default, units in type names.

pub mod units;
