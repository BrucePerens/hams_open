// SPDX-License-Identifier: AGPL-3.0-or-later
//! MDS 4710 and 9710 data radios: sans-IO client engine and virtual twin.
//!
//! **Sources.** Facts only, read from Hamlib 4.6.2 (released tarball, sha256
//! b2ac73f44dd1161e95fdee6c95276144757647bf92d7fdb369ee2fe41ed47ae8): `rigs/mds/mds.c`, `4710.c`
//! and `9710.c`. No MDS document was read (the technical manual is on ManualsLib, not fetched);
//! nothing is verified against a radio. Hamlib's MDS driver shares the shape of its Barrett
//! driver, and it has no mode commands (they are compiled out).
//!
//! Commands are text ended by LF; answers, when Hamlib reads one, end with CR.
//!
//! * `TX` and `RX` and a frequency in megahertz with four decimals set the transmit and receive
//!   frequencies; Hamlib sends both for one frequency and reads no answer. `TX` alone asks the
//!   transmit frequency, answered with a megahertz number.
//! * Hamlib's open asks `MODEL`, `SER`, `SREV` and `SHOW DC`, then sends `MODEM NONE` and `PTT 0`;
//!   the twin answers the questions with short text and takes the others silently.
//! * `KEY` and `DKEY` key and unkey; Hamlib's code reads no answer and then compares the missing
//!   answer with `OK`, which crashes it, so the description has no PTT.

use std::time::Duration;

use radioctl_core::description::RigDescription;
use radioctl_core::engine::{Inbound, ProtocolEngine, ReplyKind, Request};
use radioctl_core::error::{ErrorKind, RigError};
use radioctl_core::op::{Field, Observation, Op};
use radioctl_core::transcript::Panel;
use radioctl_core::units::{Hz, Vfo};

use crate::serial_vendor::{vendor_of, RxBuffer, TwinSnapshot, VendorTwin};

const CR: u8 = b'\r';
const LF: u8 = b'\n';

fn unsupported(what: &str) -> RigError {
    RigError::new(ErrorKind::Unsupported, what.to_string())
}

// [@ANCHOR: radioctl_mds_mhz_text]
/// A frequency as megahertz with four decimals (`450.0000`), rounded to 100 Hz.
pub fn mhz_text(hz: Hz) -> String {
    let units = (hz.0 + 50) / 100;
    format!("{}.{:04}", units / 10_000, units % 10_000)
}

// [@ANCHOR: radioctl_mds_parse_mhz]
/// The frequency of a megahertz number with up to four decimals, or `None`.
pub fn parse_mhz(text: &str) -> Option<Hz> {
    let text = text.trim();
    let (whole, frac) = text.split_once('.').unwrap_or((text, ""));
    if whole.is_empty()
        || frac.len() > 4
        || !whole
            .bytes()
            .chain(frac.bytes())
            .all(|b| b.is_ascii_digit())
    {
        return None;
    }
    let w: u64 = whole.parse().ok()?;
    let f: u64 = if frac.is_empty() {
        0
    } else {
        frac.parse::<u64>().ok()? * 10u64.pow(4 - u32::try_from(frac.len()).ok()?)
    };
    w.checked_mul(1_000_000)?.checked_add(f * 100).map(Hz)
}

/// The MDS client engine.
#[derive(Debug)]
pub struct MdsEngine {
    desc: RigDescription,
    waiting: bool,
    rx: RxBuffer,
}

impl MdsEngine {
    // [@ANCHOR: radioctl_mds_engine_new]
    /// Builds an engine from an MDS vendor description.
    pub fn new(desc: RigDescription) -> Result<MdsEngine, RigError> {
        let (_, name) = vendor_of(&desc)?;
        if name != "mds" {
            return Err(unsupported("unknown MDS dialect in the description"));
        }
        Ok(MdsEngine {
            desc,
            waiting: false,
            rx: RxBuffer::default(),
        })
    }
}

impl ProtocolEngine for MdsEngine {
    fn description(&self) -> &RigDescription {
        &self.desc
    }

