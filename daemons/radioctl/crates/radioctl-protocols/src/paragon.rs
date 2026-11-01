// SPDX-License-Identifier: AGPL-3.0-or-later
//! Ten-Tec TT-585 Paragon: sans-IO client engine and virtual twin.
//!
//! **Sources.** Facts only, read from Hamlib 4.6.2 (released tarball, sha256
//! b2ac73f44dd1161e95fdee6c95276144757647bf92d7fdb369ee2fe41ed47ae8): `rigs/tentec/paragon.c`.
//! No Ten-Tec document for the Paragon was found (the other Ten-Tec programmer's guides in
//! `~/manuals/tentec` are for other radios); nothing is verified against a radio.
//!
//! The link is 1200 baud, 8N1, with long gaps between writes (Hamlib: 100 ms between bytes and
//! 200 ms after a command). Commands are bare characters, no terminator:
//!
//! * a frequency is megahertz with five decimals, the decimal point written as `W`, then `@`
//!   (`7W07400@` is 7.074 MHz): a resolution of 10 Hz;
//! * a mode is one letter (`N` LSB, `O` USB, `P` CW, `L` FM, `M` AM) or `XP` for RTTY;
//! * `\` asks for the status, a block of 30 bytes: bytes 0 to 6 are the digits of the frequency in
//!   units of 10 Hz (each byte holds the digit's value, not its ASCII code); byte 7 has the mode
//!   as a bit (0x02 CW, 0x04 USB, 0x08 LSB, 0x10 AM, 0x20 FM, 0x40 RTTY); byte 8 the filter as a bit
//!   (0x08 250 Hz, 0x10 500 Hz, 0x20 1.8 kHz, 0x40 2.4 kHz, 0x80 6 kHz); byte 9 has the VFO (0x08 A)
//!   and split (0x02); byte 11 the memory channel (above 61: not in memory mode).
//!
//! Not in the engine: VFO and split (Hamlib only toggles them: `F` and `J`), memories, the filter
//! letters (`R` to `V`; Hamlib's own width write sends the wrong length) and the announce
//! toggle `>`. The Paragon has no PTT command. Radioctl treats the status frequency as VFO A.

use std::time::Duration;

use radioctl_core::description::RigDescription;
use radioctl_core::engine::{Inbound, ProtocolEngine, ReplyKind, Request};
use radioctl_core::error::{ErrorKind, RigError};
use radioctl_core::op::{Field, Observation, Op};
use radioctl_core::transcript::Panel;
use radioctl_core::units::{Hz, Mode, Vfo};

use crate::serial_vendor::{vendor_of, RxBuffer, TwinSnapshot, VendorTwin};

/// Length of the status block.
pub const STATUS_LEN: usize = 30;
/// Lowest frequency, hertz (Hamlib's receive range).
pub const MIN_HZ: u64 = 100_000;
/// Highest frequency, hertz: Hamlib's range ends ten hertz below 30 MHz.
pub const MAX_HZ: u64 = 29_999_990;

/// The status mode bits and their modes, in the order Hamlib tests them.
const MODE_BITS: [(u8, Mode); 6] = [
    (0x02, Mode::Cw),
    (0x04, Mode::Usb),
    (0x08, Mode::Lsb),
    (0x10, Mode::Am),
    (0x20, Mode::Fm),
    (0x40, Mode::Rtty),
];

fn unsupported(what: &str) -> RigError {
    RigError::new(ErrorKind::Unsupported, what.to_string())
}

// [@ANCHOR: radioctl_paragon_frequency_text]
/// The frequency command: megahertz with five decimals, `W` for the point, `@` at the end.
pub fn frequency_text(hz: Hz) -> String {
    let units = (hz.0 + 5) / 10;
    format!("{}W{:05}@", units / 100_000, units % 100_000)
}

// [@ANCHOR: radioctl_paragon_mode_command]
/// The command that selects `mode`, or `None` for a mode the radio has not got.
pub fn mode_command(mode: Mode) -> Option<&'static str> {
    Some(match mode {
        Mode::Lsb => "N",
        Mode::Usb => "O",
        Mode::Cw => "P",
        Mode::Fm => "L",
        Mode::Am => "M",
        Mode::Rtty => "XP",
        _ => return None,
    })
}

// [@ANCHOR: radioctl_paragon_decode_status]
/// What a 30-byte status block says: the frequency, and the mode if one bit is set.
pub fn decode_status(block: &[u8]) -> Option<(Hz, Option<Mode>)> {
    if block.len() != STATUS_LEN {
        return None;
    }
    let digits = block.get(..7)?;
    if digits.iter().any(|d| *d > 9) {
        return None;
    }
    let units = digits.iter().fold(0u64, |a, d| a * 10 + u64::from(*d));
    let mode_byte = *block.get(7)?;
    let mode = MODE_BITS
        .iter()
        .find(|(bit, _)| mode_byte & bit != 0)
        .map(|(_, mode)| *mode);
    Some((Hz(units * 10), mode))
}

