// SPDX-License-Identifier: AGPL-3.0-or-later
//! Rohde and Schwarz receivers with the `FREQ` / `DEM` / `SENS` command set (ESMC, EB200): sans-IO
//! client engine and virtual twin.
//!
//! **Source.** Hamlib 4.6.2 (released source tarball, sha256
//! b2ac73f44dd1161e95fdee6c95276144757647bf92d7fdb369ee2fe41ed47ae8), read for facts only:
//! `rigs/rs/rs.c` (`rs_transaction`, `rs_set_freq`, `rs_get_freq`, `rs_set_mode`, `rs_get_mode`,
//! `rs_get_level`) and `esmc.c`, `eb200.c` (both use these functions unchanged). No code or comment
//! was copied and no Rohde and Schwarz manual was read; every fact is `hamlib-source` and unverified.
//!
//! * Every command is sent as CR, the text and CR; answers end with CR. A setting is not answered.
//! * `FREQ <hertz>` sets and `FREQ?` reads the frequency (an integer).
//! * `DEM <name>` sets and `DEM?` reads the demodulation: `AM`, `FM`, `CW`, `USB`, `LSB`. The
//!   description's codes are radioctl's own numbering (1 AM, 2 FM, 3 CW, 4 USB, 5 LSB).
//! * `SENS:DATA? "VOLT:AC"` reads the level in dBuV (an integer, possibly with a fraction); the
//!   engine reports the integer part, floored at 0, as the raw signal.
//!
//! The EK895/6, XK852 and XK2100 are in other backends of Hamlib (`ek89x.c`, `xk852.c`,
//! `gp2000.c`) and are not covered here. Bandwidth (`BAND`), attenuator, audio volume and squelch
//! are not implemented.

use radioctl_core::description::{ReceiverProtocol, RigDescription};
use radioctl_core::engine::{Inbound, ReplyKind};
use radioctl_core::error::{ErrorKind, RigError};
use radioctl_core::op::{Field, Observation, Op};
use radioctl_core::units::{Hz, Mode, Vfo};

use crate::receiver::{Dialect, LineEngine, ReceiverTwin};
use crate::virtual_rig::RigState;

/// The Rohde and Schwarz engine.
pub type RsEngine = LineEngine<RsDialect>;

// [@ANCHOR: radioctl_rs_engine]
/// The engine for a Rohde and Schwarz description.
pub fn engine(desc: RigDescription) -> Result<RsEngine, RigError> {
    LineEngine::new(desc, ReceiverProtocol::RohdeSchwarz)
}

/// The demodulation names by radioctl's mode code (1 to 5).
const MODES: [(u8, &str, Mode); 5] = [
    (1, "AM", Mode::Am),
    (2, "FM", Mode::Fm),
    (3, "CW", Mode::Cw),
    (4, "USB", Mode::Usb),
    (5, "LSB", Mode::Lsb),
];

/// The `FREQ` / `DEM` command set.
#[derive(Debug, Clone, Copy)]
pub struct RsDialect;

impl Dialect for RsDialect {
    // [@ANCHOR: radioctl_rs_dialect_from_description]
    fn from_description(desc: &RigDescription) -> Result<RsDialect, RigError> {
        match desc.receiver.as_ref().map_or("", |r| r.dialect.as_str()) {
            "rs" => Ok(RsDialect),
            other => Err(RigError::new(
                ErrorKind::Unsupported,
                format!("unknown Rohde and Schwarz dialect {other:?}"),
            )),
        }
    }

