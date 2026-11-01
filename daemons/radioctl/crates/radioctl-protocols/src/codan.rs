// SPDX-License-Identifier: AGPL-3.0-or-later
//! CODAN Envoy and NGT HF transceivers: sans-IO client engine and virtual twin.
//!
//! **Sources.** Facts only, read from Hamlib 4.6.2 (released tarball, sha256
//! b2ac73f44dd1161e95fdee6c95276144757647bf92d7fdb369ee2fe41ed47ae8): `rigs/codan/codan.c` and
//! `README.codan`. No CODAN document was read; nothing is verified against a radio.
//!
//! The radio's "CICS" text console: a command is a line ended by LF (Hamlib joins two commands in
//! one write with a CR between them), answers are lines ended by CR LF. Hamlib first sends `echo off` and logs in as `admin` with an empty password (the
//! README asks the user to clear the Admin password and to enable the free-transmit option);
//! the engine has no open hook and sends none of that, so the console must already be set up.
//!
//! * `freq` is answered `FREQ: <kHz>` (a real number). `connect tcvr rf <Hz> <Hz>` sets the receive
//!   and transmit frequency (Hamlib sets both equal); Hamlib appends `freq` in the same write and
//!   reads only the first answer line.
//! * `mode` is answered `MODE: <a>, <b>, <centre>, <width>`; `mode USBW` and `mode LSBW` set (the
//!   only two modes Hamlib writes). The description's mode codes are `U` and `L`.
//! * `connect tcvr rf ptt` is answered with text containing `Ptt=Off` or another `Ptt=` value;
//!   `connect tcvr rf ptt on` / `off` sets, followed by `ptt` in the same write.

use std::time::Duration;

use radioctl_core::description::RigDescription;
use radioctl_core::engine::{Inbound, ProtocolEngine, ReplyKind, Request};
use radioctl_core::error::{ErrorKind, RigError};
use radioctl_core::op::{Field, Observation, Op};
use radioctl_core::transcript::Panel;
use radioctl_core::units::{Hz, Vfo};

use crate::serial_vendor::{vendor_of, RxBuffer, TwinSnapshot, VendorTwin};

const LF: u8 = b'\n';

fn unsupported(what: &str) -> RigError {
    RigError::new(ErrorKind::Unsupported, what.to_string())
}

// [@ANCHOR: radioctl_codan_khz_text]
/// A frequency as kilohertz with up to three decimals and no trailing zeros (`14074`, `14074.5`).
pub fn khz_text(hz: Hz) -> String {
    let whole = hz.0 / 1000;
    let frac = hz.0 % 1000;
    if frac == 0 {
        whole.to_string()
    } else {
        format!("{whole}.{}", format!("{frac:03}").trim_end_matches('0'))
    }
}

