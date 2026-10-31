// SPDX-License-Identifier: AGPL-3.0-or-later
//! Status changes: the async event stream, with a second subscriber on its own task, fed by a knob
//! turned on a virtual radio.
use std::error::Error;
use std::time::Duration;

use radioctl_protocols::prelude::*;
use tokio::time::timeout;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn Error>> {
    let SpawnedVirtualRig {
        rig,
        control: front_panel,
    } = VirtualRig::spawn("elecraft-kx3")?;
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

    // Wait for the next change of VFO A; other events are skipped.
    let wanted = Field::Frequency(Vfo::A);
    let (change, source) = timeout(Duration::from_secs(5), stream.next_change(wanted))
        .await?
        .ok_or("the rig stopped")?;
    assert_eq!(change.new, Observation::Frequency(Vfo::A, Hz(7_100_000)));
    println!("stream: VFO A changed to 7100000 Hz (source {source:?})");

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
