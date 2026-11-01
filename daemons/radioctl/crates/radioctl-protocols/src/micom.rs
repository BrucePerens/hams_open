// SPDX-License-Identifier: AGPL-3.0-or-later
//! Motorola MICOM 2 and 3 HF transceivers: sans-IO client engine and virtual twin.
//!
//! **Sources.** Facts only, read from Hamlib 4.6.2 (released tarball, sha256
//! b2ac73f44dd1161e95fdee6c95276144757647bf92d7fdb369ee2fe41ed47ae8): `rigs/motorola/micom.c`
//! (status Alpha in Hamlib, whose own comments call parts of it a guess). No Motorola document
//! was found; nothing is verified against a radio.
//!
//! The line is 9600 baud, 8 data bits, odd parity, 2 stop bits. A frame is `24`, a length `L`,
//! `L + 1` further bytes, a checksum (the sum of every byte before it, modulo 256) and `03`; the
//! frame is `L + 5` bytes long and some Hamlib captures carry a trailing `15` after the `03`.
//!
//! * Set frequency: five frames. The first, `24 06 18 05 01` and the frequency as four big-endian
//!   bytes, then the checksum, `03` and `15`; then four fixed frames (`24 05 18 36 FE 7B EF 01
//!   E0 03 15`, `... FE ... E1` (whose checksum fits an FF in byte 4, as Hamlib's comment lists it; the
//!   code has FE: an open question, the engine sends the code's bytes), `24 05 18 36 DF 7B EF 01 C1 03 15` and `24 05 18 36 FF 7B EF 01
//!   E1 03`). Hamlib reads one answer frame after each write and two more after the last, and
//!   does not look at their contents.
//! * Get frequency: `24 01 18 06 43 03`. The radio answers an acknowledgement (`24 01 80 FE 98 03`
//!   in Hamlib's comment, whose checksum does not follow the sum rule above) and then a frame
//!   with the frequency in bytes 4 to 7, big-endian; Hamlib then writes `24 01 18 F3 FF 03`.
//!   The engine cannot write that last frame (it answers nothing by itself), which is an open
//!   question; the layout of the answer frame outside bytes 4 to 7 is an assumption of the twin.
//! * PTT: `24 02 81 13 01 BB 03` keys and `24 02 81 14 01 BC 03` unkeys; no answer is read.
//!
//! Hamlib has no mode, VFO or level for the radio.

use std::time::Duration;

use radioctl_core::description::RigDescription;
use radioctl_core::engine::{Inbound, ProtocolEngine, ReplyKind, Request};
use radioctl_core::error::{ErrorKind, RigError};
use radioctl_core::op::{Field, Observation, Op};
use radioctl_core::transcript::Panel;
use radioctl_core::units::{Hz, Vfo};

use crate::serial_vendor::{vendor_of, RxBuffer, TwinSnapshot, VendorTwin};

const START: u8 = 0x24;
const END: u8 = 0x03;
const TRAILER: u8 = 0x15;

/// The four fixed frames that follow the frequency frame (Hamlib's captured sequence).
const FIXED_FRAMES: [&[u8]; 4] = [
    &[
        0x24, 0x05, 0x18, 0x36, 0xfe, 0x7b, 0xef, 0x01, 0xe0, 0x03, 0x15,
    ],
    &[
        0x24, 0x05, 0x18, 0x36, 0xfe, 0x7b, 0xef, 0x01, 0xe1, 0x03, 0x15,
    ],
    &[
        0x24, 0x05, 0x18, 0x36, 0xdf, 0x7b, 0xef, 0x01, 0xc1, 0x03, 0x15,
    ],
    &[0x24, 0x05, 0x18, 0x36, 0xff, 0x7b, 0xef, 0x01, 0xe1, 0x03],
];
/// The frame that asks for the frequency.
const GET_FREQUENCY: [u8; 6] = [0x24, 0x01, 0x18, 0x06, 0x43, 0x03];
/// Keys the transmitter.
const KEY_DOWN: [u8; 7] = [0x24, 0x02, 0x81, 0x13, 0x01, 0xbb, 0x03];
/// Unkeys the transmitter.
const KEY_UP: [u8; 7] = [0x24, 0x02, 0x81, 0x14, 0x01, 0xbc, 0x03];
/// An acknowledgement frame, as Hamlib's comment spells it.
const ACK: [u8; 6] = [0x24, 0x01, 0x80, 0xfe, 0x98, 0x03];

fn unsupported(what: &str) -> RigError {
    RigError::new(ErrorKind::Unsupported, what.to_string())
}

