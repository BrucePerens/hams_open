// SPDX-License-Identifier: AGPL-3.0-or-later
//! Skanti TRP8000 and TRP 8255 S R transceivers: sans-IO client engine and virtual twin.
//!
//! **Sources.** Facts only, read from Hamlib 4.6.2 (released tarball, sha256
//! b2ac73f44dd1161e95fdee6c95276144757647bf92d7fdb369ee2fe41ed47ae8): `rigs/skanti/skanti.c`,
//! `trp8000.c` and `trp8255.c`. No Skanti document was read; nothing is verified against a radio.
//! Hamlib has no read commands for either radio, so the descriptions have no read capabilities.
//!
//! * Dialect `trp8000` (7 data bits, odd parity, 300 baud in Hamlib): commands end with CR and the
//!   radio answers each with a `>` prompt. `Z` and six digits of 100 Hz units set the frequency;
//!   a mode is a letter (`L` LSB, `J` USB, `H` AM, `F` RTTY, `A1` CW); `XN` / `XF` key and unkey.
//!   The description's mode code for CW is `A` and the engine sends `A1`.
//! * Dialect `trp8255` (through the control unit): the radio acknowledges every byte it receives
//!   with ACK (06) or refuses it with NAK (15). `:` and six digits of 100 Hz units then CR set the
//!   receive frequency, followed (unless split) by `:;` CR, which copies it to the transmitter;
//!   a mode is one byte (`X` USB, `Y` LSB, `Z` AM, `[` RTTY, `]` CW); `u` / `v` key and unkey.
//!   Hamlib writes and acknowledges one byte at a time; this engine sends a whole command and
//!   counts the acknowledgements, which a real control unit may not accept (open question). The
//!   control-unit start-up bytes (SOH STX) are not sent: the engine has no open hook.

use std::time::Duration;

use radioctl_core::description::RigDescription;
use radioctl_core::engine::{Inbound, ProtocolEngine, ReplyKind, Request};
use radioctl_core::error::{ErrorKind, RigError};
use radioctl_core::op::Op;
use radioctl_core::transcript::Panel;
use radioctl_core::units::{Hz, Vfo};

use crate::serial_vendor::{vendor_of, TwinSnapshot, VendorTwin};

const CR: u8 = b'\r';
const ACK: u8 = 0x06;
const NAK: u8 = 0x15;
const PROMPT: u8 = b'>';

/// Largest frequency in 100 Hz units the six digits hold, as a frequency in hertz.
const MAX_HZ: u64 = 99_999_999;

fn unsupported(what: &str) -> RigError {
    RigError::new(ErrorKind::Unsupported, what.to_string())
}

/// The two Skanti command sets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dialect {
    /// TRP8000: CR-ended commands, `>` prompt.
    Trp8000,
    /// TRP 8255 S R: byte-wise acknowledgements.
    Trp8255,
}

impl Dialect {
    // [@ANCHOR: radioctl_skanti_dialect_from_name]
    /// The dialect with a file-format name.
    pub fn from_name(name: &str) -> Option<Dialect> {
        match name {
            "trp8000" => Some(Dialect::Trp8000),
            "trp8255" => Some(Dialect::Trp8255),
            _ => None,
        }
    }
}

// [@ANCHOR: radioctl_skanti_units]
/// A frequency in 100 Hz units for the six-digit field, or `None` above 99.999999 MHz.
pub fn units(hz: Hz) -> Option<u64> {
    (hz.0 <= MAX_HZ).then_some(hz.0 / 100)
}

// [@ANCHOR: radioctl_skanti_mode_text]
/// What the TRP8000 takes for a mode code of the description (`A` is CW and is sent as `A1`).
fn mode_text(code: u8) -> String {
    if code == b'A' {
        "A1".to_string()
    } else {
        char::from(code).to_string()
    }
}

/// The Skanti client engine.
#[derive(Debug)]
pub struct SkantiEngine {
    desc: RigDescription,
    dialect: Dialect,
    /// Acknowledgements (TRP 8255: bytes sent; TRP8000: prompts) still to come.
    owed: usize,
}

impl SkantiEngine {
    // [@ANCHOR: radioctl_skanti_engine_new]
    /// Builds an engine from a Skanti vendor description.
    pub fn new(desc: RigDescription) -> Result<SkantiEngine, RigError> {
        let (_, name) = vendor_of(&desc)?;
        let dialect = Dialect::from_name(name)
            .ok_or_else(|| unsupported("unknown Skanti dialect in the description"))?;
        Ok(SkantiEngine {
            desc,
            dialect,
            owed: 0,
        })
    }
}

impl ProtocolEngine for SkantiEngine {
    fn description(&self) -> &RigDescription {
        &self.desc
    }

