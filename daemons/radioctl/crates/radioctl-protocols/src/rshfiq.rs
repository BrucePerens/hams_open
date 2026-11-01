// SPDX-License-Identifier: AGPL-3.0-or-later
//! HobbyPCB RS-HFIQ transceiver: sans-IO client engine and virtual twin.
//!
//! **Sources.** Facts only, read from Hamlib 4.6.2 (released tarball, sha256
//! b2ac73f44dd1161e95fdee6c95276144757647bf92d7fdb369ee2fe41ed47ae8): `rigs/kit/rs_hfiq.c`. No
//! HobbyPCB document was read; nothing is verified against a radio.
//!
//! Commands start with `*` and end with CR. `*f<Hz>` sets the frequency (no reply); `*f?` asks
//! for it, answered with the digits and CR LF. `*x1` and `*x0` key and unkey (no reply). `*L`
//! asks for the transmit power in percent of nominal (0 to 110, firmware 4 and later), answered
//! the same way. The radio has no mode (Hamlib reports IQ).

use std::time::Duration;

use radioctl_core::description::RigDescription;
use radioctl_core::engine::{Inbound, ProtocolEngine, ReplyKind, Request};
use radioctl_core::error::{ErrorKind, RigError};
use radioctl_core::op::{Field, Observation, Op};
use radioctl_core::transcript::Panel;
use radioctl_core::units::{Hz, Vfo};

use crate::serial_vendor::{parse_decimal, vendor_of, RxBuffer, VendorTwin};

