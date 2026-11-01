// SPDX-License-Identifier: AGPL-3.0-or-later
//! Elecraft XG3 signal source: sans-IO client engine and virtual twin.
//!
//! **Sources.** The XG3 Owner's Manual (Elecraft E740154, `xg3_owners_manual.pdf`, sha256
//! c387e632e48074f4fe5e4e990b73acc24e3c6612fdfdd2baa5fd72132c992c59, archive.org copy kept outside
//! the repositories), "XG3 Remote Control Commands" on pages 18 to 22, read for facts: serial line
//! 9600 8N1; commands are one letter, arguments follow after a comma, a command ends at `;` or CR;
//! `F`, `O`, `L`, `C`, `M`, `G`, `V` and `I` set when given an argument and answer (echoing the
//! letter, then `,` and the value, ended by `;`) when not; a command the XG3 does not know is
//! ignored without a reply; frequencies are eleven digits in hertz, 1.5 to 1400 MHz (the XG3
//! clamps outside it); `O,01` and `O,00` switch the output on and off. Hamlib 4.6.2
//! (`rigs/kenwood/xg3.c`, released tarball sha256
//! b2ac73f44dd1161e95fdee6c95276144757647bf92d7fdb369ee2fe41ed47ae8) was read for facts too: it sends
//! `F,<11 digits>`, `O,01`/`O,00` for "PTT", and reads `O;` and `F;`. Nothing is verified against a
//! radio.
//!
//! Open questions: the manual says `;` is echoed ("semicolons are synonymous with CR except that
//! they are echoed"), but its own examples show no echo ahead of an answer; the engine ignores a
//! bare `;` either way. The output level (`L`, 0 to 3, 0 dBm down to -107 dBm, so a bigger number
//! is a weaker signal) and the memories are not in the engine: a level whose scale runs backwards
//! has no place in radioctl's `RfPower`.
//!
//! The XG3 is a signal source: its "PTT" is the RF output switch, so setting it on stays behind
//! the transmit gate like any other key-down.

use std::time::Duration;

use radioctl_core::description::RigDescription;
use radioctl_core::engine::{Inbound, ProtocolEngine, ReplyKind, Request};
use radioctl_core::error::{ErrorKind, RigError};
use radioctl_core::op::{Field, Observation, Op};
use radioctl_core::transcript::Panel;
use radioctl_core::units::{Hz, Vfo};

use crate::serial_vendor::{vendor_of, RxBuffer, TwinSnapshot, VendorTwin};

/// Lowest frequency, hertz (manual page 20).
pub const MIN_HZ: u64 = 1_500_000;
/// Highest frequency, hertz (manual page 20).
pub const MAX_HZ: u64 = 1_400_000_000;

fn unsupported(what: &str) -> RigError {
    RigError::new(ErrorKind::Unsupported, what.to_string())
}

// [@ANCHOR: radioctl_xg3_frequency_text]
/// The frequency argument: eleven digits of hertz.
pub fn frequency_text(hz: Hz) -> String {
    format!("{:011}", hz.0)
}

// [@ANCHOR: radioctl_xg3_decode]
/// What one answer (without its `;`) says: `F,<11 digits>`, `O,<2 digits>` or `V,<major>.<minor>`.
pub fn decode(text: &str) -> Option<Observation> {
    let (letter, value) = text.split_once(',')?;
    match letter {
        "F" if value.len() == 11 => {
            let hz: u64 = value.parse().ok()?;
            Some(Observation::Frequency(Vfo::A, Hz(hz)))
        }
        "O" if value.len() == 2 && value.bytes().all(|b| b.is_ascii_digit()) => {
            Some(Observation::Ptt(value != "00"))
        }
        "V" => {
            let (major, minor) = value.split_once('.')?;
            Some(Observation::Firmware(
                major.parse().ok()?,
                minor.parse().ok()?,
            ))
        }
        _ => None,
    }
}

/// The XG3 client engine.
#[derive(Debug)]
pub struct Xg3Engine {
    desc: RigDescription,
    rx: RxBuffer,
}

impl Xg3Engine {
    // [@ANCHOR: radioctl_xg3_engine_new]
    /// Builds an engine from an XG3 vendor description.
    pub fn new(desc: RigDescription) -> Result<Xg3Engine, RigError> {
        let (_, name) = vendor_of(&desc)?;
        if name != "xg3" {
            return Err(unsupported("unknown XG3 dialect in the description"));
        }
        Ok(Xg3Engine {
            desc,
            rx: RxBuffer::default(),
        })
    }
}

