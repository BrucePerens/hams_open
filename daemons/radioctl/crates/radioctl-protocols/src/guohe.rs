// SPDX-License-Identifier: AGPL-3.0-or-later
//! Guohe PMR-171 transceiver: sans-IO client engine and virtual twin.
//!
//! **Sources.** Facts only, read from Hamlib 4.6.2 (released tarball, sha256
//! b2ac73f44dd1161e95fdee6c95276144757647bf92d7fdb369ee2fe41ed47ae8): `rigs/yaesu/pmr171.c`.
//! No Guohe document was read; nothing is verified against a radio. (The Q900 of the same maker
//! speaks the FT-817 five-byte CAT in Hamlib and is a `yaesu-legacy` description.)
//!
//! Every frame is `A5 A5 A5 A5`, a length byte, the command and its data, and a CRC-16/CCITT-FALSE
//! (initial value FFFF, polynomial 1021, high byte first) over the length byte, the command and
//! the data. The length byte counts the command, the data and the two CRC bytes.
//!
//! * `0B` asks for the status; the answer carries, after the command, the PTT flag, the two modes
//!   (VFO A, VFO B) and the two frequencies (four bytes big-endian hertz each). The length of the
//!   real answer is not known: the twin sends exactly these fields.
//! * `09` sets both frequencies (VFO A then VFO B); Hamlib writes the VFO it was not asked to set
//!   from its own copy, as the engine does (14 999 000 Hz until it has read the status).
//! * `0A` sets both modes (VFO A, VFO B): 0 USB, 1 LSB, 2 CW-R, 3 CW, 4 AM, 5 FM, 7 data (the
//!   description's mode codes are these digits as ASCII).
//! * `07` and one data byte keys: 0 is PTT ON and 1 is PTT OFF ("according to the manual", says
//!   Hamlib).
//!
//! Hamlib reads an answer frame after each setting and does not examine it; the engine takes any
//! valid frame as the acknowledgement and the twin echoes the command frame.

use std::time::Duration;

use radioctl_core::description::RigDescription;
use radioctl_core::engine::{Inbound, ProtocolEngine, ReplyKind, Request};
use radioctl_core::error::{ErrorKind, RigError};
use radioctl_core::op::{Field, Observation, Op};
use radioctl_core::transcript::Panel;
use radioctl_core::units::{Hz, Vfo};

use crate::serial_vendor::{vendor_of, RxBuffer, TwinSnapshot, VendorTwin};

const HEADER: [u8; 4] = [0xa5; 4];
/// What Hamlib starts both VFOs at.
const START_HZ: u64 = 14_999_000;

fn unsupported(what: &str) -> RigError {
    RigError::new(ErrorKind::Unsupported, what.to_string())
}

// [@ANCHOR: radioctl_guohe_crc16]
/// CRC-16/CCITT-FALSE.
pub fn crc16(data: &[u8]) -> u16 {
    let mut crc: u16 = 0xffff;
    for byte in data {
        crc ^= u16::from(*byte) << 8;
        for _ in 0..8 {
            crc = if crc & 0x8000 != 0 {
                (crc << 1) ^ 0x1021
            } else {
                crc << 1
            };
        }
    }
    crc
}

// [@ANCHOR: radioctl_guohe_build_frame]
/// A frame for `command` with `data`.
pub fn build_frame(command: u8, data: &[u8]) -> Vec<u8> {
    let mut body = vec![u8::try_from(data.len() + 3).unwrap_or(u8::MAX), command];
    body.extend_from_slice(data);
    let crc = crc16(&body);
    let mut out = HEADER.to_vec();
    out.extend(body);
    out.extend_from_slice(&crc.to_be_bytes());
    out
}

