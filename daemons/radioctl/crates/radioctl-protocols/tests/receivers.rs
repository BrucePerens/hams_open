// SPDX-License-Identifier: AGPL-3.0-or-later
//! Every receiver-family description (`family = "receiver"`: AOR, Uniden, JRC, Icom PCR and marine,
//! Rohde and Schwarz) against its engine and virtual twin, table driven over `rigs/*.toml`. Nothing
//! here touches a radio.
#![allow(clippy::expect_used, clippy::panic)]

use std::path::Path;
use std::time::Duration;

use radioctl_core::capability::Capability;
use radioctl_core::description::{Family, RigDescription};
use radioctl_core::engine::{Inbound, ProtocolEngine, ReplyKind};
use radioctl_core::op::{Observation, Op};
use radioctl_core::rigfile;
use radioctl_core::units::{Hz, Vfo};
use radioctl_protocols::conformance::engine_for;
use radioctl_protocols::virtual_rig::{PanelChange, VirtualRig};

const NOW: Duration = Duration::from_secs(5);

/// The models of these families that transmit; every other receiver-family file must have no
/// transmit capability at all.
const TRANSCEIVERS: [&str; 6] = [
    "jrc-jst-145",
    "jrc-jst-245",
    "icom-ic-m700pro",
    "icom-ic-m710",
    "icom-ic-m802",
    "icom-ic-m803",
];

fn receivers() -> Vec<RigDescription> {
    static CACHE: std::sync::OnceLock<Vec<RigDescription>> = std::sync::OnceLock::new();
    CACHE.get_or_init(load_receivers).clone()
}

fn load_receivers() -> Vec<RigDescription> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../rigs");
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir).expect("rigs") {
        let path = entry.expect("entry").path();
        if path.extension().is_some_and(|e| e == "toml") {
            let text = std::fs::read_to_string(&path).expect("read");
            let desc = rigfile::parse(&text)
                .unwrap_or_else(|e| panic!("{}: {e}", path.display()))
                .description;
            if desc.family == Family::Receiver {
                out.push(desc);
            }
        }
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    out
}

/// Sends an operation to the twin and feeds the answer back; a setting the protocol does not
/// answer (`ReplyKind::None`) is reported as the acknowledgement it would have been, after
/// checking that the twin really said nothing.
fn exchange(engine: &mut dyn ProtocolEngine, rig: &mut VirtualRig, op: Op) -> Vec<Inbound> {
    let request = engine.encode(&op).expect("encode");
    let answer = rig.feed(&request.bytes, NOW);
    if request.reply == ReplyKind::None {
        assert!(
            answer.is_empty(),
            "{op:?}: the protocol has no answer to a setting"
        );
        return vec![Inbound::Ack];
    }
    engine.feed(&answer, NOW)
}

// Tests [@ANCHOR: radioctl_conformance_receiver_engine]
// Tests [@ANCHOR: radioctl_core_rigfile_build_receiver]
#[test]
fn the_receiver_files_are_present_and_none_of_the_receivers_can_transmit() {
    let all = receivers();
    assert!(all.len() >= 7, "AOR files at least");
    for desc in &all {
        let transmits = [Capability::SetPtt, Capability::SendCw]
            .iter()
            .any(|c| desc.capabilities.has(*c));
        if !TRANSCEIVERS.contains(&desc.id.as_str()) {
            assert!(!transmits, "{} is a receiver", desc.id);
            for c in [
                Capability::ReadPower,
                Capability::ReadSwr,
                Capability::ReadMeters,
            ] {
                assert!(
                    !desc.capabilities.has(c),
                    "{} has no transmitter meters",
                    desc.id
                );
            }
        }
        assert!(desc.receiver.is_some() && desc.civ.is_none(), "{}", desc.id);
    }
}

