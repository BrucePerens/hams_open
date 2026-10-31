// SPDX-License-Identifier: AGPL-3.0-or-later
//! The names and bit positions of the `rigctld` text protocol: modes, VFOs, levels and functions.
//!
//! The names and the bit positions come from the `rigctl` man page and from what Hamlib 4.6.2's
//! `rigctld` and `rigctl` print (`\dump_state`, `--dump-caps`); the tests/rigctl_oracle.rs
//! differential test reads the bits back through Hamlib's own client. Nothing here is copied from
//! Hamlib source.

use radioctl_core::description::{Family, RigDescription};
use radioctl_core::units::{Level, Mode, Vfo};

/// One mode in Hamlib's vocabulary.
struct ModeEntry {
    mode: Mode,
    name: &'static str,
    bit: u32,
    nominal_width_hz: u32,
}

const MODES: [ModeEntry; 11] = [
    ModeEntry {
        mode: Mode::Am,
        name: "AM",
        bit: 0,
        nominal_width_hz: 6000,
    },
    ModeEntry {
        mode: Mode::Cw,
        name: "CW",
        bit: 1,
        nominal_width_hz: 500,
    },
    ModeEntry {
        mode: Mode::Usb,
        name: "USB",
        bit: 2,
        nominal_width_hz: 2400,
    },
    ModeEntry {
        mode: Mode::Lsb,
        name: "LSB",
        bit: 3,
        nominal_width_hz: 2400,
    },
    ModeEntry {
        mode: Mode::Rtty,
        name: "RTTY",
        bit: 4,
        nominal_width_hz: 500,
    },
    ModeEntry {
        mode: Mode::Fm,
        name: "FM",
        bit: 5,
        nominal_width_hz: 15000,
    },
    ModeEntry {
        mode: Mode::Wfm,
        name: "WFM",
        bit: 6,
        nominal_width_hz: 230_000,
    },
    ModeEntry {
        mode: Mode::CwR,
        name: "CWR",
        bit: 7,
        nominal_width_hz: 500,
    },
    ModeEntry {
        mode: Mode::RttyR,
        name: "RTTYR",
        bit: 8,
        nominal_width_hz: 500,
    },
    ModeEntry {
        mode: Mode::DataR,
        name: "PKTLSB",
        bit: 10,
        nominal_width_hz: 3000,
    },
    ModeEntry {
        mode: Mode::Data,
        name: "PKTUSB",
        bit: 11,
        nominal_width_hz: 3000,
    },
];

// [@ANCHOR: radioctld_names_mode_from_hamlib]
/// The mode a Hamlib mode name stands for (case-insensitive).
pub fn mode_from_hamlib(name: &str) -> Option<Mode> {
    MODES
        .iter()
        .find(|e| e.name.eq_ignore_ascii_case(name))
        .map(|e| e.mode)
}

// [@ANCHOR: radioctld_names_mode_to_hamlib]
/// Hamlib's name for a mode.
pub fn mode_to_hamlib(mode: Mode) -> &'static str {
    MODES
        .iter()
        .find(|e| e.mode == mode)
        .map_or("NONE", |e| e.name)
}

// [@ANCHOR: radioctld_names_mode_mask]
/// The bit mask of a set of modes, as `\dump_state` writes it.
pub fn mode_mask(modes: &[Mode]) -> u64 {
    modes
        .iter()
        .filter_map(|m| MODES.iter().find(|e| e.mode == *m))
        .fold(0u64, |mask, e| mask | (1u64 << e.bit))
}

// [@ANCHOR: radioctld_names_mode_bit]
/// The single mask bit of one mode.
pub fn mode_bit(mode: Mode) -> u64 {
    mode_mask(&[mode])
}

// [@ANCHOR: radioctld_names_nominal_width]
/// A typical receive bandwidth for the mode, reported when the rig cannot tell its own.
pub fn nominal_width(mode: Mode) -> u32 {
    MODES
        .iter()
        .find(|e| e.mode == mode)
        .map_or(0, |e| e.nominal_width_hz)
}

