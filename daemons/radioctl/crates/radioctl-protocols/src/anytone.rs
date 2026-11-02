// SPDX-License-Identifier: AGPL-3.0-or-later
//! AnyTone D578A (AT-D578UV III) mobile: sans-IO client engine and virtual twin.
//!
//! **Sources.** Facts only, read from Hamlib 4.6.2 (released tarball, sha256
//! b2ac73f44dd1161e95fdee6c95276144757647bf92d7fdb369ee2fe41ed47ae8): `rigs/anytone/anytone.c`
//! (`anytone_open`, `anytone_get_freq`, `anytone_set_ptt`) and `d578.c`. Hamlib calls the driver Beta;
//! it imitates the vendor programming tool's serial dialogue and its author wrote no protocol text.
//! No AnyTone document exists in public (the Hamlib developers say the radio documents no CAT), so
//! nothing here is verified against a radio, and the framing below is Hamlib's guess at it.
//!
//! The serial line runs at 115200 baud, 8N1. Every record is the ASCII header `+ADATA:00,<n>` and
//! CR LF, then a body. What the engine speaks:
//!
//! * the open dialogue: Hamlib sends a record `001` with body `a`, waits half a second, then a
//!   `016` record whose body is `0x01 "D578UV COM MODE"` and a `000` record that the radio echoes.
//!   **The engine sends only the last two.** Hamlib's PTT-on record is byte for byte the same as
//!   its `001`/`a` open record and its one-second keep-alive, so that record may key the radio; an
//!   engine must never key at open, so the question is left to a capture;
//! * read frequency: record `006` with body `04 2C 07 00 00 00 00 00 00 CR LF` (VFO B: `04 2D`
//!   instead); the radio answers 138 bytes whose bytes 17 to 20 are eight BCD digits in units of
//!   10 Hz. Hamlib leaves one byte of its own request buffer unset (offset 20); the engine sends
//!   zero there;
//! * Hamlib's frequency setter writes frames and then returns "not implemented", its VFO setter
//!   does nothing, and its PTT state is its own memory, so those are not in the engine. The PTT-off
//!   record (`023` with body `V`) is not either: unkeying needs the keying record to be known.
//!
//! The one-second keep-alive that Hamlib's thread sends is not in the engine (a sans-IO engine has
//! no clock); whether the radio drops the session without it is an open question.

use std::time::Duration;

use radioctl_core::description::RigDescription;
use radioctl_core::engine::{Inbound, ProtocolEngine, ReplyKind, Request};
use radioctl_core::error::{ErrorKind, RigError};
use radioctl_core::op::{Field, Observation, Op};
use radioctl_core::transcript::Panel;
use radioctl_core::units::{Hz, Vfo};

use crate::serial_vendor::{vendor_of, RxBuffer, TwinSnapshot, VendorTwin};

/// Length of a frequency answer, bytes.
const ANSWER_LEN: usize = 138;
/// Offset of the four BCD bytes in the answer.
const BCD_AT: usize = 17;
/// Header of the `000` record, which the radio echoes.
const CLOSE_RECORD: &[u8] = b"+ADATA:00,000\r\n";

fn unsupported(what: &str) -> RigError {
    RigError::new(ErrorKind::Unsupported, what.to_string())
}

// [@ANCHOR: radioctl_anytone_open_records]
/// The two open records the engine sends: the mode announcement and the `000` record.
pub fn open_records() -> Vec<u8> {
    let mut out = b"+ADATA:00,016\r\n\x01D578UV COM MODE\r\n".to_vec();
    out.extend_from_slice(CLOSE_RECORD);
    out
}

// [@ANCHOR: radioctl_anytone_read_frequency_record]
/// The 25-byte record that asks for the frequency of a VFO.
pub fn read_frequency_record(vfo: Vfo) -> Vec<u8> {
    let mut out = b"+ADATA:00,006\r\n".to_vec();
    out.extend_from_slice(&[
        0x04,
        if vfo == Vfo::B { 0x2D } else { 0x2C },
        0x07,
        0,
        0,
        0,
        0,
        0,
        0x0D,
        0x0A,
    ]);
    out
}

