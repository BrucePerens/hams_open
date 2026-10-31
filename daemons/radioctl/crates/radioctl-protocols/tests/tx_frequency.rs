// SPDX-License-Identifier: AGPL-3.0-or-later
//! The transmit frequency is first class (Hamlib issue 138): in split operation, with the rig
//! keyed, `active_tx_frequency` is the transmit VFO's frequency on every engine, and the
//! status-change callback reports it.
//!
//! Everything here runs on virtual rigs in memory. The transmit gate stays closed throughout, so
//! the actor refuses every keying command; the "key" is the virtual rig's front-panel PTT, which
//! sets a field in memory and reaches no hardware.
#![allow(clippy::expect_used, clippy::panic)]

use std::sync::{Arc, Mutex};
use std::time::Duration;

use radioctl_core::description::RigDescription;
use radioctl_core::engine::ProtocolEngine;
use radioctl_core::event::RigEvent;
use radioctl_core::op::{Field, Observation, Op};
use radioctl_core::rig::{spawn, Freshness, RigHandle, RigOptions};
use radioctl_core::rigfile;
use radioctl_core::units::{Hz, Vfo};
use radioctl_protocols::civ::CivEngine;
use radioctl_protocols::elecraft::ElecraftEngine;
use radioctl_protocols::io::{spawn_virtual_rig, VirtualRigControl};
use radioctl_protocols::kenwood::KenwoodEngine;
use radioctl_protocols::virtual_rig::{PanelChange, VirtualRig};
use radioctl_protocols::yaesu::YaesuEngine;
use tokio::runtime::Handle;

const RX: Hz = Hz(14_195_000);
const TX: Hz = Hz(14_205_000);

fn description(file: &str) -> RigDescription {
    let text = std::fs::read_to_string(format!(
        "{}/../../rigs/{file}.toml",
        env!("CARGO_MANIFEST_DIR")
    ))
    .expect("rig file");
    rigfile::parse(&text).expect("rig description").description
}

fn start<E: ProtocolEngine>(
    desc: &RigDescription,
    engine: E,
    events: Arc<Mutex<Vec<RigEvent>>>,
) -> (RigHandle, VirtualRigControl) {
    let rig = VirtualRig::from_description(desc).expect("virtual rig");
    let (stream, control) = spawn_virtual_rig(&Handle::current(), rig, Duration::ZERO);
    let mut options = RigOptions::default().with_callback(move |event| {
        events.lock().expect("events").push(event);
    });
    options.poll.enabled = false;
    let handle = spawn(&Handle::current(), engine, stream, options).expect("spawn");
    (handle, control)
}

