// SPDX-License-Identifier: AGPL-3.0-or-later
//! Status changes: the async event stream, with a second subscriber on its own task, fed by a knob
//! turned on a virtual radio.
use std::error::Error;
use std::time::Duration;

use radioctl_core::op::{Field, Observation, Op};
use radioctl_core::rig::{spawn_current, RigOptions};
use radioctl_core::rigfile;
use radioctl_core::units::{Hz, Vfo};
use radioctl_protocols::elecraft::ElecraftEngine;
use radioctl_protocols::io::spawn_virtual_rig;
use radioctl_protocols::virtual_rig::{PanelChange, VirtualRig};
use tokio::runtime::Handle;
use tokio::time::timeout;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn Error>> {
    let description = rigfile::parse(include_str!("../../../rigs/elecraft-kx3.toml"))?.description;
    let radio = VirtualRig::from_description(&description)?;
    let (transport, front_panel) = spawn_virtual_rig(&Handle::current(), radio, Duration::ZERO);

    let rig = spawn_current(
        ElecraftEngine::new(description)?,
        transport,
        RigOptions::default(),
    )?;
    // Every `events()` call is its own subscriber. This one runs on a task of its own, so a slow
    // subscriber never delays the actor or another subscriber; it only skips events.
    let mut other = rig.events();
    let (seen, subscriber_events) = tokio::sync::mpsc::unbounded_channel();
    tokio::spawn(async move {
        while let Some(event) = other.next().await {
            if seen.send(event).is_err() {
                break;
            }
        }
    });
    let mut stream = rig.events();

    // Ask the radio to report changes by itself, then turn a knob on it.
    rig.execute(Op::SetTransceive(true)).await?;
    front_panel.front_panel(PanelChange::Frequency(Hz(7_100_000)));

    let wanted = Field::Frequency(Vfo::A);
    loop {
        let event = timeout(Duration::from_secs(5), stream.next())
            .await?
            .ok_or("the rig stopped")?;
        if let Some(change) = event.changes.iter().find(|c| c.field == wanted) {
            if change.new == Observation::Frequency(Vfo::A, Hz(7_100_000)) {
                println!(
                    "stream: VFO A changed to 7100000 Hz (source {:?})",
                    event.source
                );
                break;
            }
        }
    }
    // The other subscriber saw the same changes, in order, on its own task.
    let mut subscriber_events = subscriber_events;
    loop {
        let delivered = timeout(Duration::from_secs(5), subscriber_events.recv())
            .await?
            .ok_or("the subscriber task ended")?;
        if delivered.changes.iter().any(|c| c.field == wanted) {
            println!(
                "subscriber: rig {} reported the change of VFO A",
                delivered.rig
            );
            break;
        }
    }
    Ok(())
}
