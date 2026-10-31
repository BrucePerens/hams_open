// SPDX-License-Identifier: AGPL-3.0-or-later
//! Rig descriptions: the per-model data that drives both a protocol engine and the virtual rig
//! that stands in for the model in tests.
//!
//! This is the in-memory form. The TOML file format, with provenance and `verified` fields, is
//! [`crate::rigfile`], which deserialises into these structs (the shipped files are in `rigs/`).
//! The two built-in descriptions below come from `tools/rig_simulator.py` and the relay's CI-V
//! and TS-2000 bridges (our own code); they are **not** verified against any radio.

use std::time::Duration;

use crate::capability::{Capability, CapabilitySet};
use crate::units::{Hz, Mode};

/// A protocol family.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Family {
    /// Icom CI-V, binary frames.
    IcomCiv,
    /// Kenwood-style ASCII commands ended by `;`.
    KenwoodAscii,
    /// Elecraft K3/KX3/KX2 ASCII commands ended by `;` (Kenwood-like, with its own replies).
    ElecraftAscii,
    /// Yaesu "newcat" ASCII commands ended by `;` (FT-991A, FTDX10, FTDX101).
    YaesuNewcat,
    /// Yaesu legacy five-byte binary CAT blocks (FT-817, FT-857, FT-897).
    YaesuLegacy,
    /// FlexRadio SmartSDR API: line text over TCP (FLEX-6000 and later).
    Flex,
    /// OpenHPSDR (ANAN, Hermes) over UDP, control plane only (Protocol 1 or 2).
    Hpsdr,
}

/// CI-V parameters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CivParams {
    /// The rig's CI-V address.
    pub address: u8,
    /// The controller's address (the software side), conventionally 0xE0.
    pub controller: u8,
    /// The byte the rig returns for the read-ID command.
    pub model_id: u8,
    /// True when the bus echoes every transmitted frame back (single-wire interface).
    pub echo: bool,
}

/// Which command set a Kenwood-style rig speaks.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum KenwoodDialect {
    /// The TS-2000 and TS-590 command set: `IF`, `MD`, `AG0nnn`.
    #[default]
    Classic,
    /// The TS-890S command set: no `IF`, the mode through `OM`, `AGnnn`, split through `TB`,
    /// one-digit `RA` and `PA`.
    Ts890,
}

impl KenwoodDialect {
    // [@ANCHOR: radioctl_core_kenwood_dialect_name]
    /// The name used in rig description files.
    pub fn name(self) -> &'static str {
        match self {
            KenwoodDialect::Classic => "classic",
            KenwoodDialect::Ts890 => "ts890",
        }
    }

    // [@ANCHOR: radioctl_core_kenwood_dialect_from_name]
    /// The dialect with a file-format name.
    pub fn from_name(name: &str) -> Option<KenwoodDialect> {
        match name {
            "classic" => Some(KenwoodDialect::Classic),
            "ts890" => Some(KenwoodDialect::Ts890),
            _ => None,
        }
    }
}

/// Kenwood-style parameters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KenwoodParams {
    /// The number the rig returns for `ID;`.
    pub model_id: u16,
    /// The command set.
    pub dialect: KenwoodDialect,
}

/// Elecraft-style parameters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ElecraftParams {
    /// The number the rig returns for `ID;` (017 on the K3, KX3 and KX2).
    pub model_id: u16,
}

/// Spectrum scope (band scope) parameters of a model, for waveform data the rig sends on its own
/// (Icom CI-V command 0x27).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopeParams {
    /// Independent scopes (receivers): 1, or 2 on a Main/Sub rig.
    pub receivers: u8,
    /// Amplitude samples in one complete sweep ("line").
    pub line_length: u16,
    /// Amplitude samples carried by one waveform frame.
    pub samples_per_frame: u16,
    /// Largest amplitude sample the rig sends (0 is the bottom of the display).
    pub amplitude_max: u16,
    /// The span choices in hertz (full width, as the rig's menu shows them).
    pub spans_hz: Vec<u64>,
}

