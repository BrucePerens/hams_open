// SPDX-License-Identifier: AGPL-3.0-or-later
//! Uniden BCD396T and BCD996T digital trunking scanners (the `GLG` / `QSH` remote command set):
//! sans-IO client engine and virtual twin.
//!
//! **Sources.** Uniden's "BCD396T Operation Specification" and "BCD996T Operation Specification"
//! remote command chapters (7.13, "Programming Control Codes for use with Uniden scanners"; the
//! files are kept outside the repositories in `~/manuals/uniden/` with their sha256 in
//! `SHA256SUMS`), read for facts only: the line format and the error replies (PDF page 7 of each),
//! the command list (page 8), `QSH` (page 11), `STS` and `GLG` (pages 12 to 14), `MDL` and `VER`.
//! Hamlib 4.6.2's `uniden_digital.c` has only stubs for frequency and mode (it can read the
//! scanner's status, model and version), so there is no second source. The documents carry an end
//! user licence agreement with confidentiality and attribution terms; only facts were taken, no text
//! or tables were copied, and the licence's notice wording ("contains Uniden proprietary and/or
//! copyright control codes. Used with permission") is NOT claimed here: whether the project may
//! ship these command facts under that agreement is a decision for the project owner.
//!
//! * Line: 9600 to 115200 baud, 8N1, no flow control; commands and answers end with CR only.
//! * Every command is answered before the next is sent. Errors: `ERR` (format or value error),
//!   `NG` (the command is invalid now), `FER` (framing), `ORER` (overrun); a setting is answered
//!   `<CMD>,OK`.
//! * `GLG` reads the reception: `GLG,<frequency or TGID>,<modulation>,<attenuation>,<CTCSS/DCS>,
//!   <name 1>,<name 2>,<name 3>,<squelch>,<mute>`; while nothing is received the fields are empty.
//!   The frequency is eight digits in units of 100 Hz (the document's `FRQ` format).
//! * `QSH,<frq>,<stp>,<mod>,<att>,<dly>,<skp>,<code>,<bsc>,<rep>` goes to quick search hold at a
//!   frequency (the scanner's VFO); empty fields are left unchanged. Frequency and modulation
//!   (`AUTO`, `AM`, `FM`, `NFM`, `WFM`) are set this way. The command is refused (`QSH,NG`) while
//!   the scanner is in a menu, during direct entry or quick save.
//! * The documents describe no command that enters a "PC control mode" and no signal-strength
//!   reading (`GLG` gives only the squelch and mute states), so signal strength is not offered.
//!
//! Not implemented: program mode (`PRG`/`EPG`), memories, systems, `STS`, key presses, volume,
//! squelch.

use radioctl_core::description::{ReceiverProtocol, RigDescription};
use radioctl_core::engine::{Inbound, ReplyKind};
use radioctl_core::error::{ErrorKind, RigError};
use radioctl_core::op::{Field, Observation, Op};
use radioctl_core::units::{Hz, Mode, Vfo};

use crate::receiver::{Dialect, LineEngine, ReceiverTwin};
use crate::virtual_rig::RigState;

/// The engine of the BCD396T and BCD996T.
pub type UnidenDigitalEngine = LineEngine<UnidenDigitalDialect>;

// [@ANCHOR: radioctl_unidig_engine]
/// The engine for a BCD396T or BCD996T description.
pub fn engine(desc: RigDescription) -> Result<UnidenDigitalEngine, RigError> {
    LineEngine::new(desc, ReceiverProtocol::Uniden)
}

/// The mode spellings by radioctl's mode code (the same numbering as the older Uniden files).
const MODE_NAMES: [(u8, &str, Mode); 3] = [
    (1, "AM", Mode::Am),
    (2, "FM", Mode::Fm),
    (3, "WFM", Mode::Wfm),
];

/// The scanner's lowest and highest receive frequency in units of 100 Hz (25 MHz to 1.3 GHz); the
/// twin refuses anything outside, and a gap inside the real coverage is not modelled.
const FRQ_RANGE: (u64, u64) = (250_000, 13_000_000);

/// The BCD396T / BCD996T command set.
#[derive(Debug, Clone, Copy)]
pub struct UnidenDigitalDialect;

/// True when the description's dialect is `bcd396t` or `bcd996t`.
// [@ANCHOR: radioctl_unidig_handles]
pub fn handles(desc: &RigDescription) -> bool {
    matches!(
        desc.receiver.as_ref().map(|r| r.dialect.as_str()),
        Some("bcd396t" | "bcd996t")
    )
}