// [@ANCHOR: radioctl_anytone_answer_frequency]
/// The frequency in an answer of 138 bytes: eight BCD digits at offset 17, units of 10 Hz.
pub fn answer_frequency(answer: &[u8]) -> Option<Hz> {
    if answer.len() != ANSWER_LEN || !answer.starts_with(b"+ADATA:") {
        return None;
    }
    let mut tens = 0u64;
    for byte in &answer[BCD_AT..BCD_AT + 4] {
        let (hi, lo) = (byte >> 4, byte & 0x0F);
        if hi > 9 || lo > 9 {
            return None;
        }
        tens = tens * 100 + u64::from(hi) * 10 + u64::from(lo);
    }
    Some(Hz(tens * 10))
}

// [@ANCHOR: radioctl_anytone_frequency_answer]
/// An answer of 138 bytes for a frequency (the twin's shape; the payload length is a guess).
pub fn frequency_answer(vfo: Vfo, hz: Hz) -> Vec<u8> {
    let mut out = b"+ADATA:00,121\r\n".to_vec();
    out.push(0x04);
    out.push(if vfo == Vfo::B { 0x2D } else { 0x2C });
    let tens = hz.0 / 10;
    for shift in [6u32, 4, 2, 0] {
        let pair = (tens / 10u64.pow(shift)) % 100;
        out.push((((pair / 10) << 4) | (pair % 10)) as u8);
    }
    out.resize(ANSWER_LEN - 2, 0);
    out.extend_from_slice(b"\r\n");
    out
}

/// The AnyTone D578A client engine.
#[derive(Debug)]
pub struct AnytoneEngine {
    desc: RigDescription,
    rx: RxBuffer,
    asked: Option<Vfo>,
}

impl AnytoneEngine {
    // [@ANCHOR: radioctl_anytone_engine_new]
    /// Builds an engine from an AnyTone vendor description.
    pub fn new(desc: RigDescription) -> Result<AnytoneEngine, RigError> {
        let (_, name) = vendor_of(&desc)?;
        if name != "d578a" {
            return Err(unsupported("unknown AnyTone dialect in the description"));
        }
        Ok(AnytoneEngine {
            desc,
            rx: RxBuffer::default(),
            asked: None,
        })
    }
}

impl ProtocolEngine for AnytoneEngine {
    fn description(&self) -> &RigDescription {
        &self.desc
    }

    // [@ANCHOR: radioctl_anytone_engine_encode]
    fn encode(&mut self, op: &Op) -> Result<Request, RigError> {
        if !self.desc.capabilities.has(op.required_capability()) {
            return Err(unsupported("capability not in the rig description"));
        }
        match op {
            Op::GetFrequency(vfo) => {
                self.asked = Some(*vfo);
                Ok(Request {
                    bytes: read_frequency_record(*vfo),
                    reply: ReplyKind::Observation(Field::Frequency(*vfo)),
                })
            }
            _ => Err(unsupported("not implemented for the AnyTone engine")),
        }
    }

    // [@ANCHOR: radioctl_anytone_engine_feed]
    fn feed(&mut self, bytes: &[u8], now: Duration) -> Vec<Inbound> {
        let mut out = Vec::new();
        if let Some(stale) = self.rx.push(bytes, now) {
            out.push(Inbound::Unrecognised(stale));
        }
        loop {
            // The radio echoes the `000` record; that is not an answer.
            if self.rx.peek().starts_with(CLOSE_RECORD) {
                self.rx.take(CLOSE_RECORD.len());
                continue;
            }
            // Wait while the head can still become the echo.
            if CLOSE_RECORD.starts_with(self.rx.peek()) && !self.rx.peek().is_empty() {
                break;
            }
            let Some(answer) = self.rx.take(ANSWER_LEN) else {
                break;
            };
            match (self.asked.take(), answer_frequency(&answer)) {
                (Some(vfo), Some(hz)) => {
                    out.push(Inbound::Observation(Observation::Frequency(vfo, hz)))
                }
                _ => out.push(Inbound::Unrecognised(answer)),
            }
        }
        out
    }

