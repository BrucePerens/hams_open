// SPDX-License-Identifier: AGPL-3.0-or-later
//! Icom marine transceivers (IC-M700PRO, IC-M710, IC-M802, IC-M803): `$PICOA` sentences, sans-IO
//! client engine and virtual twin.
//!
//! **Source.** Hamlib 4.6.2 (released source tarball, sha256
//! b2ac73f44dd1161e95fdee6c95276144757647bf92d7fdb369ee2fe41ed47ae8), read for facts only:
//! `rigs/icmarine/icmarine.c` (`icmarine_transaction`, `icmarine_set_freq`, `icmarine_get_freq`,
//! `icmarine_set_mode`, `icmarine_get_mode`, `icmarine_set_ptt`, `icmarine_get_ptt`,
//! `icmarine_get_level`), `icm710.c` (its own copy of the same commands, with a different RTTY mode
//! name) and the `default_remote_id` of each model file. No code or comment was copied and no Icom
//! manual was read; every fact is `hamlib-source` and unverified.
//!
//! * A command is the NMEA-style sentence `$PICOA,90,<id>,<CMD>[,<param>]*<XX>` and CR LF: 90 is the
//!   controller, `<id>` the radio's two-digit remote id (the description's `address`), `XX` the XOR
//!   of the characters between `$` and `*` in two hex digits.
//! * The radio echoes a setting back; Hamlib calls it accepted when the echo matches the command from
//!   the command name on, rejected otherwise. A query is answered with a sentence whose last field
//!   is the value.
//! * `RXF` and `TXF` carry the receive and transmit frequency in megahertz with six decimals. A
//!   frequency setting sends `TXF` and then `RXF` (Hamlib does that unless split is on), and the
//!   engine does the same so the radio never transmits on a stale frequency.
//! * `MODE` takes `LSB`, `USB`, `CW`, `AM` or the RTTY name (`J2B`; `AFS` on the IC-M710); `TRX,TX`
//!   and `TRX,RX` key and unkey, `TRX` reads; `SIGM` reads the S-meter as one digit.
//!
//! These are transceivers: the PTT is encoded but unverified and the transmit gate stays closed
//! by default. Not implemented: split, levels, the tuner, noise blanker, AGC, squelch status and
//! the `REMOTE,ON` sentence Hamlib sends when it opens the port (the operator must put the radio
//! in remote control).

use radioctl_core::description::{ReceiverProtocol, RigDescription};
use radioctl_core::engine::{Inbound, ReplyKind};
use radioctl_core::error::{ErrorKind, RigError};
use radioctl_core::op::{Field, Observation, Op};
use radioctl_core::units::{Hz, Mode, Vfo};

use crate::receiver::{Dialect, LineEngine, ReceiverTwin};
use crate::virtual_rig::RigState;

/// The marine engine.
pub type MarineEngine = LineEngine<MarineDialect>;

// [@ANCHOR: radioctl_marine_engine]
/// The engine for an Icom marine description.
pub fn engine(desc: RigDescription) -> Result<MarineEngine, RigError> {
    LineEngine::new(desc, ReceiverProtocol::IcomMarine)
}

/// The controller's id in every sentence.
const CONTROLLER_ID: u8 = 90;

/// Mode names by radioctl's mode code (1 to 5); the fifth is the RTTY name, set per dialect.
const MODES: [(u8, &str, Mode); 4] = [
    (1, "LSB", Mode::Lsb),
    (2, "USB", Mode::Usb),
    (3, "CW", Mode::Cw),
    (4, "AM", Mode::Am),
];
/// The code of the RTTY mode.
const RTTY_CODE: u8 = 5;

// [@ANCHOR: radioctl_marine_sentence]
/// The command sentence for `command` (with its `,param` if any) to radio `id`, checksum and CR LF.
pub fn sentence(id: u8, command: &str) -> String {
    let body = format!("PICOA,{CONTROLLER_ID:02},{id:02},{command}");
    let sum = body.bytes().fold(0u8, |acc, b| acc ^ b);
    format!("${body}*{sum:02X}\r\n")
}

