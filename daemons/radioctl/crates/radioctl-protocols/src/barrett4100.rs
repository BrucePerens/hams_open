// SPDX-License-Identifier: AGPL-3.0-or-later
//! Barrett 4100 HF transceiver: sans-IO client engine and virtual twin.
//!
//! **Sources.** Facts only, read from Hamlib 4.6.2 (released tarball, sha256
//! b2ac73f44dd1161e95fdee6c95276144757647bf92d7fdb369ee2fe41ed47ae8): `rigs/barrett/4100.c` and
//! `barrett.c` (`barrett_transaction2`). Hamlib registers the radio under the maker name
//! "Rhode&Schwarz", which is a mistake in its table. Hamlib calls the driver Beta and says nobody
//! has tested it. No Barrett document was read; nothing is verified against a radio.
//!
//! Commands are a line feed, the text and a CR. The radio is put in remote mode first
//! (`M:REMOTE SENTER2,1`, answered `s`; `M:REMOTE SENTER0` leaves it). Then:
//!
//! * `M:FF SRF<hz> GRF` sets the receive frequency and `M:FF STF<hz> GTF` the transmit
//!   frequency (both are sent, as Hamlib does); each is answered `s gRF<hz>` / `s gTF<hz>`;
//!   `M:FF GRF` reads the receive frequency, answered `gRF<hz>`;
//! * `M:FF SRPTT1 GRPTT` keys and `M:FF SRPTT0 GRPTT` unkeys, answered `s gRPTT<0|1>`. Hamlib
//!   sends `M:FF GRPTT` to read the state but never looks at the answer, so there is no PTT read.
//!
//! Answers are framed as the other Barrett radios do (XOFF, the text, CR LF, XON); Hamlib reads
//! them with odd fixed lengths and the framing of the 4100's answers is an open question, as is
//! the exact answer text (the twin uses the forms Hamlib's `sscanf` formats name). The 4100 is a
//! network radio in Hamlib; the engine speaks over any byte stream. The remote-mode command is
//! sent before the first command and again by [`ProtocolEngine::link_restored`].

use std::collections::VecDeque;
use std::time::Duration;

use radioctl_core::description::RigDescription;
use radioctl_core::engine::{Inbound, ProtocolEngine, ReplyKind, Request};
use radioctl_core::error::{ErrorKind, RigError};
use radioctl_core::op::{Field, Observation, Op};
use radioctl_core::transcript::Panel;
use radioctl_core::units::{Hz, Vfo};

use crate::barrett::{frame, unframe};
use crate::serial_vendor::{parse_decimal, vendor_of, RxBuffer, TwinSnapshot, VendorTwin};

const XON: u8 = 0x11;
const CR: u8 = b'\r';
const LF: u8 = b'\n';
/// The remote-mode command.
const ENTER: &str = "M:REMOTE SENTER2,1";
/// Lowest frequency, hertz (Hamlib's receive range).
pub const MIN_HZ: u64 = 10_000;
/// Highest frequency, hertz (Hamlib's receive range).
pub const MAX_HZ: u64 = 30_000_000;

fn unsupported(what: &str) -> RigError {
    RigError::new(ErrorKind::Unsupported, what.to_string())
}

// [@ANCHOR: radioctl_barrett4100_command]
/// A command on the wire: line feed, text, CR.
pub fn command(text: &str) -> Vec<u8> {
    let mut out = vec![LF];
    out.extend_from_slice(text.as_bytes());
    out.push(CR);
    out
}

/// What the next answer is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Expect {
    /// The answer to the remote-mode command.
    Session,
    /// This many `s ...` answers to writes, the last one ends the operation.
    Writes(u8),
    /// The receive frequency.
    Frequency,
}

/// The Barrett 4100 client engine.
#[derive(Debug)]
pub struct Barrett4100Engine {
    desc: RigDescription,
    rx: RxBuffer,
    entered: bool,
    expect: VecDeque<Expect>,
}

impl Barrett4100Engine {
    // [@ANCHOR: radioctl_barrett4100_engine_new]
    /// Builds an engine from a Barrett 4100 vendor description.
    pub fn new(desc: RigDescription) -> Result<Barrett4100Engine, RigError> {
        let (_, name) = vendor_of(&desc)?;
        if name != "barrett4100" {
            return Err(unsupported(
                "unknown Barrett 4100 dialect in the description",
            ));
        }
        Ok(Barrett4100Engine {
            desc,
            rx: RxBuffer::default(),
            entered: false,
            expect: VecDeque::new(),
        })
    }
}

