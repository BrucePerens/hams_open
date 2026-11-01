// SPDX-License-Identifier: AGPL-3.0-or-later
//! Dorji DRA818U and DRA818V FM transceiver modules: sans-IO client engine and virtual twin.
//!
//! **Sources.** The DRA818U datasheet (Dorji, revision 1.20, July 2016, 10 pages, file
//! `dra818u_datasheet_direnc.pdf` from https://pdf.direnc.net/upload/dra818u.pdf, sha256
//! 15489d34ebd9bc8e7ad6afae96a1e87909972502c9f78a41c3e289bb0ffc9743), pages 4 and 5, read for
//! the serial format and the group-setting command; Hamlib 4.6.2 (released tarball, sha256
//! b2ac73f44dd1161e95fdee6c95276144757647bf92d7fdb369ee2fe41ed47ae8) `rigs/dorji/dra818.c` for
//! what Hamlib sends (the DRA818V's range is Hamlib's, no DRA818V document was found).
//!
//! The modules speak ASCII lines ended by CR LF at 9600 baud, 8N1. They have no read commands for
//! the frequency: Hamlib (and this engine) keep what they last set.
//!
//! * `AT+DMOCONNECT` is answered `+DMOCONNECT:0`; `AT+DMOSETVOLUME=<1-8>` is answered
//!   `+DMOSETVOLUME:0`.
//! * `AT+DMOSETGROUP=<bw>,<tx MHz>,<rx MHz>,<tx tone>,<squelch>,<rx tone>` sets everything at
//!   once and is answered `+DMOSETGROUP:0` (success) or `:1` (a value out of range). `bw` is 0 for
//!   12.5 kHz and 1 for 25 kHz channels; the frequencies have three digits, a dot and four
//!   decimals (a 12.5 kHz grid); the tones are four characters (`0000` for none); the squelch
//!   level is 0 to 8.
//!
//! The engine rounds a frequency to the nearest 12.5 kHz channel, as Hamlib does, and sends the
//! squelch level 4 and no tones that Hamlib starts with. Mode codes: `0` for the 12.5 kHz channel
//! (radioctl `fm`), `1` for the 25 kHz channel (radioctl `wfm`).

use std::time::Duration;

use radioctl_core::description::RigDescription;
use radioctl_core::engine::{Inbound, ProtocolEngine, ReplyKind, Request};
use radioctl_core::error::{ErrorKind, RigError};
use radioctl_core::op::Op;
use radioctl_core::transcript::Panel;
use radioctl_core::units::{Hz, Vfo};

use crate::serial_vendor::{vendor_of, RxBuffer, TwinSnapshot, VendorTwin};

const LF: u8 = b'\n';
/// The squelch level Hamlib starts with.
const SQUELCH: u8 = 4;

fn unsupported(what: &str) -> RigError {
    RigError::new(ErrorKind::Unsupported, what.to_string())
}

/// The two modules.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Band {
    /// DRA818U, 400 to 480 MHz.
    Uhf,
    /// DRA818V, 134 to 174 MHz.
    Vhf,
}

impl Band {
    // [@ANCHOR: radioctl_dorji_band_from_name]
    /// The band of a dialect name (`dra818u`, `dra818v`).
    pub fn from_name(name: &str) -> Option<Band> {
        match name {
            "dra818u" => Some(Band::Uhf),
            "dra818v" => Some(Band::Vhf),
            _ => None,
        }
    }

    // [@ANCHOR: radioctl_dorji_band_contains]
    /// Whether the module can tune `hz`.
    pub fn contains(self, hz: u64) -> bool {
        match self {
            Band::Uhf => (400_000_000..=480_000_000).contains(&hz),
            Band::Vhf => (134_000_000..=174_000_000).contains(&hz),
        }
    }
}

// [@ANCHOR: radioctl_dorji_channel]
/// The frequency rounded to the nearest 12.5 kHz channel.
pub fn channel(hz: Hz) -> Hz {
    Hz((hz.0 + 6250) / 12_500 * 12_500)
}

