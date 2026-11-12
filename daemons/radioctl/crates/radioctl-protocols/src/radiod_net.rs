// SPDX-License-Identifier: AGPL-3.0-or-later
//! The `radiod-channel` engine: one receive channel of a ka9q-radio `radiod` (an RX-888 or any other
//! front end it drives) as a virtual rig.
//!
//! `radiod` is controlled by multicast TLV datagrams and sends audio as RTP; neither is a command
//! stream of the kind the actor drives. So, as for OpenHPSDR, the engine speaks a small line protocol
//! to the bridge in [`crate::radiod_io`], which owns the `radiod_client` sockets and the channel:
//!
//! | Request | Reply | Meaning |
//! |---|---|---|
//! | `GF` | `F <hz>` | dial (carrier) frequency |
//! | `F <hz>` | `OK` or `ERR <text>` | tune |
//! | `GM` | `M <usb\|lsb\|cw\|am\|fm\|sam\|sam-l\|sam-u>` | mode |
//! | `M <name>` | `OK` or `ERR <text>` | set mode (the mode's own passband comes back) |
//! | `GW` | `W <hz>` | passband width |
//! | `W <hz>` | `OK` or `ERR <text>` | set passband width (edges placed per mode by the bridge) |
//! | `GS` | `S <raw>` | signal: channel baseband power, tenths of a dB above -200 dB re full scale |
//!
//! **Receive only.** No transmit operation is encoded: PTT, CW and power are refused as unsupported,
//! and a `radiod-channel` description may not claim a transmit capability (the description loader
//! refuses one, [`check_receive_only`]). The console therefore never offers Transmit for it.
//!
//! The signal is `radiod`'s own baseband power, which is referred to the input (the front end's RF
//! gain is already removed), not calibrated to dBm, and not an S-meter. Everything is
//! `verified=unverified` against the rig-control conventions; the bridge was run against the RX-888.

use std::time::Duration;

use radioctl_core::description::{Family, RigDescription};
use radioctl_core::engine::{Inbound, ProtocolEngine, ReplyKind, Request};
use radioctl_core::error::{ErrorKind, RigError};
use radioctl_core::op::{Field, Observation, Op};
use radioctl_core::units::{Hz, Mode, Vfo};

/// Longest line accepted from the bridge.
const MAX_LINE: usize = 128;

/// Largest passband edge magnitude, Hz: `radiod_client::command::MAX_FILTER_EDGE_HZ`, repeated here
/// so the sans-IO engine does not need the client (a test pins the two equal).
pub const MAX_EDGE_HZ: f32 = 12_000.0;

// [@ANCHOR: radioctl_radiod_sam_passband]
/// How far a single-sideband synchronous passband (SAM-L, SAM-U) reaches across the carrier, Hz:
/// the PLL tracks the carrier, so it must stay inside the passband (`radiod_client`'s `SamLower` and
/// `SamUpper` use the same 100 Hz, from `radiod`'s `[ame]` preset).
pub const SAM_CARRIER_GUARD_HZ: f32 = 100.0;
/// The narrowest SAM-L or SAM-U passband, Hz: twice the guard, so the carrier is never at an edge.
pub const SAM_SIDEBAND_MIN_WIDTH_HZ: f32 = 200.0;

/// Signal scale: the raw value is `(dB - SIGNAL_FLOOR_DB) * 10`, clamped to `0..=u16::MAX`.
pub const SIGNAL_FLOOR_DB: f32 = -200.0;

// [@ANCHOR: radioctl_radiod_signal_raw]
/// The raw signal value for a baseband power in dB re full scale: tenths of a dB above
/// [`SIGNAL_FLOOR_DB`], rounded, clamped to `0..=65535`. `-inf` (no signal yet) and NaN give 0.
pub fn signal_raw(db: f32) -> u16 {
    if !db.is_finite() {
        return 0;
    }
    let tenths = ((db - SIGNAL_FLOOR_DB) * 10.0).round();
    tenths.clamp(0.0, f32::from(u16::MAX)) as u16
}

// [@ANCHOR: radioctl_radiod_signal_db]
/// The inverse of [`signal_raw`]: dB re full scale for a raw value.
pub fn signal_db(raw: u16) -> f32 {
    SIGNAL_FLOOR_DB + f32::from(raw) / 10.0
}

