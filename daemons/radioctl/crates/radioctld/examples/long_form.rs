// SPDX-License-Identifier: AGPL-3.0-or-later
//! The quick start without the helpers: what `VirtualRig::spawn("elecraft-kx3")` does, call by
//! call. The lower-level API is unchanged and is what the helpers are made of.
use std::error::Error;
use std::time::Duration;

use radioctl_core::rig::{spawn_current, Freshness, RigOptions};
use radioctl_core::rigfile;
use radioctl_core::units::{Hz, Vfo};
use radioctl_protocols::conformance::engine_for;
use radioctl_protocols::io::spawn_virtual_rig;
use radioctl_protocols::virtual_rig::VirtualRig;
use tokio::runtime::Handle;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn Error>> {
    // 1. The rig description is data; the virtual rig and the engine are both built from it.
    let description = rigfile::load("elecraft-kx3")?.description;
    // 2. The stand-in radio, run behind an in-memory stream on the caller's runtime.
    let radio = VirtualRig::from_description(&description)?;
    let (transport, _front_panel) = spawn_virtual_rig(&Handle::current(), radio, Duration::ZERO);
    // 3. The engine for the description's protocol family, and the rig actor over both.
    let engine = engine_for(&description)?;
    let rig = spawn_current(engine, transport, RigOptions::default())?;

    let frequency = rig.frequency(Vfo::A, Freshness::Fresh).await?;
    println!("VFO A: {} Hz", frequency.value.0);
    rig.set_frequency(Vfo::A, Hz(14_074_000)).await?;
    Ok(())
}