impl ProtocolEngine for Barrett4100Engine {
    fn description(&self) -> &RigDescription {
        &self.desc
    }

    // [@ANCHOR: radioctl_barrett4100_engine_encode]
    fn encode(&mut self, op: &Op) -> Result<Request, RigError> {
        if !self.desc.capabilities.has(op.required_capability()) {
            return Err(unsupported("capability not in the rig description"));
        }
        let (bytes, expect, reply) = match op {
            Op::GetFrequency(Vfo::A) => (
                command("M:FF GRF"),
                Expect::Frequency,
                ReplyKind::Observation(Field::Frequency(Vfo::A)),
            ),
            Op::SetFrequency(Vfo::A, hz) => {
                if hz.0 < MIN_HZ || hz.0 > MAX_HZ {
                    return Err(unsupported("frequency outside 10 kHz to 30 MHz"));
                }
                let mut bytes = command(&format!("M:FF SRF{} GRF", hz.0));
                bytes.extend(command(&format!("M:FF STF{} GTF", hz.0)));
                (bytes, Expect::Writes(2), ReplyKind::Ack)
            }
            Op::SetPtt(on) => (
                command(&format!("M:FF SRPTT{} GRPTT", u8::from(*on))),
                Expect::Writes(1),
                ReplyKind::Ack,
            ),
            _ => return Err(unsupported("not implemented for the Barrett 4100 engine")),
        };
        let mut all = Vec::new();
        if !self.entered {
            all.extend(command(ENTER));
            self.expect.push_back(Expect::Session);
            self.entered = true;
        }
        all.extend(bytes);
        self.expect.push_back(expect);
        Ok(Request { bytes: all, reply })
    }

    // [@ANCHOR: radioctl_barrett4100_engine_feed]
    fn feed(&mut self, bytes: &[u8], now: Duration) -> Vec<Inbound> {
        let mut out = Vec::new();
        if let Some(stale) = self.rx.push(bytes, now) {
            out.push(Inbound::Unrecognised(stale));
        }
        while let Some(raw) = self.rx.take_line(XON) {
            let text = String::from_utf8_lossy(unframe(&raw)).trim().to_string();
            match self.expect.pop_front() {
                Some(Expect::Session) => {}
                Some(Expect::Writes(n)) => {
                    if !text.starts_with('s') {
                        // The refusal ends the operation; the answers still owed to it go.
                        out.push(Inbound::Nak);
                    } else if n > 1 {
                        self.expect.push_front(Expect::Writes(n - 1));
                    } else {
                        out.push(Inbound::Ack);
                    }
                }
                Some(Expect::Frequency) => {
                    match text
                        .strip_prefix("gRF")
                        .and_then(|d| parse_decimal(d.as_bytes()))
                    {
                        Some(hz) => {
                            out.push(Inbound::Observation(Observation::Frequency(Vfo::A, Hz(hz))))
                        }
                        None => out.push(Inbound::Unrecognised(raw)),
                    }
                }
                None => out.push(Inbound::Unrecognised(raw)),
            }
        }
        out
    }

    // [@ANCHOR: radioctl_barrett4100_engine_link_restored]
    fn link_restored(&mut self) -> Vec<Request> {
        self.rx.take_all();
        self.expect.clear();
        if !self.entered {
            return Vec::new();
        }
        // The radio left remote mode with the old link: enter it again, once.
        self.expect.push_back(Expect::Session);
        vec![Request {
            bytes: command(ENTER),
            reply: ReplyKind::None,
        }]
    }
}

/// A virtual Barrett 4100.
#[derive(Debug)]
pub struct Barrett4100Twin {
    rx: RxBuffer,
    rx_hz: u64,
    tx_hz: u64,
    keyed: bool,
    remote: bool,
}

impl Barrett4100Twin {
    // [@ANCHOR: radioctl_barrett4100_twin_new]
    /// A twin in the description's default state.
    pub fn new(desc: &RigDescription) -> Result<Barrett4100Twin, RigError> {
        let (_, name) = vendor_of(desc)?;
        if name != "barrett4100" {
            return Err(unsupported(
                "unknown Barrett 4100 dialect in the description",
            ));
        }
        Ok(Barrett4100Twin {
            rx: RxBuffer::default(),
            rx_hz: desc.default_frequency.0,
            tx_hz: desc.default_frequency.0,
            keyed: false,
            remote: false,
        })
    }

