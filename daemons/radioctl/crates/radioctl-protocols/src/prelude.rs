// SPDX-License-Identifier: AGPL-3.0-or-later
//! The usual names of radioctl in one `use`: everything in `radioctl_core::prelude`, the engine
//! factory, the virtual rig, and (with the default `tokio-io` feature) the one-call constructors.
//!
//! ```
//! use radioctl_protocols::prelude::*;
//! ```

pub use radioctl_core::prelude::*;

pub use crate::conformance::engine_for;
pub use crate::virtual_rig::{PanelChange, VirtualRig};

#[cfg(feature = "tokio-io")]
pub use crate::easy::{
    open_serial_rig, open_serial_rig_with, spawn_virtual, SerialRig, SpawnedVirtualRig,
};
#[cfg(feature = "tokio-io")]
pub use crate::io::VirtualRigControl;