// [@ANCHOR: radioctl_radiod_mode_name]
/// The line-protocol name of a mode the channel can demodulate, or `None`.
pub fn mode_name(mode: Mode) -> Option<&'static str> {
    Some(match mode {
        Mode::Usb => "usb",
        Mode::Lsb => "lsb",
        Mode::Cw => "cw",
        Mode::Am => "am",
        Mode::Fm => "fm",
        Mode::Sam => "sam",
        Mode::SamL => "sam-l",
        Mode::SamU => "sam-u",
        _ => return None,
    })
}

// [@ANCHOR: radioctl_radiod_mode_from_name]
/// The mode for a line-protocol name, or `None`.
pub fn mode_from_name(name: &str) -> Option<Mode> {
    Some(match name {
        "usb" => Mode::Usb,
        "lsb" => Mode::Lsb,
        "cw" => Mode::Cw,
        "am" => Mode::Am,
        "fm" => Mode::Fm,
        "sam" => Mode::Sam,
        "sam-l" => Mode::SamL,
        "sam-u" => Mode::SamU,
        _ => return None,
    })
}

// [@ANCHOR: radioctl_radiod_mode_spec]
/// How a mode is demodulated by `radiod` and where a passband of a given width sits for it. The
/// edges are Hz from the carrier, in the `radiod_client` modes' own placement: USB and LSB keep their
/// 50 Hz edge next to the carrier, CW centres on its 600 Hz tone, AM, SAM and FM centre on the carrier,
/// and SAM-L and SAM-U keep their sideband plus 100 Hz across the carrier (the PLL must see it).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModeSpec {
    /// The line-protocol name (`usb`, `lsb`, `cw`, `am`, `fm`, `sam`, `sam-l`, `sam-u`).
    pub name: &'static str,
    /// The passband width the mode comes back with after a mode change, Hz.
    pub default_width_hz: u64,
}

impl ModeSpec {
    // [@ANCHOR: radioctl_radiod_mode_spec_of]
    /// The spec of a mode; a mode the channel cannot demodulate gets USB's (the caller refuses such
    /// a mode before it gets here, [`mode_name`]).
    pub fn of(mode: Mode) -> ModeSpec {
        let (name, default_width_hz) = match mode {
            Mode::Lsb => ("lsb", 2950),
            Mode::Cw => ("cw", 400),
            Mode::Am => ("am", 10_000),
            Mode::Fm => ("fm", 16_000),
            Mode::Sam => ("sam", 10_000),
            Mode::SamL => ("sam-l", 5_100),
            Mode::SamU => ("sam-u", 5_100),
            _ => ("usb", 2950),
        };
        ModeSpec {
            name,
            default_width_hz,
        }
    }

    // [@ANCHOR: radioctl_radiod_mode_spec_edges]
    /// The passband edges, Hz from the carrier, for a width in Hz (clamped to 1 Hz to 24 kHz, and
    /// to the 12 kHz edge limit of `radiod_client`). SAM-L and SAM-U take at least
    /// [`SAM_SIDEBAND_MIN_WIDTH_HZ`], so the carrier stays inside the passband.
    pub fn edges(self, width_hz: u64) -> (f32, f32) {
        let w = width_hz.clamp(1, 24_000) as f32;
        let ws = w.max(SAM_SIDEBAND_MIN_WIDTH_HZ);
        let (lo, hi) = match self.name {
            "lsb" => (-50.0 - w, -50.0),
            "cw" => (600.0 - w / 2.0, 600.0 + w / 2.0),
            "am" | "fm" | "sam" => (-w / 2.0, w / 2.0),
            "sam-l" => (SAM_CARRIER_GUARD_HZ - ws, SAM_CARRIER_GUARD_HZ),
            "sam-u" => (-SAM_CARRIER_GUARD_HZ, ws - SAM_CARRIER_GUARD_HZ),
            _ => (50.0, 50.0 + w),
        };
        (
            lo.clamp(-MAX_EDGE_HZ, MAX_EDGE_HZ),
            hi.clamp(-MAX_EDGE_HZ, MAX_EDGE_HZ),
        )
    }
}

// [@ANCHOR: radioctl_radiod_check_receive_only]
/// Refuses a description that claims any transmit-capable capability: a `radiod` channel cannot
/// transmit, and claiming it would make a console offer Transmit.
pub fn check_receive_only(desc: &RigDescription) -> Result<(), RigError> {
    match desc.capabilities.iter().find(|c| c.transmits()) {
        Some(c) => Err(RigError::new(
            ErrorKind::Unsupported,
            format!(
                "a radiod channel is receive-only; the description claims {}",
                c.name()
            ),
        )),
        None => Ok(()),
    }
}