    // [@ANCHOR: radioctl_skanti_engine_encode]
    fn encode(&mut self, op: &Op) -> Result<Request, RigError> {
        if !self.desc.capabilities.has(op.required_capability()) {
            return Err(unsupported("capability not in the rig description"));
        }
        let eight = self.dialect == Dialect::Trp8255;
        let (bytes, owed) = match op {
            Op::SetFrequency(Vfo::A, hz) => {
                let u = units(*hz).ok_or_else(|| unsupported("frequency above 100 MHz"))?;
                if eight {
                    let mut b = format!(":{u:06}\r").into_bytes();
                    b.extend_from_slice(b":;\r");
                    let n = b.len();
                    (b, n)
                } else {
                    (format!("Z{u:06}\r").into_bytes(), 1)
                }
            }
            Op::SetMode(mode) => {
                let code = self
                    .desc
                    .mode_code(*mode)
                    .ok_or_else(|| unsupported("mode not in the description"))?;
                if eight {
                    (vec![code], 1)
                } else {
                    let mut b = mode_text(code).into_bytes();
                    b.push(CR);
                    (b, 1)
                }
            }
            Op::SetPtt(on) => {
                if eight {
                    (vec![if *on { b'u' } else { b'v' }], 1)
                } else {
                    (vec![b'X', if *on { b'N' } else { b'F' }, CR], 1)
                }
            }
            _ => return Err(unsupported("not implemented for the Skanti engine")),
        };
        self.owed = owed;
        Ok(Request {
            bytes,
            reply: ReplyKind::Ack,
        })
    }

    // [@ANCHOR: radioctl_skanti_engine_feed]
    fn feed(&mut self, bytes: &[u8], _now: Duration) -> Vec<Inbound> {
        let mut out = Vec::new();
        let (good, bad) = match self.dialect {
            Dialect::Trp8000 => (PROMPT, None),
            Dialect::Trp8255 => (ACK, Some(NAK)),
        };
        for b in bytes {
            if *b == good && self.owed > 0 {
                self.owed -= 1;
                if self.owed == 0 {
                    out.push(Inbound::Ack);
                }
            } else if Some(*b) == bad {
                self.owed = 0;
                out.push(Inbound::Nak);
            } else if !matches!(*b, CR | b'\n') {
                out.push(Inbound::Unrecognised(vec![*b]));
            }
        }
        out
    }
}

/// A virtual Skanti transceiver.
#[derive(Debug)]
pub struct SkantiTwin {
    dialect: Dialect,
    codes: Vec<u8>,
    line: Vec<u8>,
    rx_units: u64,
    tx_units: u64,
    mode: u8,
    ptt: bool,
}

impl SkantiTwin {
    // [@ANCHOR: radioctl_skanti_twin_new]
    /// A twin in the description's default state.
    pub fn new(desc: &RigDescription) -> Result<SkantiTwin, RigError> {
        let (_, name) = vendor_of(desc)?;
        let dialect = Dialect::from_name(name)
            .ok_or_else(|| unsupported("unknown Skanti dialect in the description"))?;
        let codes: Vec<u8> = desc.mode_codes.iter().map(|(_, c)| *c).collect();
        Ok(SkantiTwin {
            dialect,
            mode: desc
                .mode_code(desc.default_mode)
                .or_else(|| codes.first().copied())
                .unwrap_or(b'X'),
            codes,
            line: Vec::new(),
            rx_units: desc.default_frequency.0 / 100,
            tx_units: desc.default_frequency.0 / 100,
            ptt: false,
        })
    }

    // [@ANCHOR: radioctl_skanti_twin_apply_8000]
    /// Applies one TRP8000 command line; `false` for one the radio does not know.
    fn apply_8000(&mut self, line: &[u8]) -> bool {
        match line {
            [b'Z', digits @ ..] if digits.len() == 6 => {
                match crate::serial_vendor::parse_decimal(digits) {
                    Some(u) => {
                        self.rx_units = u;
                        true
                    }
                    None => false,
                }
            }
            [b'T', digits @ ..] if digits.len() == 6 => {
                match crate::serial_vendor::parse_decimal(digits) {
                    Some(u) => {
                        self.tx_units = u;
                        true
                    }
                    None => false,
                }
            }
            [b'X', b'N'] => {
                self.ptt = true;
                true
            }
            [b'X', b'F'] => {
                self.ptt = false;
                true
            }
            [b'A', b'1'] if self.codes.contains(&b'A') => {
                self.mode = b'A';
                true
            }
            [c] if self.codes.contains(c) && *c != b'A' => {
                self.mode = *c;
                true
            }
            // Filter selections and AGC, RF and power settings that Hamlib sends after a mode
            // write: acknowledged, not kept.
            [b'I' | b'V' | b'N' | b'W'] | [b'A', b'S' | b'A' | b'F'] => true,
            _ => false,
        }
    }

