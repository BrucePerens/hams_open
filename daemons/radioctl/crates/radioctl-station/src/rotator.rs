// SPDX-License-Identifier: AGPL-3.0-or-later
//! Rotators (antenna azimuth and elevation) through the same description and transport machinery
//! as the rigs: a TOML description per rotator, a sans-IO protocol engine, any byte transport
//! (a serial port, a TCP link that reconnects, `radioctl_core::net`), and a bounded, serialised
//! command handle.
//!
//! Two protocols are covered, written from the behaviour the relay's rotator code and Hamlib's
//! `rotctld` text protocol exhibit (the `rotctld(1)` manual page: `P az el`, `p`, `S`, `K`, replies
//! of values one per line or `RPRT n`) and the Yaesu GS-232 ASCII family (`Waaa eee` to move,
//! `C2` to read, answered `+0aaa+eee`):
//!
//! - [`RotctldEngine`]: a Hamlib `rotctld`, which fronts every Hamlib rotator backend.
//! - [`Gs232Engine`]: a Yaesu/Kenpro GS-232A/B controller on a serial port or serial server.
//!
//! Safety: a rotator is not a transmit path, but a position outside the description's travel is
//! refused (never clamped silently), every wait is bounded, nothing is queued across a link loss
//! (a command on a down link fails at once), and [`Rotator::stop`] is never refused by anything but
//! the link. Nothing restores a position after a reconnect: the rotator keeps its own target.

use crate::error::{StationError, StationErrorKind};
use radioctl_core::serial::LinkStatus;
use radioctl_core::transport::Transport;
use serde::Deserialize;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::{watch, Mutex};
use tokio::time::{sleep, timeout, Instant};

/// The rotator descriptions shipped with radioctl.
pub const BUILTIN_ROTATORS: &str = include_str!("../../../data/station/rotators.toml");

/// Which wire protocol the rotator speaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RotatorProtocol {
    /// Hamlib's `rotctld` text protocol.
    Rotctld,
    /// Yaesu GS-232A/B ASCII.
    Gs232,
    /// Easycomm I: set-only `AZaaa.a ELeee.e` lines.
    Easycomm1,
    /// Easycomm II: `AZ`/`EL` set and read, `SA SE` stop.
    Easycomm2,
    /// Easycomm III: the Easycomm II commands radioctl uses (the velocity, status and
    /// configuration extensions are not used).
    Easycomm3,
    /// SPID Rot2Prog / MD-01/02 (13-byte command frames, 12-byte replies).
    #[serde(rename = "rot2prog")]
    Rot2Prog,
    /// Hy-Gain DCU-1/DCU-2/DCU-3 (`AP1nnn;` target, `AM1;` start); azimuth only, no read.
    Dcu,
}

/// A park position.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
pub struct ParkPosition {
    /// Azimuth, degrees.
    pub az: f32,
    /// Elevation, degrees.
    pub el: f32,
}

/// What a rotator is and can do.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct RotatorDescription {
    /// A short unique name.
    pub id: String,
    /// A display name.
    pub name: String,
    /// The wire protocol.
    pub protocol: RotatorProtocol,
    /// Lowest azimuth, degrees.
    pub az_min: f32,
    /// Highest azimuth, degrees (above 360 for an overlap).
    pub az_max: f32,
    /// Lowest elevation, degrees.
    pub el_min: f32,
    /// Highest elevation, degrees.
    pub el_max: f32,
    /// Whether there is an elevation axis.
    pub has_elevation: bool,
    /// Typical slew rate, degrees per second (informational; the virtual rotator uses it).
    pub slew_deg_per_s: f32,
    /// Where `park` goes, if the rotator has one.
    #[serde(default)]
    pub park: Option<ParkPosition>,
    /// Longest wait for a reply.
    pub reply_timeout_ms: u64,
    /// Pulses per degree the controller counts in (Rot2Prog: 1 to 10; a reply carries the
    /// controller's own value, which is used to read it).
    #[serde(default)]
    pub pulses_per_degree: Option<u8>,
    /// Whether any of this has been checked on the rotator.
    #[serde(default)]
    pub verified: bool,
}

#[derive(Deserialize)]
struct RotatorFile {
    #[serde(default)]
    rotators: Vec<RotatorDescription>,
}

// [@ANCHOR: radioctl_station_rot_err]
fn rot_err(kind: StationErrorKind, m: impl Into<String>) -> StationError {
    StationError::new(kind, m)
}

impl RotatorDescription {
    // [@ANCHOR: radioctl_station_rot_desc_validate]
    /// Checks the description on its own: ordered finite limits, sane travel, a park position
    /// inside the travel, a GS-232 description inside what that protocol can carry.
    pub fn validate(&self) -> Result<(), StationError> {
        let bad = |m: String| {
            Err(rot_err(
                StationErrorKind::Config,
                format!("rotator {}: {m}", self.id),
            ))
        };
        let finite = [
            self.az_min,
            self.az_max,
            self.el_min,
            self.el_max,
            self.slew_deg_per_s,
        ];
        if self.id.is_empty() || finite.iter().any(|v| !v.is_finite()) {
            return bad("needs a name and finite numbers".into());
        }
        if self.az_min >= self.az_max || self.az_max - self.az_min > 720.0 {
            return bad("azimuth travel must be between 0 and 720 degrees and ordered".into());
        }
        if self.el_min > self.el_max || self.el_min < -90.0 || self.el_max > 180.0 {
            return bad("elevation limits are out of range".into());
        }
        if !self.has_elevation && (self.el_min != 0.0 || self.el_max != 0.0) {
            return bad("an azimuth-only rotator has elevation 0 to 0".into());
        }
        if self.slew_deg_per_s <= 0.0
            || self.reply_timeout_ms == 0
            || self.reply_timeout_ms > 60_000
        {
            return bad("slew rate and reply timeout must be positive and sane".into());
        }
        if self.protocol == RotatorProtocol::Gs232
            && (self.az_min < 0.0
                || self.az_max > 450.0
                || self.el_min < 0.0
                || self.el_max > 180.0)
        {
            return bad("GS-232 carries azimuth 0 to 450 and elevation 0 to 180".into());
        }
        if self.protocol == RotatorProtocol::Dcu
            && (self.has_elevation || self.az_min < 0.0 || self.az_max > 360.0)
        {
            return bad("a DCU is azimuth only, 0 to 360 degrees".into());
        }
        if self.protocol == RotatorProtocol::Rot2Prog {
            let Some(ppd) = self.pulses_per_degree.filter(|p| (1..=10).contains(p)) else {
                return bad("Rot2Prog needs pulses_per_degree from 1 to 10".into());
            };
            if self.az_min < 0.0
                || (360.0 + self.az_max) * f32::from(ppd) > 9999.0
                || (360.0 + self.el_max) * f32::from(ppd) > 9999.0
            {
                return bad("Rot2Prog carries four digits of 360 plus the angle".into());
            }
        }
        if let Some(p) = self.park {
            if !self.contains(p.az, p.el) {
                return bad("the park position is outside the travel".into());
            }
        }
        Ok(())
    }

