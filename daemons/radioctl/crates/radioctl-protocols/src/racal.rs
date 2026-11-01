// SPDX-License-Identifier: AGPL-3.0-or-later
//! Racal RA6790/GM receiver: sans-IO client engine and virtual twin.
//!
//! **Sources.** Facts only, read from Hamlib 4.6.2 (released tarball, sha256
//! b2ac73f44dd1161e95fdee6c95276144757647bf92d7fdb369ee2fe41ed47ae8): `rigs/racal/racal.c` and
//! `ra6790.c`. No Racal document was read; nothing is verified against a radio. The RA3702
//! (`ra37xx.c`, its own packet framing with a receiver number in a header) is not implemented.
//!
//! Commands are `$`, the receiver number (0 by default in Hamlib), the command text and CR.
//! `F<MHz>` sets the frequency in megahertz as a decimal number; `TF` asks for it, answered
//! `F<MHz>` CR. `D<n>` sets the mode (1 AM, 2 FM, 3 CW with variable BFO, 4 CW, 5 ISB, 6 LSB,
//! 7 USB); `TDI` asks, answered `D<n>I<kHz bandwidth>` CR. Settings get no reply.

use std::time::Duration;

use radioctl_core::description::RigDescription;
use radioctl_core::engine::{Inbound, ProtocolEngine, ReplyKind, Request};
use radioctl_core::error::{ErrorKind, RigError};
use radioctl_core::op::{Field, Observation, Op};
use radioctl_core::transcript::Panel;
use radioctl_core::units::{Hz, Vfo};

use crate::serial_vendor::{parse_decimal, vendor_of, RxBuffer, VendorTwin};

const CR: u8 = b'\r';

fn unsupported(what: &str) -> RigError {
    RigError::new(ErrorKind::Unsupported, what.to_string())
}

// [@ANCHOR: radioctl_racal_format_mhz]
/// A frequency as a megahertz decimal with up to six decimals and no trailing zeros (`14.2`).
pub fn format_mhz(hz: Hz) -> String {
    let whole = hz.0 / 1_000_000;
    let frac = hz.0 % 1_000_000;
    if frac == 0 {
        return whole.to_string();
    }
    let digits = format!("{frac:06}");
    format!("{whole}.{}", digits.trim_end_matches('0'))
}

