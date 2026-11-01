// SPDX-License-Identifier: AGPL-3.0-or-later
//! Philips (Simoco) PRM8060 mobile radio: sans-IO client engine (read only) and virtual twin.
//!
//! **Sources.** Facts only, read from Hamlib 4.6.2 (released tarball, sha256
//! b2ac73f44dd1161e95fdee6c95276144757647bf92d7fdb369ee2fe41ed47ae8): `rigs/prm80/prm80.c` and
//! `prm8060.c`. No Philips document was read; nothing is verified against a radio.
//!
//! The radio is a console that prints a `>` prompt. The command `E` answers 22 characters: eleven
//! hex bytes for the mode, channel, channel state, squelch, volume, lock, the receive PLL word
//! (two bytes), the transmit PLL word (two bytes) and the signal strength; the prompt follows.
//! The PLL words count 12.5 kHz steps; the receive word includes the 21.4 MHz IF, added below 300
//! MHz of PLL frequency and subtracted above it. Mode bit 3 means transmitting.
//!
//! **Not implemented: setting.** `R` (set receive and transmit frequencies), `N` and the other
//! setters are dialogues: the radio prints a `:` prompt for each argument and Hamlib waits for it
//! before sending the hex digits. The engine interface sends one request and reads replies; it
//! cannot continue a dialogue from `feed`. Pipelining the arguments could lose characters on the
//! radio's software UART, which is why Hamlib waits; that is an open question for a bench test.

use std::time::Duration;

use radioctl_core::description::RigDescription;
use radioctl_core::engine::{Inbound, ProtocolEngine, ReplyKind, Request};
use radioctl_core::error::{ErrorKind, RigError};
use radioctl_core::op::{Field, Observation, Op};
use radioctl_core::transcript::Panel;
use radioctl_core::units::{Hz, Vfo};

use crate::serial_vendor::{vendor_of, RxBuffer, VendorTwin};

/// Length of the answer to `E`.
const STATE_LEN: usize = 22;
/// Receive IF offset in hertz.
const RX_IF: u64 = 21_400_000;
/// PLL step in hertz.
const STEP: u64 = 12_500;
/// PLL frequency above which the IF is added rather than subtracted.
const IF_SPLIT: u64 = 300_000_000;

fn unsupported(what: &str) -> RigError {
    RigError::new(ErrorKind::Unsupported, what.to_string())
}

/// The decoded state line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct State {
    /// Transmitting.
    pub ptt: bool,
    /// Receive frequency.
    pub rx: Hz,
    /// Transmit frequency.
    pub tx: Hz,
    /// Raw signal strength byte.
    pub rssi: u8,
}

fn hex_byte(text: &[u8]) -> Option<u8> {
    let s = std::str::from_utf8(text).ok()?;
    if s.len() != 2 || !s.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    u8::from_str_radix(s, 16).ok()
}

// [@ANCHOR: radioctl_prm80_rx_from_pll]
/// The receive frequency of a PLL word.
pub fn rx_from_pll(word: u16) -> Hz {
    let pll = u64::from(word) * STEP;
    Hz(if pll > IF_SPLIT {
        pll + RX_IF
    } else {
        pll.saturating_sub(RX_IF)
    })
}

// [@ANCHOR: radioctl_prm80_pll_from_rx]
/// The PLL word that receives `hz`, or `None` when it does not fit sixteen bits.
pub fn pll_from_rx(hz: Hz) -> Option<u16> {
    let pll = if hz.0 > IF_SPLIT {
        hz.0.checked_sub(RX_IF)?
    } else {
        hz.0 + RX_IF
    };
    u16::try_from(pll / STEP).ok()
}

// [@ANCHOR: radioctl_prm80_decode_state]
/// The state of a 22-character `E` answer, or `None` when it is not eleven hex bytes.
pub fn decode_state(line: &[u8]) -> Option<State> {
    if line.len() != STATE_LEN {
        return None;
    }
    let byte = |i: usize| hex_byte(&line[i * 2..i * 2 + 2]);
    let word = |i: usize| Some(u16::from(byte(i)?) << 8 | u16::from(byte(i + 1)?));
    Some(State {
        ptt: byte(0)? & 0x08 != 0,
        rx: rx_from_pll(word(6)?),
        tx: Hz(u64::from(word(8)?) * STEP),
        rssi: byte(10)?,
    })
}

/// The PRM8060 client engine.
#[derive(Debug)]
pub struct Prm80Engine {
    desc: RigDescription,
    pending: Option<Field>,
    rx: RxBuffer,
}

impl Prm80Engine {
    // [@ANCHOR: radioctl_prm80_engine_new]
    /// Builds an engine from a PRM80 vendor description.
    pub fn new(desc: RigDescription) -> Result<Prm80Engine, RigError> {
        let (_, dialect) = vendor_of(&desc)?;
        if dialect != "prm8060" {
            return Err(unsupported("unknown PRM80 dialect in the description"));
        }
        Ok(Prm80Engine {
            desc,
            pending: None,
            rx: RxBuffer::default(),
        })
    }
}

