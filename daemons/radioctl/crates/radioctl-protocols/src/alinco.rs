// SPDX-License-Identifier: AGPL-3.0-or-later
//! Alinco DX-77 and DX-SR8 transceivers: sans-IO client engine and virtual twin.
//!
//! **Sources.** Facts only, read from Hamlib 4.6.2 (released tarball, sha256
//! b2ac73f44dd1161e95fdee6c95276144757647bf92d7fdb369ee2fe41ed47ae8): `rigs/alinco/dx77.c`,
//! `dxsr8.c`, `alinco.h`. No Alinco document was read (Hamlib's comments name a DX-77 software
//! manual and a blog post on the DX-SR8, neither fetched); nothing is verified against a radio.
//!
//! Both radios echo each command as a line, then answer with a second line: `OK` for a setting,
//! the value for a read. Lines end with LF.
//!
//! * `dx77`: commands start `AL` and end with CR. `AL0B<digits>` sets the receive frequency in
//!   hertz (at least six digits), `AL2G<c>` the mode, `AL3H` reads a 26-character status line:
//!   characters 0-1 hexadecimal settings, 3 the mode character, 6-15 the receive frequency.
//!   Mode characters: 0 LSB, 1 USB, 2 CW (lower), 3 CW (upper), 4 AM, 5 FM. Hamlib's DX-77 has
//!   no PTT.
//! * `dxsr8`: commands are `AL~` and a verb, ended by CR LF: `RW_RXF<8 digits>` and `RR_RXF`
//!   (frequency in hertz), `RW_RFM<2 digits>` and `RR_RFM` (mode 0 USB, 1 LSB, 2 CW upper, 3 CW
//!   lower, 4 AM, 5 FM), `RW_PTT<2 digits>` and `RR_PTT`. Hamlib reports both CW codes as CW; this
//!   engine reports code 3 as CW-R when the description lists it.

use std::time::Duration;

use radioctl_core::description::RigDescription;
use radioctl_core::engine::{Inbound, ProtocolEngine, ReplyKind, Request};
use radioctl_core::error::{ErrorKind, RigError};
use radioctl_core::op::{Field, Observation, Op};
use radioctl_core::transcript::Panel;
use radioctl_core::units::{Hz, Vfo};

use crate::serial_vendor::{parse_decimal, vendor_of, RxBuffer, TwinSnapshot, VendorTwin};

const LF: u8 = b'\n';

fn unsupported(what: &str) -> RigError {
    RigError::new(ErrorKind::Unsupported, what.to_string())
}

/// The two Alinco dialects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dialect {
    /// DX-77: `AL` commands ended by CR, one 26-character status line.
    Dx77,
    /// DX-SR8: `AL~` commands ended by CR LF.
    Dxsr8,
}

impl Dialect {
    // [@ANCHOR: radioctl_alinco_dialect_from_name]
    /// The dialect with a file-format name.
    pub fn from_name(name: &str) -> Option<Dialect> {
        match name {
            "dx77" => Some(Dialect::Dx77),
            "dxsr8" => Some(Dialect::Dxsr8),
            _ => None,
        }
    }

    // [@ANCHOR: radioctl_alinco_dialect_end]
    fn end(self) -> &'static [u8] {
        match self {
            Dialect::Dx77 => b"\r",
            Dialect::Dxsr8 => b"\r\n",
        }
    }
}

