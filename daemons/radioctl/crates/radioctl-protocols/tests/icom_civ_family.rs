// SPDX-License-Identifier: AGPL-3.0-or-later
//! Every Icom CI-V description in `rigs/` through the CI-V engine and its virtual rig, table
//! driven: whatever a description claims (frequency, mode, PTT read, second VFO, split, RIT
//! offset, level, model id) must round-trip, and what it does not claim must be refused by the
//! engine. No radio is involved and nothing is keyed. The oracle comparison against Hamlib is in
//! `rigctld_oracle.rs` (cargo feature `hamlib-oracle`).
#![allow(clippy::expect_used, clippy::panic)]

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::time::Duration;

use radioctl_core::capability::Capability;
use radioctl_core::description::{Family, RigDescription};
use radioctl_core::engine::{Inbound, ProtocolEngine, ReplyKind};
use radioctl_core::error::ErrorKind;
use radioctl_core::op::{Observation, Op};
use radioctl_core::rigfile;
use radioctl_core::units::{Hz, Level, Vfo};
use radioctl_protocols::conformance::engine_for;
use radioctl_protocols::virtual_rig::VirtualRig;

const NOW: Duration = Duration::from_secs(10);

/// Description id and the CI-V address recorded from Hamlib 4.6.2 (`rigs/icom/<model>.c`, the
/// `default address` field of the model's private caps) and read in Icom's instruction manual
/// where a copy could be had (every one but the IC-7000, IC-7760, IC-706 and IC-706MkII). A
/// second, independent copy of the number in each file: a typo in either shows up here.
const ADDRESSES: [(&str, u8); 19] = [
    ("icom-ic-7000", 0x70),
    ("icom-ic-7100", 0x88),
    ("icom-ic-7200", 0x76),
    ("icom-ic-7410", 0x80),
    ("icom-ic-7600", 0x7A),
    ("icom-ic-7700", 0x74),
    ("icom-ic-7760", 0xB2),
    ("icom-ic-7800", 0x6A),
    ("icom-ic-7851", 0x8E),
    ("icom-ic-9100", 0x7C),
    ("icom-ic-r8600", 0x96),
    ("icom-ic-r75", 0x5A),
    ("icom-ic-r9500", 0x72),
    ("icom-ic-706", 0x48),
    ("icom-ic-706mkii", 0x4E),
    ("icom-ic-706mkiig", 0x58),
    ("icom-ic-718", 0x5E),
    ("icom-ic-746", 0x56),
    ("icom-ic-746pro", 0x66),
];

fn icom_files() -> Vec<RigDescription> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../rigs");
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir).expect("rigs directory") {
        let path = entry.expect("entry").path();
        if path.extension().is_some_and(|e| e == "toml") {
            let text = std::fs::read_to_string(&path).expect("read");
            let desc = rigfile::parse(&text).expect("loads").description;
            if desc.family == Family::IcomCiv {
                out.push(desc);
            }
        }
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    out
}

/// One op through the engine and the virtual rig; what the engine decoded.
fn exchange(engine: &mut dyn ProtocolEngine, rig: &mut VirtualRig, op: &Op) -> Vec<Inbound> {
    let request = engine.encode(op).expect("encode");
    let answer = rig.feed(&request.bytes, NOW);
    match request.reply {
        ReplyKind::None => assert!(answer.is_empty(), "unexpected answer to {op:?}"),
        _ => assert!(!answer.is_empty(), "no answer to {op:?}"),
    }
    engine.feed(&answer, NOW)
}

fn read(engine: &mut dyn ProtocolEngine, rig: &mut VirtualRig, op: &Op) -> Observation {
    match exchange(engine, rig, op).as_slice() {
        [Inbound::Observation(observation)] => *observation,
        other => panic!("{op:?}: expected one observation, got {other:?}"),
    }
}

fn write(engine: &mut dyn ProtocolEngine, rig: &mut VirtualRig, op: &Op) {
    assert_eq!(exchange(engine, rig, op), vec![Inbound::Ack], "{op:?}");
}