    // [@ANCHOR: radioctl_station_rot_desc_contains]
    /// Whether a position is inside the travel.
    pub fn contains(&self, az: f32, el: f32) -> bool {
        az.is_finite()
            && el.is_finite()
            && (self.az_min..=self.az_max).contains(&az)
            && (self.el_min..=self.el_max).contains(&el)
    }

    // [@ANCHOR: radioctl_station_rot_desc_parse]
    /// Parses a file of `[[rotators]]` and validates every one.
    pub fn parse_all(text: &str) -> Result<Vec<RotatorDescription>, StationError> {
        let file: RotatorFile = toml::from_str(text)
            .map_err(|e| rot_err(StationErrorKind::Config, format!("rotators: {e}")))?;
        let mut seen = std::collections::BTreeSet::new();
        for r in &file.rotators {
            r.validate()?;
            if !seen.insert(r.id.clone()) {
                return Err(rot_err(
                    StationErrorKind::Config,
                    format!("rotator {} is defined twice", r.id),
                ));
            }
        }
        Ok(file.rotators)
    }

    // [@ANCHOR: radioctl_station_rot_desc_builtin]
    /// The shipped descriptions.
    pub fn builtin() -> Vec<RotatorDescription> {
        RotatorDescription::parse_all(BUILTIN_ROTATORS).unwrap_or_default()
    }

    // [@ANCHOR: radioctl_station_rot_desc_engine]
    /// The protocol engine for this description.
    pub fn engine(&self) -> Result<Box<dyn RotatorEngine>, StationError> {
        self.validate()?;
        Ok(match self.protocol {
            RotatorProtocol::Rotctld => Box::new(RotctldEngine::new(self.clone())),
            RotatorProtocol::Gs232 => Box::new(Gs232Engine::new(self.clone())),
            RotatorProtocol::Easycomm1
            | RotatorProtocol::Easycomm2
            | RotatorProtocol::Easycomm3 => Box::new(EasycommEngine::new(self.clone())),
            RotatorProtocol::Rot2Prog => Box::new(Rot2ProgEngine::new(self.clone())),
            RotatorProtocol::Dcu => Box::new(DcuEngine::new(self.clone())),
        })
    }
}

/// A rotator command.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RotOp {
    /// Move to a position.
    SetPosition {
        /// Azimuth, degrees.
        az: f32,
        /// Elevation, degrees.
        el: f32,
    },
    /// Read the position.
    GetPosition,
    /// Stop moving.
    Stop,
    /// Go to the park position.
    Park,
}

/// What reply a command expects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RotExpect {
    /// Nothing comes back.
    Nothing,
    /// A status line.
    Status,
    /// A position.
    Position,
}

/// A command encoded for the wire.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Encoded {
    /// The bytes to write.
    pub bytes: Vec<u8>,
    /// The reply to wait for.
    pub expect: RotExpect,
}

/// A parsed reply.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RotReply {
    /// The command was accepted.
    Done,
    /// The position.
    Position {
        /// Azimuth, degrees.
        az: f32,
        /// Elevation, degrees.
        el: f32,
    },
}

/// A sans-IO rotator protocol.
pub trait RotatorEngine: Send + Sync {
    /// The description this engine serves.
    fn description(&self) -> &RotatorDescription;
    /// Encodes a command. A position outside the travel is a `Refused` error, never clamped.
    fn encode(&self, op: &RotOp) -> Result<Encoded, StationError>;
    /// Parses a reply from the start of `buf`: `Ok(None)` needs more bytes, otherwise the reply
    /// and how many bytes it used. A rotator that reports an error is an `Err`.
    fn parse(
        &self,
        expect: RotExpect,
        buf: &[u8],
    ) -> Result<Option<(RotReply, usize)>, StationError>;
}

// [@ANCHOR: radioctl_station_rot_resolve]
/// The position a command means: the explicit one (checked against the travel), or the park
/// position.
fn target(desc: &RotatorDescription, op: &RotOp) -> Result<Option<(f32, f32)>, StationError> {
    match *op {
        RotOp::SetPosition { az, el } => {
            let el = if desc.has_elevation { el } else { 0.0 };
            if !desc.contains(az, el) {
                return Err(rot_err(
                    StationErrorKind::Refused,
                    format!(
                        "{az:.1}, {el:.1} is outside the travel of {} (azimuth {} to {}, elevation {} to {})",
                        desc.id, desc.az_min, desc.az_max, desc.el_min, desc.el_max
                    ),
                ));
            }
            Ok(Some((az, el)))
        }
        RotOp::Park => desc.park.map(|p| Some((p.az, p.el))).ok_or_else(|| {
            rot_err(
                StationErrorKind::Refused,
                format!("{} has no park position", desc.id),
            )
        }),
        RotOp::GetPosition | RotOp::Stop => Ok(None),
    }
}

// [@ANCHOR: radioctl_station_rot_line]
/// The first line in `buf` (any leading CR or LF skipped): its text and the bytes used including
/// the terminator, or `None` if the line is not complete yet.
fn line(buf: &[u8]) -> Option<(String, usize)> {
    let start = buf
        .iter()
        .position(|b| *b != b'\r' && *b != b'\n')
        .unwrap_or(buf.len());
    let rest = buf.get(start..)?;
    let end = rest.iter().position(|b| *b == b'\n' || *b == b'\r')?;
    let text = String::from_utf8_lossy(rest.get(..end)?).trim().to_string();
    Some((text, start + end + 1))
}

/// Hamlib's `rotctld` protocol.
pub struct RotctldEngine {
    desc: RotatorDescription,
}

impl RotctldEngine {
    // [@ANCHOR: radioctl_station_rotctld_new]
    /// An engine for `desc`.
    pub fn new(desc: RotatorDescription) -> RotctldEngine {
        RotctldEngine { desc }
    }
}

// [@ANCHOR: radioctl_station_rotctld_rprt]
/// The meaning of a Hamlib `RPRT` code.
fn rprt_text(code: i32) -> &'static str {
    match code {
        -1 => "invalid parameter",
        -2 => "invalid configuration",
        -4 => "function not implemented",
        -5 => "communication timed out",
        -6 => "I/O error",
        -8 => "protocol error",
        -9 => "command rejected by the rotator",
        -11 => "function not available",
        -17 => "value out of the rotator's domain",
        _ => "error",
    }
}

impl RotatorEngine for RotctldEngine {
    // [@ANCHOR: radioctl_station_rotctld_description]
    fn description(&self) -> &RotatorDescription {
        &self.desc
    }

    // [@ANCHOR: radioctl_station_rotctld_encode]
    fn encode(&self, op: &RotOp) -> Result<Encoded, StationError> {
        let pos = target(&self.desc, op)?;
        Ok(match (op, pos) {
            (RotOp::SetPosition { .. } | RotOp::Park, Some((az, el))) => Encoded {
                bytes: format!("P {az:.2} {el:.2}\n").into_bytes(),
                expect: RotExpect::Status,
            },
            (RotOp::GetPosition, _) => Encoded {
                bytes: b"p\n".to_vec(),
                expect: RotExpect::Position,
            },
            _ => Encoded {
                bytes: b"S\n".to_vec(),
                expect: RotExpect::Status,
            },
        })
    }

