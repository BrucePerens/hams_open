// SPDX-License-Identifier: AGPL-3.0-or-later
//! CommRadio CTX-10 transceiver: sans-IO client engine and virtual twin.
//!
//! **Sources.** Facts only, read from Hamlib 4.6.2 (released tarball, sha256
//! b2ac73f44dd1161e95fdee6c95276144757647bf92d7fdb369ee2fe41ed47ae8): `rigs/commradio/frame.c`,
//! `commradio.c` and `ctx10.c`. No CommRadio document was read; nothing is verified against a
//! radio. The 3 Mbaud line speed is the one Hamlib lists.
//!
//! A frame is `FE`, the destination address (`21` to the radio, `11` to the host), the message
//! type, the message bytes, a CRC-16 and `FD`. The CRC is the Kermit CRC-16 (reflected
//! polynomial `1021`, initial value 0, low byte first in the register) over the address, the
//! type and the message bytes, sent high byte first. Message bytes and the CRC bytes (not the
//! type) are escaped: `FC`, `FD` and `FE` become `FC` followed by the byte XOR 0x14. Type `32`
//! asks for the frequency; type `30` plus four bytes (big-endian hertz) sets it. The answer
//! (`33`, `31` or `34`) carries the four-byte frequency. The radio also sends a `34` frame by
//! itself when the knob turns, so the engine takes a frequency frame at any time. The radio has
//! no mode command in Hamlib's view (reads return none) and no PTT.

use std::time::Duration;

use radioctl_core::description::RigDescription;
use radioctl_core::engine::{Inbound, ProtocolEngine, ReplyKind, Request};
use radioctl_core::error::{ErrorKind, RigError};
use radioctl_core::op::{Field, Observation, Op};
use radioctl_core::transcript::Panel;
use radioctl_core::units::{Hz, Vfo};

use crate::serial_vendor::{vendor_of, RxBuffer, VendorTwin};

const SOF: u8 = 0xFE;
const EOF: u8 = 0xFD;
const ESC: u8 = 0xFC;
const TO_RADIO: u8 = 0x21;
const TO_HOST: u8 = 0x11;
const ESC_XOR: u8 = 0x14;

/// The tuning range Hamlib lists, in hertz.
const RANGE: std::ops::RangeInclusive<u64> = 150_000..=30_000_000;

fn unsupported(what: &str) -> RigError {
    RigError::new(ErrorKind::Unsupported, what.to_string())
}

// [@ANCHOR: radioctl_commradio_crc16]
/// CRC-16/KERMIT of `data` (`"123456789"` gives `0x2189`), continuing from `crc`.
pub fn crc16(data: &[u8], mut crc: u16) -> u16 {
    for byte in data {
        crc ^= u16::from(*byte);
        for _ in 0..8 {
            crc = if crc & 1 == 1 {
                (crc >> 1) ^ 0x8408
            } else {
                crc >> 1
            };
        }
    }
    crc
}

fn push_escaped(out: &mut Vec<u8>, byte: u8) {
    if matches!(byte, SOF | EOF | ESC) {
        out.push(ESC);
        out.push(byte ^ ESC_XOR);
    } else {
        out.push(byte);
    }
}

// [@ANCHOR: radioctl_commradio_frame]
/// A frame to `address` carrying message `kind` and `payload`.
pub fn frame(address: u8, kind: u8, payload: &[u8]) -> Vec<u8> {
    let mut crc = crc16(&[address, kind], 0);
    crc = crc16(payload, crc);
    let mut out = vec![SOF, address, kind];
    for byte in payload {
        push_escaped(&mut out, *byte);
    }
    push_escaped(&mut out, (crc >> 8) as u8);
    push_escaped(&mut out, (crc & 0xFF) as u8);
    out.push(EOF);
    out
}

