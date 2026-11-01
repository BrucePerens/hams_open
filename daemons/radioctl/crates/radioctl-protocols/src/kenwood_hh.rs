// SPDX-License-Identifier: AGPL-3.0-or-later
//! Kenwood handheld and mobile ASCII CAT (TH-D7A, TH-F6A/F7E, TH-G71, TM-D700, TM-V7, TM-D710, TM-V71,
//! TH-D72, TH-D74): sans-IO client engine and the virtual rig that stands in for it.
//!
//! Commands are short ASCII words, a space, comma-separated parameters, ended by a carriage return
//! (no `IF`, no semicolon). A setting is answered by an echo of the command; a refusal is `?`.
//!
//! **Sources.** Hamlib 4.6.2 (`rigs/kenwood/th.c`, `thd72.c`, `thd74.c`, `tmd710.c`, `tmd700.c`,
//! `tmv7.c`, `thd7.c`, `thf7.c`, `thf6a.c`, `thg71.c`), read for facts only (command strings,
//! field offsets, mode tables), nothing copied. All of it is unverified:
//!
//! * dialect `th`: `FQ` reads `FQ 00146520000,0` (eleven digits of hertz, then the step code);
//!   `FQ <11 digits>,<step>` sets, the step code chosen as Hamlib does (0 for 5 kHz, 1 for 6.25 kHz,
//!   4 at 470 MHz and above, where the frequency is rounded to 10 kHz). `MD` reads and `MD n` sets
//!   the mode digit, `SM 0` reads the signal (`SM 0,n`), `TX` and `RX` key and unkey. Only the
//!   current band is addressable (`FQ` has no band argument; Hamlib switches bands with `BC`), so
//!   VFO B is refused.
//! * dialects `tm710`, `th72`, `th74`: `FO b` (b is 0 or 1, the band) reads a fixed-column line:
//!   `FO b,` then ten digits of hertz at columns 5 to 14, and the mode digit at a fixed column
//!   (47 on a 48-character TM-D710/TM-V71 line, 51 on the TH-D72's 52 characters, 31 on the TH-D74's
//!   72). A set rewrites the whole line, so the engine keeps the last line it read for each band
//!   and refuses a set before the band has been read. The TH-D74 sets the mode with `MD b,k`.
//!
//! Not implemented: bands/VFO switching (`BC`, `VMC`), memories, tones, repeater shifts, split,
//! levels other than the S-meter, FM narrow and DV/DR modes (no radioctl mode). `TC 1` (the
//! PC-control mode entry named in the work brief) appears in none of the sources read, so it is not
//! sent: an open question for a manual or a volunteer's capture.

use std::time::Duration;

use radioctl_core::description::{Family, HhDialect, RigDescription};
use radioctl_core::engine::{Inbound, ProtocolEngine, ReplyKind, Request, PARTIAL_FRAME_TIMEOUT};
use radioctl_core::error::{ErrorKind, RigError};
use radioctl_core::op::{Field, Observation, Op};
use radioctl_core::units::{Hz, Vfo};

/// The line terminator.
const CR: u8 = b'\r';
/// Columns of the frequency digits in an `FO` line.
const FO_FREQ_AT: usize = 5;
/// Digits of hertz in an `FO` line.
const FO_FREQ_DIGITS: usize = 10;

// [@ANCHOR: radioctl_hh_fo_layout]
/// `(line length, mode column)` of the `FO` line, or `None` for the `FQ` dialect.
pub fn fo_layout(dialect: HhDialect) -> Option<(usize, usize)> {
    match dialect {
        HhDialect::Th => None,
        HhDialect::Tm710 => Some((48, 47)),
        HhDialect::Th72 => Some((52, 51)),
        HhDialect::Th74 => Some((72, 31)),
    }
}

/// Rounds half away from zero to a multiple of `unit`.
fn round_to(hz: u64, unit: u64) -> u64 {
    (hz + unit / 2) / unit * unit
}

