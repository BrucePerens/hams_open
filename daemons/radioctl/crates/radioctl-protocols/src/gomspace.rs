// SPDX-License-Identifier: AGPL-3.0-or-later
//! GomSpace GS100 (NanoCom AX100 console) ground-station transceiver: sans-IO client engine and
//! virtual twin.
//!
//! **Sources.** Facts only, read from Hamlib 4.6.2 (released tarball, sha256
//! b2ac73f44dd1161e95fdee6c95276144757647bf92d7fdb369ee2fe41ed47ae8): `rigs/gomspace/gs100.c`.
//! No GomSpace document was read; nothing is verified against a radio.
//!
//! The radio is a text console. `param mem <n>` selects a configuration table (1 receive, 5
//! transmit), `param set freq <Hz>` sets the frequency in the selected table and `param get
//! freq` asks for it. Every command is echoed and ends with the console prompt, which carries
//! colour escape codes. Hamlib sends the table selection as a separate command before each
//! set or get; the engine sends both in one write and waits for two prompts. A get is answered
//! on a line `freq = <Hz>`; a set must answer with nothing but the echo. VFO A is the receive
//! table and VFO B the transmit table (Hamlib's split frequency).

use std::time::Duration;

use radioctl_core::description::RigDescription;
use radioctl_core::engine::{Inbound, ProtocolEngine, ReplyKind, Request};
use radioctl_core::error::{ErrorKind, RigError};
use radioctl_core::op::{Field, Observation, Op};
use radioctl_core::transcript::Panel;
use radioctl_core::units::{Hz, Vfo};

use crate::serial_vendor::{parse_decimal, vendor_of, RxBuffer, VendorTwin};

/// The console prompt, escape codes included.
pub const PROMPT: &[u8] = b"\x1b[1;32mnanocom-ax\x1b[1;30m # \x1b[0m\x1b[0m";

const TABLE_RX: u8 = 1;
const TABLE_TX: u8 = 5;

fn unsupported(what: &str) -> RigError {
    RigError::new(ErrorKind::Unsupported, what.to_string())
}

fn table_of(vfo: Vfo) -> u8 {
    match vfo {
        Vfo::A => TABLE_RX,
        Vfo::B => TABLE_TX,
    }
}

// [@ANCHOR: radioctl_gomspace_count_prompts]
/// How many prompts `bytes` holds.
pub fn count_prompts(bytes: &[u8]) -> usize {
    bytes.windows(PROMPT.len()).filter(|w| *w == PROMPT).count()
}

// [@ANCHOR: radioctl_gomspace_lines_between_prompts]
/// The text lines of `bytes` outside the prompts, without line ends and without blank lines.
pub fn lines_without_prompts(bytes: &[u8]) -> Vec<Vec<u8>> {
    let mut text = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i..].starts_with(PROMPT) {
            text.push(b'\n');
            i += PROMPT.len();
        } else {
            text.push(bytes[i]);
            i += 1;
        }
    }
    text.split(|b| *b == b'\n')
        .map(|l| l.trim_ascii().to_vec())
        .filter(|l| !l.is_empty())
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pending {
    Nothing,
    Get(Vfo),
    Set,
}

/// The GS100 client engine.
#[derive(Debug)]
pub struct GomspaceEngine {
    desc: RigDescription,
    pending: Pending,
    rx: RxBuffer,
}

impl GomspaceEngine {
    // [@ANCHOR: radioctl_gomspace_engine_new]
    /// Builds an engine from a GomSpace vendor description.
    pub fn new(desc: RigDescription) -> Result<GomspaceEngine, RigError> {
        let (_, dialect) = vendor_of(&desc)?;
        if dialect != "gs100" {
            return Err(unsupported("unknown GomSpace dialect in the description"));
        }
        Ok(GomspaceEngine {
            desc,
            pending: Pending::Nothing,
            rx: RxBuffer::default(),
        })
    }
}

impl ProtocolEngine for GomspaceEngine {
    fn description(&self) -> &RigDescription {
        &self.desc
    }

    // [@ANCHOR: radioctl_gomspace_engine_encode]
    fn encode(&mut self, op: &Op) -> Result<Request, RigError> {
        if !self.desc.capabilities.has(op.required_capability()) {
            return Err(unsupported("capability not in the rig description"));
        }
        let (text, reply, pending) = match op {
            Op::GetFrequency(vfo) => (
                format!("param mem {}\nparam get freq\n", table_of(*vfo)),
                ReplyKind::Observation(Field::Frequency(*vfo)),
                Pending::Get(*vfo),
            ),
            Op::SetFrequency(vfo, hz) => (
                format!("param mem {}\nparam set freq {}\n", table_of(*vfo), hz.0),
                ReplyKind::Ack,
                Pending::Set,
            ),
            _ => return Err(unsupported("not implemented for the GomSpace engine")),
        };
        self.pending = pending;
        self.rx.take_all();
        Ok(Request {
            bytes: text.into_bytes(),
            reply,
        })
    }