// [@ANCHOR: radioctl_racal_parse_mhz]
/// The frequency of a megahertz decimal (`14.2`, `7`, `0.5`), or `None`.
pub fn parse_mhz(text: &[u8]) -> Option<Hz> {
    let (whole, frac) = match text.iter().position(|b| *b == b'.') {
        Some(i) => (text.get(..i)?, text.get(i + 1..)?),
        None => (text, &b""[..]),
    };
    if frac.len() > 6 {
        return None;
    }
    let w = parse_decimal(whole)?;
    let f = if frac.is_empty() {
        0
    } else {
        parse_decimal(frac)? * 10u64.pow(6 - u32::try_from(frac.len()).ok()?)
    };
    w.checked_mul(1_000_000)?.checked_add(f).map(Hz)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pending {
    Nothing,
    Frequency,
    Mode,
}

/// The Racal client engine.
#[derive(Debug)]
pub struct RacalEngine {
    desc: RigDescription,
    pending: Pending,
    rx: RxBuffer,
}

impl RacalEngine {
    // [@ANCHOR: radioctl_racal_engine_new]
    /// Builds an engine from a Racal vendor description.
    pub fn new(desc: RigDescription) -> Result<RacalEngine, RigError> {
        let (_, dialect) = vendor_of(&desc)?;
        if dialect != "ra6790" {
            return Err(unsupported("unknown Racal dialect in the description"));
        }
        Ok(RacalEngine {
            desc,
            pending: Pending::Nothing,
            rx: RxBuffer::default(),
        })
    }
}

impl ProtocolEngine for RacalEngine {
    fn description(&self) -> &RigDescription {
        &self.desc
    }

    // [@ANCHOR: radioctl_racal_engine_encode]
    fn encode(&mut self, op: &Op) -> Result<Request, RigError> {
        if !self.desc.capabilities.has(op.required_capability()) {
            return Err(unsupported("capability not in the rig description"));
        }
        let (text, reply, pending) = match op {
            Op::GetFrequency(Vfo::A) => (
                "$0TF\r".to_string(),
                ReplyKind::Observation(Field::Frequency(Vfo::A)),
                Pending::Frequency,
            ),
            Op::SetFrequency(Vfo::A, hz) => (
                format!("$0F{}\r", format_mhz(*hz)),
                ReplyKind::None,
                Pending::Nothing,
            ),
            Op::GetMode => (
                "$0TDI\r".to_string(),
                ReplyKind::Observation(Field::Mode),
                Pending::Mode,
            ),
            Op::SetMode(mode) => {
                let code = self
                    .desc
                    .mode_code(*mode)
                    .ok_or_else(|| unsupported("mode not in the description"))?;
                (
                    format!("$0D{}\r", char::from(code)),
                    ReplyKind::None,
                    Pending::Nothing,
                )
            }
            _ => return Err(unsupported("not implemented for the Racal engine")),
        };
        self.pending = pending;
        self.rx.take_all();
        Ok(Request {
            bytes: text.into_bytes(),
            reply,
        })
    }

    // [@ANCHOR: radioctl_racal_engine_feed]
    fn feed(&mut self, bytes: &[u8], now: Duration) -> Vec<Inbound> {
        let mut out = Vec::new();
        if let Some(stale) = self.rx.push(bytes, now) {
            out.push(Inbound::Unrecognised(stale));
        }
        while let Some(line) = self.rx.take_line(CR) {
            let pending = std::mem::replace(&mut self.pending, Pending::Nothing);
            out.extend(match (pending, line.as_slice()) {
                (Pending::Frequency, [b'F', rest @ ..]) => match parse_mhz(rest) {
                    Some(hz) => vec![Inbound::Observation(Observation::Frequency(Vfo::A, hz))],
                    None => vec![Inbound::Unrecognised(line.clone())],
                },
                (Pending::Mode, [b'D', code, b'I', width @ ..]) if !width.is_empty() => {
                    // Both CW codes (3 variable BFO, 4 BFO centre) read as CW.
                    let code = if *code == b'3' { b'4' } else { *code };
                    match self.desc.code_mode(code) {
                        Some(m) => vec![Inbound::Observation(Observation::Mode(m))],
                        None => vec![Inbound::Unrecognised(line.clone())],
                    }
                }
                _ => vec![Inbound::Unrecognised(line.clone())],
            });
        }
        out
    }
}

/// A virtual Racal RA6790.
#[derive(Debug)]
pub struct RacalTwin {
    rx: RxBuffer,
    hz: Hz,
    mode: u8,
    codes: Vec<u8>,
}

impl RacalTwin {
    // [@ANCHOR: radioctl_racal_twin_new]
    /// A twin in the description's default state.
    pub fn new(desc: &RigDescription) -> Result<RacalTwin, RigError> {
        vendor_of(desc)?;
        Ok(RacalTwin {
            rx: RxBuffer::default(),
            hz: desc.default_frequency,
            mode: desc.mode_code(desc.default_mode).unwrap_or(b'1'),
            codes: desc.mode_codes.iter().map(|(_, c)| *c).collect(),
        })
    }
}

impl VendorTwin for RacalTwin {
    // [@ANCHOR: radioctl_racal_twin_feed]
    fn feed(&mut self, bytes: &[u8], now: Duration) -> Vec<u8> {
        self.rx.push(bytes, now);
        let mut out = Vec::new();
        while let Some(line) = self.rx.take_line(CR) {
            match line.as_slice() {
                b"$0TF" => out.extend(format!("F{}\r", format_mhz(self.hz)).into_bytes()),
                b"$0TDI" => out.extend(format!("D{}I3\r", char::from(self.mode)).into_bytes()),
                [b'$', b'0', b'F', rest @ ..] => {
                    if let Some(hz) = parse_mhz(rest) {
                        self.hz = hz;
                    }
                }
                [b'$', b'0', b'D', c] if self.codes.contains(c) => self.mode = *c,
                _ => {}
            }
        }
        out
    }

    // [@ANCHOR: radioctl_racal_twin_front_panel]
    fn front_panel(&mut self, panel: Panel) -> Result<Vec<u8>, String> {
        match panel {
            Panel::Frequency(hz) => self.hz = hz,
            other => return Err(format!("a Racal twin has no {other:?} knob")),
        }
        Ok(Vec::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use radioctl_core::units::Mode;

    const T0: Duration = Duration::from_secs(1);

    fn shipped() -> RigDescription {
        let path = format!(
            "{}/../../rigs/racal-ra6790-gm.toml",
            env!("CARGO_MANIFEST_DIR")
        );
        let text = std::fs::read_to_string(path).expect("rig file");
        radioctl_core::rigfile::parse(&text)
            .expect("loads")
            .description
    }

    // Tests [@ANCHOR: radioctl_racal_format_mhz]
    // Tests [@ANCHOR: radioctl_racal_parse_mhz]
    #[test]
    fn megahertz_decimals_round_trip() {
        for hz in [14_200_000, 7_074_000, 500_000, 29_999_999, 1_000_000, 10] {
            assert_eq!(
                parse_mhz(format_mhz(Hz(hz)).as_bytes()),
                Some(Hz(hz)),
                "{hz}"
            );
        }
        assert_eq!(format_mhz(Hz(14_200_000)), "14.2");
        assert_eq!(format_mhz(Hz(7_000_000)), "7");
        assert_eq!(format_mhz(Hz(500_000)), "0.5");
        assert_eq!(parse_mhz(b"14.2000001"), None);
        assert_eq!(parse_mhz(b"1x"), None);
        assert_eq!(parse_mhz(b"."), None);
    }

    // Tests [@ANCHOR: radioctl_racal_engine_new]
    #[test]
    fn engine_refuses_other_families() {
        assert!(RacalEngine::new(RigDescription::ts2000()).is_err());
        assert!(RacalTwin::new(&RigDescription::ts2000()).is_err());
    }

    // Tests [@ANCHOR: radioctl_racal_engine_encode]
    // Tests [@ANCHOR: radioctl_racal_engine_feed]
    // Tests [@ANCHOR: radioctl_racal_twin_new]
    // Tests [@ANCHOR: radioctl_racal_twin_feed]
    // Tests [@ANCHOR: radioctl_racal_twin_front_panel]
    #[test]
    fn the_engine_round_trips_against_the_twin() {
        let desc = shipped();
        let mut e = RacalEngine::new(desc.clone()).expect("engine");
        let mut t = RacalTwin::new(&desc).expect("twin");
        let run = |e: &mut RacalEngine, t: &mut RacalTwin, op: Op| {
            let r = e.encode(&op).expect("encodes");
            let a = t.feed(&r.bytes, T0);
            (r.bytes, e.feed(&a, T0))
        };
        let (bytes, seen) = run(&mut e, &mut t, Op::SetFrequency(Vfo::A, Hz(14_200_000)));
        assert_eq!(bytes, b"$0F14.2\r");
        assert!(seen.is_empty());
        assert_eq!(
            run(&mut e, &mut t, Op::GetFrequency(Vfo::A)).1,
            vec![Inbound::Observation(Observation::Frequency(
                Vfo::A,
                Hz(14_200_000)
            ))]
        );
        for mode in [Mode::Am, Mode::Fm, Mode::Cw, Mode::Lsb, Mode::Usb] {
            run(&mut e, &mut t, Op::SetMode(mode));
            assert_eq!(
                run(&mut e, &mut t, Op::GetMode).1,
                vec![Inbound::Observation(Observation::Mode(mode))],
                "{mode:?}"
            );
        }
        assert!(e.encode(&Op::SetPtt(true)).is_err());
        assert!(e.encode(&Op::SetMode(Mode::Wfm)).is_err());
        t.front_panel(Panel::Frequency(Hz(5_000_000)))
            .expect("tunes");
        assert_eq!(t.feed(b"$0TF\r", T0), b"F5\r");
        assert!(t.front_panel(Panel::Ptt(true)).is_err());
        e.encode(&Op::GetFrequency(Vfo::A)).expect("get");
        assert!(matches!(
            e.feed(b"Fabc\r", T0).as_slice(),
            [Inbound::Unrecognised(_)]
        ));
        e.encode(&Op::GetMode).expect("get");
        assert!(matches!(
            e.feed(b"D9I3\r", T0).as_slice(),
            [Inbound::Unrecognised(_)]
        ));
    }
}