    // [@ANCHOR: radioctl_station_rotctld_parse]
    fn parse(
        &self,
        expect: RotExpect,
        buf: &[u8],
    ) -> Result<Option<(RotReply, usize)>, StationError> {
        let status = |text: &str| -> Result<(), StationError> {
            let code: i32 = text
                .strip_prefix("RPRT ")
                .and_then(|c| c.trim().parse().ok())
                .ok_or_else(|| {
                    rot_err(
                        StationErrorKind::Driver,
                        format!("unexpected reply \"{text}\""),
                    )
                })?;
            if code == 0 {
                Ok(())
            } else {
                Err(rot_err(
                    StationErrorKind::Driver,
                    format!("the rotator reported {code}: {}", rprt_text(code)),
                ))
            }
        };
        match expect {
            RotExpect::Nothing => Ok(Some((RotReply::Done, 0))),
            RotExpect::Status => {
                let Some((text, used)) = line(buf) else {
                    return Ok(None);
                };
                status(&text)?;
                Ok(Some((RotReply::Done, used)))
            }
            RotExpect::Position => {
                let Some((first, used1)) = line(buf) else {
                    return Ok(None);
                };
                if first.starts_with("RPRT") {
                    status(&first)?;
                    return Err(rot_err(
                        StationErrorKind::Driver,
                        "the rotator answered a read with a status",
                    ));
                }
                let Some((second, used2)) = line(buf.get(used1..).unwrap_or(&[])) else {
                    return Ok(None);
                };
                let number = |t: &str| {
                    t.parse::<f32>()
                        .ok()
                        .filter(|v| v.is_finite())
                        .ok_or_else(|| {
                            rot_err(
                                StationErrorKind::Driver,
                                format!("\"{t}\" is not a position"),
                            )
                        })
                };
                Ok(Some((
                    RotReply::Position {
                        az: number(&first)?,
                        el: number(&second)?,
                    },
                    used1 + used2,
                )))
            }
        }
    }
}

/// The Yaesu GS-232A/B ASCII protocol.
pub struct Gs232Engine {
    desc: RotatorDescription,
}

impl Gs232Engine {
    // [@ANCHOR: radioctl_station_gs232_new]
    /// An engine for `desc`.
    pub fn new(desc: RotatorDescription) -> Gs232Engine {
        Gs232Engine { desc }
    }
}

// [@ANCHOR: radioctl_station_gs232_parse_position]
/// A GS-232 position line: `+0aaa+eee` (signed fields, the leading zero of the azimuth
/// optional), or `AZ=aaa EL=eee`.
fn gs232_position(text: &str) -> Option<(f32, f32)> {
    let digits = |s: &str| -> Option<f32> {
        let s = s.trim().trim_start_matches('+');
        (!s.is_empty() && s.len() <= 4 && s.chars().all(|c| c.is_ascii_digit()))
            .then(|| s.parse::<f32>().ok())?
    };
    if let Some(rest) = text.strip_prefix('+') {
        let (az, el) = rest.split_once('+')?;
        return Some((digits(az)?, digits(el)?));
    }
    let rest = text.strip_prefix("AZ=")?;
    let (az, el) = rest.split_once("EL=")?;
    Some((digits(az)?, digits(el)?))
}

impl RotatorEngine for Gs232Engine {
    // [@ANCHOR: radioctl_station_gs232_description]
    fn description(&self) -> &RotatorDescription {
        &self.desc
    }

    // [@ANCHOR: radioctl_station_gs232_encode]
    fn encode(&self, op: &RotOp) -> Result<Encoded, StationError> {
        let pos = target(&self.desc, op)?;
        Ok(match (op, pos) {
            (RotOp::SetPosition { .. } | RotOp::Park, Some((az, el))) => Encoded {
                bytes: format!("W{:03} {:03}\r", az.round() as i32, el.round() as i32).into_bytes(),
                expect: RotExpect::Nothing,
            },
            (RotOp::GetPosition, _) => Encoded {
                bytes: b"C2\r".to_vec(),
                expect: RotExpect::Position,
            },
            _ => Encoded {
                bytes: b"S\r".to_vec(),
                expect: RotExpect::Nothing,
            },
        })
    }

    // [@ANCHOR: radioctl_station_gs232_parse]
    fn parse(
        &self,
        expect: RotExpect,
        buf: &[u8],
    ) -> Result<Option<(RotReply, usize)>, StationError> {
        match expect {
            RotExpect::Nothing => Ok(Some((RotReply::Done, 0))),
            RotExpect::Status => Err(rot_err(
                StationErrorKind::Driver,
                "GS-232 has no status replies",
            )),
            RotExpect::Position => {
                let Some((text, used)) = line(buf) else {
                    return Ok(None);
                };
                let (az, el) = gs232_position(&text).ok_or_else(|| {
                    rot_err(
                        StationErrorKind::Driver,
                        format!("unexpected reply \"{text}\""),
                    )
                })?;
                Ok(Some((RotReply::Position { az, el }, used)))
            }
        }
    }
}

/// The Easycomm I, II and III ASCII protocols (Chris Jackson G7UPN; Easycomm III by Alexander
/// Schultze), from the specification the AMSAT WiSP programs ship and Hamlib carries:
/// `AZaaa.a ELeee.e` moves, a bare `AZ EL` reads (the controller echoes `AZaaa.a ELeee.e`),
/// `SA SE` stops both axes. Easycomm I has no read.
pub struct EasycommEngine {
    desc: RotatorDescription,
}

impl EasycommEngine {
    // [@ANCHOR: radioctl_station_easycomm_new]
    /// An engine for `desc` (its protocol picks the Easycomm version).
    pub fn new(desc: RotatorDescription) -> EasycommEngine {
        EasycommEngine { desc }
    }
}

// [@ANCHOR: radioctl_station_easycomm_parse_position]
/// Looks for `AZ<number>` and `EL<number>` words (separated by space, CR or LF, each complete
/// only once its separator has arrived) in `buf`. `Ok(None)` while a wanted word is missing or
/// unfinished; the bytes used end after the separator of the later wanted word. An `AL` alarm
/// word is an error. Words that are neither (an echo, a version) are skipped.
fn easycomm_position(buf: &[u8], want_el: bool) -> Result<Option<(RotReply, usize)>, StationError> {
    let (mut az, mut el) = (None, None);
    let mut start = 0;
    for (i, b) in buf.iter().enumerate() {
        if !matches!(*b, b' ' | b'\r' | b'\n') {
            continue;
        }
        let word = String::from_utf8_lossy(buf.get(start..i).unwrap_or(&[])).to_string();
        start = i + 1;
        let number = |t: &str| {
            t.parse::<f32>()
                .ok()
                .filter(|v| v.is_finite())
                .ok_or_else(|| {
                    rot_err(
                        StationErrorKind::Driver,
                        format!("\"{word}\" is not a position"),
                    )
                })
        };
        if let Some(t) = word.strip_prefix("AZ") {
            az = Some(number(t)?);
        } else if let Some(t) = word.strip_prefix("EL") {
            el = Some(number(t)?);
        } else if let Some(t) = word.strip_prefix("AL") {
            return Err(rot_err(
                StationErrorKind::Driver,
                format!("the controller raised alarm \"{t}\""),
            ));
        }
        if let (Some(a), true) = (az, !want_el || el.is_some()) {
            return Ok(Some((
                RotReply::Position {
                    az: a,
                    el: el.unwrap_or(0.0),
                },
                i + 1,
            )));
        }
    }
    Ok(None)
}

