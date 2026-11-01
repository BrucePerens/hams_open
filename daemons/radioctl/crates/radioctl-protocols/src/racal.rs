// SPDX-License-Identifier: AGPL-3.0-or-later
//! Racal RA6790/GM receiver: sans-IO client engine and virtual twin.
//!
//! **Sources.** Facts only, read from Hamlib 4.6.2 (released tarball, sha256
//! b2ac73f44dd1161e95fdee6c95276144757647bf92d7fdb369ee2fe41ed47ae8): `rigs/racal/racal.c` and
//! `ra6790.c`, and for the Ten-Tec RX-340 and RX-331 (which speak the same command letters)
//! `rigs/tentec/rx340.c` and `rx331.c`. No vendor document was read; nothing is verified against a
//! radio.
//!
//! Dialects: `ra6790` (the commands below, `$0` prefix, shortest megahertz decimal), `rx340`
//! (no prefix, six-decimal megahertz, both CW codes 3 and 4 read as CW; Hamlib's open sends `*R1`
//! to take remote control, which the engine has no hook for and the twin ignores) and `rx331`
//! (the `$0` prefix, six decimals, and replies that also start with `$0`), and `ra3702`
//! (`rigs/racal/ra37xx.c`: packet frames, LF then the command then CR, no receiver address;
//! frequency in hertz, set with `F` and asked with `QF`; mode a number, set with `M` and asked with
//! `QM`, answered `M<n>`; replies start with LF).
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

use crate::serial_vendor::{parse_decimal, vendor_of, RxBuffer, TwinSnapshot, VendorTwin};

const CR: u8 = b'\r';

/// The Racal-family command dialects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dialect {
    /// Racal RA6790/GM.
    Ra6790,
    /// Ten-Tec RX-340.
    Rx340,
    /// Ten-Tec RX-331.
    Rx331,
    /// Racal RA3702: packet frames (LF, command, CR), frequency in hertz.
    Ra3702,
}

impl Dialect {
    // [@ANCHOR: radioctl_racal_dialect_from_name]
    /// The dialect with a file-format name.
    pub fn from_name(name: &str) -> Option<Dialect> {
        match name {
            "ra6790" => Some(Dialect::Ra6790),
            "rx340" => Some(Dialect::Rx340),
            "rx331" => Some(Dialect::Rx331),
            "ra3702" => Some(Dialect::Ra3702),
            _ => None,
        }
    }

    // [@ANCHOR: radioctl_racal_dialect_prefix]
    /// What every command starts with.
    fn prefix(self) -> &'static str {
        match self {
            Dialect::Rx340 => "",
            Dialect::Ra3702 => "\n",
            Dialect::Ra6790 | Dialect::Rx331 => "$0",
        }
    }

    // [@ANCHOR: radioctl_racal_dialect_frequency]
    /// The megahertz field of a set-frequency command.
    pub fn frequency_text(self, hz: Hz) -> String {
        match self {
            Dialect::Ra6790 => format_mhz(hz),
            Dialect::Ra3702 => hz.0.to_string(),
            Dialect::Rx340 | Dialect::Rx331 => {
                format!("{}.{:06}", hz.0 / 1_000_000, hz.0 % 1_000_000)
            }
        }
    }

    // [@ANCHOR: radioctl_racal_dialect_body]
    /// A reply line without the receiver number the RX-331 puts in front of it.
    fn body(self, line: &[u8]) -> Option<&[u8]> {
        match self {
            Dialect::Rx331 => match line {
                [b'$', rest @ ..] => {
                    let digits = rest.iter().take_while(|b| b.is_ascii_digit()).count();
                    // Hamlib compares the number with the receiver it addressed (0).
                    (digits > 0 && rest.get(..digits) == Some(b"0")).then(|| &rest[digits..])
                }
                _ => None,
            },
            Dialect::Ra3702 => line.strip_prefix(b"\n"),
            Dialect::Ra6790 | Dialect::Rx340 => Some(line),
        }
    }

    // [@ANCHOR: radioctl_racal_dialect_words]
    /// The command words of the dialect: ask frequency, ask mode, set frequency, set mode.
    fn words(self) -> (&'static str, &'static str, &'static str, &'static str) {
        match self {
            Dialect::Ra3702 => ("QF", "QM", "F", "M"),
            _ => ("TF", "TDI", "F", "D"),
        }
    }

    // [@ANCHOR: radioctl_racal_dialect_parse_frequency]
    /// The frequency in a reply's number: megahertz on the Racal-6790 family, hertz on the RA3702.
    fn parse_frequency(self, text: &[u8]) -> Option<Hz> {
        match self {
            Dialect::Ra3702 => parse_decimal(text).map(Hz),
            _ => parse_mhz(text),
        }
    }
}

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
    dialect: Dialect,
    pending: Pending,
    rx: RxBuffer,
}