impl Dialect for UnidenDigitalDialect {
    // [@ANCHOR: radioctl_unidig_dialect_from_description]
    fn from_description(desc: &RigDescription) -> Result<UnidenDigitalDialect, RigError> {
        if handles(desc) {
            Ok(UnidenDigitalDialect)
        } else {
            Err(RigError::new(
                ErrorKind::Unsupported,
                "not a BCD396T or BCD996T dialect",
            ))
        }
    }

    // [@ANCHOR: radioctl_unidig_dialect_encode]
    fn encode(&mut self, desc: &RigDescription, op: &Op) -> Result<(Vec<u8>, ReplyKind), RigError> {
        let unsupported = |why: &str| RigError::new(ErrorKind::Unsupported, why.to_string());
        Ok(match op {
            Op::GetFrequency(Vfo::A) => (
                b"GLG\r".to_vec(),
                ReplyKind::Observation(Field::Frequency(Vfo::A)),
            ),
            Op::GetMode => (b"GLG\r".to_vec(), ReplyKind::Observation(Field::Mode)),
            Op::SetFrequency(Vfo::A, hz) => {
                let units = hz.0 / 100;
                if hz.0 % 100 != 0 || !(FRQ_RANGE.0..=FRQ_RANGE.1).contains(&units) {
                    return Err(unsupported(
                        "frequency out of range or not a multiple of 100 Hz",
                    ));
                }
                (qsh(&format!("{units:08}"), "").into_bytes(), ReplyKind::Ack)
            }
            Op::SetMode(mode) => {
                let code = desc
                    .mode_code(*mode)
                    .ok_or_else(|| unsupported("mode not in the rig description"))?;
                let name = MODE_NAMES
                    .iter()
                    .find(|(c, _, _)| *c == code)
                    .map(|(_, n, _)| *n)
                    .ok_or_else(|| unsupported("mode code is not a modulation"))?;
                (qsh("", name).into_bytes(), ReplyKind::Ack)
            }
            _ => {
                return Err(unsupported(
                    "not implemented for the BCD396T / BCD996T engine",
                ))
            }
        })
    }

    // [@ANCHOR: radioctl_unidig_dialect_status]
    fn status(&mut self, expecting: ReplyKind, line: &str) -> Option<Vec<Inbound>> {
        if line.is_empty() {
            return Some(Vec::new());
        }
        if matches!(line, "ERR" | "NG" | "FER" | "ORER") || line.ends_with(",NG") {
            return Some(vec![Inbound::Nak]);
        }
        (expecting == ReplyKind::Ack && line.ends_with(",OK")).then(|| vec![Inbound::Ack])
    }

    // [@ANCHOR: radioctl_unidig_dialect_decode]
    fn decode(&mut self, desc: &RigDescription, expecting: ReplyKind, line: &str) -> Vec<Inbound> {
        let unknown = || vec![Inbound::Unrecognised(line.as_bytes().to_vec())];
        let ReplyKind::Observation(field) = expecting else {
            return unknown();
        };
        let fields: Vec<&str> = line.split(',').collect();
        if fields.first() != Some(&"GLG") || fields.len() < 3 {
            return unknown();
        }
        // While nothing is received the scanner answers with empty fields: no value to report.
        if fields.get(1).is_some_and(|f| f.is_empty()) {
            return vec![Inbound::Nak];
        }
        let found = match field {
            Field::Frequency(_) => {
                frequency(fields[1]).map(|hz| Observation::Frequency(Vfo::A, hz))
            }
            Field::Mode => modulation(fields[2])
                .filter(|mode| desc.mode_code(*mode).is_some())
                .map(Observation::Mode),
            _ => None,
        };
        found.map_or_else(unknown, |o| vec![Inbound::Observation(o)])
    }
}

// [@ANCHOR: radioctl_unidig_qsh]
/// A `QSH` command line with only the frequency (eight digits) and/or the modulation filled in.
fn qsh(frequency: &str, modulation: &str) -> String {
    format!("QSH,{frequency},,{modulation},,,,,,\r")
}

// [@ANCHOR: radioctl_unidig_frequency]
/// A `GLG` frequency field: eight digits in 100 Hz, or a decimal number of megahertz.
fn frequency(field: &str) -> Option<Hz> {
    if let Some((whole, fraction)) = field.split_once('.') {
        let mhz: u64 = whole.trim().parse().ok()?;
        let digits = format!("{fraction:0<6}");
        let hz: u64 = digits.get(..6)?.parse().ok()?;
        return Some(Hz(mhz * 1_000_000 + hz));
    }
    let units: u64 = field.trim().parse().ok()?;
    Some(Hz(units * 100))
}

