// SPDX-License-Identifier: AGPL-3.0-or-later
//! Chapter 7's per-family "Covers" lists are checked against the engines. Each list is followed by a
//! marker `<!--ops rig=<id>: Token Token ...-->`. The test builds the engine of that rig's description,
//! gives it every capability (so the description's capability list cannot hide an operation), tries one
//! or more sample values of every `Op` (levels one by one) and compares the operations the engine
//! encodes with the marker. An engine that gains an operation, or a manual that claims one the engine
//! refuses, fails here: update the marker and the Covers text above it, do not weaken the test.
//!
//! A token written `?Name` is covered but needs engine state a fresh engine does not have (a filter width
//! needs the mode first): it is left out of the comparison in both directions.
//!
//! A token is an `Op` variant name, or `GetLevel.<Level>` / `SetLevel.<Level>`. An operation counts as
//! covered when the engine encodes at least one of its sample values; a value the rig's tables do not
//! offer does not count against it.
#![allow(clippy::expect_used, clippy::panic)]

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use radioctl_core::capability::{Capability, CapabilitySet};
use radioctl_core::op::Op;
use radioctl_core::rigfile;
use radioctl_core::scope::{ScopeAction, ScopeAxis, ScopeSetting};
use radioctl_core::units::{Hz, Level, Meter, Mode, Receiver, Vfo};
use radioctl_protocols::conformance::engine_for;

fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

const LEVELS: [(&str, Level); 11] = [
    ("AfGain", Level::AfGain),
    ("RfGain", Level::RfGain),
    ("MicGain", Level::MicGain),
    ("MonitorLevel", Level::MonitorLevel),
    ("KeyerSpeed", Level::KeyerSpeed),
    ("RfPower", Level::RfPower),
    ("Attenuator", Level::Attenuator),
    ("Preamp", Level::Preamp),
    ("Dither", Level::Dither),
    ("Preselector", Level::Preselector),
    ("Antenna", Level::Antenna),
];

/// Every operation token with the sample values to try.
fn samples() -> Vec<(String, Vec<Op>)> {
    let mut all: Vec<(String, Vec<Op>)> = Vec::new();
    let mut add = |name: &str, ops: Vec<Op>| all.push((name.to_string(), ops));
    add("GetFrequency", vec![Op::GetFrequency(Vfo::A)]);
    add("GetFrequency.B", vec![Op::GetFrequency(Vfo::B)]);
    add(
        "SetFrequency",
        vec![Op::SetFrequency(Vfo::A, Hz(14_074_000))],
    );
    add(
        "SetFrequency.B",
        vec![Op::SetFrequency(Vfo::B, Hz(14_074_000))],
    );
    add("GetMode", vec![Op::GetMode]);
    add("SetMode", vec![Op::SetMode(Mode::Usb)]);
    add("GetPtt", vec![Op::GetPtt]);
    add("SetPtt", vec![Op::SetPtt(true), Op::SetPtt(false)]);
    add("GetSignal", vec![Op::GetSignal]);
    add("GetPower", vec![Op::GetPower]);
    add("SendCw", vec![Op::SendCw("CQ".into())]);
    add("StopCw", vec![Op::StopCw]);
    add("SetTransceive", vec![Op::SetTransceive(true)]);
    add("GetSplit", vec![Op::GetSplit]);
    add("GetTxVfo", vec![Op::GetTxVfo]);
    add("SetSplit", vec![Op::SetSplit(true)]);
    add("GetRit", vec![Op::GetRit]);
    add("SetRit", vec![Op::SetRit(true)]);
    add("GetXit", vec![Op::GetXit]);
    add("SetXit", vec![Op::SetXit(true)]);
    add("GetRitOffset", vec![Op::GetRitOffset]);
    add("SetRitOffset", vec![Op::SetRitOffset(50)]);
    add("GetFilterWidth", vec![Op::GetFilterWidth]);
    add(
        "SetFilterWidth",
        vec![
            Op::SetFilterWidth(Hz(2400)),
            Op::SetFilterWidth(Hz(500)),
            Op::SetFilterWidth(Hz(1800)),
        ],
    );
    for (name, level) in LEVELS {
        add(&format!("GetLevel.{name}"), vec![Op::GetLevel(level)]);
        add(
            &format!("SetLevel.{name}"),
            [0, 1, 6, 10, 12, 20, 30, 50, 100, 190]
                .into_iter()
                .map(|v| Op::SetLevel(level, v))
                .collect(),
        );
    }
    add("GetSwr", vec![Op::GetSwr]);
    add("GetMeter", vec![Op::GetMeter(Meter::Swr)]);
    add("GetModelId", vec![Op::GetModelId]);
    add("GetFirmware", vec![Op::GetFirmware]);
    add("GetPowerAmplifier", vec![Op::GetPowerAmplifier]);
    add("SetScopeOutput", vec![Op::SetScopeOutput(true)]);
    add("GetScopeOutput", vec![Op::GetScopeOutput]);
    add("SetScopeAxis", vec![Op::SetScopeAxis(0, ScopeAxis::Center)]);
    add("GetScopeAxis", vec![Op::GetScopeAxis(0)]);
    add("SetScopeSpan", vec![Op::SetScopeSpan(0, Hz(50_000))]);
    add("GetScopeSpan", vec![Op::GetScopeSpan(0)]);
    add(
        "SetScopeEdges",
        vec![Op::SetScopeEdges(1, 1, Hz(14_000_000), Hz(14_100_000))],
    );
    add("SetScopeReference", vec![Op::SetScopeReference(0, 0)]);
    add("GetScopeReference", vec![Op::GetScopeReference(0)]);
    add(
        "SetScopeSetting",
        vec![Op::SetScopeSetting(0, ScopeSetting::Display, 0)],
    );
    add(
        "GetScopeSetting",
        vec![Op::GetScopeSetting(0, ScopeSetting::Display)],
    );
    add(
        "ScopeAction",
        vec![Op::ScopeAction(0, ScopeAction::ClearWaterfall)],
    );
    add("SelectReceiver", vec![Op::SelectReceiver(Receiver::Sub)]);
    add("GetReceiver", vec![Op::GetReceiver]);
    add("SetTxReceiver", vec![Op::SetTxReceiver(Receiver::Sub)]);
    add("GetTxReceiver", vec![Op::GetTxReceiver]);
    add("SetDualWatch", vec![Op::SetDualWatch(true)]);
    add("GetDualWatch", vec![Op::GetDualWatch]);
    all
}