    // [@ANCHOR: radioctl_gomspace_engine_feed]
    fn feed(&mut self, bytes: &[u8], now: Duration) -> Vec<Inbound> {
        let mut out = Vec::new();
        if let Some(stale) = self.rx.push(bytes, now) {
            out.push(Inbound::Unrecognised(stale));
        }
        // Two commands were sent, so the answer is whole at the second prompt.
        if self.pending == Pending::Nothing || count_prompts(self.rx.peek()) < 2 {
            return out;
        }
        let all = self.rx.take_all();
        let lines = lines_without_prompts(&all);
        // Drop the two command echoes; whatever is left is the answer to the second command.
        let answer: Vec<&Vec<u8>> = lines.iter().filter(|l| !l.starts_with(b"param ")).collect();
        let pending = std::mem::replace(&mut self.pending, Pending::Nothing);
        out.push(match (pending, answer.as_slice()) {
            (Pending::Set, []) => Inbound::Ack,
            (Pending::Set, _) => Inbound::Nak,
            (Pending::Get(vfo), [line]) => {
                let value = line
                    .iter()
                    .position(|b| *b == b'=')
                    .and_then(|i| parse_decimal(line[i + 1..].trim_ascii()));
                match value {
                    Some(hz) => Inbound::Observation(Observation::Frequency(vfo, Hz(hz))),
                    None => Inbound::Unrecognised((*line).clone()),
                }
            }
            _ => Inbound::Unrecognised(all),
        });
        out
    }
}

/// A virtual GS100.
#[derive(Debug)]
pub struct GomspaceTwin {
    rx: RxBuffer,
    table: u8,
    rx_hz: Hz,
    tx_hz: Hz,
}

impl GomspaceTwin {
    // [@ANCHOR: radioctl_gomspace_twin_new]
    /// A twin in the description's default state.
    pub fn new(desc: &RigDescription) -> Result<GomspaceTwin, RigError> {
        vendor_of(desc)?;
        Ok(GomspaceTwin {
            rx: RxBuffer::default(),
            table: TABLE_RX,
            rx_hz: desc.default_frequency,
            tx_hz: desc.default_frequency,
        })
    }

    fn slot(&mut self) -> Option<&mut Hz> {
        match self.table {
            TABLE_RX => Some(&mut self.rx_hz),
            TABLE_TX => Some(&mut self.tx_hz),
            _ => None,
        }
    }
}

impl VendorTwin for GomspaceTwin {
    // [@ANCHOR: radioctl_gomspace_twin_feed]
    fn feed(&mut self, bytes: &[u8], now: Duration) -> Vec<u8> {
        self.rx.push(bytes, now);
        let mut out = Vec::new();
        while let Some(line) = self.rx.take_line(b'\n') {
            out.extend_from_slice(&line);
            out.extend_from_slice(b"\r\n");
            if let Some(n) = line.strip_prefix(b"param mem ") {
                if let Some(n) = parse_decimal(n).and_then(|n| u8::try_from(n).ok()) {
                    self.table = n;
                }
            } else if line == b"param get freq" {
                if let Some(hz) = self.slot().map(|h| h.0) {
                    out.extend(format!("freq = {hz}\r\n").into_bytes());
                }
            } else if let Some(v) = line.strip_prefix(b"param set freq ") {
                if let (Some(hz), Some(slot)) = (parse_decimal(v), self.slot()) {
                    *slot = Hz(hz);
                }
            }
            out.extend_from_slice(PROMPT);
            out.push(b'\n');
        }
        out
    }

    // [@ANCHOR: radioctl_gomspace_twin_front_panel]
    fn front_panel(&mut self, panel: Panel) -> Result<Vec<u8>, String> {
        match panel {
            Panel::Frequency(hz) => self.rx_hz = hz,
            other => return Err(format!("a GS100 twin has no {other:?} knob")),
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
            "{}/../../rigs/gomspace-gs100.toml",
            env!("CARGO_MANIFEST_DIR")
        );
        let text = std::fs::read_to_string(path).expect("rig file");
        radioctl_core::rigfile::parse(&text)
            .expect("loads")
            .description
    }