/// The receive attenuator and preamplifier steps a model has. Empty and zero mean the model has
/// neither, or radioctl knows no command for them: the engines then refuse the levels.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LevelParams {
    /// The attenuation in decibels of each attenuator step, ascending; step `n` (1-based) is the
    /// `n`th entry and step 0 is off. A one-entry list is an attenuator that is simply on or off.
    pub attenuator_db: Vec<u8>,
    /// For a step attenuator that takes any whole number of decibels from 0 up to this (an
    /// OpenHPSDR radio's 0 to 31 dB), the largest; 0 when `attenuator_db` lists the steps.
    pub attenuator_any_up_to_db: u8,
    /// How many preamplifier steps above off the model has (0 for none).
    pub preamp_steps: u8,
}

impl LevelParams {
    // [@ANCHOR: radioctl_core_levelparams_max_attenuation]
    /// The largest attenuation in decibels, `None` when the model has no attenuator.
    pub fn max_attenuation_db(&self) -> Option<u8> {
        if self.attenuator_any_up_to_db > 0 {
            Some(self.attenuator_any_up_to_db)
        } else {
            self.attenuator_db.last().copied()
        }
    }

    // [@ANCHOR: radioctl_core_levelparams_attenuator_step]
    /// The 1-based step whose attenuation is `db`, or 0 for `db == 0` (off); `None` if no step has
    /// that attenuation.
    pub fn attenuator_step(&self, db: u16) -> Option<u8> {
        if db == 0 {
            return Some(0);
        }
        if self.attenuator_any_up_to_db > 0 {
            return u8::try_from(db)
                .ok()
                .filter(|d| *d <= self.attenuator_any_up_to_db);
        }
        let index = self
            .attenuator_db
            .iter()
            .position(|d| u16::from(*d) == db)?;
        u8::try_from(index + 1).ok()
    }

    // [@ANCHOR: radioctl_core_levelparams_attenuator_db]
    /// The attenuation in decibels of 1-based step `step` (0 is off, 0 dB); `None` for a step the
    /// model does not have.
    pub fn attenuator_db_of(&self, step: u8) -> Option<u16> {
        if step == 0 {
            return Some(0);
        }
        if self.attenuator_any_up_to_db > 0 {
            return (step <= self.attenuator_any_up_to_db).then_some(u16::from(step));
        }
        usize::from(step)
            .checked_sub(1)
            .and_then(|index| self.attenuator_db.get(index))
            .map(|d| u16::from(*d))
    }
}

/// Yaesu newcat parameters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct YaesuParams {
    /// The number the rig returns for `ID;` (`ID0670;` is 670).
    pub model_id: u16,
}

/// Yaesu legacy five-byte CAT parameters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct YaesuLegacyParams {
    /// Stop bits on the CAT serial line (2 on the FT-817, FT-857 and FT-897).
    pub stop_bits: u8,
}

/// FlexRadio SmartSDR parameters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlexParams {
    /// The model name the radio reports in `info` and in discovery (`FLEX-6300`).
    pub model: String,
    /// Spectrum capture units (receiver front ends): 1, or 2 on the FLEX-6700.
    pub scus: u8,
}

/// OpenHPSDR network radio parameters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HpsdrParams {
    /// Model name (`ANAN-100`).
    pub model: String,
    /// Wire protocol, 1 or 2.
    pub protocol: u8,
    /// True for the ANAN-10 power calibration constant, false for the ANAN-100.
    pub anan10: bool,
}

/// Command timing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Timing {
    /// How long to wait for a reply.
    pub timeout: Duration,
    /// How many times the actor repeats a command that timed out.
    pub retries: u8,
    /// Minimum gap between commands.
    pub inter_command: Duration,
}

