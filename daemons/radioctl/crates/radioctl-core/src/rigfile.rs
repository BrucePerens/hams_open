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
//! `civ.controller`, `civ.model_id`, `civ.echo`, `kenwood.model_id`, `kenwood.dialect` (only when not `classic`), `elecraft.model_id`, `yaesu.model_id`,
//! `yaesu.frequency_digits` (only when not 9),
//! `scope.receivers`, `scope.line_length`, `scope.samples_per_frame`, `scope.amplitude_max`,
//! `scope.spans_hz` (an optional `[scope]` block, icom-civ only), `levels.attenuator_db` and
//! `levels.preamp_steps` (an optional `[levels]` block), `capability.<name>`, `mode.<name>`.
//!
//! Verified states: `unverified`; `manual-checked` (needs `reference`); `transcript-checked`
//! (needs `reference`, the transcript); `hardware-verified` (needs `radio`, `firmware`, `date`
//! and `tested`: exactly what was tested on which radio and firmware).

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Deserialize;

use crate::capability::{Capability, CapabilitySet};
use crate::description::{
    CivCommandSet, CivParams, ElecraftParams, Family, FlexParams, HhDialect, HpsdrParams,
    KenwoodDialect, KenwoodHhParams, KenwoodParams, LegacyBlock, LegacyCommand, LegacyFlag,
    LegacyFreqForm, LegacyRead, LevelParams, ReceiverParams, ReceiverProtocol, RigDescription,
    ScopeParams, Timing, UsbParams, UsbProtocol, VendorParams, VendorProtocol, YaesuLegacyParams,
    YaesuParams,
};
use crate::error::{ErrorKind, RigError};
use crate::units::{Hz, Level, Mode};

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
    yaesu_legacy: Option<YaesuLegacySection>,
    kenwood_hh: Option<KenwoodHhSection>,
    flex: Option<FlexSection>,
    hpsdr: Option<HpsdrSection>,
    vendor: Option<VendorSection>,
    receiver: Option<ReceiverSection>,
    #[serde(default)]
    usb: Option<UsbSection>,
    scope: Option<ScopeSection>,
    levels: Option<LevelsSection>,
    timing: TimingSection,
    defaults: DefaultsSection,
    capabilities: Vec<String>,
    #[serde(default)]
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
    #[serde(default = "default_frequency_bytes")]
    frequency_bytes: u8,
    #[serde(default)]
    command_set: CivCommandSetName,
}

#[derive(Deserialize, Default, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
enum CivCommandSetName {
    #[default]
    Standard,
    F8101,
}