// [@ANCHOR: radioctl_alinco_decode_status]
/// The mode character and receive frequency of a DX-77 status line (26 characters).
pub fn decode_status(line: &[u8]) -> Option<(u8, Hz)> {
    if line.len() != 26 {
        return None;
    }
    let mode = *line.get(3)?;
    let hz = parse_decimal(line.get(6..16)?)?;
    Some((mode, Hz(hz)))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pending {
    Nothing,
    Set,
    Read(Field),
}

/// The Alinco client engine.
#[derive(Debug)]
pub struct AlincoEngine {
    desc: RigDescription,
    dialect: Dialect,
    pending: Pending,
    rx: RxBuffer,
    echoed: bool,
}

impl AlincoEngine {
    // [@ANCHOR: radioctl_alinco_engine_new]
    /// Builds an engine from an Alinco vendor description.
    pub fn new(desc: RigDescription) -> Result<AlincoEngine, RigError> {
        let (_, name) = vendor_of(&desc)?;
        let dialect = Dialect::from_name(name)
            .ok_or_else(|| unsupported("unknown Alinco dialect in the description"))?;
        Ok(AlincoEngine {
            desc,
            dialect,
            pending: Pending::Nothing,
            rx: RxBuffer::default(),
            echoed: false,
        })
    }

    // [@ANCHOR: radioctl_alinco_command]
    fn command(&self, text: &str) -> Vec<u8> {
        let mut bytes = text.as_bytes().to_vec();
        bytes.extend_from_slice(self.dialect.end());
        bytes
    }

    // [@ANCHOR: radioctl_alinco_decode_reply]
    fn decode_reply(&self, field: Field, line: &[u8]) -> Option<Observation> {
        match (self.dialect, field) {
            (Dialect::Dx77, Field::Frequency(v)) => {
                decode_status(line).map(|(_, hz)| Observation::Frequency(v, hz))
            }
            (Dialect::Dx77, Field::Mode) => {
                let (code, _) = decode_status(line)?;
                // Code 2 is the lower CW setting; the description lists the upper one.
                let code = if code == b'2' { b'3' } else { code };
                self.desc.code_mode(code).map(Observation::Mode)
            }
            (Dialect::Dxsr8, Field::Frequency(v)) => {
                parse_decimal(line).map(|hz| Observation::Frequency(v, Hz(hz)))
            }
            (Dialect::Dxsr8, Field::Mode) => {
                let code = u8::try_from(parse_decimal(line)?).ok()?;
                self.desc.code_mode(code).map(Observation::Mode)
            }
            (Dialect::Dxsr8, Field::Ptt) => parse_decimal(line).map(|v| Observation::Ptt(v != 0)),
            _ => None,
        }
    }
}

impl ProtocolEngine for AlincoEngine {
    fn description(&self) -> &RigDescription {
        &self.desc
    }

    // [@ANCHOR: radioctl_alinco_engine_encode]
    fn encode(&mut self, op: &Op) -> Result<Request, RigError> {
        if !self.desc.capabilities.has(op.required_capability()) {
            return Err(unsupported("capability not in the rig description"));
        }
        let sr8 = self.dialect == Dialect::Dxsr8;
        let (text, reply, pending) = match op {
            Op::GetFrequency(Vfo::A) => (
                if sr8 { "AL~RR_RXF" } else { "AL3H" }.to_string(),
                ReplyKind::Observation(Field::Frequency(Vfo::A)),
                Pending::Read(Field::Frequency(Vfo::A)),
            ),
            Op::SetFrequency(Vfo::A, hz) => {
                if hz.0 >= 10_000_000_000 {
                    return Err(unsupported("frequency has more than ten digits"));
                }
                let text = if sr8 {
                    format!("AL~RW_RXF{:08}", hz.0)
                } else {
                    format!("AL0B{:06}", hz.0)
                };
                (text, ReplyKind::Ack, Pending::Set)
            }
            Op::GetMode => (
                if sr8 { "AL~RR_RFM" } else { "AL3H" }.to_string(),
                ReplyKind::Observation(Field::Mode),
                Pending::Read(Field::Mode),
            ),
            Op::SetMode(mode) => {
                let code = self
                    .desc
                    .mode_code(*mode)
                    .ok_or_else(|| unsupported("mode not in the description"))?;
                let text = if sr8 {
                    format!("AL~RW_RFM{code:02}")
                } else {
                    format!("AL2G{}", char::from(code))
                };
                (text, ReplyKind::Ack, Pending::Set)
            }
            Op::SetPtt(on) if sr8 => (
                format!("AL~RW_PTT{:02}", u8::from(*on)),
                ReplyKind::Ack,
                Pending::Set,
            ),
            Op::GetPtt if sr8 => (
                "AL~RR_PTT".to_string(),
                ReplyKind::Observation(Field::Ptt),
                Pending::Read(Field::Ptt),
            ),
            _ => return Err(unsupported("not implemented for the Alinco engine")),
        };
        self.pending = pending;
        self.echoed = false;
        self.rx.take_all();
        Ok(Request {
            bytes: self.command(&text),
            reply,
        })
    }

    // [@ANCHOR: radioctl_alinco_engine_feed]
    fn feed(&mut self, bytes: &[u8], now: Duration) -> Vec<Inbound> {
        let mut out = Vec::new();
        if let Some(stale) = self.rx.push(bytes, now) {
            out.push(Inbound::Unrecognised(stale));
        }
        while let Some(raw) = self.rx.take_line(LF) {
            let line = raw.strip_suffix(b"\r").unwrap_or(&raw).to_vec();
            if self.pending == Pending::Nothing {
                out.push(Inbound::Unrecognised(raw));
                continue;
            }
            if !self.echoed {
                self.echoed = true;
                continue;
            }
            let pending = std::mem::replace(&mut self.pending, Pending::Nothing);
            self.echoed = false;
            out.extend(match pending {
                Pending::Set if line == b"OK" => vec![Inbound::Ack],
                Pending::Set => vec![Inbound::Nak],
                Pending::Read(field) => match self.decode_reply(field, &line) {
                    Some(obs) => vec![Inbound::Observation(obs)],
                    None => vec![Inbound::Unrecognised(raw)],
                },
                Pending::Nothing => Vec::new(),
            });
        }
        out
    }
}

/// A virtual Alinco radio.
#[derive(Debug)]
pub struct AlincoTwin {
    dialect: Dialect,
    rx: RxBuffer,
    hz: u64,
    mode: u8,
    ptt: bool,
    codes: Vec<u8>,
}

impl AlincoTwin {
    // [@ANCHOR: radioctl_alinco_twin_new]
    /// A twin in the description's default state.
    pub fn new(desc: &RigDescription) -> Result<AlincoTwin, RigError> {
        let (_, name) = vendor_of(desc)?;
        let dialect = Dialect::from_name(name)
            .ok_or_else(|| unsupported("unknown Alinco dialect in the description"))?;
        let codes: Vec<u8> = desc.mode_codes.iter().map(|(_, c)| *c).collect();
        Ok(AlincoTwin {
            dialect,
            rx: RxBuffer::default(),
            hz: desc.default_frequency.0,
            mode: desc.mode_code(desc.default_mode).unwrap_or(0),
            ptt: false,
            codes,
        })
    }

    // [@ANCHOR: radioctl_alinco_twin_answer]
    /// The second line (without its CR LF) for a command, or `None` for an unknown command.
    fn answer(&mut self, cmd: &str) -> Option<String> {
        let ok = || Some("OK".to_string());
        match (self.dialect, cmd) {
            (Dialect::Dx77, "AL3H") => Some(format!(
                "00 {}  {:010}          ",
                char::from(self.mode),
                self.hz
            )),
            (Dialect::Dx77, c) if c.starts_with("AL0B") => {
                self.hz = c.get(4..)?.parse().ok()?;
                ok()
            }
            (Dialect::Dx77, c) if c.starts_with("AL2G") => {
                let m = *c.as_bytes().get(4)?;
                self.codes.contains(&m).then(|| self.mode = m)?;
                ok()
            }
            // Hamlib follows a mode write with a filter write (`AL2J` and two digits on the
            // DX-77, `AL~RW_NAR` and two digits on the DX-SR8); the engine never sends them and
            // the twin acknowledges them without keeping the width.
            (Dialect::Dx77, c) if c.starts_with("AL2J") => ok(),
            (Dialect::Dxsr8, c) if c.starts_with("AL~RW_NAR") => ok(),
            (Dialect::Dxsr8, "AL~RR_RXF") => Some(format!("{:08}", self.hz)),
            (Dialect::Dxsr8, "AL~RR_RFM") => Some(format!("{:02}", self.mode)),
            (Dialect::Dxsr8, "AL~RR_PTT") => Some(format!("{:02}", u8::from(self.ptt))),
            (Dialect::Dxsr8, c) if c.starts_with("AL~RW_RXF") => {
                self.hz = c.get(9..)?.parse().ok()?;
                ok()
            }
            (Dialect::Dxsr8, c) if c.starts_with("AL~RW_RFM") => {
                let m: u8 = c.get(9..)?.parse().ok()?;
                self.codes.contains(&m).then(|| self.mode = m)?;
                ok()
            }
            (Dialect::Dxsr8, c) if c.starts_with("AL~RW_PTT") => {
                self.ptt = c.get(9..)?.parse::<u8>().ok()? != 0;
                ok()
            }
            _ => None,
        }
    }
}

impl VendorTwin for AlincoTwin {
    // [@ANCHOR: radioctl_alinco_twin_snapshot]
    fn snapshot(&self) -> Option<TwinSnapshot> {
        Some(TwinSnapshot {
            frequency: Hz(self.hz),
            mode_code: Some(self.mode),
            ptt: self.ptt,
        })
    }

    // [@ANCHOR: radioctl_alinco_twin_feed]
    fn feed(&mut self, bytes: &[u8], now: Duration) -> Vec<u8> {
        self.rx.push(bytes, now);
        let terminator = if self.dialect == Dialect::Dx77 {
            b'\r'
        } else {
            LF
        };
        let mut out = Vec::new();
        while let Some(raw) = self.rx.take_line(terminator) {
            let text = String::from_utf8_lossy(&raw)
                .trim_end_matches('\r')
                .to_string();
            if let Some(reply) = self.answer(&text) {
                out.extend(format!("{text}\r\n{reply}\r\n").into_bytes());
            }
        }
        out
    }

    // [@ANCHOR: radioctl_alinco_twin_front_panel]
    fn front_panel(&mut self, panel: Panel) -> Result<Vec<u8>, String> {
        match panel {
            Panel::Frequency(hz) => self.hz = hz.0,
            Panel::Ptt(on) => self.ptt = on,
            other => return Err(format!("an Alinco twin has no {other:?} knob")),
        }
        Ok(Vec::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use radioctl_core::units::Mode;

    const T0: Duration = Duration::from_secs(1);

    fn shipped(id: &str) -> RigDescription {
        let path = format!("{}/../../rigs/{id}.toml", env!("CARGO_MANIFEST_DIR"));
        let text = std::fs::read_to_string(path).expect("rig file");
        radioctl_core::rigfile::parse(&text)
            .expect("loads")
            .description
    }

    fn ask(e: &mut AlincoEngine, t: &mut AlincoTwin, op: Op) -> Vec<Inbound> {
        let r = e.encode(&op).expect("encodes");
        let answer = t.feed(&r.bytes, T0);
        e.feed(&answer, T0)
    }

    // Tests [@ANCHOR: radioctl_alinco_dialect_from_name]
    // Tests [@ANCHOR: radioctl_alinco_decode_status]
    #[test]
    fn status_line_has_mode_at_3_and_frequency_at_6() {
        assert_eq!(Dialect::from_name("dx77"), Some(Dialect::Dx77));
        assert_eq!(Dialect::from_name("x"), None);
        assert_eq!(Dialect::Dxsr8.end(), b"\r\n");
        let line = b"00 1  0014200000          ";
        assert_eq!(line.len(), 26);
        assert_eq!(decode_status(line), Some((b'1', Hz(14_200_000))));
        assert_eq!(decode_status(b"short"), None);
        assert_eq!(decode_status(b"00 1  00142x0000          "), None);
    }

    // Tests [@ANCHOR: radioctl_alinco_engine_new]
    // Tests [@ANCHOR: radioctl_alinco_dialect_end]
    // Tests [@ANCHOR: radioctl_alinco_command]
    #[test]
    fn engine_refuses_other_families() {
        assert!(AlincoEngine::new(RigDescription::ts2000()).is_err());
        assert!(AlincoTwin::new(&RigDescription::ts2000()).is_err());
    }

    // Tests [@ANCHOR: radioctl_alinco_engine_encode]
    #[test]
    fn commands_are_the_documented_strings() {
        let mut d = AlincoEngine::new(shipped("alinco-dx-77")).expect("engine");
        let b = |e: &mut AlincoEngine, op: Op| e.encode(&op).expect("encodes").bytes;
        assert_eq!(
            b(&mut d, Op::SetFrequency(Vfo::A, Hz(14_200_000))),
            b"AL0B14200000\r"
        );
        assert_eq!(
            b(&mut d, Op::SetFrequency(Vfo::A, Hz(500_000))),
            b"AL0B500000\r"
        );
        assert_eq!(b(&mut d, Op::SetMode(Mode::Cw)), b"AL2G3\r");
        assert_eq!(b(&mut d, Op::GetMode), b"AL3H\r");
        assert!(
            d.encode(&Op::SetPtt(true)).is_err(),
            "Hamlib's DX-77 has no PTT"
        );
        let mut s = AlincoEngine::new(shipped("alinco-dx-sr8")).expect("engine");
        assert_eq!(
            b(&mut s, Op::SetFrequency(Vfo::A, Hz(7_074_000))),
            b"AL~RW_RXF07074000\r\n"
        );
        assert_eq!(b(&mut s, Op::GetFrequency(Vfo::A)), b"AL~RR_RXF\r\n");
        assert_eq!(b(&mut s, Op::SetMode(Mode::CwR)), b"AL~RW_RFM03\r\n");
        assert_eq!(b(&mut s, Op::SetPtt(true)), b"AL~RW_PTT01\r\n");
        assert_eq!(b(&mut s, Op::GetPtt), b"AL~RR_PTT\r\n");
        assert!(s
            .encode(&Op::SetFrequency(Vfo::A, Hz(10_000_000_000)))
            .is_err());
        assert!(s.encode(&Op::SetMode(Mode::Wfm)).is_err());
    }

    // Tests [@ANCHOR: radioctl_alinco_engine_feed]
    // Tests [@ANCHOR: radioctl_alinco_decode_reply]
    // Tests [@ANCHOR: radioctl_alinco_twin_new]
    // Tests [@ANCHOR: radioctl_alinco_twin_answer]
    // Tests [@ANCHOR: radioctl_alinco_twin_feed]
    // Tests [@ANCHOR: radioctl_alinco_twin_front_panel]
    #[test]
    fn both_models_round_trip_against_the_twin() {
        for id in ["alinco-dx-77", "alinco-dx-sr8"] {
            let desc = shipped(id);
            let mut e = AlincoEngine::new(desc.clone()).expect("engine");
            let mut t = AlincoTwin::new(&desc).expect("twin");
            assert_eq!(
                ask(&mut e, &mut t, Op::SetFrequency(Vfo::A, Hz(7_074_000))),
                vec![Inbound::Ack],
                "{id}"
            );
            assert_eq!(
                ask(&mut e, &mut t, Op::GetFrequency(Vfo::A)),
                vec![Inbound::Observation(Observation::Frequency(
                    Vfo::A,
                    Hz(7_074_000)
                ))],
                "{id}"
            );
            for mode in [Mode::Lsb, Mode::Usb, Mode::Am, Mode::Fm, Mode::Cw] {
                assert_eq!(
                    ask(&mut e, &mut t, Op::SetMode(mode)),
                    vec![Inbound::Ack],
                    "{id} {mode:?}"
                );
                assert_eq!(
                    ask(&mut e, &mut t, Op::GetMode),
                    vec![Inbound::Observation(Observation::Mode(mode))],
                    "{id} {mode:?}"
                );
            }
            t.front_panel(Panel::Frequency(Hz(3_573_000)))
                .expect("tunes");
            assert_eq!(
                ask(&mut e, &mut t, Op::GetFrequency(Vfo::A)),
                vec![Inbound::Observation(Observation::Frequency(
                    Vfo::A,
                    Hz(3_573_000)
                ))]
            );
            assert!(t.front_panel(Panel::Split(true)).is_err());
            assert!(t.feed(b"AL9Z\r\n", T0).is_empty());
        }
        let desc = shipped("alinco-dx-sr8");
        let mut e = AlincoEngine::new(desc.clone()).expect("engine");
        let mut t = AlincoTwin::new(&desc).expect("twin");
        let on = e.encode(&Op::SetPtt(true)).expect("ptt");
        assert_eq!(e.feed(&t.feed(&on.bytes, T0), T0), vec![Inbound::Ack]);
        let get = e.encode(&Op::GetPtt).expect("get");
        assert_eq!(
            e.feed(&t.feed(&get.bytes, T0), T0),
            vec![Inbound::Observation(Observation::Ptt(true))]
        );
        e.encode(&Op::SetMode(Mode::Usb)).expect("set");
        assert_eq!(e.feed(b"AL~RW_RFM00\r\nERR\r\n", T0), vec![Inbound::Nak]);
        e.encode(&Op::GetFrequency(Vfo::A)).expect("get");
        assert!(matches!(
            e.feed(b"AL~RR_RXF\r\nxx\r\n", T0).as_slice(),
            [Inbound::Unrecognised(_)]
        ));
        assert!(matches!(
            e.feed(b"late\r\n", T0).as_slice(),
            [Inbound::Unrecognised(_)]
        ));
    }

    // Tests [@ANCHOR: radioctl_alinco_twin_answer]
    #[test]
    fn the_twins_acknowledge_the_filter_writes_hamlib_adds_to_a_mode_write() {
        let mut t = AlincoTwin::new(&shipped("alinco-dx-77")).expect("twin");
        assert_eq!(t.feed(b"AL2J01\r", T0), b"AL2J01\r\nOK\r\n");
        let mut t = AlincoTwin::new(&shipped("alinco-dx-sr8")).expect("twin");
        assert_eq!(t.feed(b"AL~RW_NAR00\r\n", T0), b"AL~RW_NAR00\r\nOK\r\n");
    }
}
