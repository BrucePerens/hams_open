// SPDX-License-Identifier: AGPL-3.0-or-later
//! The GPIO function map (proposal 4.4): every physical line of a switch board is general
//! purpose I/O first, and a map assigns functions to lines. The map is TOML data, per board
//! model (the pins the board exposes) and per station (the assignment), validated at load.

use crate::error::{StationError, StationErrorKind};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// The direction of a line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    /// A driven output.
    Output,
    /// A high-impedance input.
    Input,
    /// An input with a pull-up.
    InputPullUp,
    /// An input with a pull-down.
    InputPullDown,
}

/// How an output drives.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutputType {
    /// Drives high and low.
    #[default]
    PushPull,
    /// Drives low only.
    OpenDrain,
}

/// A logic level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Level {
    /// Low.
    Low,
    /// High.
    High,
}

/// What a line does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Function {
    /// A power switch channel.
    Power,
    /// A PTT line, timed in firmware.
    Ptt,
    /// An amplifier key line.
    Keyline,
    /// One bit or one way of an antenna or filter selector.
    Selector,
    /// A momentary pulse.
    Momentary,
    /// A band-decoder output.
    Band,
    /// A status input (SWR trip, over-temperature, door, power good).
    Sensor,
    /// A heartbeat indicator.
    Heartbeat,
    /// Unused.
    Spare,
}

/// One line of a map.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Line {
    /// The pin name, as the board model lists it.
    pub pin: String,
    /// Direction.
    pub direction: Direction,
    /// Output type (outputs only).
    #[serde(default)]
    pub output: OutputType,
    /// The level the line takes on release and after load; required for power, ptt and keyline.
    #[serde(default)]
    pub safe: Option<Level>,
    /// Input debounce.
    #[serde(default)]
    pub debounce_ms: u64,
    /// The function.
    pub function: Function,
    /// The switch channel this line belongs to (a selector way names its selector).
    #[serde(default)]
    pub channel: Option<String>,
    /// The way number of a selector line (from 1) or the bit of a band output.
    #[serde(default)]
    pub bit: Option<u32>,
    /// The pulse width of a momentary line.
    #[serde(default)]
    pub pulse_ms: Option<u64>,
    /// The longest key-down of a ptt line.
    #[serde(default)]
    pub max_on_ms: Option<u64>,
}

/// A station's assignment of functions to the lines of one board.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GpioMap {
    /// The board model name.
    pub board: String,
    /// The lines.
    #[serde(default, rename = "line")]
    pub lines: Vec<Line>,
}

/// A board model: the pins it exposes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BoardModel {
    /// The model name.
    pub name: String,
    /// Exposed pins.
    pub pins: Vec<String>,
    /// Pins that are outputs only or inputs only, if any (pin to direction class).
    #[serde(default)]
    pub notes: String,
    /// Whether the board can report its relay or pin state back. False for a board whose status
    /// read is not trusted: the driver keeps its own record and never claims a confirmation.
    #[serde(default = "yes")]
    pub status_readable: bool,
    /// The level of each pin the driver applies when the link opens and in the emergency all-off
    /// (empty: all off).
    #[serde(default)]
    pub safe_state: Vec<bool>,
    /// Optional listing status in `docs/RECOMMENDED_HARDWARE.md`: `recommended`, `acceptable` or
    /// `not-recommended`. Informational only; it never changes how a board is driven.
    #[serde(default)]
    pub recommendation: Option<String>,
}

fn yes() -> bool {
    true
}

/// The board models known to a build, parsed from TOML.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BoardCatalog {
    boards: BTreeMap<String, BoardModel>,
}

#[derive(Deserialize)]
struct CatalogFile {
    #[serde(default, rename = "board")]
    boards: Vec<BoardModel>,
}

/// The built-in board models, as data.
pub const BUILTIN_BOARDS: &str = include_str!("../../../data/station/boards.toml");