    // [@ANCHOR: radioctl_skanti_twin_feed_8255]
    /// One byte of the control-unit dialect: the answer byte.
    fn feed_8255(&mut self, b: u8) -> u8 {
        if !self.line.is_empty() {
            self.line.push(b);
            if b == CR {
                let line = std::mem::take(&mut self.line);
                return if self.apply_8255_line(&line) {
                    ACK
                } else {
                    NAK
                };
            }
            return ACK;
        }
        match b {
            b':' | b';' | b'<' | b'd' | b'y' => {
                self.line.push(b);
                ACK
            }
            0x01 | 0x02 | 0x16 | b'R' | b'C' | b'B' | b'D' | b'm' | b'n' | b'o' | b'p' | b'J'
            | b'K' | b'L' | b'M' | b'S' | b'U' | b'W' | b'k' | b'l' => ACK,
            b'u' => {
                self.ptt = true;
                ACK
            }
            b'v' => {
                self.ptt = false;
                ACK
            }
            c if self.codes.contains(&c) => {
                self.mode = c;
                ACK
            }
            _ => NAK,
        }
    }

    // [@ANCHOR: radioctl_skanti_twin_apply_8255_line]
    /// A completed control-unit line (CR included).
    fn apply_8255_line(&mut self, line: &[u8]) -> bool {
        match line {
            [b':', b';', CR] => {
                self.tx_units = self.rx_units;
                true
            }
            [b':', digits @ .., CR] if digits.len() == 6 => {
                crate::serial_vendor::parse_decimal(digits).is_some_and(|u| {
                    self.rx_units = u;
                    true
                })
            }
            [b';', digits @ .., CR] if digits.len() == 6 => {
                crate::serial_vendor::parse_decimal(digits).is_some_and(|u| {
                    self.tx_units = u;
                    true
                })
            }
            [b'<' | b'd' | b'y', .., CR] => true,
            _ => false,
        }
    }
}

impl VendorTwin for SkantiTwin {
    // [@ANCHOR: radioctl_skanti_twin_snapshot]
    fn snapshot(&self) -> Option<TwinSnapshot> {
        Some(TwinSnapshot {
            frequency: Hz(self.rx_units * 100),
            mode_code: Some(self.mode),
            ptt: self.ptt,
        })
    }

    // [@ANCHOR: radioctl_skanti_twin_feed]
    fn feed(&mut self, bytes: &[u8], _now: Duration) -> Vec<u8> {
        let mut out = Vec::new();
        match self.dialect {
            Dialect::Trp8255 => {
                for b in bytes {
                    out.push(self.feed_8255(*b));
                }
            }
            Dialect::Trp8000 => {
                for b in bytes {
                    if *b == CR {
                        let line = std::mem::take(&mut self.line);
                        if self.apply_8000(&line) {
                            out.push(PROMPT);
                        }
                    } else {
                        self.line.push(*b);
                    }
                }
            }
        }
        out
    }