impl ProtocolEngine for Prm80Engine {
    fn description(&self) -> &RigDescription {
        &self.desc
    }

    // [@ANCHOR: radioctl_prm80_engine_encode]
    fn encode(&mut self, op: &Op) -> Result<Request, RigError> {
        if !self.desc.capabilities.has(op.required_capability()) {
            return Err(unsupported("capability not in the rig description"));
        }
        let field = match op {
            Op::GetFrequency(vfo) => Field::Frequency(*vfo),
            Op::GetPtt => Field::Ptt,
            Op::GetSignal => Field::Signal,
            _ => return Err(unsupported("not implemented for the PRM80 engine")),
        };
        self.pending = Some(field);
        self.rx.take_all();
        Ok(Request {
            bytes: b"E".to_vec(),
            reply: ReplyKind::Observation(field),
        })
    }

    // [@ANCHOR: radioctl_prm80_engine_feed]
    fn feed(&mut self, bytes: &[u8], now: Duration) -> Vec<Inbound> {
        let mut out = Vec::new();
        if let Some(stale) = self.rx.push(bytes, now) {
            out.push(Inbound::Unrecognised(stale));
        }
        // A prompt left over from the previous command may come first: drop it and what precedes it.
        while let Some(i) = self
            .rx
            .peek()
            .iter()
            .take(STATE_LEN)
            .position(|b| *b == b'>')
        {
            self.rx.take(i + 1);
        }
        let Some(field) = self.pending else {
            return out;
        };
        let Some(line) = self.rx.take(STATE_LEN) else {
            return out;
        };
        self.pending = None;
        out.push(match (decode_state(&line), field) {
            (Some(s), Field::Frequency(Vfo::A)) => {
                Inbound::Observation(Observation::Frequency(Vfo::A, s.rx))
            }
            (Some(s), Field::Frequency(Vfo::B)) => {
                Inbound::Observation(Observation::Frequency(Vfo::B, s.tx))
            }
            (Some(s), Field::Ptt) => Inbound::Observation(Observation::Ptt(s.ptt)),
            (Some(s), Field::Signal) => Inbound::Observation(Observation::Signal(s.rssi.into())),
            _ => Inbound::Unrecognised(line),
        });
        out
    }
}

/// A virtual PRM8060.
#[derive(Debug)]
pub struct Prm80Twin {
    rx: RxBuffer,
    rx_hz: Hz,
    tx_hz: Hz,
    ptt: bool,
    rssi: u8,
}

impl Prm80Twin {
    // [@ANCHOR: radioctl_prm80_twin_new]
    /// A twin in the description's default state.
    pub fn new(desc: &RigDescription) -> Result<Prm80Twin, RigError> {
        vendor_of(desc)?;
        Ok(Prm80Twin {
            rx: RxBuffer::default(),
            rx_hz: desc.default_frequency,
            tx_hz: desc.default_frequency,
            ptt: false,
            rssi: 0,
        })
    }

    // [@ANCHOR: radioctl_prm80_twin_state_line]
    /// The 22-character `E` answer.
    pub fn state_line(&self) -> String {
        let rx = pll_from_rx(self.rx_hz).unwrap_or(0);
        let tx = u16::try_from(self.tx_hz.0 / STEP).unwrap_or(0);
        format!(
            "{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}{:04X}{:04X}{:02X}",
            if self.ptt { 0x0A } else { 0x02 },
            0,
            0,
            0x0F,
            0x08,
            0,
            rx,
            tx,
            self.rssi
        )
    }
}

impl VendorTwin for Prm80Twin {
    // [@ANCHOR: radioctl_prm80_twin_feed]
    fn feed(&mut self, bytes: &[u8], now: Duration) -> Vec<u8> {
        self.rx.push(bytes, now);
        let mut out = Vec::new();
        for byte in self.rx.take_all() {
            if byte == b'E' {
                out.extend(self.state_line().into_bytes());
                out.push(b'>');
            }
        }
        out
    }

    // [@ANCHOR: radioctl_prm80_twin_front_panel]
    fn front_panel(&mut self, panel: Panel) -> Result<Vec<u8>, String> {
        match panel {
            Panel::Frequency(hz) => {
                self.rx_hz = hz;
                self.tx_hz = hz;
            }
            Panel::Ptt(on) => self.ptt = on,
            Panel::Signal(s) => self.rssi = u8::try_from(s).unwrap_or(u8::MAX),
            other => return Err(format!("a PRM8060 twin has no {other:?} knob")),
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
            "{}/../../rigs/philips-prm8060.toml",
            env!("CARGO_MANIFEST_DIR")
        );
        let text = std::fs::read_to_string(path).expect("rig file");
        radioctl_core::rigfile::parse(&text)
            .expect("loads")
            .description
    }

