// SPDX-License-Identifier: AGPL-3.0-or-later
//! ADAT ADT-200A transceiver: sans-IO client engine and virtual twin.
//!
//! **Sources.** Facts only, read from Hamlib 4.6.2 (released tarball, sha256
//! b2ac73f44dd1161e95fdee6c95276144757647bf92d7fdb369ee2fe41ed47ae8): `rigs/adat/adat.c`,
//! `adat.h` and `adt_200a.c`. No ADAT document was read; nothing is verified against a radio.
//!
//! Commands start with `$` and end with CR. `$FR1:<Hz>` sets the frequency and `$MOD:<nn>` the
//! mode (two decimal digits: 0 CW-R, 1 CW, 2 LSB, 3 USB, 5 AM, 6 AM-SL, 7 AM-SU, 8 FM);
//! `$MOX>` and `$MOX<` key and unkey. None of them is answered. `$FRA?`, `$MOD?` and `$MTR?`
//! read the frequency, mode and transmit state. Hamlib takes everything after the first space
//! of the answer line as the value: the frequency as `<vfo> <number><unit>` (Hz, kHz, MHz or
//! GHz; VFO 0 means no VFO is active), the mode as its name (`USB`, `CW-R`, `AM-SL`, ...), the
//! transmit state as 0 or 1. What comes before the first space is not examined, so the engine
//! ignores it too; the exact echo is unknown.

use std::time::Duration;

use radioctl_core::description::RigDescription;
use radioctl_core::engine::{Inbound, ProtocolEngine, ReplyKind, Request};
use radioctl_core::error::{ErrorKind, RigError};
use radioctl_core::op::{Field, Observation, Op};
use radioctl_core::transcript::Panel;
use radioctl_core::units::{Hz, Vfo};

use crate::serial_vendor::{parse_decimal, vendor_of, RxBuffer, VendorTwin};

/// The radio's mode names by wire code.
const MODE_NAMES: [(u8, &str); 8] = [
    (0, "CW-R"),
    (1, "CW"),
    (2, "LSB"),
    (3, "USB"),
    (5, "AM"),
    (6, "AM-SL"),
    (7, "AM-SU"),
    (8, "FM"),
];

fn unsupported(what: &str) -> RigError {
    RigError::new(ErrorKind::Unsupported, what.to_string())
}

// [@ANCHOR: radioctl_adat_mode_code_of_name]
/// The wire code of a mode name, or `None`.
pub fn mode_code_of_name(name: &[u8]) -> Option<u8> {
    MODE_NAMES
        .iter()
        .find(|(_, n)| n.as_bytes() == name)
        .map(|(c, _)| *c)
}