impl RacalEngine {
    // [@ANCHOR: radioctl_racal_engine_new]
    /// Builds an engine from a Racal vendor description.
    pub fn new(desc: RigDescription) -> Result<RacalEngine, RigError> {
        let (_, name) = vendor_of(&desc)?;
        let dialect = Dialect::from_name(name)
            .ok_or_else(|| unsupported("unknown Racal dialect in the description"))?;
        Ok(RacalEngine {
            desc,
            dialect,
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
        let prefix = self.dialect.prefix();
        let (text, reply, pending) = match op {
            Op::GetFrequency(Vfo::A) => (
                format!("{prefix}{}\r", self.dialect.words().0),
                ReplyKind::Observation(Field::Frequency(Vfo::A)),
                Pending::Frequency,
            ),
            Op::SetFrequency(Vfo::A, hz) => (
                format!(
                    "{prefix}{}{}\r",
                    self.dialect.words().2,
                    self.dialect.frequency_text(*hz)
                ),
                ReplyKind::None,
                Pending::Nothing,
            ),
            Op::GetMode => (
                format!("{prefix}{}\r", self.dialect.words().1),
                ReplyKind::Observation(Field::Mode),
                Pending::Mode,
            ),
            Op::SetMode(mode) => {
                let code = self
                    .desc
                    .mode_code(*mode)
                    .ok_or_else(|| unsupported("mode not in the description"))?;
                (
                    format!("{prefix}{}{}\r", self.dialect.words().3, char::from(code)),
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
            let Some(body) = self.dialect.body(&line) else {
                out.push(Inbound::Unrecognised(line));
                continue;
            };
            out.extend(match (pending, body) {
                (Pending::Frequency, [b'F', rest @ ..]) => match self.dialect.parse_frequency(rest)
                {
                    Some(hz) => vec![Inbound::Observation(Observation::Frequency(Vfo::A, hz))],
                    None => vec![Inbound::Unrecognised(line.clone())],
                },
                (Pending::Mode, [b'M', code]) if self.dialect == Dialect::Ra3702 => {
                    match self.desc.code_mode(*code) {
                        Some(m) => vec![Inbound::Observation(Observation::Mode(m))],
                        None => vec![Inbound::Unrecognised(line.clone())],
                    }
                }
                (Pending::Mode, [b'D', code, b'I', width @ ..]) if !width.is_empty() => {
                    // Both CW codes (3 variable BFO, 4 BFO centre) read as CW: the one the
                    // description does not carry is read as the one it does.
                    let found = self.desc.code_mode(*code).or_else(|| match code {
                        b'3' => self.desc.code_mode(b'4'),
                        b'4' => self.desc.code_mode(b'3'),
                        _ => None,
                    });
                    match found {
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
    dialect: Dialect,
    rx: RxBuffer,
    hz: Hz,
    mode: u8,
    codes: Vec<u8>,
}

impl RacalTwin {
    // [@ANCHOR: radioctl_racal_twin_new]
    /// A twin in the description's default state.
    pub fn new(desc: &RigDescription) -> Result<RacalTwin, RigError> {
        let (_, name) = vendor_of(desc)?;
        let dialect = Dialect::from_name(name)
            .ok_or_else(|| unsupported("unknown Racal dialect in the description"))?;
        Ok(RacalTwin {
            dialect,
            rx: RxBuffer::default(),
            hz: desc.default_frequency,
            mode: desc.mode_code(desc.default_mode).unwrap_or(b'1'),
            codes: desc.mode_codes.iter().map(|(_, c)| *c).collect(),
        })
    }
}

impl VendorTwin for RacalTwin {
    // [@ANCHOR: radioctl_racal_twin_snapshot]
    fn snapshot(&self) -> Option<TwinSnapshot> {
        Some(TwinSnapshot {
            frequency: self.hz,
            mode_code: Some(self.mode),
            ptt: false,
        })
    }

    // [@ANCHOR: radioctl_racal_twin_feed]
    fn feed(&mut self, bytes: &[u8], now: Duration) -> Vec<u8> {
        self.rx.push(bytes, now);
        let mut out = Vec::new();
        while let Some(line) = self.rx.take_line(CR) {
            // Commands carry the dialect's prefix; replies of the RX-331 carry `$0` too.
            let prefix = self.dialect.prefix().as_bytes();
            let Some(cmd) = line.strip_prefix(prefix) else {
                continue;
            };
            let reply_prefix = match self.dialect {
                Dialect::Rx331 => "$0",
                Dialect::Ra3702 => "\n",
                _ => "",
            };
            let (ask_freq, ask_mode, _, _) = self.dialect.words();
            let ra3702 = self.dialect == Dialect::Ra3702;
            match cmd {
                c if c == ask_freq.as_bytes() => out.extend(
                    format!("{reply_prefix}F{}\r", self.dialect.frequency_text(self.hz))
                        .into_bytes(),
                ),
                c if c == ask_mode.as_bytes() && ra3702 => {
                    out.extend(format!("{reply_prefix}M{}\r", char::from(self.mode)).into_bytes());
                }
                c if c == ask_mode.as_bytes() => {
                    out.extend(format!("{reply_prefix}D{}I3\r", char::from(self.mode)).into_bytes())
                }
                // Hamlib's RA3702 mode read goes on to ask the filter type and a filter's
                // bandwidth; the answers only need the right first letter and length.
                b"QB" if ra3702 => out.extend(format!("{reply_prefix}B1,0\r").into_bytes()),
                c if ra3702 && c.starts_with(b"QBCON") => {
                    out.extend(format!("{reply_prefix}BCON1,0,6000\r").into_bytes());
                }
                [b'F', rest @ ..] => {
                    if let Some(hz) = self.dialect.parse_frequency(rest) {
                        self.hz = hz;
                    }
                }
                [b'M', c] if ra3702 && self.codes.contains(c) => self.mode = *c,
                [b'D', c] if !ra3702 && self.codes.contains(c) => self.mode = *c,
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

    fn dialect_file(id: &str) -> RigDescription {
        let path = format!("{}/../../rigs/{id}.toml", env!("CARGO_MANIFEST_DIR"));
        let text = std::fs::read_to_string(path).expect("rig file");
        radioctl_core::rigfile::parse(&text)
            .expect("loads")
            .description
    }

    // Tests [@ANCHOR: radioctl_racal_dialect_from_name]
    // Tests [@ANCHOR: radioctl_racal_dialect_frequency]
    // Tests [@ANCHOR: radioctl_racal_dialect_body]
    // Tests [@ANCHOR: radioctl_racal_dialect_prefix]
    // Tests [@ANCHOR: radioctl_racal_dialect_words]
    // Tests [@ANCHOR: radioctl_racal_dialect_parse_frequency]
    #[test]
    fn the_ten_tec_dialects_have_their_own_prefix_decimals_and_reply_prefix() {
        assert_eq!(Dialect::from_name("rx340"), Some(Dialect::Rx340));
        assert_eq!(Dialect::from_name("nope"), None);
        assert_eq!(Dialect::Ra6790.frequency_text(Hz(14_200_000)), "14.2");
        assert_eq!(Dialect::Rx340.frequency_text(Hz(14_200_000)), "14.200000");
        assert_eq!(Dialect::Rx331.frequency_text(Hz(500)), "0.000500");
        assert_eq!(Dialect::Rx331.body(b"$0F7.0"), Some(&b"F7.0"[..]));
        assert_eq!(Dialect::Rx331.body(b"$1F7.0"), None);
        assert_eq!(Dialect::Rx331.body(b"F7.0"), None);
        assert_eq!(Dialect::Rx340.body(b"F7.0"), Some(&b"F7.0"[..]));
        assert_eq!(Dialect::Ra6790.prefix(), "$0");
        assert_eq!(Dialect::Rx340.prefix(), "");
        assert_eq!(Dialect::Ra3702.body(b"\nQF"), Some(&b"QF"[..]));
        assert_eq!(Dialect::Ra3702.body(b"QF"), None);
        assert_eq!(Dialect::from_name("ra3702"), Some(Dialect::Ra3702));
        assert_eq!(Dialect::Ra3702.frequency_text(Hz(7_074_000)), "7074000");
        assert_eq!(Dialect::Ra3702.words(), ("QF", "QM", "F", "M"));
        assert_eq!(Dialect::Rx340.words().0, "TF");
        assert_eq!(
            Dialect::Ra3702.parse_frequency(b"7074000"),
            Some(Hz(7_074_000))
        );
        assert_eq!(Dialect::Ra3702.parse_frequency(b"7.0"), None);
        assert_eq!(
            Dialect::Ra6790.parse_frequency(b"7.074"),
            Some(Hz(7_074_000))
        );
    }

    #[test]
    fn the_rx340_and_rx331_files_round_trip_against_their_twins() {
        for (id, set, get, reply) in [
            ("tentec-rx-340", "F14.200000\r", "TF\r", "F14.200000\r"),
            (
                "tentec-rx-331",
                "$0F14.200000\r",
                "$0TF\r",
                "$0F14.200000\r",
            ),
            ("racal-ra3702", "\nF14200000\r", "\nQF\r", "\nF14200000\r"),
        ] {
            let desc = dialect_file(id);
            let mut e = RacalEngine::new(desc.clone()).expect("engine");
            let mut t = RacalTwin::new(&desc).expect("twin");
            let r = e
                .encode(&Op::SetFrequency(Vfo::A, Hz(14_200_000)))
                .expect("encodes");
            assert_eq!(r.bytes, set.as_bytes(), "{id}");
            t.feed(&r.bytes, T0);
            let r = e.encode(&Op::GetFrequency(Vfo::A)).expect("encodes");
            assert_eq!(r.bytes, get.as_bytes(), "{id}");
            let answer = t.feed(&r.bytes, T0);
            assert_eq!(answer, reply.as_bytes(), "{id}");
            assert_eq!(
                e.feed(&answer, T0),
                vec![Inbound::Observation(Observation::Frequency(
                    Vfo::A,
                    Hz(14_200_000)
                ))]
            );
            for mode in [Mode::Am, Mode::Fm, Mode::Cw, Mode::Lsb, Mode::Usb] {
                let r = e.encode(&Op::SetMode(mode)).expect("encodes");
                t.feed(&r.bytes, T0);
                let r = e.encode(&Op::GetMode).expect("encodes");
                let answer = t.feed(&r.bytes, T0);
                assert_eq!(
                    e.feed(&answer, T0),
                    vec![Inbound::Observation(Observation::Mode(mode))],
                    "{id} {mode:?}"
                );
            }
            // The radio's other CW code, and a reply from another receiver number.
            if id == "racal-ra3702" {
                // Hamlib's RA3702 mode read goes on to the filter questions.
                assert_eq!(t.feed(b"\nQB\r", T0), b"\nB1,0\r");
                assert_eq!(t.feed(b"\nQBCON1,0\r", T0), b"\nBCON1,0,6000\r");
                e.encode(&Op::GetMode).expect("encodes");
                assert_eq!(
                    e.feed(b"\nM5\r", T0),
                    vec![Inbound::Observation(Observation::Mode(Mode::Cw))]
                );
            } else {
                e.encode(&Op::GetMode).expect("encodes");
                let other_cw = if id == "tentec-rx-331" {
                    "$0D4I3\r"
                } else {
                    "D4I3\r"
                };
                assert_eq!(
                    e.feed(other_cw.as_bytes(), T0),
                    vec![Inbound::Observation(Observation::Mode(Mode::Cw))],
                    "{id}"
                );
                e.encode(&Op::GetFrequency(Vfo::A)).expect("encodes");
                assert!(matches!(
                    e.feed(b"$7F7\r", T0).as_slice(),
                    [Inbound::Unrecognised(_)]
                ));
            }
            // The radio ignores what it does not know (Hamlib's remote-control command).
            assert!(t.feed(b"*R1\r", T0).is_empty());
        }
    }
}