/// The command set of one model.
#[derive(Debug, Clone)]
pub struct MarineDialect {
    id: u8,
    rtty_name: &'static str,
    /// Echoes still to arrive for the request in flight, with the command each must carry.
    waiting: Vec<String>,
}

impl MarineDialect {
    fn mode_name(&self, desc: &RigDescription, mode: Mode) -> Option<&'static str> {
        let code = desc.mode_code(mode)?;
        if code == RTTY_CODE {
            return Some(self.rtty_name);
        }
        MODES
            .iter()
            .find(|(c, _, _)| *c == code)
            .map(|(_, n, _)| *n)
    }
}

impl Dialect for MarineDialect {
    // [@ANCHOR: radioctl_marine_dialect_from_description]
    fn from_description(desc: &RigDescription) -> Result<MarineDialect, RigError> {
        let params = desc.receiver.as_ref();
        let rtty_name = match params.map_or("", |r| r.dialect.as_str()) {
            "icmarine" => "J2B",
            "icm710" => "AFS",
            other => {
                return Err(RigError::new(
                    ErrorKind::Unsupported,
                    format!("unknown marine dialect {other:?}"),
                ))
            }
        };
        let id = params
            .and_then(|r| r.address)
            .filter(|id| *id <= 99)
            .ok_or_else(|| {
                RigError::new(
                    ErrorKind::Unsupported,
                    "marine description needs a remote id 0 to 99",
                )
            })?;
        Ok(MarineDialect {
            id,
            rtty_name,
            waiting: Vec::new(),
        })
    }

    // [@ANCHOR: radioctl_marine_dialect_encode]
    fn encode(&mut self, desc: &RigDescription, op: &Op) -> Result<(Vec<u8>, ReplyKind), RigError> {
        let unsupported = |why: &str| RigError::new(ErrorKind::Unsupported, why.to_string());
        let mhz = |hz: Hz| format!("{}.{:06}", hz.0 / 1_000_000, hz.0 % 1_000_000);
        let (commands, reply): (Vec<String>, ReplyKind) = match op {
            Op::SetFrequency(Vfo::A, hz) => (
                vec![format!("TXF,{}", mhz(*hz)), format!("RXF,{}", mhz(*hz))],
                ReplyKind::Ack,
            ),
            Op::GetFrequency(Vfo::A) => (
                vec!["RXF".into()],
                ReplyKind::Observation(Field::Frequency(Vfo::A)),
            ),
            Op::SetMode(mode) => {
                let name = self
                    .mode_name(desc, *mode)
                    .ok_or_else(|| unsupported("mode not in the description"))?;
                (vec![format!("MODE,{name}")], ReplyKind::Ack)
            }
            Op::GetMode => (vec!["MODE".into()], ReplyKind::Observation(Field::Mode)),
            Op::SetPtt(on) => (
                vec![format!("TRX,{}", if *on { "TX" } else { "RX" })],
                ReplyKind::Ack,
            ),
            Op::GetPtt => (vec!["TRX".into()], ReplyKind::Observation(Field::Ptt)),
            Op::GetSignal => (vec!["SIGM".into()], ReplyKind::Observation(Field::Signal)),
            _ => return Err(unsupported("not implemented for the marine engine")),
        };
        self.waiting = commands.clone();
        let bytes = commands
            .iter()
            .flat_map(|c| sentence(self.id, c).into_bytes())
            .collect();
        Ok((bytes, reply))
    }

    fn terminator(&self) -> u8 {
        b'\n'
    }

    // [@ANCHOR: radioctl_marine_dialect_more_lines]
    fn more_lines(&self) -> bool {
        !self.waiting.is_empty()
    }

    // [@ANCHOR: radioctl_marine_dialect_status]
    fn status(&mut self, _expecting: ReplyKind, line: &str) -> Option<Vec<Inbound>> {
        line.is_empty().then(Vec::new)
    }