impl ProtocolEngine for Xg3Engine {
    fn description(&self) -> &RigDescription {
        &self.desc
    }

    // [@ANCHOR: radioctl_xg3_engine_encode]
    fn encode(&mut self, op: &Op) -> Result<Request, RigError> {
        if !self.desc.capabilities.has(op.required_capability()) {
            return Err(unsupported("capability not in the rig description"));
        }
        let (text, reply) = match op {
            Op::GetFrequency(Vfo::A) => (
                "F;".to_string(),
                ReplyKind::Observation(Field::Frequency(Vfo::A)),
            ),
            Op::SetFrequency(Vfo::A, hz) => {
                if hz.0 < MIN_HZ || hz.0 > MAX_HZ {
                    return Err(unsupported("frequency outside 1.5 to 1400 MHz"));
                }
                (format!("F,{};", frequency_text(*hz)), ReplyKind::None)
            }
            Op::GetPtt => ("O;".to_string(), ReplyKind::Observation(Field::Ptt)),
            Op::SetPtt(on) => (format!("O,{:02};", u8::from(*on)), ReplyKind::None),
            Op::GetFirmware => ("V;".to_string(), ReplyKind::Observation(Field::Firmware)),
            _ => return Err(unsupported("not implemented for the XG3 engine")),
        };
        self.rx.take_all();
        Ok(Request {
            bytes: text.into_bytes(),
            reply,
        })
    }

    // [@ANCHOR: radioctl_xg3_engine_feed]
    fn feed(&mut self, bytes: &[u8], now: Duration) -> Vec<Inbound> {
        let mut out = Vec::new();
        if let Some(stale) = self.rx.push(bytes, now) {
            out.push(Inbound::Unrecognised(stale));
        }
        while let Some(line) = self.rx.take_line(b';') {
            let text = String::from_utf8_lossy(&line).trim().to_string();
            // A bare `;` is the echo of a terminator, not an answer.
            if text.is_empty() {
                continue;
            }
            out.push(match decode(&text) {
                Some(observation) => Inbound::Observation(observation),
                None => Inbound::Unrecognised(line),
            });
        }
        out
    }
}

/// A virtual XG3.
#[derive(Debug)]
pub struct Xg3Twin {
    rx: RxBuffer,
    hz: u64,
    output: bool,
    level: u8,
    memory: u8,
    backlight: u8,
    memories: [u64; 12],
}

impl Xg3Twin {
    // [@ANCHOR: radioctl_xg3_twin_new]
    /// A twin in the description's default state. The real XG3 switches its output on at power-up
    /// (manual page 22); the twin starts with it off, like every twin here starts unkeyed, and
    /// the unkey that radioctld sends at start-up would switch the real one off.
    pub fn new(desc: &RigDescription) -> Result<Xg3Twin, RigError> {
        let (_, name) = vendor_of(desc)?;
        if name != "xg3" {
            return Err(unsupported("unknown XG3 dialect in the description"));
        }
        Ok(Xg3Twin {
            rx: RxBuffer::default(),
            hz: desc.default_frequency.0,
            output: false,
            level: 0,
            memory: 0,
            backlight: 0,
            memories: [MIN_HZ; 12],
        })
    }

    fn clamp(hz: u64) -> u64 {
        hz.clamp(MIN_HZ, MAX_HZ)
    }