fn default_frequency_bytes() -> u8 {
    5
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct KenwoodSection {
    model_id: u16,
    #[serde(default)]
    dialect: Option<String>,
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
    #[serde(default)]
    frequency_digits: Option<u8>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct YaesuLegacySection {
    stop_bits: u8,
    #[serde(default)]
    high_band_flag: bool,
    #[serde(default)]
    echo: bool,
    set_frequency: Option<LegacyCommandEntry>,
    set_mode: Option<LegacyCommandEntry>,
    ptt_on: Option<[u8; 5]>,
    ptt_off: Option<[u8; 5]>,
    split_on: Option<[u8; 5]>,
    split_off: Option<[u8; 5]>,
    #[serde(default)]
    reads: Vec<LegacyReadEntry>,
    #[serde(default)]
    status_modes: Vec<ModeEntry>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacyCommandEntry {
    template: [u8; 5],
    at: u8,
    form: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacyFreqEntry {
    offset: u16,
    form: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacyModeEntry {
    offset: u16,
    mask: u8,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacyFlagEntry {
    offset: u16,
    mask: u8,
    on_when_set: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacyReadEntry {
    request: [u8; 5],
    length: u16,
    frequency: Option<LegacyFreqEntry>,
    mode: Option<LegacyModeEntry>,
    ptt: Option<LegacyFlagEntry>,
    split: Option<LegacyFlagEntry>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct KenwoodHhSection {
    dialect: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FlexSection {
    model: String,
    scus: u8,
    #[serde(default)]
    slice: u8,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReceiverSection {
    protocol: String,
    dialect: String,
    #[serde(default)]
    address: Option<u8>,
    baud: u32,
    data_bits: u8,
    parity: String,
    stop_bits: u8,
    flow: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct HpsdrSection {
    model: String,
    protocol: u8,
    anan10: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct VendorSection {
    protocol: String,
    dialect: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct UsbSection {
    protocol: String,
    vendor_id: u16,
    product_id: u16,
    manufacturer: String,
    product: String,
    multiplier: u8,
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
struct LevelsSection {
    attenuator_db: Vec<u8>,
    #[serde(default)]
    attenuator_any_up_to_db: u8,
    preamp_steps: u8,
    #[serde(default)]
    absent: Vec<String>,
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

// [@ANCHOR: radioctl_core_rigfile_rigs_dir]
/// The directory of rig descriptions: `$RADIOCTL_RIGS`; else `./rigs` when it exists (binaries
/// copied to a bench machine run from the directory that holds `rigs/`); else the `rigs/`
/// directory of the checkout this library was built from.
pub fn rigs_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("RADIOCTL_RIGS") {
        return PathBuf::from(dir);
    }
    let here = PathBuf::from("rigs");
    if here.is_dir() {
        return here;
    }
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../rigs")
}

// [@ANCHOR: radioctl_core_rigfile_load]
/// Reads and validates a description. `which` is a rig id found in [`rigs_dir`]
/// (`"icom-ic-7300"` reads `icom-ic-7300.toml`), or a path to a description file (anything that
/// ends in `.toml` or has a directory part). The error names the file.
pub fn load(which: &str) -> Result<LoadedDescription, RigError> {
    let path = if which.ends_with(".toml") || which.contains('/') {
        PathBuf::from(which)
    } else {
        rigs_dir().join(format!("{which}.toml"))
    };
    let text = std::fs::read_to_string(&path)
        .map_err(|e| bad(format!("cannot read {}: {e}", path.display())))?;
    parse(&text).map_err(|e| bad(format!("{}: {}", path.display(), e.detail)))
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
        "yaesu-legacy" => Family::YaesuLegacy,
        "flex-smartsdr" => Family::Flex,
        "hpsdr-network" => Family::Hpsdr,
        "kenwood-hh" => Family::KenwoodHh,
        "serial-vendor" => Family::SerialVendor,
        "receiver" => Family::Receiver,
        "usb-device" => Family::UsbDevice,
        other => return Err(bad(format!("unknown family {other:?}"))),
    };
    let others = doc.civ.is_some()
        || doc.kenwood.is_some()
        || doc.elecraft.is_some()
        || doc.yaesu.is_some()
        || doc.yaesu_legacy.is_some()
        || doc.flex.is_some()
        || doc.hpsdr.is_some()
        || doc.kenwood_hh.is_some();
    if doc.usb.is_some() != (family == Family::UsbDevice) {
        return Err(bad("the family block does not match rig.family"));
    }
    if doc.receiver.is_some() != (family == Family::Receiver) {
        return Err(bad("the family block does not match rig.family"));
    }
    match (
        family,
        &doc.civ,
        &doc.kenwood,
        &doc.elecraft,
        &doc.yaesu,
        &doc.yaesu_legacy,
    ) {
        (Family::IcomCiv, Some(_), None, None, None, None)
        | (Family::KenwoodAscii, None, Some(_), None, None, None)
        | (Family::ElecraftAscii, None, None, Some(_), None, None)
        | (Family::YaesuNewcat, None, None, None, Some(_), None)
        | (Family::YaesuLegacy, None, None, None, None, Some(_))
            if doc.flex.is_none() && doc.hpsdr.is_none() && doc.kenwood_hh.is_none() => {}
        (Family::Flex, None, None, None, None, None)
            if doc.flex.is_some() && doc.hpsdr.is_none() && doc.kenwood_hh.is_none() => {}
        (Family::Hpsdr, None, None, None, None, None)
            if doc.hpsdr.is_some() && doc.flex.is_none() && doc.kenwood_hh.is_none() => {}
        (Family::KenwoodHh, None, None, None, None, None)
            if doc.kenwood_hh.is_some() && doc.flex.is_none() && doc.hpsdr.is_none() => {}
        (Family::SerialVendor, ..) if doc.vendor.is_some() && !others => {}
        (Family::UsbDevice, None, None, None, None, None)
            if doc.flex.is_none()
                && doc.hpsdr.is_none()
                && doc.kenwood_hh.is_none()
                && doc.vendor.is_none() => {}
        (Family::Receiver, None, None, None, None, None)
            if doc.flex.is_none()
                && doc.hpsdr.is_none()
                && doc.kenwood_hh.is_none()
                && doc.vendor.is_none() => {}
        _ => return Err(bad("the family block does not match rig.family")),
    }
    if family != Family::SerialVendor && doc.vendor.is_some() {
        return Err(bad("the family block does not match rig.family"));
    }
    if doc
        .civ
        .as_ref()
        .is_some_and(|c| !(4..=6).contains(&c.frequency_bytes))
    {
        return Err(bad("civ.frequency_bytes must be 4, 5 or 6"));
    }
    let civ = doc.civ.as_ref().map(|c| CivParams {
        address: c.address,
        controller: c.controller,
        model_id: c.model_id,
        echo: c.echo,
        frequency_bytes: c.frequency_bytes,
        command_set: match c.command_set {
            CivCommandSetName::Standard => CivCommandSet::Standard,
            CivCommandSetName::F8101 => CivCommandSet::F8101,
        },
    });
    let kenwood = doc
        .kenwood
        .as_ref()
        .map(|k| {
            let dialect = match k.dialect.as_deref() {
                None => KenwoodDialect::Classic,
                Some(name) => KenwoodDialect::from_name(name)
                    .ok_or_else(|| bad(format!("unknown kenwood.dialect {name:?}")))?,
            };
            Ok::<KenwoodParams, RigError>(KenwoodParams {
                model_id: k.model_id,
                dialect,
            })
        })
        .transpose()?;
    let elecraft = doc.elecraft.as_ref().map(|e| ElecraftParams {
        model_id: e.model_id,
    });
    let scope = build_scope(doc.scope.as_ref(), family)?;
    let levels = build_levels(doc.levels.as_ref())?;
    let yaesu = doc
        .yaesu
        .as_ref()
        .map(|y| {
            let frequency_digits = y.frequency_digits.unwrap_or(9);
            if !matches!(frequency_digits, 8 | 9) {
                return Err(bad(format!(
                    "yaesu.frequency_digits must be 8 or 9, not {frequency_digits}"
                )));
            }
            Ok::<YaesuParams, RigError>(YaesuParams {
                model_id: y.model_id,
                frequency_digits,
            })
        })
        .transpose()?;
    let yaesu_legacy = doc
        .yaesu_legacy
        .as_ref()
        .map(|y| {
            Ok::<YaesuLegacyParams, RigError>(YaesuLegacyParams {
                stop_bits: y.stop_bits,
                high_band_flag: y.high_band_flag,
                block: build_legacy_block(y)?,
            })
        })
        .transpose()?;
    let kenwood_hh = doc
        .kenwood_hh
        .as_ref()
        .map(|k| {
            HhDialect::from_name(&k.dialect)
                .map(|dialect| KenwoodHhParams { dialect })
                .ok_or_else(|| bad(format!("unknown kenwood_hh.dialect {:?}", k.dialect)))
        })
        .transpose()?;
    let flex = doc.flex.as_ref().map(|f| FlexParams {
        model: f.model.clone(),
        scus: f.scus,
        slice: f.slice,
    });
    let hpsdr = doc.hpsdr.as_ref().map(|h| HpsdrParams {
        model: h.model.clone(),
        protocol: h.protocol,
        anan10: h.anan10,
    });
    let vendor = doc
        .vendor
        .as_ref()
        .map(|v| {
            let protocol = VendorProtocol::from_name(&v.protocol)
                .ok_or_else(|| bad(format!("unknown vendor.protocol {:?}", v.protocol)))?;
            Ok::<VendorParams, RigError>(VendorParams {
                protocol,
                dialect: v.dialect.clone(),
            })
        })
        .transpose()?;
    let receiver = doc.receiver.as_ref().map(build_receiver).transpose()?;
    let usb = doc.usb.as_ref().map(build_usb).transpose()?;
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
        yaesu_legacy,
        kenwood_hh,
        flex,
        hpsdr,
        vendor,
        receiver,
        usb,
        scope,
        levels,
        capabilities: CapabilitySet::of(&capabilities),
        // A USB device has no engine yet, so no mode has a wire code.
        mode_codes: build_mode_codes(&doc.modes, family == Family::UsbDevice)?,
        timing: check_timing(&doc.timing)?,
        default_frequency: Hz(doc.defaults.frequency_hz),
        default_mode,
    })
}

// [@ANCHOR: radioctl_core_rigfile_build_usb]
fn build_usb(u: &UsbSection) -> Result<UsbParams, RigError> {
    let protocol = UsbProtocol::from_name(&u.protocol)
        .ok_or_else(|| bad(format!("unknown usb.protocol {:?}", u.protocol)))?;
    if !matches!(u.multiplier, 0 | 1 | 2 | 4 | 8) {
        return Err(bad("usb.multiplier must be 0, 1, 2, 4 or 8"));
    }
    Ok(UsbParams {
        protocol,
        vendor_id: u.vendor_id,
        product_id: u.product_id,
        manufacturer: u.manufacturer.clone(),
        product: u.product.clone(),
        multiplier: u.multiplier,
    })
}

// [@ANCHOR: radioctl_core_rigfile_build_receiver]
fn build_receiver(r: &ReceiverSection) -> Result<ReceiverParams, RigError> {
    let protocol = ReceiverProtocol::from_name(&r.protocol)
        .ok_or_else(|| bad(format!("unknown receiver.protocol {:?}", r.protocol)))?;
    let parity = match r.parity.as_str() {
        "N" => 'N',
        "E" => 'E',
        "O" => 'O',
        other => {
            return Err(bad(format!(
                "receiver.parity must be N, E or O, not {other:?}"
            )))
        }
    };
    if !(5..=8).contains(&r.data_bits) || !(1..=2).contains(&r.stop_bits) || r.baud == 0 {
        return Err(bad("receiver line settings out of range"));
    }
    if !matches!(r.flow.as_str(), "none" | "xonxoff" | "rtscts") {
        return Err(bad("receiver.flow must be none, xonxoff or rtscts"));
    }
    if r.dialect.is_empty() {
        return Err(bad("receiver.dialect is empty"));
    }
    Ok(ReceiverParams {
        protocol,
        dialect: r.dialect.clone(),
        address: r.address,
        baud: r.baud,
        data_bits: r.data_bits,
        parity,
        stop_bits: r.stop_bits,
        flow: r.flow.clone(),
    })
}

/// Bounds on the timing block. A zero reply timeout makes every command time out at once (and
/// every key a fail-closed unkey); a long inter-command gap holds every command, so both are
/// refused at load rather than discovered on the air.
const TIMEOUT_MS: std::ops::RangeInclusive<u64> = 1..=30_000;
const INTER_COMMAND_MS_MAX: u64 = 2_000;

// [@ANCHOR: radioctl_core_rigfile_check_timing]
/// Validates the timing block: `timeout_ms` in 1..=30000, `inter_command_ms` at most 2000.
fn check_timing(timing: &TimingSection) -> Result<Timing, RigError> {
    if !TIMEOUT_MS.contains(&timing.timeout_ms) {
        return Err(bad(format!(
            "timing.timeout_ms = {} is outside {}..={} ms",
            timing.timeout_ms,
            TIMEOUT_MS.start(),
            TIMEOUT_MS.end()
        )));
    }
    if timing.inter_command_ms > INTER_COMMAND_MS_MAX {
        return Err(bad(format!(
            "timing.inter_command_ms = {} is more than {INTER_COMMAND_MS_MAX} ms",
            timing.inter_command_ms
        )));
    }
    Ok(Timing {
        timeout: Duration::from_millis(timing.timeout_ms),
        retries: timing.retries,
        inter_command: Duration::from_millis(timing.inter_command_ms),
    })
}

/// Most attenuator steps a model may list.
const MAX_ATTENUATOR_STEPS: usize = 9;
/// Most preamplifier steps a model may declare.
const MAX_PREAMP_STEPS: u8 = 3;

// [@ANCHOR: radioctl_core_rigfile_build_levels]
fn build_levels(section: Option<&LevelsSection>) -> Result<LevelParams, RigError> {
    let Some(levels) = section else {
        return Ok(LevelParams::default());
    };
    let ascending = levels
        .attenuator_db
        .windows(2)
        .all(|w| matches!(w, [a, b] if a < b));
    let first_positive = levels.attenuator_db.first().is_none_or(|d| *d > 0);
    if !ascending || !first_positive || levels.attenuator_db.len() > MAX_ATTENUATOR_STEPS {
        return Err(bad(format!(
            "levels.attenuator_db must be at most {MAX_ATTENUATOR_STEPS} ascending positive decibel values"
        )));
    }
    if levels.attenuator_any_up_to_db > 0 && !levels.attenuator_db.is_empty() {
        return Err(bad(
            "levels.attenuator_db and levels.attenuator_any_up_to_db are alternatives",
        ));
    }
    if levels.preamp_steps > MAX_PREAMP_STEPS {
        return Err(bad(format!(
            "levels.preamp_steps is more than {MAX_PREAMP_STEPS}"
        )));
    }
    let mut absent = Vec::new();
    for name in &levels.absent {
        match Level::from_name(name) {
            Some(level) if !matches!(level, Level::Attenuator | Level::Preamp) => {
                if !absent.contains(&level) {
                    absent.push(level);
                }
            }
            _ => {
                return Err(bad(format!(
                    "levels.absent names {name:?}, which is not one of af-gain, rf-gain, mic-gain, monitor-level, keyer-speed, rf-power"
                )));
            }
        }
    }
    Ok(LevelParams {
        attenuator_db: levels.attenuator_db.clone(),
        attenuator_any_up_to_db: levels.attenuator_any_up_to_db,
        preamp_steps: levels.preamp_steps,
        absent,
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

// [@ANCHOR: radioctl_core_rigfile_legacy_form]
fn legacy_form(name: &str) -> Result<LegacyFreqForm, RigError> {
    Ok(match name {
        "bcd-10hz-msb" => LegacyFreqForm::Bcd10HzMsb,
        "bcd-10hz-lsb" => LegacyFreqForm::Bcd10HzLsb,
        "bcd-1hz-msb" => LegacyFreqForm::Bcd1HzMsb,
        "bin-10hz-msb3" => LegacyFreqForm::Bin10HzMsb3,
        "bin-10hz-lsb3" => LegacyFreqForm::Bin10HzLsb3,
        "bin-10hz-msb4" => LegacyFreqForm::Bin10HzMsb4,
        "bin-1hz-msb4" => LegacyFreqForm::Bin1HzMsb4,
        "bin-1.25hz-msb4" => LegacyFreqForm::Bin125Msb4,
        "bin-0.625hz-msb4" => LegacyFreqForm::Bin0625Msb4,
        "frg8800" => LegacyFreqForm::Frg8800,
        other => return Err(bad(format!("unknown frequency form {other:?}"))),
    })
}

// [@ANCHOR: radioctl_core_rigfile_build_legacy_block]
fn build_legacy_block(y: &YaesuLegacySection) -> Result<Option<LegacyBlock>, RigError> {
    let command = |entry: &LegacyCommandEntry| -> Result<LegacyCommand, RigError> {
        if entry.at > 4 {
            return Err(bad("a command slot starts inside the five-byte block"));
        }
        Ok(LegacyCommand {
            template: entry.template,
            at: entry.at,
            form: entry.form.as_deref().map(legacy_form).transpose()?,
        })
    };
    let pair = |on: Option<[u8; 5]>, off: Option<[u8; 5]>| match (on, off) {
        (Some(a), Some(b)) => Ok(Some((a, b))),
        (None, None) => Ok(None),
        _ => Err(bad("an on block needs its off block")),
    };
    let flag = |f: &LegacyFlagEntry| LegacyFlag {
        offset: f.offset,
        mask: f.mask,
        on_when_set: f.on_when_set,
    };
    let mut reads = Vec::new();
    for r in &y.reads {
        reads.push(LegacyRead {
            request: r.request,
            length: r.length,
            frequency: r
                .frequency
                .as_ref()
                .map(|f| legacy_form(&f.form).map(|form| (f.offset, form)))
                .transpose()?,
            mode: r.mode.as_ref().map(|m| (m.offset, m.mask)),
            ptt: r.ptt.as_ref().map(flag),
            split: r.split.as_ref().map(flag),
        });
    }
    let status_modes = if y.status_modes.is_empty() {
        Vec::new()
    } else {
        build_mode_codes(&y.status_modes, false)?
            .into_iter()
            .map(|(mode, code)| (code, mode))
            .collect()
    };
    let block = LegacyBlock {
        set_frequency: y.set_frequency.as_ref().map(command).transpose()?,
        set_mode: y.set_mode.as_ref().map(command).transpose()?,
        ptt: pair(y.ptt_on, y.ptt_off)?,
        split: pair(y.split_on, y.split_off)?,
        reads,
        status_modes,
        echo: y.echo,
    };
    let any = block.set_frequency.is_some()
        || block.set_mode.is_some()
        || block.ptt.is_some()
        || block.split.is_some()
        || !block.reads.is_empty();
    if block
        .set_frequency
        .as_ref()
        .is_some_and(|c| c.form.is_none())
    {
        return Err(bad("the set-frequency command needs a frequency form"));
    }
    if block.reads.iter().any(|r| {
        r.frequency
            .is_some_and(|(o, f)| usize::from(o) + f.width() > usize::from(r.length))
            || r.mode.is_some_and(|(o, _)| o >= r.length)
            || r.ptt.is_some_and(|p| p.offset >= r.length)
            || r.split.is_some_and(|p| p.offset >= r.length)
    }) {
        return Err(bad("a status field lies outside its reply"));
    }
    Ok(any.then_some(block))
}

// [@ANCHOR: radioctl_core_rigfile_build_mode_codes]
fn build_mode_codes(
    entries: &[ModeEntry],
    may_be_empty: bool,
) -> Result<Vec<(Mode, u8)>, RigError> {
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
        // A code may be shared by a sideband and its data variant (CI-V: `data` is the USB code
        // plus the DATA flag), the plain mode listed first so a code read back without the flag
        // is the plain mode.
        let sideband_of = |data: Mode| match data {
            Mode::Data => Some(Mode::Usb),
            Mode::DataR => Some(Mode::Lsb),
            _ => None,
        };
        let shared_with_sideband = sideband_of(mode)
            .is_some_and(|sideband| out.iter().any(|(m, c)| *m == sideband && *c == code));
        let duplicate = out
            .iter()
            .any(|(m, c)| *m == mode || (*c == code && !shared_with_sideband));
        if duplicate {
            return Err(bad(format!(
                "mode {:?} or its code {code:#x} appears twice",
                entry.mode
            )));
        }
        out.push((mode, code));
    }
    if out.is_empty() && !may_be_empty {
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
    if description
        .civ
        .as_ref()
        .is_some_and(|c| c.frequency_bytes != 5)
    {
        keys.push("civ.frequency_bytes".to_string());
    }
    if description
        .civ
        .as_ref()
        .is_some_and(|c| c.command_set != CivCommandSet::Standard)
    {
        keys.push("civ.command_set".to_string());
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
    if description.levels != LevelParams::default() {
        keys.extend(
            [
                "levels.attenuator_db",
                "levels.attenuator_any_up_to_db",
                "levels.preamp_steps",
            ]
            .map(String::from),
        );
        if !description.levels.absent.is_empty() {
            keys.push("levels.absent".to_string());
        }
    }
    if description.kenwood.is_some() {
        keys.push("kenwood.model_id".to_string());
        if description
            .kenwood
            .as_ref()
            .is_some_and(|k| k.dialect != KenwoodDialect::Classic)
        {
            keys.push("kenwood.dialect".to_string());
        }
    }
    if description.elecraft.is_some() {
        keys.push("elecraft.model_id".to_string());
    }
    if let Some(yaesu) = &description.yaesu {
        keys.push("yaesu.model_id".to_string());
        if yaesu.frequency_digits != 9 {
            keys.push("yaesu.frequency_digits".to_string());
        }
    }
    if let Some(legacy) = &description.yaesu_legacy {
        keys.push("yaesu_legacy.stop_bits".to_string());
        if legacy.high_band_flag {
            keys.push("yaesu_legacy.high_band_flag".to_string());
        }
        if legacy.block.is_some() {
            keys.push("yaesu_legacy.block".to_string());
        }
    }
    if description.kenwood_hh.is_some() {
        keys.push("kenwood_hh.dialect".to_string());
    }
    if description.flex.is_some() {
        keys.push("flex.model".to_string());
        keys.push("flex.scus".to_string());
        if description.flex.as_ref().is_some_and(|f| f.slice != 0) {
            keys.push("flex.slice".to_string());
        }
    }
    if description.vendor.is_some() {
        keys.extend(["vendor.protocol", "vendor.dialect"].map(String::from));
    }
    if let Some(receiver) = &description.receiver {
        keys.extend(["receiver.protocol", "receiver.dialect", "receiver.line"].map(String::from));
        if receiver.address.is_some() {
            keys.push("receiver.address".to_string());
        }
    }
    if description.usb.is_some() {
        keys.extend(["usb.protocol", "usb.ids"].map(String::from));
    }
    if description.hpsdr.is_some() {
        keys.extend(["hpsdr.model", "hpsdr.protocol", "hpsdr.anan10"].map(String::from));
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

    // Tests [@ANCHOR: radioctl_core_rigfile_check_timing]
    #[test]
    fn timing_that_would_delay_or_defeat_an_unkey_is_rejected() {
        expect_rejected(
            &GOOD.replace("timeout_ms = 200", "timeout_ms = 0"),
            "timing.timeout_ms = 0",
        );
        expect_rejected(
            &GOOD.replace("timeout_ms = 200", "timeout_ms = 60000"),
            "timing.timeout_ms = 60000",
        );
        expect_rejected(
            &GOOD.replace("inter_command_ms = 0", "inter_command_ms = 2001"),
            "timing.inter_command_ms = 2001",
        );
        let loaded = parse(&GOOD.replace("inter_command_ms = 0", "inter_command_ms = 2000"))
            .expect("the bound itself loads");
        assert_eq!(
            loaded.description.timing.inter_command,
            Duration::from_millis(2000)
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
    // Tests [@ANCHOR: radioctl_core_rigfile_parse]
    #[test]
    fn a_kenwood_dialect_is_named_known_and_needs_its_fact() {
        let ts890 = GOOD.replace("model_id = 19", "model_id = 19\ndialect = \"ts890\"");
        expect_rejected(&ts890, "no provenance fact covers \"kenwood.dialect\"");
        let fact = "\n[[facts]]\nkey = \"kenwood.dialect\"\nprovenance = { kind = \"manual\", reference = \"x\" }\nverified = { state = \"unverified\" }\n";
        let loaded = parse(&format!("{ts890}{fact}")).expect("loads");
        assert_eq!(
            loaded.description.kenwood.map(|k| k.dialect),
            Some(KenwoodDialect::Ts890)
        );
        expect_rejected(&ts890.replace("ts890", "ts9000"), "unknown kenwood.dialect");
        let classic = parse(&GOOD.replace("model_id = 19", "model_id = 19\ndialect = \"classic\""))
            .expect("loads");
        assert_eq!(
            classic.description.kenwood.map(|k| k.dialect),
            Some(KenwoodDialect::Classic)
        );
    }

    // Tests [@ANCHOR: radioctl_core_rigfile_build_description]
    // Tests [@ANCHOR: radioctl_core_rigfile_expected_keys]
    #[test]
    fn a_yaesu_frequency_width_is_8_or_9_and_a_non_default_needs_its_fact() {
        let yaesu = GOOD
            .replace("family = \"kenwood-ascii\"", "family = \"yaesu-newcat\"")
            .replace("[kenwood]", "[yaesu]")
            .replace("key = \"kenwood.model_id\"", "key = \"yaesu.model_id\"");
        let nine = parse(&yaesu).expect("loads");
        assert_eq!(nine.description.yaesu.map(|y| y.frequency_digits), Some(9));
        let eight = yaesu.replace("model_id = 19", "model_id = 19\nfrequency_digits = 8");
        expect_rejected(
            &eight,
            "no provenance fact covers \"yaesu.frequency_digits\"",
        );
        let fact = "\n[[facts]]\nkey = \"yaesu.frequency_digits\"\nprovenance = { kind = \"manual\", reference = \"x\" }\nverified = { state = \"unverified\" }\n";
        let loaded = parse(&format!("{eight}{fact}")).expect("loads");
        assert_eq!(
            loaded.description.yaesu.map(|y| y.frequency_digits),
            Some(8)
        );
        expect_rejected(
            &yaesu.replace("model_id = 19", "model_id = 19\nfrequency_digits = 10"),
            "yaesu.frequency_digits must be 8 or 9",
        );
    }

    // Tests [@ANCHOR: radioctl_core_civparams_field_len]
    #[test]
    fn civ_frequency_bytes_is_four_five_or_six_and_needs_its_fact() {
        let civ = "[civ]\naddress = 1\ncontroller = 224\nmodel_id = 1\necho = false\n";
        let doc = |extra: &str| {
            format!(
                "schema = 1\ncapabilities = [\"read-frequency\"]\n[rig]\nid = \"x\"\nname = \"X\"\nfamily = \"icom-civ\"\n{civ}{extra}\n[timing]\ntimeout_ms = 1000\nretries = 3\ninter_command_ms = 0\n[defaults]\nfrequency_hz = 7074000\nmode = \"usb\"\n[[modes]]\nmode = \"usb\"\ncode = 1\n"
            )
        };
        let facts = "[[facts]]\nkey = \"rig.*\"\nprovenance = { kind = \"manual\", reference = \"x\" }\nverified = { state = \"unverified\" }\n";
        let more = "[[facts]]\nkey = \"civ.*\"\nprovenance = { kind = \"manual\", reference = \"x\" }\nverified = { state = \"unverified\" }\n[[facts]]\nkey = \"timing.*\"\nprovenance = { kind = \"manual\", reference = \"x\" }\nverified = { state = \"unverified\" }\n[[facts]]\nkey = \"defaults.*\"\nprovenance = { kind = \"manual\", reference = \"x\" }\nverified = { state = \"unverified\" }\n[[facts]]\nkey = \"capability.*\"\nprovenance = { kind = \"manual\", reference = \"x\" }\nverified = { state = \"unverified\" }\n[[facts]]\nkey = \"mode.*\"\nprovenance = { kind = \"manual\", reference = \"x\" }\nverified = { state = \"unverified\" }\n";
        let ok5 = parse(&format!("{}{facts}{more}", doc(""))).expect("loads");
        let civ5 = ok5.description.civ.expect("civ");
        assert_eq!(civ5.field_len(Hz(14_074_000)), 5);
        let ok4 = parse(
            &format!("{}{facts}{more}", doc(""))
                .replace("echo = false", "echo = false\nfrequency_bytes = 4"),
        );
        // Without a `civ.frequency_bytes` fact it is rejected; `civ.*` covers it.
        let civ4 = ok4
            .expect("civ.* covers the key")
            .description
            .civ
            .expect("civ");
        assert_eq!(civ4.field_len(Hz(7_074_000)), 4);
        let six = parse(
            &format!("{}{facts}{more}", doc(""))
                .replace("echo = false", "echo = false\nfrequency_bytes = 6"),
        )
        .expect("loads")
        .description
        .civ
        .expect("civ");
        assert_eq!(six.field_len(Hz(145_000_000)), 5);
        assert_eq!(six.field_len(Hz(10_368_100_000)), 6);
        expect_rejected(
            &format!("{}{facts}{more}", doc(""))
                .replace("echo = false", "echo = false\nfrequency_bytes = 3"),
            "civ.frequency_bytes must be 4, 5 or 6",
        );
    }

    // Tests [@ANCHOR: radioctl_core_rigfile_build_levels]
    // Tests [@ANCHOR: radioctl_core_levelparams_lacks]
    #[test]
    fn levels_absent_names_plain_levels_only() {
        let fact = "\n[[facts]]\nkey = \"levels.*\"\nprovenance = { kind = \"manual\", reference = \"x\" }\nverified = { state = \"unverified\" }\n";
        let head = format!("{GOOD}\n[levels]\nattenuator_db = []\npreamp_steps = 0\n");
        let ok = parse(&format!(
            "{head}absent = [\"af-gain\", \"af-gain\", \"rf-power\"]\n{fact}"
        ))
        .expect("loads");
        let levels = ok.description.levels;
        assert!(levels.lacks(Level::AfGain) && levels.lacks(Level::RfPower));
        assert!(!levels.lacks(Level::MicGain));
        assert_eq!(levels.absent.len(), 2);
        for bad in ["absent = [\"attenuator\"]", "absent = [\"nonsense\"]"] {
            expect_rejected(&format!("{head}{bad}\n{fact}"), "levels.absent names");
        }
    }

    // Tests [@ANCHOR: radioctl_core_rigfile_build_levels]
    // Tests [@ANCHOR: radioctl_core_levelparams_attenuator_step]
    // Tests [@ANCHOR: radioctl_core_levelparams_attenuator_db]
    // Tests [@ANCHOR: radioctl_core_levelparams_max_attenuation]
    #[test]
    fn a_levels_block_is_sane_and_needs_its_facts() {
        let levels = "\n[levels]\nattenuator_db = [6, 12, 18]\npreamp_steps = 2\n";
        let with = format!("{GOOD}{levels}");
        expect_rejected(&with, "no provenance fact covers \"levels.attenuator_db\"");
        let fact = "\n[[facts]]\nkey = \"levels.*\"\nprovenance = { kind = \"manual\", reference = \"x\" }\nverified = { state = \"unverified\" }\n";
        let loaded = parse(&format!("{with}{fact}")).expect("loads");
        let params = loaded.description.levels;
        assert_eq!(params.attenuator_step(0), Some(0));
        assert_eq!(params.attenuator_step(12), Some(2));
        assert_eq!(params.attenuator_step(7), None);
        assert_eq!(params.attenuator_db_of(3), Some(18));
        assert_eq!(params.attenuator_db_of(0), Some(0));
        assert_eq!(params.attenuator_db_of(4), None);
        assert_eq!(params.preamp_steps, 2);
        let any = "attenuator_db = []\nattenuator_any_up_to_db = 31\npreamp_steps = 0";
        let loaded = parse(&format!("{GOOD}\n[levels]\n{any}\n{fact}")).expect("loads");
        let params = loaded.description.levels;
        assert_eq!(params.attenuator_step(31), Some(31));
        assert_eq!(params.attenuator_step(32), None);
        assert_eq!(params.attenuator_db_of(7), Some(7));
        assert_eq!(params.attenuator_db_of(32), None);
        assert_eq!(params.max_attenuation_db(), Some(31));
        assert_eq!(LevelParams::default().max_attenuation_db(), None);
        for bad in [
            "attenuator_db = [12]\nattenuator_any_up_to_db = 31",
            "attenuator_db = [12, 6]",
            "attenuator_db = [0, 6]",
            "attenuator_db = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10]",
            "preamp_steps = 4",
        ] {
            let line = if bad.starts_with("preamp") {
                bad.to_string()
            } else {
                format!("{bad}\npreamp_steps = 0")
            };
            let doc = if bad.starts_with("preamp") {
                format!("{GOOD}\n[levels]\nattenuator_db = []\n{line}\n{fact}")
            } else {
                format!("{GOOD}\n[levels]\n{line}\n{fact}")
            };
            expect_rejected(&doc, "levels.");
        }
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