// [@ANCHOR: radioctl_micom_checksum]
/// The checksum of a frame's leading bytes: their sum modulo 256.
pub fn checksum(bytes: &[u8]) -> u8 {
    bytes.iter().fold(0u8, |a, b| a.wrapping_add(*b))
}

// [@ANCHOR: radioctl_micom_set_frequency_frames]
/// The five frames that set the frequency (Hamlib's sequence), written one after the other.
pub fn set_frequency_frames(hz: Hz) -> Vec<u8> {
    let f = u32::try_from(hz.0).unwrap_or(u32::MAX).to_be_bytes();
    let mut first = vec![START, 0x06, 0x18, 0x05, 0x01, f[0], f[1], f[2], f[3]];
    first.push(checksum(&first));
    first.extend([END, TRAILER]);
    let mut out = first;
    for frame in FIXED_FRAMES {
        out.extend_from_slice(frame);
    }
    out
}

// [@ANCHOR: radioctl_micom_take_frame]
/// The frames of a byte stream: each starts at `24`, is `L + 5` bytes long, and may be followed
/// by a stray `15`.
fn take_frame(rx: &mut RxBuffer) -> Option<Vec<u8>> {
    loop {
        let first = *rx.peek().first()?;
        if first == START {
            break;
        }
        rx.take(1);
    }
    let length = usize::from(*rx.peek().get(1)?);
    let frame = rx.take(length + 5)?;
    if rx.peek().first() == Some(&TRAILER) {
        rx.take(1);
    }
    Some(frame)
}

// [@ANCHOR: radioctl_micom_decode_frequency]
/// The frequency of an answer frame (bytes 4 to 7, big-endian), if it is the long kind.
pub fn decode_frequency(frame: &[u8]) -> Option<Hz> {
    if frame.len() != 10 || frame.first() != Some(&START) || frame.last() != Some(&END) {
        return None;
    }
    let bytes: [u8; 4] = frame.get(4..8)?.try_into().ok()?;
    Some(Hz(u64::from(u32::from_be_bytes(bytes))))
}

/// The MICOM client engine.
#[derive(Debug)]
pub struct MicomEngine {
    desc: RigDescription,
    rx: RxBuffer,
}

impl MicomEngine {
    // [@ANCHOR: radioctl_micom_engine_new]
    /// Builds an engine from a MICOM vendor description.
    pub fn new(desc: RigDescription) -> Result<MicomEngine, RigError> {
        let (_, name) = vendor_of(&desc)?;
        if name != "micom" {
            return Err(unsupported("unknown MICOM dialect in the description"));
        }
        Ok(MicomEngine {
            desc,
            rx: RxBuffer::default(),
        })
    }
}

impl ProtocolEngine for MicomEngine {
    fn description(&self) -> &RigDescription {
        &self.desc
    }

    // [@ANCHOR: radioctl_micom_engine_encode]
    fn encode(&mut self, op: &Op) -> Result<Request, RigError> {
        if !self.desc.capabilities.has(op.required_capability()) {
            return Err(unsupported("capability not in the rig description"));
        }
        let (bytes, reply) = match op {
            Op::GetFrequency(Vfo::A) => (
                GET_FREQUENCY.to_vec(),
                ReplyKind::Observation(Field::Frequency(Vfo::A)),
            ),
            Op::SetFrequency(Vfo::A, hz) => {
                if u32::try_from(hz.0).is_err() {
                    return Err(unsupported("frequency above 32 bits"));
                }
                (set_frequency_frames(*hz), ReplyKind::None)
            }
            Op::SetPtt(true) => (KEY_DOWN.to_vec(), ReplyKind::None),
            Op::SetPtt(false) => (KEY_UP.to_vec(), ReplyKind::None),
            _ => return Err(unsupported("not implemented for the MICOM engine")),
        };
        self.rx.take_all();
        Ok(Request { bytes, reply })
    }

    // [@ANCHOR: radioctl_micom_engine_feed]
    fn feed(&mut self, bytes: &[u8], now: Duration) -> Vec<Inbound> {
        let mut out = Vec::new();
        if let Some(stale) = self.rx.push(bytes, now) {
            out.push(Inbound::Unrecognised(stale));
        }
        while let Some(frame) = take_frame(&mut self.rx) {
            match decode_frequency(&frame) {
                Some(hz) => out.push(Inbound::Observation(Observation::Frequency(Vfo::A, hz))),
                // The short acknowledgement frames carry nothing the engine uses.
                None if frame.len() == 6 => {}
                None => out.push(Inbound::Unrecognised(frame)),
            }
        }
        out
    }
}

