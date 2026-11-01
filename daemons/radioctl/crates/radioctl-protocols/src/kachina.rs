// SPDX-License-Identifier: AGPL-3.0-or-later
//! Kachina 505DSP transceiver: sans-IO client engine and virtual twin.
//!
//! **Sources.** Facts only, read from Hamlib 4.6.2 (released tarball, sha256
//! b2ac73f44dd1161e95fdee6c95276144757647bf92d7fdb369ee2fe41ed47ae8): `rigs/kachina/kachina.c`
//! and `505dsp.c`. No Kachina document was read; nothing is verified against a radio.
//! Hamlib has no frequency or mode read for this radio (only a signal-strength read), so the
//! description has none.
//!
//! Every command is STX (02), a command letter, data and ETX (03); the radio answers a single byte,
//! FF for good (FE for an error).
//!
//! * `R` and four bytes set the receive frequency, `T` and four bytes the transmit frequency
//!   (Hamlib sends both for one frequency). The four bytes carry a DDS word: `2.2369621333 *
//!   (75 000 000 + f)` rounded down, big-endian, with the top byte's two high bits replaced by the
//!   antenna port (`01` for port A).
//! * `M` and a mode byte (1 AM, 2 CW, 3 FM, 4 USB, 5 LSB); the description's mode codes are the
//!   ASCII digits `1` to `5`.
//! * `x` and 0 or 1 unkey and key.

use std::time::Duration;

use radioctl_core::description::RigDescription;
use radioctl_core::engine::{Inbound, ProtocolEngine, ReplyKind, Request};
use radioctl_core::error::{ErrorKind, RigError};
use radioctl_core::op::Op;
use radioctl_core::transcript::Panel;
use radioctl_core::units::{Hz, Vfo};

use crate::serial_vendor::{vendor_of, RxBuffer, TwinSnapshot, VendorTwin};

const STX: u8 = 0x02;
const ETX: u8 = 0x03;
const GOOD: u8 = 0xff;
const ERROR: u8 = 0xfe;
/// Antenna port A in the top two bits of the first DDS byte.
const PORT_A: u8 = 0x40;
const DDS_BASE: f64 = 75_000_000.0;
const DDS_CONST: f64 = 2.236_962_133_3;

fn unsupported(what: &str) -> RigError {
    RigError::new(ErrorKind::Unsupported, what.to_string())
}

// [@ANCHOR: radioctl_kachina_dds_bytes]
/// The four bytes of a frequency (30 kHz to 30 MHz), or `None` outside that range.
pub fn dds_bytes(hz: Hz) -> Option<[u8; 4]> {
    if !(30_000..=30_000_000).contains(&hz.0) {
        return None;
    }
    // The product is below 2^30, so the conversion is exact.
    let dds = (DDS_CONST * (DDS_BASE + hz.0 as f64)) as u64;
    let b = u32::try_from(dds).ok()?.to_be_bytes();
    Some([PORT_A | (b[0] & 0x3f), b[1], b[2], b[3]])
}

// [@ANCHOR: radioctl_kachina_frequency_of]
/// The frequency of four DDS bytes (the antenna bits are ignored), rounded to the hertz.
pub fn frequency_of(bytes: [u8; 4]) -> Hz {
    let dds = u32::from_be_bytes([bytes[0] & 0x3f, bytes[1], bytes[2], bytes[3]]);
    let f = f64::from(dds) / DDS_CONST - DDS_BASE;
    Hz(f.round().max(0.0) as u64)
}

// [@ANCHOR: radioctl_kachina_frame]
/// A frame: STX, the command letter, the data and ETX.
fn frame(letter: u8, data: &[u8]) -> Vec<u8> {
    let mut out = vec![STX, letter];
    out.extend_from_slice(data);
    out.push(ETX);
    out
}

/// The Kachina client engine.
#[derive(Debug)]
pub struct KachinaEngine {
    desc: RigDescription,
    owed: usize,
}

impl KachinaEngine {
    // [@ANCHOR: radioctl_kachina_engine_new]
    /// Builds an engine from a Kachina vendor description.
    pub fn new(desc: RigDescription) -> Result<KachinaEngine, RigError> {
        let (_, name) = vendor_of(&desc)?;
        if name != "505dsp" {
            return Err(unsupported("unknown Kachina dialect in the description"));
        }
        Ok(KachinaEngine { desc, owed: 0 })
    }
}

impl ProtocolEngine for KachinaEngine {
    fn description(&self) -> &RigDescription {
        &self.desc
    }

    // [@ANCHOR: radioctl_kachina_engine_encode]
    fn encode(&mut self, op: &Op) -> Result<Request, RigError> {
        if !self.desc.capabilities.has(op.required_capability()) {
            return Err(unsupported("capability not in the rig description"));
        }
        let (bytes, owed) = match op {
            Op::SetFrequency(Vfo::A, hz) => {
                let dds = dds_bytes(*hz)
                    .ok_or_else(|| unsupported("frequency outside 30 kHz to 30 MHz"))?;
                let mut b = frame(b'R', &dds);
                b.extend(frame(b'T', &dds));
                (b, 2)
            }
            Op::SetMode(mode) => {
                let code = self
                    .desc
                    .mode_code(*mode)
                    .ok_or_else(|| unsupported("mode not in the description"))?;
                (frame(b'M', &[code.wrapping_sub(b'0')]), 1)
            }
            Op::SetPtt(on) => (frame(b'x', &[u8::from(*on)]), 1),
            _ => return Err(unsupported("not implemented for the Kachina engine")),
        };
        self.owed = owed;
        Ok(Request {
            bytes,
            reply: ReplyKind::Ack,
        })
    }