/// Everything radioctl knows about one model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RigDescription {
    /// A stable identifier, e.g. `icom-ic-7300`.
    pub id: String,
    /// A display name.
    pub name: String,
    /// The protocol family.
    pub family: Family,
    /// CI-V parameters, present for [`Family::IcomCiv`].
    pub civ: Option<CivParams>,
    /// Kenwood parameters, present for [`Family::KenwoodAscii`].
    pub kenwood: Option<KenwoodParams>,
    /// Elecraft parameters, present for [`Family::ElecraftAscii`].
    pub elecraft: Option<ElecraftParams>,
    /// Yaesu newcat parameters, present for [`Family::YaesuNewcat`].
    pub yaesu: Option<YaesuParams>,
    /// Yaesu legacy parameters, present for [`Family::YaesuLegacy`].
    pub yaesu_legacy: Option<YaesuLegacyParams>,
    /// FlexRadio parameters, present for [`Family::Flex`].
    pub flex: Option<FlexParams>,
    /// OpenHPSDR parameters, present for [`Family::Hpsdr`].
    pub hpsdr: Option<HpsdrParams>,
    /// Spectrum scope parameters, for the models that have a scope over CAT.
    pub scope: Option<ScopeParams>,
    /// Attenuator and preamplifier steps.
    pub levels: LevelParams,
    /// What the rig can do.
    pub capabilities: CapabilitySet,
    /// Mode to wire code: a CI-V mode byte, or the ASCII byte of a Kenwood `MD` digit.
    pub mode_codes: Vec<(Mode, u8)>,
    /// Command timing.
    pub timing: Timing,
    /// Frequency a virtual rig of this model starts at.
    pub default_frequency: Hz,
    /// Mode a virtual rig of this model starts in.
    pub default_mode: Mode,
}

impl RigDescription {
    // [@ANCHOR: radioctl_core_description_mode_code]
    /// The wire code for a mode, if the model has one.
    pub fn mode_code(&self, mode: Mode) -> Option<u8> {
        self.mode_codes
            .iter()
            .find(|(m, _)| *m == mode)
            .map(|(_, code)| *code)
    }

    // [@ANCHOR: radioctl_core_description_code_mode]
    /// The mode for a wire code, if the model has one.
    pub fn code_mode(&self, code: u8) -> Option<Mode> {
        self.mode_codes
            .iter()
            .find(|(_, c)| *c == code)
            .map(|(mode, _)| *mode)
    }

    // [@ANCHOR: radioctl_core_description_ic7300]
    /// An Icom IC-7300 style CI-V description (address 0x94). Unverified; see the module note.
    pub fn ic7300() -> RigDescription {
        RigDescription {
            id: "icom-ic-7300".into(),
            name: "Icom IC-7300".into(),
            family: Family::IcomCiv,
            civ: Some(CivParams {
                address: 0x94,
                controller: 0xE0,
                model_id: 0x94,
                echo: false,
            }),
            kenwood: None,
            elecraft: None,
            yaesu: None,
            yaesu_legacy: None,
            flex: None,
            hpsdr: None,
            scope: None,
            levels: LevelParams::default(),
            capabilities: CapabilitySet::of(&[
                Capability::ReadFrequency,
                Capability::SetFrequency,
                Capability::ReadMode,
                Capability::SetMode,
                Capability::ReadPtt,
                Capability::SetPtt,
                Capability::ReadSignal,
                Capability::ReadPower,
                Capability::SendCw,
                Capability::Transceive,
            ]),
            mode_codes: vec![
                (Mode::Lsb, 0x00),
                (Mode::Usb, 0x01),
                (Mode::Am, 0x02),
                (Mode::Cw, 0x03),
                (Mode::Rtty, 0x04),
                (Mode::Fm, 0x05),
                (Mode::Wfm, 0x06),
                (Mode::CwR, 0x07),
                (Mode::RttyR, 0x08),
            ],
            timing: Timing {
                timeout: Duration::from_millis(1000),
                retries: 3,
                inter_command: Duration::ZERO,
            },
            default_frequency: Hz(14_074_000),
            default_mode: Mode::Usb,
        }
    }