impl BoardCatalog {
    // [@ANCHOR: radioctl_station_catalog_from_toml]
    /// Parses a catalog.
    pub fn from_toml(text: &str) -> Result<BoardCatalog, StationError> {
        let file: CatalogFile = toml::from_str(text)
            .map_err(|e| StationError::new(StationErrorKind::Config, format!("boards: {e}")))?;
        let mut boards = BTreeMap::new();
        for b in file.boards {
            let unique: BTreeSet<&String> = b.pins.iter().collect();
            if unique.len() != b.pins.len() || b.pins.is_empty() {
                return Err(StationError::new(
                    StationErrorKind::Config,
                    format!("board {} lists no pins or a pin twice", b.name),
                ));
            }
            if boards.insert(b.name.clone(), b).is_some() {
                return Err(StationError::new(
                    StationErrorKind::Config,
                    "a board is listed twice",
                ));
            }
        }
        Ok(BoardCatalog { boards })
    }

    // [@ANCHOR: radioctl_station_catalog_builtin]
    /// The built-in catalog.
    pub fn builtin() -> BoardCatalog {
        BoardCatalog::from_toml(BUILTIN_BOARDS).unwrap_or_default()
    }

    // [@ANCHOR: radioctl_station_catalog_get]
    /// A board by name.
    pub fn get(&self, name: &str) -> Option<&BoardModel> {
        self.boards.get(name)
    }
}

impl GpioMap {
    // [@ANCHOR: radioctl_station_map_from_toml]
    /// Parses a map and validates it against `catalog`.
    pub fn from_toml(text: &str, catalog: &BoardCatalog) -> Result<GpioMap, StationError> {
        let map: GpioMap = toml::from_str(text)
            .map_err(|e| StationError::new(StationErrorKind::Config, format!("gpio map: {e}")))?;
        map.validate(catalog)?;
        Ok(map)
    }

    // [@ANCHOR: radioctl_station_map_validate]
    /// No pin has two functions or appears twice, no pin the board does not expose, no power,
    /// ptt or keyline line without a safe state, and each function's own fields are present and
    /// agree with the direction.
    pub fn validate(&self, catalog: &BoardCatalog) -> Result<(), StationError> {
        let bad = |m: String| {
            Err(StationError::new(
                StationErrorKind::Config,
                format!("gpio map: {m}"),
            ))
        };
        let Some(board) = catalog.get(&self.board) else {
            return bad(format!("unknown board model {}", self.board));
        };
        let mut used = BTreeSet::new();
        let mut ways = BTreeSet::new();
        for l in &self.lines {
            if !board.pins.contains(&l.pin) {
                return bad(format!(
                    "board {} does not expose pin {}",
                    board.name, l.pin
                ));
            }
            if !used.insert(&l.pin) {
                return bad(format!("pin {} has two functions", l.pin));
            }
            let output = l.direction == Direction::Output;
            let needs_output = matches!(
                l.function,
                Function::Power
                    | Function::Ptt
                    | Function::Keyline
                    | Function::Selector
                    | Function::Momentary
                    | Function::Band
                    | Function::Heartbeat
            );
            if needs_output && !output {
                return bad(format!("pin {}: {:?} must be an output", l.pin, l.function));
            }
            if l.function == Function::Sensor && output {
                return bad(format!("pin {}: a sensor must be an input", l.pin));
            }
            if matches!(
                l.function,
                Function::Power | Function::Ptt | Function::Keyline
            ) && l.safe.is_none()
            {
                return bad(format!(
                    "pin {}: a {:?} line needs a defined safe state",
                    l.pin, l.function
                ));
            }
            if !output && l.safe.is_some() {
                return bad(format!("pin {}: an input has no safe level", l.pin));
            }
            if matches!(
                l.function,
                Function::Power
                    | Function::Ptt
                    | Function::Keyline
                    | Function::Selector
                    | Function::Momentary
                    | Function::Sensor
            ) && l.channel.as_deref().unwrap_or("").is_empty()
            {
                return bad(format!("pin {}: {:?} needs a channel", l.pin, l.function));
            }
            if l.function == Function::Momentary && l.pulse_ms.unwrap_or(0) == 0 {
                return bad(format!("pin {}: a momentary line needs pulse_ms", l.pin));
            }
            if l.function == Function::Ptt && l.max_on_ms.unwrap_or(0) == 0 {
                return bad(format!("pin {}: a ptt line needs max_on_ms", l.pin));
            }
            if matches!(l.function, Function::Selector | Function::Band) {
                let Some(bit) = l.bit.filter(|b| *b >= 1) else {
                    return bad(format!("pin {}: needs bit from 1", l.pin));
                };
                if !ways.insert((l.channel.clone(), l.function as u8, bit)) {
                    return bad(format!(
                        "pin {}: bit {bit} of the channel is assigned twice",
                        l.pin
                    ));
                }
            }
        }
        Ok(())
    }

