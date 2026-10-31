// SPDX-License-Identifier: AGPL-3.0-or-later
//! Status changes: the callback (its own thread) and the async event stream, fed by a knob
//! turned on a virtual radio.
use std::error::Error;
use std::sync::mpsc;
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

    // The callback is plain synchronous code. It runs on a dedicated thread, never on the
    // actor, so a slow callback cannot delay an unkey.
    let (seen, callback_events) = mpsc::channel();
    let options = RigOptions::default().with_callback(move |event| {
        let _ = seen.send(event);
    });
    let rig = spawn_current(ElecraftEngine::new(description)?, transport, options)?;
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
    let delivered = callback_events.recv_timeout(Duration::from_secs(5))?;
    println!(
        "callback: rig {} reported {} change(s)",
        delivered.rig,
        delivered.changes.len()
    );
    Ok(())
}
