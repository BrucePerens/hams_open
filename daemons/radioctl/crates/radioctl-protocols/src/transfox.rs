// SPDX-License-Identifier: AGPL-3.0-or-later
//! SigFox Transfox transverter controller: sans-IO client engine and virtual twin.
//!
//! **Sources.** Facts only, read from Hamlib 4.6.2 (released tarball, sha256
//! b2ac73f44dd1161e95fdee6c95276144757647bf92d7fdb369ee2fe41ed47ae8): `rigs/kenwood/transfox.c`
//! and the Kenwood routines it uses (`kenwood.c`: `kenwood_set_freq`, `kenwood_get_freq`,
//! `kenwood_set_ptt`). No SigFox document was read (none was found); nothing is verified
//! against a radio.
//!
//! The Transfox speaks the Kenwood framing (commands end with `;`) without the `ID` handshake.
//! `FA` and eleven digits set the frequency in hertz, `FA;` asks it (answer `FA` and eleven
//! digits); `TX;` and `RX;` key and unkey; `Cs;` asks the transmit state (Hamlib reads a two
//! character answer and calls the transmitter keyed when the first is `T`; this engine and twin
//! use `TX` and `RX`, an assumption). Not in the engine: the attenuator and preamplifier (`C2x`
//! and `C3x` read, `C20` to `C22` and `C30` to `C32` set, two stages each) and the firmware
//! string (`CS`), whose answer format is not known.

use std::time::Duration;

use radioctl_core::description::RigDescription;
use radioctl_core::engine::{Inbound, ProtocolEngine, ReplyKind, Request};
use radioctl_core::error::{ErrorKind, RigError};
use radioctl_core::op::{Field, Observation, Op};
use radioctl_core::transcript::Panel;
use radioctl_core::units::{Hz, Vfo};

use crate::serial_vendor::{parse_decimal, vendor_of, RxBuffer, TwinSnapshot, VendorTwin};

/// Lowest frequency, hertz (Hamlib's receive range).
pub const MIN_HZ: u64 = 1_000_000;
/// Highest frequency, hertz (Hamlib's receive range).
pub const MAX_HZ: u64 = 1_450_000_000;

fn unsupported(what: &str) -> RigError {
    RigError::new(ErrorKind::Unsupported, what.to_string())
}

// [@ANCHOR: radioctl_transfox_decode]
/// What one answer (without its `;`) says: `FA` and eleven digits, or `TX` / `RX`.
pub fn decode(text: &str) -> Option<Observation> {
    if let Some(digits) = text.strip_prefix("FA") {
        if digits.len() == 11 {
            return Some(Observation::Frequency(
                Vfo::A,
                Hz(parse_decimal(digits.as_bytes())?),
            ));
        }
        return None;
    }
    match text {
        "TX" => Some(Observation::Ptt(true)),
        "RX" => Some(Observation::Ptt(false)),
        _ => None,
    }
}

/// The Transfox client engine.
#[derive(Debug)]
pub struct TransfoxEngine {
    desc: RigDescription,
    rx: RxBuffer,
}

impl TransfoxEngine {
    // [@ANCHOR: radioctl_transfox_engine_new]
    /// Builds an engine from a Transfox vendor description.
    pub fn new(desc: RigDescription) -> Result<TransfoxEngine, RigError> {
        let (_, name) = vendor_of(&desc)?;
        if name != "transfox" {
            return Err(unsupported("unknown Transfox dialect in the description"));
        }
        Ok(TransfoxEngine {
            desc,
            rx: RxBuffer::default(),
        })
    }
}

impl ProtocolEngine for TransfoxEngine {
    fn description(&self) -> &RigDescription {
        &self.desc
    }