// [@ANCHOR: radioctl_hh_th_step_and_frequency]
/// The frequency to send and its step code for the `FQ` dialect: the nearer of the 5 kHz and
/// 6.25 kHz grids (a tie takes 6.25 kHz), and 10 kHz grid with step code 4 from 470 MHz up.
pub fn th_step_and_frequency(hz: u64) -> (u64, u8) {
    let f5 = round_to(hz, 5_000);
    let f625 = round_to(hz * 4, 25_000) / 4;
    let (sent, step) = if f5.abs_diff(hz) < f625.abs_diff(hz) {
        (f5, 0)
    } else {
        (f625, 1)
    };
    if sent >= 470_000_000 {
        (round_to(sent, 10_000), 4)
    } else {
        (sent, step)
    }
}

/// What the engine is waiting for.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Pending {
    Nothing,
    Frequency(usize),
    Mode,
    Signal,
    /// The echo of a setting; for an `FO` set, the band and line to remember once it is accepted.
    Ack(Option<(usize, String)>),
}

/// The Kenwood handheld/mobile client engine for one rig.
#[derive(Debug)]
pub struct KenwoodHhEngine {
    desc: RigDescription,
    dialect: HhDialect,
    pending: Pending,
    buffer: Vec<u8>,
    last_byte_at: Duration,
    lines: [Option<String>; 2],
}

fn band(vfo: Vfo) -> usize {
    usize::from(vfo == Vfo::B)
}

impl KenwoodHhEngine {
    // [@ANCHOR: radioctl_hh_engine_new]
    /// Builds an engine from a kenwood-hh description.
    pub fn new(desc: RigDescription) -> Result<KenwoodHhEngine, RigError> {
        let dialect = match (&desc.family, &desc.kenwood_hh) {
            (Family::KenwoodHh, Some(params)) => params.dialect,
            _ => {
                return Err(RigError::new(
                    ErrorKind::Unsupported,
                    "not a Kenwood handheld description",
                ))
            }
        };
        Ok(KenwoodHhEngine {
            desc,
            dialect,
            pending: Pending::Nothing,
            buffer: Vec::new(),
            last_byte_at: Duration::ZERO,
            lines: [None, None],
        })
    }

    fn unsupported(what: &str) -> RigError {
        RigError::new(ErrorKind::Unsupported, what.to_string())
    }

    // [@ANCHOR: radioctl_hh_command]
    fn command(text: String, reply: ReplyKind) -> Request {
        let mut bytes = text.into_bytes();
        bytes.push(CR);
        Request { bytes, reply }
    }

    // [@ANCHOR: radioctl_hh_edited_line]
    /// The remembered `FO` line of a band with `edit` applied, or the reason it cannot be built.
    fn edited_line(
        &self,
        vfo: Vfo,
        edit: impl FnOnce(&mut Vec<u8>),
    ) -> Result<(usize, String), RigError> {
        let b = band(vfo);
        let mut bytes = self.lines[b]
            .clone()
            .ok_or_else(|| Self::unsupported("read the band first: an FO set rewrites the line"))?
            .into_bytes();
        edit(&mut bytes);
        let text = String::from_utf8(bytes)
            .map_err(|_| Self::unsupported("the remembered FO line is not text"))?;
        Ok((b, text))
    }

    // [@ANCHOR: radioctl_hh_parse_fo]
    /// The frequency and mode digit of an `FO` line of this dialect for `band`, or `None`.
    fn parse_fo(&self, line: &str, b: usize) -> Option<(Hz, u8)> {
        let (len, mode_at) = fo_layout(self.dialect)?;
        let bytes = line.as_bytes();
        if bytes.len() != len
            || !line.starts_with(&format!("FO {b},"))
            || bytes.get(15) != Some(&b',')
        {
            return None;
        }
        let digits = line.get(FO_FREQ_AT..FO_FREQ_AT + FO_FREQ_DIGITS)?;
        if !digits.bytes().all(|d| d.is_ascii_digit()) {
            return None;
        }
        let mode = *bytes.get(mode_at)?;
        let hz = digits.parse().ok()?;
        mode.is_ascii_digit().then_some((Hz(hz), mode - b'0'))
    }

    fn mode_observation(&self, digit: u8) -> Option<Inbound> {
        self.desc
            .code_mode(digit)
            .map(|m| Inbound::Observation(Observation::Mode(m)))
    }