impl RotatorEngine for EasycommEngine {
    // [@ANCHOR: radioctl_station_easycomm_description]
    fn description(&self) -> &RotatorDescription {
        &self.desc
    }

    // [@ANCHOR: radioctl_station_easycomm_encode]
    fn encode(&self, op: &RotOp) -> Result<Encoded, StationError> {
        let pos = target(&self.desc, op)?;
        let el = self.desc.has_elevation;
        let v1 = self.desc.protocol == RotatorProtocol::Easycomm1;
        Ok(match (op, pos) {
            (RotOp::SetPosition { .. } | RotOp::Park, Some((az, elv))) => Encoded {
                bytes: match (el, v1) {
                    (true, true) => format!("AZ{az:.1} EL{elv:.1} UP000 XXX DN000 XXX\n"),
                    (true, false) => format!("AZ{az:.1} EL{elv:.1}\n"),
                    (false, _) => format!("AZ{az:.1}\n"),
                }
                .into_bytes(),
                expect: RotExpect::Nothing,
            },
            (RotOp::GetPosition, _) if v1 => {
                return Err(rot_err(
                    StationErrorKind::Refused,
                    format!(
                        "{} speaks Easycomm I, which cannot report a position",
                        self.desc.id
                    ),
                ))
            }
            (RotOp::GetPosition, _) => Encoded {
                bytes: if el {
                    b"AZ EL\n".to_vec()
                } else {
                    b"AZ\n".to_vec()
                },
                expect: RotExpect::Position,
            },
            _ => Encoded {
                bytes: if el {
                    b"SA SE\n".to_vec()
                } else {
                    b"SA\n".to_vec()
                },
                expect: RotExpect::Nothing,
            },
        })
    }

    // [@ANCHOR: radioctl_station_easycomm_parse]
    fn parse(
        &self,
        expect: RotExpect,
        buf: &[u8],
    ) -> Result<Option<(RotReply, usize)>, StationError> {
        match expect {
            RotExpect::Nothing => Ok(Some((RotReply::Done, 0))),
            RotExpect::Status => Err(rot_err(
                StationErrorKind::Driver,
                "Easycomm has no status replies",
            )),
            RotExpect::Position => easycomm_position(buf, self.desc.has_elevation),
        }
    }
}

/// The SPID Rot2Prog / MD-01/02 binary protocol, from SPID's "ROT2PROG Protocol" sheet: every
/// command is 13 bytes `W` + 10 payload bytes + command + `0x20` (set: two groups of four ASCII
/// digits of `(360 + angle) * pulses` each followed by the pulses-per-degree byte, command `0x2F`;
/// status `0x1F`; stop `0x0F`), and status and stop answer 12 bytes `W` + four digit bytes + PH +
/// four digit bytes + PV + `0x20`. A digit byte is read as ASCII (`0x30` to `0x39`) or as a plain
/// 0 to 9, because the sheet and Hamlib disagree on which a controller sends.
pub struct Rot2ProgEngine {
    desc: RotatorDescription,
}

impl Rot2ProgEngine {
    // [@ANCHOR: radioctl_station_rot2prog_new]
    /// An engine for `desc`.
    pub fn new(desc: RotatorDescription) -> Rot2ProgEngine {
        Rot2ProgEngine { desc }
    }
}

/// The Rot2Prog command byte for a status read.
const R2P_STATUS: u8 = 0x1F;
/// The Rot2Prog command byte for stop.
const R2P_STOP: u8 = 0x0F;
/// The Rot2Prog command byte for set.
const R2P_SET: u8 = 0x2F;

// [@ANCHOR: radioctl_station_rot2prog_frame]
/// A 13-byte Rot2Prog command frame. `angles` is `(azimuth, elevation, pulses per degree)` for a
/// set, which fills the four digits and the pulses byte of each axis.
fn r2p_frame(command: u8, angles: Option<(f32, f32, u8)>) -> Vec<u8> {
    let mut f = vec![0u8; 13];
    f[0] = 0x57;
    if let Some((az, el, ppd)) = angles {
        let digits =
            |angle: f32| format!("{:04}", ((360.0 + angle) * f32::from(ppd)).round() as u32);
        f[1..5].copy_from_slice(digits(az).as_bytes());
        f[5] = ppd;
        f[6..10].copy_from_slice(digits(el).as_bytes());
        f[10] = ppd;
    }
    f[11] = command;
    f[12] = 0x20;
    f
}

// [@ANCHOR: radioctl_station_rot2prog_reading]
/// One axis of a reply: four digit bytes and the pulses-per-degree byte to degrees.
fn r2p_angle(digits: &[u8], ppd: u8) -> Result<f32, StationError> {
    if ppd == 0 {
        return Err(rot_err(
            StationErrorKind::Driver,
            "the controller reported 0 pulses per degree",
        ));
    }
    let mut value = 0u32;
    for b in digits {
        let d = if (0x30..=0x39).contains(b) {
            b - 0x30
        } else {
            *b
        };
        if d > 9 {
            return Err(rot_err(
                StationErrorKind::Driver,
                format!("byte {b:#04x} is not a digit"),
            ));
        }
        value = value * 10 + u32::from(d);
    }
    Ok(value as f32 / f32::from(ppd) - 360.0)
}

impl RotatorEngine for Rot2ProgEngine {
    // [@ANCHOR: radioctl_station_rot2prog_description]
    fn description(&self) -> &RotatorDescription {
        &self.desc
    }

    // [@ANCHOR: radioctl_station_rot2prog_encode]
    fn encode(&self, op: &RotOp) -> Result<Encoded, StationError> {
        let pos = target(&self.desc, op)?;
        let ppd = self.desc.pulses_per_degree.unwrap_or(10);
        Ok(match (op, pos) {
            (RotOp::SetPosition { .. } | RotOp::Park, Some((az, el))) => Encoded {
                bytes: r2p_frame(R2P_SET, Some((az, el, ppd))),
                expect: RotExpect::Nothing,
            },
            (RotOp::GetPosition, _) => Encoded {
                bytes: r2p_frame(R2P_STATUS, None),
                expect: RotExpect::Position,
            },
            _ => Encoded {
                bytes: r2p_frame(R2P_STOP, None),
                expect: RotExpect::Position,
            },
        })
    }