    // [@ANCHOR: radioctl_mds_engine_encode]
    fn encode(&mut self, op: &Op) -> Result<Request, RigError> {
        if !self.desc.capabilities.has(op.required_capability()) {
            return Err(unsupported("capability not in the rig description"));
        }
        let (text, reply) = match op {
            Op::GetFrequency(Vfo::A) => {
                self.waiting = true;
                (
                    "TX\n".to_string(),
                    ReplyKind::Observation(Field::Frequency(Vfo::A)),
                )
            }
            Op::SetFrequency(Vfo::A, hz) => {
                self.waiting = false;
                let t = mhz_text(*hz);
                (format!("TX{t}\nRX{t}\n"), ReplyKind::None)
            }
            _ => return Err(unsupported("not implemented for the MDS engine")),
        };
        self.rx.take_all();
        Ok(Request {
            bytes: text.into_bytes(),
            reply,
        })
    }

    // [@ANCHOR: radioctl_mds_engine_feed]
    fn feed(&mut self, bytes: &[u8], now: Duration) -> Vec<Inbound> {
        let mut out = Vec::new();
        if let Some(stale) = self.rx.push(bytes, now) {
            out.push(Inbound::Unrecognised(stale));
        }
        while let Some(line) = self.rx.take_line(CR) {
            let text = String::from_utf8_lossy(&line).into_owned();
            let waiting = std::mem::replace(&mut self.waiting, false);
            out.push(match (waiting, parse_mhz(&text)) {
                (true, Some(hz)) => Inbound::Observation(Observation::Frequency(Vfo::A, hz)),
                _ => Inbound::Unrecognised(line),
            });
        }
        out
    }
}

/// A virtual MDS radio.
#[derive(Debug)]
pub struct MdsTwin {
    rx: RxBuffer,
    tx_hz: Hz,
    rx_hz: Hz,
}

impl MdsTwin {
    // [@ANCHOR: radioctl_mds_twin_new]
    /// A twin in the description's default state.
    pub fn new(desc: &RigDescription) -> Result<MdsTwin, RigError> {
        let (_, name) = vendor_of(desc)?;
        if name != "mds" {
            return Err(unsupported("unknown MDS dialect in the description"));
        }
        Ok(MdsTwin {
            rx: RxBuffer::default(),
            tx_hz: desc.default_frequency,
            rx_hz: desc.default_frequency,
        })
    }
}

impl VendorTwin for MdsTwin {
    // [@ANCHOR: radioctl_mds_twin_snapshot]
    fn snapshot(&self) -> Option<TwinSnapshot> {
        Some(TwinSnapshot {
            frequency: self.rx_hz,
            mode_code: None,
            ptt: false,
        })
    }

    // [@ANCHOR: radioctl_mds_twin_feed]
    fn feed(&mut self, bytes: &[u8], now: Duration) -> Vec<u8> {
        self.rx.push(bytes, now);
        let mut out = Vec::new();
        while let Some(line) = self.rx.take_line(LF) {
            let text = String::from_utf8_lossy(&line).into_owned();
            match text.as_str() {
                "TX" => out.extend(format!("{}\r", mhz_text(self.tx_hz)).into_bytes()),
                "MODEL" => out.extend(b"MDS 4710\r"),
                "SER" => out.extend(b"000000\r"),
                "SREV" => out.extend(b"1.0\r"),
                "SHOW DC" => out.extend(b"DC OFF\r"),
                t => {
                    if let Some(hz) = t.strip_prefix("TX").and_then(parse_mhz) {
                        self.tx_hz = hz;
                    } else if let Some(hz) = t.strip_prefix("RX").and_then(parse_mhz) {
                        self.rx_hz = hz;
                    }
                }
            }
        }
        out
    }