    // Tests [@ANCHOR: radioctl_prm80_rx_from_pll]
    // Tests [@ANCHOR: radioctl_prm80_pll_from_rx]
    // Tests [@ANCHOR: radioctl_prm80_decode_state]
    #[test]
    fn pll_words_and_state_lines_decode() {
        for hz in [145_000_000u64, 144_000_000, 145_787_500, 438_000_000] {
            let word = pll_from_rx(Hz(hz)).expect("fits");
            assert_eq!(rx_from_pll(word), Hz(hz), "{hz}");
        }
        // 0x3400 steps of 12.5 kHz is 166.4 MHz, 145 MHz plus the IF.
        assert_eq!(rx_from_pll(0x3400), Hz(145_000_000));
        assert_eq!(decode_state(b"12400800FF003400"), None);
        assert_eq!(decode_state(b"0A00000F0800340"), None);
        let s = decode_state(b"0A00000F080034002D5040").expect("decodes");
        assert_eq!(
            s,
            State {
                ptt: true,
                rx: Hz(145_000_000),
                tx: Hz(145_000_000),
                rssi: 0x40
            }
        );
        assert_eq!(decode_state(b"0A00000F08003400ZZ5040"), None);
    }

    // Tests [@ANCHOR: radioctl_prm80_engine_new]
    #[test]
    fn engine_refuses_other_families() {
        assert!(Prm80Engine::new(RigDescription::ts2000()).is_err());
        assert!(Prm80Twin::new(&RigDescription::ts2000()).is_err());
    }

    // Tests [@ANCHOR: radioctl_prm80_engine_encode]
    // Tests [@ANCHOR: radioctl_prm80_engine_feed]
    // Tests [@ANCHOR: radioctl_prm80_twin_new]
    // Tests [@ANCHOR: radioctl_prm80_twin_state_line]
    // Tests [@ANCHOR: radioctl_prm80_twin_feed]
    // Tests [@ANCHOR: radioctl_prm80_twin_front_panel]
    #[test]
    fn the_engine_reads_the_twin() {
        let desc = shipped();
        let mut e = Prm80Engine::new(desc.clone()).expect("engine");
        let mut t = Prm80Twin::new(&desc).expect("twin");
        let run = |e: &mut Prm80Engine, t: &mut Prm80Twin, op: Op| {
            let r = e.encode(&op).expect("encodes");
            assert_eq!(r.bytes, b"E");
            let a = t.feed(&r.bytes, T0);
            e.feed(&a, T0)
        };
        let f = desc.default_frequency;
        assert_eq!(
            run(&mut e, &mut t, Op::GetFrequency(Vfo::A)),
            vec![Inbound::Observation(Observation::Frequency(Vfo::A, f))]
        );
        assert_eq!(
            run(&mut e, &mut t, Op::GetFrequency(Vfo::B)),
            vec![Inbound::Observation(Observation::Frequency(Vfo::B, f))]
        );
        t.front_panel(Panel::Frequency(Hz(145_500_000)))
            .expect("tunes");
        t.front_panel(Panel::Ptt(true)).expect("keys");
        t.front_panel(Panel::Signal(77)).expect("signal");
        assert_eq!(
            run(&mut e, &mut t, Op::GetFrequency(Vfo::A)),
            vec![Inbound::Observation(Observation::Frequency(
                Vfo::A,
                Hz(145_500_000)
            ))]
        );
        assert_eq!(
            run(&mut e, &mut t, Op::GetPtt),
            vec![Inbound::Observation(Observation::Ptt(true))]
        );
        assert_eq!(
            run(&mut e, &mut t, Op::GetSignal),
            vec![Inbound::Observation(Observation::Signal(77))]
        );
        // The setters need the prompt dialogue and are not offered.
        assert!(e
            .encode(&Op::SetFrequency(Vfo::A, Hz(145_000_000)))
            .is_err());
        assert!(e.encode(&Op::SetPtt(true)).is_err());
        assert!(t.front_panel(Panel::Split(true)).is_err());
        // A prompt left over from before is skipped; a half answer waits; garbage is refused.
        e.encode(&Op::GetPtt).expect("get");
        let line = t.state_line();
        let mut late = b">".to_vec();
        late.extend_from_slice(&line.as_bytes()[..10]);
        assert!(e.feed(&late, T0).is_empty());
        assert_eq!(
            e.feed(&line.as_bytes()[10..], T0),
            vec![Inbound::Observation(Observation::Ptt(true))]
        );
        e.encode(&Op::GetPtt).expect("get");
        assert!(matches!(
            e.feed(b"ZZZZZZZZZZZZZZZZZZZZZZ", T0).as_slice(),
            [Inbound::Unrecognised(_)]
        ));
    }
}