    // [@ANCHOR: radioctl_hh_decode_line]
    fn decode_line(&mut self, line: &str) -> Vec<Inbound> {
        if line == "?" {
            self.pending = Pending::Nothing;
            return vec![Inbound::Nak];
        }
        let pending = std::mem::replace(&mut self.pending, Pending::Nothing);
        let bad = || vec![Inbound::Unrecognised(line.as_bytes().to_vec())];
        match pending {
            Pending::Nothing => bad(),
            Pending::Ack(remember) => {
                if let Some((b, text)) = remember {
                    if line != text {
                        return bad();
                    }
                    self.lines[b] = Some(text);
                }
                vec![Inbound::Ack]
            }
            Pending::Frequency(b) if fo_layout(self.dialect).is_some() => {
                match self.parse_fo(line, b) {
                    Some((hz, _)) => {
                        self.lines[b] = Some(line.to_string());
                        let vfo = if b == 1 { Vfo::B } else { Vfo::A };
                        vec![Inbound::Observation(Observation::Frequency(vfo, hz))]
                    }
                    None => bad(),
                }
            }
            Pending::Mode if fo_layout(self.dialect).is_some() => match self.parse_fo(line, 0) {
                Some((_, digit)) => {
                    self.lines[0] = Some(line.to_string());
                    self.mode_observation(digit).map_or_else(bad, |i| vec![i])
                }
                None => bad(),
            },
            Pending::Frequency(_) => {
                let parsed = line
                    .strip_prefix("FQ ")
                    .and_then(|r| r.split_once(','))
                    .and_then(|(f, _)| f.parse::<u64>().ok());
                match parsed {
                    Some(hz) => vec![Inbound::Observation(Observation::Frequency(Vfo::A, Hz(hz)))],
                    None => bad(),
                }
            }
            Pending::Mode => {
                let digit = line
                    .strip_prefix("MD ")
                    .and_then(|r| r.bytes().next())
                    .filter(u8::is_ascii_digit)
                    .map(|d| d - b'0');
                digit
                    .and_then(|d| self.mode_observation(d))
                    .map_or_else(bad, |i| vec![i])
            }
            Pending::Signal => {
                let level = line
                    .strip_prefix("SM 0,")
                    .and_then(|r| r.parse::<u16>().ok());
                level.map_or_else(bad, |l| vec![Inbound::Observation(Observation::Signal(l))])
            }
        }
    }
}

impl ProtocolEngine for KenwoodHhEngine {
    fn description(&self) -> &RigDescription {
        &self.desc
    }