    // [@ANCHOR: radioctl_transfox_engine_encode]
    fn encode(&mut self, op: &Op) -> Result<Request, RigError> {
        if !self.desc.capabilities.has(op.required_capability()) {
            return Err(unsupported("capability not in the rig description"));
        }
        let (text, reply) = match op {
            Op::GetFrequency(Vfo::A) => (
                "FA;".to_string(),
                ReplyKind::Observation(Field::Frequency(Vfo::A)),
            ),
            Op::SetFrequency(Vfo::A, hz) => {
                if hz.0 < MIN_HZ || hz.0 > MAX_HZ {
                    return Err(unsupported("frequency outside 1 to 1450 MHz"));
                }
                (format!("FA{:011};", hz.0), ReplyKind::None)
            }
            Op::GetPtt => ("Cs;".to_string(), ReplyKind::Observation(Field::Ptt)),
            Op::SetPtt(on) => (if *on { "TX;" } else { "RX;" }.to_string(), ReplyKind::None),
            _ => return Err(unsupported("not implemented for the Transfox engine")),
        };
        self.rx.take_all();
        Ok(Request {
            bytes: text.into_bytes(),
            reply,
        })
    }

    // [@ANCHOR: radioctl_transfox_engine_feed]
    fn feed(&mut self, bytes: &[u8], now: Duration) -> Vec<Inbound> {
        let mut out = Vec::new();
        if let Some(stale) = self.rx.push(bytes, now) {
            out.push(Inbound::Unrecognised(stale));
        }
        while let Some(line) = self.rx.take_line(b';') {
            let text = String::from_utf8_lossy(&line).trim().to_string();
            if text.is_empty() {
                continue;
            }
            out.push(match decode(&text) {
                Some(observation) => Inbound::Observation(observation),
                None => Inbound::Unrecognised(line),
            });
        }
        out
    }
}

/// A virtual Transfox.
#[derive(Debug)]
pub struct TransfoxTwin {
    rx: RxBuffer,
    hz: u64,
    keyed: bool,
}

impl TransfoxTwin {
    // [@ANCHOR: radioctl_transfox_twin_new]
    /// A twin in the description's default state.
    pub fn new(desc: &RigDescription) -> Result<TransfoxTwin, RigError> {
        let (_, name) = vendor_of(desc)?;
        if name != "transfox" {
            return Err(unsupported("unknown Transfox dialect in the description"));
        }
        Ok(TransfoxTwin {
            rx: RxBuffer::default(),
            hz: desc.default_frequency.0,
            keyed: false,
        })
    }
}

impl VendorTwin for TransfoxTwin {
    // [@ANCHOR: radioctl_transfox_twin_snapshot]
    fn snapshot(&self) -> Option<TwinSnapshot> {
        Some(TwinSnapshot {
            frequency: Hz(self.hz),
            mode_code: None,
            ptt: self.keyed,
        })
    }

    // [@ANCHOR: radioctl_transfox_twin_feed]
    fn feed(&mut self, bytes: &[u8], now: Duration) -> Vec<u8> {
        self.rx.push(bytes, now);
        let mut out = Vec::new();
        while let Some(line) = self.rx.take_line(b';') {
            let text = String::from_utf8_lossy(&line).trim().to_string();
            match text.as_str() {
                "FA" => out.extend(format!("FA{:011};", self.hz).into_bytes()),
                "Cs" => out.extend(if self.keyed { b"TX;" } else { b"RX;" }),
                "TX" | "TX0" | "TX1" => self.keyed = true,
                "RX" => self.keyed = false,
                t => {
                    if let Some(hz) = t
                        .strip_prefix("FA")
                        .and_then(|d| parse_decimal(d.as_bytes()))
                    {
                        self.hz = hz.clamp(MIN_HZ, MAX_HZ);
                    }
                }
            }
        }
        out
    }

    // [@ANCHOR: radioctl_transfox_twin_front_panel]
    fn front_panel(&mut self, panel: Panel) -> Result<Vec<u8>, String> {
        match panel {
            Panel::Frequency(hz) => self.hz = hz.0.clamp(MIN_HZ, MAX_HZ),
            Panel::Ptt(on) => self.keyed = on,
            other => return Err(format!("a Transfox twin has no {other:?} knob")),
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
            "{}/../../rigs/sigfox-transfox.toml",
            env!("CARGO_MANIFEST_DIR")
        );
        let text = std::fs::read_to_string(path).expect("rig file");
        radioctl_core::rigfile::parse(&text)
            .expect("loads")
            .description
    }