// [@ANCHOR: radioctl_dorji_mhz_text]
/// A frequency as `fff.ffff` megahertz.
pub fn mhz_text(hz: Hz) -> String {
    format!("{:03}.{:04}", hz.0 / 1_000_000, (hz.0 % 1_000_000) / 100)
}

// [@ANCHOR: radioctl_dorji_parse_mhz]
/// The frequency of `fff.ffff`, or `None`.
pub fn parse_mhz(text: &str) -> Option<Hz> {
    let (whole, frac) = text.split_once('.')?;
    if whole.len() != 3 || frac.len() != 4 {
        return None;
    }
    let w: u64 = whole.parse().ok()?;
    let f: u64 = frac.parse().ok()?;
    Some(Hz(w * 1_000_000 + f * 100))
}

/// The DRA818 client engine.
#[derive(Debug)]
pub struct DorjiEngine {
    desc: RigDescription,
    band: Band,
    hz: Hz,
    wide: bool,
    rx: RxBuffer,
    waiting: bool,
}

impl DorjiEngine {
    // [@ANCHOR: radioctl_dorji_engine_new]
    /// Builds an engine from a Dorji vendor description.
    pub fn new(desc: RigDescription) -> Result<DorjiEngine, RigError> {
        let (_, name) = vendor_of(&desc)?;
        let band = Band::from_name(name)
            .ok_or_else(|| unsupported("unknown Dorji dialect in the description"))?;
        Ok(DorjiEngine {
            hz: channel(desc.default_frequency),
            desc,
            band,
            wide: false,
            rx: RxBuffer::default(),
            waiting: false,
        })
    }

    fn group(&self) -> Vec<u8> {
        format!(
            "AT+DMOSETGROUP={},{},{},0000,{SQUELCH},0000\r\n",
            u8::from(self.wide),
            mhz_text(self.hz),
            mhz_text(self.hz)
        )
        .into_bytes()
    }
}

impl ProtocolEngine for DorjiEngine {
    fn description(&self) -> &RigDescription {
        &self.desc
    }

    // [@ANCHOR: radioctl_dorji_engine_encode]
    fn encode(&mut self, op: &Op) -> Result<Request, RigError> {
        if !self.desc.capabilities.has(op.required_capability()) {
            return Err(unsupported("capability not in the rig description"));
        }
        match op {
            Op::SetFrequency(Vfo::A, hz) => {
                let c = channel(*hz);
                if !self.band.contains(c.0) {
                    return Err(unsupported("frequency outside the module's band"));
                }
                self.hz = c;
            }
            Op::SetMode(mode) => {
                let code = self
                    .desc
                    .mode_code(*mode)
                    .ok_or_else(|| unsupported("mode not in the description"))?;
                self.wide = code == b'1';
            }
            _ => return Err(unsupported("not implemented for the Dorji engine")),
        }
        self.waiting = true;
        self.rx.take_all();
        Ok(Request {
            bytes: self.group(),
            reply: ReplyKind::Ack,
        })
    }

    // [@ANCHOR: radioctl_dorji_engine_feed]
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
            let waiting = std::mem::replace(&mut self.waiting, false);
            out.push(match (waiting, line.as_str()) {
                (true, "+DMOSETGROUP:0") => Inbound::Ack,
                (true, "+DMOSETGROUP:1") => Inbound::Nak,
                _ => Inbound::Unrecognised(raw),
            });
        }
        out
    }
}

/// A virtual DRA818.
#[derive(Debug)]
pub struct DorjiTwin {
    band: Band,
    rx: RxBuffer,
    rx_hz: Hz,
    tx_hz: Hz,
    wide: bool,
}

impl DorjiTwin {
    // [@ANCHOR: radioctl_dorji_twin_new]
    /// A twin in the description's default state.
    pub fn new(desc: &RigDescription) -> Result<DorjiTwin, RigError> {
        let (_, name) = vendor_of(desc)?;
        let band = Band::from_name(name)
            .ok_or_else(|| unsupported("unknown Dorji dialect in the description"))?;
        Ok(DorjiTwin {
            band,
            rx: RxBuffer::default(),
            rx_hz: channel(desc.default_frequency),
            tx_hz: channel(desc.default_frequency),
            wide: false,
        })
    }

