// SPDX-License-Identifier: AGPL-3.0-or-later
//! The transmit gate on a virtual radio: closed by default, opened by an explicit permit (or by a
//! lease that takes one), unkeying always allowed. Nothing here touches a real transmitter.
use std::error::Error;
use std::time::Duration;

use radioctl_protocols::prelude::*;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn Error>> {
    let SpawnedVirtualRig { rig, control } = VirtualRig::spawn("elecraft-kx3")?;

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
    println!("virtual radio is transmitting: {}", control.state().ptt);

    // Unkeying uses the emergency path and does not need the gate.
    rig.close_transmit_gate();
    rig.set_ptt(false).await?;
    println!(
        "gate closed again: unkeyed, virtual radio transmitting: {}",
        control.state().ptt
    );

    // A lease is the same decision for a bounded time: it takes the permit, opens the gate, and
    // closes it and unkeys when it ends (here by `transmit_for` returning and the lease dropping).
    let lease = rig.transmit_lease(
        TransmitPermit::new("manual example: lease"),
        Duration::from_secs(5),
    )?;
    lease.transmit_for(Duration::from_millis(100)).await?;
    drop(lease);
    println!("lease ended: gate permit {:?}", rig.transmit_permit());
    Ok(())
}
