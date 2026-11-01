// SPDX-License-Identifier: AGPL-3.0-or-later
//! Lowe HF-235 receiver: sans-IO client engine and virtual twin.
//!
//! **Sources.** Facts only, read from Hamlib 4.6.2 (released tarball, sha256
//! b2ac73f44dd1161e95fdee6c95276144757647bf92d7fdb369ee2fe41ed47ae8): `rigs/lowe/lowe.c` and
//! `hf235.c`. No Lowe document was read; nothing is verified against a radio.
//!
//! Commands are text ended by CR; the radio answers each with a line ended by CR (Hamlib reads
//! one line after a setting and takes a timeout for an answer).
//!
//! * `FRQ` and kilohertz as a decimal number (six decimals) sets the frequency; `FRQ?` asks. Hamlib
//!   skips the first byte of the answer and reads a number; the twin sends `F` and the number.
//! * `MOD` and a mode name (`USB`, `LSB`, `CW`, `FM`, `AM`; Hamlib also writes `FAX` and `AMS`,
//!   which have no radioctl mode) sets the mode; `MOD?` asks. Hamlib skips the first byte of the
//!   answer and compares the rest with the names; the twin sends `M` and the name. The first byte
//!   is an assumption. The description's mode codes are the first letters `U`, `L`, `C`, `F`, `A`.
//! * Hamlib formats the frequency from a single-precision float; the engine writes the exact
//!   kilohertz value instead.

use std::time::Duration;

use radioctl_core::description::RigDescription;
use radioctl_core::engine::{Inbound, ProtocolEngine, ReplyKind, Request};
use radioctl_core::error::{ErrorKind, RigError};
use radioctl_core::op::{Field, Observation, Op};
use radioctl_core::transcript::Panel;
use radioctl_core::units::{Hz, Vfo};

use crate::serial_vendor::{vendor_of, RxBuffer, TwinSnapshot, VendorTwin};

const CR: u8 = b'\r';

fn unsupported(what: &str) -> RigError {
    RigError::new(ErrorKind::Unsupported, what.to_string())
}

// [@ANCHOR: radioctl_lowe_khz_text]
/// A frequency as kilohertz with six decimals (`14200.000000`).
pub fn khz_text(hz: Hz) -> String {
    format!("{}.{:03}000", hz.0 / 1000, hz.0 % 1000)
}

// [@ANCHOR: radioctl_lowe_parse_khz]
/// The frequency of a kilohertz decimal with at most six decimals, or `None`.
pub fn parse_khz(text: &str) -> Option<Hz> {
    let (whole, frac) = text.split_once('.').unwrap_or((text, ""));
    if whole.is_empty() || frac.len() > 6 || !whole.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    if !frac.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let w: u64 = whole.parse().ok()?;
    let padded = format!("{frac:0<6}");
    let micro: u64 = padded.parse().ok()?;
    // Whole kilohertz and millionths of a kilohertz (milli-hertz): round to the hertz.
    w.checked_mul(1000)?
        .checked_add((micro + 500) / 1000)
        .map(Hz)
}