// [@ANCHOR: radioctl_unidig_modulation]
/// The radioctl mode of a modulation name; narrow FM and FM both read as [`Mode::Fm`].
fn modulation(name: &str) -> Option<Mode> {
    match name {
        "AM" => Some(Mode::Am),
        "FM" | "NFM" => Some(Mode::Fm),
        "WFM" | "FMB" => Some(Mode::Wfm),
        _ => None,
    }
}

/// The virtual BCD396T / BCD996T.
#[derive(Debug)]
pub struct UnidenDigitalTwin {
    desc: RigDescription,
}

impl UnidenDigitalTwin {
    // [@ANCHOR: radioctl_unidig_twin_new]
    /// The twin of a BCD396T or BCD996T description.
    pub fn new(desc: &RigDescription) -> Result<UnidenDigitalTwin, RigError> {
        UnidenDigitalDialect::from_description(desc)?;
        Ok(UnidenDigitalTwin { desc: desc.clone() })
    }

    // [@ANCHOR: radioctl_unidig_twin_reply]
    /// One answer line, ended by CR only.
    fn reply(text: &str) -> Vec<u8> {
        format!("{text}\r").into_bytes()
    }
}

impl ReceiverTwin for UnidenDigitalTwin {
    // [@ANCHOR: radioctl_unidig_twin_line]
    fn line(&mut self, state: &mut RigState, line: &str) -> Vec<u8> {
        let fields: Vec<&str> = line.split(',').collect();
        match fields.first().copied() {
            Some("GLG") if fields.len() == 1 => {
                let name = MODE_NAMES
                    .iter()
                    .find(|(_, _, m)| *m == state.mode)
                    .map_or("FM", |(_, n, _)| *n);
                Self::reply(&format!(
                    "GLG,{:08},{name},0,0,,,,{},0",
                    state.freq_a.0 / 100,
                    u8::from(state.signal > 0)
                ))
            }
            Some("QSH") if fields.len() == 10 => {
                let frq = fields[1];
                let mode = fields[3];
                let new_freq = if frq.is_empty() {
                    None
                } else {
                    match frq.parse::<u64>() {
                        Ok(units)
                            if frq.len() == 8 && (FRQ_RANGE.0..=FRQ_RANGE.1).contains(&units) =>
                        {
                            Some(Hz(units * 100))
                        }
                        _ => return Self::reply("ERR"),
                    }
                };
                let new_mode = if mode.is_empty() {
                    None
                } else {
                    match modulation(mode).filter(|m| self.desc.mode_code(*m).is_some()) {
                        Some(m) => Some(m),
                        None if mode == "AUTO" => None,
                        None => return Self::reply("ERR"),
                    }
                };
                if let Some(hz) = new_freq {
                    state.freq_a = hz;
                }
                if let Some(m) = new_mode {
                    state.mode = m;
                }
                Self::reply("QSH,OK")
            }
            Some("MDL") if fields.len() == 1 => {
                let name = self.desc.name.rsplit(' ').next().unwrap_or("");
                Self::reply(&format!("MDL,{}", name.replace('-', "")))
            }
            _ => Self::reply("ERR"),
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

    // Tests [@ANCHOR: radioctl_unidig_engine]
    // Tests [@ANCHOR: radioctl_unidig_handles]
    // Tests [@ANCHOR: radioctl_unidig_dialect_from_description]
    // Tests [@ANCHOR: radioctl_unidig_dialect_encode]
    // Tests [@ANCHOR: radioctl_unidig_qsh]
    #[test]
    fn commands_are_glg_and_qsh() {
        let mut e = engine(shipped("uniden-bcd396t")).expect("engine");
        let bytes = |e: &mut UnidenDigitalEngine, op: Op| e.encode(&op).expect("encode").bytes;
        assert_eq!(bytes(&mut e, Op::GetFrequency(Vfo::A)), b"GLG\r");
        assert_eq!(bytes(&mut e, Op::GetMode), b"GLG\r");
        assert_eq!(
            bytes(&mut e, Op::SetFrequency(Vfo::A, Hz(154_445_000))),
            b"QSH,01544450,,,,,,,,\r"
        );
        assert_eq!(bytes(&mut e, Op::SetMode(Mode::Wfm)), b"QSH,,,WFM,,,,,,\r");
        assert!(e
            .encode(&Op::SetFrequency(Vfo::A, Hz(154_445_050)))
            .is_err());
        assert!(e.encode(&Op::SetFrequency(Vfo::A, Hz(10_000_000))).is_err());
        assert!(e.encode(&Op::SetMode(Mode::Usb)).is_err());
        assert!(e.encode(&Op::GetSignal).is_err());
        assert!(e.encode(&Op::SetPtt(true)).is_err());
        assert!(
            engine(shipped("uniden-bc898t")).is_err(),
            "an older dialect"
        );
        assert!(UnidenDigitalDialect::from_description(&shipped("uniden-bc898t")).is_err());
        assert!(!handles(&shipped("uniden-bc898t")));
    }

    // Tests [@ANCHOR: radioctl_unidig_dialect_status]
    // Tests [@ANCHOR: radioctl_unidig_dialect_decode]
    // Tests [@ANCHOR: radioctl_unidig_frequency]
    // Tests [@ANCHOR: radioctl_unidig_modulation]
    #[test]
    fn glg_answers_are_read_and_errors_refuse() {
        let mut e = engine(shipped("uniden-bcd996t")).expect("engine");
        e.encode(&Op::GetFrequency(Vfo::A)).expect("get");
        assert_eq!(
            e.feed(b"GLG,08510125,NFM,0,0,SYS,GRP,CH,1,0\r", T0),
            vec![Inbound::Observation(Observation::Frequency(
                Vfo::A,
                Hz(851_012_500)
            ))]
        );
        e.encode(&Op::GetMode).expect("get");
        assert_eq!(
            e.feed(b"GLG,0851.0125,NFM,0,0,,,,1,0\r", T0),
            vec![Inbound::Observation(Observation::Mode(Mode::Fm))]
        );
        e.encode(&Op::GetMode).expect("get");
        assert_eq!(
            e.feed(b"GLG,,,,,,,,,\r", T0),
            vec![Inbound::Nak],
            "idle scanner has no value"
        );
        e.encode(&Op::GetFrequency(Vfo::A)).expect("get");
        assert!(matches!(
            e.feed(b"GLG,TGID:1234,FM,0,0,,,,1,0\r", T0).as_slice(),
            [Inbound::Unrecognised(_)]
        ));
        e.encode(&Op::SetMode(Mode::Am)).expect("set");
        assert_eq!(e.feed(b"QSH,OK\r", T0), vec![Inbound::Ack]);
        e.encode(&Op::SetMode(Mode::Am)).expect("set");
        assert_eq!(e.feed(b"QSH,NG\r", T0), vec![Inbound::Nak]);
        e.encode(&Op::SetMode(Mode::Am)).expect("set");
        assert_eq!(e.feed(b"ERR\r", T0), vec![Inbound::Nak]);
        assert_eq!(frequency("0462.5500"), Some(Hz(462_550_000)));
        assert_eq!(frequency("4625500"), Some(Hz(462_550_000)));
        assert_eq!(frequency("x"), None);
        assert_eq!(modulation("FMB"), Some(Mode::Wfm));
        assert_eq!(modulation("AUTO"), None);
    }

    // Tests [@ANCHOR: radioctl_unidig_twin_new]
    // Tests [@ANCHOR: radioctl_unidig_twin_reply]
    // Tests [@ANCHOR: radioctl_unidig_twin_line]
    #[test]
    fn the_twin_answers_like_the_document_says() {
        let desc = shipped("uniden-bcd396t");
        let mut twin = UnidenDigitalTwin::new(&desc).expect("twin");
        let mut state = crate::virtual_rig::VirtualRig::from_description(&desc)
            .expect("rig")
            .state()
            .clone();
        assert_eq!(twin.line(&mut state, "QSH,01544450,,,,,,,,"), b"QSH,OK\r");
        assert_eq!(state.freq_a, Hz(154_445_000));
        assert_eq!(twin.line(&mut state, "QSH,,,AM,,,,,,"), b"QSH,OK\r");
        assert_eq!(state.mode, Mode::Am);
        assert_eq!(twin.line(&mut state, "QSH,1544450,,,,,,,,"), b"ERR\r");
        assert_eq!(twin.line(&mut state, "QSH,00100000,,,,,,,,"), b"ERR\r");
        assert_eq!(twin.line(&mut state, "QSH,,,XM,,,,,,"), b"ERR\r");
        assert_eq!(twin.line(&mut state, "NOPE"), b"ERR\r");
        assert_eq!(twin.line(&mut state, "MDL"), b"MDL,BCD396T\r");
        let glg = String::from_utf8(twin.line(&mut state, "GLG")).expect("text");
        assert!(glg.starts_with("GLG,01544450,AM,"), "{glg}");
        assert!(UnidenDigitalTwin::new(&shipped("uniden-bc898t")).is_err());
    }
}