// Tests [@ANCHOR: radioctl_receiver_virtual_new]
// Tests [@ANCHOR: radioctl_receiver_virtual_feed]
// Tests [@ANCHOR: radioctl_receiver_engine_new]
// Tests [@ANCHOR: radioctl_receiver_engine_encode]
// Tests [@ANCHOR: radioctl_receiver_engine_feed]
// Tests [@ANCHOR: radioctl_receiver_engine_split_lines]
// Tests [@ANCHOR: radioctl_receiver_dialect_status_default]
// Tests [@ANCHOR: radioctl_receiver_twin_raw_default]
#[test]
fn every_receiver_round_trips_frequency_mode_and_signal_through_its_twin() {
    for desc in receivers() {
        let mut engine = engine_for(&desc).unwrap_or_else(|e| panic!("{}: {e}", desc.id));
        let mut rig = VirtualRig::from_description(&desc).expect("twin");
        let id = &desc.id;
        // A frequency every model covers: 100 kHz above its own starting frequency (the scanners
        // refuse what is outside their bands, so a fixed HF frequency would not do for all).
        let target = Hz(desc.default_frequency.0 + 100_000);
        if desc.capabilities.has(Capability::SetFrequency) {
            let seen = exchange(&mut *engine, &mut rig, Op::SetFrequency(Vfo::A, target));
            assert_eq!(
                seen,
                vec![Inbound::Ack],
                "{id}: set frequency is acknowledged"
            );
        }
        if desc.capabilities.has(Capability::ReadFrequency) {
            let seen = exchange(&mut *engine, &mut rig, Op::GetFrequency(Vfo::A));
            assert!(
                seen.contains(&Inbound::Observation(Observation::Frequency(
                    Vfo::A,
                    target
                ))),
                "{id}: {seen:?}"
            );
        }
        for (mode, _) in desc.mode_codes.clone() {
            if desc.capabilities.has(Capability::SetMode) {
                let seen = exchange(&mut *engine, &mut rig, Op::SetMode(mode));
                assert_eq!(seen, vec![Inbound::Ack], "{id}: set {mode:?}");
            }
            if desc.capabilities.has(Capability::ReadMode) {
                let seen = exchange(&mut *engine, &mut rig, Op::GetMode);
                assert!(
                    seen.contains(&Inbound::Observation(Observation::Mode(mode))),
                    "{id}: {mode:?} -> {seen:?}"
                );
            }
        }
        if desc.capabilities.has(Capability::ReadSignal) {
            for level in [0u16, 5, 9] {
                rig.front_panel(PanelChange::Signal(level));
                let seen = exchange(&mut *engine, &mut rig, Op::GetSignal);
                assert_eq!(
                    seen,
                    vec![Inbound::Observation(Observation::Signal(level))],
                    "{id}: signal {level}"
                );
            }
        }
        if desc.capabilities.has(Capability::ReadPtt) {
            for on in [true, false] {
                rig.front_panel(PanelChange::Ptt(on));
                let seen = exchange(&mut *engine, &mut rig, Op::GetPtt);
                assert_eq!(
                    seen,
                    vec![Inbound::Observation(Observation::Ptt(on))],
                    "{id}: ptt"
                );
            }
            let seen = exchange(&mut *engine, &mut rig, Op::SetPtt(true));
            assert_eq!(seen, vec![Inbound::Ack], "{id}: key");
            assert!(rig.state().ptt, "{id}: the twin is keyed");
            let seen = exchange(&mut *engine, &mut rig, Op::SetPtt(false));
            assert_eq!(seen, vec![Inbound::Ack], "{id}: unkey");
            assert!(!rig.state().ptt, "{id}: the twin is unkeyed");
        }
        let mut forbidden = vec![Op::SendCw("cq".into())];
        if !TRANSCEIVERS.contains(&id.as_str()) {
            forbidden.push(Op::SetPtt(true));
        }
        for op in forbidden {
            assert!(
                engine.encode(&op).is_err(),
                "{id}: no transmit path for {op:?}"
            );
        }
    }
}

proptest::proptest! {
    // Tests [@ANCHOR: radioctl_receiver_engine_feed]
    // Tests [@ANCHOR: radioctl_receiver_virtual_feed]
    #[test]
    fn no_receiver_engine_or_twin_panics_on_any_bytes(
        bytes in proptest::collection::vec(proptest::prelude::any::<u8>(), 0..400),
        cut in 0usize..400,
    ) {
        for desc in receivers() {
            let mut engine = engine_for(&desc).expect("engine");
            let _ = engine.encode(&Op::GetFrequency(Vfo::A));
            let cut = cut.min(bytes.len());
            let _ = engine.feed(&bytes[..cut], NOW);
            let _ = engine.feed(&bytes[cut..], NOW + Duration::from_secs(1));
            let mut rig = VirtualRig::from_description(&desc).expect("twin");
            let _ = rig.feed(&bytes, NOW);
        }
    }
}