// [@ANCHOR: radioctl_adat_parse_frequency]
/// The frequency of a number with a unit (`14.2MHz`, `7100.5kHz`, `800Hz`, `1.296GHz`), exact to
/// the hertz, or `None` when malformed or finer than a hertz.
pub fn parse_frequency(text: &[u8]) -> Option<Hz> {
    let split = text.iter().position(u8::is_ascii_alphabetic)?;
    let (number, unit) = text.split_at(split);
    let scale: u64 = match unit {
        b"Hz" => 1,
        b"kHz" => 1_000,
        b"MHz" => 1_000_000,
        b"GHz" => 1_000_000_000,
        _ => return None,
    };
    let (whole, frac) = match number.iter().position(|b| *b == b'.') {
        Some(i) => (number.get(..i)?, number.get(i + 1..)?),
        None => (number, &b""[..]),
    };
    let w = parse_decimal(whole)?;
    let mut hz = w.checked_mul(scale)?;
    let mut place = scale;
    for digit in frac {
        if !digit.is_ascii_digit() {
            return None;
        }
        let d = u64::from(digit - b'0');
        if !place.is_multiple_of(10) {
            if d != 0 {
                return None;
            }
            continue;
        }
        place /= 10;
        hz = hz.checked_add(d * place)?;
    }
    Some(Hz(hz))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pending {
    Nothing,
    Frequency,
    Mode,
    Ptt,
}

/// The ADT-200A client engine.
#[derive(Debug)]
pub struct AdatEngine {
    desc: RigDescription,
    pending: Pending,
    rx: RxBuffer,
}

impl AdatEngine {
    // [@ANCHOR: radioctl_adat_engine_new]
    /// Builds an engine from an ADAT vendor description.
    pub fn new(desc: RigDescription) -> Result<AdatEngine, RigError> {
        let (_, dialect) = vendor_of(&desc)?;
        if dialect != "adt200a" {
            return Err(unsupported("unknown ADAT dialect in the description"));
        }
        Ok(AdatEngine {
            desc,
            pending: Pending::Nothing,
            rx: RxBuffer::default(),
        })
    }
}

/// What follows the first space of a reply line, with surrounding blanks removed.
fn value_of(line: &[u8]) -> Option<&[u8]> {
    let line = line.strip_prefix(&[0u8]).unwrap_or(line);
    let space = line.iter().position(|b| *b == b' ')?;
    Some(line[space + 1..].trim_ascii())
}

impl ProtocolEngine for AdatEngine {
    fn description(&self) -> &RigDescription {
        &self.desc
    }

    // [@ANCHOR: radioctl_adat_engine_encode]
    fn encode(&mut self, op: &Op) -> Result<Request, RigError> {
        if !self.desc.capabilities.has(op.required_capability()) {
            return Err(unsupported("capability not in the rig description"));
        }
        let (text, reply, pending) = match op {
            Op::GetFrequency(Vfo::A) => (
                "$FRA?\r".to_string(),
                ReplyKind::Observation(Field::Frequency(Vfo::A)),
                Pending::Frequency,
            ),
            Op::SetFrequency(Vfo::A, hz) => (
                format!("$FR1:{}\r", hz.0),
                ReplyKind::None,
                Pending::Nothing,
            ),
            Op::GetMode => (
                "$MOD?\r".to_string(),
                ReplyKind::Observation(Field::Mode),
                Pending::Mode,
            ),
            Op::SetMode(mode) => {
                let code = self
                    .desc
                    .mode_code(*mode)
                    .ok_or_else(|| unsupported("mode not in the description"))?;
                (
                    format!("$MOD:{code:02}\r"),
                    ReplyKind::None,
                    Pending::Nothing,
                )
            }
            Op::SetPtt(on) => (
                format!("$MOX{}\r", if *on { '>' } else { '<' }),
                ReplyKind::None,
                Pending::Nothing,
            ),
            Op::GetPtt => (
                "$MTR?\r".to_string(),
                ReplyKind::Observation(Field::Ptt),
                Pending::Ptt,
            ),
            _ => return Err(unsupported("not implemented for the ADAT engine")),
        };
        self.pending = pending;
        self.rx.take_all();
        Ok(Request {
            bytes: text.into_bytes(),
            reply,
        })
    }

    // [@ANCHOR: radioctl_adat_engine_feed]
    fn feed(&mut self, bytes: &[u8], now: Duration) -> Vec<Inbound> {
        let mut out = Vec::new();
        if let Some(stale) = self.rx.push(bytes, now) {
            out.push(Inbound::Unrecognised(stale));
        }
        while let Some(mut line) = self.rx.take_line(b'\n') {
            if line.last() == Some(&b'\r') {
                line.pop();
            }
            if line.is_empty() {
                continue;
            }
            let pending = std::mem::replace(&mut self.pending, Pending::Nothing);
            let value = value_of(&line);
            let seen = match (pending, value) {
                (Pending::Frequency, Some(v)) => {
                    // `<vfo> <number><unit>`; VFO 0 is "no VFO active".
                    let mut parts = v.splitn(2, |b| *b == b' ');
                    let vfo = parts.next().and_then(parse_decimal);
                    let freq = parts.next().map(<[u8]>::trim_ascii);
                    match (vfo, freq.and_then(parse_frequency)) {
                        (Some(n), Some(hz)) if n != 0 => Some(Observation::Frequency(Vfo::A, hz)),
                        _ => None,
                    }
                }
                (Pending::Mode, Some(v)) => mode_code_of_name(v)
                    .and_then(|c| self.desc.code_mode(c))
                    .map(Observation::Mode),
                (Pending::Ptt, Some(v)) => match v {
                    b"0" => Some(Observation::Ptt(false)),
                    b"1" => Some(Observation::Ptt(true)),
                    _ => None,
                },
                _ => None,
            };
            out.push(match seen {
                Some(o) => Inbound::Observation(o),
                None => Inbound::Unrecognised(line),
            });
        }
        out
    }
}

/// A virtual ADT-200A.
#[derive(Debug)]
pub struct AdatTwin {
    rx: RxBuffer,
    hz: Hz,
    mode: u8,
    ptt: bool,
    codes: Vec<u8>,
}

impl AdatTwin {
    // [@ANCHOR: radioctl_adat_twin_new]
    /// A twin in the description's default state.
    pub fn new(desc: &RigDescription) -> Result<AdatTwin, RigError> {
        vendor_of(desc)?;
        Ok(AdatTwin {
            rx: RxBuffer::default(),
            hz: desc.default_frequency,
            mode: desc.mode_code(desc.default_mode).unwrap_or(3),
            ptt: false,
            codes: desc.mode_codes.iter().map(|(_, c)| *c).collect(),
        })
    }
}

impl VendorTwin for AdatTwin {
    // [@ANCHOR: radioctl_adat_twin_feed]
    fn feed(&mut self, bytes: &[u8], now: Duration) -> Vec<u8> {
        self.rx.push(bytes, now);
        let mut out = Vec::new();
        while let Some(line) = self.rx.take_line(b'\r') {
            match line.as_slice() {
                b"$FRA?" => out.extend(format!("$FRA? 1 {}Hz\r\n", self.hz.0).into_bytes()),
                b"$MOD?" => {
                    let name = MODE_NAMES
                        .iter()
                        .find(|(c, _)| *c == self.mode)
                        .map_or("USB", |(_, n)| n);
                    out.extend(format!("$MOD? {name}\r\n").into_bytes());
                }
                b"$MTR?" => out.extend(format!("$MTR? {}\r\n", u8::from(self.ptt)).into_bytes()),
                b"$MOX>" => self.ptt = true,
                b"$MOX<" => self.ptt = false,
                [b'$', b'F', b'R', b'1', b':', digits @ ..] => {
                    if let Some(hz) = parse_decimal(digits) {
                        self.hz = Hz(hz);
                    }
                }
                [b'$', b'M', b'O', b'D', b':', digits @ ..] => {
                    if let Some(code) = parse_decimal(digits).and_then(|c| u8::try_from(c).ok()) {
                        if self.codes.contains(&code) {
                            self.mode = code;
                        }
                    }
                }
                _ => {}
            }
        }
        out
    }

    // [@ANCHOR: radioctl_adat_twin_front_panel]
    fn front_panel(&mut self, panel: Panel) -> Result<Vec<u8>, String> {
        match panel {
            Panel::Frequency(hz) => self.hz = hz,
            Panel::Ptt(on) => self.ptt = on,
            other => return Err(format!("an ADT-200A twin has no {other:?} knob")),
        }
        Ok(Vec::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use radioctl_core::units::Mode;

    const T0: Duration = Duration::from_secs(1);

    fn shipped() -> RigDescription {
        let path = format!(
            "{}/../../rigs/adat-adt-200a.toml",
            env!("CARGO_MANIFEST_DIR")
        );
        let text = std::fs::read_to_string(path).expect("rig file");
        radioctl_core::rigfile::parse(&text)
            .expect("loads")
            .description
    }

    // Tests [@ANCHOR: radioctl_adat_parse_frequency]
    // Tests [@ANCHOR: radioctl_adat_mode_code_of_name]
    #[test]
    fn units_and_mode_names_decode() {
        assert_eq!(parse_frequency(b"14200000Hz"), Some(Hz(14_200_000)));
        assert_eq!(parse_frequency(b"14.2MHz"), Some(Hz(14_200_000)));
        assert_eq!(parse_frequency(b"7100.5kHz"), Some(Hz(7_100_500)));
        assert_eq!(parse_frequency(b"1.296GHz"), Some(Hz(1_296_000_000)));
        assert_eq!(parse_frequency(b"800Hz"), Some(Hz(800)));
        assert_eq!(parse_frequency(b"7.0000001MHz"), None);
        assert_eq!(parse_frequency(b"7.5Hz"), None);
        assert_eq!(parse_frequency(b"7THz"), None);
        assert_eq!(parse_frequency(b"7"), None);
        assert_eq!(mode_code_of_name(b"CW-R"), Some(0));
        assert_eq!(mode_code_of_name(b"AM-SU"), Some(7));
        assert_eq!(mode_code_of_name(b"RTTY"), None);
    }

    // Tests [@ANCHOR: radioctl_adat_engine_new]
    #[test]
    fn engine_refuses_other_families() {
        assert!(AdatEngine::new(RigDescription::ts2000()).is_err());
        assert!(AdatTwin::new(&RigDescription::ts2000()).is_err());
    }

    // Tests [@ANCHOR: radioctl_adat_engine_encode]
    // Tests [@ANCHOR: radioctl_adat_engine_feed]
    // Tests [@ANCHOR: radioctl_adat_twin_new]
    // Tests [@ANCHOR: radioctl_adat_twin_feed]
    // Tests [@ANCHOR: radioctl_adat_twin_front_panel]
    #[test]
    fn the_engine_round_trips_against_the_twin() {
        let desc = shipped();
        let mut e = AdatEngine::new(desc.clone()).expect("engine");
        let mut t = AdatTwin::new(&desc).expect("twin");
        let run = |e: &mut AdatEngine, t: &mut AdatTwin, op: Op| {
            let r = e.encode(&op).expect("encodes");
            let a = t.feed(&r.bytes, T0);
            (r.bytes, e.feed(&a, T0))
        };
        let (bytes, seen) = run(&mut e, &mut t, Op::SetFrequency(Vfo::A, Hz(14_200_000)));
        assert_eq!(bytes, b"$FR1:14200000\r");
        assert!(seen.is_empty());
        assert_eq!(
            run(&mut e, &mut t, Op::GetFrequency(Vfo::A)).1,
            vec![Inbound::Observation(Observation::Frequency(
                Vfo::A,
                Hz(14_200_000)
            ))]
        );
        for (mode, wire) in [
            (Mode::CwR, "$MOD:00\r"),
            (Mode::Cw, "$MOD:01\r"),
            (Mode::Lsb, "$MOD:02\r"),
            (Mode::Usb, "$MOD:03\r"),
            (Mode::Am, "$MOD:05\r"),
            (Mode::Fm, "$MOD:08\r"),
        ] {
            assert_eq!(run(&mut e, &mut t, Op::SetMode(mode)).0, wire.as_bytes());
            assert_eq!(
                run(&mut e, &mut t, Op::GetMode).1,
                vec![Inbound::Observation(Observation::Mode(mode))],
                "{mode:?}"
            );
        }
        assert_eq!(run(&mut e, &mut t, Op::SetPtt(true)).0, b"$MOX>\r");
        assert_eq!(
            run(&mut e, &mut t, Op::GetPtt).1,
            vec![Inbound::Observation(Observation::Ptt(true))]
        );
        assert_eq!(run(&mut e, &mut t, Op::SetPtt(false)).0, b"$MOX<\r");
        assert_eq!(
            run(&mut e, &mut t, Op::GetPtt).1,
            vec![Inbound::Observation(Observation::Ptt(false))]
        );
        assert!(e.encode(&Op::SetMode(Mode::Wfm)).is_err());
        assert!(e.encode(&Op::GetSignal).is_err());
        t.front_panel(Panel::Frequency(Hz(5_000_000)))
            .expect("tunes");
        assert_eq!(t.feed(b"$FRA?\r", T0), b"$FRA? 1 5000000Hz\r\n");
        t.front_panel(Panel::Ptt(true)).expect("keys");
        assert_eq!(t.feed(b"$MTR?\r", T0), b"$MTR? 1\r\n");
        assert!(t.front_panel(Panel::Signal(3)).is_err());
        // VFO 0 (no VFO active), unknown modes and a reply with no space are not understood.
        e.encode(&Op::GetFrequency(Vfo::A)).expect("get");
        assert!(matches!(
            e.feed(b"$FRA? 0 5MHz\r\n", T0).as_slice(),
            [Inbound::Unrecognised(_)]
        ));
        e.encode(&Op::GetMode).expect("get");
        assert!(matches!(
            e.feed(b"$MOD? AM-SL\r\n", T0).as_slice(),
            [Inbound::Unrecognised(_)]
        ));
        e.encode(&Op::GetPtt).expect("get");
        assert!(matches!(
            e.feed(b"junk\r\n", T0).as_slice(),
            [Inbound::Unrecognised(_)]
        ));
        // A reply that arrives with a leading NUL byte, as Hamlib's comments describe, still decodes.
        e.encode(&Op::GetPtt).expect("get");
        assert_eq!(
            e.feed(b"\x00$MTR? 1\r\n", T0),
            vec![Inbound::Observation(Observation::Ptt(true))]
        );
    }
}