// [@ANCHOR: radioctl_codan_parse_khz]
/// The frequency of a kilohertz number with up to three decimals, or `None`.
pub fn parse_khz(text: &str) -> Option<Hz> {
    let text = text.trim();
    let (whole, frac) = text.split_once('.').unwrap_or((text, ""));
    if whole.is_empty() || frac.len() > 3 {
        return None;
    }
    let w: u64 = whole.parse().ok()?;
    let f: u64 = if frac.is_empty() {
        0
    } else {
        frac.parse::<u64>().ok()? * 10u64.pow(3 - u32::try_from(frac.len()).ok()?)
    };
    w.checked_mul(1000)?.checked_add(f).map(Hz)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pending {
    Nothing,
    Frequency,
    Mode,
    Ptt,
    Ack,
}

/// The CODAN client engine.
#[derive(Debug)]
pub struct CodanEngine {
    desc: RigDescription,
    pending: Pending,
    /// A second answer line (`FREQ:` or `PTT`) the radio sends after a set is to be dropped.
    drop_next: bool,
    rx: RxBuffer,
}

impl CodanEngine {
    // [@ANCHOR: radioctl_codan_engine_new]
    /// Builds an engine from a CODAN vendor description.
    pub fn new(desc: RigDescription) -> Result<CodanEngine, RigError> {
        let (_, name) = vendor_of(&desc)?;
        if name != "codan" {
            return Err(unsupported("unknown CODAN dialect in the description"));
        }
        Ok(CodanEngine {
            desc,
            pending: Pending::Nothing,
            drop_next: false,
            rx: RxBuffer::default(),
        })
    }
}

impl ProtocolEngine for CodanEngine {
    fn description(&self) -> &RigDescription {
        &self.desc
    }

    // [@ANCHOR: radioctl_codan_engine_encode]
    fn encode(&mut self, op: &Op) -> Result<Request, RigError> {
        if !self.desc.capabilities.has(op.required_capability()) {
            return Err(unsupported("capability not in the rig description"));
        }
        let (text, reply, pending) = match op {
            Op::GetFrequency(Vfo::A) => (
                "freq\n".to_string(),
                ReplyKind::Observation(Field::Frequency(Vfo::A)),
                Pending::Frequency,
            ),
            Op::SetFrequency(Vfo::A, hz) => (
                format!("connect tcvr rf {0} {0}\rfreq\n", hz.0),
                ReplyKind::Ack,
                Pending::Ack,
            ),
            Op::GetMode => (
                "mode\n".to_string(),
                ReplyKind::Observation(Field::Mode),
                Pending::Mode,
            ),
            Op::SetMode(mode) => {
                let name = match self.desc.mode_code(*mode) {
                    Some(b'U') => "USBW",
                    Some(b'L') => "LSBW",
                    _ => return Err(unsupported("mode not in the description")),
                };
                (format!("mode {name}\n"), ReplyKind::Ack, Pending::Ack)
            }
            Op::GetPtt => (
                "connect tcvr rf ptt\n".to_string(),
                ReplyKind::Observation(Field::Ptt),
                Pending::Ptt,
            ),
            Op::SetPtt(on) => (
                format!(
                    "connect tcvr rf ptt {}\rptt\n",
                    if *on { "on" } else { "off" }
                ),
                ReplyKind::Ack,
                Pending::Ack,
            ),
            _ => return Err(unsupported("not implemented for the CODAN engine")),
        };
        self.pending = pending;
        self.drop_next = matches!(op, Op::SetFrequency(..) | Op::SetPtt(_));
        self.rx.take_all();
        Ok(Request {
            bytes: text.into_bytes(),
            reply,
        })
    }

    // [@ANCHOR: radioctl_codan_engine_feed]
    fn feed(&mut self, bytes: &[u8], now: Duration) -> Vec<Inbound> {
        let mut out = Vec::new();
        if let Some(stale) = self.rx.push(bytes, now) {
            out.push(Inbound::Unrecognised(stale));
        }
        while let Some(raw) = self.rx.take_line(LF) {
            let line = String::from_utf8_lossy(&raw).trim().to_string();
            if line.is_empty() {
                continue;
            }
            let pending = std::mem::replace(&mut self.pending, Pending::Nothing);
            out.extend(match pending {
                Pending::Ack => {
                    // Any first answer line is the acknowledgement, as in Hamlib.
                    vec![Inbound::Ack]
                }
                Pending::Nothing if self.drop_next => {
                    self.drop_next = false;
                    Vec::new()
                }
                Pending::Frequency => match line.strip_prefix("FREQ:").and_then(parse_khz) {
                    Some(hz) => vec![Inbound::Observation(Observation::Frequency(Vfo::A, hz))],
                    None => vec![Inbound::Unrecognised(raw.clone())],
                },
                Pending::Mode => {
                    // `MODE: <a>, <b>, <centre>, <width>`: the first name decides, by its first
                    // three letters.
                    let first = line
                        .strip_prefix("MODE:")
                        .and_then(|rest| rest.split(',').next())
                        .map(str::trim)
                        .unwrap_or("");
                    let code = match first.get(..3) {
                        Some("USB") => Some(b'U'),
                        Some("LSB") => Some(b'L'),
                        _ => None,
                    };
                    match code.and_then(|c| self.desc.code_mode(c)) {
                        Some(m) => vec![Inbound::Observation(Observation::Mode(m))],
                        None => vec![Inbound::Unrecognised(raw.clone())],
                    }
                }
                Pending::Ptt => match line.find("Ptt") {
                    Some(i) => vec![Inbound::Observation(Observation::Ptt(
                        &line[i..] != "Ptt=Off",
                    ))],
                    None => vec![Inbound::Unrecognised(raw.clone())],
                },
                Pending::Nothing => vec![Inbound::Unrecognised(raw.clone())],
            });
        }
        out
    }
}

/// A virtual CODAN radio.
#[derive(Debug)]
pub struct CodanTwin {
    rx: RxBuffer,
    rx_hz: u64,
    tx_hz: u64,
    mode: u8,
    ptt: bool,
}

impl CodanTwin {
    // [@ANCHOR: radioctl_codan_twin_new]
    /// A twin in the description's default state.
    pub fn new(desc: &RigDescription) -> Result<CodanTwin, RigError> {
        let (_, name) = vendor_of(desc)?;
        if name != "codan" {
            return Err(unsupported("unknown CODAN dialect in the description"));
        }
        Ok(CodanTwin {
            rx: RxBuffer::default(),
            rx_hz: desc.default_frequency.0,
            tx_hz: desc.default_frequency.0,
            mode: desc.mode_code(desc.default_mode).unwrap_or(b'U'),
            ptt: false,
        })
    }

    // [@ANCHOR: radioctl_codan_twin_answer]
    /// The answer line to one command line.
    fn answer(&mut self, line: &str) -> String {
        let name = if self.mode == b'L' { "LSB" } else { "USB" };
        match line {
            "freq" => format!("FREQ: {}", khz_text(Hz(self.rx_hz))),
            "mode" => format!("MODE: {name}, {name}, 1500, 2400"),
            "ptt" | "connect tcvr rf ptt" => {
                format!("Ptt={}", if self.ptt { "On" } else { "Off" })
            }
            "mode USBW" => {
                self.mode = b'U';
                "OK".to_string()
            }
            "mode LSBW" => {
                self.mode = b'L';
                "OK".to_string()
            }
            "connect tcvr rf ptt on" | "connect tcvr rf ptt off" => {
                self.ptt = line.ends_with("on");
                "OK".to_string()
            }
            l if l.starts_with("connect tcvr rf ") => {
                let mut parts = l["connect tcvr rf ".len()..].split_whitespace();
                match (
                    parts.next().and_then(|v| v.parse::<u64>().ok()),
                    parts.next().and_then(|v| v.parse::<u64>().ok()),
                ) {
                    (Some(rx), Some(tx)) => {
                        self.rx_hz = rx;
                        self.tx_hz = tx;
                        "OK".to_string()
                    }
                    _ => "ERROR".to_string(),
                }
            }
            // The console set-up lines of Hamlib's open.
            "echo off" | "ver" | "login" | "login admin ''" => "OK".to_string(),
            _ => "ERROR".to_string(),
        }
    }
}

impl VendorTwin for CodanTwin {
    // [@ANCHOR: radioctl_codan_twin_snapshot]
    fn snapshot(&self) -> Option<TwinSnapshot> {
        Some(TwinSnapshot {
            frequency: Hz(self.rx_hz),
            mode_code: Some(self.mode),
            ptt: self.ptt,
        })
    }

    // [@ANCHOR: radioctl_codan_twin_feed]
    fn feed(&mut self, bytes: &[u8], now: Duration) -> Vec<u8> {
        self.rx.push(bytes, now);
        let mut out = Vec::new();
        // A command ends with LF; Hamlib joins two commands in one write with a CR between.
        while let Some(raw) = self.rx.take_line(LF) {
            for piece in raw.split(|b| *b == b'\r') {
                let line = String::from_utf8_lossy(piece).trim().to_string();
                if line.is_empty() {
                    continue;
                }
                let answer = self.answer(&line);
                out.extend(format!("{answer}\r\n").into_bytes());
            }
        }
        out
    }

    // [@ANCHOR: radioctl_codan_twin_front_panel]
    fn front_panel(&mut self, panel: Panel) -> Result<Vec<u8>, String> {
        match panel {
            Panel::Frequency(hz) => {
                self.rx_hz = hz.0;
                self.tx_hz = hz.0;
            }
            Panel::Ptt(on) => self.ptt = on,
            other => return Err(format!("a CODAN twin has no {other:?} knob")),
        }
        Ok(Vec::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use radioctl_core::units::Mode;

    const T0: Duration = Duration::from_secs(1);

    fn run(e: &mut CodanEngine, t: &mut CodanTwin, op: Op) -> (Vec<u8>, Vec<Inbound>) {
        let r = e.encode(&op).expect("encodes");
        let a = t.feed(&r.bytes, T0);
        (r.bytes, e.feed(&a, T0))
    }

    fn shipped(id: &str) -> RigDescription {
        let path = format!("{}/../../rigs/{id}.toml", env!("CARGO_MANIFEST_DIR"));
        let text = std::fs::read_to_string(path).expect("rig file");
        radioctl_core::rigfile::parse(&text)
            .expect("loads")
            .description
    }

    // Tests [@ANCHOR: radioctl_codan_khz_text]
    // Tests [@ANCHOR: radioctl_codan_parse_khz]
    #[test]
    fn kilohertz_numbers_round_trip() {
        for hz in [14_074_000, 14_074_500, 500_100, 1, 29_999_999] {
            assert_eq!(parse_khz(&khz_text(Hz(hz))), Some(Hz(hz)), "{hz}");
        }
        assert_eq!(khz_text(Hz(14_074_000)), "14074");
        assert_eq!(khz_text(Hz(14_074_500)), "14074.5");
        assert_eq!(parse_khz(" 7000 "), Some(Hz(7_000_000)));
        assert_eq!(parse_khz("7000.1234"), None);
        assert_eq!(parse_khz(".5"), None);
        assert_eq!(parse_khz("x"), None);
    }

    // Tests [@ANCHOR: radioctl_codan_engine_new]
    #[test]
    fn other_families_are_refused() {
        assert!(CodanEngine::new(RigDescription::ts2000()).is_err());
        assert!(CodanTwin::new(&RigDescription::ts2000()).is_err());
    }

    // Tests [@ANCHOR: radioctl_codan_engine_encode]
    // Tests [@ANCHOR: radioctl_codan_engine_feed]
    // Tests [@ANCHOR: radioctl_codan_twin_new]
    // Tests [@ANCHOR: radioctl_codan_twin_answer]
    // Tests [@ANCHOR: radioctl_codan_twin_feed]
    // Tests [@ANCHOR: radioctl_codan_twin_snapshot]
    // Tests [@ANCHOR: radioctl_codan_twin_front_panel]
    #[test]
    fn both_models_round_trip_against_the_twin() {
        for id in ["codan-envoy", "codan-ngt"] {
            let desc = shipped(id);
            let mut e = CodanEngine::new(desc.clone()).expect("engine");
            let mut t = CodanTwin::new(&desc).expect("twin");
            let (wire, seen) = run(&mut e, &mut t, Op::SetFrequency(Vfo::A, Hz(14_074_500)));
            assert_eq!(wire, b"connect tcvr rf 14074500 14074500\rfreq\n", "{id}");
            // The first line acknowledges; the `freq` answer after it is dropped.
            assert_eq!(seen, vec![Inbound::Ack], "{id}");
            assert_eq!(
                run(&mut e, &mut t, Op::GetFrequency(Vfo::A)).1,
                vec![Inbound::Observation(Observation::Frequency(
                    Vfo::A,
                    Hz(14_074_500)
                ))]
            );
            for mode in [Mode::Usb, Mode::Lsb] {
                assert_eq!(run(&mut e, &mut t, Op::SetMode(mode)).1, vec![Inbound::Ack]);
                assert_eq!(
                    run(&mut e, &mut t, Op::GetMode).1,
                    vec![Inbound::Observation(Observation::Mode(mode))],
                    "{id} {mode:?}"
                );
            }
            assert_eq!(run(&mut e, &mut t, Op::SetPtt(true)).1, vec![Inbound::Ack]);
            assert_eq!(
                run(&mut e, &mut t, Op::GetPtt).1,
                vec![Inbound::Observation(Observation::Ptt(true))]
            );
            assert!(t.snapshot().expect("snapshot").ptt);
            run(&mut e, &mut t, Op::SetPtt(false));
            assert_eq!(
                run(&mut e, &mut t, Op::GetPtt).1,
                vec![Inbound::Observation(Observation::Ptt(false))]
            );
            t.front_panel(Panel::Frequency(Hz(5_000_000)))
                .expect("tunes");
            assert_eq!(t.feed(b"freq\n", T0), b"FREQ: 5000\r\n");
            assert_eq!(t.snapshot().expect("snapshot").frequency, Hz(5_000_000));
            assert!(t.front_panel(Panel::Mode(Mode::Am)).is_err());
            assert!(e.encode(&Op::SetMode(Mode::Am)).is_err());
            assert_eq!(t.feed(b"nonsense\n", T0), b"ERROR\r\n");
            assert_eq!(t.feed(b"\r\n", T0), b"");
            e.encode(&Op::GetFrequency(Vfo::A)).expect("encodes");
            assert!(matches!(
                e.feed(b"FREQ: abc\r\n", T0).as_slice(),
                [Inbound::Unrecognised(_)]
            ));
            e.encode(&Op::GetMode).expect("encodes");
            assert!(matches!(
                e.feed(b"MODE: AM, AM, 1, 2\r\n", T0).as_slice(),
                [Inbound::Unrecognised(_)]
            ));
            e.encode(&Op::GetPtt).expect("encodes");
            assert!(matches!(
                e.feed(b"hello\r\n", T0).as_slice(),
                [Inbound::Unrecognised(_)]
            ));
        }
    }
}