// [@ANCHOR: radioctl_guohe_parse_frame]
/// The command and data of a whole frame, or `None` if the header, length or CRC is wrong.
pub fn parse_frame(frame: &[u8]) -> Option<(u8, &[u8])> {
    let rest = frame.strip_prefix(&HEADER)?;
    let (&len, rest) = rest.split_first()?;
    let len = usize::from(len);
    if len < 3 || rest.len() != len {
        return None;
    }
    let (payload, crc) = rest.split_at(len - 2);
    let mut body = vec![u8::try_from(len).ok()?];
    body.extend_from_slice(payload);
    (crc16(&body).to_be_bytes() == [crc[0], crc[1]]).then(|| (payload[0], &payload[1..]))
}

// [@ANCHOR: radioctl_guohe_take_frame]
/// Takes one frame candidate off the front of `rx`: `None` when more bytes are needed. Bytes in
/// front of a header are dropped.
fn take_frame(rx: &mut RxBuffer) -> Option<Vec<u8>> {
    loop {
        let held = rx.peek();
        let start = held
            .windows(HEADER.len())
            .position(|w| w == HEADER)
            .unwrap_or_else(|| held.len().saturating_sub(HEADER.len() - 1));
        if start > 0 {
            rx.take(start);
            continue;
        }
        if held.len() < HEADER.len() + 1 {
            return None;
        }
        let total = HEADER.len() + 1 + usize::from(held[HEADER.len()]);
        return rx.take(total);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pending {
    Nothing,
    Status,
    Ack,
}

/// The Guohe client engine.
#[derive(Debug)]
pub struct GuoheEngine {
    desc: RigDescription,
    pending: Pending,
    rx: RxBuffer,
    freq: [u64; 2],
    mode: [u8; 2],
}

impl GuoheEngine {
    // [@ANCHOR: radioctl_guohe_engine_new]
    /// Builds an engine from a Guohe vendor description.
    pub fn new(desc: RigDescription) -> Result<GuoheEngine, RigError> {
        let (_, name) = vendor_of(&desc)?;
        if name != "pmr171" {
            return Err(unsupported("unknown Guohe dialect in the description"));
        }
        let mode = desc.mode_code(desc.default_mode).unwrap_or(b'0');
        Ok(GuoheEngine {
            freq: [desc.default_frequency.0, START_HZ],
            mode: [mode, mode],
            desc,
            pending: Pending::Nothing,
            rx: RxBuffer::default(),
        })
    }
}

impl ProtocolEngine for GuoheEngine {
    fn description(&self) -> &RigDescription {
        &self.desc
    }

    // [@ANCHOR: radioctl_guohe_engine_encode]
    fn encode(&mut self, op: &Op) -> Result<Request, RigError> {
        if !self.desc.capabilities.has(op.required_capability()) {
            return Err(unsupported("capability not in the rig description"));
        }
        let (bytes, reply, pending) = match op {
            Op::GetFrequency(vfo) => (
                build_frame(0x0b, &[]),
                ReplyKind::Observation(Field::Frequency(*vfo)),
                Pending::Status,
            ),
            Op::GetMode => (
                build_frame(0x0b, &[]),
                ReplyKind::Observation(Field::Mode),
                Pending::Status,
            ),
            Op::GetPtt => (
                build_frame(0x0b, &[]),
                ReplyKind::Observation(Field::Ptt),
                Pending::Status,
            ),
            Op::SetFrequency(vfo, hz) => {
                let i = usize::from(*vfo == Vfo::B);
                let new = u32::try_from(hz.0).map_err(|_| unsupported("frequency too large"))?;
                let mut freq = self.freq;
                freq[i] = u64::from(new);
                let mut data = Vec::new();
                for f in freq {
                    data.extend_from_slice(&u32::try_from(f).unwrap_or(0).to_be_bytes());
                }
                self.freq = freq;
                (build_frame(0x09, &data), ReplyKind::Ack, Pending::Ack)
            }
            Op::SetMode(mode) => {
                let code = self
                    .desc
                    .mode_code(*mode)
                    .ok_or_else(|| unsupported("mode not in the description"))?;
                self.mode[0] = code;
                let data = [
                    self.mode[0].wrapping_sub(b'0'),
                    self.mode[1].wrapping_sub(b'0'),
                ];
                (build_frame(0x0a, &data), ReplyKind::Ack, Pending::Ack)
            }
            Op::SetPtt(on) => (
                build_frame(0x07, &[u8::from(!*on)]),
                ReplyKind::Ack,
                Pending::Ack,
            ),
            _ => return Err(unsupported("not implemented for the Guohe engine")),
        };
        self.pending = pending;
        self.rx.take_all();
        Ok(Request { bytes, reply })
    }

    // [@ANCHOR: radioctl_guohe_engine_feed]
    fn feed(&mut self, bytes: &[u8], now: Duration) -> Vec<Inbound> {
        let mut out = Vec::new();
        if let Some(stale) = self.rx.push(bytes, now) {
            out.push(Inbound::Unrecognised(stale));
        }
        while let Some(frame) = take_frame(&mut self.rx) {
            let pending = std::mem::replace(&mut self.pending, Pending::Nothing);
            let Some((command, data)) = parse_frame(&frame) else {
                out.push(Inbound::Unrecognised(frame));
                continue;
            };
            match (pending, command, data) {
                (Pending::Status, 0x0b, [ptt, ma, mb, a0, a1, a2, a3, b0, b1, b2, b3, ..]) => {
                    let a = u32::from_be_bytes([*a0, *a1, *a2, *a3]);
                    let b = u32::from_be_bytes([*b0, *b1, *b2, *b3]);
                    self.freq = [u64::from(a), u64::from(b)];
                    self.mode = [ma + b'0', mb + b'0'];
                    out.push(Inbound::Observation(Observation::Frequency(
                        Vfo::A,
                        Hz(u64::from(a)),
                    )));
                    out.push(Inbound::Observation(Observation::Frequency(
                        Vfo::B,
                        Hz(u64::from(b)),
                    )));
                    if let Some(m) = self.desc.code_mode(self.mode[0]) {
                        out.push(Inbound::Observation(Observation::Mode(m)));
                    }
                    out.push(Inbound::Observation(Observation::Ptt(*ptt == 1)));
                }
                (Pending::Ack, _, _) => out.push(Inbound::Ack),
                _ => out.push(Inbound::Unrecognised(frame)),
            }
        }
        out
    }
}

/// A virtual PMR-171.
#[derive(Debug)]
pub struct GuoheTwin {
    rx: RxBuffer,
    freq: [u64; 2],
    mode: [u8; 2],
    ptt: bool,
}

impl GuoheTwin {
    // [@ANCHOR: radioctl_guohe_twin_new]
    /// A twin in the description's default state.
    pub fn new(desc: &RigDescription) -> Result<GuoheTwin, RigError> {
        let (_, name) = vendor_of(desc)?;
        if name != "pmr171" {
            return Err(unsupported("unknown Guohe dialect in the description"));
        }
        let mode = desc.mode_code(desc.default_mode).unwrap_or(b'0');
        Ok(GuoheTwin {
            rx: RxBuffer::default(),
            freq: [desc.default_frequency.0, START_HZ],
            mode: [mode, mode],
            ptt: false,
        })
    }
}

impl VendorTwin for GuoheTwin {
    // [@ANCHOR: radioctl_guohe_twin_snapshot]
    fn snapshot(&self) -> Option<TwinSnapshot> {
        Some(TwinSnapshot {
            frequency: Hz(self.freq[0]),
            mode_code: Some(self.mode[0]),
            ptt: self.ptt,
        })
    }

    // [@ANCHOR: radioctl_guohe_twin_feed]
    fn feed(&mut self, bytes: &[u8], now: Duration) -> Vec<u8> {
        self.rx.push(bytes, now);
        let mut out = Vec::new();
        while let Some(frame) = take_frame(&mut self.rx) {
            let Some((command, data)) = parse_frame(&frame) else {
                continue;
            };
            match (command, data) {
                (0x0b, []) => {
                    let mut d = vec![u8::from(self.ptt), self.mode[0] - b'0', self.mode[1] - b'0'];
                    for f in self.freq {
                        d.extend_from_slice(&u32::try_from(f).unwrap_or(0).to_be_bytes());
                    }
                    out.extend(build_frame(0x0b, &d));
                }
                (0x09, [a0, a1, a2, a3, b0, b1, b2, b3]) => {
                    self.freq = [
                        u64::from(u32::from_be_bytes([*a0, *a1, *a2, *a3])),
                        u64::from(u32::from_be_bytes([*b0, *b1, *b2, *b3])),
                    ];
                    out.extend(frame);
                }
                (0x0a, [a, b]) if *a <= 9 && *b <= 9 => {
                    self.mode = [a + b'0', b + b'0'];
                    out.extend(frame);
                }
                (0x07, [p @ (0 | 1)]) => {
                    // 0 is PTT ON.
                    self.ptt = *p == 0;
                    out.extend(frame);
                }
                _ => {}
            }
        }
        out
    }

    // [@ANCHOR: radioctl_guohe_twin_front_panel]
    fn front_panel(&mut self, panel: Panel) -> Result<Vec<u8>, String> {
        match panel {
            Panel::Frequency(hz) => self.freq[0] = hz.0,
            Panel::Ptt(on) => self.ptt = on,
            other => return Err(format!("a Guohe twin has no {other:?} knob")),
        }
        Ok(Vec::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use radioctl_core::units::Mode;

    const T0: Duration = Duration::from_secs(1);

    fn run(e: &mut GuoheEngine, t: &mut GuoheTwin, op: Op) -> (Vec<u8>, Vec<Inbound>) {
        let r = e.encode(&op).expect("encodes");
        let a = t.feed(&r.bytes, T0);
        (r.bytes, e.feed(&a, T0))
    }

    fn shipped() -> RigDescription {
        let path = format!(
            "{}/../../rigs/guohe-pmr-171.toml",
            env!("CARGO_MANIFEST_DIR")
        );
        let text = std::fs::read_to_string(path).expect("rig file");
        radioctl_core::rigfile::parse(&text)
            .expect("loads")
            .description
    }

    // Tests [@ANCHOR: radioctl_guohe_crc16]
    // Tests [@ANCHOR: radioctl_guohe_build_frame]
    // Tests [@ANCHOR: radioctl_guohe_parse_frame]
    #[test]
    fn frames_carry_a_ccitt_false_crc() {
        // The standard check value of CRC-16/CCITT-FALSE.
        assert_eq!(crc16(b"123456789"), 0x29b1);
        let f = build_frame(0x0b, &[]);
        assert_eq!(&f[..6], &[0xa5, 0xa5, 0xa5, 0xa5, 3, 0x0b]);
        assert_eq!(f.len(), 8);
        assert_eq!(parse_frame(&f), Some((0x0b, &[][..])));
        let g = build_frame(0x09, &[1, 2, 3, 4, 5, 6, 7, 8]);
        assert_eq!(g[4], 11);
        assert_eq!(parse_frame(&g), Some((0x09, &[1, 2, 3, 4, 5, 6, 7, 8][..])));
        let mut bad = g.clone();
        bad[7] ^= 1;
        assert_eq!(parse_frame(&bad), None);
        assert_eq!(parse_frame(&g[..g.len() - 1]), None);
        assert_eq!(parse_frame(b"nothing here"), None);
        assert_eq!(parse_frame(&[0xa5, 0xa5, 0xa5, 0xa5, 2, 0, 0]), None);
    }

    // Tests [@ANCHOR: radioctl_guohe_engine_new]
    // Tests [@ANCHOR: radioctl_guohe_take_frame]
    #[test]
    fn other_families_are_refused() {
        assert!(GuoheEngine::new(RigDescription::ts2000()).is_err());
        assert!(GuoheTwin::new(&RigDescription::ts2000()).is_err());
    }

    // Tests [@ANCHOR: radioctl_guohe_engine_encode]
    // Tests [@ANCHOR: radioctl_guohe_engine_feed]
    // Tests [@ANCHOR: radioctl_guohe_twin_new]
    // Tests [@ANCHOR: radioctl_guohe_twin_feed]
    // Tests [@ANCHOR: radioctl_guohe_twin_snapshot]
    // Tests [@ANCHOR: radioctl_guohe_twin_front_panel]
    #[test]
    fn the_engine_round_trips_against_the_twin() {
        let desc = shipped();
        let mut e = GuoheEngine::new(desc.clone()).expect("engine");
        let mut t = GuoheTwin::new(&desc).expect("twin");
        let (wire, seen) = run(&mut e, &mut t, Op::SetFrequency(Vfo::A, Hz(14_074_000)));
        assert_eq!(parse_frame(&wire).map(|(c, _)| c), Some(0x09));
        assert_eq!(seen, vec![Inbound::Ack]);
        assert_eq!(t.snapshot().expect("snapshot").frequency, Hz(14_074_000));
        let (_, seen) = run(&mut e, &mut t, Op::GetFrequency(Vfo::A));
        assert!(seen.contains(&Inbound::Observation(Observation::Frequency(
            Vfo::A,
            Hz(14_074_000)
        ))));
        assert!(seen.contains(&Inbound::Observation(Observation::Frequency(
            Vfo::B,
            Hz(14_999_000)
        ))));
        run(&mut e, &mut t, Op::SetFrequency(Vfo::B, Hz(14_025_000)));
        let (_, seen) = run(&mut e, &mut t, Op::GetFrequency(Vfo::B));
        assert!(seen.contains(&Inbound::Observation(Observation::Frequency(
            Vfo::B,
            Hz(14_025_000)
        ))));
        for mode in [
            Mode::Usb,
            Mode::Lsb,
            Mode::CwR,
            Mode::Cw,
            Mode::Am,
            Mode::Fm,
            Mode::Data,
        ] {
            assert_eq!(
                run(&mut e, &mut t, Op::SetMode(mode)).1,
                vec![Inbound::Ack],
                "{mode:?}"
            );
            assert!(run(&mut e, &mut t, Op::GetMode)
                .1
                .contains(&Inbound::Observation(Observation::Mode(mode))));
        }
        // 0 keys, 1 unkeys.
        let (wire, seen) = run(&mut e, &mut t, Op::SetPtt(true));
        assert_eq!(parse_frame(&wire), Some((0x07, &[0][..])));
        assert_eq!(seen, vec![Inbound::Ack]);
        assert!(t.snapshot().expect("snapshot").ptt);
        assert!(run(&mut e, &mut t, Op::GetPtt)
            .1
            .contains(&Inbound::Observation(Observation::Ptt(true))));
        run(&mut e, &mut t, Op::SetPtt(false));
        assert!(!t.snapshot().expect("snapshot").ptt);
        t.front_panel(Panel::Frequency(Hz(7_000_000)))
            .expect("tunes");
        assert_eq!(t.snapshot().expect("snapshot").frequency, Hz(7_000_000));
        assert!(t.front_panel(Panel::Mode(Mode::Am)).is_err());
        assert!(e.encode(&Op::SetMode(Mode::Rtty)).is_err());
        assert!(e
            .encode(&Op::SetFrequency(Vfo::A, Hz(5_000_000_000)))
            .is_err());
        // Garbage before a frame is dropped; a frame with a bad CRC is not an answer; a split
        // frame waits.
        let frame = build_frame(0x0b, &[]);
        let mut noisy = b"xx".to_vec();
        noisy.extend_from_slice(&frame);
        assert!(!t.feed(&noisy, T0).is_empty());
        assert!(t.feed(&frame[..5], T0).is_empty());
        assert!(!t.feed(&frame[5..], T0).is_empty());
        let mut bad = frame.clone();
        bad[6] ^= 0xff;
        assert!(t.feed(&bad, T0).is_empty());
        e.encode(&Op::GetPtt).expect("encodes");
        assert!(matches!(
            e.feed(&bad, T0).as_slice(),
            [Inbound::Unrecognised(_)]
        ));
        e.encode(&Op::GetPtt).expect("encodes");
        assert!(matches!(
            e.feed(&build_frame(0x0c, &[1]), T0).as_slice(),
            [Inbound::Unrecognised(_)]
        ));
    }
}