/// A VFO as a client names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VfoSel {
    /// `currVFO`: the VFO selected by `V`.
    Current,
    /// `VFOA`, `Main` or `MainA`.
    A,
    /// `VFOB` or `MainB`.
    B,
    /// `RX`: the receive VFO.
    Rx,
    /// `TX`: the transmit VFO (VFO B while split is on).
    Tx,
    /// A VFO Hamlib knows that radioctl rigs do not have (`VFOC`, `Sub`, `MEM`, ...).
    Absent,
}

const ABSENT_VFOS: [&str; 7] = ["VFOC", "SubA", "SubB", "Sub", "MEM", "Mem", "None"];

// [@ANCHOR: radioctld_names_parse_vfo]
/// The VFO a token names, if it is a VFO name at all (case-insensitive).
pub fn parse_vfo(token: &str) -> Option<VfoSel> {
    let is = |name: &str| token.eq_ignore_ascii_case(name);
    if is("currVFO") {
        Some(VfoSel::Current)
    } else if is("VFOA") || is("Main") || is("MainA") {
        Some(VfoSel::A)
    } else if is("VFOB") || is("MainB") {
        Some(VfoSel::B)
    } else if is("RX") {
        Some(VfoSel::Rx)
    } else if is("TX") {
        Some(VfoSel::Tx)
    } else if ABSENT_VFOS.iter().any(|n| is(n)) {
        Some(VfoSel::Absent)
    } else {
        None
    }
}

// [@ANCHOR: radioctld_names_vfo_name]
/// Hamlib's name for a VFO.
pub fn vfo_name(vfo: Vfo) -> &'static str {
    match vfo {
        Vfo::A => "VFOA",
        Vfo::B => "VFOB",
    }
}

/// Where a level's value comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LevelSource {
    /// A numeric level of the rig, scaled by the rig family's range.
    Radio(Level),
    /// The SWR meter, tenths of a unit on the rig side.
    Swr,
    /// The raw S-meter reading.
    RawStr,
}

/// A level radioctld can serve.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LevelInfo {
    /// Hamlib's name.
    pub name: &'static str,
    /// Bit in the `has_get_level` / `has_set_level` masks and index in `level_gran`.
    pub bit: u32,
    /// Where the value comes from.
    pub source: LevelSource,
    /// True when the client sees a 0..1 fraction; false when it sees the rig's own integer.
    pub fraction: bool,
}

const LEVELS: [LevelInfo; 8] = [
    LevelInfo {
        name: "AF",
        bit: 3,
        source: LevelSource::Radio(Level::AfGain),
        fraction: true,
    },
    LevelInfo {
        name: "RF",
        bit: 4,
        source: LevelSource::Radio(Level::RfGain),
        fraction: true,
    },
    LevelInfo {
        name: "RFPOWER",
        bit: 12,
        source: LevelSource::Radio(Level::RfPower),
        fraction: true,
    },
    LevelInfo {
        name: "MICGAIN",
        bit: 13,
        source: LevelSource::Radio(Level::MicGain),
        fraction: true,
    },
    LevelInfo {
        name: "KEYSPD",
        bit: 14,
        source: LevelSource::Radio(Level::KeyerSpeed),
        fraction: false,
    },
    LevelInfo {
        name: "RAWSTR",
        bit: 26,
        source: LevelSource::RawStr,
        fraction: false,
    },
    LevelInfo {
        name: "SWR",
        bit: 28,
        source: LevelSource::Swr,
        fraction: true,
    },
    LevelInfo {
        name: "MONITOR_GAIN",
        bit: 37,
        source: LevelSource::Radio(Level::MonitorLevel),
        fraction: true,
    },
];

