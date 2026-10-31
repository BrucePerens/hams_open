// SPDX-License-Identifier: AGPL-3.0-or-later
//! Every Kenwood, Elecraft and Yaesu newcat description in `rigs/` through its ASCII engine and
//! virtual rig, table driven (the counterpart of `icom_civ_family.rs`): whatever a description
//! claims (frequency, mode, PTT read, second VFO, split, RIT offset, level, model id) must
//! round-trip, and what it does not claim must be refused by the engine. No radio is involved and
//! nothing is keyed. The oracle comparison against Hamlib is in `rigctld_oracle.rs` (cargo
//! feature `hamlib-oracle`).
#![allow(clippy::expect_used, clippy::panic)]

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::time::Duration;

use radioctl_core::capability::Capability;
use radioctl_core::description::{Family, KenwoodDialect, RigDescription};
use radioctl_core::engine::{Inbound, ProtocolEngine, ReplyKind};
use radioctl_core::error::ErrorKind;
use radioctl_core::op::{Observation, Op};
use radioctl_core::rigfile;
use radioctl_core::units::{Hz, Level, Vfo};
use radioctl_protocols::conformance::engine_for;
use radioctl_protocols::virtual_rig::VirtualRig;

const NOW: Duration = Duration::from_secs(10);

/// Description id, the number the model answers to `ID;` and the digits of its frequency fields
/// (Yaesu only; 0 elsewhere). A second, independent copy of the numbers in each file: a typo in
/// either shows up here. The sources are named in each file's facts.
const BATCH: [(&str, u16, u8); 31] = [
    ("yaesu-ft-450", 241, 8),
    ("yaesu-ft-450d", 244, 8),
    ("yaesu-ft-950", 310, 8),
    ("yaesu-ft-2000", 251, 8),
    ("yaesu-ftdx-1200", 583, 8),
    ("yaesu-ftdx-3000", 462, 8),
    ("yaesu-ftdx-5000", 362, 8),
    ("yaesu-ftdx-9000", 101, 8),
    ("yaesu-ft-891", 650, 9),
    ("yaesu-ft-710", 800, 9),
    ("yaesu-ftdx-101mp", 682, 9),
    ("elecraft-k3", 17, 0),
    ("elecraft-k3s", 17, 0),
    ("elecraft-kx2", 17, 0),
    ("elecraft-k4", 17, 0),
    ("kenwood-ts-50s", 13, 0),
    ("kenwood-ts-140s", 6, 0),
    ("kenwood-ts-450s", 10, 0),
    ("kenwood-ts-480", 20, 0),
    ("kenwood-ts-570d", 17, 0),
    ("kenwood-ts-570s", 18, 0),
    ("kenwood-ts-680s", 6, 0),
    ("kenwood-ts-690s", 11, 0),
    ("kenwood-ts-711", 3, 0),
    ("kenwood-ts-790", 7, 0),
    ("kenwood-ts-811", 2, 0),
    ("kenwood-ts-850", 9, 0),
    ("kenwood-ts-870s", 15, 0),
    ("kenwood-ts-940s", 1, 0),
    ("kenwood-ts-950s", 8, 0),
    ("kenwood-ts-950sdx", 12, 0),
];