    // [@ANCHOR: radioctl_mds_twin_front_panel]
    fn front_panel(&mut self, panel: Panel) -> Result<Vec<u8>, String> {
        match panel {
            Panel::Frequency(hz) => {
                self.tx_hz = hz;
                self.rx_hz = hz;
            }
            other => return Err(format!("an MDS twin has no {other:?} knob")),
        }
        Ok(Vec::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const T0: Duration = Duration::from_secs(1);

    fn shipped(id: &str) -> RigDescription {
        let path = format!("{}/../../rigs/{id}.toml", env!("CARGO_MANIFEST_DIR"));
        let text = std::fs::read_to_string(path).expect("rig file");
        radioctl_core::rigfile::parse(&text)
            .expect("loads")
            .description
    }

    // Tests [@ANCHOR: radioctl_mds_mhz_text]
    // Tests [@ANCHOR: radioctl_mds_parse_mhz]
    #[test]
    fn megahertz_numbers_round_trip() {
        for hz in [450_000_000, 406_125_000, 928_000_100, 380_000_000] {
            assert_eq!(parse_mhz(&mhz_text(Hz(hz))), Some(Hz(hz)), "{hz}");
        }
        assert_eq!(mhz_text(Hz(450_000_000)), "450.0000");
        assert_eq!(mhz_text(Hz(450_000_049)), "450.0000");
        assert_eq!(mhz_text(Hz(450_000_050)), "450.0001");
        assert_eq!(parse_mhz("450"), Some(Hz(450_000_000)));
        assert_eq!(parse_mhz("450.1"), Some(Hz(450_100_000)));
        assert_eq!(parse_mhz("450.12345"), None);
        assert_eq!(parse_mhz("x"), None);
        assert_eq!(parse_mhz(".5"), None);
    }

    // Tests [@ANCHOR: radioctl_mds_engine_new]
    #[test]
    fn other_families_are_refused() {
        assert!(MdsEngine::new(RigDescription::ts2000()).is_err());
        assert!(MdsTwin::new(&RigDescription::ts2000()).is_err());
    }

    // Tests [@ANCHOR: radioctl_mds_engine_encode]
    // Tests [@ANCHOR: radioctl_mds_engine_feed]
    // Tests [@ANCHOR: radioctl_mds_twin_new]
    // Tests [@ANCHOR: radioctl_mds_twin_feed]
    // Tests [@ANCHOR: radioctl_mds_twin_snapshot]
    // Tests [@ANCHOR: radioctl_mds_twin_front_panel]
    #[test]
    fn both_models_set_and_read_the_frequency() {
        for id in ["mds-4710", "mds-9710"] {
            let desc = shipped(id);
            let mut e = MdsEngine::new(desc.clone()).expect("engine");
            let mut t = MdsTwin::new(&desc).expect("twin");
            let r = e
                .encode(&Op::SetFrequency(Vfo::A, Hz(450_125_000)))
                .expect("encodes");
            assert_eq!(r.bytes, b"TX450.1250\nRX450.1250\n", "{id}");
            assert_eq!(r.reply, ReplyKind::None);
            assert!(t.feed(&r.bytes, T0).is_empty());
            assert_eq!(t.snapshot().expect("snapshot").frequency, Hz(450_125_000));
            let r = e.encode(&Op::GetFrequency(Vfo::A)).expect("encodes");
            let answer = t.feed(&r.bytes, T0);
            assert_eq!(answer, b"450.1250\r");
            assert_eq!(
                e.feed(&answer, T0),
                vec![Inbound::Observation(Observation::Frequency(
                    Vfo::A,
                    Hz(450_125_000)
                ))]
            );
            // Hamlib's open questions and silent settings.
            assert_eq!(t.feed(b"MODEL\n", T0), b"MDS 4710\r");
            assert_eq!(t.feed(b"SER\n", T0), b"000000\r");
            assert_eq!(t.feed(b"SREV\n", T0), b"1.0\r");
            assert_eq!(t.feed(b"SHOW DC\n", T0), b"DC OFF\r");
            assert!(t.feed(b"MODEM NONE\nPTT 0\n", T0).is_empty());
            t.front_panel(Panel::Frequency(Hz(406_000_000)))
                .expect("tunes");
            assert_eq!(t.feed(b"TX\n", T0), b"406.0000\r");
            assert!(t.front_panel(Panel::Ptt(true)).is_err());
            assert!(e.encode(&Op::SetPtt(true)).is_err());
            e.encode(&Op::SetFrequency(Vfo::A, Hz(450_000_000)))
                .expect("encodes");
            assert!(matches!(
                e.feed(b"450.0000\r", T0).as_slice(),
                [Inbound::Unrecognised(_)]
            ));
        }
    }
}
