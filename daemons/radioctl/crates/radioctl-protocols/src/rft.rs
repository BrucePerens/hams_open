// SPDX-License-Identifier: AGPL-3.0-or-later
//! RFT EKD-500 receiver: sans-IO client engine and virtual twin.
//!
//! **Sources.** Facts only, read from Hamlib 4.6.2 (released tarball, sha256
//! b2ac73f44dd1161e95fdee6c95276144757647bf92d7fdb369ee2fe41ed47ae8): `rigs/rft/rft.c` and
//! `ekd500.c`. No RFT document was read; nothing is verified against a radio. Hamlib's EKD-500
//! caps block is commented out in the source, so the model is listed by the matrix but the
//! backend never registers it; the command below is all the backend has.
//!
//! The only command is `FRQ<kHz>` CR with the frequency in kilohertz as a decimal number with
//! six decimals. The receiver may answer with a line; Hamlib waits for it and accepts a timeout.
//! There is no read command, no mode command and no PTT.

use std::time::Duration;

use radioctl_core::description::RigDescription;
use radioctl_core::engine::{Inbound, ProtocolEngine, ReplyKind, Request};
use radioctl_core::error::{ErrorKind, RigError};
use radioctl_core::op::Op;
use radioctl_core::transcript::Panel;
use radioctl_core::units::{Hz, Vfo};

use crate::serial_vendor::{parse_decimal, vendor_of, RxBuffer, VendorTwin};

fn unsupported(what: &str) -> RigError {
    RigError::new(ErrorKind::Unsupported, what.to_string())
}

// [@ANCHOR: radioctl_rft_format_khz]
/// A frequency as kilohertz with exactly six decimals (`14200.000000`).
pub fn format_khz(hz: Hz) -> String {
    format!("{}.{:03}000", hz.0 / 1000, hz.0 % 1000)
}

// [@ANCHOR: radioctl_rft_parse_khz]
/// The frequency of a kilohertz decimal (at most three decimals are significant), or `None` when
/// it is malformed or carries a fraction of a hertz.
pub fn parse_khz(text: &[u8]) -> Option<Hz> {
    let dot = text.iter().position(|b| *b == b'.');
    let (whole, frac) = match dot {
        Some(i) => (text.get(..i)?, text.get(i + 1..)?),
        None => (text, &b""[..]),
    };
    let w = parse_decimal(whole)?;
    let mut hz_part = 0u64;
    for (i, digit) in frac.iter().enumerate() {
        if !digit.is_ascii_digit() {
            return None;
        }
        let d = u64::from(digit - b'0');
        match i {
            0 => hz_part += d * 100,
            1 => hz_part += d * 10,
            2 => hz_part += d,
            _ if d != 0 => return None,
            _ => {}
        }
    }
    w.checked_mul(1000)?.checked_add(hz_part).map(Hz)
}

/// The EKD-500 client engine.
#[derive(Debug)]
pub struct RftEngine {
    desc: RigDescription,
    rx: RxBuffer,
}

impl RftEngine {
    // [@ANCHOR: radioctl_rft_engine_new]
    /// Builds an engine from an RFT vendor description.
    pub fn new(desc: RigDescription) -> Result<RftEngine, RigError> {
        let (_, dialect) = vendor_of(&desc)?;
        if dialect != "ekd500" {
            return Err(unsupported("unknown RFT dialect in the description"));
        }
        Ok(RftEngine {
            desc,
            rx: RxBuffer::default(),
        })
    }
}

impl ProtocolEngine for RftEngine {
    fn description(&self) -> &RigDescription {
        &self.desc
    }

    // [@ANCHOR: radioctl_rft_engine_encode]
    fn encode(&mut self, op: &Op) -> Result<Request, RigError> {
        if !self.desc.capabilities.has(op.required_capability()) {
            return Err(unsupported("capability not in the rig description"));
        }
        match op {
            Op::SetFrequency(Vfo::A, hz) => {
                self.rx.take_all();
                Ok(Request {
                    bytes: format!("FRQ{}\r", format_khz(*hz)).into_bytes(),
                    reply: ReplyKind::None,
                })
            }
            _ => Err(unsupported("not implemented for the RFT engine")),
        }
    }

    // [@ANCHOR: radioctl_rft_engine_feed]
    fn feed(&mut self, bytes: &[u8], now: Duration) -> Vec<Inbound> {
        let mut out = Vec::new();
        if let Some(stale) = self.rx.push(bytes, now) {
            out.push(Inbound::Unrecognised(stale));
        }
        while self.rx.take_line(b'\r').is_some() {
            out.push(Inbound::Ack);
        }
        out
    }
}