/// The ASCII descriptions in `rigs/` that this test drives (the TS-890S dialect has its own
/// engine and tests).
fn ascii_files() -> Vec<RigDescription> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../rigs");
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir).expect("rigs directory") {
        let path = entry.expect("entry").path();
        if path.extension().is_some_and(|e| e == "toml") {
            let text = std::fs::read_to_string(&path).expect("read");
            let desc = rigfile::parse(&text).expect("loads").description;
            let classic = desc
                .kenwood
                .as_ref()
                .is_none_or(|k| k.dialect == KenwoodDialect::Classic);
            if matches!(
                desc.family,
                Family::KenwoodAscii | Family::ElecraftAscii | Family::YaesuNewcat
            ) && classic
            {
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

/// Everything an answer decoded to: an `IF;` answer carries frequency, mode, PTT and more at
/// once, so a read is equal to an observation when the answer contains it.
#[derive(Debug)]
struct Seen(Vec<Observation>);

impl PartialEq<Observation> for Seen {
    fn eq(&self, other: &Observation) -> bool {
        self.0.contains(other)
    }
}

fn read(engine: &mut dyn ProtocolEngine, rig: &mut VirtualRig, op: &Op) -> Seen {
    let decoded = exchange(engine, rig, op);
    let seen: Vec<Observation> = decoded
        .iter()
        .filter_map(|i| match i {
            Inbound::Observation(o) => Some(*o),
            _ => None,
        })
        .collect();
    assert_eq!(seen.len(), decoded.len(), "{op:?}: {decoded:?}");
    Seen(seen)
}

/// A set command: the rig stays silent (a refusal would be `?;`, which `exchange` rejects), so
/// the engine decodes nothing or reports the write as acknowledged.
fn write(engine: &mut dyn ProtocolEngine, rig: &mut VirtualRig, op: &Op) {
    let decoded = exchange(engine, rig, op);
    assert!(
        decoded.is_empty() || decoded == vec![Inbound::Ack],
        "{op:?}: {decoded:?}"
    );
}

/// The number the description says the model answers to `ID;`.
fn model_id(desc: &RigDescription) -> u16 {
    desc.kenwood
        .as_ref()
        .map(|k| k.model_id)
        .or(desc.elecraft.as_ref().map(|e| e.model_id))
        .or(desc.yaesu.as_ref().map(|y| y.model_id))
        .expect("an ASCII family block")
}

// Tests [@ANCHOR: radioctl_kenwood_engine_encode]
// Tests [@ANCHOR: radioctl_kenwood_engine_feed]
// Tests [@ANCHOR: radioctl_elecraft_engine_encode]
// Tests [@ANCHOR: radioctl_elecraft_engine_feed]
// Tests [@ANCHOR: radioctl_yaesu_engine_encode]
// Tests [@ANCHOR: radioctl_yaesu_engine_feed]
// Tests [@ANCHOR: radioctl_virtual_from_description]
#[test]
fn every_ascii_file_round_trips_what_it_claims_and_refuses_the_rest() {
    let files = ascii_files();
    assert!(files.len() >= 31 + 7, "{} ASCII files", files.len());
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

        // Every mode the file lists, in the order listed.
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
        if has(Capability::Transceive) {
            write(engine.as_mut(), &mut rig, &Op::SetTransceive(true));
            write(engine.as_mut(), &mut rig, &Op::SetTransceive(false));
        }
        if has(Capability::Identify) {
            assert_eq!(
                read(engine.as_mut(), &mut rig, &Op::GetModelId),
                Observation::ModelId(model_id(&desc)),
                "{id}"
            );
        }

        // What the file does not claim, the engine refuses without sending anything.
        for (capability, op) in [
            (Capability::SendCw, Op::SendCw("E".into())),
            (Capability::Split, Op::SetSplit(true)),
            (Capability::RitXit, Op::SetRitOffset(10)),
            (Capability::ReadPtt, Op::GetPtt),
            (Capability::Levels, Op::SetLevel(Level::AfGain, 128)),
            (Capability::Transceive, Op::SetTransceive(true)),
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
fn the_batch_ids_and_frequency_widths_match_the_sources_and_no_two_files_share_an_id() {
    let files = ascii_files();
    let ids: BTreeSet<&str> = files.iter().map(|d| d.id.as_str()).collect();
    assert_eq!(ids.len(), files.len(), "duplicate ids");
    for (id, number, digits) in BATCH {
        let desc = files
            .iter()
            .find(|d| d.id == id)
            .unwrap_or_else(|| panic!("{id} has no file"));
        assert_eq!(model_id(desc), number, "{id}");
        if let Some(yaesu) = &desc.yaesu {
            assert_eq!(yaesu.frequency_digits, digits, "{id}");
        }
        // The TS-680S has no identify: Hamlib's number for it is a guess (see its file).
        assert!(
            desc.capabilities.has(Capability::Identify) || id == "kenwood-ts-680s",
            "{id}"
        );
    }
}

/// A description by id.
fn described(id: &str) -> RigDescription {
    ascii_files()
        .into_iter()
        .find(|d| d.id == id)
        .unwrap_or_else(|| panic!("{id} has no file"))
}

// Tests [@ANCHOR: radioctl_kenwood_engine_encode]
// Tests [@ANCHOR: radioctl_kenwood_level_wire]
// Tests [@ANCHOR: radioctl_kenwood_decode_level]
#[test]
fn the_kenwood_manual_checks_are_pinned_where_the_guides_and_the_engine_differ() {
    // TS-480 (PC control command reference, 2003/11/28): AG0nnn 000-255 and MG 000-100 and
    // KS 010-060 agree with the engine; RG is 000-100 in the guide.
    let desc = described("kenwood-ts-480");
    let mut engine = engine_for(&desc).expect("engine");
    let wire = |engine: &mut dyn ProtocolEngine, op: Op| engine.encode(&op).expect("encodes").bytes;
    assert_eq!(
        wire(engine.as_mut(), Op::SetLevel(Level::AfGain, 255)),
        b"AG0255;"
    );
    assert_eq!(
        wire(engine.as_mut(), Op::SetLevel(Level::MicGain, 100)),
        b"MG100;"
    );
    assert_eq!(
        wire(engine.as_mut(), Op::SetLevel(Level::KeyerSpeed, 60)),
        b"KS060;"
    );
    assert!(engine.encode(&Op::SetLevel(Level::MicGain, 101)).is_err());
    // Open question, pinned: the engine still writes RG up to 255 to a radio whose guide stops
    // at 100. When the engine takes a per-description range this expectation flips to an error.
    assert_eq!(
        wire(engine.as_mut(), Op::SetLevel(Level::RfGain, 255)),
        b"RG255;"
    );

    // TS-870S (instruction manual appendix D): RU and RD take no parameter, AG and RG have no
    // leading zero, SM has no leading zero: the engine's forms would be refused, so the
    // description claims neither RIT/XIT offsets, levels nor the signal.
    let desc = described("kenwood-ts-870s");
    let mut engine = engine_for(&desc).expect("engine");
    for op in [
        Op::SetRitOffset(150),
        Op::SetLevel(Level::AfGain, 100),
        Op::GetSignal,
    ] {
        let err = engine.encode(&op).expect_err("refused");
        assert_eq!(err.kind, ErrorKind::Unsupported, "{op:?}");
    }
}

// Tests [@ANCHOR: radioctl_yaesu_engine_encode]
// Tests [@ANCHOR: radioctl_yaesu_engine_feed]
// Tests [@ANCHOR: radioctl_virtual_from_description]
#[test]
fn the_ft_450_reads_its_po_meter_with_rm5_as_its_cat_book_says() {
    let desc = described("yaesu-ft-450");
    assert!(desc.capabilities.has(Capability::ReadPower));
    let mut engine = engine_for(&desc).expect("engine");
    let request = engine.encode(&Op::GetPower).expect("encodes");
    assert_eq!(request.bytes, b"RM5;");
    let mut rig = VirtualRig::from_description(&desc).expect("virtual rig");
    let answer = rig.feed(&request.bytes, NOW);
    assert!(!answer.is_empty());
    assert!(engine
        .feed(&answer, NOW)
        .iter()
        .any(|i| matches!(i, Inbound::Observation(Observation::Power(_)))));
    // The book has no XT command and no MG/PC of the engine's ranges: not claimed.
    assert!(!desc.capabilities.has(Capability::RitXit));
    assert!(!desc.capabilities.has(Capability::Levels));
}