    // [@ANCHOR: radioctl_station_rot2prog_parse]
    fn parse(
        &self,
        expect: RotExpect,
        buf: &[u8],
    ) -> Result<Option<(RotReply, usize)>, StationError> {
        match expect {
            RotExpect::Nothing => Ok(Some((RotReply::Done, 0))),
            RotExpect::Status => Err(rot_err(
                StationErrorKind::Driver,
                "Rot2Prog has no status replies",
            )),
            RotExpect::Position => {
                let Some(start) = buf.iter().position(|b| *b == 0x57) else {
                    return Ok(None);
                };
                let Some(f) = buf.get(start..start + 12) else {
                    return Ok(None);
                };
                if f[11] != 0x20 {
                    return Err(rot_err(
                        StationErrorKind::Driver,
                        "a Rot2Prog reply does not end with 0x20",
                    ));
                }
                let az = r2p_angle(&f[1..5], f[5])?;
                let el = if self.desc.has_elevation {
                    r2p_angle(&f[6..10], f[10])?
                } else {
                    0.0
                };
                Ok(Some((RotReply::Position { az, el }, start + 12)))
            }
        }
    }
}

/// The Hy-Gain DCU-1/DCU-2/DCU-3 ASCII protocol, from Hy-Gain's DCU-3 manual ("Command Set":
/// `AP1nnn;` sets the target bearing, three digits 000 to 359, and `AM1;` starts the rotation to
/// it; 4800 baud 8N1; the controller sends nothing back). Both commands go out in one write.
/// Stop is `AS1;`, which Hamlib's DCU-1 backend uses and the DCU-3 manual does not list: a bench
/// has to confirm it. There is no position read, so `GetPosition` is refused.
pub struct DcuEngine {
    desc: RotatorDescription,
}

impl DcuEngine {
    // [@ANCHOR: radioctl_station_dcu_new]
    /// An engine for `desc`.
    pub fn new(desc: RotatorDescription) -> DcuEngine {
        DcuEngine { desc }
    }
}

impl RotatorEngine for DcuEngine {
    // [@ANCHOR: radioctl_station_dcu_description]
    fn description(&self) -> &RotatorDescription {
        &self.desc
    }

    // [@ANCHOR: radioctl_station_dcu_encode]
    fn encode(&self, op: &RotOp) -> Result<Encoded, StationError> {
        let pos = target(&self.desc, op)?;
        match (op, pos) {
            (RotOp::SetPosition { .. } | RotOp::Park, Some((az, _))) => {
                // 360 is the same bearing as 000, and the controller takes no more than 359.
                let bearing = az.round() as i32 % 360;
                Ok(Encoded {
                    bytes: format!("AP1{bearing:03};AM1;").into_bytes(),
                    expect: RotExpect::Nothing,
                })
            }
            (RotOp::GetPosition, _) => Err(rot_err(
                StationErrorKind::Refused,
                format!("{} (a DCU) cannot report its bearing", self.desc.id),
            )),
            _ => Ok(Encoded {
                bytes: b"AS1;".to_vec(),
                expect: RotExpect::Nothing,
            }),
        }
    }

    // [@ANCHOR: radioctl_station_dcu_parse]
    fn parse(
        &self,
        expect: RotExpect,
        _buf: &[u8],
    ) -> Result<Option<(RotReply, usize)>, StationError> {
        match expect {
            RotExpect::Nothing => Ok(Some((RotReply::Done, 0))),
            _ => Err(rot_err(
                StationErrorKind::Driver,
                "a DCU sends nothing back",
            )),
        }
    }
}

/// A rotator on a transport: commands are serialised, every wait is bounded, and a down link
/// fails at once.
pub struct Rotator {
    engine: Box<dyn RotatorEngine>,
    io: Mutex<Box<dyn Transport>>,
    link: Option<watch::Receiver<LinkStatus>>,
}

impl Rotator {
    // [@ANCHOR: radioctl_station_rotator_new]
    /// A rotator over `transport`. Pass the transport's link status (`NetLink::watch`) so a lost
    /// link is reported as such and no command is written into a dead stream.
    pub fn new(
        engine: Box<dyn RotatorEngine>,
        transport: impl Transport,
        link: Option<watch::Receiver<LinkStatus>>,
    ) -> Rotator {
        Rotator {
            engine,
            io: Mutex::new(Box::new(transport)),
            link,
        }
    }

    // [@ANCHOR: radioctl_station_rotator_description]
    /// The description.
    pub fn description(&self) -> &RotatorDescription {
        self.engine.description()
    }

    // [@ANCHOR: radioctl_station_rotator_set_position]
    /// Moves to a position inside the travel. Returns when the rotator has accepted the command,
    /// not when it arrives (see [`Rotator::wait_for`]).
    pub async fn set_position(&self, az: f32, el: f32) -> Result<(), StationError> {
        self.transact(RotOp::SetPosition { az, el })
            .await
            .map(|_| ())
    }

    // [@ANCHOR: radioctl_station_rotator_position]
    /// Reads the position.
    pub async fn position(&self) -> Result<(f32, f32), StationError> {
        match self.transact(RotOp::GetPosition).await? {
            RotReply::Position { az, el } => Ok((az, el)),
            RotReply::Done => Err(rot_err(
                StationErrorKind::Driver,
                "no position in the reply",
            )),
        }
    }

    // [@ANCHOR: radioctl_station_rotator_stop]
    /// Stops the rotator. No interlock applies; only a dead link or a rotator that does not
    /// answer can make it fail.
    pub async fn stop(&self) -> Result<(), StationError> {
        self.transact(RotOp::Stop).await.map(|_| ())
    }

    // [@ANCHOR: radioctl_station_rotator_park]
    /// Goes to the park position, if the description has one.
    pub async fn park(&self) -> Result<(), StationError> {
        self.transact(RotOp::Park).await.map(|_| ())
    }

    // [@ANCHOR: radioctl_station_rotator_wait_for]
    /// Polls the position until it is within `tolerance` degrees of `az`/`el`, up to `limit`.
    /// The error says where the rotator was last seen.
    pub async fn wait_for(
        &self,
        az: f32,
        el: f32,
        tolerance: f32,
        limit: Duration,
    ) -> Result<(), StationError> {
        let deadline = Instant::now() + limit;
        loop {
            let seen = match self.position().await {
                Ok((a, e)) => {
                    let el_ok = !self.description().has_elevation || (e - el).abs() <= tolerance;
                    if (a - az).abs() <= tolerance && el_ok {
                        return Ok(());
                    }
                    format!("{a:.1}, {e:.1}")
                }
                Err(e) if e.kind == StationErrorKind::Driver && e.detail.contains("link") => {
                    return Err(e)
                }
                Err(e) => e.detail,
            };
            if Instant::now() >= deadline {
                return Err(rot_err(
                    StationErrorKind::Timeout,
                    format!("the rotator did not reach {az:.1}, {el:.1} (last seen {seen})"),
                ));
            }
            sleep(Duration::from_millis(50)).await;
        }
    }