    // [@ANCHOR: radioctl_dorji_twin_answer]
    /// The answer line to one command line, or `None` for a line the module does not know.
    fn answer(&mut self, line: &str) -> Option<&'static str> {
        if line == "AT+DMOCONNECT" {
            return Some("+DMOCONNECT:0");
        }
        if let Some(v) = line.strip_prefix("AT+DMOSETVOLUME=") {
            return Some(match v.parse::<u8>() {
                Ok(1..=8) => "+DMOSETVOLUME:0",
                _ => "+DMOSETVOLUME:1",
            });
        }
        let args = line.strip_prefix("AT+DMOSETGROUP=")?;
        let parts: Vec<&str> = args.split(',').collect();
        let [bw, tx, rx, _tx_tone, sq, _rx_tone] = parts.as_slice() else {
            return Some("+DMOSETGROUP:1");
        };
        let ok = matches!(
            (bw.parse::<u8>(), parse_mhz(tx), parse_mhz(rx), sq.parse::<u8>()),
            (Ok(0 | 1), Some(t), Some(r), Ok(0..=8))
                if self.band.contains(t.0) && self.band.contains(r.0)
        );
        if !ok {
            return Some("+DMOSETGROUP:1");
        }
        self.wide = *bw == "1";
        self.tx_hz = parse_mhz(tx)?;
        self.rx_hz = parse_mhz(rx)?;
        Some("+DMOSETGROUP:0")
    }
}

impl VendorTwin for DorjiTwin {
    // [@ANCHOR: radioctl_dorji_twin_snapshot]
    fn snapshot(&self) -> Option<TwinSnapshot> {
        Some(TwinSnapshot {
            frequency: self.rx_hz,
            mode_code: Some(if self.wide { b'1' } else { b'0' }),
            ptt: false,
        })
    }

    // [@ANCHOR: radioctl_dorji_twin_feed]
    fn feed(&mut self, bytes: &[u8], now: Duration) -> Vec<u8> {
        self.rx.push(bytes, now);
        let mut out = Vec::new();
        while let Some(raw) = self.rx.take_line(LF) {
            let line = String::from_utf8_lossy(&raw).trim().to_string();
            if let Some(answer) = self.answer(&line) {
                out.extend(format!("{answer}\r\n").into_bytes());
            }
        }
        out
    }

