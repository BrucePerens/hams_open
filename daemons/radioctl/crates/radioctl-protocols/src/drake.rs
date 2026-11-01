// SPDX-License-Identifier: AGPL-3.0-or-later
//! Drake R-8A and R-8B receivers: sans-IO client engine and virtual twin.
//!
//! **Sources.** Facts only, read from Hamlib 4.6.2 (released tarball, sha256
//! b2ac73f44dd1161e95fdee6c95276144757647bf92d7fdb369ee2fe41ed47ae8): `rigs/drake/drake.c`,
//! `r8a.c`, `r8b.c`. No Drake document was read; nothing here is verified against a radio.
//!
//! * Commands end with CR; replies end with LF.
//! * `F<7 digits>` CR sets the frequency in units of 10 Hz; `RF` CR reads it, answered with a
//!   15-byte line `*fffff.ff*uHz` CR LF (digits in kHz, or in MHz when the unit letter `u` is `m`).
//! * `M<c>` CR sets the mode (1 USB, 2 LSB, 3 RTTY, 4 CW, 5 FM, 6 AM, from the description);
//!   `RM` CR reads the state in an 8-byte line whose bytes 3, 4 and 5 are mode, width and
//!   synchronous-detector flags, each an ASCII digit with flag bits. A width byte of `0` to `4`
//!   (after masking) with bit 3 clear selects the lower-sideband family (0 LSB, 1 RTTY, 2 FM),
//!   with bit 3 set the upper family (0 USB, 1 CW, 2 AM).
//! * Hamlib reads one line after every setting; its content is not examined there, so any
//!   LF-ended line is taken as the acknowledgement.
//! * No PTT: these are receivers.

use std::time::Duration;

use radioctl_core::description::RigDescription;
use radioctl_core::engine::{Inbound, ProtocolEngine, ReplyKind, Request};
use radioctl_core::error::{ErrorKind, RigError};
use radioctl_core::op::{Field, Observation, Op};
use radioctl_core::transcript::Panel;
use radioctl_core::units::{Hz, Mode, Vfo};

use crate::serial_vendor::{vendor_of, RxBuffer, VendorTwin};

const CR: u8 = b'\r';
const LF: u8 = b'\n';
/// Largest value of the seven-digit field, in units of 10 Hz.
const MAX_UNITS: u64 = 9_999_999;

fn unsupported(what: &str) -> RigError {
    RigError::new(ErrorKind::Unsupported, what.to_string())
}

// [@ANCHOR: radioctl_drake_encode_frequency]
/// `F` and seven digits of 10 Hz units, CR; `None` unless a multiple of 10 Hz below 100 MHz.
pub fn encode_frequency(hz: Hz) -> Option<Vec<u8>> {
    if !hz.0.is_multiple_of(10) || hz.0 / 10 > MAX_UNITS {
        return None;
    }
    Some(format!("F{:07}\r", hz.0 / 10).into_bytes())
}

// [@ANCHOR: radioctl_drake_decode_frequency]
/// The frequency of a `*fffff.ff*uHz` line (without CR LF), or `None`.
pub fn decode_frequency(line: &[u8]) -> Option<Hz> {
    let [b'*', number @ .., b'*', unit, b'H', b'z'] = line else {
        return None;
    };
    let (whole, frac) = match number {
        [w @ .., b'.', f1, f2] => (w, [*f1, *f2]),
        _ => return None,
    };
    let digits = crate::serial_vendor::parse_decimal(whole)?;
    let fraction = crate::serial_vendor::parse_decimal(&frac)?;
    let hundredths = digits.checked_mul(100)?.checked_add(fraction)?;
    let scale = if matches!(unit, b'm' | b'M') {
        10_000
    } else {
        10
    };
    Some(Hz(hundredths.checked_mul(scale)?))
}