    // [@ANCHOR: radioctl_anytone_engine_open_requests]
    fn open_requests(&mut self) -> Vec<Request> {
        vec![Request {
            bytes: open_records(),
            reply: ReplyKind::None,
        }]
    }

    // [@ANCHOR: radioctl_anytone_engine_link_restored]
    fn link_restored(&mut self) -> Vec<Request> {
        self.rx.take_all();
        self.asked = None;
        self.open_requests()
    }
}

/// A virtual AnyTone D578A.
#[derive(Debug)]
pub struct AnytoneTwin {
    rx: RxBuffer,
    hz: [Hz; 2],
}

impl AnytoneTwin {
    // [@ANCHOR: radioctl_anytone_twin_new]
    /// A twin in the description's default state (both VFOs on the default frequency).
    pub fn new(desc: &RigDescription) -> Result<AnytoneTwin, RigError> {
        let (_, name) = vendor_of(desc)?;
        if name != "d578a" {
            return Err(unsupported("unknown AnyTone dialect in the description"));
        }
        Ok(AnytoneTwin {
            rx: RxBuffer::default(),
            hz: [desc.default_frequency; 2],
        })
    }
}

impl VendorTwin for AnytoneTwin {
    // [@ANCHOR: radioctl_anytone_twin_snapshot]
    fn snapshot(&self) -> Option<TwinSnapshot> {
        Some(TwinSnapshot {
            frequency: self.hz[0],
            mode_code: None,
            ptt: false,
        })
    }

    // [@ANCHOR: radioctl_anytone_twin_feed]
    fn feed(&mut self, bytes: &[u8], now: Duration) -> Vec<u8> {
        self.rx.push(bytes, now);
        let mut out = Vec::new();
        loop {
            let head = self.rx.peek();
            if head.starts_with(CLOSE_RECORD) {
                self.rx.take(CLOSE_RECORD.len());
                out.extend_from_slice(CLOSE_RECORD);
            } else if head.starts_with(b"+ADATA:00,016\r\n") {
                // The mode announcement: header, 0x01, the text, CR LF.
                let Some(end) = head.iter().position(|b| *b == b'\n').and_then(|first| {
                    head[first + 1..]
                        .iter()
                        .position(|b| *b == b'\n')
                        .map(|n| first + 1 + n + 1)
                }) else {
                    break;
                };
                self.rx.take(end);
            } else if head.starts_with(b"+ADATA:00,006\r\n") {
                if head.len() < 25 {
                    break;
                }
                let record = self.rx.take(25).unwrap_or_default();
                let vfo = if record[16] == 0x2D { Vfo::B } else { Vfo::A };
                let hz = self.hz[usize::from(vfo == Vfo::B)];
                out.extend(frequency_answer(vfo, hz));
            } else {
                // Not a record the twin knows: drop one byte to resynchronise.
                if head.is_empty() || (b"+ADATA:00,".starts_with(head) && head.len() < 15) {
                    break;
                }
                self.rx.take(1);
            }
        }
        out
    }

    // [@ANCHOR: radioctl_anytone_twin_front_panel]
    fn front_panel(&mut self, panel: Panel) -> Result<Vec<u8>, String> {
        match panel {
            Panel::Frequency(hz) => self.hz[0] = hz,
            other => return Err(format!("an AnyTone twin has no {other:?} knob")),
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
            "{}/../../rigs/anytone-d578a.toml",
            env!("CARGO_MANIFEST_DIR")
        );
        let text = std::fs::read_to_string(path).expect("rig file");
        radioctl_core::rigfile::parse(&text)
            .expect("loads")
            .description
    }