/// Level names Hamlib has that radioctld does not serve: asking for one is "not available".
const OTHER_LEVELS: [&str; 24] = [
    "PREAMP",
    "ATT",
    "VOXDELAY",
    "SQL",
    "IF",
    "APF",
    "NR",
    "PBT_IN",
    "PBT_OUT",
    "CWPITCH",
    "NOTCHF",
    "COMP",
    "AGC",
    "BKINDL",
    "BAL",
    "METER",
    "VOXGAIN",
    "ANTIVOX",
    "ALC",
    "STRENGTH",
    "RFPOWER_METER",
    "RFPOWER_METER_WATTS",
    "NB",
    "TEMP_METER",
];

/// A level name lookup.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LevelLookup {
    /// A level radioctld can serve (if the rig has it).
    Known(LevelInfo),
    /// A level Hamlib has that radioctld does not serve.
    Unserved,
    /// Not a level name at all.
    Unknown,
}

// [@ANCHOR: radioctld_names_level_lookup]
/// Finds a level by its Hamlib name (case-insensitive).
pub fn level_lookup(name: &str) -> LevelLookup {
    if let Some(info) = LEVELS.iter().find(|l| l.name.eq_ignore_ascii_case(name)) {
        LevelLookup::Known(*info)
    } else if OTHER_LEVELS.iter().any(|n| n.eq_ignore_ascii_case(name)) {
        LevelLookup::Unserved
    } else {
        LevelLookup::Unknown
    }
}

// [@ANCHOR: radioctld_names_level_range]
/// The rig's own range for a level, when radioctld knows it for the rig's family. These are the
/// ranges the engines enforce (the Elecraft one is read from the engine; the CI-V and Kenwood
/// tables are private to their engines, so they are repeated here and the test
/// `level_ranges_match_what_each_engine_accepts` pins them to the engines' own limits).
pub fn level_range(description: &RigDescription, level: Level) -> Option<(u16, u16)> {
    match description.family {
        Family::ElecraftAscii => {
            let wire = radioctl_protocols::elecraft::level_wire(level);
            Some((wire.min, wire.max))
        }
        Family::IcomCiv => match level {
            Level::KeyerSpeed => None,
            _ => Some((0, 255)),
        },
        Family::KenwoodAscii => Some(match level {
            Level::AfGain | Level::RfGain => (0, 255),
            Level::MicGain => (0, 100),
            Level::MonitorLevel => (0, 9),
            Level::KeyerSpeed => (10, 60),
            Level::RfPower => (5, 100),
        }),
        Family::YaesuNewcat => match level {
            Level::AfGain | Level::RfGain => Some((0, 255)),
            Level::MicGain => Some((0, 100)),
            Level::KeyerSpeed => Some((4, 60)),
            Level::RfPower => Some((5, 100)),
            Level::MonitorLevel => None,
        },
        // The legacy five-byte engine has no level commands, so no level has a range.
        Family::YaesuLegacy => None,
        // The OpenHPSDR control plane has no level commands.
        Family::Hpsdr => None,
        // SmartSDR levels are percentages (0-100) in the engine.
        Family::Flex => match level {
            Level::AfGain | Level::MicGain | Level::RfPower => Some((0, 100)),
            Level::RfGain | Level::KeyerSpeed | Level::MonitorLevel => None,
        },
    }
}

// [@ANCHOR: radioctld_names_served_levels]
/// The levels this rig can serve: its capabilities, and a known range for numeric levels.
pub fn served_levels(description: &RigDescription) -> Vec<LevelInfo> {
    use radioctl_core::capability::Capability;
    LEVELS
        .iter()
        .filter(|info| match info.source {
            LevelSource::Radio(level) => {
                description.capabilities.has(Capability::Levels)
                    && level_range(description, level).is_some()
            }
            LevelSource::Swr => description.capabilities.has(Capability::ReadSwr),
            LevelSource::RawStr => description.capabilities.has(Capability::ReadSignal),
        })
        .copied()
        .collect()
}