// [@ANCHOR: radioctl_drake_decode_mode]
/// The mode of the mode, width and synchronous bytes of an `RM` line, per `description`.
pub fn decode_mode(mode: u8, width: u8) -> Option<Mode> {
    if !(0x30..=0x34).contains(&(width & 0x37)) {
        return None;
    }
    let lower = (b'0'..=b'4').contains(&width);
    match (lower, mode & 0x33) {
        (true, b'0') => Some(Mode::Lsb),
        (true, b'1') => Some(Mode::Rtty),
        (true, b'2') => Some(Mode::Fm),
        (false, b'0') => Some(Mode::Usb),
        (false, b'1') => Some(Mode::Cw),
        (false, b'2') => Some(Mode::Am),
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

/// The Drake client engine.
#[derive(Debug)]
pub struct DrakeEngine {
    desc: RigDescription,
    pending: Pending,
    rx: RxBuffer,
}

impl DrakeEngine {
    // [@ANCHOR: radioctl_drake_engine_new]
    /// Builds an engine from a Drake vendor description.
    pub fn new(desc: RigDescription) -> Result<DrakeEngine, RigError> {
        let (_, dialect) = vendor_of(&desc)?;
        if dialect != "r8" {
            return Err(unsupported("unknown Drake dialect in the description"));
        }
        Ok(DrakeEngine {
            desc,
            pending: Pending::Nothing,
            rx: RxBuffer::default(),
        })
    }
}

impl ProtocolEngine for DrakeEngine {
    fn description(&self) -> &RigDescription {
        &self.desc
    }

    // [@ANCHOR: radioctl_drake_engine_encode]
    fn encode(&mut self, op: &Op) -> Result<Request, RigError> {
        if !self.desc.capabilities.has(op.required_capability()) {
            return Err(unsupported("capability not in the rig description"));
        }
        let (bytes, reply, pending) = match op {
            Op::GetFrequency(Vfo::A) => (
                b"RF\r".to_vec(),
                ReplyKind::Observation(Field::Frequency(Vfo::A)),
                Pending::Frequency,
            ),
            Op::SetFrequency(Vfo::A, hz) => (
                encode_frequency(*hz).ok_or_else(|| {
                    unsupported("frequency must be a multiple of 10 Hz below 100 MHz")
                })?,
                ReplyKind::Ack,
                Pending::Ack,
            ),
            Op::GetMode => (
                b"RM\r".to_vec(),
                ReplyKind::Observation(Field::Mode),
                Pending::Mode,
            ),
            Op::SetMode(mode) => {
                let code = self
                    .desc
                    .mode_code(*mode)
                    .ok_or_else(|| unsupported("mode not in the description"))?;
                (vec![b'M', code, CR], ReplyKind::Ack, Pending::Ack)
            }
            _ => return Err(unsupported("not implemented for the Drake engine")),
        };
        self.pending = pending;
        self.rx.take_all();
        Ok(Request { bytes, reply })
    }

    // [@ANCHOR: radioctl_drake_engine_feed]
    fn feed(&mut self, bytes: &[u8], now: Duration) -> Vec<Inbound> {
        let mut out = Vec::new();
        if let Some(stale) = self.rx.push(bytes, now) {
            out.push(Inbound::Unrecognised(stale));
        }
        while let Some(line) = self.rx.take_line(LF) {
            let pending = std::mem::replace(&mut self.pending, Pending::Nothing);
            let body = line.strip_suffix(&[CR]).unwrap_or(&line);
            out.extend(match (pending, body) {
                (Pending::Ack, _) => vec![Inbound::Ack],
                (Pending::Frequency, b) => match decode_frequency(b) {
                    Some(hz) => vec![Inbound::Observation(Observation::Frequency(Vfo::A, hz))],
                    None => vec![Inbound::Unrecognised(line.clone())],
                },
                (Pending::Mode, [_, _, _, mode, width, _]) => match decode_mode(*mode, *width) {
                    Some(m) if self.desc.mode_code(m).is_some() => {
                        vec![Inbound::Observation(Observation::Mode(m))]
                    }
                    _ => vec![Inbound::Unrecognised(line.clone())],
                },
                _ => vec![Inbound::Unrecognised(line.clone())],
            });
        }
        out
    }
}

/// A virtual Drake receiver.
#[derive(Debug)]
pub struct DrakeTwin {
    rx: RxBuffer,
    units: u64,
    mode: Mode,
    codes: Vec<(Mode, u8)>,
}

impl DrakeTwin {
    // [@ANCHOR: radioctl_drake_twin_new]
    /// A twin in the description's default state.
    pub fn new(desc: &RigDescription) -> Result<DrakeTwin, RigError> {
        vendor_of(desc)?;
        Ok(DrakeTwin {
            rx: RxBuffer::default(),
            units: desc.default_frequency.0 / 10,
            mode: desc.default_mode,
            codes: desc.mode_codes.clone(),
        })
    }

    // [@ANCHOR: radioctl_drake_twin_state_line]
    /// The `RM` answer: three filler bytes, then mode, width and synchronous bytes.
    fn state_line(&self) -> Vec<u8> {
        let (mode_byte, width) = match self.mode {
            Mode::Lsb => (b'0', b'1'),
            Mode::Rtty => (b'1', b'1'),
            Mode::Fm => (b'2', b'1'),
            Mode::Usb => (b'0', b'9'),
            Mode::Cw => (b'1', b'9'),
            _ => (b'2', b'9'),
        };
        vec![b'R', b'M', b' ', mode_byte, width, b'0', CR, LF]
    }
}

impl VendorTwin for DrakeTwin {
    // [@ANCHOR: radioctl_drake_twin_feed]
    fn feed(&mut self, bytes: &[u8], now: Duration) -> Vec<u8> {
        self.rx.push(bytes, now);
        let mut out = Vec::new();
        while let Some(line) = self.rx.take_line(CR) {
            let ack = vec![CR, LF];
            out.extend(match line.as_slice() {
                b"RF" => {
                    let hundredths = self.units;
                    format!("*{:05}.{:02}*kHz\r\n", hundredths / 100, hundredths % 100).into_bytes()
                }
                b"RM" => self.state_line(),
                [b'F', digits @ ..] if digits.len() == 7 => {
                    match crate::serial_vendor::parse_decimal(digits) {
                        Some(u) => {
                            self.units = u;
                            ack
                        }
                        None => b"?\r\n".to_vec(),
                    }
                }
                [b'M', c] => match self.codes.iter().find(|(_, code)| code == c) {
                    Some((m, _)) => {
                        self.mode = *m;
                        ack
                    }
                    None => b"?\r\n".to_vec(),
                },
                _ => b"?\r\n".to_vec(),
            });
        }
        out
    }

    // [@ANCHOR: radioctl_drake_twin_front_panel]
    fn front_panel(&mut self, panel: Panel) -> Result<Vec<u8>, String> {
        match panel {
            Panel::Frequency(hz) => self.units = hz.0 / 10,
            Panel::Mode(mode) => self.mode = mode,
            other => return Err(format!("a Drake twin has no {other:?} knob")),
        }
        Ok(Vec::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const T0: Duration = Duration::from_secs(1);

    fn shipped(id: &str) -> RigDescription {
        let path = format!("{}/../../rigs/{id}.toml", env!("CARGO_MANIFEST_DIR"));
        let text = std::fs::read_to_string(path).expect("rig file");
        radioctl_core::rigfile::parse(&text)
            .expect("loads")
            .description
    }

    // Tests [@ANCHOR: radioctl_drake_encode_frequency]
    // Tests [@ANCHOR: radioctl_drake_decode_frequency]
    #[test]
    fn frequency_is_seven_digits_of_ten_hertz_and_a_star_line_back() {
        assert_eq!(
            encode_frequency(Hz(14_200_000)),
            Some(b"F1420000\r".to_vec())
        );
        assert_eq!(encode_frequency(Hz(14_200_005)), None);
        assert_eq!(encode_frequency(Hz(100_000_000)), None);
        assert_eq!(decode_frequency(b"*14200.00*kHz"), Some(Hz(14_200_000)));
        assert_eq!(decode_frequency(b"*00010.05*kHz"), Some(Hz(10_050)));
        assert_eq!(decode_frequency(b"*00014.20*mHz"), Some(Hz(14_200_000)));
        assert_eq!(decode_frequency(b"14200.00"), None);
        assert_eq!(decode_frequency(b"*14200,00*kHz"), None);
    }

    // Tests [@ANCHOR: radioctl_drake_decode_mode]
    #[test]
    fn mode_bytes_split_into_two_families_by_the_width_byte() {
        assert_eq!(decode_mode(b'0', b'1'), Some(Mode::Lsb));
        assert_eq!(decode_mode(b'1', b'3'), Some(Mode::Rtty));
        assert_eq!(decode_mode(b'2', b'0'), Some(Mode::Fm));
        assert_eq!(decode_mode(b'0', b'9'), Some(Mode::Usb));
        assert_eq!(decode_mode(b'1', b'9'), Some(Mode::Cw));
        assert_eq!(decode_mode(b'2', b'<'), Some(Mode::Am));
        assert_eq!(decode_mode(b'3', b'1'), None);
        assert_eq!(decode_mode(b'0', b'5'), None);
    }

    // Tests [@ANCHOR: radioctl_drake_engine_new]
    #[test]
    fn engine_refuses_other_families() {
        assert!(DrakeEngine::new(RigDescription::ts2000()).is_err());
        assert!(DrakeTwin::new(&RigDescription::ts2000()).is_err());
    }

    // Tests [@ANCHOR: radioctl_drake_engine_encode]
    // Tests [@ANCHOR: radioctl_drake_engine_feed]
    // Tests [@ANCHOR: radioctl_drake_twin_new]
    // Tests [@ANCHOR: radioctl_drake_twin_feed]
    // Tests [@ANCHOR: radioctl_drake_twin_state_line]
    // Tests [@ANCHOR: radioctl_drake_twin_front_panel]
    #[test]
    fn both_models_round_trip_against_the_twin() {
        for id in ["drake-r-8a", "drake-r-8b"] {
            let desc = shipped(id);
            let mut e = DrakeEngine::new(desc.clone()).expect("engine");
            let mut t = DrakeTwin::new(&desc).expect("twin");
            let mut ask = |e: &mut DrakeEngine, op: Op| {
                let r = e.encode(&op).expect("encodes");
                let answer = t.feed(&r.bytes, T0);
                e.feed(&answer, T0)
            };
            assert_eq!(
                ask(&mut e, Op::SetFrequency(Vfo::A, Hz(7_074_000))),
                vec![Inbound::Ack],
                "{id}"
            );
            assert_eq!(
                ask(&mut e, Op::GetFrequency(Vfo::A)),
                vec![Inbound::Observation(Observation::Frequency(
                    Vfo::A,
                    Hz(7_074_000)
                ))]
            );
            for mode in [
                Mode::Lsb,
                Mode::Usb,
                Mode::Cw,
                Mode::Am,
                Mode::Fm,
                Mode::Rtty,
            ] {
                assert_eq!(
                    ask(&mut e, Op::SetMode(mode)),
                    vec![Inbound::Ack],
                    "{id} {mode:?}"
                );
                assert_eq!(
                    ask(&mut e, Op::GetMode),
                    vec![Inbound::Observation(Observation::Mode(mode))],
                    "{id} {mode:?}"
                );
            }
            assert!(e.encode(&Op::SetPtt(true)).is_err());
            assert!(e.encode(&Op::GetFrequency(Vfo::B)).is_err());
            assert!(e.encode(&Op::SetFrequency(Vfo::A, Hz(7_074_001))).is_err());
            assert!(e.encode(&Op::SetMode(Mode::Wfm)).is_err());
            assert!(t.front_panel(Panel::Split(true)).is_err());
            t.front_panel(Panel::Frequency(Hz(10_000_000)))
                .expect("tunes");
            t.front_panel(Panel::Mode(Mode::Usb)).expect("mode");
            assert_eq!(t.feed(b"RF\r", T0), b"*10000.00*kHz\r\n");
            assert_eq!(t.feed(b"X\r", T0), b"?\r\n");
        }
        let mut e = DrakeEngine::new(shipped("drake-r-8a")).expect("engine");
        e.encode(&Op::GetFrequency(Vfo::A)).expect("get");
        assert!(matches!(
            e.feed(b"junk\r\n", T0).as_slice(),
            [Inbound::Unrecognised(_)]
        ));
        e.encode(&Op::GetMode).expect("get");
        assert!(matches!(
            e.feed(b"abcdef\r\n", T0).as_slice(),
            [Inbound::Unrecognised(_)]
        ));
    }
}
