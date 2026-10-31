// SPDX-License-Identifier: AGPL-3.0-or-later
//! The usual names of radioctl-core in one `use`:
//!
//! ```
//! use radioctl_core::prelude::*;
//! ```
//!
//! Only the vocabulary a program needs to read and set a rig, watch it and (deliberately) hold its
//! transmit gate. The lower-level modules stay where they are and are not shadowed. Nothing here
//! opens a gate or starts a task. `radioctl_protocols::prelude` adds the engines and the
//! constructors that need them.

pub use crate::capability::Capability;
pub use crate::description::RigDescription;
pub use crate::error::{ErrorKind, RigError};
pub use crate::event::{Change, RigEvent, Source};
pub use crate::op::{Field, Observation, Op};
pub use crate::rig::{
    spawn, spawn_current, Cached, EventStream, Freshness, RigHandle, RigOptions, ScopeStream,
    TransmitLease, TransmitPermit,
};
pub use crate::rigfile;
pub use crate::units::{Hz, Level, Meter, Mode, Vfo};