// [@ANCHOR: radioctl_commradio_unframe]
/// The message type and payload of a whole frame (`FE` to `FD`) addressed to `address`, or
/// `None` when it is short, misaddressed, badly escaped or fails its CRC.
pub fn unframe(bytes: &[u8], address: u8) -> Option<(u8, Vec<u8>)> {
    let body = bytes.strip_prefix(&[SOF])?.strip_suffix(&[EOF])?;
    let (addr, rest) = body.split_first()?;
    if *addr != address {
        return None;
    }
    let mut message = Vec::new();
    let mut iter = rest.iter();
    while let Some(b) = iter.next() {
        match *b {
            SOF | EOF => return None,
            ESC => message.push(iter.next()? ^ ESC_XOR),
            other => message.push(other),
        }
    }
    if message.len() < 3 {
        return None;
    }
    let crc_at = message.len() - 2;
    let sent = u16::from(message[crc_at]) << 8 | u16::from(message[crc_at + 1]);
    message.truncate(crc_at);
    if crc16(&message, crc16(&[address], 0)) != sent {
        return None;
    }
    let kind = message[0];
    Some((kind, message[1..].to_vec()))
}

fn frequency_of(payload: &[u8]) -> Option<Hz> {
    let word: [u8; 4] = payload.try_into().ok()?;
    Some(Hz(u64::from(u32::from_be_bytes(word))))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pending {
    Nothing,
    Get,
    Set(Hz),
}

/// The CTX-10 client engine.
#[derive(Debug)]
pub struct CommradioEngine {
    desc: RigDescription,
    pending: Pending,
    rx: RxBuffer,
}

impl CommradioEngine {
    // [@ANCHOR: radioctl_commradio_engine_new]
    /// Builds an engine from a CommRadio vendor description.
    pub fn new(desc: RigDescription) -> Result<CommradioEngine, RigError> {
        let (_, dialect) = vendor_of(&desc)?;
        if dialect != "ctx10" {
            return Err(unsupported("unknown CommRadio dialect in the description"));
        }
        Ok(CommradioEngine {
            desc,
            pending: Pending::Nothing,
            rx: RxBuffer::default(),
        })
    }
}

impl ProtocolEngine for CommradioEngine {
    fn description(&self) -> &RigDescription {
        &self.desc
    }

    // [@ANCHOR: radioctl_commradio_engine_encode]
    fn encode(&mut self, op: &Op) -> Result<Request, RigError> {
        if !self.desc.capabilities.has(op.required_capability()) {
            return Err(unsupported("capability not in the rig description"));
        }
        let (bytes, reply, pending) = match op {
            Op::GetFrequency(Vfo::A) => (
                frame(TO_RADIO, 0x32, &[]),
                ReplyKind::Observation(Field::Frequency(Vfo::A)),
                Pending::Get,
            ),
            Op::SetFrequency(Vfo::A, hz) => {
                if !RANGE.contains(&hz.0) {
                    return Err(RigError::new(
                        ErrorKind::Rejected,
                        "frequency outside 150 kHz to 30 MHz".to_string(),
                    ));
                }
                let word = u32::try_from(hz.0)
                    .map_err(|_| unsupported("frequency does not fit the radio"))?;
                (
                    frame(TO_RADIO, 0x30, &word.to_be_bytes()),
                    ReplyKind::Ack,
                    Pending::Set(*hz),
                )
            }
            _ => return Err(unsupported("not implemented for the CommRadio engine")),
        };
        self.pending = pending;
        self.rx.take_all();
        Ok(Request { bytes, reply })
    }

    // [@ANCHOR: radioctl_commradio_engine_feed]
    fn feed(&mut self, bytes: &[u8], now: Duration) -> Vec<Inbound> {
        let mut out = Vec::new();
        if let Some(stale) = self.rx.push(bytes, now) {
            out.push(Inbound::Unrecognised(stale));
        }
        while let Some(mut body) = self.rx.take_line(EOF) {
            body.push(EOF);
            let pending = std::mem::replace(&mut self.pending, Pending::Nothing);
            let parsed = unframe(&body, TO_HOST);
            let seen = match (pending, parsed) {
                (Pending::Set(want), Some((0x31 | 0x34, p))) => match frequency_of(&p) {
                    Some(hz) if hz == want => Some(Inbound::Ack),
                    Some(_) => Some(Inbound::Nak),
                    None => None,
                },
                (_, Some((0x33 | 0x34, p))) => frequency_of(&p)
                    .map(|hz| Inbound::Observation(Observation::Frequency(Vfo::A, hz))),
                _ => None,
            };
            // A frame that the pending operation did not use leaves it pending.
            if seen.is_none() {
                self.pending = pending;
            }
            out.push(seen.unwrap_or(Inbound::Unrecognised(body)));
        }
        out
    }
}

/// A virtual CTX-10.
#[derive(Debug)]
pub struct CommradioTwin {
    rx: RxBuffer,
    hz: Hz,
}

impl CommradioTwin {
    // [@ANCHOR: radioctl_commradio_twin_new]
    /// A twin in the description's default state.
    pub fn new(desc: &RigDescription) -> Result<CommradioTwin, RigError> {
        vendor_of(desc)?;
        Ok(CommradioTwin {
            rx: RxBuffer::default(),
            hz: desc.default_frequency,
        })
    }

    fn word(&self) -> [u8; 4] {
        u32::try_from(self.hz.0).unwrap_or(0).to_be_bytes()
    }
}

impl VendorTwin for CommradioTwin {
    // [@ANCHOR: radioctl_commradio_twin_feed]
    fn feed(&mut self, bytes: &[u8], now: Duration) -> Vec<u8> {
        self.rx.push(bytes, now);
        let mut out = Vec::new();
        while let Some(mut body) = self.rx.take_line(EOF) {
            body.push(EOF);
            match unframe(&body, TO_RADIO) {
                Some((0x32, p)) if p.is_empty() => {
                    out.extend(frame(TO_HOST, 0x33, &self.word()));
                }
                Some((0x30, p)) => {
                    if let Some(hz) = frequency_of(&p).filter(|h| RANGE.contains(&h.0)) {
                        self.hz = hz;
                    }
                    out.extend(frame(TO_HOST, 0x31, &self.word()));
                }
                _ => {}
            }
        }
        out
    }

    // [@ANCHOR: radioctl_commradio_twin_front_panel]
    fn front_panel(&mut self, panel: Panel) -> Result<Vec<u8>, String> {
        match panel {
            Panel::Frequency(hz) => {
                self.hz = hz;
                // The radio announces a knob turn by itself.
                Ok(frame(TO_HOST, 0x34, &self.word()))
            }
            other => Err(format!("a CTX-10 twin has no {other:?} knob")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const T0: Duration = Duration::from_secs(1);

    fn shipped() -> RigDescription {
        let path = format!(
            "{}/../../rigs/commradio-ctx-10.toml",
            env!("CARGO_MANIFEST_DIR")
        );
        let text = std::fs::read_to_string(path).expect("rig file");
        radioctl_core::rigfile::parse(&text)
            .expect("loads")
            .description
    }

    // Tests [@ANCHOR: radioctl_commradio_crc16]
    // Tests [@ANCHOR: radioctl_commradio_frame]
    // Tests [@ANCHOR: radioctl_commradio_unframe]
    #[test]
    fn frames_have_the_kermit_crc_and_escape_their_payload() {
        assert_eq!(crc16(b"123456789", 0), 0x2189);
        // The frequency request, built by hand: FE 21 32 crc16(21 32) FD.
        let crc = crc16(&[0x21, 0x32], 0);
        assert_eq!(
            frame(0x21, 0x32, &[]),
            vec![SOF, 0x21, 0x32, (crc >> 8) as u8, (crc & 0xFF) as u8, EOF]
        );
        // A payload full of the reserved bytes survives the round trip and carries no bare one.
        let nasty = [0xFC, 0xFD, 0xFE, 0x00];
        let f = frame(0x11, 0x33, &nasty);
        assert!(f[1..f.len() - 1].iter().all(|b| *b != SOF && *b != EOF));
        assert_eq!(unframe(&f, 0x11), Some((0x33, nasty.to_vec())));
        // Wrong address, a damaged CRC, a missing end and a bare reserved byte are refused.
        assert_eq!(unframe(&f, 0x21), None);
        let mut bad = f.clone();
        bad[3] ^= 1;
        assert_eq!(unframe(&bad, 0x11), None);
        assert_eq!(unframe(&f[..f.len() - 1], 0x11), None);
        assert_eq!(unframe(&[SOF, 0x11, SOF, 0, 0, EOF], 0x11), None);
        assert_eq!(unframe(&[SOF, 0x11, EOF], 0x11), None);
    }

    // Tests [@ANCHOR: radioctl_commradio_engine_new]
    #[test]
    fn engine_refuses_other_families() {
        assert!(CommradioEngine::new(RigDescription::ts2000()).is_err());
        assert!(CommradioTwin::new(&RigDescription::ts2000()).is_err());
    }

    // Tests [@ANCHOR: radioctl_commradio_engine_encode]
    // Tests [@ANCHOR: radioctl_commradio_engine_feed]
    // Tests [@ANCHOR: radioctl_commradio_twin_new]
    // Tests [@ANCHOR: radioctl_commradio_twin_feed]
    // Tests [@ANCHOR: radioctl_commradio_twin_front_panel]
    #[test]
    fn the_engine_round_trips_against_the_twin() {
        let desc = shipped();
        let mut e = CommradioEngine::new(desc.clone()).expect("engine");
        let mut t = CommradioTwin::new(&desc).expect("twin");
        let run = |e: &mut CommradioEngine, t: &mut CommradioTwin, op: Op| {
            let r = e.encode(&op).expect("encodes");
            let a = t.feed(&r.bytes, T0);
            (r.bytes, e.feed(&a, T0))
        };
        let (bytes, seen) = run(&mut e, &mut t, Op::SetFrequency(Vfo::A, Hz(14_200_000)));
        assert_eq!(unframe(&bytes, TO_RADIO).expect("frame").0, 0x30);
        assert_eq!(seen, vec![Inbound::Ack]);
        assert_eq!(
            run(&mut e, &mut t, Op::GetFrequency(Vfo::A)).1,
            vec![Inbound::Observation(Observation::Frequency(
                Vfo::A,
                Hz(14_200_000)
            ))]
        );
        // A frequency whose bytes are 00 FD FC FE: every reserved byte needs an escape.
        let odd = 0x00FD_FCFEu64;
        assert!(RANGE.contains(&odd));
        run(&mut e, &mut t, Op::SetFrequency(Vfo::A, Hz(odd)));
        assert_eq!(
            run(&mut e, &mut t, Op::GetFrequency(Vfo::A)).1,
            vec![Inbound::Observation(Observation::Frequency(
                Vfo::A,
                Hz(odd)
            ))]
        );
        assert!(e.encode(&Op::SetFrequency(Vfo::A, Hz(100))).is_err());
        assert!(e.encode(&Op::SetFrequency(Vfo::A, Hz(31_000_000))).is_err());
        assert!(e.encode(&Op::GetMode).is_err());
        assert!(e.encode(&Op::SetPtt(true)).is_err());
        // The knob turn arrives unasked.
        let announce = t
            .front_panel(Panel::Frequency(Hz(7_100_000)))
            .expect("tunes");
        assert_eq!(
            e.feed(&announce, T0),
            vec![Inbound::Observation(Observation::Frequency(
                Vfo::A,
                Hz(7_100_000)
            ))]
        );
        assert!(t.front_panel(Panel::Ptt(true)).is_err());
        // A different frequency in the set answer is refused, and so is a damaged frame.
        e.encode(&Op::SetFrequency(Vfo::A, Hz(7_000_000)))
            .expect("set");
        let wrong = frame(TO_HOST, 0x31, &7_001_000u32.to_be_bytes());
        assert_eq!(e.feed(&wrong, T0), vec![Inbound::Nak]);
        let mut damaged = frame(TO_HOST, 0x33, &7_001_000u32.to_be_bytes());
        damaged[3] ^= 1;
        assert!(matches!(
            e.feed(&damaged, T0).as_slice(),
            [Inbound::Unrecognised(_)]
        ));
    }
}