    // Tests [@ANCHOR: radioctl_anytone_open_records]
    // Tests [@ANCHOR: radioctl_anytone_read_frequency_record]
    // Tests [@ANCHOR: radioctl_anytone_answer_frequency]
    // Tests [@ANCHOR: radioctl_anytone_frequency_answer]
    // Tests [@ANCHOR: radioctl_anytone_engine_new]
    #[test]
    fn records_and_answers_have_the_shape_hamlib_uses() {
        assert!(open_records().ends_with(b"+ADATA:00,000\r\n"));
        assert!(!open_records()
            .windows(15)
            .any(|w| w == b"+ADATA:00,001\r\n"));
        let a = read_frequency_record(Vfo::A);
        assert_eq!(a.len(), 25);
        assert_eq!(&a[15..18], &[0x04, 0x2C, 0x07]);
        assert_eq!(read_frequency_record(Vfo::B)[16], 0x2D);
        let answer = frequency_answer(Vfo::A, Hz(430_100_000));
        assert_eq!(answer.len(), ANSWER_LEN);
        assert_eq!(&answer[17..21], &[0x43, 0x01, 0x00, 0x00]);
        assert_eq!(answer_frequency(&answer), Some(Hz(430_100_000)));
        assert_eq!(answer_frequency(&answer[1..]), None);
        let mut bad = answer.clone();
        bad[18] = 0xA0;
        assert_eq!(answer_frequency(&bad), None);
        assert!(AnytoneEngine::new(RigDescription::ts2000()).is_err());
        assert!(AnytoneTwin::new(&RigDescription::ts2000()).is_err());
    }

    // Tests [@ANCHOR: radioctl_anytone_engine_encode]
    // Tests [@ANCHOR: radioctl_anytone_engine_feed]
    // Tests [@ANCHOR: radioctl_anytone_engine_open_requests]
    // Tests [@ANCHOR: radioctl_anytone_engine_link_restored]
    // Tests [@ANCHOR: radioctl_anytone_twin_new]
    // Tests [@ANCHOR: radioctl_anytone_twin_snapshot]
    // Tests [@ANCHOR: radioctl_anytone_twin_feed]
    // Tests [@ANCHOR: radioctl_anytone_twin_front_panel]
    #[test]
    fn the_engine_reads_both_vfos_from_the_twin_and_the_open_dialogue_is_echoed() {
        let desc = shipped();
        let mut e = AnytoneEngine::new(desc.clone()).expect("engine");
        let mut t = AnytoneTwin::new(&desc).expect("twin");
        let open = e.open_requests();
        assert_eq!(open.len(), 1);
        assert_eq!(open[0].reply, ReplyKind::None);
        // The twin echoes the 000 record; the engine takes that for no answer.
        let echo = t.feed(&open[0].bytes, T0);
        assert_eq!(echo, CLOSE_RECORD);
        assert!(e.feed(&echo, T0).is_empty());
        t.front_panel(Panel::Frequency(Hz(144_390_000)))
            .expect("knob");
        assert_eq!(t.snapshot().expect("snapshot").frequency, Hz(144_390_000));
        let r = e.encode(&Op::GetFrequency(Vfo::A)).expect("encodes");
        assert_eq!(r.reply, ReplyKind::Observation(Field::Frequency(Vfo::A)));
        let answer = t.feed(&r.bytes, T0);
        // It may arrive in pieces.
        assert!(e.feed(&answer[..60], T0).is_empty());
        assert_eq!(
            e.feed(&answer[60..], T0),
            vec![Inbound::Observation(Observation::Frequency(
                Vfo::A,
                Hz(144_390_000)
            ))]
        );
        let r = e.encode(&Op::GetFrequency(Vfo::B)).expect("encodes");
        let events = e.feed(&t.feed(&r.bytes, T0), T0);
        assert_eq!(
            events,
            vec![Inbound::Observation(Observation::Frequency(
                Vfo::B,
                desc.default_frequency
            ))]
        );
        // An answer nobody asked for is unrecognised; a link restored opens again.
        let stray = frequency_answer(Vfo::A, Hz(1));
        assert_eq!(
            e.feed(&stray, T0),
            vec![Inbound::Unrecognised(stray.clone())]
        );
        e.feed(&stray[..10], T0);
        assert_eq!(e.link_restored().len(), 1);
        assert!(e.feed(&echo, T0).is_empty(), "the old half answer is gone");
        // The engine refuses what Hamlib's driver does not do.
        assert!(e.encode(&Op::SetFrequency(Vfo::A, Hz(1))).is_err());
        assert!(e.encode(&Op::SetPtt(true)).is_err());
        assert!(t.front_panel(Panel::Ptt(true)).is_err());
    }
}