    // [@ANCHOR: radioctl_station_map_safe_levels]
    /// The pins with a defined safe level, for the firmware or driver to apply on release.
    pub fn safe_levels(&self) -> BTreeMap<String, Level> {
        self.lines
            .iter()
            .filter_map(|l| l.safe.map(|s| (l.pin.clone(), s)))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MAP: &str = r#"
board = "fx2-lcsoft-mini"
[[line]]
pin = "PB0"
direction = "output"
safe = "low"
function = "power"
channel = "amp_pwr"
[[line]]
pin = "PB1"
direction = "output"
function = "selector"
channel = "ant"
bit = 1
[[line]]
pin = "PB2"
direction = "output"
function = "selector"
channel = "ant"
bit = 2
[[line]]
pin = "PB3"
direction = "output"
output = "open_drain"
safe = "high"
function = "ptt"
channel = "ptt1"
max_on_ms = 180000
[[line]]
pin = "PD0"
direction = "input_pull_up"
debounce_ms = 20
function = "sensor"
channel = "swr_trip"
[[line]]
pin = "PD1"
direction = "output"
function = "momentary"
channel = "btn"
pulse_ms = 300
"#;

    // Tests [@ANCHOR: radioctl_station_catalog_from_toml]
    // Tests [@ANCHOR: radioctl_station_catalog_builtin]
    // Tests [@ANCHOR: radioctl_station_catalog_get]
    #[test]
    fn the_builtin_catalog_lists_the_boards() {
        let c = BoardCatalog::builtin();
        assert_eq!(c.get("fx2-lcsoft-mini").unwrap().pins.len(), 24);
        assert_eq!(c.get("sainsmart-16ch-modbus").unwrap().pins.len(), 16);
        assert!(c.get("nope").is_none());
        assert!(BoardCatalog::from_toml("[[board]]\nname=\"x\"\npins=[\"a\",\"a\"]").is_err());
        assert!(BoardCatalog::from_toml("[[board]]\nname=\"x\"\npins=[]").is_err());
    }

    // Tests [@ANCHOR: radioctl_station_map_from_toml]
    // Tests [@ANCHOR: radioctl_station_map_safe_levels]
    #[test]
    fn a_good_map_loads_and_reports_safe_levels() {
        let map = GpioMap::from_toml(MAP, &BoardCatalog::builtin()).unwrap();
        assert_eq!(map.lines.len(), 6);
        let safe = map.safe_levels();
        assert_eq!(safe.get("PB0"), Some(&Level::Low));
        assert_eq!(safe.get("PB3"), Some(&Level::High));
        assert_eq!(safe.len(), 2);
    }

    // Tests [@ANCHOR: radioctl_station_map_validate]
    #[test]
    fn bad_maps_are_refused() {
        let c = BoardCatalog::builtin();
        let dup = format!(
            "{MAP}\n[[line]]\npin = \"PB0\"\ndirection = \"output\"\nfunction = \"spare\"\n"
        );
        let cases = [
            (dup, "two functions"),
            (
                MAP.replace("pin = \"PB2\"", "pin = \"PC9\""),
                "does not expose",
            ),
            (MAP.replacen("safe = \"low\"\n", "", 1), "safe state"),
            (
                MAP.replace("board = \"fx2-lcsoft-mini\"", "board = \"mystery\""),
                "unknown board",
            ),
            (MAP.replace("bit = 2", "bit = 1"), "assigned twice"),
            (
                MAP.replace("max_on_ms = 180000", "max_on_ms = 0"),
                "max_on_ms",
            ),
            (MAP.replace("pulse_ms = 300", "pulse_ms = 0"), "pulse_ms"),
            (
                MAP.replacen("direction = \"input_pull_up\"", "direction = \"output\"", 1),
                "sensor must be an input",
            ),
            (
                MAP.replacen("direction = \"output\"", "direction = \"input\"", 1),
                "must be an output",
            ),
            (MAP.replace("channel = \"btn\"\n", ""), "needs a channel"),
        ];
        for (text, want) in cases {
            let err = GpioMap::from_toml(&text, &c).unwrap_err();
            assert!(err.detail.contains(want), "{want}: {}", err.detail);
        }
    }
}