    // [@ANCHOR: radioctl_station_rotator_link_ok]
    /// Whether the transport's link, when it reports one, is up.
    fn link_ok(&self) -> bool {
        self.link
            .as_ref()
            .is_none_or(|l| matches!(*l.borrow(), LinkStatus::Connected { .. }))
    }

    // [@ANCHOR: radioctl_station_rotator_transact]
    /// One command and its reply, bounded by the description's reply timeout. Stale bytes are
    /// drained first, and after a failure, so a late reply cannot be taken for the next one.
    async fn transact(&self, op: RotOp) -> Result<RotReply, StationError> {
        let encoded = self.engine.encode(&op)?;
        if !self.link_ok() {
            return Err(rot_err(
                StationErrorKind::Driver,
                "the link to the rotator is down",
            ));
        }
        let limit = Duration::from_millis(self.engine.description().reply_timeout_ms);
        let mut io = timeout(limit, self.io.lock()).await.map_err(|_| {
            rot_err(
                StationErrorKind::Timeout,
                "the rotator is busy with another command",
            )
        })?;
        drain(&mut io).await;
        let result = timeout(limit, async {
            io.write_all(&encoded.bytes).await.map_err(|e| {
                rot_err(
                    StationErrorKind::Driver,
                    format!("write to the rotator: {e}"),
                )
            })?;
            io.flush().await.map_err(|e| {
                rot_err(
                    StationErrorKind::Driver,
                    format!("write to the rotator: {e}"),
                )
            })?;
            let mut buf = Vec::new();
            let mut chunk = [0u8; 256];
            loop {
                if let Some((reply, _)) = self.engine.parse(encoded.expect, &buf)? {
                    return Ok(reply);
                }
                if buf.len() > 4096 {
                    return Err(rot_err(StationErrorKind::Driver, "the reply has no end"));
                }
                let n = io.read(&mut chunk).await.map_err(|e| {
                    rot_err(
                        StationErrorKind::Driver,
                        format!("read from the rotator: {e}"),
                    )
                })?;
                if n == 0 {
                    return Err(rot_err(
                        StationErrorKind::Driver,
                        "the link to the rotator closed",
                    ));
                }
                buf.extend_from_slice(chunk.get(..n).unwrap_or(&[]));
            }
        })
        .await;
        match result {
            Ok(Ok(reply)) => Ok(reply),
            Ok(Err(e)) => {
                drain(&mut io).await;
                Err(e)
            }
            Err(_) => {
                drain(&mut io).await;
                Err(rot_err(
                    StationErrorKind::Timeout,
                    "the rotator did not answer",
                ))
            }
        }
    }
}

// [@ANCHOR: radioctl_station_rotator_drain]
/// Reads and discards whatever is waiting, until the line has been quiet for 10 ms.
async fn drain(io: &mut Box<dyn Transport>) {
    let mut chunk = [0u8; 256];
    for _ in 0..64 {
        match timeout(Duration::from_millis(10), io.read(&mut chunk)).await {
            Ok(Ok(n)) if n > 0 => {}
            _ => return,
        }
    }
}

/// A rotator shared between tasks.
pub type SharedRotator = Arc<Rotator>;

#[cfg(test)]
mod tests {
    use super::*;

    fn desc(id: &str) -> RotatorDescription {
        RotatorDescription::builtin()
            .into_iter()
            .find(|d| d.id == id)
            .unwrap()
    }

    // Tests [@ANCHOR: radioctl_station_rot_desc_parse]
    // Tests [@ANCHOR: radioctl_station_rot_desc_builtin]
    // Tests [@ANCHOR: radioctl_station_rot_desc_validate]
    // Tests [@ANCHOR: radioctl_station_rot_desc_engine]
    // Tests [@ANCHOR: radioctl_station_rot_err]
    #[test]
    fn the_shipped_descriptions_are_valid_and_bad_ones_are_refused() {
        let all = RotatorDescription::builtin();
        assert_eq!(all.len(), 8);
        assert!(all.iter().all(|d| d.validate().is_ok() && !d.verified));
        for d in &all {
            assert_eq!(d.engine().unwrap().description().id, d.id);
        }
        let g = desc("yaesu-g-5500-gs232b");
        let mut cases = Vec::new();
        for (what, mutate) in [
            (
                "ordered",
                Box::new(|d: &mut RotatorDescription| d.az_min = 500.0)
                    as Box<dyn Fn(&mut RotatorDescription)>,
            ),
            ("travel", Box::new(|d| d.az_max = 900.0)),
            ("elevation limits", Box::new(|d| d.el_min = -200.0)),
            ("GS-232 carries", Box::new(|d| d.az_max = 500.0)),
            (
                "park",
                Box::new(|d| d.park = Some(ParkPosition { az: 999.0, el: 0.0 })),
            ),
            ("finite", Box::new(|d| d.az_max = f32::NAN)),
            ("positive", Box::new(|d| d.reply_timeout_ms = 0)),
            ("azimuth-only", { Box::new(|d| d.has_elevation = false) }),
        ] {
            let mut d = g.clone();
            mutate(&mut d);
            cases.push((what, d));
        }
        for (what, d) in cases {
            let e = d.validate().expect_err(what);
            assert!(e.detail.contains(what), "{what}: {}", e.detail);
        }
        assert!(RotatorDescription::parse_all("[[rotators]]\nid='x'").is_err());
        let twice = format!("{BUILTIN_ROTATORS}\n{BUILTIN_ROTATORS}");
        assert!(RotatorDescription::parse_all(&twice)
            .unwrap_err()
            .detail
            .contains("twice"));
    }

    // Tests [@ANCHOR: radioctl_station_rot_desc_contains]
    // Tests [@ANCHOR: radioctl_station_rot_resolve]
    #[test]
    fn positions_outside_the_travel_are_refused_not_clamped() {
        let d = desc("yaesu-g-5500-gs232b");
        assert!(d.contains(450.0, 180.0) && d.contains(0.0, 0.0));
        assert!(!d.contains(450.1, 0.0) && !d.contains(-0.1, 0.0));
        assert!(!d.contains(f32::NAN, 0.0) && !d.contains(10.0, f32::INFINITY));
        let e = Gs232Engine::new(d.clone());
        let err = e
            .encode(&RotOp::SetPosition { az: 500.0, el: 0.0 })
            .unwrap_err();
        assert_eq!(err.kind, StationErrorKind::Refused);
        // An azimuth-only rotator ignores a requested elevation (it has none).
        let az_only = Gs232Engine::new(desc("yaesu-g-450-gs232a"));
        assert_eq!(
            az_only
                .encode(&RotOp::SetPosition { az: 90.0, el: 45.0 })
                .unwrap()
                .bytes,
            b"W090 000\r"
        );
        // No park position, no park.
        let none = RotctldEngine::new(desc("generic-rotctld"));
        assert!(none.encode(&RotOp::Park).is_err());
    }