    // [@ANCHOR: radioctl_hh_engine_encode]
    fn encode(&mut self, op: &Op) -> Result<Request, RigError> {
        if !self.desc.capabilities.has(op.required_capability()) {
            return Err(Self::unsupported("capability not in the rig description"));
        }
        let fo = fo_layout(self.dialect);
        let (request, pending) = match op {
            Op::GetFrequency(vfo) => match fo {
                Some(_) => (
                    Self::command(
                        format!("FO {}", band(*vfo)),
                        ReplyKind::Observation(Field::Frequency(*vfo)),
                    ),
                    Pending::Frequency(band(*vfo)),
                ),
                None if *vfo == Vfo::A => (
                    Self::command(
                        "FQ".into(),
                        ReplyKind::Observation(Field::Frequency(Vfo::A)),
                    ),
                    Pending::Frequency(0),
                ),
                None => return Err(Self::unsupported("only the current band is addressable")),
            },
            Op::SetFrequency(vfo, hz) => match fo {
                Some(_) => {
                    if hz.0 >= 10u64.pow(FO_FREQ_DIGITS as u32) {
                        return Err(Self::unsupported("frequency exceeds ten digits"));
                    }
                    let digits = format!("{:010}", hz.0);
                    let (b, text) = self.edited_line(*vfo, |line| {
                        line.splice(FO_FREQ_AT..FO_FREQ_AT + FO_FREQ_DIGITS, digits.bytes());
                    })?;
                    (
                        Self::command(text.clone(), ReplyKind::Ack),
                        Pending::Ack(Some((b, text))),
                    )
                }
                None if *vfo == Vfo::A => {
                    let (sent, step) = th_step_and_frequency(hz.0);
                    if sent > 99_999_999_999 {
                        return Err(Self::unsupported("frequency exceeds eleven digits"));
                    }
                    (
                        Self::command(format!("FQ {sent:011},{step:X}"), ReplyKind::Ack),
                        Pending::Ack(None),
                    )
                }
                None => return Err(Self::unsupported("only the current band is addressable")),
            },
            Op::GetMode => match fo {
                Some(_) => (
                    Self::command("FO 0".into(), ReplyKind::Observation(Field::Mode)),
                    Pending::Mode,
                ),
                None => (
                    Self::command("MD".into(), ReplyKind::Observation(Field::Mode)),
                    Pending::Mode,
                ),
            },
            Op::SetMode(mode) => {
                let code = self
                    .desc
                    .mode_code(*mode)
                    .filter(|c| *c <= 9)
                    .ok_or_else(|| Self::unsupported("mode not in the description"))?;
                match (self.dialect, fo) {
                    (HhDialect::Th74, _) => (
                        Self::command(format!("MD 0,{code}"), ReplyKind::Ack),
                        Pending::Ack(None),
                    ),
                    (_, Some((_, mode_at))) => {
                        let (b, text) = self.edited_line(Vfo::A, |line| {
                            if let Some(slot) = line.get_mut(mode_at) {
                                *slot = b'0' + code;
                            }
                        })?;
                        (
                            Self::command(text.clone(), ReplyKind::Ack),
                            Pending::Ack(Some((b, text))),
                        )
                    }
                    (_, None) => (
                        Self::command(format!("MD {code}"), ReplyKind::Ack),
                        Pending::Ack(None),
                    ),
                }
            }
            Op::SetPtt(on) => (
                Self::command(if *on { "TX" } else { "RX" }.into(), ReplyKind::Ack),
                Pending::Ack(None),
            ),
            Op::GetSignal => (
                Self::command("SM 0".into(), ReplyKind::Observation(Field::Signal)),
                Pending::Signal,
            ),
            _ => {
                return Err(Self::unsupported(
                    "not implemented for the Kenwood handheld engine",
                ))
            }
        };
        self.pending = pending;
        self.buffer.clear();
        Ok(request)
    }

    // [@ANCHOR: radioctl_hh_engine_feed]
    fn feed(&mut self, bytes: &[u8], now: Duration) -> Vec<Inbound> {
        let mut out = Vec::new();
        if now.saturating_sub(self.last_byte_at) > PARTIAL_FRAME_TIMEOUT && !self.buffer.is_empty()
        {
            out.push(Inbound::Unrecognised(std::mem::take(&mut self.buffer)));
        }
        self.last_byte_at = now;
        self.buffer.extend_from_slice(bytes);
        while let Some(end) = self.buffer.iter().position(|b| *b == CR) {
            let raw: Vec<u8> = self.buffer.drain(..=end).collect();
            let body = raw.get(..raw.len() - 1).unwrap_or(&[]);
            match std::str::from_utf8(body) {
                Ok("") => {}
                Ok(line) => out.extend(self.decode_line(line)),
                Err(_) => out.push(Inbound::Unrecognised(body.to_vec())),
            }
        }
        out
    }
}

/// The virtual handheld: the radio side of a kenwood-hh description, kept apart from the engine.
#[derive(Debug)]
pub struct HhSim {
    dialect: HhDialect,
    freq: [u64; 2],
    mode: [u8; 2],
    buffer: Vec<u8>,
    last_byte_at: Duration,
}

impl HhSim {
    // [@ANCHOR: radioctl_hh_sim_new]
    /// A handheld on `freq` in the mode with code `mode`, both bands alike.
    pub fn new(dialect: HhDialect, freq: Hz, mode: u8) -> HhSim {
        HhSim {
            dialect,
            freq: [freq.0; 2],
            mode: [mode; 2],
            buffer: Vec::new(),
            last_byte_at: Duration::ZERO,
        }
    }