/// The Paragon client engine.
#[derive(Debug)]
pub struct ParagonEngine {
    desc: RigDescription,
    rx: RxBuffer,
}

impl ParagonEngine {
    // [@ANCHOR: radioctl_paragon_engine_new]
    /// Builds an engine from a Paragon vendor description.
    pub fn new(desc: RigDescription) -> Result<ParagonEngine, RigError> {
        let (_, name) = vendor_of(&desc)?;
        if name != "paragon" {
            return Err(unsupported("unknown Paragon dialect in the description"));
        }
        Ok(ParagonEngine {
            desc,
            rx: RxBuffer::default(),
        })
    }
}

impl ProtocolEngine for ParagonEngine {
    fn description(&self) -> &RigDescription {
        &self.desc
    }

    // [@ANCHOR: radioctl_paragon_engine_encode]
    fn encode(&mut self, op: &Op) -> Result<Request, RigError> {
        if !self.desc.capabilities.has(op.required_capability()) {
            return Err(unsupported("capability not in the rig description"));
        }
        let (text, reply) = match op {
            Op::GetFrequency(Vfo::A) => (
                "\\".to_string(),
                ReplyKind::Observation(Field::Frequency(Vfo::A)),
            ),
            Op::GetMode => ("\\".to_string(), ReplyKind::Observation(Field::Mode)),
            Op::SetFrequency(Vfo::A, hz) => {
                if hz.0 < MIN_HZ || hz.0 > MAX_HZ {
                    return Err(unsupported("frequency outside 100 kHz to 30 MHz"));
                }
                (frequency_text(*hz), ReplyKind::None)
            }
            Op::SetMode(mode) => {
                if self.desc.mode_code(*mode).is_none() {
                    return Err(unsupported("mode not in the description"));
                }
                let command =
                    mode_command(*mode).ok_or_else(|| unsupported("mode the radio has not got"))?;
                (command.to_string(), ReplyKind::None)
            }
            _ => return Err(unsupported("not implemented for the Paragon engine")),
        };
        self.rx.take_all();
        Ok(Request {
            bytes: text.into_bytes(),
            reply,
        })
    }

    // [@ANCHOR: radioctl_paragon_engine_feed]
    fn feed(&mut self, bytes: &[u8], now: Duration) -> Vec<Inbound> {
        let mut out = Vec::new();
        if let Some(stale) = self.rx.push(bytes, now) {
            out.push(Inbound::Unrecognised(stale));
        }
        while let Some(block) = self.rx.take(STATUS_LEN) {
            match decode_status(&block) {
                Some((hz, mode)) => {
                    out.push(Inbound::Observation(Observation::Frequency(Vfo::A, hz)));
                    if let Some(mode) = mode {
                        out.push(Inbound::Observation(Observation::Mode(mode)));
                    }
                }
                None => out.push(Inbound::Unrecognised(block)),
            }
        }
        out
    }
}

/// A virtual Paragon.
#[derive(Debug)]
pub struct ParagonTwin {
    rx: RxBuffer,
    units: u64,
    mode_bit: u8,
    filter_bit: u8,
    vfo_a: bool,
    split: bool,
}

impl ParagonTwin {
    // [@ANCHOR: radioctl_paragon_twin_new]
    /// A twin in the description's default state.
    pub fn new(desc: &RigDescription) -> Result<ParagonTwin, RigError> {
        let (_, name) = vendor_of(desc)?;
        if name != "paragon" {
            return Err(unsupported("unknown Paragon dialect in the description"));
        }
        Ok(ParagonTwin {
            rx: RxBuffer::default(),
            units: desc.default_frequency.0 / 10,
            mode_bit: desc.mode_code(desc.default_mode).unwrap_or(0x04),
            filter_bit: 0x40,
            vfo_a: true,
            split: false,
        })
    }

    // [@ANCHOR: radioctl_paragon_twin_status]
    fn status(&self) -> Vec<u8> {
        let mut block = vec![0u8; STATUS_LEN];
        let mut rest = self.units;
        for slot in block.iter_mut().take(7).rev() {
            *slot = u8::try_from(rest % 10).unwrap_or(0);
            rest /= 10;
        }
        if let Some(b) = block.get_mut(7) {
            *b = self.mode_bit;
        }
        if let Some(b) = block.get_mut(8) {
            *b = self.filter_bit;
        }
        if let Some(b) = block.get_mut(9) {
            *b = (if self.vfo_a { 0x08 } else { 0 }) | (if self.split { 0x02 } else { 0 });
        }
        if let Some(b) = block.get_mut(11) {
            *b = 63;
        }
        block
    }
}