    // [@ANCHOR: radioctl_rs_dialect_encode]
    fn encode(&mut self, desc: &RigDescription, op: &Op) -> Result<(Vec<u8>, ReplyKind), RigError> {
        let unsupported = |why: &str| RigError::new(ErrorKind::Unsupported, why.to_string());
        let ask = |text: &str, field| {
            (
                format!("\r{text}\r").into_bytes(),
                ReplyKind::Observation(field),
            )
        };
        Ok(match op {
            Op::SetFrequency(Vfo::A, hz) => {
                (format!("\rFREQ {}\r", hz.0).into_bytes(), ReplyKind::None)
            }
            Op::GetFrequency(Vfo::A) => ask("FREQ?", Field::Frequency(Vfo::A)),
            Op::SetMode(mode) => {
                let code = desc
                    .mode_code(*mode)
                    .ok_or_else(|| unsupported("mode not in the description"))?;
                let name = MODES
                    .iter()
                    .find(|(c, _, _)| *c == code)
                    .map(|(_, n, _)| *n);
                let name = name.ok_or_else(|| unsupported("mode code not a demodulation"))?;
                (format!("\rDEM {name}\r").into_bytes(), ReplyKind::None)
            }
            Op::GetMode => ask("DEM?", Field::Mode),
            Op::GetSignal => ask("SENS:DATA? \"VOLT:AC\"", Field::Signal),
            _ => {
                return Err(unsupported(
                    "not implemented for the Rohde and Schwarz engine",
                ))
            }
        })
    }

    // [@ANCHOR: radioctl_rs_dialect_status]
    fn status(&mut self, _expecting: ReplyKind, line: &str) -> Option<Vec<Inbound>> {
        line.is_empty().then(Vec::new)
    }

    // [@ANCHOR: radioctl_rs_dialect_decode]
    fn decode(&mut self, desc: &RigDescription, expecting: ReplyKind, line: &str) -> Vec<Inbound> {
        let unknown = || vec![Inbound::Unrecognised(line.as_bytes().to_vec())];
        let ReplyKind::Observation(field) = expecting else {
            return unknown();
        };
        let found = match field {
            Field::Frequency(_) => line
                .parse::<u64>()
                .ok()
                .map(|hz| Observation::Frequency(Vfo::A, Hz(hz))),
            Field::Mode => MODES
                .iter()
                .find(|(_, name, _)| *name == line)
                .map(|(_, _, mode)| *mode)
                .filter(|mode| desc.mode_code(*mode).is_some())
                .map(Observation::Mode),
            Field::Signal => line
                .parse::<f64>()
                .ok()
                .filter(|v| v.is_finite())
                .map(|v| Observation::Signal(v.trunc().clamp(0.0, f64::from(u16::MAX)) as u16)),
            _ => None,
        };
        found.map_or_else(unknown, |o| vec![Inbound::Observation(o)])
    }
}

/// The virtual Rohde and Schwarz receiver.
#[derive(Debug)]
pub struct RsTwin {
    desc: RigDescription,
}

impl RsTwin {
    // [@ANCHOR: radioctl_rs_twin_new]
    /// The twin of a Rohde and Schwarz description.
    pub fn new(desc: &RigDescription) -> Result<RsTwin, RigError> {
        RsDialect::from_description(desc)?;
        Ok(RsTwin { desc: desc.clone() })
    }
}