/// The engine for one `radiod` channel.
// [@ANCHOR: radioctl_radiod_net_radiod_engine]
#[derive(Debug)]
pub struct RadiodEngine {
    desc: RigDescription,
    line: Vec<u8>,
    discarding: bool,
}

impl RadiodEngine {
    // [@ANCHOR: radioctl_radiod_engine_new]
    /// Builds an engine for a `radiod-channel` description; refuses another family or a
    /// description that claims a transmit capability.
    pub fn new(desc: RigDescription) -> Result<RadiodEngine, RigError> {
        if desc.family != Family::Radiod {
            return Err(RigError::new(
                ErrorKind::Unsupported,
                "not a radiod-channel description",
            ));
        }
        check_receive_only(&desc)?;
        Ok(RadiodEngine {
            desc,
            line: Vec::new(),
            discarding: false,
        })
    }
}

// [@ANCHOR: radioctl_radiod_engine_parse_line]
/// One complete line from the bridge.
fn parse_line(line: &str) -> Inbound {
    let mut parts = line.split_whitespace();
    let head = parts.next();
    let arg = parts.next();
    let number = arg.and_then(|v| v.parse::<u64>().ok());
    match (head, number) {
        (Some("OK"), _) => Inbound::Ack,
        (Some("ERR"), _) => Inbound::Nak,
        (Some("F"), Some(hz)) => Inbound::Observation(Observation::Frequency(Vfo::A, Hz(hz))),
        (Some("W"), Some(hz)) => Inbound::Observation(Observation::FilterWidth(Hz(hz))),
        (Some("S"), Some(raw)) => match u16::try_from(raw) {
            Ok(raw) => Inbound::Observation(Observation::Signal(raw)),
            Err(_) => Inbound::Unrecognised(line.as_bytes().to_vec()),
        },
        (Some("M"), _) => match arg.and_then(mode_from_name) {
            Some(mode) => Inbound::Observation(Observation::Mode(mode)),
            None => Inbound::Unrecognised(line.as_bytes().to_vec()),
        },
        _ => Inbound::Unrecognised(line.as_bytes().to_vec()),
    }
}

impl ProtocolEngine for RadiodEngine {
    fn description(&self) -> &RigDescription {
        &self.desc
    }

    // [@ANCHOR: radioctl_radiod_engine_encode]
    fn encode(&mut self, op: &Op) -> Result<Request, RigError> {
        let (text, reply) = match op {
            Op::GetFrequency(Vfo::A) => (
                "GF".to_string(),
                ReplyKind::Observation(Field::Frequency(Vfo::A)),
            ),
            Op::SetFrequency(Vfo::A, Hz(hz)) if *hz <= u64::from(u32::MAX) => {
                (format!("F {hz}"), ReplyKind::Ack)
            }
            Op::GetMode => ("GM".to_string(), ReplyKind::Observation(Field::Mode)),
            Op::SetMode(mode) => match mode_name(*mode) {
                Some(name) => (format!("M {name}"), ReplyKind::Ack),
                None => {
                    return Err(RigError::new(
                        ErrorKind::Unsupported,
                        "a radiod channel demodulates USB, LSB, CW, AM and FM only",
                    ))
                }
            },
            Op::GetFilterWidth => ("GW".to_string(), ReplyKind::Observation(Field::FilterWidth)),
            Op::SetFilterWidth(Hz(hz)) if (1..=24_000).contains(hz) => {
                (format!("W {hz}"), ReplyKind::Ack)
            }
            Op::GetSignal => ("GS".to_string(), ReplyKind::Observation(Field::Signal)),
            _ => {
                return Err(RigError::new(
                    ErrorKind::Unsupported,
                    "a radiod channel is receive-only: frequency, mode, filter width and signal",
                ))
            }
        };
        let mut bytes = text.into_bytes();
        bytes.push(b'\n');
        Ok(Request { bytes, reply })
    }

    // [@ANCHOR: radioctl_radiod_engine_feed]
    fn feed(&mut self, bytes: &[u8], _now: Duration) -> Vec<Inbound> {
        let mut out = Vec::new();
        for &b in bytes {
            if b == b'\n' {
                if !self.discarding {
                    let text = String::from_utf8_lossy(&self.line).into_owned();
                    let text = text.trim();
                    if !text.is_empty() {
                        out.push(parse_line(text));
                    }
                }
                self.line.clear();
                self.discarding = false;
            } else if self.discarding {
                continue;
            } else if self.line.len() >= MAX_LINE {
                self.line.clear();
                self.discarding = true;
                out.push(Inbound::Unrecognised(Vec::new()));
            } else {
                self.line.push(b);
            }
        }
        out
    }