fn unsupported(what: &str) -> RigError {
    RigError::new(ErrorKind::Unsupported, what.to_string())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pending {
    Nothing,
    Frequency,
    Power,
}

/// The RS-HFIQ client engine.
#[derive(Debug)]
pub struct RshfiqEngine {
    desc: RigDescription,
    pending: Pending,
    rx: RxBuffer,
}

impl RshfiqEngine {
    // [@ANCHOR: radioctl_rshfiq_engine_new]
    /// Builds an engine from an RS-HFIQ vendor description.
    pub fn new(desc: RigDescription) -> Result<RshfiqEngine, RigError> {
        let (_, dialect) = vendor_of(&desc)?;
        if dialect != "rshfiq" {
            return Err(unsupported("unknown RS-HFIQ dialect in the description"));
        }
        Ok(RshfiqEngine {
            desc,
            pending: Pending::Nothing,
            rx: RxBuffer::default(),
        })
    }
}

impl ProtocolEngine for RshfiqEngine {
    fn description(&self) -> &RigDescription {
        &self.desc
    }

    // [@ANCHOR: radioctl_rshfiq_engine_encode]
    fn encode(&mut self, op: &Op) -> Result<Request, RigError> {
        if !self.desc.capabilities.has(op.required_capability()) {
            return Err(unsupported("capability not in the rig description"));
        }
        let (text, reply, pending) = match op {
            Op::GetFrequency(Vfo::A) => (
                "*f?\r".to_string(),
                ReplyKind::Observation(Field::Frequency(Vfo::A)),
                Pending::Frequency,
            ),
            Op::SetFrequency(Vfo::A, hz) => {
                (format!("*f{}\r", hz.0), ReplyKind::None, Pending::Nothing)
            }
            Op::SetPtt(on) => (
                format!("*x{}\r", u8::from(*on)),
                ReplyKind::None,
                Pending::Nothing,
            ),
            Op::GetPower => (
                "*L\r".to_string(),
                ReplyKind::Observation(Field::Power),
                Pending::Power,
            ),
            _ => return Err(unsupported("not implemented for the RS-HFIQ engine")),
        };
        self.pending = pending;
        self.rx.take_all();
        Ok(Request {
            bytes: text.into_bytes(),
            reply,
        })
    }

    // [@ANCHOR: radioctl_rshfiq_engine_feed]
    fn feed(&mut self, bytes: &[u8], now: Duration) -> Vec<Inbound> {
        let mut out = Vec::new();
        if let Some(stale) = self.rx.push(bytes, now) {
            out.push(Inbound::Unrecognised(stale));
        }
        while let Some(mut line) = self.rx.take_line(b'\n') {
            if line.last() == Some(&b'\r') {
                line.pop();
            }
            if line.is_empty() {
                continue;
            }
            let pending = std::mem::replace(&mut self.pending, Pending::Nothing);
            let number = parse_decimal(&line);
            out.push(match (pending, number) {
                (Pending::Frequency, Some(hz)) => {
                    Inbound::Observation(Observation::Frequency(Vfo::A, Hz(hz)))
                }
                (Pending::Power, Some(percent)) => match u16::try_from(percent) {
                    Ok(p) if p <= 110 => Inbound::Observation(Observation::Power(p)),
                    _ => Inbound::Unrecognised(line),
                },
                _ => Inbound::Unrecognised(line),
            });
        }
        out
    }
}

/// A virtual RS-HFIQ.
#[derive(Debug)]
pub struct RshfiqTwin {
    rx: RxBuffer,
    hz: Hz,
    ptt: bool,
    percent: u16,
}

impl RshfiqTwin {
    // [@ANCHOR: radioctl_rshfiq_twin_new]
    /// A twin in the description's default state.
    pub fn new(desc: &RigDescription) -> Result<RshfiqTwin, RigError> {
        vendor_of(desc)?;
        Ok(RshfiqTwin {
            rx: RxBuffer::default(),
            hz: desc.default_frequency,
            ptt: false,
            percent: 0,
        })
    }
}

impl VendorTwin for RshfiqTwin {
    // [@ANCHOR: radioctl_rshfiq_twin_feed]
    fn feed(&mut self, bytes: &[u8], now: Duration) -> Vec<u8> {
        self.rx.push(bytes, now);
        let mut out = Vec::new();
        while let Some(line) = self.rx.take_line(b'\r') {
            match line.as_slice() {
                b"*f?" => out.extend(format!("{}\r\n", self.hz.0).into_bytes()),
                b"*L" => out.extend(format!("{}\r\n", self.percent).into_bytes()),
                b"*x1" => {
                    self.ptt = true;
                    self.percent = 100;
                }
                b"*x0" => {
                    self.ptt = false;
                    self.percent = 0;
                }
                [b'*', b'f', digits @ ..] => {
                    if let Some(hz) = parse_decimal(digits) {
                        self.hz = Hz(hz);
                    }
                }
                _ => {}
            }
        }
        out
    }

    // [@ANCHOR: radioctl_rshfiq_twin_front_panel]
    fn front_panel(&mut self, panel: Panel) -> Result<Vec<u8>, String> {
        match panel {
            Panel::Frequency(hz) => self.hz = hz,
            other => return Err(format!("an RS-HFIQ twin has no {other:?} knob")),
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
            "{}/../../rigs/hobbypcb-rs-hfiq.toml",
            env!("CARGO_MANIFEST_DIR")
        );
        let text = std::fs::read_to_string(path).expect("rig file");
        radioctl_core::rigfile::parse(&text)
            .expect("loads")
            .description
    }

    // Tests [@ANCHOR: radioctl_rshfiq_engine_new]
    #[test]
    fn engine_refuses_other_families() {
        assert!(RshfiqEngine::new(RigDescription::ts2000()).is_err());
        assert!(RshfiqTwin::new(&RigDescription::ts2000()).is_err());
    }

    // Tests [@ANCHOR: radioctl_rshfiq_engine_encode]
    // Tests [@ANCHOR: radioctl_rshfiq_engine_feed]
    // Tests [@ANCHOR: radioctl_rshfiq_twin_new]
    // Tests [@ANCHOR: radioctl_rshfiq_twin_feed]
    // Tests [@ANCHOR: radioctl_rshfiq_twin_front_panel]
    #[test]
    fn the_engine_round_trips_against_the_twin() {
        let desc = shipped();
        let mut e = RshfiqEngine::new(desc.clone()).expect("engine");
        let mut t = RshfiqTwin::new(&desc).expect("twin");
        let run = |e: &mut RshfiqEngine, t: &mut RshfiqTwin, op: Op| {
            let r = e.encode(&op).expect("encodes");
            let a = t.feed(&r.bytes, T0);
            (r.bytes, e.feed(&a, T0))
        };
        let (bytes, seen) = run(&mut e, &mut t, Op::SetFrequency(Vfo::A, Hz(14_200_000)));
        assert_eq!(bytes, b"*f14200000\r");
        assert!(seen.is_empty());
        assert_eq!(
            run(&mut e, &mut t, Op::GetFrequency(Vfo::A)).1,
            vec![Inbound::Observation(Observation::Frequency(
                Vfo::A,
                Hz(14_200_000)
            ))]
        );
        assert_eq!(run(&mut e, &mut t, Op::SetPtt(true)).0, b"*x1\r");
        assert_eq!(
            run(&mut e, &mut t, Op::GetPower).1,
            vec![Inbound::Observation(Observation::Power(100))]
        );
        assert_eq!(run(&mut e, &mut t, Op::SetPtt(false)).0, b"*x0\r");
        assert_eq!(
            run(&mut e, &mut t, Op::GetPower).1,
            vec![Inbound::Observation(Observation::Power(0))]
        );
        assert!(e.encode(&Op::GetMode).is_err());
        t.front_panel(Panel::Frequency(Hz(7_000_000)))
            .expect("tunes");
        assert_eq!(t.feed(b"*f?\r", T0), b"7000000\r\n");
        assert!(t.front_panel(Panel::Ptt(true)).is_err());
        e.encode(&Op::GetFrequency(Vfo::A)).expect("get");
        assert!(matches!(
            e.feed(b"abc\r\n", T0).as_slice(),
            [Inbound::Unrecognised(_)]
        ));
        e.encode(&Op::GetPower).expect("get");
        assert!(matches!(
            e.feed(b"900\r\n", T0).as_slice(),
            [Inbound::Unrecognised(_)]
        ));
    }
}