    // [@ANCHOR: radioctl_marine_dialect_decode]
    fn decode(&mut self, desc: &RigDescription, expecting: ReplyKind, line: &str) -> Vec<Inbound> {
        let unknown = |this: &mut MarineDialect| {
            this.waiting.clear();
            vec![Inbound::Unrecognised(line.as_bytes().to_vec())]
        };
        let Some(fields) = Fields::parse(line) else {
            return unknown(self);
        };
        match expecting {
            ReplyKind::Ack => {
                let Some(sent) = self.waiting.first().cloned() else {
                    return unknown(self);
                };
                self.waiting.remove(0);
                if fields.command_and_param != sent {
                    self.waiting.clear();
                    return vec![Inbound::Nak];
                }
                if self.waiting.is_empty() {
                    vec![Inbound::Ack]
                } else {
                    Vec::new()
                }
            }
            ReplyKind::Observation(field) => {
                self.waiting.clear();
                let value = fields.last;
                let found = match field {
                    Field::Frequency(_) => if value.is_empty() {
                        Some(Hz(0))
                    } else {
                        value
                            .parse::<f64>()
                            .ok()
                            .filter(|m| (0.0..10_000.0).contains(m))
                            .map(|m| Hz((m * 1e6).round() as u64))
                    }
                    .map(|hz| Observation::Frequency(Vfo::A, hz)),
                    Field::Mode => self.mode_of(desc, value).map(Observation::Mode),
                    Field::Ptt => {
                        (value.len() >= 2).then(|| Observation::Ptt(value.starts_with("TX")))
                    }
                    Field::Signal => value
                        .bytes()
                        .next()
                        .filter(u8::is_ascii_digit)
                        .map(|d| Observation::Signal(u16::from(d - b'0'))),
                    _ => None,
                };
                found.map_or_else(|| unknown(self), |o| vec![Inbound::Observation(o)])
            }
            ReplyKind::None => unknown(self),
        }
    }
}

impl MarineDialect {
    // [@ANCHOR: radioctl_marine_mode_of]
    /// The mode a `MODE` answer names, if the description has it.
    fn mode_of(&self, desc: &RigDescription, value: &str) -> Option<Mode> {
        let mode = if value.starts_with(self.rtty_name) {
            Mode::Rtty
        } else {
            MODES.iter().find(|(_, n, _)| value.starts_with(n))?.2
        };
        desc.mode_code(mode).map(|_| mode)
    }
}

/// The parts of a received `$PICOA` sentence.
struct Fields<'a> {
    /// Everything after the radio ids: the command and its parameter.
    command_and_param: &'a str,
    /// The text after the last comma before the checksum.
    last: &'a str,
}

impl<'a> Fields<'a> {
    // [@ANCHOR: radioctl_marine_fields_parse]
    /// Splits a sentence, checking `$PICOA,`, the checksum and the minimum length Hamlib requires.
    fn parse(line: &'a str) -> Option<Fields<'a>> {
        let body = line.strip_prefix('$')?;
        let (text, sum) = body.rsplit_once('*')?;
        let want = u8::from_str_radix(sum.trim(), 16).ok()?;
        if text.bytes().fold(0u8, |a, b| a ^ b) != want || !text.starts_with("PICOA,") {
            return None;
        }
        let mut parts = text.splitn(4, ',');
        let (_, _, _, rest) = (parts.next()?, parts.next()?, parts.next()?, parts.next()?);
        Some(Fields {
            command_and_param: rest,
            last: rest.rsplit(',').next().unwrap_or(""),
        })
    }
}

/// The virtual marine radio.
#[derive(Debug)]
pub struct MarineTwin {
    desc: RigDescription,
    dialect: MarineDialect,
    tx: Hz,
}

impl MarineTwin {
    // [@ANCHOR: radioctl_marine_twin_new]
    /// The twin of an Icom marine description.
    pub fn new(desc: &RigDescription) -> Result<MarineTwin, RigError> {
        Ok(MarineTwin {
            dialect: MarineDialect::from_description(desc)?,
            desc: desc.clone(),
            tx: desc.default_frequency,
        })
    }
}

impl ReceiverTwin for MarineTwin {
    fn terminator(&self) -> u8 {
        b'\n'
    }