    // [@ANCHOR: radioctl_dorji_twin_front_panel]
    fn front_panel(&mut self, panel: Panel) -> Result<Vec<u8>, String> {
        Err(format!("a Dorji module has no {panel:?} knob"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use radioctl_core::units::Mode;

    const T0: Duration = Duration::from_secs(1);

    fn run(e: &mut DorjiEngine, t: &mut DorjiTwin, op: Op) -> (Vec<u8>, Vec<Inbound>) {
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

    // Tests [@ANCHOR: radioctl_dorji_band_from_name]
    // Tests [@ANCHOR: radioctl_dorji_band_contains]
    // Tests [@ANCHOR: radioctl_dorji_channel]
    // Tests [@ANCHOR: radioctl_dorji_mhz_text]
    // Tests [@ANCHOR: radioctl_dorji_parse_mhz]
    #[test]
    fn channels_text_and_bands() {
        assert_eq!(Band::from_name("dra818v"), Some(Band::Vhf));
        assert_eq!(Band::from_name("x"), None);
        assert!(Band::Uhf.contains(415_125_000));
        assert!(!Band::Uhf.contains(145_000_000));
        assert!(Band::Vhf.contains(145_000_000));
        assert_eq!(channel(Hz(145_501_000)), Hz(145_500_000));
        assert_eq!(channel(Hz(145_507_000)), Hz(145_512_500));
        assert_eq!(mhz_text(Hz(415_125_000)), "415.1250");
        assert_eq!(mhz_text(Hz(145_500_000)), "145.5000");
        assert_eq!(parse_mhz("415.1250"), Some(Hz(415_125_000)));
        assert_eq!(parse_mhz("415.125"), None);
        assert_eq!(parse_mhz("41.1250"), None);
        assert_eq!(parse_mhz("abc.defg"), None);
    }

    // Tests [@ANCHOR: radioctl_dorji_engine_new]
    #[test]
    fn other_families_are_refused() {
        assert!(DorjiEngine::new(RigDescription::ts2000()).is_err());
        assert!(DorjiTwin::new(&RigDescription::ts2000()).is_err());
    }

    // Tests [@ANCHOR: radioctl_dorji_engine_encode]
    // Tests [@ANCHOR: radioctl_dorji_engine_feed]
    // Tests [@ANCHOR: radioctl_dorji_twin_new]
    // Tests [@ANCHOR: radioctl_dorji_twin_answer]
    // Tests [@ANCHOR: radioctl_dorji_twin_feed]
    // Tests [@ANCHOR: radioctl_dorji_twin_snapshot]
    // Tests [@ANCHOR: radioctl_dorji_twin_front_panel]
    #[test]
    fn both_modules_set_the_group_and_hear_the_result() {
        for (id, hz) in [
            ("dorji-dra818u", 435_500_000),
            ("dorji-dra818v", 145_500_000),
        ] {
            let desc = shipped(id);
            let mut e = DorjiEngine::new(desc.clone()).expect("engine");
            let mut t = DorjiTwin::new(&desc).expect("twin");
            let (wire, seen) = run(&mut e, &mut t, Op::SetFrequency(Vfo::A, Hz(hz + 3_000)));
            let text = String::from_utf8(wire).expect("ascii");
            assert_eq!(
                text,
                format!("AT+DMOSETGROUP=0,{0},{0},0000,4,0000\r\n", mhz_text(Hz(hz))),
                "{id}"
            );
            assert_eq!(seen, vec![Inbound::Ack], "{id}");
            assert_eq!(t.snapshot().expect("snapshot").frequency, Hz(hz));
            assert_eq!(
                run(&mut e, &mut t, Op::SetMode(Mode::Wfm)).1,
                vec![Inbound::Ack]
            );
            assert_eq!(t.snapshot().expect("snapshot").mode_code, Some(b'1'));
            assert_eq!(
                run(&mut e, &mut t, Op::SetMode(Mode::Fm)).1,
                vec![Inbound::Ack]
            );
            assert_eq!(t.snapshot().expect("snapshot").mode_code, Some(b'0'));
            // Out of the module's band: the engine refuses; a bad group is refused by the twin.
            assert!(e
                .encode(&Op::SetFrequency(Vfo::A, Hz(1_000_000_000)))
                .is_err());
            assert_eq!(
                t.feed(b"AT+DMOSETGROUP=0,001.0000,001.0000,0000,4,0000\r\n", T0),
                b"+DMOSETGROUP:1\r\n"
            );
            assert_eq!(t.feed(b"AT+DMOSETGROUP=0,1\r\n", T0), b"+DMOSETGROUP:1\r\n");
            assert_eq!(t.feed(b"AT+DMOCONNECT\r\n", T0), b"+DMOCONNECT:0\r\n");
            assert_eq!(t.feed(b"AT+DMOSETVOLUME=6\r\n", T0), b"+DMOSETVOLUME:0\r\n");
            assert_eq!(t.feed(b"AT+DMOSETVOLUME=9\r\n", T0), b"+DMOSETVOLUME:1\r\n");
            assert!(t.feed(b"HELLO\r\n", T0).is_empty());
            assert!(t.front_panel(Panel::Ptt(true)).is_err());
            assert!(e.encode(&Op::SetPtt(true)).is_err());
            e.encode(&Op::SetMode(Mode::Fm)).expect("encodes");
            assert_eq!(e.feed(b"+DMOSETGROUP:1\r\n", T0), vec![Inbound::Nak]);
            assert!(matches!(
                e.feed(b"+DMOSETGROUP:0\r\n", T0).as_slice(),
                [Inbound::Unrecognised(_)]
            ));
        }
    }
}
