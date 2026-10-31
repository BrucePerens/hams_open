// SPDX-License-Identifier: AGPL-3.0-or-later
//! The transmit gate on a virtual radio: closed by default, opened with a permit, unkeying
//! always allowed. Nothing here touches a real transmitter.
use std::error::Error;
use std::time::Duration;

use radioctl_core::error::ErrorKind;
use radioctl_core::rig::{spawn_current, Freshness, RigOptions, TransmitPermit};
use radioctl_core::rigfile;
use radioctl_protocols::elecraft::ElecraftEngine;
use radioctl_protocols::io::spawn_virtual_rig;
use radioctl_protocols::virtual_rig::VirtualRig;
use tokio::runtime::Handle;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn Error>> {
    let description = rigfile::parse(include_str!("../../../rigs/elecraft-kx3.toml"))?.description;
    let radio = VirtualRig::from_description(&description)?;
    let (transport, virtual_control) = spawn_virtual_rig(&Handle::current(), radio, Duration::ZERO);
    let rig = spawn_current(
        ElecraftEngine::new(description)?,
        transport,
        RigOptions::default(),
    )?;

    let refused = rig.set_ptt(true).await.err().map(|e| e.kind);
    println!("gate closed: keying -> {refused:?}");
    assert_eq!(refused, Some(ErrorKind::TransmitGateClosed));
    assert_eq!(rig.transmit_permit(), None);

    // The permit records who decided. There is no boolean shortcut.
    rig.open_transmit_gate(TransmitPermit::new("manual example: virtual radio"));
    rig.set_ptt(true).await?;
    println!(
        "gate open: PTT reads {}",
        rig.ptt(Freshness::Fresh).await?.value
    );
    println!(
        "virtual radio is transmitting: {}",
        virtual_control.state().ptt
    );

    // Unkeying uses the emergency path and does not need the gate.
    rig.close_transmit_gate();
    rig.set_ptt(false).await?;
    println!(
        "gate closed again: unkeyed, virtual radio transmitting: {}",
        virtual_control.state().ptt
    );
    Ok(())
}