    // [@ANCHOR: radioctl_barrett4100_twin_answer]
    /// The answer text for one command line, if the radio gives one.
    fn answer(&mut self, text: &str) -> Option<String> {
        if text == ENTER {
            self.remote = true;
            return Some("s".to_string());
        }
        if text == "M:REMOTE SENTER0" {
            self.remote = false;
            return Some("s".to_string());
        }
        let body = text.strip_prefix("M:FF ")?;
        if !self.remote {
            return None;
        }
        if body == "GRF" {
            return Some(format!("gRF{}", self.rx_hz));
        }
        if body == "GRPTT" {
            return Some(format!("gRPTT{}", u8::from(self.keyed)));
        }
        if let Some(rest) = body.strip_prefix("SRF") {
            let hz = parse_decimal(rest.strip_suffix(" GRF")?.as_bytes())?;
            self.rx_hz = hz;
            return Some(format!("s gRF{hz}"));
        }
        if let Some(rest) = body.strip_prefix("STF") {
            let hz = parse_decimal(rest.strip_suffix(" GTF")?.as_bytes())?;
            self.tx_hz = hz;
            return Some(format!("s gTF{hz}"));
        }
        if let Some(rest) = body.strip_prefix("SRPTT") {
            let flag = parse_decimal(rest.strip_suffix(" GRPTT")?.as_bytes())?;
            self.keyed = flag != 0;
            return Some(format!("s gRPTT{}", u8::from(self.keyed)));
        }
        None
    }
}

impl VendorTwin for Barrett4100Twin {
    // [@ANCHOR: radioctl_barrett4100_twin_snapshot]
    fn snapshot(&self) -> Option<TwinSnapshot> {
        Some(TwinSnapshot {
            frequency: Hz(self.rx_hz),
            mode_code: None,
            ptt: self.keyed,
        })
    }

    // [@ANCHOR: radioctl_barrett4100_twin_feed]
    fn feed(&mut self, bytes: &[u8], now: Duration) -> Vec<u8> {
        self.rx.push(bytes, now);
        let mut out = Vec::new();
        while let Some(line) = self.rx.take_line(CR) {
            let text = String::from_utf8_lossy(&line)
                .trim_start_matches('\n')
                .to_string();
            if let Some(answer) = self.answer(&text) {
                out.extend(frame(&answer));
            }
        }
        out
    }

    // [@ANCHOR: radioctl_barrett4100_twin_front_panel]
    fn front_panel(&mut self, panel: Panel) -> Result<Vec<u8>, String> {
        match panel {
            Panel::Frequency(hz) => {
                self.rx_hz = hz.0;
                self.tx_hz = hz.0;
            }
            Panel::Ptt(on) => self.keyed = on,
            other => return Err(format!("a Barrett 4100 twin has no {other:?} knob")),
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
            "{}/../../rigs/barrett-4100.toml",
            env!("CARGO_MANIFEST_DIR")
        );
        let text = std::fs::read_to_string(path).expect("rig file");
        radioctl_core::rigfile::parse(&text)
            .expect("loads")
            .description
    }

    // Tests [@ANCHOR: radioctl_barrett4100_command]
    // Tests [@ANCHOR: radioctl_barrett4100_engine_new]
    #[test]
    fn commands_have_a_line_feed_in_front_and_other_families_are_refused() {
        assert_eq!(command("M:FF GRF"), b"\nM:FF GRF\r");
        assert!(Barrett4100Engine::new(RigDescription::ts2000()).is_err());
        assert!(Barrett4100Twin::new(&RigDescription::ts2000()).is_err());
    }