    // [@ANCHOR: radioctl_hh_sim_fo_line]
    /// The `FO` line of a band: the documented columns carry the band, frequency and mode, every
    /// other column is a fixed filler of our own.
    pub fn fo_line(&self, b: usize) -> String {
        let Some((len, mode_at)) = fo_layout(self.dialect) else {
            return String::new();
        };
        let mut bytes = format!("FO {b},{:010},", self.freq[b]).into_bytes();
        bytes.resize(len, b'0');
        if let Some(slot) = bytes.get_mut(mode_at) {
            *slot = b'0' + self.mode[b];
        }
        String::from_utf8(bytes).unwrap_or_default()
    }

    // [@ANCHOR: radioctl_hh_sim_line]
    fn line(
        &mut self,
        line: &str,
        state: &mut super::virtual_rig::RigState,
        signal: u16,
    ) -> String {
        let refuse = "?".to_string();
        let fo = fo_layout(self.dialect);
        let (word, args) = line.split_once(' ').unwrap_or((line, ""));
        match (word, args, fo) {
            ("FO", "0" | "1", Some(_)) => self.fo_line(usize::from(args == "1")),
            ("FO", _, Some((len, mode_at))) if line.len() == len => {
                let b = usize::from(line.as_bytes().get(3) == Some(&b'1'));
                let digits = line.get(FO_FREQ_AT..FO_FREQ_AT + FO_FREQ_DIGITS);
                let mode = line.as_bytes().get(mode_at).filter(|m| m.is_ascii_digit());
                match (digits.and_then(|d| d.parse::<u64>().ok()), mode) {
                    (Some(hz), Some(m)) if line.starts_with("FO ") => {
                        self.freq[b] = hz;
                        self.mode[b] = m - b'0';
                        line.to_string()
                    }
                    _ => refuse,
                }
            }
            ("FQ", "", None) => format!("FQ {:011},0", self.freq[0]),
            ("FQ", set, None) => match set.split_once(',') {
                Some((f, s)) if f.len() == 11 && s.len() == 1 => match f.parse::<u64>() {
                    Ok(hz) => {
                        self.freq[0] = hz;
                        line.to_string()
                    }
                    Err(_) => refuse,
                },
                _ => refuse,
            },
            ("MD", "", None) => format!("MD {}", self.mode[0]),
            ("MD", set, _) => {
                let (b, digit) = match set.split_once(',') {
                    Some((b, d)) if self.dialect == HhDialect::Th74 => (b, d),
                    None if self.dialect != HhDialect::Th74 && fo.is_none() => ("0", set),
                    _ => return refuse,
                };
                match (b, digit.as_bytes()) {
                    ("0" | "1", [d @ b'0'..=b'9']) => {
                        self.mode[usize::from(b == "1")] = d - b'0';
                        line.to_string()
                    }
                    _ => refuse,
                }
            }
            ("TX", "", _) => {
                state.ptt = true;
                "TX".to_string()
            }
            ("RX", "", _) => {
                state.ptt = false;
                "RX".to_string()
            }
            ("SM", "0", _) => format!("SM 0,{signal}"),
            _ => refuse,
        }
    }

