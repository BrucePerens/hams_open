// SPDX-License-Identifier: AGPL-3.0-or-later
//! The shipped rig description files and their transcripts, checked as production would load them.
#![allow(clippy::expect_used, clippy::panic)]

use std::fs;
use std::path::{Path, PathBuf};

use radioctl_core::description::RigDescription;
use radioctl_core::rigfile::{self, LoadedDescription};
use radioctl_core::transcript::{self, Transcript};
use radioctl_protocols::conformance;

fn rigs_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../rigs")
}

fn toml_files(dir: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = fs::read_dir(dir)
        .expect("rigs dir")
        .map(|e| e.expect("entry").path())
        .filter(|p| p.extension().is_some_and(|x| x == "toml"))
        .collect();
    files.sort();
    files
}

fn load_all() -> Vec<LoadedDescription> {
    toml_files(&rigs_dir())
        .iter()
        .map(|p| {
            rigfile::parse(&fs::read_to_string(p).expect("read"))
                .unwrap_or_else(|e| panic!("{}: {e}", p.display()))
        })
        .collect()
}

fn transcripts() -> Vec<(PathBuf, Transcript)> {
    toml_files(&rigs_dir().join("transcripts"))
        .into_iter()
        .map(|p| {
            let parsed = transcript::parse(&fs::read_to_string(&p).expect("read"))
                .unwrap_or_else(|e| panic!("{}: {e}", p.display()));
            (p, parsed)
        })
        .collect()
}

// Tests [@ANCHOR: radioctl_core_rigfile_parse]
#[test]
fn the_ic7300_file_loads_and_extends_the_builtin_description() {
    let loaded = load_all();
    assert!(!loaded.is_empty());
    let ic7300 = loaded
        .iter()
        .find(|l| l.description.id == "icom-ic-7300")
        .expect("worked description");
    let builtin = RigDescription::ic7300();
    assert_eq!(ic7300.description.civ, builtin.civ);
    assert_eq!(ic7300.description.timing, builtin.timing);
    for capability in builtin.capabilities.iter() {
        assert!(ic7300.description.capabilities.has(capability));
    }
    for (mode, code) in &builtin.mode_codes {
        let in_file = ic7300.description.mode_code(*mode);
        assert!(in_file == Some(*code) || *mode == radioctl_core::units::Mode::Wfm);
    }
    assert!(ic7300.description.scope.is_some());
}

// Tests [@ANCHOR: radioctl_core_rigfile_verification_summary]
// Tests [@ANCHOR: radioctl_core_rigfile_is_hardware_verified]
#[test]
fn the_worked_file_claims_no_verification_it_does_not_have() {
    // The only files that may hold `manual-checked` facts: those whose vendor manual was read
    // (see the facts' references, each with the manual's SHA-256). Nothing else is verified in
    // any way, and nothing is hardware-verified or transcript-checked.
    const MANUAL_READ: [&str; 3] = ["kenwood-ts-480", "kenwood-ts-870s", "yaesu-ft-450"];
    for loaded in load_all() {
        let summary = loaded.verification_summary();
        let id = loaded.description.id.as_str();
        assert!(
            summary.contains_key("unverified"),
            "{id} claims verification: {summary:?}"
        );
        let allowed: &[&str] = if MANUAL_READ.contains(&id) {
            &["unverified", "manual-checked"]
        } else {
            &["unverified"]
        };
        assert!(
            summary.keys().all(|k| allowed.contains(k)),
            "{id} claims verification it does not have: {summary:?}"
        );
        assert!(!loaded.is_hardware_verified());
    }
}

// Tests [@ANCHOR: radioctl_conformance_replay]
// Tests [@ANCHOR: radioctl_conformance_stand_in_for]
// Tests [@ANCHOR: radioctl_conformance_stand_in_prologue]
// Tests [@ANCHOR: radioctl_conformance_stand_in_feed]
// Tests [@ANCHOR: radioctl_conformance_stand_in_front_panel]
// Tests [@ANCHOR: radioctl_conformance_bridge_stand_in_feed]
// Tests [@ANCHOR: radioctl_core_transcript_parse]
#[test]
fn every_shipped_transcript_replays_against_its_description() {
    let descriptions = load_all();
    let all = transcripts();
    assert!(!all.is_empty());
    for (path, transcript) in all {
        let loaded = descriptions
            .iter()
            .find(|l| l.description.id == transcript.rig)
            .unwrap_or_else(|| {
                panic!("{} names an unknown rig {}", path.display(), transcript.rig)
            });
        let steps = conformance::replay(&loaded.description, &transcript)
            .unwrap_or_else(|p| panic!("{}: {p:#?}", path.display()));
        assert_eq!(steps, transcript.steps.len());
        assert!(steps >= 10, "{} is too thin to mean much", path.display());
    }
}

// Tests [@ANCHOR: radioctl_conformance_replay]
#[test]
fn a_corrupted_byte_in_the_shipped_transcript_is_caught() {
    let (path, _) = transcripts()
        .into_iter()
        .find(|(_, t)| t.rig == "icom-ic-7300")
        .expect("the IC-7300 transcript");
    let text = fs::read_to_string(path).expect("read");
    let corrupted = text.replacen(
        "fe fe e0 94 26 00 01 00 01 fd",
        "fe fe e0 94 26 00 02 00 01 fd",
        1,
    );
    assert_ne!(text, corrupted);
    let transcript = transcript::parse(&corrupted).expect("still parses");
    let description = load_all()
        .into_iter()
        .map(|l| l.description)
        .find(|d| d.id == "icom-ic-7300")
        .expect("the IC-7300 file");
    let problems = conformance::replay(&description, &transcript).expect_err("must be caught");
    assert!(
        problems.iter().any(|p| p.contains("virtual rig answered")),
        "{problems:?}"
    );
}

// Tests [@ANCHOR: radioctl_conformance_replay]
// Tests [@ANCHOR: radioctl_elecraft_engine_feed]
#[test]
fn a_corrupted_byte_in_the_kx3_transcript_is_caught_and_the_kx3_file_is_unverified() {
    let (path, _) = transcripts()
        .into_iter()
        .find(|(_, t)| t.rig == "elecraft-kx3")
        .expect("the KX3 transcript");
    let text = fs::read_to_string(path).expect("read");
    let corrupted = text.replacen("FA00014060000;", "FA00014060001;", 1);
    assert_ne!(text, corrupted);
    let transcript = transcript::parse(&corrupted).expect("still parses");
    let loaded = load_all()
        .into_iter()
        .find(|l| l.description.id == "elecraft-kx3")
        .expect("kx3 description");
    let problems = conformance::replay(&loaded.description, &transcript).expect_err("caught");
    assert!(
        problems.iter().any(|p| p.contains("virtual rig answered")),
        "{problems:?}"
    );
    assert!(loaded.verification_summary().contains_key("unverified"));
}