// [@ANCHOR: radioctl_lowe_mode_name]
/// The mode name Hamlib writes for a description mode code, or `None`.
pub fn mode_name(code: u8) -> Option<&'static str> {
    match code {
        b'U' => Some("USB"),
        b'L' => Some("LSB"),
        b'C' => Some("CW"),
        b'F' => Some("FM"),
        b'A' => Some("AM"),
        _ => None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pending {
    Nothing,
    Frequency,
    Mode,
    Ack,
}

/// The Lowe client engine.
#[derive(Debug)]
pub struct LoweEngine {
    desc: RigDescription,
    pending: Pending,
    rx: RxBuffer,
}

impl LoweEngine {
    // [@ANCHOR: radioctl_lowe_engine_new]
    /// Builds an engine from a Lowe vendor description.
    pub fn new(desc: RigDescription) -> Result<LoweEngine, RigError> {
        let (_, name) = vendor_of(&desc)?;
        if name != "hf235" {
            return Err(unsupported("unknown Lowe dialect in the description"));
        }
        Ok(LoweEngine {
            desc,
            pending: Pending::Nothing,
            rx: RxBuffer::default(),
        })
    }
}

impl ProtocolEngine for LoweEngine {
    fn description(&self) -> &RigDescription {
        &self.desc
    }

    // [@ANCHOR: radioctl_lowe_engine_encode]
    fn encode(&mut self, op: &Op) -> Result<Request, RigError> {
        if !self.desc.capabilities.has(op.required_capability()) {
            return Err(unsupported("capability not in the rig description"));
        }
        let (text, reply, pending) = match op {
            Op::GetFrequency(Vfo::A) => (
                "FRQ?\r".to_string(),
                ReplyKind::Observation(Field::Frequency(Vfo::A)),
                Pending::Frequency,
            ),
            Op::SetFrequency(Vfo::A, hz) => (
                format!("FRQ{}\r", khz_text(*hz)),
                ReplyKind::Ack,
                Pending::Ack,
            ),
            Op::GetMode => (
                "MOD?\r".to_string(),
                ReplyKind::Observation(Field::Mode),
                Pending::Mode,
            ),
            Op::SetMode(mode) => {
                let name = self
                    .desc
                    .mode_code(*mode)
                    .and_then(mode_name)
                    .ok_or_else(|| unsupported("mode not in the description"))?;
                (format!("MOD{name}\r"), ReplyKind::Ack, Pending::Ack)
            }
            _ => return Err(unsupported("not implemented for the Lowe engine")),
        };
        self.pending = pending;
        self.rx.take_all();
        Ok(Request {
            bytes: text.into_bytes(),
            reply,
        })
    }

    // [@ANCHOR: radioctl_lowe_engine_feed]
    fn feed(&mut self, bytes: &[u8], now: Duration) -> Vec<Inbound> {
        let mut out = Vec::new();
        if let Some(stale) = self.rx.push(bytes, now) {
            out.push(Inbound::Unrecognised(stale));
        }
        while let Some(line) = self.rx.take_line(CR) {
            let pending = std::mem::replace(&mut self.pending, Pending::Nothing);
            let text = String::from_utf8_lossy(&line).into_owned();
            // Both answers carry a one-byte tag that Hamlib skips.
            let rest = text.get(1..).unwrap_or("");
            out.extend(match pending {
                Pending::Ack => vec![Inbound::Ack],
                Pending::Frequency => match parse_khz(rest) {
                    Some(hz) => vec![Inbound::Observation(Observation::Frequency(Vfo::A, hz))],
                    None => vec![Inbound::Unrecognised(line)],
                },
                Pending::Mode => {
                    let found = self
                        .desc
                        .mode_codes
                        .iter()
                        .find(|(_, c)| mode_name(*c) == Some(rest))
                        .map(|(m, _)| *m);
                    match found {
                        Some(m) => vec![Inbound::Observation(Observation::Mode(m))],
                        None => vec![Inbound::Unrecognised(line)],
                    }
                }
                Pending::Nothing => vec![Inbound::Unrecognised(line)],
            });
        }
        out
    }
}

/// A virtual HF-235.
#[derive(Debug)]
pub struct LoweTwin {
    rx: RxBuffer,
    hz: Hz,
    mode: u8,
}

impl LoweTwin {
    // [@ANCHOR: radioctl_lowe_twin_new]
    /// A twin in the description's default state.
    pub fn new(desc: &RigDescription) -> Result<LoweTwin, RigError> {
        let (_, name) = vendor_of(desc)?;
        if name != "hf235" {
            return Err(unsupported("unknown Lowe dialect in the description"));
        }
        Ok(LoweTwin {
            rx: RxBuffer::default(),
            hz: desc.default_frequency,
            mode: desc.mode_code(desc.default_mode).unwrap_or(b'A'),
        })
    }
}

impl VendorTwin for LoweTwin {
    // [@ANCHOR: radioctl_lowe_twin_snapshot]
    fn snapshot(&self) -> Option<TwinSnapshot> {
        Some(TwinSnapshot {
            frequency: self.hz,
            mode_code: Some(self.mode),
            ptt: false,
        })
    }

    // [@ANCHOR: radioctl_lowe_twin_feed]
    fn feed(&mut self, bytes: &[u8], now: Duration) -> Vec<u8> {
        self.rx.push(bytes, now);
        let mut out = Vec::new();
        while let Some(line) = self.rx.take_line(CR) {
            let text = String::from_utf8_lossy(&line).into_owned();
            match text.as_str() {
                "FRQ?" => out.extend(format!("F{}\r", khz_text(self.hz)).into_bytes()),
                "MOD?" => {
                    out.extend(format!("M{}\r", mode_name(self.mode).unwrap_or("AM")).into_bytes())
                }
                t if t.starts_with("FRQ") => {
                    if let Some(hz) = t.get(3..).and_then(parse_khz) {
                        self.hz = hz;
                    }
                    out.push(CR);
                }
                t if t.starts_with("MOD") => {
                    if let Some(code) = (b'A'..=b'Z').find(|c| mode_name(*c) == t.get(3..)) {
                        self.mode = code;
                    }
                    out.push(CR);
                }
                _ => {}
            }
        }
        out
    }

    // [@ANCHOR: radioctl_lowe_twin_front_panel]
    fn front_panel(&mut self, panel: Panel) -> Result<Vec<u8>, String> {
        match panel {
            Panel::Frequency(hz) => self.hz = hz,
            other => return Err(format!("a Lowe twin has no {other:?} knob")),
        }
        Ok(Vec::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use radioctl_core::units::Mode;

    const T0: Duration = Duration::from_secs(1);

    fn run(e: &mut LoweEngine, t: &mut LoweTwin, op: Op) -> (Vec<u8>, Vec<Inbound>) {
        let r = e.encode(&op).expect("encodes");
        let a = t.feed(&r.bytes, T0);
        (r.bytes, e.feed(&a, T0))
    }

    fn shipped() -> RigDescription {
        let path = format!("{}/../../rigs/lowe-hf-235.toml", env!("CARGO_MANIFEST_DIR"));
        let text = std::fs::read_to_string(path).expect("rig file");
        radioctl_core::rigfile::parse(&text)
            .expect("loads")
            .description
    }

    // Tests [@ANCHOR: radioctl_lowe_khz_text]
    // Tests [@ANCHOR: radioctl_lowe_parse_khz]
    // Tests [@ANCHOR: radioctl_lowe_mode_name]
    #[test]
    fn kilohertz_decimals_and_mode_names() {
        for hz in [30_000, 14_200_000, 7_074_123, 29_999_999, 1] {
            assert_eq!(parse_khz(&khz_text(Hz(hz))), Some(Hz(hz)), "{hz}");
        }
        assert_eq!(khz_text(Hz(14_200_000)), "14200.000000");
        assert_eq!(khz_text(Hz(7_074_123)), "7074.123000");
        assert_eq!(parse_khz("7074"), Some(Hz(7_074_000)));
        assert_eq!(parse_khz("7074.5"), Some(Hz(7_074_500)));
        assert_eq!(parse_khz("7074.1234567"), None);
        assert_eq!(parse_khz("x"), None);
        assert_eq!(parse_khz(".5"), None);
        assert_eq!(mode_name(b'U'), Some("USB"));
        assert_eq!(mode_name(b'Z'), None);
    }

    // Tests [@ANCHOR: radioctl_lowe_engine_new]
    #[test]
    fn other_families_are_refused() {
        assert!(LoweEngine::new(RigDescription::ts2000()).is_err());
        assert!(LoweTwin::new(&RigDescription::ts2000()).is_err());
    }

    // Tests [@ANCHOR: radioctl_lowe_engine_encode]
    // Tests [@ANCHOR: radioctl_lowe_engine_feed]
    // Tests [@ANCHOR: radioctl_lowe_twin_new]
    // Tests [@ANCHOR: radioctl_lowe_twin_feed]
    // Tests [@ANCHOR: radioctl_lowe_twin_snapshot]
    // Tests [@ANCHOR: radioctl_lowe_twin_front_panel]
    #[test]
    fn the_engine_round_trips_against_the_twin() {
        let desc = shipped();
        let mut e = LoweEngine::new(desc.clone()).expect("engine");
        let mut t = LoweTwin::new(&desc).expect("twin");
        let (wire, seen) = run(&mut e, &mut t, Op::SetFrequency(Vfo::A, Hz(14_200_000)));
        assert_eq!(wire, b"FRQ14200.000000\r");
        assert_eq!(seen, vec![Inbound::Ack]);
        assert_eq!(
            run(&mut e, &mut t, Op::GetFrequency(Vfo::A)).1,
            vec![Inbound::Observation(Observation::Frequency(
                Vfo::A,
                Hz(14_200_000)
            ))]
        );
        for mode in [Mode::Usb, Mode::Lsb, Mode::Cw, Mode::Fm, Mode::Am] {
            assert_eq!(run(&mut e, &mut t, Op::SetMode(mode)).1, vec![Inbound::Ack]);
            assert_eq!(
                run(&mut e, &mut t, Op::GetMode).1,
                vec![Inbound::Observation(Observation::Mode(mode))],
                "{mode:?}"
            );
        }
        assert_eq!(run(&mut e, &mut t, Op::SetMode(Mode::Usb)).0, b"MODUSB\r");
        assert_eq!(t.snapshot().expect("snapshot").frequency, Hz(14_200_000));
        t.front_panel(Panel::Frequency(Hz(5_000_000)))
            .expect("tunes");
        assert_eq!(t.feed(b"FRQ?\r", T0), b"F5000.000000\r");
        assert!(t.front_panel(Panel::Ptt(true)).is_err());
        assert!(t.feed(b"???\r", T0).is_empty());
        assert!(e.encode(&Op::SetPtt(true)).is_err());
        assert!(e.encode(&Op::SetMode(Mode::Wfm)).is_err());
        e.encode(&Op::GetFrequency(Vfo::A)).expect("encodes");
        assert!(matches!(
            e.feed(b"Fabc\r", T0).as_slice(),
            [Inbound::Unrecognised(_)]
        ));
        e.encode(&Op::GetMode).expect("encodes");
        assert!(matches!(
            e.feed(b"MXYZ\r", T0).as_slice(),
            [Inbound::Unrecognised(_)]
        ));
    }
}