    // [@ANCHOR: radioctl_xg3_twin_command]
    fn command(&mut self, text: &str) -> Vec<u8> {
        let mut parts = text.split([',', ' ']).filter(|p| !p.is_empty());
        let letter = parts.next().map(str::to_ascii_uppercase);
        let args: Vec<&str> = parts.collect();
        let number = |i: usize| args.get(i).and_then(|a| a.parse::<u64>().ok());
        match (letter.as_deref(), args.len()) {
            (Some("F"), 0) => format!("F,{:011};", self.hz).into_bytes(),
            (Some("F"), _) => {
                if let Some(hz) = number(0) {
                    self.hz = Self::clamp(hz);
                }
                Vec::new()
            }
            (Some("O"), 0) => format!("O,{:02};", u8::from(self.output)).into_bytes(),
            (Some("O"), _) => {
                if let Some(v) = number(0) {
                    self.output = v != 0;
                }
                Vec::new()
            }
            (Some("L"), 0) => format!("L,{:02};", self.level).into_bytes(),
            (Some("L"), _) => {
                if let Some(v) = number(0).filter(|v| *v <= 3) {
                    self.level = u8::try_from(v).unwrap_or(0);
                }
                Vec::new()
            }
            (Some("G"), 0) => format!("G,{:02};", self.backlight).into_bytes(),
            (Some("G"), _) => {
                if let Some(v) = number(0).filter(|v| *v <= 3) {
                    self.backlight = u8::try_from(v).unwrap_or(0);
                }
                Vec::new()
            }
            (Some("C"), 0) => format!("C,{:02};", self.memory).into_bytes(),
            (Some("C"), _) => {
                if let Some(v) = number(0).filter(|v| *v <= 11) {
                    self.memory = u8::try_from(v).unwrap_or(0);
                    self.hz = self.memories[usize::from(self.memory)];
                }
                Vec::new()
            }
            (Some("M"), 1) => match number(0).filter(|v| *v <= 11) {
                Some(slot) => format!(
                    "M,{slot:02},{:011};",
                    self.memories[usize::try_from(slot).unwrap_or(0)]
                )
                .into_bytes(),
                None => Vec::new(),
            },
            (Some("M"), _) => {
                if let (Some(slot), Some(hz)) = (number(0).filter(|v| *v <= 11), number(1)) {
                    self.memories[usize::try_from(slot).unwrap_or(0)] = Self::clamp(hz);
                }
                Vec::new()
            }
            (Some("V"), 0) => b"V,1.00;".to_vec(),
            (Some("I"), 0) => format!(
                "I,{:011},{:02},{:02},{:02},01,00,00,00;",
                self.hz,
                self.level,
                self.memory,
                u8::from(self.output)
            )
            .into_bytes(),
            // Unknown or ill-formed: ignored, no reply (manual page 19).
            _ => Vec::new(),
        }
    }
}

impl VendorTwin for Xg3Twin {
    // [@ANCHOR: radioctl_xg3_twin_snapshot]
    fn snapshot(&self) -> Option<TwinSnapshot> {
        Some(TwinSnapshot {
            frequency: Hz(self.hz),
            mode_code: None,
            ptt: self.output,
        })
    }

    // [@ANCHOR: radioctl_xg3_twin_feed]
    fn feed(&mut self, bytes: &[u8], now: Duration) -> Vec<u8> {
        // A command ends at `;` or CR.
        let converted: Vec<u8> = bytes
            .iter()
            .map(|b| if *b == b'\r' { b';' } else { *b })
            .collect();
        self.rx.push(&converted, now);
        let mut out = Vec::new();
        while let Some(line) = self.rx.take_line(b';') {
            let text = String::from_utf8_lossy(&line).trim().to_string();
            if !text.is_empty() {
                out.extend(self.command(&text));
            }
        }
        out
    }