    // Tests [@ANCHOR: radioctl_station_rotctld_new]
    // Tests [@ANCHOR: radioctl_station_rotctld_description]
    // Tests [@ANCHOR: radioctl_station_rotctld_encode]
    // Tests [@ANCHOR: radioctl_station_rotctld_parse]
    // Tests [@ANCHOR: radioctl_station_rotctld_rprt]
    // Tests [@ANCHOR: radioctl_station_rot_line]
    #[test]
    fn rotctld_encodes_and_parses() {
        let e = RotctldEngine::new(desc("generic-rotctld"));
        assert_eq!(e.description().id, "generic-rotctld");
        assert_eq!(
            e.encode(&RotOp::SetPosition {
                az: 180.0,
                el: 45.5
            })
            .unwrap(),
            Encoded {
                bytes: b"P 180.00 45.50\n".to_vec(),
                expect: RotExpect::Status
            }
        );
        assert_eq!(e.encode(&RotOp::GetPosition).unwrap().bytes, b"p\n");
        assert_eq!(e.encode(&RotOp::Stop).unwrap().bytes, b"S\n");
        // Status
        assert_eq!(
            e.parse(RotExpect::Status, b"RPRT 0\n").unwrap(),
            Some((RotReply::Done, 7))
        );
        assert_eq!(e.parse(RotExpect::Status, b"RPRT ").unwrap(), None);
        let err = e.parse(RotExpect::Status, b"RPRT -1\n").unwrap_err();
        assert!(err.detail.contains("-1") && err.detail.contains("invalid parameter"));
        assert!(e.parse(RotExpect::Status, b"hello\n").is_err());
        // Position: two lines, in pieces
        assert_eq!(e.parse(RotExpect::Position, b"123.50\n").unwrap(), None);
        assert_eq!(
            e.parse(RotExpect::Position, b"123.50\n45.00\n").unwrap(),
            Some((
                RotReply::Position {
                    az: 123.5,
                    el: 45.0
                },
                13
            ))
        );
        assert!(e.parse(RotExpect::Position, b"RPRT -6\n").is_err());
        assert!(e.parse(RotExpect::Position, b"RPRT 0\n").is_err());
        assert!(e.parse(RotExpect::Position, b"abc\n1\n").is_err());
        assert!(e.parse(RotExpect::Position, b"NaN\n1\n").is_err());
        assert_eq!(
            e.parse(RotExpect::Nothing, b"").unwrap(),
            Some((RotReply::Done, 0))
        );
    }

    // Tests [@ANCHOR: radioctl_station_easycomm_new]
    // Tests [@ANCHOR: radioctl_station_easycomm_description]
    // Tests [@ANCHOR: radioctl_station_easycomm_encode]
    // Tests [@ANCHOR: radioctl_station_easycomm_parse]
    // Tests [@ANCHOR: radioctl_station_easycomm_parse_position]
    #[test]
    fn easycomm_encodes_and_parses_the_specification_shapes() {
        let e1 = EasycommEngine::new(desc("easycomm1-azel"));
        let e2 = EasycommEngine::new(desc("easycomm2-azel"));
        assert_eq!(e2.description().protocol, RotatorProtocol::Easycomm2);
        let set = RotOp::SetPosition {
            az: 123.46,
            el: 6.0,
        };
        assert_eq!(
            e1.encode(&set).unwrap().bytes,
            b"AZ123.5 EL6.0 UP000 XXX DN000 XXX\n"
        );
        assert_eq!(e2.encode(&set).unwrap().bytes, b"AZ123.5 EL6.0\n");
        assert_eq!(e2.encode(&RotOp::Park).unwrap().bytes, b"AZ0.0 EL0.0\n");
        assert_eq!(e2.encode(&RotOp::Stop).unwrap().bytes, b"SA SE\n");
        assert_eq!(e2.encode(&RotOp::GetPosition).unwrap().bytes, b"AZ EL\n");
        assert_eq!(
            e1.encode(&RotOp::GetPosition).unwrap_err().kind,
            StationErrorKind::Refused
        );
        assert_eq!(
            e2.encode(&RotOp::SetPosition { az: 361.0, el: 0.0 })
                .unwrap_err()
                .kind,
            StationErrorKind::Refused
        );
        // One line or two, in pieces, with an echo word in front.
        let want = Some((RotReply::Position { az: 12.5, el: 3.0 }, 13));
        assert_eq!(
            e2.parse(RotExpect::Position, b"AZ12.5 EL3.0\r\n").unwrap(),
            want
        );
        assert_eq!(
            e2.parse(RotExpect::Position, b"AZ12.5 EL3.0").unwrap(),
            None
        );
        assert_eq!(e2.parse(RotExpect::Position, b"AZ12.5\nEL3").unwrap(), None);
        assert_eq!(
            e2.parse(RotExpect::Position, b"VE1.0\nAZ12.5\nEL3.0\n")
                .unwrap(),
            Some((RotReply::Position { az: 12.5, el: 3.0 }, 19))
        );
        assert!(e2.parse(RotExpect::Position, b"ALjam\n").is_err());
        assert!(e2.parse(RotExpect::Position, b"AZxx EL1\n").is_err());
        assert!(e2.parse(RotExpect::Status, b"x").is_err());
        assert_eq!(
            e2.parse(RotExpect::Nothing, b"").unwrap(),
            Some((RotReply::Done, 0))
        );
        // Azimuth only.
        let mut d = desc("easycomm2-azel");
        d.has_elevation = false;
        d.el_max = 0.0;
        let az = EasycommEngine::new(d);
        assert_eq!(
            az.encode(&RotOp::SetPosition { az: 10.0, el: 50.0 })
                .unwrap()
                .bytes,
            b"AZ10.0\n"
        );
        assert_eq!(az.encode(&RotOp::GetPosition).unwrap().bytes, b"AZ\n");
        assert_eq!(az.encode(&RotOp::Stop).unwrap().bytes, b"SA\n");
        assert_eq!(
            az.parse(RotExpect::Position, b"AZ10.0\n").unwrap(),
            Some((RotReply::Position { az: 10.0, el: 0.0 }, 7))
        );
    }