impl VendorTwin for ParagonTwin {
    // [@ANCHOR: radioctl_paragon_twin_snapshot]
    fn snapshot(&self) -> Option<TwinSnapshot> {
        Some(TwinSnapshot {
            frequency: Hz(self.units * 10),
            mode_code: Some(self.mode_bit),
            ptt: false,
        })
    }

    // [@ANCHOR: radioctl_paragon_twin_feed]
    fn feed(&mut self, bytes: &[u8], now: Duration) -> Vec<u8> {
        self.rx.push(bytes, now);
        let mut out = Vec::new();
        loop {
            let pending = self.rx.peek().to_vec();
            let Some(&first) = pending.first() else {
                break;
            };
            // How many bytes the next command takes; 0 = not complete yet.
            let used = match first {
                b'0'..=b'9' | b'W' => match pending.iter().position(|b| *b == b'@') {
                    Some(end) => {
                        let text: String = pending
                            .get(..end)
                            .unwrap_or(&[])
                            .iter()
                            .map(|b| if *b == b'W' { '.' } else { char::from(*b) })
                            .collect();
                        if let Ok(mhz) = text.parse::<f64>() {
                            let units = (mhz * 100_000.0).round();
                            if (0.0..1.0e8).contains(&units) {
                                self.units = units as u64;
                            }
                        }
                        end + 1
                    }
                    None => 0,
                },
                b'X' => match pending.get(1) {
                    Some(b'P') => {
                        self.mode_bit = 0x40;
                        2
                    }
                    Some(_) => 2,
                    None => 0,
                },
                b':' | b'<' => {
                    if pending.len() >= 3 {
                        3
                    } else {
                        0
                    }
                }
                other => {
                    match other {
                        b'N' => self.mode_bit = 0x08,
                        b'O' => self.mode_bit = 0x04,
                        b'P' => self.mode_bit = 0x02,
                        b'L' => self.mode_bit = 0x20,
                        b'M' => self.mode_bit = 0x10,
                        b'R' => self.filter_bit = 0x80,
                        b'S' => self.filter_bit = 0x40,
                        b'T' => self.filter_bit = 0x20,
                        b'U' => self.filter_bit = 0x10,
                        b'V' => self.filter_bit = 0x08,
                        b'F' => self.vfo_a = !self.vfo_a,
                        b'J' => self.split = !self.split,
                        b'\\' => out.extend(self.status()),
                        _ => {}
                    }
                    1
                }
            };
            if used == 0 {
                break;
            }
            self.rx.take(used);
        }
        out
    }

    // [@ANCHOR: radioctl_paragon_twin_front_panel]
    fn front_panel(&mut self, panel: Panel) -> Result<Vec<u8>, String> {
        match panel {
            Panel::Frequency(hz) => self.units = hz.0.clamp(MIN_HZ, MAX_HZ) / 10,
            Panel::Mode(mode) => {
                self.mode_bit = MODE_BITS
                    .iter()
                    .find(|(_, m)| *m == mode)
                    .map(|(bit, _)| *bit)
                    .ok_or_else(|| format!("a Paragon has no {mode:?} mode"))?;
            }
            other => return Err(format!("a Paragon twin has no {other:?} knob")),
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
            "{}/../../rigs/tentec-tt-585.toml",
            env!("CARGO_MANIFEST_DIR")
        );
        let text = std::fs::read_to_string(path).expect("rig file");
        radioctl_core::rigfile::parse(&text)
            .expect("loads")
            .description
    }

    // Tests [@ANCHOR: radioctl_paragon_frequency_text]
    // Tests [@ANCHOR: radioctl_paragon_mode_command]
    // Tests [@ANCHOR: radioctl_paragon_decode_status]
    #[test]
    fn frames_and_status_blocks_round_trip() {
        assert_eq!(frequency_text(Hz(7_074_000)), "7W07400@");
        assert_eq!(frequency_text(Hz(14_195_550)), "14W19555@");
        assert_eq!(frequency_text(Hz(100_000)), "0W10000@");
        assert_eq!(mode_command(Mode::Rtty), Some("XP"));
        assert_eq!(mode_command(Mode::Wfm), None);
        let mut block = vec![0u8; STATUS_LEN];
        block[..7].copy_from_slice(&[0, 7, 0, 7, 4, 0, 0]);
        block[7] = 0x04;
        assert_eq!(
            decode_status(&block),
            Some((Hz(7_074_000), Some(Mode::Usb)))
        );
        block[7] = 0;
        assert_eq!(decode_status(&block), Some((Hz(7_074_000), None)));
        block[0] = 10;
        assert_eq!(decode_status(&block), None);
        assert_eq!(decode_status(&block[..29]), None);
    }