    // [@ANCHOR: radioctl_kachina_engine_feed]
    fn feed(&mut self, bytes: &[u8], _now: Duration) -> Vec<Inbound> {
        let mut out = Vec::new();
        for b in bytes {
            match *b {
                GOOD if self.owed > 0 => {
                    self.owed -= 1;
                    if self.owed == 0 {
                        out.push(Inbound::Ack);
                    }
                }
                ERROR => {
                    self.owed = 0;
                    out.push(Inbound::Nak);
                }
                other => out.push(Inbound::Unrecognised(vec![other])),
            }
        }
        out
    }
}

/// A virtual Kachina 505DSP.
#[derive(Debug)]
pub struct KachinaTwin {
    rx: RxBuffer,
    rx_hz: Hz,
    tx_hz: Hz,
    mode: u8,
    codes: Vec<u8>,
    ptt: bool,
}

impl KachinaTwin {
    // [@ANCHOR: radioctl_kachina_twin_new]
    /// A twin in the description's default state.
    pub fn new(desc: &RigDescription) -> Result<KachinaTwin, RigError> {
        let (_, name) = vendor_of(desc)?;
        if name != "505dsp" {
            return Err(unsupported("unknown Kachina dialect in the description"));
        }
        let codes: Vec<u8> = desc.mode_codes.iter().map(|(_, c)| *c).collect();
        Ok(KachinaTwin {
            rx: RxBuffer::default(),
            rx_hz: desc.default_frequency,
            tx_hz: desc.default_frequency,
            mode: desc
                .mode_code(desc.default_mode)
                .or_else(|| codes.first().copied())
                .unwrap_or(b'4'),
            codes,
            ptt: false,
        })
    }

    // [@ANCHOR: radioctl_kachina_twin_frame]
    /// Handles one frame at the front of the buffer: its answer byte, or `None` when more bytes
    /// are needed.
    fn take_frame(&mut self) -> Option<u8> {
        let held = self.rx.peek();
        let first = *held.first()?;
        if first != STX {
            self.rx.take(1);
            return Some(ERROR);
        }
        let letter = *held.get(1)?;
        let data_len = match letter {
            b'R' | b'T' => 4,
            b'M' | b'x' => 1,
            _ => 0,
        };
        let frame = self.rx.take(data_len + 3)?;
        if frame.last() != Some(&ETX) {
            return Some(ERROR);
        }
        let data = &frame[2..2 + data_len];
        Some(match (letter, data) {
            (b'R', [a, b, c, d]) => {
                self.rx_hz = frequency_of([*a, *b, *c, *d]);
                GOOD
            }
            (b'T', [a, b, c, d]) => {
                self.tx_hz = frequency_of([*a, *b, *c, *d]);
                GOOD
            }
            (b'M', [m]) if self.codes.contains(&(m + b'0')) => {
                self.mode = m + b'0';
                GOOD
            }
            (b'x', [p @ (0 | 1)]) => {
                self.ptt = *p == 1;
                GOOD
            }
            _ => ERROR,
        })
    }
}

impl VendorTwin for KachinaTwin {
    // [@ANCHOR: radioctl_kachina_twin_snapshot]
    fn snapshot(&self) -> Option<TwinSnapshot> {
        Some(TwinSnapshot {
            frequency: self.rx_hz,
            mode_code: Some(self.mode),
            ptt: self.ptt,
        })
    }

    // [@ANCHOR: radioctl_kachina_twin_feed]
    fn feed(&mut self, bytes: &[u8], now: Duration) -> Vec<u8> {
        self.rx.push(bytes, now);
        let mut out = Vec::new();
        while let Some(answer) = self.take_frame() {
            out.push(answer);
        }
        out
    }