    // Tests [@ANCHOR: radioctl_station_rot2prog_new]
    // Tests [@ANCHOR: radioctl_station_rot2prog_description]
    // Tests [@ANCHOR: radioctl_station_rot2prog_encode]
    // Tests [@ANCHOR: radioctl_station_rot2prog_parse]
    // Tests [@ANCHOR: radioctl_station_rot2prog_frame]
    // Tests [@ANCHOR: radioctl_station_rot2prog_reading]
    #[test]
    fn rot2prog_encodes_and_parses_the_sheet_frames() {
        let e = Rot2ProgEngine::new(desc("spid-rot2prog"));
        assert_eq!(e.description().protocol, RotatorProtocol::Rot2Prog);
        // The sheet's set example: 5.5 degrees and 10 degrees at divisor 10 is "3655" and "3700".
        assert_eq!(
            e.encode(&RotOp::SetPosition { az: 5.5, el: 10.0 })
                .unwrap()
                .bytes,
            [0x57, b'3', b'6', b'5', b'5', 10, b'3', b'7', b'0', b'0', 10, 0x2F, 0x20]
        );
        assert_eq!(
            e.encode(&RotOp::GetPosition).unwrap().bytes,
            [0x57, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x1F, 0x20]
        );
        assert_eq!(
            e.encode(&RotOp::Stop).unwrap().bytes,
            [0x57, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x0F, 0x20]
        );
        assert_eq!(
            e.encode(&RotOp::SetPosition { az: 361.0, el: 0.0 })
                .unwrap_err()
                .kind,
            StationErrorKind::Refused
        );
        // The sheet's reply example: "3823" and "3605", divisor 10: azimuth 22.3, elevation 0.5.
        let ascii = [
            0x57, b'3', b'8', b'2', b'3', 10, b'3', b'6', b'0', b'5', 10, 0x20,
        ];
        let plain = [0x57, 3, 8, 2, 3, 10, 3, 6, 0, 5, 10, 0x20];
        for wire in [ascii, plain] {
            let Some((RotReply::Position { az, el }, used)) =
                e.parse(RotExpect::Position, &wire).unwrap()
            else {
                panic!("no position");
            };
            assert!((az - 22.3).abs() < 1e-3 && (el - 0.5).abs() < 1e-3 && used == 12);
        }
        // Junk before the frame, a short frame, a bad end, a bad digit, a zero divisor.
        let mut junk = vec![0x00, 0x13];
        junk.extend_from_slice(&ascii);
        assert_eq!(e.parse(RotExpect::Position, &junk).unwrap().unwrap().1, 14);
        assert_eq!(e.parse(RotExpect::Position, &ascii[..11]).unwrap(), None);
        assert_eq!(e.parse(RotExpect::Position, b"").unwrap(), None);
        let mut bad = ascii;
        bad[11] = 0x21;
        assert!(e.parse(RotExpect::Position, &bad).is_err());
        let mut bad = ascii;
        bad[2] = b'x';
        assert!(e.parse(RotExpect::Position, &bad).is_err());
        let mut bad = ascii;
        bad[5] = 0;
        assert!(e.parse(RotExpect::Position, &bad).is_err());
        assert!(e.parse(RotExpect::Status, b"x").is_err());
        assert_eq!(
            e.parse(RotExpect::Nothing, b"").unwrap(),
            Some((RotReply::Done, 0))
        );
        // The description needs its resolution.
        let mut d = desc("spid-rot2prog");
        d.pulses_per_degree = None;
        assert!(d
            .validate()
            .unwrap_err()
            .detail
            .contains("pulses_per_degree"));
        d.pulses_per_degree = Some(11);
        assert!(d.validate().is_err());
    }

    // Tests [@ANCHOR: radioctl_station_dcu_new]
    // Tests [@ANCHOR: radioctl_station_dcu_description]
    // Tests [@ANCHOR: radioctl_station_dcu_encode]
    // Tests [@ANCHOR: radioctl_station_dcu_parse]
    #[test]
    fn dcu_encodes_the_manual_commands_and_reads_nothing() {
        let e = DcuEngine::new(desc("hygain-dcu3"));
        assert_eq!(e.description().protocol, RotatorProtocol::Dcu);
        let set = |az: f32| e.encode(&RotOp::SetPosition { az, el: 0.0 });
        assert_eq!(set(5.0).unwrap().bytes, b"AP1005;AM1;");
        assert_eq!(set(359.4).unwrap().bytes, b"AP1359;AM1;");
        assert_eq!(set(360.0).unwrap().bytes, b"AP1000;AM1;", "360 is 000");
        assert_eq!(set(359.6).unwrap().bytes, b"AP1000;AM1;");
        assert_eq!(set(360.5).unwrap_err().kind, StationErrorKind::Refused);
        assert_eq!(e.encode(&RotOp::Stop).unwrap().bytes, b"AS1;");
        assert_eq!(
            e.encode(&RotOp::GetPosition).unwrap_err().kind,
            StationErrorKind::Refused
        );
        assert_eq!(
            e.encode(&RotOp::Park).unwrap_err().kind,
            StationErrorKind::Refused
        );
        assert_eq!(
            e.parse(RotExpect::Nothing, b"").unwrap(),
            Some((RotReply::Done, 0))
        );
        assert!(e.parse(RotExpect::Position, b"; 90").is_err());
        let mut d = desc("hygain-dcu3");
        d.has_elevation = true;
        assert!(d.validate().is_err());
    }

    // Tests [@ANCHOR: radioctl_station_gs232_new]
    // Tests [@ANCHOR: radioctl_station_gs232_description]
    // Tests [@ANCHOR: radioctl_station_gs232_encode]
    // Tests [@ANCHOR: radioctl_station_gs232_parse]
    // Tests [@ANCHOR: radioctl_station_gs232_parse_position]
    #[test]
    fn gs232_encodes_and_parses_the_captured_shapes() {
        let e = Gs232Engine::new(desc("yaesu-g-5500-gs232b"));
        assert_eq!(e.description().protocol, RotatorProtocol::Gs232);
        // The shapes the relay's simulator captured from Hamlib: zero-padded, rounded.
        assert_eq!(
            e.encode(&RotOp::SetPosition {
                az: 180.0,
                el: 45.0
            })
            .unwrap()
            .bytes,
            b"W180 045\r"
        );
        assert_eq!(
            e.encode(&RotOp::SetPosition { az: 5.0, el: 90.0 })
                .unwrap()
                .bytes,
            b"W005 090\r"
        );
        assert_eq!(
            e.encode(&RotOp::SetPosition {
                az: 359.6,
                el: 12.3
            })
            .unwrap()
            .bytes,
            b"W360 012\r"
        );
        assert_eq!(e.encode(&RotOp::Park).unwrap().bytes, b"W000 000\r");
        assert_eq!(e.encode(&RotOp::GetPosition).unwrap().bytes, b"C2\r");
        assert_eq!(e.encode(&RotOp::Stop).unwrap().bytes, b"S\r");
        for (text, want) in [
            ("+0180+045", (180.0, 45.0)),
            ("+0005+009", (5.0, 9.0)),
            ("+0450+0180", (450.0, 180.0)),
            ("AZ=180 EL=045", (180.0, 45.0)),
        ] {
            let wire = format!("\r\n{text}\r\n");
            assert_eq!(
                e.parse(RotExpect::Position, wire.as_bytes()).unwrap(),
                Some((
                    RotReply::Position {
                        az: want.0,
                        el: want.1
                    },
                    wire.len() - 1
                )),
                "{text}"
            );
        }
        assert_eq!(e.parse(RotExpect::Position, b"+0180+0").unwrap(), None);
        for bad in [
            "180 45\r",
            "+01a0+045\r",
            "+0180\r",
            "+00000000+045\r",
            "AZ=1 EL=x\r",
        ] {
            assert!(
                e.parse(RotExpect::Position, bad.as_bytes()).is_err(),
                "{bad}"
            );
        }
        assert!(e.parse(RotExpect::Status, b"x").is_err());
        assert_eq!(
            e.parse(RotExpect::Nothing, b"").unwrap(),
            Some((RotReply::Done, 0))
        );
    }
}