    // Tests [@ANCHOR: radioctl_barrett4100_engine_encode]
    // Tests [@ANCHOR: radioctl_barrett4100_engine_feed]
    // Tests [@ANCHOR: radioctl_barrett4100_engine_link_restored]
    // Tests [@ANCHOR: radioctl_barrett4100_twin_new]
    // Tests [@ANCHOR: radioctl_barrett4100_twin_answer]
    // Tests [@ANCHOR: radioctl_barrett4100_twin_feed]
    // Tests [@ANCHOR: radioctl_barrett4100_twin_snapshot]
    // Tests [@ANCHOR: radioctl_barrett4100_twin_front_panel]
    #[test]
    fn the_engine_and_the_twin_agree_and_the_session_is_entered_once() {
        let desc = shipped();
        let mut e = Barrett4100Engine::new(desc.clone()).expect("engine");
        let mut t = Barrett4100Twin::new(&desc).expect("twin");
        // The first command carries the remote-mode command; later ones do not.
        let r = e
            .encode(&Op::SetFrequency(Vfo::A, Hz(14_200_000)))
            .expect("encodes");
        assert_eq!(r.reply, ReplyKind::Ack);
        assert_eq!(
            r.bytes,
            b"\nM:REMOTE SENTER2,1\r\nM:FF SRF14200000 GRF\r\nM:FF STF14200000 GTF\r"
        );
        let answers = t.feed(&r.bytes, T0);
        // The Ack comes with the second write answer, not before.
        let events = e.feed(&answers, T0);
        assert_eq!(events, vec![Inbound::Ack]);
        assert_eq!(t.snapshot().expect("snapshot").frequency, Hz(14_200_000));
        let r = e.encode(&Op::GetFrequency(Vfo::A)).expect("encodes");
        assert_eq!(r.bytes, b"\nM:FF GRF\r");
        let answer = t.feed(&r.bytes, T0);
        assert_eq!(
            e.feed(&answer, T0),
            vec![Inbound::Observation(Observation::Frequency(
                Vfo::A,
                Hz(14_200_000)
            ))]
        );
        let r = e.encode(&Op::SetPtt(true)).expect("keys");
        assert_eq!(r.bytes, b"\nM:FF SRPTT1 GRPTT\r");
        assert_eq!(e.feed(&t.feed(&r.bytes, T0), T0), vec![Inbound::Ack]);
        assert!(t.snapshot().expect("snapshot").ptt);
        assert_eq!(t.feed(b"\nM:FF GRPTT\r", T0), frame("gRPTT1"));
        let r = e.encode(&Op::SetPtt(false)).expect("unkeys");
        assert_eq!(e.feed(&t.feed(&r.bytes, T0), T0), vec![Inbound::Ack]);
        assert!(!t.snapshot().expect("snapshot").ptt);
        // The radio leaves remote mode with the link: one command re-enters it, once; a command
        // before that is ignored by the twin.
        t.feed(b"\nM:REMOTE SENTER0\r", T0);
        assert!(t.feed(b"\nM:FF GRF\r", T0).is_empty());
        let again = e.link_restored();
        assert_eq!(again.len(), 1);
        assert_eq!(again[0].bytes, b"\nM:REMOTE SENTER2,1\r");
        assert!(e.feed(&t.feed(&again[0].bytes, T0), T0).is_empty());
        let r = e.encode(&Op::GetFrequency(Vfo::A)).expect("encodes");
        assert_eq!(r.bytes, b"\nM:FF GRF\r", "not entered a second time");
        assert_eq!(e.feed(&t.feed(&r.bytes, T0), T0).len(), 1);
        // A refusal and stray answers; refusals before sending.
        let r = e.encode(&Op::SetPtt(true)).expect("keys");
        assert!(!r.bytes.is_empty());
        assert_eq!(e.feed(&frame("e"), T0), vec![Inbound::Nak]);
        assert!(matches!(
            e.feed(&frame("s"), T0).as_slice(),
            [Inbound::Unrecognised(_)]
        ));
        assert!(e.encode(&Op::SetFrequency(Vfo::A, Hz(5_000))).is_err());
        assert!(e.encode(&Op::SetFrequency(Vfo::A, Hz(31_000_000))).is_err());
        assert!(e.encode(&Op::GetMode).is_err());
        assert!(e.encode(&Op::GetPtt).is_err());
        assert!(Barrett4100Engine::new(desc.clone())
            .expect("fresh")
            .link_restored()
            .is_empty());
        t.front_panel(Panel::Frequency(Hz(7_100_000)))
            .expect("tunes");
        t.front_panel(Panel::Ptt(false)).expect("ptt");
        assert!(t.front_panel(Panel::Signal(3)).is_err());
    }
}
