// SPDX-License-Identifier: AGPL-3.0-or-later
//! Quick start: a virtual Elecraft KX3 behind a rig actor, driven through a `RigHandle`.
use std::error::Error;
use std::time::Duration;

use radioctl_core::rig::{spawn_current, Freshness, RigOptions};
use radioctl_core::rigfile;
use radioctl_core::units::{Hz, Mode, Vfo};
use radioctl_protocols::elecraft::ElecraftEngine;
use radioctl_protocols::io::spawn_virtual_rig;
use radioctl_protocols::virtual_rig::VirtualRig;
use tokio::runtime::Handle;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn Error>> {
    // The rig description is data; the virtual rig and the engine are both built from it.
    let description = rigfile::parse(include_str!("../../../rigs/elecraft-kx3.toml"))?.description;
    let radio = VirtualRig::from_description(&description)?;
    let (transport, _front_panel) = spawn_virtual_rig(&Handle::current(), radio, Duration::ZERO);
    // The caller's runtime runs the actor; the library creates none.
    let rig = spawn_current(
        ElecraftEngine::new(description)?,
        transport,
        RigOptions::default(),
    )?;

    let frequency = rig.frequency(Vfo::A, Freshness::Fresh).await?;
    println!("VFO A: {} Hz", frequency.value.0);
    rig.set_frequency(Vfo::A, Hz(14_074_000)).await?;
    rig.set_mode(Mode::Usb).await?;
    let mode = rig.mode(Freshness::Cached).await?;
    println!("mode: {:?}", mode.value);

    // Transmitting is refused until someone opens the transmit gate.
    match rig.set_ptt(true).await {
        Err(error) => println!("keying refused: {:?}", error.kind),
        Ok(()) => return Err("the closed gate let a transmit command through".into()),
    }
    Ok(())
}
