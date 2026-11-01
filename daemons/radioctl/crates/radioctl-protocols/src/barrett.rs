// SPDX-License-Identifier: AGPL-3.0-or-later
//! Barrett 950, 2050 and 4050 HF transceivers: sans-IO client engine and virtual twin.
//!
//! **Sources.** Facts only, read from Hamlib 4.6.2 (released tarball, sha256
//! b2ac73f44dd1161e95fdee6c95276144757647bf92d7fdb369ee2fe41ed47ae8): `rigs/barrett/barrett.c`
//! (shared by the three models), `950.c` and `4050.c`. No Barrett document was read; nothing is
//! verified against a radio.
//!
//! Commands are text ended by CR. Every answer is framed by the flow-control bytes: XOFF (13) in
//! front and XON (11) at the end, the text between ending in CR LF (Hamlib's source comment gives
//! the opposite order, its code reads XOFF first and stops at XON, which is what is done here).
//!
//! * `IR` / `IT` ask the receive / transmit frequency (decimal digits, hertz); `TR` and eight
//!   digits set the receive frequency, `TC9999T` and eight digits the transmit frequency; the
//!   radio answers `OK`. Hamlib sets both when it is not in split, so the engine does.
//! * `TB` and a letter set the mode (`U` USB, `L` LSB, `C` CW, `A` AM, `F` RTTY); `IB` asks, and
//!   the letter is the second byte of the answer (the first byte is not read by Hamlib; this
//!   engine and twin use `0`, an assumption). Hamlib adds a second CR to the mode write; this
//!   engine does not.
//! * `XP1` / `XP0` key and unkey (answer `OK`); `IP` asks (answer `1` or `0`).
//!
//! Dialects: `barrett` (the 2050: the `TR` / `TC` frequency commands above) and `barrett950` and
//! `barrett4050` (the 950 and 4050, which Hamlib tunes only through a memory channel: `XC` and
//! four digits select a channel, `IDC` and four digits read it back as the channel number, the
//! receive and the transmit frequency (eight digits each; an answer containing `E5` means the
//! channel is empty), `PC`, four digits, `R`, eight digits, `T`, eight digits program it). There
//! are ten band channels, one per band from 0 to 30 MHz, at a base number of 0 (950) or 9990
//! (4050); Hamlib reads the channel before writing it to spare the EEPROM, so the engine writes
//! a channel only when its own record of what it last wrote differs. Split is not supported there.

use std::collections::HashMap;
use std::time::Duration;

use radioctl_core::description::RigDescription;
use radioctl_core::engine::{Inbound, ProtocolEngine, ReplyKind, Request};
use radioctl_core::error::{ErrorKind, RigError};
use radioctl_core::op::{Field, Observation, Op};
use radioctl_core::transcript::Panel;
use radioctl_core::units::{Hz, Vfo};

use crate::serial_vendor::{parse_decimal, vendor_of, RxBuffer, TwinSnapshot, VendorTwin};

const CR: u8 = b'\r';
const XOFF: u8 = 0x13;
const XON: u8 = 0x11;

fn unsupported(what: &str) -> RigError {
    RigError::new(ErrorKind::Unsupported, what.to_string())
}

// [@ANCHOR: radioctl_barrett_frame]
/// An answer of the radio: XOFF, the text, CR LF, XON.
pub fn frame(text: &str) -> Vec<u8> {
    let mut out = vec![XOFF];
    out.extend_from_slice(text.as_bytes());
    out.extend_from_slice(b"\r\n");
    out.push(XON);
    out
}

// [@ANCHOR: radioctl_barrett_unframe]
/// The text of an answer taken up to (not including) its XON: the XOFF in front and the CR LF at
/// the end are dropped. An answer with no XOFF is accepted.
pub fn unframe(raw: &[u8]) -> &[u8] {
    let body = raw.strip_prefix(&[XOFF]).unwrap_or(raw);
    let body = body.strip_suffix(b"\n").unwrap_or(body);
    body.strip_suffix(b"\r").unwrap_or(body)
}