// Tests [@ANCHOR: radioctl_civ_engine_encode]
// Tests [@ANCHOR: radioctl_civ_engine_decode]
// Tests [@ANCHOR: radioctl_civ_engine_feed]
// Tests [@ANCHOR: radioctl_virtual_civ_dispatch]
// Tests [@ANCHOR: radioctl_virtual_from_description]
#[test]
fn every_icom_file_round_trips_what_it_claims_and_refuses_the_rest() {
    let files = icom_files();
    assert!(files.len() >= 23, "{} Icom/Xiegu files", files.len());
    for desc in files {
        let id = desc.id.clone();
        let mut engine = engine_for(&desc).expect("engine");
        let mut rig = VirtualRig::from_description(&desc).expect("virtual rig");
        let has = |c: Capability| desc.capabilities.has(c);

        let start = desc.default_frequency;
        assert_eq!(
            read(engine.as_mut(), &mut rig, &Op::GetFrequency(Vfo::A)),
            Observation::Frequency(Vfo::A, start),
            "{id}"
        );
        write(
            engine.as_mut(),
            &mut rig,
            &Op::SetFrequency(Vfo::A, Hz(7_074_000)),
        );
        assert_eq!(
            read(engine.as_mut(), &mut rig, &Op::GetFrequency(Vfo::A)),
            Observation::Frequency(Vfo::A, Hz(7_074_000)),
            "{id}"
        );

        // Every mode the file lists, in the order listed (data modes follow their sidebands).
        for (mode, _) in &desc.mode_codes {
            write(engine.as_mut(), &mut rig, &Op::SetMode(*mode));
            assert_eq!(
                read(engine.as_mut(), &mut rig, &Op::GetMode),
                Observation::Mode(*mode),
                "{id} {mode:?}"
            );
        }

        if has(Capability::ReadPtt) {
            assert_eq!(
                read(engine.as_mut(), &mut rig, &Op::GetPtt),
                Observation::Ptt(false),
                "{id}"
            );
        }
        if has(Capability::SecondVfo) {
            write(
                engine.as_mut(),
                &mut rig,
                &Op::SetFrequency(Vfo::B, Hz(14_025_000)),
            );
            assert_eq!(
                read(engine.as_mut(), &mut rig, &Op::GetFrequency(Vfo::B)),
                Observation::Frequency(Vfo::B, Hz(14_025_000)),
                "{id}"
            );
        }
        if has(Capability::Split) {
            write(engine.as_mut(), &mut rig, &Op::SetSplit(true));
            assert_eq!(
                read(engine.as_mut(), &mut rig, &Op::GetSplit),
                Observation::Split(true),
                "{id}"
            );
            write(engine.as_mut(), &mut rig, &Op::SetSplit(false));
        }
        if has(Capability::RitXit) {
            write(engine.as_mut(), &mut rig, &Op::SetRitOffset(-120));
            assert_eq!(
                read(engine.as_mut(), &mut rig, &Op::GetRitOffset),
                Observation::RitOffset(-120),
                "{id}"
            );
        }
        if has(Capability::Levels) {
            write(engine.as_mut(), &mut rig, &Op::SetLevel(Level::AfGain, 128));
            assert_eq!(
                read(engine.as_mut(), &mut rig, &Op::GetLevel(Level::AfGain)),
                Observation::Level(Level::AfGain, 128),
                "{id}"
            );
        }
        if has(Capability::Identify) {
            let civ = desc.civ.as_ref().expect("civ block");
            assert_eq!(
                read(engine.as_mut(), &mut rig, &Op::GetModelId),
                Observation::ModelId(u16::from(civ.model_id)),
                "{id}"
            );
        }

        // What the file does not claim, the engine refuses without sending anything.
        for (capability, op) in [
            (Capability::SetPtt, Op::SetPtt(true)),
            (Capability::SendCw, Op::SendCw("E".into())),
            (Capability::Split, Op::SetSplit(true)),
            (Capability::RitXit, Op::SetRitOffset(10)),
            (Capability::ReadPtt, Op::GetPtt),
        ] {
            if !has(capability) {
                let err = engine.encode(&op).expect_err("refused");
                assert_eq!(err.kind, ErrorKind::Unsupported, "{id} {op:?}");
            }
        }
    }
}

// Tests [@ANCHOR: radioctl_core_rigfile_parse]
#[test]
fn the_batch_addresses_match_hamlib_and_no_two_files_share_an_id() {
    let files = icom_files();
    let ids: BTreeSet<&str> = files.iter().map(|d| d.id.as_str()).collect();
    assert_eq!(ids.len(), files.len(), "duplicate ids");
    for (id, address) in ADDRESSES {
        let desc = files
            .iter()
            .find(|d| d.id == id)
            .unwrap_or_else(|| panic!("{id} has no file"));
        let civ = desc.civ.as_ref().expect("civ block");
        assert_eq!(civ.address, address, "{id}");
        assert_eq!(civ.controller, 0xE0, "{id}");
        assert!(!civ.echo, "{id}");
    }
}