    // [@ANCHOR: radioctl_core_description_ts2000]
    /// A Kenwood TS-2000 style ASCII description. Unverified; see the module note.
    pub fn ts2000() -> RigDescription {
        RigDescription {
            id: "kenwood-ts-2000".into(),
            name: "Kenwood TS-2000".into(),
            family: Family::KenwoodAscii,
            civ: None,
            kenwood: Some(KenwoodParams {
                model_id: 19,
                dialect: KenwoodDialect::Classic,
            }),
            elecraft: None,
            yaesu: None,
            yaesu_legacy: None,
            flex: None,
            hpsdr: None,
            scope: None,
            levels: LevelParams::default(),
            capabilities: CapabilitySet::of(&[
                Capability::ReadFrequency,
                Capability::SetFrequency,
                Capability::SecondVfo,
                Capability::ReadMode,
                Capability::SetMode,
                Capability::SetPtt,
                Capability::ReadSignal,
                Capability::Transceive,
            ]),
            mode_codes: vec![
                (Mode::Lsb, b'1'),
                (Mode::Usb, b'2'),
                (Mode::Cw, b'3'),
                (Mode::Fm, b'4'),
                (Mode::Am, b'5'),
                (Mode::Rtty, b'6'),
                (Mode::CwR, b'7'),
                (Mode::RttyR, b'9'),
            ],
            timing: Timing {
                timeout: Duration::from_millis(200),
                retries: 3,
                inter_command: Duration::ZERO,
            },
            default_frequency: Hz(14_074_000),
            default_mode: Mode::Usb,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Tests [@ANCHOR: radioctl_core_description_mode_code]
    // Tests [@ANCHOR: radioctl_core_description_code_mode]
    #[test]
    fn mode_tables_round_trip() {
        for desc in [RigDescription::ic7300(), RigDescription::ts2000()] {
            for (mode, code) in desc.mode_codes.clone() {
                assert_eq!(desc.mode_code(mode), Some(code));
                assert_eq!(desc.code_mode(code), Some(mode));
            }
        }
        assert_eq!(RigDescription::ic7300().mode_code(Mode::Usb), Some(0x01));
        assert_eq!(RigDescription::ts2000().mode_code(Mode::Usb), Some(b'2'));
        assert_eq!(RigDescription::ts2000().code_mode(b'8'), None);
    }

    // Tests [@ANCHOR: radioctl_core_kenwood_dialect_name]
    // Tests [@ANCHOR: radioctl_core_kenwood_dialect_from_name]
    #[test]
    fn kenwood_dialect_names_round_trip() {
        for d in [KenwoodDialect::Classic, KenwoodDialect::Ts890] {
            assert_eq!(KenwoodDialect::from_name(d.name()), Some(d));
        }
        assert_eq!(KenwoodDialect::from_name("ts2000"), None);
        assert_eq!(KenwoodDialect::default(), KenwoodDialect::Classic);
    }

    // Tests [@ANCHOR: radioctl_core_description_ic7300]
    #[test]
    fn ic7300_is_civ_at_0x94() {
        let desc = RigDescription::ic7300();
        assert_eq!(desc.family, Family::IcomCiv);
        assert_eq!(desc.civ.as_ref().map(|c| c.address), Some(0x94));
        assert!(desc.kenwood.is_none());
        assert!(desc.capabilities.has(Capability::ReadPtt));
    }

    // Tests [@ANCHOR: radioctl_core_description_ts2000]
    #[test]
    fn ts2000_is_kenwood_without_ptt_readback() {
        let desc = RigDescription::ts2000();
        assert_eq!(desc.family, Family::KenwoodAscii);
        assert_eq!(desc.kenwood.as_ref().map(|k| k.model_id), Some(19));
        assert!(!desc.capabilities.has(Capability::ReadPtt));
        assert!(desc.capabilities.has(Capability::SetPtt));
    }
}