    // [@ANCHOR: radioctl_marine_twin_line]
    fn line(&mut self, state: &mut RigState, line: &str) -> Vec<u8> {
        let Some(fields) = Fields::parse(line) else {
            return Vec::new();
        };
        let id = self.dialect.id;
        let (command, param) = fields
            .command_and_param
            .split_once(',')
            .unwrap_or((fields.command_and_param, ""));
        let has_param = fields.command_and_param.contains(',');
        let parse_mhz = |p: &str| {
            p.parse::<f64>()
                .ok()
                .filter(|m| (0.0..10_000.0).contains(m))
                .map(|m| Hz((m * 1e6).round() as u64))
        };
        let echo = |this: &Self, ok: bool| {
            if ok {
                sentence(this.dialect.id, fields.command_and_param).into_bytes()
            } else {
                Vec::new()
            }
        };
        let answer = |value: String| sentence(id, &format!("{command},{value}")).into_bytes();
        match (command, has_param) {
            ("TXF", true) => match parse_mhz(param) {
                Some(hz) => {
                    self.tx = hz;
                    echo(self, true)
                }
                None => Vec::new(),
            },
            ("RXF", true) => match parse_mhz(param) {
                Some(hz) => {
                    state.freq_a = hz;
                    echo(self, true)
                }
                None => Vec::new(),
            },
            ("RXF", false) => answer(format!(
                "{}.{:06}",
                state.freq_a.0 / 1_000_000,
                state.freq_a.0 % 1_000_000
            )),
            ("MODE", true) => {
                let mode = [Mode::Lsb, Mode::Usb, Mode::Cw, Mode::Am, Mode::Rtty]
                    .into_iter()
                    .find(|m| self.dialect.mode_name(&self.desc, *m) == Some(param));
                match mode {
                    Some(mode) => {
                        state.mode = mode;
                        echo(self, true)
                    }
                    None => Vec::new(),
                }
            }
            ("MODE", false) => answer(
                self.dialect
                    .mode_name(&self.desc, state.mode)
                    .unwrap_or("USB")
                    .to_string(),
            ),
            ("TRX", true) if matches!(param, "TX" | "RX") => {
                state.ptt = param == "TX";
                echo(self, true)
            }
            ("TRX", false) => answer(if state.ptt { "TX" } else { "RX" }.to_string()),
            ("SIGM", false) => answer(state.signal.min(9).to_string()),
            _ => Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use radioctl_core::engine::ProtocolEngine;

    use super::*;

    const T0: Duration = Duration::from_secs(1);

    fn shipped(id: &str) -> RigDescription {
        let path = format!("{}/../../rigs/{id}.toml", env!("CARGO_MANIFEST_DIR"));
        let text = std::fs::read_to_string(path).expect("rig file");
        radioctl_core::rigfile::parse(&text)
            .expect("loads")
            .description
    }

    // Tests [@ANCHOR: radioctl_marine_sentence]
    #[test]
    fn a_sentence_carries_the_xor_checksum_and_crlf() {
        let s = sentence(2, "TRX");
        assert!(s.starts_with("$PICOA,90,02,TRX*") && s.ends_with("\r\n"));
        let body = &s[1..s.find('*').expect("star")];
        let sum = body.bytes().fold(0u8, |a, b| a ^ b);
        assert_eq!(
            &s[s.find('*').expect("star") + 1..s.len() - 2],
            format!("{sum:02X}")
        );
    }

    // Tests [@ANCHOR: radioctl_marine_engine]
    // Tests [@ANCHOR: radioctl_marine_dialect_from_description]
    // Tests [@ANCHOR: radioctl_marine_dialect_encode]
    #[test]
    fn a_frequency_setting_sends_the_transmit_frequency_first() {
        let mut e = engine(shipped("icom-ic-m802")).expect("engine");
        let sent = String::from_utf8(
            e.encode(&Op::SetFrequency(Vfo::A, Hz(2_182_000)))
                .expect("encode")
                .bytes,
        )
        .expect("ascii");
        let lines: Vec<&str> = sent.split("\r\n").filter(|l| !l.is_empty()).collect();
        assert_eq!(lines.len(), 2);
        assert!(
            lines[0].starts_with("$PICOA,90,08,TXF,2.182000*"),
            "{}",
            lines[0]
        );
        assert!(
            lines[1].starts_with("$PICOA,90,08,RXF,2.182000*"),
            "{}",
            lines[1]
        );
        let mut m = engine(shipped("icom-ic-m710")).expect("engine");
        let rtty = String::from_utf8(m.encode(&Op::SetMode(Mode::Rtty)).expect("encode").bytes)
            .expect("ascii");
        assert!(rtty.contains(",MODE,AFS*"), "{rtty}");
        assert!(e.encode(&Op::SetMode(Mode::Fm)).is_err());
        assert!(engine(shipped("aor-ar8000")).is_err());
    }

    // Tests [@ANCHOR: radioctl_marine_dialect_more_lines]
    // Tests [@ANCHOR: radioctl_marine_dialect_status]
    // Tests [@ANCHOR: radioctl_marine_dialect_decode]
    // Tests [@ANCHOR: radioctl_marine_fields_parse]
    // Tests [@ANCHOR: radioctl_marine_mode_of]
    #[test]
    fn echoes_acknowledge_and_a_wrong_echo_rejects() {
        let mut e = engine(shipped("icom-ic-m802")).expect("engine");
        let request = e
            .encode(&Op::SetFrequency(Vfo::A, Hz(2_182_000)))
            .expect("encode");
        let (txf, rxf) = {
            let text = String::from_utf8(request.bytes).expect("ascii");
            let mut it = text.split_inclusive("\r\n").map(str::to_string);
            (it.next().expect("txf"), it.next().expect("rxf"))
        };
        assert!(
            e.feed(txf.as_bytes(), T0).is_empty(),
            "first echo waits for the second"
        );
        assert_eq!(e.feed(rxf.as_bytes(), T0), vec![Inbound::Ack]);
        e.encode(&Op::SetMode(Mode::Usb)).expect("encode");
        let wrong = sentence(8, "MODE,LSB");
        assert_eq!(e.feed(wrong.as_bytes(), T0), vec![Inbound::Nak]);
        e.encode(&Op::GetPtt).expect("encode");
        assert_eq!(
            e.feed(sentence(8, "TRX,TX").as_bytes(), T0),
            vec![Inbound::Observation(Observation::Ptt(true))]
        );
        e.encode(&Op::GetPtt).expect("encode");
        assert!(
            matches!(
                e.feed(b"$PICOA,90,08,TRX,TX*00\r\n", T0).as_slice(),
                [Inbound::Unrecognised(_)]
            ),
            "bad checksum"
        );
        e.encode(&Op::GetSignal).expect("encode");
        assert_eq!(
            e.feed(sentence(8, "SIGM,7").as_bytes(), T0),
            vec![Inbound::Observation(Observation::Signal(7))]
        );
        e.encode(&Op::GetFrequency(Vfo::A)).expect("encode");
        assert_eq!(
            e.feed(sentence(8, "RXF,2.182000").as_bytes(), T0),
            vec![Inbound::Observation(Observation::Frequency(
                Vfo::A,
                Hz(2_182_000)
            ))]
        );
        e.encode(&Op::GetMode).expect("encode");
        assert_eq!(
            e.feed(sentence(8, "MODE,J2B").as_bytes(), T0),
            vec![Inbound::Observation(Observation::Mode(Mode::Rtty))]
        );
        assert!(e.feed(b"\r\n", T0).is_empty());
    }

    // Tests [@ANCHOR: radioctl_marine_twin_new]
    // Tests [@ANCHOR: radioctl_marine_twin_line]
    #[test]
    fn the_twin_ignores_bad_sentences_and_unknown_commands() {
        let desc = shipped("icom-ic-m700pro");
        let mut twin = MarineTwin::new(&desc).expect("twin");
        let mut state = crate::virtual_rig::VirtualRig::from_description(&desc)
            .expect("rig")
            .state()
            .clone();
        assert!(
            twin.line(&mut state, "$PICOA,90,02,TRX*00").is_empty(),
            "bad checksum"
        );
        let bad_freq = sentence(2, "RXF,abc");
        assert!(twin.line(&mut state, bad_freq.trim()).is_empty());
        let unknown = sentence(2, "ZZZ,1");
        assert!(twin.line(&mut state, unknown.trim()).is_empty());
        let good = sentence(2, "RXF,4.125000");
        assert_eq!(twin.line(&mut state, good.trim()), good.as_bytes());
        assert_eq!(state.freq_a, Hz(4_125_000));
        assert!(MarineTwin::new(&shipped("aor-ar8000")).is_err());
    }
}
