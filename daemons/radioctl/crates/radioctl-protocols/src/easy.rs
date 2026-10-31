// SPDX-License-Identifier: AGPL-3.0-or-later
//! One-call constructors that compose the four or five calls every program used to repeat:
//! read the rig description, build its engine, open a transport, spawn the rig actor.
//!
//! - [`VirtualRig::spawn`]: a virtual rig of a named model, ready to use.
//! - [`spawn_virtual`]: the same from a description you already hold, on a runtime and options
//!   you choose.
//! - [`open_serial_rig`] and [`open_serial_rig_with`]: a real rig on a serial port.
//!
//! None of them opens the transmit gate; a rig comes back with the gate closed. Nothing here
//! creates a runtime. Everything underneath stays public (`engine_for`, `spawn_virtual_rig`,
//! `radioctl_core::serial::open_serial`, `radioctl_core::rig::spawn`) and the manual shows the
//! long form once so that a reader sees what these do.

use std::time::Duration;

use radioctl_core::capability::Capability;
use radioctl_core::description::RigDescription;
use radioctl_core::error::{ErrorKind, RigError};
use radioctl_core::rig::{spawn, RigHandle, RigOptions};
use radioctl_core::rigfile;
use radioctl_core::serial::{open_serial, PortTarget, SerialConfig, SerialMonitor};
use tokio::runtime::Handle;

use crate::conformance::engine_for;
use crate::io::{spawn_virtual_rig, VirtualRigControl};
use crate::virtual_rig::VirtualRig;

/// A virtual rig that is up: the rig handle to drive it and the control of the stand-in behind it
/// (front panel, state, fault injection).
pub struct SpawnedVirtualRig {
    /// The rig actor's handle. Its transmit gate is closed.
    pub rig: RigHandle,
    /// The virtual radio behind the handle.
    pub control: VirtualRigControl,
}

// [@ANCHOR: radioctl_easy_spawn_virtual]
/// Builds the virtual radio and the engine from `description`, runs the radio behind an in-memory
/// stream (answering after `reply_delay`) and spawns the rig actor on `runtime`. The gate is closed.
pub fn spawn_virtual(
    runtime: &Handle,
    description: RigDescription,
    options: RigOptions,
    reply_delay: Duration,
) -> Result<SpawnedVirtualRig, RigError> {
    let radio = VirtualRig::from_description(&description)?;
    let engine = engine_for(&description)?;
    let (transport, control) = spawn_virtual_rig(runtime, radio, reply_delay);
    let rig = spawn(runtime, engine, transport, options)?;
    Ok(SpawnedVirtualRig { rig, control })
}

impl VirtualRig {
    // [@ANCHOR: radioctl_easy_virtual_rig_spawn]
    /// A virtual rig of the model `which` (a rig id such as `"icom-ic-7300"`, or the path of a
    /// description file; see [`rigfile::load`]) on the current tokio runtime, with default
    /// options, answering at once. One call instead of four. The transmit gate is closed.
    pub fn spawn(which: &str) -> Result<SpawnedVirtualRig, RigError> {
        let runtime = current_runtime()?;
        spawn_virtual(
            &runtime,
            rigfile::load(which)?.description,
            RigOptions::default(),
            Duration::ZERO,
        )
    }
}

/// A rig on a serial port: the handle, and the port's status, counters and control lines. Keep
/// `link` for as long as the rig is in use: the actor's link watch and any line keyer depend on it.
pub struct SerialRig {
    /// The rig actor's handle. Its transmit gate is closed.
    pub rig: RigHandle,
    /// The port's status, counters and control lines.
    pub link: SerialMonitor,
}

// [@ANCHOR: radioctl_easy_open_serial_rig_with]
/// Opens the serial port in `config`, builds the engine for `description`, spawns the rig actor
/// with the port's link watch set (a port loss marks the link down, a reopen unkeys the rig), and
/// sends the start-up unkey that radioctld sends: a process killed while it had the radio keyed
/// leaves it keyed. Unkeying needs no gate. A radio that cannot be unkeyed is an error, because
/// it may be keyed. The transmit gate is closed.
pub async fn open_serial_rig_with(
    runtime: &Handle,
    description: &RigDescription,
    config: SerialConfig,
    mut options: RigOptions,
) -> Result<SerialRig, RigError> {
    let engine = engine_for(description)?;
    let link = open_serial(runtime, config).await?;
    options.link_watch = Some(link.watch());
    let (transport, monitor) = link.into_parts();
    let rig = spawn(runtime, engine, transport, options)?;
    if rig.capabilities().has(Capability::SetPtt) {
        rig.set_ptt(false).await.map_err(|e| {
            RigError::new(
                e.kind,
                format!(
                    "the start-up unkey failed, the radio may be keyed: {}",
                    e.detail
                ),
            )
        })?;
    }
    Ok(SerialRig { rig, link: monitor })
}

// [@ANCHOR: radioctl_easy_open_serial_rig]
/// The rig described by `which` (an id in [`rigfile::rigs_dir`], or a description file path) on
/// the serial port `port` at `baud`, 8N1, default options, on the current tokio runtime. See
/// [`open_serial_rig_with`] for what it does besides opening the port. The gate is closed.
pub async fn open_serial_rig(which: &str, port: &str, baud: u32) -> Result<SerialRig, RigError> {
    let runtime = current_runtime()?;
    let description = rigfile::load(which)?.description;
    let config = SerialConfig::new(PortTarget::Path(port.into()), baud);
    open_serial_rig_with(&runtime, &description, config, RigOptions::default()).await
}

fn current_runtime() -> Result<Handle, RigError> {
    Handle::try_current()
        .map_err(|e| RigError::new(ErrorKind::Unsupported, format!("no tokio runtime: {e}")))
}