/// A function (on/off switch) in Hamlib's vocabulary that radioctld serves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FuncKind {
    /// RIT on or off.
    Rit,
    /// XIT on or off.
    Xit,
}

/// Bit positions and names of the functions radioctld serves.
const FUNCS: [(&str, u32, FuncKind); 2] = [("RIT", 24, FuncKind::Rit), ("XIT", 31, FuncKind::Xit)];

/// Function names Hamlib has that radioctld does not serve.
const OTHER_FUNCS: [&str; 30] = [
    "FAGC", "NB", "COMP", "VOX", "TONE", "TSQL", "SBKIN", "FBKIN", "ANF", "NR", "AIP", "APF",
    "MON", "MN", "RF", "ARO", "LOCK", "MUTE", "VSC", "REV", "SQL", "ABM", "BC", "MBC", "AFC",
    "SATMODE", "SCOPE", "RESUME", "TBURST", "TUNER",
];

/// A function name lookup.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FuncLookup {
    /// A function radioctld serves (if the rig has it).
    Known(FuncKind),
    /// A function Hamlib has that radioctld does not serve.
    Unserved,
    /// Not a function name at all.
    Unknown,
}

// [@ANCHOR: radioctld_names_func_lookup]
/// Finds a function by its Hamlib name (case-insensitive).
pub fn func_lookup(name: &str) -> FuncLookup {
    if let Some((_, _, kind)) = FUNCS.iter().find(|(n, _, _)| n.eq_ignore_ascii_case(name)) {
        FuncLookup::Known(*kind)
    } else if OTHER_FUNCS.iter().any(|n| n.eq_ignore_ascii_case(name)) {
        FuncLookup::Unserved
    } else {
        FuncLookup::Unknown
    }
}

// [@ANCHOR: radioctld_names_func_mask]
/// The `has_get_func` / `has_set_func` mask for a rig that has RIT and XIT or neither.
pub fn func_mask(rit_xit: bool) -> u64 {
    if rit_xit {
        FUNCS.iter().fold(0u64, |m, (_, bit, _)| m | (1u64 << bit))
    } else {
        0
    }
}

// [@ANCHOR: radioctld_names_level_mask]
/// The `has_get_level` / `has_set_level` mask of a list of levels; `settable_only` leaves out
/// the read-only meters.
pub fn level_mask(levels: &[LevelInfo], settable_only: bool) -> u64 {
    levels
        .iter()
        .filter(|l| !settable_only || matches!(l.source, LevelSource::Radio(_)))
        .fold(0u64, |m, l| m | (1u64 << l.bit))
}

#[cfg(test)]
mod tests {
    use super::*;
    use radioctl_core::capability::{Capability, CapabilitySet};

    // Tests [@ANCHOR: radioctld_names_mode_from_hamlib]
    // Tests [@ANCHOR: radioctld_names_mode_to_hamlib]
    #[test]
    fn every_mode_round_trips_through_its_hamlib_name() {
        for entry in &MODES {
            assert_eq!(mode_from_hamlib(entry.name), Some(entry.mode));
            assert_eq!(mode_to_hamlib(entry.mode), entry.name);
        }
        assert_eq!(mode_from_hamlib("usb"), Some(Mode::Usb));
        assert_eq!(mode_from_hamlib("SSB"), None);
    }

    // Tests [@ANCHOR: radioctld_names_mode_mask]
    // Tests [@ANCHOR: radioctld_names_mode_bit]
    #[test]
    fn mode_masks_use_hamlibs_low_bits() {
        assert_eq!(mode_bit(Mode::Usb), 0x4);
        assert_eq!(mode_bit(Mode::Lsb), 0x8);
        assert_eq!(mode_mask(&[Mode::Am, Mode::Cw, Mode::Fm]), 0x23);
        assert_eq!(mode_mask(&[]), 0);
    }

