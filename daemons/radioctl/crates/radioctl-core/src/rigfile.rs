// SPDX-License-Identifier: AGPL-3.0-or-later
//! The rig description file format (TOML, schema 1), with provenance and `verified` fields.
//!
//! Every datum of a description must be covered by a `[[facts]]` entry that says where it came
//! from (`provenance`) and what has been checked (`verified`). A description with an
//! unexplained datum does not load: nothing here is taken on trust. A fact key is exact
//! (`civ.address`) or a prefix wildcard (`capability.*`); an exact fact wins over a wildcard.
//!
//! Keys: `rig.id`, `rig.name`, `rig.family`, `timing.timeout_ms`, `timing.retries`,
//! `timing.inter_command_ms`, `defaults.frequency_hz`, `defaults.mode`, `civ.address`,
//! `civ.controller`, `civ.model_id`, `civ.echo`, `kenwood.model_id`, `elecraft.model_id`, `yaesu.model_id`,
//! `scope.receivers`, `scope.line_length`, `scope.samples_per_frame`, `scope.amplitude_max`,
//! `scope.spans_hz` (an optional `[scope]` block, icom-civ only), `capability.<name>`, `mode.<name>`.
//!
//! Verified states: `unverified`; `manual-checked` (needs `reference`); `transcript-checked`
//! (needs `reference`, the transcript); `hardware-verified` (needs `radio`, `firmware`, `date`
//! and `tested`: exactly what was tested on which radio and firmware).

use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

use serde::Deserialize;

use crate::capability::{Capability, CapabilitySet};
use crate::description::{
    CivParams, ElecraftParams, Family, KenwoodParams, RigDescription, ScopeParams, Timing,
    YaesuParams,
};
use crate::error::{ErrorKind, RigError};
use crate::units::{Hz, Mode};

/// The only schema version this reader accepts.
pub const SCHEMA: u32 = 1;

/// Where a datum came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProvenanceKind {
    /// A vendor manual; `reference` names the document and section.
    Manual,
    /// A recorded capture of a real radio; `reference` names the capture.
    Capture,
    /// Run by the owner on a radio; `reference` says who and when.
    Owner,
    /// Existing code in this repository; `reference` names the file and symbol.
    RepoCode,
    /// The output of a program run; `reference` names the command.
    ProgramOutput,
    /// A fact read from Hamlib's source (LGPL; facts only, no code or table copied): used only
    /// where no vendor manual could be used. `reference` names the Hamlib release, file and
    /// function; `note` says why a manual was not the source.
    HamlibSource,
}

/// A provenance record.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Provenance {
    /// What kind of source.
    pub kind: ProvenanceKind,
    /// The precise source.
    pub reference: String,
    /// Anything a reader should know about the source.
    #[serde(default)]
    pub note: String,
}

/// What has been checked about a datum.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verified {
    /// Nothing has been checked.
    Unverified,
    /// Compared with a vendor manual.
    ManualChecked {
        /// The document and section.
        reference: String,
    },
    /// Replayed against a recorded transcript.
    TranscriptChecked {
        /// The transcript.
        reference: String,
    },
    /// Tested on a real radio.
    HardwareVerified {
        /// The radio.
        radio: String,
        /// Its firmware version.
        firmware: String,
        /// The date, `YYYY-MM-DD`.
        date: String,
        /// Exactly what was tested.
        tested: String,
    },
}

/// One provenance and verification record for a key or a key prefix.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fact {
    /// An exact key, or a prefix ending in `.*`.
    pub key: String,
    /// Where the datum came from.
    pub provenance: Provenance,
    /// What has been checked.
    pub verified: Verified,
}