/// A virtual RFT EKD-500. It accepts the frequency command and says nothing.
#[derive(Debug)]
pub struct RftTwin {
    rx: RxBuffer,
    hz: Hz,
}

impl RftTwin {
    // [@ANCHOR: radioctl_rft_twin_new]
    /// A twin in the description's default state.
    pub fn new(desc: &RigDescription) -> Result<RftTwin, RigError> {
        vendor_of(desc)?;
        Ok(RftTwin {
            rx: RxBuffer::default(),
            hz: desc.default_frequency,
        })
    }

    // [@ANCHOR: radioctl_rft_twin_frequency]
    /// The frequency the twin is tuned to.
    pub fn frequency(&self) -> Hz {
        self.hz
    }
}

impl VendorTwin for RftTwin {
    // [@ANCHOR: radioctl_rft_twin_feed]
    fn feed(&mut self, bytes: &[u8], now: Duration) -> Vec<u8> {
        self.rx.push(bytes, now);
        while let Some(line) = self.rx.take_line(b'\r') {
            if let Some(hz) = line.strip_prefix(b"FRQ").and_then(parse_khz) {
                self.hz = hz;
            }
        }
        Vec::new()
    }

    // [@ANCHOR: radioctl_rft_twin_front_panel]
    fn front_panel(&mut self, panel: Panel) -> Result<Vec<u8>, String> {
        match panel {
            Panel::Frequency(hz) => self.hz = hz,
            other => return Err(format!("an EKD-500 twin has no {other:?} knob")),
        }
        Ok(Vec::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const T0: Duration = Duration::from_secs(1);

    fn shipped() -> RigDescription {
        let path = format!("{}/../../rigs/rft-ekd-500.toml", env!("CARGO_MANIFEST_DIR"));
        let text = std::fs::read_to_string(path).expect("rig file");
        radioctl_core::rigfile::parse(&text)
            .expect("loads")
            .description
    }

    // Tests [@ANCHOR: radioctl_rft_format_khz]
    // Tests [@ANCHOR: radioctl_rft_parse_khz]
    #[test]
    fn kilohertz_decimals_round_trip() {
        for hz in [14_200_000, 10_000, 29_999_999, 1, 7_074_123] {
            assert_eq!(parse_khz(format_khz(Hz(hz)).as_bytes()), Some(Hz(hz)));
        }
        assert_eq!(format_khz(Hz(14_200_000)), "14200.000000");
        assert_eq!(format_khz(Hz(7_074_123)), "7074.123000");
        assert_eq!(parse_khz(b"14200.0000001"), None);
        assert_eq!(parse_khz(b"1x"), None);
        assert_eq!(parse_khz(b"7074.5"), Some(Hz(7_074_500)));
        assert_eq!(parse_khz(b"7074"), Some(Hz(7_074_000)));
    }

    // Tests [@ANCHOR: radioctl_rft_engine_new]
    #[test]
    fn engine_refuses_other_families() {
        assert!(RftEngine::new(RigDescription::ts2000()).is_err());
        assert!(RftTwin::new(&RigDescription::ts2000()).is_err());
    }

    // Tests [@ANCHOR: radioctl_rft_engine_encode]
    // Tests [@ANCHOR: radioctl_rft_engine_feed]
    // Tests [@ANCHOR: radioctl_rft_twin_new]
    // Tests [@ANCHOR: radioctl_rft_twin_frequency]
    // Tests [@ANCHOR: radioctl_rft_twin_feed]
    // Tests [@ANCHOR: radioctl_rft_twin_front_panel]
    #[test]
    fn the_engine_tunes_the_twin() {
        let desc = shipped();
        let mut e = RftEngine::new(desc.clone()).expect("engine");
        let mut t = RftTwin::new(&desc).expect("twin");
        let r = e
            .encode(&Op::SetFrequency(Vfo::A, Hz(14_200_000)))
            .expect("encodes");
        assert_eq!(r.bytes, b"FRQ14200.000000\r");
        assert_eq!(r.reply, ReplyKind::None);
        assert!(t.feed(&r.bytes, T0).is_empty());
        assert_eq!(t.frequency(), Hz(14_200_000));
        assert!(e.encode(&Op::GetFrequency(Vfo::A)).is_err());
        assert!(e.encode(&Op::GetMode).is_err());
        assert!(e.encode(&Op::SetPtt(true)).is_err());
        assert_eq!(e.feed(b"OK\r", T0), vec![Inbound::Ack]);
        t.front_panel(Panel::Frequency(Hz(5_000_000)))
            .expect("tunes");
        assert_eq!(t.frequency(), Hz(5_000_000));
        assert!(t.front_panel(Panel::Ptt(true)).is_err());
    }
}