/// A virtual MICOM.
#[derive(Debug)]
pub struct MicomTwin {
    rx: RxBuffer,
    hz: u32,
    keyed: bool,
}

impl MicomTwin {
    // [@ANCHOR: radioctl_micom_twin_new]
    /// A twin in the description's default state.
    pub fn new(desc: &RigDescription) -> Result<MicomTwin, RigError> {
        let (_, name) = vendor_of(desc)?;
        if name != "micom" {
            return Err(unsupported("unknown MICOM dialect in the description"));
        }
        Ok(MicomTwin {
            rx: RxBuffer::default(),
            hz: u32::try_from(desc.default_frequency.0).unwrap_or(0),
            keyed: false,
        })
    }

    // [@ANCHOR: radioctl_micom_twin_frequency_frame]
    /// The answer frame of a frequency read: bytes 4 to 7 hold the frequency. Bytes 2 and 3 are
    /// an assumption (Hamlib reads only the frequency out of it).
    fn frequency_frame(&self) -> Vec<u8> {
        let f = self.hz.to_be_bytes();
        let mut frame = vec![START, 0x05, 0x80, 0x06, f[0], f[1], f[2], f[3]];
        frame.push(checksum(&frame));
        frame.push(END);
        frame
    }
}

impl VendorTwin for MicomTwin {
    // [@ANCHOR: radioctl_micom_twin_snapshot]
    fn snapshot(&self) -> Option<TwinSnapshot> {
        Some(TwinSnapshot {
            frequency: Hz(u64::from(self.hz)),
            mode_code: None,
            ptt: self.keyed,
        })
    }

    // [@ANCHOR: radioctl_micom_twin_feed]
    fn feed(&mut self, bytes: &[u8], now: Duration) -> Vec<u8> {
        self.rx.push(bytes, now);
        let mut out = Vec::new();
        while let Some(frame) = take_frame(&mut self.rx) {
            match (frame.get(2), frame.get(3), frame.get(4)) {
                (Some(0x18), Some(0x05), Some(0x01)) if frame.len() >= 10 => {
                    if let Some(f) = frame.get(5..9).and_then(|b| <[u8; 4]>::try_from(b).ok()) {
                        self.hz = u32::from_be_bytes(f);
                    }
                    out.extend(ACK);
                }
                (Some(0x18), Some(0x36), _) => out.extend(ACK),
                (Some(0x18), Some(0x06), _) => {
                    out.extend(ACK);
                    out.extend(self.frequency_frame());
                }
                (Some(0x81), Some(0x13), _) => self.keyed = true,
                (Some(0x81), Some(0x14), _) => self.keyed = false,
                _ => {}
            }
        }
        out
    }