// [@ANCHOR: radioctl_barrett_band_offset]
/// The channel offset (0 to 9) of the band holding `hz`, or `None` above 30 MHz. The bands end at
/// 3.5, 5.3, 7.0, 10.1, 14.0, 18.068, 21.0, 24.89, 28.0 and 30.0 MHz; a frequency exactly on an
/// edge belongs to the upper band, as in Hamlib's loop that keeps the last match.
pub fn band_offset(hz: Hz) -> Option<u32> {
    const EDGES: [u64; 10] = [
        3_500_000, 5_300_000, 7_000_000, 10_100_000, 14_000_000, 18_068_000, 21_000_000,
        24_890_000, 28_000_000, 30_000_000,
    ];
    if hz.0 > 30_000_000 {
        return None;
    }
    let mut chosen = None;
    let mut lo = 0u64;
    for (i, hi) in EDGES.iter().enumerate() {
        if hz.0 >= lo && hz.0 <= *hi {
            chosen = u32::try_from(i).ok();
        }
        lo = *hi;
    }
    chosen
}

/// How the radio is tuned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tuning {
    /// `TR` and `TC` (the 2050).
    Direct,
    /// Memory channels from this base number (the 950: 0, the 4050: 9990).
    Channel(u32),
}

impl Tuning {
    // [@ANCHOR: radioctl_barrett_tuning_from_name]
    /// The tuning of a dialect name.
    fn from_name(name: &str) -> Option<Tuning> {
        match name {
            "barrett" => Some(Tuning::Direct),
            "barrett950" => Some(Tuning::Channel(0)),
            "barrett4050" => Some(Tuning::Channel(9990)),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pending {
    Nothing,
    Frequency(Vfo),
    Mode,
    Ptt,
    /// This many `OK` answers.
    Ack(usize),
}

/// The Barrett client engine.
#[derive(Debug)]
pub struct BarrettEngine {
    desc: RigDescription,
    tuning: Tuning,
    /// The frequency this engine last wrote to each channel, to write only on a change.
    written: HashMap<u32, u64>,
    /// The channel of a write in flight.
    writing: Option<u32>,
    pending: Pending,
    rx: RxBuffer,
}

impl BarrettEngine {
    // [@ANCHOR: radioctl_barrett_engine_new]
    /// Builds an engine from a Barrett vendor description.
    pub fn new(desc: RigDescription) -> Result<BarrettEngine, RigError> {
        let (_, name) = vendor_of(&desc)?;
        let tuning = Tuning::from_name(name)
            .ok_or_else(|| unsupported("unknown Barrett dialect in the description"))?;
        Ok(BarrettEngine {
            desc,
            tuning,
            written: HashMap::new(),
            writing: None,
            pending: Pending::Nothing,
            rx: RxBuffer::default(),
        })
    }
}

impl ProtocolEngine for BarrettEngine {
    fn description(&self) -> &RigDescription {
        &self.desc
    }

    // [@ANCHOR: radioctl_barrett_engine_encode]
    fn encode(&mut self, op: &Op) -> Result<Request, RigError> {
        if !self.desc.capabilities.has(op.required_capability()) {
            return Err(unsupported("capability not in the rig description"));
        }
        let (text, reply, pending) = match op {
            Op::GetFrequency(vfo) => (
                if *vfo == Vfo::A { "IR\r" } else { "IT\r" }.to_string(),
                ReplyKind::Observation(Field::Frequency(*vfo)),
                Pending::Frequency(*vfo),
            ),
            Op::SetFrequency(vfo, hz) => {
                if hz.0 > 99_999_999 {
                    return Err(unsupported("frequency does not fit eight digits"));
                }
                let mut t = String::new();
                let n = match self.tuning {
                    Tuning::Direct => {
                        if *vfo == Vfo::A {
                            t.push_str(&format!("TR{:08}\r", hz.0));
                        }
                        t.push_str(&format!("TC9999T{:08}\r", hz.0));
                        if *vfo == Vfo::A {
                            2
                        } else {
                            1
                        }
                    }
                    Tuning::Channel(base) => {
                        if *vfo != Vfo::A {
                            return Err(unsupported("no split on the channel-tuned Barretts"));
                        }
                        let offset = band_offset(*hz)
                            .ok_or_else(|| unsupported("frequency above 30 MHz"))?;
                        let chan = base + offset;
                        t.push_str(&format!("XC{chan:04}\r"));
                        let mut count = 1;
                        if self.written.get(&chan) != Some(&hz.0) {
                            t.push_str(&format!("PC{chan:04}R{:08}T{:08}\r", hz.0, hz.0));
                            count = 2;
                            self.writing = Some(chan);
                            self.written.insert(chan, hz.0);
                        }
                        count
                    }
                };
                (t, ReplyKind::Ack, Pending::Ack(n))
            }
            Op::GetMode => (
                "IB\r".to_string(),
                ReplyKind::Observation(Field::Mode),
                Pending::Mode,
            ),
            Op::SetMode(mode) => {
                let code = self
                    .desc
                    .mode_code(*mode)
                    .ok_or_else(|| unsupported("mode not in the description"))?;
                (
                    format!("TB{}\r", char::from(code)),
                    ReplyKind::Ack,
                    Pending::Ack(1),
                )
            }
            Op::GetPtt => (
                "IP\r".to_string(),
                ReplyKind::Observation(Field::Ptt),
                Pending::Ptt,
            ),
            Op::SetPtt(on) => (
                format!("XP{}\r", u8::from(*on)),
                ReplyKind::Ack,
                Pending::Ack(1),
            ),
            _ => return Err(unsupported("not implemented for the Barrett engine")),
        };
        if !matches!(op, Op::SetFrequency(..)) {
            self.writing = None;
        }
        self.pending = pending;
        self.rx.take_all();
        Ok(Request {
            bytes: text.into_bytes(),
            reply,
        })
    }

    // [@ANCHOR: radioctl_barrett_engine_feed]
    fn feed(&mut self, bytes: &[u8], now: Duration) -> Vec<Inbound> {
        let mut out = Vec::new();
        if let Some(stale) = self.rx.push(bytes, now) {
            out.push(Inbound::Unrecognised(stale));
        }
        while let Some(raw) = self.rx.take_line(XON) {
            let text = unframe(&raw).to_vec();
            let pending = self.pending;
            out.extend(match (pending, text.as_slice()) {
                (Pending::Ack(n), t) if t.starts_with(b"OK") => {
                    self.pending = if n > 1 {
                        Pending::Ack(n - 1)
                    } else {
                        self.writing = None;
                        Pending::Nothing
                    };
                    if n > 1 {
                        Vec::new()
                    } else {
                        vec![Inbound::Ack]
                    }
                }
                (Pending::Ack(_), _) => {
                    self.pending = Pending::Nothing;
                    // A refused write is not on the radio: forget it.
                    if let Some(chan) = self.writing.take() {
                        self.written.remove(&chan);
                    }
                    vec![Inbound::Nak]
                }
                (Pending::Frequency(vfo), t) => {
                    self.pending = Pending::Nothing;
                    match parse_decimal(t) {
                        Some(hz) => vec![Inbound::Observation(Observation::Frequency(vfo, Hz(hz)))],
                        None => vec![Inbound::Unrecognised(raw.clone())],
                    }
                }
                (Pending::Mode, [_, letter]) => {
                    self.pending = Pending::Nothing;
                    match self.desc.code_mode(*letter) {
                        Some(m) => vec![Inbound::Observation(Observation::Mode(m))],
                        None => vec![Inbound::Unrecognised(raw.clone())],
                    }
                }
                (Pending::Ptt, [c @ (b'0' | b'1')]) => {
                    self.pending = Pending::Nothing;
                    vec![Inbound::Observation(Observation::Ptt(*c == b'1'))]
                }
                _ => vec![Inbound::Unrecognised(raw.clone())],
            });
        }
        out
    }
}

/// A virtual Barrett transceiver.
#[derive(Debug)]
pub struct BarrettTwin {
    tuning: Tuning,
    /// The selected memory channel and what the channels hold (receive, transmit hertz).
    selected: Option<u32>,
    channels: HashMap<u32, (u64, u64)>,
    rx: RxBuffer,
    rx_hz: u64,
    tx_hz: u64,
    mode: u8,
    codes: Vec<u8>,
    ptt: bool,
}

impl BarrettTwin {
    // [@ANCHOR: radioctl_barrett_twin_new]
    /// A twin in the description's default state.
    pub fn new(desc: &RigDescription) -> Result<BarrettTwin, RigError> {
        let (_, name) = vendor_of(desc)?;
        let tuning = Tuning::from_name(name)
            .ok_or_else(|| unsupported("unknown Barrett dialect in the description"))?;
        let codes: Vec<u8> = desc.mode_codes.iter().map(|(_, c)| *c).collect();
        Ok(BarrettTwin {
            tuning,
            selected: None,
            channels: HashMap::new(),
            rx: RxBuffer::default(),
            rx_hz: desc.default_frequency.0,
            tx_hz: desc.default_frequency.0,
            mode: desc
                .mode_code(desc.default_mode)
                .or_else(|| codes.first().copied())
                .unwrap_or(b'U'),
            codes,
            ptt: false,
        })
    }

    // [@ANCHOR: radioctl_barrett_twin_answer]
    /// The answer text to one command line, or `None` for a line the radio does not know.
    fn answer(&mut self, line: &[u8]) -> Option<String> {
        match line {
            b"IR" => Some(self.rx_hz.to_string()),
            b"IT" => Some(self.tx_hz.to_string()),
            // The channel commands of the 950 and 4050.
            [b'X', b'C', digits @ ..] if digits.len() == 4 && self.tuning != Tuning::Direct => {
                let chan = u32::try_from(parse_decimal(digits)?).ok()?;
                self.selected = Some(chan);
                if let Some((rx, tx)) = self.channels.get(&chan) {
                    self.rx_hz = *rx;
                    self.tx_hz = *tx;
                }
                Some("OK".to_string())
            }
            [b'I', b'D', b'C', digits @ ..]
                if digits.len() == 4 && self.tuning != Tuning::Direct =>
            {
                let chan = u32::try_from(parse_decimal(digits)?).ok()?;
                Some(match self.channels.get(&chan) {
                    Some((rx, tx)) => format!("{chan:04}{rx:08}{tx:08}"),
                    None => "E5".to_string(),
                })
            }
            [b'P', b'C', d @ .., b'R', r0, r1, r2, r3, r4, r5, r6, r7, b'T', t0, t1, t2, t3, t4, t5, t6, t7]
                if d.len() == 4 && self.tuning != Tuning::Direct =>
            {
                let chan = u32::try_from(parse_decimal(d)?).ok()?;
                let rx = parse_decimal(&[*r0, *r1, *r2, *r3, *r4, *r5, *r6, *r7])?;
                let tx = parse_decimal(&[*t0, *t1, *t2, *t3, *t4, *t5, *t6, *t7])?;
                self.channels.insert(chan, (rx, tx));
                if self.selected == Some(chan) {
                    self.rx_hz = rx;
                    self.tx_hz = tx;
                }
                Some("OK".to_string())
            }
            b"IB" => Some(format!("0{}", char::from(self.mode))),
            b"IP" => Some(u8::from(self.ptt).to_string()),
            // Hamlib's open asks for the version; any text will do.
            b"IV" => Some("Barrett virtual".to_string()),
            [b'T', b'R', digits @ ..] if digits.len() == 8 && self.tuning == Tuning::Direct => {
                self.rx_hz = parse_decimal(digits)?;
                Some("OK".to_string())
            }
            [b'T', b'C', b'9', b'9', b'9', b'9', b'T', digits @ ..]
                if digits.len() == 8 && self.tuning == Tuning::Direct =>
            {
                self.tx_hz = parse_decimal(digits)?;
                Some("OK".to_string())
            }
            [b'T', b'B', c] if self.codes.contains(c) => {
                self.mode = *c;
                Some("OK".to_string())
            }
            [b'X', b'P', c @ (b'0' | b'1')] => {
                self.ptt = *c == b'1';
                Some("OK".to_string())
            }
            _ => None,
        }
    }
}

impl VendorTwin for BarrettTwin {
    // [@ANCHOR: radioctl_barrett_twin_snapshot]
    fn snapshot(&self) -> Option<TwinSnapshot> {
        Some(TwinSnapshot {
            frequency: Hz(self.rx_hz),
            mode_code: Some(self.mode),
            ptt: self.ptt,
        })
    }

    // [@ANCHOR: radioctl_barrett_twin_feed]
    fn feed(&mut self, bytes: &[u8], now: Duration) -> Vec<u8> {
        self.rx.push(bytes, now);
        let mut out = Vec::new();
        while let Some(line) = self.rx.take_line(CR) {
            if line.is_empty() {
                // Hamlib's doubled CR after a mode write.
                continue;
            }
            let text = self.answer(&line).unwrap_or_else(|| "ERROR".to_string());
            out.extend(frame(&text));
        }
        out
    }

    // [@ANCHOR: radioctl_barrett_twin_front_panel]
    fn front_panel(&mut self, panel: Panel) -> Result<Vec<u8>, String> {
        match panel {
            Panel::Frequency(hz) => {
                self.rx_hz = hz.0;
                self.tx_hz = hz.0;
            }
            Panel::Ptt(on) => self.ptt = on,
            other => return Err(format!("a Barrett twin has no {other:?} knob")),
        }
        Ok(Vec::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use radioctl_core::units::Mode;

    const T0: Duration = Duration::from_secs(1);

    fn run(e: &mut BarrettEngine, t: &mut BarrettTwin, op: Op) -> (Vec<u8>, Vec<Inbound>) {
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

    // Tests [@ANCHOR: radioctl_barrett_frame]
    // Tests [@ANCHOR: radioctl_barrett_unframe]
    #[test]
    fn answers_are_framed_by_xoff_and_xon() {
        assert_eq!(frame("OK"), b"\x13OK\r\n\x11");
        assert_eq!(unframe(b"\x13OK\r\n"), b"OK");
        assert_eq!(unframe(b"14200000\r"), b"14200000");
        assert_eq!(unframe(b""), b"");
    }

    // Tests [@ANCHOR: radioctl_barrett_engine_new]
    #[test]
    fn other_families_are_refused() {
        assert!(BarrettEngine::new(RigDescription::ts2000()).is_err());
        assert!(BarrettTwin::new(&RigDescription::ts2000()).is_err());
    }

    // Tests [@ANCHOR: radioctl_barrett_band_offset]
    // Tests [@ANCHOR: radioctl_barrett_tuning_from_name]
    #[test]
    fn bands_pick_the_channel_and_dialects_pick_the_tuning() {
        assert_eq!(band_offset(Hz(1_800_000)), Some(0));
        assert_eq!(band_offset(Hz(3_500_000)), Some(1));
        assert_eq!(band_offset(Hz(7_074_000)), Some(3));
        assert_eq!(band_offset(Hz(14_000_000)), Some(5));
        assert_eq!(band_offset(Hz(14_200_000)), Some(5));
        assert_eq!(band_offset(Hz(29_999_999)), Some(9));
        assert_eq!(band_offset(Hz(30_000_000)), Some(9));
        assert_eq!(band_offset(Hz(30_000_001)), None);
        assert_eq!(Tuning::from_name("barrett"), Some(Tuning::Direct));
        assert_eq!(Tuning::from_name("barrett950"), Some(Tuning::Channel(0)));
        assert_eq!(
            Tuning::from_name("barrett4050"),
            Some(Tuning::Channel(9990))
        );
        assert_eq!(Tuning::from_name("x"), None);
    }

    // Tests [@ANCHOR: radioctl_barrett_engine_encode]
    // Tests [@ANCHOR: radioctl_barrett_engine_feed]
    // Tests [@ANCHOR: radioctl_barrett_twin_new]
    // Tests [@ANCHOR: radioctl_barrett_twin_answer]
    // Tests [@ANCHOR: radioctl_barrett_twin_feed]
    // Tests [@ANCHOR: radioctl_barrett_twin_snapshot]
    // Tests [@ANCHOR: radioctl_barrett_twin_front_panel]
    #[test]
    fn the_three_models_round_trip_against_the_twin() {
        for (id, set_wire, again_wire, second_vfo) in [
            (
                "barrett-2050",
                &b"TR14200000\rTC9999T14200000\r"[..],
                &b"TR14200000\rTC9999T14200000\r"[..],
                true,
            ),
            (
                "barrett-950",
                &b"XC0005\rPC0005R14200000T14200000\r"[..],
                &b"XC0005\r"[..],
                false,
            ),
            (
                "barrett-4050",
                &b"XC9995\rPC9995R14200000T14200000\r"[..],
                &b"XC9995\r"[..],
                false,
            ),
        ] {
            let desc = shipped(id);
            let mut e = BarrettEngine::new(desc.clone()).expect("engine");
            let mut t = BarrettTwin::new(&desc).expect("twin");
            let (wire, seen) = run(&mut e, &mut t, Op::SetFrequency(Vfo::A, Hz(14_200_000)));
            assert_eq!(wire, set_wire, "{id}");
            assert_eq!(seen, vec![Inbound::Ack], "{id}");
            // The channel-tuned radios are written only when the frequency changes.
            let (wire, seen) = run(&mut e, &mut t, Op::SetFrequency(Vfo::A, Hz(14_200_000)));
            assert_eq!(wire, again_wire, "{id}");
            assert_eq!(seen, vec![Inbound::Ack], "{id}");
            assert_eq!(
                run(&mut e, &mut t, Op::GetFrequency(Vfo::A)).1,
                vec![Inbound::Observation(Observation::Frequency(
                    Vfo::A,
                    Hz(14_200_000)
                ))]
            );
            if second_vfo {
                run(&mut e, &mut t, Op::SetFrequency(Vfo::B, Hz(14_205_000)));
                assert_eq!(
                    run(&mut e, &mut t, Op::GetFrequency(Vfo::B)).1,
                    vec![Inbound::Observation(Observation::Frequency(
                        Vfo::B,
                        Hz(14_205_000)
                    ))]
                );
            } else {
                assert!(e.encode(&Op::SetFrequency(Vfo::B, Hz(14_205_000))).is_err());
                // Another band is another channel, and an empty channel reads as E5.
                run(&mut e, &mut t, Op::SetFrequency(Vfo::A, Hz(7_074_000)));
                assert_eq!(
                    t.snapshot().expect("snapshot").frequency,
                    Hz(7_074_000),
                    "{id}"
                );
                assert!(t.feed(b"IDC0777\r", T0).windows(2).any(|w| w == b"E5"));
                assert_eq!(
                    run(&mut e, &mut t, Op::SetFrequency(Vfo::A, Hz(14_200_000))).0,
                    again_wire,
                    "{id}"
                );
                assert!(e.encode(&Op::SetFrequency(Vfo::A, Hz(40_000_000))).is_err());
            }
            for mode in [Mode::Usb, Mode::Lsb, Mode::Cw, Mode::Am, Mode::Rtty] {
                assert_eq!(
                    run(&mut e, &mut t, Op::SetMode(mode)).1,
                    vec![Inbound::Ack],
                    "{id} {mode:?}"
                );
                assert_eq!(
                    run(&mut e, &mut t, Op::GetMode).1,
                    vec![Inbound::Observation(Observation::Mode(mode))],
                    "{id} {mode:?}"
                );
            }
            assert_eq!(run(&mut e, &mut t, Op::SetPtt(true)).0, b"XP1\r");
            assert!(t.snapshot().expect("snapshot").ptt);
            run(&mut e, &mut t, Op::SetPtt(false));
            assert!(!t.snapshot().expect("snapshot").ptt);
            t.front_panel(Panel::Frequency(Hz(7_000_000)))
                .expect("tunes");
            assert_eq!(t.feed(b"IR\r", T0), frame("7000000"));
            assert!(t.front_panel(Panel::Mode(Mode::Am)).is_err());
            assert!(e
                .encode(&Op::SetFrequency(Vfo::A, Hz(100_000_000)))
                .is_err());
            // A refusal and a stranger.
            e.encode(&Op::SetPtt(true)).expect("encodes");
            assert_eq!(e.feed(&frame("ERROR"), T0), vec![Inbound::Nak]);
            e.encode(&Op::GetFrequency(Vfo::A)).expect("encodes");
            assert!(matches!(
                e.feed(&frame("x"), T0).as_slice(),
                [Inbound::Unrecognised(_)]
            ));
            assert_eq!(t.feed(b"ZZ\r\r", T0), frame("ERROR"));
        }
    }
}
