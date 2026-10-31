// SPDX-License-Identifier: AGPL-3.0-or-later
//! The `hpsdr-network` engine: the radioctl side of an OpenHPSDR (ANAN, Hermes) radio's CONTROL plane.
//!
//! An OpenHPSDR radio has no command stream of the kind the actor drives: control is UDP datagrams
//! and receive I/Q is a separate data plane. So the engine speaks a small line protocol (`GF`, `F <hz>`,
//! `GP`, `P 0|1`, `GW`, `GR`, `GA`, `A <db>`; replies `OK`, `ERR <text>`, `F <hz>`, `P <0|1>`, `W <raw>`, `R <tenths>`, `A <db>`)
//! to the bridge in [`crate::hpsdr_io`], which owns the `hpsdr` crate session. Frequency, PTT, forward
//! power, SWR and the step attenuator (`Level::Attenuator`, whole decibels as the description's
//! `levels.attenuator_any_up_to_db` allows) are claimed; there is no mode (an SDR radio's
//! demodulation happens in the host) and no preamplifier.
//!
//! Transmit: `Op::SetPtt(true)` is the only command that can key, and the rig actor refuses it
//! unless its transmit gate is open before it reaches [`HpsdrEngine::encode`]. The bridge then opens
//! the `hpsdr` crate's own gate for a bounded lease. Everything is `verified=unverified`.

use std::time::Duration;

use radioctl_core::description::{Family, RigDescription};
use radioctl_core::engine::{Inbound, ProtocolEngine, ReplyKind, Request};
use radioctl_core::error::{ErrorKind, RigError};
use radioctl_core::op::{Field, Observation, Op};
use radioctl_core::units::{Hz, Level, Vfo};

use crate::attenuation;

/// Longest line accepted from the bridge.
const MAX_LINE: usize = 128;

/// The engine for one OpenHPSDR radio.
#[derive(Debug)]
pub struct HpsdrEngine {
    desc: RigDescription,
    line: Vec<u8>,
    discarding: bool,
}

impl HpsdrEngine {
    // [@ANCHOR: radioctl_hpsdr_engine_new]
    /// Builds an engine for an `hpsdr-network` description.
    pub fn new(desc: RigDescription) -> Result<HpsdrEngine, RigError> {
        if desc.family != Family::Hpsdr || desc.hpsdr.is_none() {
            return Err(RigError::new(
                ErrorKind::Unsupported,
                "not an OpenHPSDR description",
            ));
        }
        Ok(HpsdrEngine {
            desc,
            line: Vec::new(),
            discarding: false,
        })
    }
}

// [@ANCHOR: radioctl_hpsdr_engine_parse_line]
/// One complete line from the bridge.
fn parse_line(line: &str) -> Inbound {
    let mut parts = line.split_whitespace();
    let value = |p: Option<&str>| p.and_then(|v| v.parse::<u64>().ok());
    match (parts.next(), value(parts.next())) {
        (Some("OK"), _) => Inbound::Ack,
        (Some("ERR"), _) => Inbound::Nak,
        (Some("F"), Some(hz)) => Inbound::Observation(Observation::Frequency(Vfo::A, Hz(hz))),
        (Some("P"), Some(v)) => Inbound::Observation(Observation::Ptt(v != 0)),
        (Some("W"), Some(v)) => {
            Inbound::Observation(Observation::Power(u16::try_from(v).unwrap_or(u16::MAX)))
        }
        (Some("R"), Some(v)) => {
            Inbound::Observation(Observation::Swr(u16::try_from(v).unwrap_or(u16::MAX)))
        }
        (Some("A"), Some(v)) => u16::try_from(v).map_or_else(
            |_| Inbound::Unrecognised(line.as_bytes().to_vec()),
            |db| Inbound::Observation(Observation::Level(Level::Attenuator, db)),
        ),
        _ => Inbound::Unrecognised(line.as_bytes().to_vec()),
    }
}

impl ProtocolEngine for HpsdrEngine {
    fn description(&self) -> &RigDescription {
        &self.desc
    }

    // [@ANCHOR: radioctl_hpsdr_engine_encode]
    fn encode(&mut self, op: &Op) -> Result<Request, RigError> {
        let (text, reply) = match op {
            Op::GetFrequency(Vfo::A) => (
                "GF".to_string(),
                ReplyKind::Observation(Field::Frequency(Vfo::A)),
            ),
            Op::SetFrequency(Vfo::A, Hz(hz)) if *hz <= u64::from(u32::MAX) => {
                (format!("F {hz}"), ReplyKind::Ack)
            }
            Op::GetPtt => ("GP".to_string(), ReplyKind::Observation(Field::Ptt)),
            Op::SetPtt(on) => (format!("P {}", u8::from(*on)), ReplyKind::Ack),
            Op::GetPower => ("GW".to_string(), ReplyKind::Observation(Field::Power)),
            Op::GetSwr => ("GR".to_string(), ReplyKind::Observation(Field::Swr)),
            Op::GetLevel(Level::Attenuator) => {
                attenuation::step_of(&self.desc, Level::Attenuator, 0)?;
                (
                    "GA".to_string(),
                    ReplyKind::Observation(Field::Level(Level::Attenuator)),
                )
            }
            Op::SetLevel(Level::Attenuator, db) => {
                attenuation::step_of(&self.desc, Level::Attenuator, *db)?;
                (format!("A {db}"), ReplyKind::Ack)
            }
            _ => {
                return Err(RigError::new(
                    ErrorKind::Unsupported,
                    "the OpenHPSDR control plane does not support this command",
                ))
            }
        };
        let mut bytes = text.into_bytes();
        bytes.push(b'\n');
        Ok(Request { bytes, reply })
    }