    // [@ANCHOR: radioctl_micom_twin_front_panel]
    fn front_panel(&mut self, panel: Panel) -> Result<Vec<u8>, String> {
        match panel {
            Panel::Frequency(hz) => self.hz = u32::try_from(hz.0).unwrap_or(self.hz),
            Panel::Ptt(on) => self.keyed = on,
            other => return Err(format!("a MICOM twin has no {other:?} knob")),
        }
        Ok(Vec::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const T0: Duration = Duration::from_secs(1);

    fn shipped() -> RigDescription {
        let path = format!("{}/../../rigs/micom-2-3.toml", env!("CARGO_MANIFEST_DIR"));
        let text = std::fs::read_to_string(path).expect("rig file");
        radioctl_core::rigfile::parse(&text)
            .expect("loads")
            .description
    }

    // Tests [@ANCHOR: radioctl_micom_checksum]
    // Tests [@ANCHOR: radioctl_micom_set_frequency_frames]
    // Tests [@ANCHOR: radioctl_micom_decode_frequency]
    #[test]
    fn frames_follow_the_captured_sequence() {
        // The checksum rule reproduces the captured fixed frames and the key frames.
        for (n, frame) in FIXED_FRAMES.iter().enumerate() {
            let sum_at = 8;
            if n == 1 {
                // Open question, pinned: this constant of Hamlib's code has FE at byte 4 and a
                // checksum (E1) that fits FF; Hamlib's own comment lists the same frame with FF.
                // The engine sends Hamlib's bytes.
                assert_eq!(checksum(&frame[..sum_at]), frame[sum_at] - 1);
                continue;
            }
            assert_eq!(checksum(&frame[..sum_at]), frame[sum_at]);
        }
        assert_eq!(checksum(&KEY_DOWN[..5]), KEY_DOWN[5]);
        assert_eq!(checksum(&KEY_UP[..5]), KEY_UP[5]);
        assert_eq!(checksum(&GET_FREQUENCY[..4]), GET_FREQUENCY[4]);
        let frames = set_frequency_frames(Hz(7_074_000));
        assert_eq!(frames.len(), 12 + 11 * 3 + 10);
        assert_eq!(
            frames.get(..9),
            Some(&[0x24, 0x06, 0x18, 0x05, 0x01, 0x00, 0x6b, 0xf0, 0xd0][..])
        );
        assert_eq!(frames.get(9), Some(&checksum(&frames[..9])));
        assert_eq!(frames.get(10..12), Some(&[0x03, 0x15][..]));
        assert_eq!(
            decode_frequency(&[0x24, 0x05, 0x80, 0x06, 0, 0x6b, 0xf0, 0xd0, 0, 3]),
            Some(Hz(7_074_000))
        );
        assert_eq!(decode_frequency(&ACK), None);
    }

    // Tests [@ANCHOR: radioctl_micom_engine_new]
    #[test]
    fn other_families_are_refused() {
        assert!(MicomEngine::new(RigDescription::ts2000()).is_err());
        assert!(MicomTwin::new(&RigDescription::ts2000()).is_err());
    }

    // Tests [@ANCHOR: radioctl_micom_engine_encode]
    // Tests [@ANCHOR: radioctl_micom_engine_feed]
    // Tests [@ANCHOR: radioctl_micom_take_frame]
    // Tests [@ANCHOR: radioctl_micom_twin_new]
    // Tests [@ANCHOR: radioctl_micom_twin_feed]
    // Tests [@ANCHOR: radioctl_micom_twin_snapshot]
    // Tests [@ANCHOR: radioctl_micom_twin_front_panel]
    // Tests [@ANCHOR: radioctl_micom_twin_frequency_frame]
    #[test]
    fn the_engine_and_the_twin_agree_on_frequency_and_ptt() {
        let desc = shipped();
        let mut e = MicomEngine::new(desc.clone()).expect("engine");
        let mut t = MicomTwin::new(&desc).expect("twin");
        let r = e
            .encode(&Op::SetFrequency(Vfo::A, Hz(7_074_000)))
            .expect("encodes");
        assert_eq!(r.reply, ReplyKind::None);
        let acks = t.feed(&r.bytes, T0);
        assert_eq!(acks.len(), 5 * ACK.len(), "one acknowledgement per frame");
        assert!(e.feed(&acks, T0).is_empty(), "acknowledgements say nothing");
        assert_eq!(t.snapshot().expect("snapshot").frequency, Hz(7_074_000));
        let r = e.encode(&Op::GetFrequency(Vfo::A)).expect("encodes");
        assert_eq!(r.bytes, GET_FREQUENCY);
        let answer = t.feed(&r.bytes, T0);
        assert_eq!(
            e.feed(&answer, T0),
            vec![Inbound::Observation(Observation::Frequency(
                Vfo::A,
                Hz(7_074_000)
            ))]
        );
        // PTT, with the stray trailer and a split delivery.
        let r = e.encode(&Op::SetPtt(true)).expect("encodes");
        assert_eq!(r.bytes, KEY_DOWN);
        assert!(t.feed(&r.bytes[..3], T0).is_empty());
        assert!(t.feed(&r.bytes[3..], T0).is_empty());
        assert!(t.snapshot().expect("snapshot").ptt);
        assert_eq!(e.encode(&Op::SetPtt(false)).expect("up").bytes, KEY_UP);
        assert!(t.feed(&KEY_UP, T0).is_empty());
        assert!(!t.snapshot().expect("snapshot").ptt);
        assert!(e
            .feed(&[0x15, 0x24, 0x01, 0x80, 0xfe, 0x98, 0x03, 0x15], T0)
            .is_empty());
        assert!(matches!(
            e.feed(&[0x24, 0x02, 0x80, 0x01, 0x02, 0x00, 0x03], T0)
                .as_slice(),
            [Inbound::Unrecognised(_)]
        ));
        assert!(e
            .encode(&Op::SetFrequency(Vfo::A, Hz(5_000_000_000)))
            .is_err());
        assert!(e.encode(&Op::GetMode).is_err());
        t.front_panel(Panel::Frequency(Hz(3_573_000)))
            .expect("tunes");
        t.front_panel(Panel::Ptt(false)).expect("ptt");
        assert!(t.front_panel(Panel::Signal(3)).is_err());
    }
}