/// A parsed description together with the record of where its data came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadedDescription {
    /// The description.
    pub description: RigDescription,
    /// Provenance and verification records.
    pub facts: Vec<Fact>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FileDoc {
    schema: u32,
    rig: RigSection,
    civ: Option<CivSection>,
    kenwood: Option<KenwoodSection>,
    elecraft: Option<ElecraftSection>,
    yaesu: Option<YaesuSection>,
    scope: Option<ScopeSection>,
    timing: TimingSection,
    defaults: DefaultsSection,
    capabilities: Vec<String>,
    modes: Vec<ModeEntry>,
    facts: Vec<FactEntry>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RigSection {
    id: String,
    name: String,
    family: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CivSection {
    address: u8,
    controller: u8,
    model_id: u8,
    echo: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct KenwoodSection {
    model_id: u16,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ElecraftSection {
    model_id: u16,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct YaesuSection {
    model_id: u16,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ScopeSection {
    receivers: u8,
    line_length: u16,
    samples_per_frame: u16,
    amplitude_max: u16,
    spans_hz: Vec<u64>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TimingSection {
    timeout_ms: u64,
    retries: u8,
    inter_command_ms: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DefaultsSection {
    frequency_hz: u64,
    mode: String,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum CodeValue {
    Byte(u8),
    Char(String),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ModeEntry {
    mode: String,
    code: CodeValue,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct VerifiedEntry {
    state: String,
    reference: Option<String>,
    radio: Option<String>,
    firmware: Option<String>,
    date: Option<String>,
    tested: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FactEntry {
    key: String,
    provenance: Provenance,
    verified: VerifiedEntry,
}

fn bad(detail: impl Into<String>) -> RigError {
    RigError::new(ErrorKind::Malformed, detail)
}

// [@ANCHOR: radioctl_core_rigfile_parse]
/// Parses and validates a description file. Any unknown field, unknown name, duplicate,
/// missing block, missing fact or incomplete verification record is an error.
pub fn parse(text: &str) -> Result<LoadedDescription, RigError> {
    let doc: FileDoc = toml::from_str(text).map_err(|e| bad(format!("rig description: {e}")))?;
    if doc.schema != SCHEMA {
        return Err(bad(format!(
            "schema {} is not supported (expected {SCHEMA})",
            doc.schema
        )));
    }
    let description = build_description(&doc)?;
    let facts = build_facts(&doc)?;
    check_coverage(&description, &facts)?;
    Ok(LoadedDescription { description, facts })
}

// [@ANCHOR: radioctl_core_rigfile_build_description]
fn build_description(doc: &FileDoc) -> Result<RigDescription, RigError> {
    let family = match doc.rig.family.as_str() {
        "icom-civ" => Family::IcomCiv,
        "kenwood-ascii" => Family::KenwoodAscii,
        "elecraft-ascii" => Family::ElecraftAscii,
        "yaesu-newcat" => Family::YaesuNewcat,
        other => return Err(bad(format!("unknown family {other:?}"))),
    };
    match (family, &doc.civ, &doc.kenwood, &doc.elecraft, &doc.yaesu) {
        (Family::IcomCiv, Some(_), None, None, None)
        | (Family::KenwoodAscii, None, Some(_), None, None)
        | (Family::ElecraftAscii, None, None, Some(_), None)
        | (Family::YaesuNewcat, None, None, None, Some(_)) => {}
        _ => return Err(bad("the family block does not match rig.family")),
    }
    let civ = doc.civ.as_ref().map(|c| CivParams {
        address: c.address,
        controller: c.controller,
        model_id: c.model_id,
        echo: c.echo,
    });
    let kenwood = doc.kenwood.as_ref().map(|k| KenwoodParams {
        model_id: k.model_id,
    });
    let elecraft = doc.elecraft.as_ref().map(|e| ElecraftParams {
        model_id: e.model_id,
    });
    let scope = build_scope(doc.scope.as_ref(), family)?;
    let yaesu = doc.yaesu.as_ref().map(|y| YaesuParams {
        model_id: y.model_id,
    });
    let mut capabilities = Vec::new();
    for name in &doc.capabilities {
        let capability = Capability::from_name(name)
            .ok_or_else(|| bad(format!("unknown capability {name:?}")))?;
        if capabilities.contains(&capability) {
            return Err(bad(format!("capability {name:?} listed twice")));
        }
        capabilities.push(capability);
    }
    if capabilities.is_empty() {
        return Err(bad("a description needs at least one capability"));
    }
    let default_mode = Mode::from_name(&doc.defaults.mode)
        .ok_or_else(|| bad(format!("unknown default mode {:?}", doc.defaults.mode)))?;
    Ok(RigDescription {
        id: doc.rig.id.clone(),
        name: doc.rig.name.clone(),
        family,
        civ,
        kenwood,
        elecraft,
        yaesu,
        scope,
        capabilities: CapabilitySet::of(&capabilities),
        mode_codes: build_mode_codes(&doc.modes)?,
        timing: Timing {
            timeout: Duration::from_millis(doc.timing.timeout_ms),
            retries: doc.timing.retries,
            inter_command: Duration::from_millis(doc.timing.inter_command_ms),
        },
        default_frequency: Hz(doc.defaults.frequency_hz),
        default_mode,
    })
}

// [@ANCHOR: radioctl_core_rigfile_build_scope]
fn build_scope(
    section: Option<&ScopeSection>,
    family: Family,
) -> Result<Option<ScopeParams>, RigError> {
    let Some(scope) = section else {
        return Ok(None);
    };
    if family != Family::IcomCiv {
        return Err(bad("a [scope] block is only defined for icom-civ"));
    }
    let sane = (1..=2).contains(&scope.receivers)
        && scope.line_length > 0
        && scope.samples_per_frame > 0
        && scope.amplitude_max > 0
        && !scope.spans_hz.is_empty();
    if !sane {
        return Err(bad("scope values must be positive and receivers 1 or 2"));
    }
    Ok(Some(ScopeParams {
        receivers: scope.receivers,
        line_length: scope.line_length,
        samples_per_frame: scope.samples_per_frame,
        amplitude_max: scope.amplitude_max,
        spans_hz: scope.spans_hz.clone(),
    }))
}

// [@ANCHOR: radioctl_core_rigfile_build_mode_codes]
fn build_mode_codes(entries: &[ModeEntry]) -> Result<Vec<(Mode, u8)>, RigError> {
    let mut out: Vec<(Mode, u8)> = Vec::new();
    for entry in entries {
        let mode = Mode::from_name(&entry.mode)
            .ok_or_else(|| bad(format!("unknown mode {:?}", entry.mode)))?;
        let code = match &entry.code {
            CodeValue::Byte(byte) => *byte,
            CodeValue::Char(text) => match text.as_bytes() {
                [byte] => *byte,
                _ => return Err(bad(format!("mode code {text:?} must be one character"))),
            },
        };
        if out.iter().any(|(m, c)| *m == mode || *c == code) {
            return Err(bad(format!(
                "mode {:?} or its code {code:#x} appears twice",
                entry.mode
            )));
        }
        out.push((mode, code));
    }
    if out.is_empty() {
        return Err(bad("a description needs at least one mode code"));
    }
    Ok(out)
}

// [@ANCHOR: radioctl_core_rigfile_build_facts]
fn build_facts(doc: &FileDoc) -> Result<Vec<Fact>, RigError> {
    let mut seen = BTreeSet::new();
    let mut facts = Vec::new();
    for entry in &doc.facts {
        if !seen.insert(entry.key.clone()) {
            return Err(bad(format!("fact {:?} appears twice", entry.key)));
        }
        if entry.provenance.reference.trim().is_empty() {
            return Err(bad(format!(
                "fact {:?} has an empty provenance reference",
                entry.key
            )));
        }
        facts.push(Fact {
            key: entry.key.clone(),
            provenance: entry.provenance.clone(),
            verified: build_verified(&entry.key, &entry.verified)?,
        });
    }
    Ok(facts)
}

// [@ANCHOR: radioctl_core_rigfile_build_verified]
fn build_verified(key: &str, entry: &VerifiedEntry) -> Result<Verified, RigError> {
    let need = |value: &Option<String>, field: &str| -> Result<String, RigError> {
        match value.as_deref().map(str::trim) {
            Some(text) if !text.is_empty() => Ok(text.to_string()),
            _ => Err(bad(format!(
                "fact {key:?}: state {:?} needs {field}",
                entry.state
            ))),
        }
    };
    match entry.state.as_str() {
        "unverified" => Ok(Verified::Unverified),
        "manual-checked" => Ok(Verified::ManualChecked {
            reference: need(&entry.reference, "reference")?,
        }),
        "transcript-checked" => Ok(Verified::TranscriptChecked {
            reference: need(&entry.reference, "reference")?,
        }),
        "hardware-verified" => {
            let date = need(&entry.date, "date")?;
            if !is_iso_date(&date) {
                return Err(bad(format!(
                    "fact {key:?}: date {date:?} is not YYYY-MM-DD"
                )));
            }
            Ok(Verified::HardwareVerified {
                radio: need(&entry.radio, "radio")?,
                firmware: need(&entry.firmware, "firmware")?,
                date,
                tested: need(&entry.tested, "tested")?,
            })
        }
        other => Err(bad(format!(
            "fact {key:?}: unknown verified state {other:?}"
        ))),
    }
}

fn is_iso_date(text: &str) -> bool {
    let parts: Vec<&str> = text.split('-').collect();
    matches!(parts.as_slice(), [y, m, d]
        if y.len() == 4 && m.len() == 2 && d.len() == 2
            && parts.iter().all(|p| p.bytes().all(|b| b.is_ascii_digit())))
}

// [@ANCHOR: radioctl_core_rigfile_expected_keys]
/// Every key a description must explain.
pub fn expected_keys(description: &RigDescription) -> Vec<String> {
    let mut keys: Vec<String> = [
        "rig.id",
        "rig.name",
        "rig.family",
        "timing.timeout_ms",
        "timing.retries",
        "timing.inter_command_ms",
        "defaults.frequency_hz",
        "defaults.mode",
    ]
    .iter()
    .map(|k| k.to_string())
    .collect();
    if description.civ.is_some() {
        keys.extend(
            ["civ.address", "civ.controller", "civ.model_id", "civ.echo"].map(String::from),
        );
    }
    if description.scope.is_some() {
        keys.extend(
            [
                "scope.receivers",
                "scope.line_length",
                "scope.samples_per_frame",
                "scope.amplitude_max",
                "scope.spans_hz",
            ]
            .map(String::from),
        );
    }
    if description.kenwood.is_some() {
        keys.push("kenwood.model_id".to_string());
    }
    if description.elecraft.is_some() {
        keys.push("elecraft.model_id".to_string());
    }
    if description.yaesu.is_some() {
        keys.push("yaesu.model_id".to_string());
    }
    keys.extend(
        description
            .capabilities
            .iter()
            .map(|c| format!("capability.{}", c.name())),
    );
    keys.extend(
        description
            .mode_codes
            .iter()
            .map(|(m, _)| format!("mode.{}", m.name())),
    );
    keys
}

// [@ANCHOR: radioctl_core_rigfile_matches_key]
fn matches_key(fact_key: &str, key: &str) -> bool {
    match fact_key.strip_suffix(".*") {
        Some(prefix) => key
            .strip_prefix(prefix)
            .is_some_and(|rest| rest.starts_with('.')),
        None => fact_key == key,
    }
}

// [@ANCHOR: radioctl_core_rigfile_check_coverage]
fn check_coverage(description: &RigDescription, facts: &[Fact]) -> Result<(), RigError> {
    let expected = expected_keys(description);
    for key in &expected {
        if !facts.iter().any(|f| matches_key(&f.key, key)) {
            return Err(bad(format!("no provenance fact covers {key:?}")));
        }
    }
    for fact in facts {
        if !expected.iter().any(|key| matches_key(&fact.key, key)) {
            return Err(bad(format!(
                "fact {:?} matches no key of this description",
                fact.key
            )));
        }
    }
    Ok(())
}

impl LoadedDescription {
    // [@ANCHOR: radioctl_core_rigfile_fact_for]
    /// The fact that explains `key`: an exact one if present, else a wildcard.
    pub fn fact_for(&self, key: &str) -> Option<&Fact> {
        let exact = self.facts.iter().find(|f| f.key == key);
        exact.or_else(|| self.facts.iter().find(|f| matches_key(&f.key, key)))
    }

    // [@ANCHOR: radioctl_core_rigfile_verification_summary]
    /// How many keys are in each verified state, keyed by state name.
    pub fn verification_summary(&self) -> BTreeMap<&'static str, usize> {
        let mut counts = BTreeMap::new();
        for key in expected_keys(&self.description) {
            let name = match self.fact_for(&key).map(|f| &f.verified) {
                Some(Verified::Unverified) | None => "unverified",
                Some(Verified::ManualChecked { .. }) => "manual-checked",
                Some(Verified::TranscriptChecked { .. }) => "transcript-checked",
                Some(Verified::HardwareVerified { .. }) => "hardware-verified",
            };
            *counts.entry(name).or_insert(0) += 1;
        }
        counts
    }

    // [@ANCHOR: radioctl_core_rigfile_is_hardware_verified]
    /// True only when every key of the description is hardware-verified.
    pub fn is_hardware_verified(&self) -> bool {
        let summary = self.verification_summary();
        summary.len() == 1 && summary.contains_key("hardware-verified")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOOD: &str = r#"
schema = 1
capabilities = ["read-frequency", "set-frequency", "set-ptt"]

[rig]
id = "test-rig"
name = "Test Rig"
family = "kenwood-ascii"

[kenwood]
model_id = 19

[timing]
timeout_ms = 200
retries = 3
inter_command_ms = 0

[defaults]
frequency_hz = 14074000
mode = "usb"

[[modes]]
mode = "usb"
code = "2"

[[modes]]
mode = "lsb"
code = 0x31

[[facts]]
key = "rig.*"
provenance = { kind = "repo-code", reference = "tools/rig_simulator.py" }
verified = { state = "unverified" }

[[facts]]
key = "timing.*"
provenance = { kind = "repo-code", reference = "tools/rig_simulator.py" }
verified = { state = "unverified" }

[[facts]]
key = "defaults.*"
provenance = { kind = "repo-code", reference = "tools/rig_simulator.py" }
verified = { state = "unverified" }

[[facts]]
key = "kenwood.model_id"
provenance = { kind = "repo-code", reference = "tools/rig_simulator.py: ID019" }
verified = { state = "unverified" }

[[facts]]
key = "capability.*"
provenance = { kind = "manual", reference = "TS-2000 command reference" }
verified = { state = "manual-checked", reference = "TS-2000 command reference, FA, TX" }

[[facts]]
key = "capability.set-ptt"
provenance = { kind = "owner", reference = "K6BP bench" }
verified = { state = "hardware-verified", radio = "TS-2000 #1", firmware = "1.0", date = "2026-10-30", tested = "TX; then RX; with a dummy load" }

[[facts]]
key = "mode.*"
provenance = { kind = "repo-code", reference = "tools/rig_simulator.py CAT_MODE_NAMES" }
verified = { state = "unverified" }
"#;

    fn expect_rejected(text: &str, needle: &str) {
        let error = parse(text).expect_err("must be rejected");
        assert_eq!(error.kind, ErrorKind::Malformed);
        assert!(
            error.detail.contains(needle),
            "{} lacks {needle:?}",
            error.detail
        );
    }

    // Tests [@ANCHOR: radioctl_core_rigfile_parse]
    // Tests [@ANCHOR: radioctl_core_rigfile_build_description]
    // Tests [@ANCHOR: radioctl_core_rigfile_build_facts]
    // Tests [@ANCHOR: radioctl_core_rigfile_build_mode_codes]
    // Tests [@ANCHOR: radioctl_core_rigfile_build_verified]
    // Tests [@ANCHOR: radioctl_core_rigfile_check_coverage]
    // Tests [@ANCHOR: radioctl_core_rigfile_matches_key]
    #[test]
    fn a_complete_description_loads_with_its_facts() {
        let loaded = parse(GOOD).expect("loads");
        assert_eq!(loaded.description.id, "test-rig");
        assert_eq!(loaded.description.mode_code(Mode::Usb), Some(b'2'));
        assert_eq!(loaded.description.mode_code(Mode::Lsb), Some(b'1'));
        assert_eq!(loaded.description.default_frequency, Hz(14_074_000));
        assert!(loaded.description.capabilities.has(Capability::SetPtt));
        assert_eq!(
            loaded.description.timing.timeout,
            Duration::from_millis(200)
        );
    }

    // Tests [@ANCHOR: radioctl_core_rigfile_parse]
    #[test]
    fn unexplained_wrong_or_unknown_data_is_rejected() {
        expect_rejected(&GOOD.replace("schema = 1", "schema = 2"), "schema 2");
        expect_rejected(
            &GOOD.replace("read-frequency", "read-voltage"),
            "unknown capability",
        );
        expect_rejected(
            &GOOD.replace("family = \"kenwood-ascii\"", "family = \"yaesu\""),
            "unknown family",
        );
        expect_rejected(
            &GOOD.replace("family = \"kenwood-ascii\"", "family = \"icom-civ\""),
            "does not match",
        );
        expect_rejected(
            &GOOD.replace("mode = \"lsb\"", "mode = \"usb\""),
            "appears twice",
        );
        expect_rejected(
            &GOOD.replace("code = \"2\"", "code = \"22\""),
            "one character",
        );
        expect_rejected(
            &GOOD.replace("retries = 3", "retries = 3\nsurprise = 1"),
            "surprise",
        );
        expect_rejected("not toml at all [", "rig description");
        expect_rejected(
            &GOOD.replace(
                "state = \"manual-checked\", reference = \"TS-2000 command reference, FA, TX\"",
                "state = \"manual-checked\"",
            ),
            "needs reference",
        );
        expect_rejected(&GOOD.replace("firmware = \"1.0\", ", ""), "needs firmware");
        expect_rejected(&GOOD.replace("2026-10-30", "30/10/2026"), "YYYY-MM-DD");
        expect_rejected(
            &GOOD.replace("state = \"unverified\"", "state = \"probably-fine\""),
            "unknown verified state",
        );
        expect_rejected(
            &GOOD.replace(
                "reference = \"tools/rig_simulator.py\"",
                "reference = \" \"",
            ),
            "empty provenance",
        );
    }

    // Tests [@ANCHOR: radioctl_core_rigfile_build_description]
    // Tests [@ANCHOR: radioctl_core_rigfile_expected_keys]
    #[test]
    fn the_elecraft_family_needs_its_own_block_and_fact() {
        let elecraft = GOOD
            .replace("family = \"kenwood-ascii\"", "family = \"elecraft-ascii\"")
            .replace("[kenwood]", "[elecraft]")
            .replace("key = \"kenwood.model_id\"", "key = \"elecraft.model_id\"");
        let loaded = parse(&elecraft).expect("loads");
        assert_eq!(loaded.description.family, Family::ElecraftAscii);
        assert_eq!(
            loaded.description.elecraft,
            Some(ElecraftParams { model_id: 19 })
        );
        assert!(expected_keys(&loaded.description).contains(&"elecraft.model_id".to_string()));
        expect_rejected(
            &elecraft.replace("[elecraft]", "[kenwood]"),
            "does not match",
        );
    }

    // Tests [@ANCHOR: radioctl_core_rigfile_expected_keys]
    // Tests [@ANCHOR: radioctl_core_rigfile_parse]
    #[test]
    fn every_key_needs_a_fact_and_every_fact_a_key() {
        let without_modes = GOOD.replace("key = \"mode.*\"", "key = \"mode.nothing\"");
        expect_rejected(&without_modes, "no provenance fact covers \"mode.usb\"");
        let extra = format!("{GOOD}\n[[facts]]\nkey = \"civ.address\"\nprovenance = {{ kind = \"manual\", reference = \"x\" }}\nverified = {{ state = \"unverified\" }}\n");
        expect_rejected(&extra, "matches no key");
        let duplicate = format!("{GOOD}\n[[facts]]\nkey = \"rig.*\"\nprovenance = {{ kind = \"manual\", reference = \"x\" }}\nverified = {{ state = \"unverified\" }}\n");
        expect_rejected(&duplicate, "appears twice");
        let keys = expected_keys(&parse(GOOD).expect("loads").description);
        assert!(keys.contains(&"kenwood.model_id".to_string()));
        assert!(keys.contains(&"capability.set-ptt".to_string()));
        assert!(!keys.iter().any(|k| k.starts_with("civ.")));
        assert!(!keys.iter().any(|k| k.starts_with("elecraft.")));
    }

    // Tests [@ANCHOR: radioctl_core_rigfile_fact_for]
    // Tests [@ANCHOR: radioctl_core_rigfile_verification_summary]
    // Tests [@ANCHOR: radioctl_core_rigfile_is_hardware_verified]
    #[test]
    fn an_exact_fact_wins_and_the_summary_counts_states() {
        let loaded = parse(GOOD).expect("loads");
        let ptt = loaded.fact_for("capability.set-ptt").expect("fact");
        assert!(matches!(ptt.verified, Verified::HardwareVerified { .. }));
        let freq = loaded.fact_for("capability.read-frequency").expect("fact");
        assert!(matches!(freq.verified, Verified::ManualChecked { .. }));
        assert!(loaded.fact_for("civ.address").is_none());
        let summary = loaded.verification_summary();
        assert_eq!(summary.get("hardware-verified"), Some(&1));
        assert_eq!(summary.get("manual-checked"), Some(&2));
        assert_eq!(summary.get("unverified"), Some(&(8 + 1 + 2)));
        assert!(!loaded.is_hardware_verified());
    }
    // Tests [@ANCHOR: radioctl_core_rigfile_build_scope]
    #[test]
    fn a_scope_block_is_icom_only_sane_and_needs_its_facts() {
        let scope = "\n[scope]\nreceivers = 1\nline_length = 475\nsamples_per_frame = 50\namplitude_max = 160\nspans_hz = [5000, 10000]\n";
        expect_rejected(&format!("{GOOD}{scope}"), "only defined for icom-civ");
        let civ = GOOD
            .replace("kenwood-ascii", "icom-civ")
            .replace(
                "[kenwood]\nmodel_id = 19",
                "[civ]\naddress = 0x94\ncontroller = 0xE0\nmodel_id = 0x94\necho = false",
            )
            .replace("key = \"kenwood.model_id\"", "key = \"civ.*\"")
            .replace("code = \"2\"", "code = 1")
            .replace("code = 0x31", "code = 0");
        let with_scope = format!("{civ}{scope}");
        expect_rejected(&with_scope, "no provenance fact covers \"scope.receivers\"");
        let fact = "\n[[facts]]\nkey = \"scope.*\"\nprovenance = { kind = \"hamlib-source\", reference = \"Hamlib 4.6.2 rigs/icom/ic7300.c\", note = \"facts only\" }\nverified = { state = \"unverified\" }\n";
        let loaded = parse(&format!("{with_scope}{fact}")).expect("loads");
        let params = loaded.description.scope.expect("scope");
        assert_eq!((params.receivers, params.line_length), (1, 475));
        assert_eq!(params.spans_hz, vec![5000, 10000]);
        let bad = with_scope.replace("receivers = 1", "receivers = 3");
        expect_rejected(&format!("{bad}{fact}"), "receivers 1 or 2");
    }
}