    // [@ANCHOR: radioctl_xg3_twin_front_panel]
    fn front_panel(&mut self, panel: Panel) -> Result<Vec<u8>, String> {
        match panel {
            Panel::Frequency(hz) => self.hz = Self::clamp(hz.0),
            Panel::Ptt(on) => self.output = on,
            other => return Err(format!("an XG3 twin has no {other:?} knob")),
        }
        Ok(Vec::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const T0: Duration = Duration::from_secs(1);

    fn shipped() -> RigDescription {
        let path = format!(
            "{}/../../rigs/elecraft-xg3.toml",
            env!("CARGO_MANIFEST_DIR")
        );
        let text = std::fs::read_to_string(path).expect("rig file");
        radioctl_core::rigfile::parse(&text)
            .expect("loads")
            .description
    }

    // Tests [@ANCHOR: radioctl_xg3_frequency_text]
    // Tests [@ANCHOR: radioctl_xg3_decode]
    #[test]
    fn the_manual_s_examples_decode() {
        assert_eq!(frequency_text(Hz(14_025_000)), "00014025000");
        assert_eq!(
            decode("F,00014025000"),
            Some(Observation::Frequency(Vfo::A, Hz(14_025_000)))
        );
        assert_eq!(decode("O,00"), Some(Observation::Ptt(false)));
        assert_eq!(decode("O,01"), Some(Observation::Ptt(true)));
        assert_eq!(decode("V,1.00"), Some(Observation::Firmware(1, 0)));
        assert_eq!(decode("F,1400"), None);
        assert_eq!(decode("Q,01"), None);
        assert_eq!(decode("nothing"), None);
    }

    // Tests [@ANCHOR: radioctl_xg3_engine_new]
    #[test]
    fn other_families_are_refused() {
        assert!(Xg3Engine::new(RigDescription::ts2000()).is_err());
        assert!(Xg3Twin::new(&RigDescription::ts2000()).is_err());
    }

    // Tests [@ANCHOR: radioctl_xg3_engine_encode]
    // Tests [@ANCHOR: radioctl_xg3_engine_feed]
    // Tests [@ANCHOR: radioctl_xg3_twin_new]
    // Tests [@ANCHOR: radioctl_xg3_twin_feed]
    // Tests [@ANCHOR: radioctl_xg3_twin_command]
    // Tests [@ANCHOR: radioctl_xg3_twin_snapshot]
    // Tests [@ANCHOR: radioctl_xg3_twin_front_panel]
    #[test]
    fn the_engine_and_the_twin_agree_on_frequency_output_and_version() {
        let desc = shipped();
        let mut e = Xg3Engine::new(desc.clone()).expect("engine");
        let mut t = Xg3Twin::new(&desc).expect("twin");
        let r = e
            .encode(&Op::SetFrequency(Vfo::A, Hz(14_025_000)))
            .expect("encodes");
        assert_eq!(r.bytes, b"F,00014025000;");
        assert_eq!(r.reply, ReplyKind::None);
        assert!(t.feed(&r.bytes, T0).is_empty());
        assert_eq!(t.snapshot().expect("snapshot").frequency, Hz(14_025_000));
        let r = e.encode(&Op::GetFrequency(Vfo::A)).expect("encodes");
        let answer = t.feed(&r.bytes, T0);
        assert_eq!(answer, b"F,00014025000;");
        assert_eq!(
            e.feed(&answer, T0),
            vec![Inbound::Observation(Observation::Frequency(
                Vfo::A,
                Hz(14_025_000)
            ))]
        );
        // The output switch, which is the radio's PTT.
        let r = e.encode(&Op::SetPtt(false)).expect("encodes");
        assert_eq!(r.bytes, b"O,00;");
        assert!(t.feed(&r.bytes, T0).is_empty());
        assert!(!t.snapshot().expect("snapshot").ptt);
        let r = e.encode(&Op::GetPtt).expect("encodes");
        let answer = t.feed(&r.bytes, T0);
        assert_eq!(answer, b"O,00;");
        assert_eq!(
            e.feed(&answer, T0),
            vec![Inbound::Observation(Observation::Ptt(false))]
        );
        assert_eq!(e.encode(&Op::SetPtt(true)).expect("on").bytes, b"O,01;");
        // The version, the memories, the info line and the manual's CR terminator.
        let answer = t.feed(b"V\r", T0);
        assert_eq!(answer, b"V,1.00;");
        assert_eq!(
            e.feed(&answer, T0),
            vec![Inbound::Observation(Observation::Firmware(1, 0))]
        );
        assert_eq!(t.feed(b"O;", T0), b"O,00;");
        assert!(t.feed(b"M,01,00003850000;", T0).is_empty());
        assert_eq!(t.feed(b"M,01;", T0), b"M,01,00003850000;");
        assert!(t.feed(b"C,01;", T0).is_empty());
        assert_eq!(t.feed(b"F;", T0), b"F,00003850000;");
        assert_eq!(t.feed(b"C;", T0), b"C,01;");
        assert!(t.feed(b"L,02;", T0).is_empty());
        assert_eq!(t.feed(b"i;", T0), b"I,00003850000,02,01,00,01,00,00,00;");
        assert!(t.feed(b"G,03;", T0).is_empty());
        assert_eq!(t.feed(b"G;", T0), b"G,03;");
        // Out of range: clamped by the radio; refused by the engine. Unknown: ignored.
        assert!(t.feed(b"F,00000000001;", T0).is_empty());
        assert_eq!(t.feed(b"F;", T0), b"F,00001500000;");
        assert!(t.feed(b"Y;Z,1;", T0).is_empty());
        assert!(e.encode(&Op::SetFrequency(Vfo::A, Hz(1_000_000))).is_err());
        assert!(e
            .encode(&Op::SetFrequency(Vfo::A, Hz(1_400_000_001)))
            .is_err());
        assert!(e.encode(&Op::SetFrequency(Vfo::B, Hz(7_000_000))).is_err());
        assert!(e.encode(&Op::GetMode).is_err());
        // A bare `;` is no answer; a stale half answer is reported.
        assert!(e.feed(b";", T0).is_empty());
        assert!(matches!(
            e.feed(b"Q,01;", T0).as_slice(),
            [Inbound::Unrecognised(_)]
        ));
        t.front_panel(Panel::Frequency(Hz(50_000_000)))
            .expect("tunes");
        assert_eq!(t.feed(b"F;", T0), b"F,00050000000;");
        assert!(t.front_panel(Panel::Signal(3)).is_err());
    }
}