/// Split with the transmit VFO selected and the rig keyed: the amplifier follower's value is
/// the transmit VFO's frequency, and the receive frequency is still the receive VFO's.
async fn split_keyed_scenario<E: ProtocolEngine>(desc: RigDescription, engine: E) {
    let events = Arc::new(Mutex::new(Vec::new()));
    let (rig, control) = start(&desc, engine, events.clone());
    rig.set_frequency(Vfo::A, RX).await.expect("set a");
    rig.set_frequency(Vfo::B, TX).await.expect("set b");
    let fresh = Freshness::Fresh;

    // Simplex: transmit where the rig receives.
    rig.execute(Op::SetSplit(false)).await.expect("simplex");
    assert_eq!(rig.active_tx_frequency(fresh).await.expect("tx"), RX);

    // Split, keyed from the front panel (the transmit gate is closed: nothing keys hardware).
    rig.execute(Op::SetSplit(true)).await.expect("split");
    control.front_panel(PanelChange::Ptt(true));
    assert!(rig.ptt(fresh).await.expect("ptt").value, "rig reads keyed");
    assert!(rig.transmit_permit().is_none(), "the gate stayed closed");
    assert!(rig.split(fresh).await.expect("split").value);
    assert_eq!(rig.tx_vfo(fresh).await.expect("vfo"), Vfo::B);
    assert_eq!(rig.rx_frequency(fresh).await.expect("rx"), RX);
    assert_eq!(rig.tx_frequency(fresh).await.expect("tx"), TX);
    assert_eq!(rig.active_tx_frequency(fresh).await.expect("active"), TX);
    assert_eq!(
        rig.cached(Field::ActiveTxFrequency).map(|c| c.value),
        Some(Observation::ActiveTxFrequency(TX))
    );

    // The callback saw the transmit frequency change, from RX to TX.
    let mut seen = Vec::new();
    for _ in 0..200 {
        seen = events
            .lock()
            .expect("events")
            .iter()
            .flat_map(|e| e.changes.clone())
            .filter(|c| c.field == Field::ActiveTxFrequency)
            .collect();
        if seen
            .iter()
            .any(|c| c.new == Observation::ActiveTxFrequency(TX))
        {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(
        seen.iter()
            .any(|c| c.old == Some(Observation::ActiveTxFrequency(RX))
                && c.new == Observation::ActiveTxFrequency(TX)),
        "callback changes: {seen:?}"
    );

    // Leaving split brings the transmit frequency back to the receive frequency.
    control.front_panel(PanelChange::Ptt(false));
    rig.execute(Op::SetSplit(false)).await.expect("simplex");
    assert_eq!(rig.active_tx_frequency(fresh).await.expect("tx"), RX);
    assert_eq!(rig.stats().commands_failed, 0);
}

// Tests [@ANCHOR: radioctl_core_handle_active_tx_frequency]
// Tests [@ANCHOR: radioctl_core_handle_tx_frequency]
// Tests [@ANCHOR: radioctl_core_handle_rx_frequency]
// Tests [@ANCHOR: radioctl_core_handle_tx_vfo]
// Tests [@ANCHOR: radioctl_core_handle_tx_state]
// Tests [@ANCHOR: radioctl_core_handle_split]
#[tokio::test(start_paused = true)]
async fn kenwood_split_keyed_transmits_on_vfo_b() {
    let desc = description("kenwood-ts-590sg");
    let engine = KenwoodEngine::new(desc.clone()).expect("engine");
    split_keyed_scenario(desc, engine).await;
}

#[tokio::test(start_paused = true)]
async fn elecraft_split_keyed_transmits_on_vfo_b() {
    let desc = description("elecraft-kx3");
    let engine = ElecraftEngine::new(desc.clone()).expect("engine");
    split_keyed_scenario(desc, engine).await;
}

#[tokio::test(start_paused = true)]
async fn civ_split_keyed_transmits_on_the_unselected_vfo() {
    let desc = description("icom-ic-7300");
    let engine = CivEngine::new(desc.clone()).expect("engine");
    split_keyed_scenario(desc, engine).await;
}

#[tokio::test(start_paused = true)]
async fn yaesu_newcat_split_keyed_transmits_on_vfo_b() {
    let desc = description("yaesu-ft-991a");
    let engine = YaesuEngine::new(desc.clone()).expect("engine");
    split_keyed_scenario(desc, engine).await;
}

// Tests [@ANCHOR: radioctl_kenwood_engine_decode_if]
// Tests [@ANCHOR: radioctl_kenwood_vfo_of]
#[test]
fn kenwood_if_and_ft_report_receive_on_b_and_transmit_on_a() {
    use radioctl_core::engine::Inbound;
    let desc = description("kenwood-ts-590sg");
    let mut engine = KenwoodEngine::new(desc.clone()).expect("engine");
    let mut rig = VirtualRig::from_description(&desc).expect("rig");
    let now = Duration::from_secs(1);
    // FR1;FT0: the rig receives on B and transmits on A, which is split.
    assert!(rig.feed(b"FR1;FT0;", now).is_empty());
    let request = engine.encode(&Op::GetSplit).expect("IF");
    let seen = engine.feed(&rig.feed(&request.bytes, now), now);
    assert!(seen.contains(&Inbound::Observation(Observation::RxVfo(Vfo::B))));
    assert!(seen.contains(&Inbound::Observation(Observation::TxVfo(Vfo::A))));
    assert!(seen.contains(&Inbound::Observation(Observation::Split(true))));
    let request = engine.encode(&Op::GetTxVfo).expect("FT");
    assert_eq!(request.bytes, b"FT;");
    let seen = engine.feed(&rig.feed(&request.bytes, now), now);
    assert_eq!(seen, vec![Inbound::Observation(Observation::TxVfo(Vfo::A))]);
}