    // [@ANCHOR: radioctl_hpsdr_engine_feed]
    fn feed(&mut self, bytes: &[u8], _now: Duration) -> Vec<Inbound> {
        let mut out = Vec::new();
        for &b in bytes {
            if b == b'\n' {
                if !self.discarding {
                    if let Ok(text) = std::str::from_utf8(&self.line) {
                        out.push(parse_line(text.trim()));
                    } else {
                        out.push(Inbound::Unrecognised(self.line.clone()));
                    }
                }
                self.line.clear();
                self.discarding = false;
            } else if self.line.len() >= MAX_LINE {
                self.discarding = true;
                self.line.clear();
            } else if !self.discarding {
                self.line.push(b);
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use radioctl_core::rigfile;

    fn engine() -> HpsdrEngine {
        let loaded = rigfile::parse(include_str!("../../../rigs/anan-100.toml")).expect("file");
        HpsdrEngine::new(loaded.description).expect("engine")
    }

    #[test]
    fn commands_encode_and_unsupported_ones_are_refused() {
        // Tests [@ANCHOR: radioctl_hpsdr_engine_new]
        // Tests [@ANCHOR: radioctl_hpsdr_engine_encode]
        let mut e = engine();
        assert_eq!(
            e.encode(&Op::SetFrequency(Vfo::A, Hz(7_100_000)))
                .expect("encodes")
                .bytes,
            b"F 7100000\n"
        );
        assert_eq!(
            e.encode(&Op::SetPtt(false)).expect("encodes").bytes,
            b"P 0\n"
        );
        assert_eq!(
            e.encode(&Op::GetMode).expect_err("no mode").kind,
            ErrorKind::Unsupported
        );
        assert!(e.encode(&Op::SetFrequency(Vfo::B, Hz(1))).is_err());
        assert_eq!(
            e.encode(&Op::SetLevel(Level::Attenuator, 12))
                .expect("encodes")
                .bytes,
            b"A 12\n"
        );
        assert_eq!(
            e.encode(&Op::GetLevel(Level::Attenuator))
                .expect("encodes")
                .bytes,
            b"GA\n"
        );
        assert!(e.encode(&Op::SetLevel(Level::Attenuator, 32)).is_err());
        assert!(e.encode(&Op::SetLevel(Level::Preamp, 1)).is_err());
        assert!(e.encode(&Op::SetFrequency(Vfo::A, Hz(u64::MAX))).is_err());
        let other = rigfile::parse(include_str!("../../../rigs/icom-ic-7300.toml")).expect("file");
        assert!(HpsdrEngine::new(other.description).is_err());
    }

    #[test]
    fn replies_and_unsolicited_lines_decode_across_chunks() {
        // Tests [@ANCHOR: radioctl_hpsdr_engine_feed]
        // Tests [@ANCHOR: radioctl_hpsdr_engine_parse_line]
        let mut e = engine();
        assert!(e.feed(b"F 71000", Duration::ZERO).is_empty());
        let got = e.feed(
            b"00\nOK\nERR gate\nW 123\nR 15\nP 1\nA 12\nA 70000\nwhat\n",
            Duration::ZERO,
        );
        assert_eq!(
            got,
            vec![
                Inbound::Observation(Observation::Frequency(Vfo::A, Hz(7_100_000))),
                Inbound::Ack,
                Inbound::Nak,
                Inbound::Observation(Observation::Power(123)),
                Inbound::Observation(Observation::Swr(15)),
                Inbound::Observation(Observation::Ptt(true)),
                Inbound::Observation(Observation::Level(Level::Attenuator, 12)),
                Inbound::Unrecognised(b"A 70000".to_vec()),
                Inbound::Unrecognised(b"what".to_vec()),
            ]
        );
        // An over-long line is dropped whole.
        let mut junk = vec![b'x'; 300];
        junk.push(b'\n');
        assert!(e.feed(&junk, Duration::ZERO).is_empty());
        assert_eq!(e.feed(b"OK\n", Duration::ZERO), vec![Inbound::Ack]);
    }
}