    // [@ANCHOR: radioctl_radiod_engine_link_restored]
    /// The bridge keeps the channel itself; a restored link drops a half line and sets up nothing.
    fn link_restored(&mut self) -> Vec<Request> {
        self.line.clear();
        self.discarding = false;
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    // Tests [@ANCHOR: radioctl_radiod_net_radiod_engine]
    use super::*;
    use radioctl_core::capability::Capability;
    use radioctl_core::rigfile;

    fn engine() -> RadiodEngine {
        let file = rigfile::parse(include_str!("../../../rigs/radiod-channel.toml")).expect("file");
        RadiodEngine::new(file.description).expect("engine")
    }

    // Tests [@ANCHOR: radioctl_radiod_engine_new]
    // Tests [@ANCHOR: radioctl_radiod_check_receive_only]
    #[test]
    fn the_shipped_file_is_receive_only_and_other_families_are_refused() {
        let e = engine();
        let caps = &e.description().capabilities;
        assert!(caps.iter().all(|c| !c.transmits()));
        for c in [
            Capability::ReadFrequency,
            Capability::SetFrequency,
            Capability::ReadMode,
            Capability::SetMode,
            Capability::Filter,
            Capability::ReadSignal,
        ] {
            assert!(caps.has(c), "{c:?}");
        }
        assert!(!caps.has(Capability::SetPtt) && !caps.has(Capability::ReadPtt));
        let other = rigfile::parse(include_str!("../../../rigs/icom-ic-7300.toml")).expect("file");
        assert!(RadiodEngine::new(other.description).is_err());
        let mut keyed = e.description().clone();
        keyed.capabilities = radioctl_core::capability::CapabilitySet::of(&[
            Capability::SetFrequency,
            Capability::SetPtt,
        ]);
        let err = RadiodEngine::new(keyed).expect_err("refused");
        assert!(err.to_string().contains("set-ptt"), "{err}");
    }

    // Tests [@ANCHOR: radioctl_radiod_engine_encode]
    // Tests [@ANCHOR: radioctl_radiod_mode_name]
    #[test]
    fn encodes_the_receive_operations_and_refuses_transmit() {
        let mut e = engine();
        let text = |e: &mut RadiodEngine, op: Op| e.encode(&op).map(|r| r.bytes);
        assert_eq!(
            text(&mut e, Op::GetFrequency(Vfo::A)).ok(),
            Some(b"GF\n".to_vec())
        );
        assert_eq!(
            text(&mut e, Op::SetFrequency(Vfo::A, Hz(10_000_000))).ok(),
            Some(b"F 10000000\n".to_vec())
        );
        assert_eq!(
            text(&mut e, Op::SetMode(Mode::Am)).ok(),
            Some(b"M am\n".to_vec())
        );
        assert_eq!(text(&mut e, Op::GetMode).ok(), Some(b"GM\n".to_vec()));
        assert_eq!(
            text(&mut e, Op::SetFilterWidth(Hz(2400))).ok(),
            Some(b"W 2400\n".to_vec())
        );
        assert_eq!(
            text(&mut e, Op::GetFilterWidth).ok(),
            Some(b"GW\n".to_vec())
        );
        assert_eq!(text(&mut e, Op::GetSignal).ok(), Some(b"GS\n".to_vec()));
        for refused in [
            Op::SetPtt(true),
            Op::SetPtt(false),
            Op::GetPtt,
            Op::GetPower,
            Op::SetMode(Mode::Rtty),
            Op::SetFrequency(Vfo::B, Hz(1)),
            Op::SetFrequency(Vfo::A, Hz(u64::MAX)),
            Op::SetFilterWidth(Hz(0)),
            Op::SetFilterWidth(Hz(24_001)),
        ] {
            let err = e.encode(&refused).expect_err("refused");
            assert_eq!(err.kind, ErrorKind::Unsupported, "{refused:?}");
        }
    }

    // Tests [@ANCHOR: radioctl_radiod_engine_feed]
    // Tests [@ANCHOR: radioctl_radiod_engine_parse_line]
    // Tests [@ANCHOR: radioctl_radiod_mode_from_name]
    // Tests [@ANCHOR: radioctl_radiod_engine_link_restored]
    #[test]
    fn replies_decode_across_chunks_and_overlong_lines_are_dropped() {
        let mut e = engine();
        assert!(e.feed(b"F 1000", Duration::ZERO).is_empty());
        assert_eq!(
            e.feed(b"0000\nM usb\nW 2950\nS 729\nOK\nERR no\n", Duration::ZERO),
            vec![
                Inbound::Observation(Observation::Frequency(Vfo::A, Hz(10_000_000))),
                Inbound::Observation(Observation::Mode(Mode::Usb)),
                Inbound::Observation(Observation::FilterWidth(Hz(2950))),
                Inbound::Observation(Observation::Signal(729)),
                Inbound::Ack,
                Inbound::Nak,
            ]
        );
        assert!(matches!(
            e.feed(b"M rtty\nS 70000\n", Duration::ZERO).as_slice(),
            [Inbound::Unrecognised(_), Inbound::Unrecognised(_)]
        ));
        let long = vec![b'x'; MAX_LINE + 5];
        let out = e.feed(&long, Duration::ZERO);
        assert_eq!(out, vec![Inbound::Unrecognised(Vec::new())]);
        assert!(
            e.feed(b"yy\n", Duration::ZERO).is_empty(),
            "rest of the long line dropped"
        );
        assert!(e.feed(b"F 12", Duration::ZERO).is_empty());
        assert!(e.link_restored().is_empty());
        assert_eq!(
            e.feed(b"OK\n", Duration::ZERO),
            vec![Inbound::Ack],
            "the half line before the restore is gone"
        );
        for name in ["usb", "lsb", "cw", "am", "fm"] {
            let mode = mode_from_name(name).expect("known");
            assert_eq!(mode_name(mode), Some(name));
        }
        assert_eq!(mode_from_name("iq"), None);
    }

    // Tests [@ANCHOR: radioctl_radiod_signal_raw]
    // Tests [@ANCHOR: radioctl_radiod_signal_db]
    #[test]
    fn signal_is_tenths_of_a_db_above_minus_200() {
        assert_eq!(signal_raw(-127.1), 729);
        assert!((signal_db(729) - -127.1).abs() < 1e-4);
        assert_eq!(signal_raw(f32::NEG_INFINITY), 0);
        assert_eq!(signal_raw(f32::NAN), 0);
        assert_eq!(signal_raw(-300.0), 0);
        assert_eq!(signal_raw(1.0e9), u16::MAX);
        assert_eq!(signal_raw(0.0), 2000);
    }

    // Tests [@ANCHOR: radioctl_radiod_mode_spec]
    // Tests [@ANCHOR: radioctl_radiod_mode_spec_of]
    // Tests [@ANCHOR: radioctl_radiod_mode_spec_edges]
    #[test]
    fn passband_edges_follow_the_mode() {
        assert_eq!(ModeSpec::of(Mode::Usb).edges(2400), (50.0, 2450.0));
        assert_eq!(ModeSpec::of(Mode::Lsb).edges(2400), (-2450.0, -50.0));
        assert_eq!(ModeSpec::of(Mode::Cw).edges(500), (350.0, 850.0));
        assert_eq!(ModeSpec::of(Mode::Am).edges(6000), (-3000.0, 3000.0));
        assert_eq!(ModeSpec::of(Mode::Fm).edges(16_000), (-8000.0, 8000.0));
        assert_eq!(
            ModeSpec::of(Mode::Usb).edges(24_000),
            (50.0, 12_000.0),
            "clamped to the edge limit"
        );
        assert_eq!(
            ModeSpec::of(Mode::Am).edges(0),
            (-0.5, 0.5),
            "at least 1 Hz"
        );
        assert_eq!(ModeSpec::of(Mode::Rtty).name, "usb");
        for mode in [Mode::Usb, Mode::Lsb, Mode::Cw, Mode::Am, Mode::Fm] {
            let spec = ModeSpec::of(mode);
            assert_eq!(Some(spec.name), mode_name(mode));
            let (lo, hi) = spec.edges(spec.default_width_hz);
            assert!(
                lo < hi && (hi - lo - spec.default_width_hz as f32).abs() < 1.0,
                "{spec:?}"
            );
        }
    }

    #[cfg(feature = "tokio-io")]
    #[test]
    fn the_edge_limit_is_the_clients() {
        assert_eq!(MAX_EDGE_HZ, radiod_client::command::MAX_FILTER_EDGE_HZ);
    }
}