    // [@ANCHOR: radioctl_skanti_twin_front_panel]
    fn front_panel(&mut self, panel: Panel) -> Result<Vec<u8>, String> {
        match panel {
            Panel::Frequency(hz) => {
                self.rx_units = hz.0 / 100;
                self.tx_units = hz.0 / 100;
            }
            Panel::Ptt(on) => self.ptt = on,
            other => return Err(format!("a Skanti twin has no {other:?} knob")),
        }
        Ok(Vec::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use radioctl_core::units::Mode;

    const T0: Duration = Duration::from_secs(1);

    fn send(e: &mut SkantiEngine, t: &mut SkantiTwin, op: Op) -> (Vec<u8>, Vec<Inbound>) {
        let r = e.encode(&op).expect("encodes");
        assert_eq!(r.reply, ReplyKind::Ack);
        let answer = t.feed(&r.bytes, T0);
        (r.bytes, e.feed(&answer, T0))
    }

    fn shipped(id: &str) -> RigDescription {
        let path = format!("{}/../../rigs/{id}.toml", env!("CARGO_MANIFEST_DIR"));
        let text = std::fs::read_to_string(path).expect("rig file");
        radioctl_core::rigfile::parse(&text)
            .expect("loads")
            .description
    }

    // Tests [@ANCHOR: radioctl_skanti_dialect_from_name]
    // Tests [@ANCHOR: radioctl_skanti_units]
    // Tests [@ANCHOR: radioctl_skanti_mode_text]
    // Tests [@ANCHOR: radioctl_skanti_engine_new]
    #[test]
    fn units_are_hundreds_of_hertz_in_six_digits_and_other_families_are_refused() {
        assert_eq!(Dialect::from_name("trp8255"), Some(Dialect::Trp8255));
        assert_eq!(Dialect::from_name("x"), None);
        assert_eq!(units(Hz(14_200_000)), Some(142_000));
        assert_eq!(units(Hz(99_999_999)), Some(999_999));
        assert_eq!(units(Hz(100_000_000)), None);
        assert_eq!(mode_text(b'A'), "A1");
        assert_eq!(mode_text(b'J'), "J");
        assert!(SkantiEngine::new(RigDescription::ts2000()).is_err());
        assert!(SkantiTwin::new(&RigDescription::ts2000()).is_err());
    }

    // Tests [@ANCHOR: radioctl_skanti_engine_encode]
    // Tests [@ANCHOR: radioctl_skanti_engine_feed]
    // Tests [@ANCHOR: radioctl_skanti_twin_new]
    // Tests [@ANCHOR: radioctl_skanti_twin_feed]
    // Tests [@ANCHOR: radioctl_skanti_twin_apply_8000]
    // Tests [@ANCHOR: radioctl_skanti_twin_apply_8255_line]
    // Tests [@ANCHOR: radioctl_skanti_twin_feed_8255]
    // Tests [@ANCHOR: radioctl_skanti_twin_snapshot]
    // Tests [@ANCHOR: radioctl_skanti_twin_front_panel]
    #[test]
    fn both_models_are_set_and_acknowledged() {
        for (id, freq_wire, ptt_wire, mode_wires) in [
            (
                "skanti-trp8000",
                &b"Z142000\r"[..],
                &b"XN\r"[..],
                [
                    (Mode::Cw, &b"A1\r"[..]),
                    (Mode::Usb, b"J\r"),
                    (Mode::Lsb, b"L\r"),
                ],
            ),
            (
                "skanti-trp-8255-s-r",
                &b":142000\r:;\r"[..],
                &b"u"[..],
                [(Mode::Cw, &b"]"[..]), (Mode::Usb, b"X"), (Mode::Lsb, b"Y")],
            ),
        ] {
            let desc = shipped(id);
            let mut e = SkantiEngine::new(desc.clone()).expect("engine");
            let mut t = SkantiTwin::new(&desc).expect("twin");
            let (wire, seen) = send(&mut e, &mut t, Op::SetFrequency(Vfo::A, Hz(14_200_000)));
            assert_eq!(wire, freq_wire, "{id}");
            assert_eq!(seen, vec![Inbound::Ack], "{id}");
            for (mode, expect) in mode_wires {
                let (wire, seen) = send(&mut e, &mut t, Op::SetMode(mode));
                assert_eq!(wire, expect, "{id} {mode:?}");
                assert_eq!(seen, vec![Inbound::Ack], "{id} {mode:?}");
                let snap = t.snapshot().expect("snapshot");
                assert_eq!(snap.mode_code.and_then(|c| desc.code_mode(c)), Some(mode));
            }
            let (wire, seen) = send(&mut e, &mut t, Op::SetPtt(true));
            assert_eq!(wire, ptt_wire, "{id}");
            assert_eq!(seen, vec![Inbound::Ack], "{id}");
            assert!(t.snapshot().expect("snapshot").ptt);
            send(&mut e, &mut t, Op::SetPtt(false));
            assert!(!t.snapshot().expect("snapshot").ptt);
            assert_eq!(
                t.snapshot().expect("snapshot").frequency,
                Hz(14_200_000),
                "{id}"
            );
            t.front_panel(Panel::Frequency(Hz(5_000_000)))
                .expect("tunes");
            assert_eq!(t.snapshot().expect("snapshot").frequency, Hz(5_000_000));
            assert!(t.front_panel(Panel::Mode(Mode::Am)).is_err());
            assert!(e.encode(&Op::GetFrequency(Vfo::A)).is_err());
            assert!(e
                .encode(&Op::SetFrequency(Vfo::A, Hz(120_000_000)))
                .is_err());
            assert!(matches!(
                e.feed(b"?", T0).as_slice(),
                [Inbound::Unrecognised(_)]
            ));
        }
        // Refusals: the control unit NAKs what it does not know; the TRP8000 stays silent.
        let mut t = SkantiTwin::new(&shipped("skanti-trp-8255-s-r")).expect("twin");
        assert_eq!(t.feed(b"#", T0), vec![NAK]);
        assert_eq!(t.feed(b":12x456\r", T0).last(), Some(&NAK));
        let mut t = SkantiTwin::new(&shipped("skanti-trp8000")).expect("twin");
        assert!(t.feed(b"Q9\r", T0).is_empty());
        let desc = shipped("skanti-trp-8255-s-r");
        let mut e = SkantiEngine::new(desc).expect("engine");
        e.encode(&Op::SetMode(Mode::Usb)).expect("encodes");
        assert_eq!(e.feed(&[NAK], T0), vec![Inbound::Nak]);
    }
}
