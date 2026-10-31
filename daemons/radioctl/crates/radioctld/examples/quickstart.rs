// SPDX-License-Identifier: AGPL-3.0-or-later
//! Quick start: a virtual Elecraft KX3, driven through a `RigHandle`.
use radioctl_protocols::prelude::*;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let rig = VirtualRig::spawn("elecraft-kx3")?.rig;
    println!(
        "VFO A: {} Hz",
        rig.frequency(Vfo::A, Freshness::Fresh).await?.value.0
    );
    rig.set_frequency(Vfo::A, Hz(14_074_000)).await?;
    rig.set_mode(Mode::Usb).await?;
    println!("mode: {:?}", rig.mode(Freshness::Cached).await?.value);
    // Transmitting is refused until someone opens the transmit gate.
    let refused = rig
        .set_ptt(true)
        .await
        .err()
        .ok_or("the closed gate let a transmit command through")?;
    println!("keying refused: {:?}", refused.kind);
    Ok(())
}