    // Tests [@ANCHOR: radioctl_paragon_engine_new]
    #[test]
    fn other_families_are_refused() {
        assert!(ParagonEngine::new(RigDescription::ts2000()).is_err());
        assert!(ParagonTwin::new(&RigDescription::ts2000()).is_err());
    }

    // Tests [@ANCHOR: radioctl_paragon_engine_encode]
    // Tests [@ANCHOR: radioctl_paragon_engine_feed]
    // Tests [@ANCHOR: radioctl_paragon_twin_new]
    // Tests [@ANCHOR: radioctl_paragon_twin_feed]
    // Tests [@ANCHOR: radioctl_paragon_twin_status]
    // Tests [@ANCHOR: radioctl_paragon_twin_snapshot]
    // Tests [@ANCHOR: radioctl_paragon_twin_front_panel]
    #[test]
    fn the_engine_and_the_twin_agree_on_frequency_and_mode() {
        let desc = shipped();
        let mut e = ParagonEngine::new(desc.clone()).expect("engine");
        let mut t = ParagonTwin::new(&desc).expect("twin");
        let r = e
            .encode(&Op::SetFrequency(Vfo::A, Hz(7_074_000)))
            .expect("encodes");
        assert_eq!(r.bytes, b"7W07400@");
        assert_eq!(r.reply, ReplyKind::None);
        assert!(t.feed(&r.bytes, T0).is_empty());
        assert_eq!(t.snapshot().expect("snapshot").frequency, Hz(7_074_000));
        for (mode, bit) in [
            (Mode::Lsb, 0x08),
            (Mode::Usb, 0x04),
            (Mode::Cw, 0x02),
            (Mode::Am, 0x10),
            (Mode::Fm, 0x20),
            (Mode::Rtty, 0x40),
        ] {
            let r = e.encode(&Op::SetMode(mode)).expect("encodes");
            assert!(t.feed(&r.bytes, T0).is_empty());
            assert_eq!(t.snapshot().expect("snapshot").mode_code, Some(bit));
            let r = e.encode(&Op::GetMode).expect("encodes");
            let answer = t.feed(&r.bytes, T0);
            assert_eq!(answer.len(), STATUS_LEN);
            assert_eq!(
                e.feed(&answer, T0),
                vec![
                    Inbound::Observation(Observation::Frequency(Vfo::A, Hz(7_074_000))),
                    Inbound::Observation(Observation::Mode(mode)),
                ]
            );
        }
        // The status arriving in two pieces, and a stale half block.
        let answer = t.feed(b"\\", T0);
        assert!(e.feed(&answer[..10], T0).is_empty());
        assert_eq!(e.feed(&answer[10..], T0).len(), 2);
        assert!(e.feed(&answer[..10], T0).is_empty());
        assert!(matches!(
            e.feed(&answer, T0 + Duration::from_secs(5)).as_slice(),
            [Inbound::Unrecognised(_), ..]
        ));
        // The twin takes a command split across writes and the toggles Hamlib uses.
        assert!(t.feed(b"14W1", T0).is_empty());
        assert!(t.feed(b"9555@", T0).is_empty());
        assert_eq!(t.snapshot().expect("snapshot").frequency, Hz(14_195_550));
        assert!(t.feed(b"FJV:05<06XYQ", T0).is_empty());
        let status = t.feed(b"\\", T0);
        assert_eq!(status.get(8), Some(&0x08));
        assert_eq!(status.get(9), Some(&0x02));
        assert_eq!(status.get(11), Some(&63));
        // Refusals.
        assert!(e.encode(&Op::SetFrequency(Vfo::A, Hz(99_990))).is_err());
        assert!(e.encode(&Op::SetFrequency(Vfo::A, Hz(30_000_000))).is_err());
        assert!(e.encode(&Op::SetMode(Mode::Wfm)).is_err());
        assert!(e.encode(&Op::SetPtt(true)).is_err());
        assert!(e.encode(&Op::GetFrequency(Vfo::B)).is_err());
        t.front_panel(Panel::Frequency(Hz(3_573_000)))
            .expect("tunes");
        t.front_panel(Panel::Mode(Mode::Lsb)).expect("mode");
        assert!(t.front_panel(Panel::Mode(Mode::Wfm)).is_err());
        assert!(t.front_panel(Panel::Signal(3)).is_err());
        assert_eq!(t.snapshot().expect("snapshot").frequency, Hz(3_573_000));
    }
}