    // [@ANCHOR: radioctl_hh_sim_feed]
    /// Consumes bytes written at `now`; the reply bytes. Frequency, mode, PTT and the signal are
    /// kept in `state` as well, so assertions on the virtual rig see them.
    pub fn feed(
        &mut self,
        bytes: &[u8],
        now: Duration,
        desc: &RigDescription,
        state: &mut super::virtual_rig::RigState,
    ) -> Vec<u8> {
        if now.saturating_sub(self.last_byte_at) > PARTIAL_FRAME_TIMEOUT {
            self.buffer.clear();
        }
        self.last_byte_at = now;
        self.buffer.extend_from_slice(bytes);
        let mut out = Vec::new();
        while let Some(end) = self.buffer.iter().position(|b| *b == CR) {
            let raw: Vec<u8> = self.buffer.drain(..=end).collect();
            let text = String::from_utf8_lossy(raw.get(..raw.len() - 1).unwrap_or(&[])).to_string();
            let reply = self.line(&text, state, state.signal);
            out.extend_from_slice(reply.as_bytes());
            out.push(CR);
        }
        state.freq_a = Hz(self.freq[0]);
        state.freq_b = Hz(self.freq[1]);
        if let Some(mode) = desc.code_mode(self.mode[0]) {
            state.mode = mode;
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::virtual_rig::VirtualRig;
    use radioctl_core::units::Mode;

    const T0: Duration = Duration::from_secs(1);

    fn shipped(id: &str) -> RigDescription {
        let path = format!("{}/../../rigs/{id}.toml", env!("CARGO_MANIFEST_DIR"));
        let text = std::fs::read_to_string(path).expect("rig file");
        radioctl_core::rigfile::parse(&text)
            .expect("loads")
            .description
    }

    /// One op through the engine and the virtual rig.
    fn run(e: &mut KenwoodHhEngine, r: &mut VirtualRig, op: Op) -> Vec<Inbound> {
        let request = e.encode(&op).expect("encodes");
        let answer = r.feed(&request.bytes, T0);
        e.feed(&answer, T0)
    }

    // Tests [@ANCHOR: radioctl_hh_th_step_and_frequency]
    #[test]
    fn the_step_follows_the_nearer_grid_and_ten_kilohertz_above_470_mhz() {
        assert_eq!(th_step_and_frequency(146_520_000), (146_520_000, 0));
        // On both grids: a tie takes the 6.25 kHz step, as the strict comparison in Hamlib does.
        assert_eq!(th_step_and_frequency(146_525_000), (146_525_000, 1));
        assert_eq!(th_step_and_frequency(146_512_500), (146_512_500, 1));
        assert_eq!(th_step_and_frequency(446_003_000), (446_005_000, 0));
        assert_eq!(th_step_and_frequency(1_200_013_000), (1_200_010_000, 4));
    }

    // Tests [@ANCHOR: radioctl_hh_fo_layout]
    // Tests [@ANCHOR: radioctl_hh_engine_new]
    #[test]
    fn engine_refuses_other_families() {
        assert!(KenwoodHhEngine::new(RigDescription::ts2000()).is_err());
        assert_eq!(fo_layout(HhDialect::Th), None);
        assert_eq!(fo_layout(HhDialect::Th74), Some((72, 31)));
    }

    // Tests [@ANCHOR: radioctl_hh_engine_encode]
    // Tests [@ANCHOR: radioctl_hh_engine_feed]
    // Tests [@ANCHOR: radioctl_hh_decode_line]
    // Tests [@ANCHOR: radioctl_hh_sim_feed]
    // Tests [@ANCHOR: radioctl_hh_command]
    // Tests [@ANCHOR: radioctl_hh_sim_line]
    #[test]
    fn the_fq_dialect_sends_the_documented_lines() {
        let d = shipped("kenwood-th-d7a");
        let mut e = KenwoodHhEngine::new(d.clone()).expect("engine");
        let mut r = VirtualRig::from_description(&d).expect("rig");
        let b = |e: &mut KenwoodHhEngine, op: Op| e.encode(&op).expect("encodes").bytes;
        assert_eq!(b(&mut e, Op::GetFrequency(Vfo::A)), b"FQ\r");
        assert_eq!(
            b(&mut e, Op::SetFrequency(Vfo::A, Hz(146_520_000))),
            b"FQ 00146520000,0\r"
        );
        assert_eq!(b(&mut e, Op::SetMode(Mode::Am)), b"MD 1\r");
        assert_eq!(b(&mut e, Op::SetPtt(true)), b"TX\r");
        assert!(e.encode(&Op::GetFrequency(Vfo::B)).is_err());
        assert!(e.encode(&Op::GetPtt).is_err());
        assert_eq!(
            run(&mut e, &mut r, Op::SetFrequency(Vfo::A, Hz(433_500_000))),
            vec![Inbound::Ack]
        );
        assert_eq!(
            run(&mut e, &mut r, Op::GetFrequency(Vfo::A)),
            vec![Inbound::Observation(Observation::Frequency(
                Vfo::A,
                Hz(433_500_000)
            ))]
        );
        run(&mut e, &mut r, Op::SetMode(Mode::Am));
        assert_eq!(
            run(&mut e, &mut r, Op::GetMode),
            vec![Inbound::Observation(Observation::Mode(Mode::Am))]
        );
        assert_eq!(
            run(&mut e, &mut r, Op::GetSignal),
            vec![Inbound::Observation(Observation::Signal(r.state().signal))]
        );
        assert_eq!(run(&mut e, &mut r, Op::SetPtt(true)), vec![Inbound::Ack]);
        assert!(r.state().ptt);
        run(&mut e, &mut r, Op::SetPtt(false));
        assert!(!r.state().ptt);
        // A refusal, an unasked line and a split line.
        e.encode(&Op::GetMode).expect("get");
        assert_eq!(e.feed(b"?\r", T0), vec![Inbound::Nak]);
        assert!(matches!(
            e.feed(b"ZZ\r", T0).as_slice(),
            [Inbound::Unrecognised(_)]
        ));
        e.encode(&Op::GetMode).expect("get");
        assert!(e.feed(b"MD", T0).is_empty());
        assert_eq!(
            e.feed(b" 0\r", T0),
            vec![Inbound::Observation(Observation::Mode(Mode::Fm))]
        );
    }

    // Tests [@ANCHOR: radioctl_hh_parse_fo]
    // Tests [@ANCHOR: radioctl_hh_edited_line]
    // Tests [@ANCHOR: radioctl_hh_sim_new]
    // Tests [@ANCHOR: radioctl_hh_sim_fo_line]
    #[test]
    fn the_fo_dialects_read_modify_and_rewrite_the_line() {
        for (id, len) in [
            ("kenwood-tm-d710", 48),
            ("kenwood-tm-v71", 48),
            ("kenwood-th-d72a", 52),
            ("kenwood-th-d74", 72),
        ] {
            let d = shipped(id);
            let mut e = KenwoodHhEngine::new(d.clone()).expect("engine");
            let mut r = VirtualRig::from_description(&d).expect("rig");
            assert!(
                e.encode(&Op::SetFrequency(Vfo::A, Hz(146_520_000)))
                    .is_err(),
                "{id}: a set before the first read"
            );
            let read = e.encode(&Op::GetFrequency(Vfo::B)).expect("encodes");
            assert_eq!(read.bytes, b"FO 1\r");
            let answer = r.feed(&read.bytes, T0);
            assert_eq!(answer.len(), len + 1, "{id}");
            assert_eq!(
                e.feed(&answer, T0),
                vec![Inbound::Observation(Observation::Frequency(
                    Vfo::B,
                    Hz(145_000_000)
                ))]
            );
            run(&mut e, &mut r, Op::GetFrequency(Vfo::A));
            assert_eq!(
                run(&mut e, &mut r, Op::SetFrequency(Vfo::A, Hz(146_520_000))),
                vec![Inbound::Ack],
                "{id}"
            );
            assert_eq!(r.state().freq_a, Hz(146_520_000), "{id}");
            assert_eq!(r.state().freq_b, Hz(145_000_000), "{id}");
            assert_eq!(
                run(&mut e, &mut r, Op::SetMode(Mode::Am)),
                vec![Inbound::Ack],
                "{id}"
            );
            assert_eq!(r.state().mode, Mode::Am, "{id}");
            assert_eq!(
                run(&mut e, &mut r, Op::GetMode),
                vec![Inbound::Observation(Observation::Mode(Mode::Am))],
                "{id}"
            );
            assert_eq!(
                run(&mut e, &mut r, Op::GetFrequency(Vfo::A)),
                vec![Inbound::Observation(Observation::Frequency(
                    Vfo::A,
                    Hz(146_520_000)
                ))],
                "{id}"
            );
            // A reply of the wrong length is not a line of this dialect.
            e.encode(&Op::GetFrequency(Vfo::A)).expect("get");
            assert!(matches!(
                e.feed(b"FO 0,0146520000,0\r", T0).as_slice(),
                [Inbound::Unrecognised(_)]
            ));
        }
        let d = shipped("kenwood-th-d74");
        let mut e = KenwoodHhEngine::new(d).expect("engine");
        assert_eq!(
            e.encode(&Op::SetMode(Mode::Usb)).expect("sets").bytes,
            b"MD 0,4\r"
        );
    }
}
