// SPDX-License-Identifier: AGPL-3.0-or-later
//! Checks one rig description the way a contributor would: load it, list what is verified and
//! what is not, and replay its transcript against the engine and the virtual rig built from it.
//!
//! Usage: `rig_file [RIG_ID]` (default `elecraft-kx3`), run from anywhere inside the workspace.
use std::collections::BTreeMap;
use std::error::Error;
use std::path::{Path, PathBuf};

use radioctl_core::rigfile::{self, Verified};
use radioctl_core::transcript;
use radioctl_protocols::conformance;

/// The directory of rig descriptions: `$RADIOCTL_RIGS`, else `./rigs` when it exists (the binaries
/// copied to a bench machine run from the directory that holds `rigs/`), else the checkout this
/// program was built from.
fn rigs_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("RADIOCTL_RIGS") {
        return PathBuf::from(dir);
    }
    let here = PathBuf::from("rigs");
    if here.is_dir() {
        return here;
    }
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../rigs")
}

fn main() -> Result<(), Box<dyn Error>> {
    let id = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "elecraft-kx3".to_string());
    let rigs = rigs_dir();
    let loaded = rigfile::parse(&std::fs::read_to_string(rigs.join(format!("{id}.toml")))?)?;
    let description = &loaded.description;
    println!("{}: family {:?}", description.name, description.family);

    let mut by_state: BTreeMap<&str, usize> = BTreeMap::new();
    for fact in &loaded.facts {
        let state = match fact.verified {
            Verified::Unverified => "unverified",
            Verified::ManualChecked { .. } => "manual-checked",
            Verified::TranscriptChecked { .. } => "transcript-checked",
            Verified::HardwareVerified { .. } => "hardware-verified",
        };
        *by_state.entry(state).or_default() += 1;
    }
    println!("facts by verified state: {by_state:?}");

    let path = rigs.join("transcripts").join(format!("{id}-basic.toml"));
    let transcript = transcript::parse(&std::fs::read_to_string(path)?)?;
    println!("transcript origin: {}", transcript.origin);
    match conformance::replay(description, &transcript) {
        Ok(steps) => println!("replayed {steps} steps: engine, virtual rig and file agree"),
        Err(problems) => return Err(problems.join("\n").into()),
    }
    Ok(())
}