/// The `Op` variant names in `op.rs`, so a new variant cannot be left out of `samples`.
fn op_variants() -> BTreeSet<String> {
    let src =
        fs::read_to_string(workspace().join("crates/radioctl-core/src/op.rs")).expect("op.rs");
    let body = src.split("pub enum Op {").nth(1).expect("enum Op");
    let body = body.split("\n}").next().expect("end of enum Op");
    body.lines()
        .filter(|l| l.starts_with("    ") && !l.starts_with("     "))
        .filter(|l| l.trim_start().starts_with(|c: char| c.is_ascii_uppercase()))
        .map(|l| {
            l.trim_start()
                .chars()
                .take_while(|c| c.is_alphanumeric())
                .collect()
        })
        .collect()
}

/// Every capability, read from the file-format names in `capability.rs`.
fn every_capability() -> Vec<Capability> {
    let src = fs::read_to_string(workspace().join("crates/radioctl-core/src/capability.rs"))
        .expect("capability.rs");
    let table = src.split("const NAMES").nth(1).expect("NAMES");
    let table = table.split("];").next().expect("end of NAMES");
    let list: Vec<Capability> = table
        .split('"')
        .skip(1)
        .step_by(2)
        .filter_map(Capability::from_name)
        .collect();
    assert!(list.len() >= 20, "capability names not read: {list:?}");
    list
}

/// The tokens the engine of `rig` encodes.
fn encoded_by(rig: &str) -> BTreeSet<String> {
    let path = workspace().join("rigs").join(format!("{rig}.toml"));
    let text = fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let mut description = rigfile::parse(&text).expect("rig file parses").description;
    let every = every_capability();
    description.capabilities = CapabilitySet::of(&every);
    let mut found = BTreeSet::new();
    for (token, ops) in samples() {
        let mut engine = engine_for(&description).expect("engine");
        if ops.iter().any(|op| engine.encode(op).is_ok()) {
            found.insert(token);
        }
    }
    found
}

/// `(rig, tokens, chapter line)` of every marker in chapter 7.
fn markers() -> Vec<(String, BTreeSet<String>)> {
    let text = fs::read_to_string(workspace().join("docs/manual/07-protocol-engines.md"))
        .expect("chapter 7");
    let mut found = Vec::new();
    let mut rest = text.as_str();
    while let Some(start) = rest.find("<!--ops rig=") {
        let after = &rest[start + "<!--ops rig=".len()..];
        let end = after.find("-->").expect("marker closes");
        let (rig, tokens) = after[..end].split_once(':').expect("marker has a colon");
        found.push((
            rig.trim().to_string(),
            tokens.split_whitespace().map(str::to_string).collect(),
        ));
        rest = &after[end..];
    }
    found
}

#[test]
fn every_op_variant_has_a_sample() {
    let named: BTreeSet<String> = samples()
        .into_iter()
        .map(|(t, _)| t.split('.').next().expect("token").to_string())
        .collect();
    let missing: Vec<String> = op_variants().difference(&named).cloned().collect();
    assert!(
        missing.is_empty(),
        "add a sample in manual_covers.rs for: {missing:?}"
    );
}

#[test]
fn the_covers_lists_of_chapter_seven_match_the_engines() {
    let markers = markers();
    assert!(markers.len() >= 8, "one marker per engine section expected");
    let mut problems = Vec::new();
    for (rig, claimed) in markers {
        let skipped: BTreeSet<String> = claimed
            .iter()
            .filter_map(|t| t.strip_prefix('?').map(str::to_string))
            .collect();
        let claimed: BTreeSet<String> = claimed
            .into_iter()
            .filter(|t| !t.starts_with('?'))
            .collect();
        let actual: BTreeSet<String> = encoded_by(&rig).difference(&skipped).cloned().collect();
        let unbacked: Vec<_> = claimed.difference(&actual).collect();
        let unlisted: Vec<_> = actual.difference(&claimed).collect();
        if !unbacked.is_empty() || !unlisted.is_empty() {
            problems.push(format!(
                "{rig}: the manual claims but the engine refuses {unbacked:?}; the engine encodes but the manual does not list {unlisted:?}"
            ));
        }
    }
    assert!(
        problems.is_empty(),
        "chapter 7 drifted:\n{}",
        problems.join("\n")
    );
}
