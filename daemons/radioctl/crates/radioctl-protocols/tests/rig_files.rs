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
fn the_worked_ic7300_file_loads_and_equals_the_builtin_description() {
    let loaded = load_all();
    assert!(!loaded.is_empty());
    let ic7300 = loaded
        .iter()
        .find(|l| l.description.id == "icom-ic-7300")
        .expect("worked description");
    assert_eq!(ic7300.description, RigDescription::ic7300());
}

// Tests [@ANCHOR: radioctl_core_rigfile_verification_summary]
// Tests [@ANCHOR: radioctl_core_rigfile_is_hardware_verified]
#[test]
fn the_worked_file_claims_no_verification_it_does_not_have() {
    for loaded in load_all() {
        let summary = loaded.verification_summary();
        assert_eq!(summary.len(), 1, "{summary:?}");
        assert!(
            summary.contains_key("unverified"),
            "{} claims verification: {summary:?}",
            loaded.description.id
        );
        assert!(!loaded.is_hardware_verified());
    }
}

// Tests [@ANCHOR: radioctl_conformance_replay]
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
    let (path, _) = transcripts().into_iter().next().expect("a transcript");
    let text = fs::read_to_string(path).expect("read");
    let corrupted = text.replacen("fe fe e0 94 04 01 01 fd", "fe fe e0 94 04 02 01 fd", 1);
    assert_ne!(text, corrupted);
    let transcript = transcript::parse(&corrupted).expect("still parses");
    let description = RigDescription::ic7300();
    let problems = conformance::replay(&description, &transcript).expect_err("must be caught");
    assert!(
        problems.iter().any(|p| p.contains("virtual rig answered")),
        "{problems:?}"
    );
}