impl ReceiverTwin for RsTwin {
    // [@ANCHOR: radioctl_rs_twin_line]
    fn line(&mut self, state: &mut RigState, line: &str) -> Vec<u8> {
        let reply = |text: String| format!("{text}\r").into_bytes();
        if let Some(hz) = line.strip_prefix("FREQ ") {
            if let Ok(hz) = hz.parse::<u64>() {
                state.freq_a = Hz(hz);
            }
            return Vec::new();
        }
        if let Some(name) = line.strip_prefix("DEM ") {
            if let Some((_, _, mode)) = MODES.iter().find(|(_, n, _)| *n == name) {
                if self.desc.mode_code(*mode).is_some() {
                    state.mode = *mode;
                }
            }
            return Vec::new();
        }
        match line {
            "FREQ?" => reply(state.freq_a.0.to_string()),
            "DEM?" => {
                let name = MODES
                    .iter()
                    .find(|(_, _, m)| *m == state.mode)
                    .map_or("FM", |(_, n, _)| *n);
                reply(name.to_string())
            }
            "SENS:DATA? \"VOLT:AC\"" => reply(state.signal.to_string()),
            _ => Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use radioctl_core::engine::ProtocolEngine;

    use super::*;

    const T0: Duration = Duration::from_secs(1);

    fn shipped(id: &str) -> RigDescription {
        let path = format!("{}/../../rigs/{id}.toml", env!("CARGO_MANIFEST_DIR"));
        let text = std::fs::read_to_string(path).expect("rig file");
        radioctl_core::rigfile::parse(&text)
            .expect("loads")
            .description
    }

    // Tests [@ANCHOR: radioctl_rs_engine]
    // Tests [@ANCHOR: radioctl_rs_dialect_from_description]
    // Tests [@ANCHOR: radioctl_rs_dialect_encode]
    #[test]
    fn commands_are_framed_by_a_leading_and_trailing_cr() {
        let mut e = engine(shipped("rs-eb200")).expect("engine");
        let bytes = |e: &mut RsEngine, op: Op| e.encode(&op).expect("encode").bytes;
        assert_eq!(
            bytes(&mut e, Op::SetFrequency(Vfo::A, Hz(98_100_000))),
            b"\rFREQ 98100000\r"
        );
        assert_eq!(bytes(&mut e, Op::GetFrequency(Vfo::A)), b"\rFREQ?\r");
        assert_eq!(bytes(&mut e, Op::SetMode(Mode::Usb)), b"\rDEM USB\r");
        assert_eq!(bytes(&mut e, Op::GetMode), b"\rDEM?\r");
        assert_eq!(bytes(&mut e, Op::GetSignal), b"\rSENS:DATA? \"VOLT:AC\"\r");
        assert!(e.encode(&Op::SetMode(Mode::Wfm)).is_err());
        assert!(e.encode(&Op::SetPtt(true)).is_err());
        assert!(engine(shipped("aor-ar8000")).is_err());
    }

    // Tests [@ANCHOR: radioctl_rs_dialect_status]
    // Tests [@ANCHOR: radioctl_rs_dialect_decode]
    #[test]
    fn answers_are_numbers_and_names() {
        let mut e = engine(shipped("rs-esmc")).expect("engine");
        e.encode(&Op::GetFrequency(Vfo::A)).expect("get");
        assert_eq!(
            e.feed(b"98100000\r", T0),
            vec![Inbound::Observation(Observation::Frequency(
                Vfo::A,
                Hz(98_100_000)
            ))]
        );
        e.encode(&Op::GetFrequency(Vfo::A)).expect("get");
        assert!(matches!(
            e.feed(b"oops\r", T0).as_slice(),
            [Inbound::Unrecognised(_)]
        ));
        e.encode(&Op::GetMode).expect("get");
        assert_eq!(
            e.feed(b"LSB\r", T0),
            vec![Inbound::Observation(Observation::Mode(Mode::Lsb))]
        );
        e.encode(&Op::GetSignal).expect("get");
        assert_eq!(
            e.feed(b"41.7\r", T0),
            vec![Inbound::Observation(Observation::Signal(41))]
        );
        e.encode(&Op::GetSignal).expect("get");
        assert_eq!(
            e.feed(b"-12.0\r", T0),
            vec![Inbound::Observation(Observation::Signal(0))]
        );
        assert!(e.feed(b"\r", T0).is_empty());
    }

    // Tests [@ANCHOR: radioctl_rs_twin_new]
    // Tests [@ANCHOR: radioctl_rs_twin_line]
    #[test]
    fn the_twin_ignores_what_it_does_not_know() {
        let desc = shipped("rs-esmc");
        let mut twin = RsTwin::new(&desc).expect("twin");
        let mut state = crate::virtual_rig::VirtualRig::from_description(&desc)
            .expect("rig")
            .state()
            .clone();
        assert!(twin.line(&mut state, "FREQ abc").is_empty());
        assert!(twin.line(&mut state, "DEM XYZ").is_empty());
        assert!(twin.line(&mut state, "NOPE?").is_empty());
        assert!(twin.line(&mut state, "FREQ 7074000").is_empty());
        assert_eq!(state.freq_a, Hz(7_074_000));
        assert!(RsTwin::new(&shipped("aor-ar8000")).is_err());
    }
}