    // Tests [@ANCHOR: radioctld_names_nominal_width]
    #[test]
    fn nominal_widths_are_typical() {
        assert_eq!(nominal_width(Mode::Usb), 2400);
        assert_eq!(nominal_width(Mode::Cw), 500);
        assert!(nominal_width(Mode::Fm) > nominal_width(Mode::Am));
    }

    // Tests [@ANCHOR: radioctld_names_parse_vfo]
    #[test]
    fn vfo_tokens_are_recognised_and_other_words_are_not() {
        assert_eq!(parse_vfo("VFOA"), Some(VfoSel::A));
        assert_eq!(parse_vfo("vfob"), Some(VfoSel::B));
        assert_eq!(parse_vfo("currVFO"), Some(VfoSel::Current));
        assert_eq!(parse_vfo("TX"), Some(VfoSel::Tx));
        assert_eq!(parse_vfo("Main"), Some(VfoSel::A));
        assert_eq!(parse_vfo("MEM"), Some(VfoSel::Absent));
        assert_eq!(parse_vfo("14074000"), None);
        assert_eq!(parse_vfo("USB"), None);
    }

    // Tests [@ANCHOR: radioctld_names_vfo_name]
    #[test]
    fn vfo_names() {
        assert_eq!(vfo_name(Vfo::A), "VFOA");
        assert_eq!(vfo_name(Vfo::B), "VFOB");
    }

    // Tests [@ANCHOR: radioctld_names_level_lookup]
    #[test]
    fn level_names_are_known_unserved_or_unknown() {
        assert!(matches!(level_lookup("keyspd"), LevelLookup::Known(i) if i.bit == 14));
        assert_eq!(level_lookup("STRENGTH"), LevelLookup::Unserved);
        assert_eq!(level_lookup("NOSUCH"), LevelLookup::Unknown);
    }

    // Tests [@ANCHOR: radioctld_names_level_range]
    // Tests [@ANCHOR: radioctld_names_served_levels]
    // Tests [@ANCHOR: radioctld_names_level_mask]
    #[test]
    fn only_a_rig_with_ranges_and_the_capability_serves_levels() {
        let ic = RigDescription::ic7300();
        let served = served_levels(&ic);
        assert!(served.iter().all(|l| l.source == LevelSource::RawStr));
        assert_eq!(level_range(&ic, Level::AfGain), Some((0, 255)));
        assert_eq!(level_range(&ic, Level::KeyerSpeed), None);
        let mut kx = RigDescription::ic7300();
        kx.family = Family::ElecraftAscii;
        kx.capabilities = CapabilitySet::of(&[
            Capability::Levels,
            Capability::ReadSwr,
            Capability::ReadSignal,
        ]);
        assert_eq!(level_range(&kx, Level::KeyerSpeed), Some((8, 50)));
        let served = served_levels(&kx);
        assert_eq!(served.len(), LEVELS.len());
        let get = level_mask(&served, false);
        let set = level_mask(&served, true);
        assert_ne!(get, set);
        assert_eq!(get & (1 << 28), 1 << 28);
        assert_eq!(set & (1 << 28), 0);
        assert_eq!(set & (1 << 14), 1 << 14);
    }

    // Tests [@ANCHOR: radioctld_names_func_lookup]
    // Tests [@ANCHOR: radioctld_names_func_mask]
    #[test]
    fn functions_are_known_unserved_or_unknown() {
        assert_eq!(func_lookup("rit"), FuncLookup::Known(FuncKind::Rit));
        assert_eq!(func_lookup("XIT"), FuncLookup::Known(FuncKind::Xit));
        assert_eq!(func_lookup("NB"), FuncLookup::Unserved);
        assert_eq!(func_lookup("NOSUCH"), FuncLookup::Unknown);
        assert_eq!(func_mask(false), 0);
        assert_eq!(func_mask(true), (1 << 24) | (1 << 31));
    }
}