    // Tests [@ANCHOR: radioctl_transfox_decode]
    #[test]
    fn answers_decode() {
        assert_eq!(
            decode("FA00144200000"),
            Some(Observation::Frequency(Vfo::A, Hz(144_200_000)))
        );
        assert_eq!(decode("TX"), Some(Observation::Ptt(true)));
        assert_eq!(decode("RX"), Some(Observation::Ptt(false)));
        assert_eq!(decode("FA1442"), None);
        assert_eq!(decode("FAx0144200000"), None);
        assert_eq!(decode("ID019"), None);
    }

    // Tests [@ANCHOR: radioctl_transfox_engine_new]
    #[test]
    fn other_families_are_refused() {
        assert!(TransfoxEngine::new(RigDescription::ts2000()).is_err());
        assert!(TransfoxTwin::new(&RigDescription::ts2000()).is_err());
    }

    // Tests [@ANCHOR: radioctl_transfox_engine_encode]
    // Tests [@ANCHOR: radioctl_transfox_engine_feed]
    // Tests [@ANCHOR: radioctl_transfox_twin_new]
    // Tests [@ANCHOR: radioctl_transfox_twin_feed]
    // Tests [@ANCHOR: radioctl_transfox_twin_snapshot]
    // Tests [@ANCHOR: radioctl_transfox_twin_front_panel]
    #[test]
    fn the_engine_and_the_twin_agree_on_frequency_and_the_transmit_state() {
        let desc = shipped();
        let mut e = TransfoxEngine::new(desc.clone()).expect("engine");
        let mut t = TransfoxTwin::new(&desc).expect("twin");
        let r = e
            .encode(&Op::SetFrequency(Vfo::A, Hz(144_200_000)))
            .expect("encodes");
        assert_eq!(r.bytes, b"FA00144200000;");
        assert_eq!(r.reply, ReplyKind::None);
        assert!(t.feed(&r.bytes, T0).is_empty());
        assert_eq!(t.snapshot().expect("snapshot").frequency, Hz(144_200_000));
        let r = e.encode(&Op::GetFrequency(Vfo::A)).expect("encodes");
        let answer = t.feed(&r.bytes, T0);
        assert_eq!(answer, b"FA00144200000;");
        assert_eq!(
            e.feed(&answer, T0),
            vec![Inbound::Observation(Observation::Frequency(
                Vfo::A,
                Hz(144_200_000)
            ))]
        );
        let r = e.encode(&Op::SetPtt(true)).expect("encodes");
        assert_eq!(r.bytes, b"TX;");
        assert!(t.feed(&r.bytes, T0).is_empty());
        assert!(t.snapshot().expect("snapshot").ptt);
        let r = e.encode(&Op::GetPtt).expect("encodes");
        let answer = t.feed(&r.bytes, T0);
        assert_eq!(answer, b"TX;");
        assert_eq!(
            e.feed(&answer, T0),
            vec![Inbound::Observation(Observation::Ptt(true))]
        );
        assert_eq!(e.encode(&Op::SetPtt(false)).expect("rx").bytes, b"RX;");
        assert!(t.feed(b"RX;", T0).is_empty());
        assert!(!t.snapshot().expect("snapshot").ptt);
        // Hamlib's other transmit forms, range, other VFO, other ops.
        assert!(t.feed(b"TX1;", T0).is_empty());
        assert!(t.snapshot().expect("snapshot").ptt);
        assert!(e.encode(&Op::SetFrequency(Vfo::A, Hz(999_999))).is_err());
        assert!(e
            .encode(&Op::SetFrequency(Vfo::A, Hz(1_450_000_001)))
            .is_err());
        assert!(e.encode(&Op::GetFrequency(Vfo::B)).is_err());
        assert!(e.encode(&Op::GetMode).is_err());
        assert!(e.feed(b";", T0).is_empty());
        assert!(matches!(
            e.feed(b"ID019;", T0).as_slice(),
            [Inbound::Unrecognised(_)]
        ));
        t.front_panel(Panel::Frequency(Hz(50_000_000)))
            .expect("tunes");
        assert_eq!(t.feed(b"FA;", T0), b"FA00050000000;");
        assert!(t.front_panel(Panel::Signal(3)).is_err());
    }
}