    // Tests [@ANCHOR: radioctl_gomspace_count_prompts]
    // Tests [@ANCHOR: radioctl_gomspace_lines_between_prompts]
    #[test]
    fn prompts_are_counted_and_stripped() {
        let mut bytes = b"param mem 1\r\n".to_vec();
        bytes.extend_from_slice(PROMPT);
        bytes.extend_from_slice(b"\nfreq = 5\r\n\r\n");
        bytes.extend_from_slice(PROMPT);
        assert_eq!(count_prompts(&bytes), 2);
        assert_eq!(
            lines_without_prompts(&bytes),
            vec![b"param mem 1".to_vec(), b"freq = 5".to_vec()]
        );
        assert_eq!(count_prompts(b"nothing"), 0);
    }

    // Tests [@ANCHOR: radioctl_gomspace_engine_new]
    #[test]
    fn engine_refuses_other_families() {
        assert!(GomspaceEngine::new(RigDescription::ts2000()).is_err());
        assert!(GomspaceTwin::new(&RigDescription::ts2000()).is_err());
    }

    // Tests [@ANCHOR: radioctl_gomspace_engine_encode]
    // Tests [@ANCHOR: radioctl_gomspace_engine_feed]
    // Tests [@ANCHOR: radioctl_gomspace_twin_new]
    // Tests [@ANCHOR: radioctl_gomspace_twin_feed]
    // Tests [@ANCHOR: radioctl_gomspace_twin_front_panel]
    #[test]
    fn the_engine_round_trips_against_the_twin() {
        let desc = shipped();
        let mut e = GomspaceEngine::new(desc.clone()).expect("engine");
        let mut t = GomspaceTwin::new(&desc).expect("twin");
        let run = |e: &mut GomspaceEngine, t: &mut GomspaceTwin, op: Op| {
            let r = e.encode(&op).expect("encodes");
            let a = t.feed(&r.bytes, T0);
            (r.bytes, e.feed(&a, T0))
        };
        let (bytes, seen) = run(&mut e, &mut t, Op::SetFrequency(Vfo::A, Hz(436_500_000)));
        assert_eq!(bytes, b"param mem 1\nparam set freq 436500000\n");
        assert_eq!(seen, vec![Inbound::Ack]);
        let (bytes, seen) = run(&mut e, &mut t, Op::SetFrequency(Vfo::B, Hz(437_000_000)));
        assert_eq!(bytes, b"param mem 5\nparam set freq 437000000\n");
        assert_eq!(seen, vec![Inbound::Ack]);
        for (vfo, hz) in [(Vfo::A, 436_500_000), (Vfo::B, 437_000_000)] {
            assert_eq!(
                run(&mut e, &mut t, Op::GetFrequency(vfo)).1,
                vec![Inbound::Observation(Observation::Frequency(vfo, Hz(hz)))]
            );
        }
        assert!(e.encode(&Op::GetMode).is_err());
        assert!(e.encode(&Op::SetPtt(true)).is_err());
        t.front_panel(Panel::Frequency(Hz(435_000_000)))
            .expect("tunes");
        assert_eq!(
            run(&mut e, &mut t, Op::GetFrequency(Vfo::A)).1,
            vec![Inbound::Observation(Observation::Frequency(
                Vfo::A,
                Hz(435_000_000)
            ))]
        );
        assert!(t.front_panel(Panel::Ptt(true)).is_err());
        // Waits for the second prompt; an extra line after a set is an error; junk is not a frequency.
        e.encode(&Op::GetFrequency(Vfo::A)).expect("get");
        assert!(e.feed(b"param mem 1\r\n", T0).is_empty());
        e.encode(&Op::SetFrequency(Vfo::A, Hz(435_000_000)))
            .expect("set");
        let mut reply = b"param mem 1\r\n".to_vec();
        reply.extend_from_slice(PROMPT);
        reply.extend_from_slice(b"param set freq 435000000\r\nerror\r\n");
        reply.extend_from_slice(PROMPT);
        assert_eq!(e.feed(&reply, T0), vec![Inbound::Nak]);
        e.encode(&Op::GetFrequency(Vfo::A)).expect("get");
        let mut reply = b"param mem 1\r\n".to_vec();
        reply.extend_from_slice(PROMPT);
        reply.extend_from_slice(b"param get freq\r\nfreq = x\r\n");
        reply.extend_from_slice(PROMPT);
        assert!(matches!(
            e.feed(&reply, T0).as_slice(),
            [Inbound::Unrecognised(_)]
        ));
    }
}