    // [@ANCHOR: radioctl_kachina_twin_front_panel]
    fn front_panel(&mut self, panel: Panel) -> Result<Vec<u8>, String> {
        match panel {
            Panel::Frequency(hz) => {
                self.rx_hz = hz;
                self.tx_hz = hz;
            }
            Panel::Ptt(on) => self.ptt = on,
            other => return Err(format!("a Kachina twin has no {other:?} knob")),
        }
        Ok(Vec::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use radioctl_core::units::Mode;

    const T0: Duration = Duration::from_secs(1);

    fn run(e: &mut KachinaEngine, t: &mut KachinaTwin, op: Op) -> (Vec<u8>, Vec<Inbound>) {
        let r = e.encode(&op).expect("encodes");
        let a = t.feed(&r.bytes, T0);
        (r.bytes, e.feed(&a, T0))
    }

    fn shipped() -> RigDescription {
        let path = format!(
            "{}/../../rigs/kachina-505dsp.toml",
            env!("CARGO_MANIFEST_DIR")
        );
        let text = std::fs::read_to_string(path).expect("rig file");
        radioctl_core::rigfile::parse(&text)
            .expect("loads")
            .description
    }

    // Tests [@ANCHOR: radioctl_kachina_dds_bytes]
    // Tests [@ANCHOR: radioctl_kachina_frequency_of]
    #[test]
    fn dds_words_round_trip_for_whole_hertz() {
        for hz in [
            30_000, 1_800_000, 7_074_000, 14_074_123, 29_999_999, 30_000_000,
        ] {
            let b = dds_bytes(Hz(hz)).expect("bytes");
            assert_eq!(b[0] & 0xc0, PORT_A);
            assert_eq!(frequency_of(b), Hz(hz), "{hz}");
        }
        // 2.2369621333 * (75 000 000 + 14 000 000) = 199 089 629.9, rounded down: 0x0bdd_dddd.
        assert_eq!(dds_bytes(Hz(14_000_000)), Some([0x4b, 0xdd, 0xdd, 0xdd]));
        assert_eq!(dds_bytes(Hz(29_999)), None);
        assert_eq!(dds_bytes(Hz(30_000_001)), None);
    }

    // Tests [@ANCHOR: radioctl_kachina_engine_new]
    // Tests [@ANCHOR: radioctl_kachina_frame]
    #[test]
    fn other_families_are_refused() {
        assert!(KachinaEngine::new(RigDescription::ts2000()).is_err());
        assert!(KachinaTwin::new(&RigDescription::ts2000()).is_err());
    }

    // Tests [@ANCHOR: radioctl_kachina_engine_encode]
    // Tests [@ANCHOR: radioctl_kachina_engine_feed]
    // Tests [@ANCHOR: radioctl_kachina_twin_new]
    // Tests [@ANCHOR: radioctl_kachina_twin_frame]
    // Tests [@ANCHOR: radioctl_kachina_twin_feed]
    // Tests [@ANCHOR: radioctl_kachina_twin_snapshot]
    // Tests [@ANCHOR: radioctl_kachina_twin_front_panel]
    #[test]
    fn the_engine_sets_the_twin_and_hears_good_and_error() {
        let desc = shipped();
        let mut e = KachinaEngine::new(desc.clone()).expect("engine");
        let mut t = KachinaTwin::new(&desc).expect("twin");
        let (wire, seen) = run(&mut e, &mut t, Op::SetFrequency(Vfo::A, Hz(14_000_000)));
        assert_eq!(wire.len(), 14);
        assert_eq!(&wire[..2], &[STX, b'R']);
        assert_eq!(&wire[2..6], &[0x4b, 0xdd, 0xdd, 0xdd]);
        assert_eq!(seen, vec![Inbound::Ack]);
        assert_eq!(t.snapshot().expect("snapshot").frequency, Hz(14_000_000));
        for mode in [Mode::Am, Mode::Cw, Mode::Fm, Mode::Usb, Mode::Lsb] {
            assert_eq!(
                run(&mut e, &mut t, Op::SetMode(mode)).1,
                vec![Inbound::Ack],
                "{mode:?}"
            );
            assert_eq!(
                t.snapshot()
                    .expect("snapshot")
                    .mode_code
                    .and_then(|c| desc.code_mode(c)),
                Some(mode)
            );
        }
        assert_eq!(
            run(&mut e, &mut t, Op::SetMode(Mode::Usb)).0,
            [STX, b'M', 4, ETX]
        );
        assert_eq!(run(&mut e, &mut t, Op::SetPtt(true)).0, [STX, b'x', 1, ETX]);
        assert!(t.snapshot().expect("snapshot").ptt);
        run(&mut e, &mut t, Op::SetPtt(false));
        assert!(!t.snapshot().expect("snapshot").ptt);
        t.front_panel(Panel::Frequency(Hz(7_000_000)))
            .expect("tunes");
        assert_eq!(t.snapshot().expect("snapshot").frequency, Hz(7_000_000));
        assert!(t.front_panel(Panel::Mode(Mode::Am)).is_err());
        assert!(e.encode(&Op::SetFrequency(Vfo::A, Hz(31_000_000))).is_err());
        assert!(e.encode(&Op::GetFrequency(Vfo::A)).is_err());
        // Garbage and bad frames are refused with the error byte; a split frame waits.
        assert_eq!(t.feed(&[0x55], T0), vec![ERROR]);
        assert_eq!(t.feed(&[STX, b'Q', ETX], T0), vec![ERROR]);
        assert!(t.feed(&[STX, b'M'], T0).is_empty());
        assert_eq!(t.feed(&[9, ETX], T0), vec![ERROR]);
        e.encode(&Op::SetPtt(true)).expect("encodes");
        assert_eq!(e.feed(&[ERROR], T0), vec![Inbound::Nak]);
        assert!(matches!(
            e.feed(&[0x01], T0).as_slice(),
            [Inbound::Unrecognised(_)]
        ));
    }
}
