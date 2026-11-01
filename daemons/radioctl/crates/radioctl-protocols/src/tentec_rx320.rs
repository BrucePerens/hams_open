// SPDX-License-Identifier: AGPL-3.0-or-later
//! Ten-Tec RX-320 DSP receiver (dialect `rx320` of the Ten-Tec protocol) and Pegasus TT-550
//! (dialect `tt550`): sans-IO client engine and virtual twin.
//!
//! **Sources.** Facts only, read from Hamlib 4.6.2 (released tarball, sha256
//! b2ac73f44dd1161e95fdee6c95276144757647bf92d7fdb369ee2fe41ed47ae8): `rigs/tentec/tentec.c`
//! (`tentec_tuning_factor_calc`, `tentec_set_freq`, `tentec_set_mode`, the filter table) and
//! `rx320.c`. No Ten-Tec document was read; nothing is verified against a radio.
//!
//! **Pegasus.** The TT-550's receive side follows the Pegasus Programmer's Reference Guide, Rev 2.0
//! (Ten-Tec, 27 pages, file `pegasus_550prg2.pdf`, sha256
//! abbcc833b190611b608a4e7f58a5c326498b12df2ccb1db385b5631ff83f583a), PDF page 12 (receive tuning
//! factors) and page 16 (mode and filter): the same `N` frame and `W` filter table as the RX-320,
//! the mode command `M` followed by a receive and a transmit mode digit, and in CW a filter
//! correction of half the bandwidth plus 200 Hz plus the CW BFO offset (700 Hz here, Hamlib's
//! default) in both the adjusted frequency and the BFO word. The guide's fine-factor line prints
//! `mod(..., 0.0025) * 2500*5.46`, which cannot give the 0 to 13 650 range of the RX-320's
//! `(offset in Hz) * 5.46`; this engine uses the latter. The Pegasus has separate transmit tuning
//! factors (`T`); they are not encoded, so the description has no PTT.
//!
//! The radio is set only: nothing it sends is read (Hamlib answers frequency and mode reads from
//! its own copy), so the description carries no read capabilities.
//!
//! * `W<n>` CR selects the n-th filter of a 34-entry table (6000 Hz down to 300 Hz, then 8000).
//! * `N` and six bytes, CR, tunes: coarse (big-endian 16 bits), fine and BFO words, from a
//!   computed tuning factor: with `a = f - 1250 + m * (c + p)` (`m` is 0 for AM and FM, -1 for
//!   LSB and CW, +1 for USB; `c` is half the filter width plus 200 Hz, or 0 in CW; `p` the pass-band
//!   shift, 0 here) the coarse word is `a / 2500 + 18000`, the fine word `(a mod 2500) * 5.46`
//!   (rounded down) and the BFO word `(c + p + b + 8000) * 2.73` (rounded down; `b` is the CW
//!   offset, 0 here).
//! * `M<c>` CR selects the mode: 0 AM, 1 USB, 2 LSB, 3 CW, 4 FM (from the description).
//!
//! The engine keeps the frequency, mode and filter itself, as Hamlib does: a frequency change
//! sends `N` with the current mode and filter; a mode change sends `W`, `N` and `M` together
//! with the filter this engine picks for the mode (AM and FM 6000 Hz, sideband 2400 Hz, CW 600 Hz:
//! an engine choice, not a radio fact). Hamlib builds these frames with `snprintf` and `%c`, so a
//! zero byte inside a frame cuts it short on its wire (the first filter's index is 0); radioctl
//! sends all the bytes.
//!
//! The twin decodes the frequency from the tuning words with the mode and filter it holds when
//! asked, so the order of `N`, `M` and `W` inside one write does not matter. The fine word is
//! rounded down by the radio, so the decoded frequency is exact for whole hertz.

use std::time::Duration;

use radioctl_core::description::RigDescription;
use radioctl_core::engine::{Inbound, ProtocolEngine, ReplyKind, Request};
use radioctl_core::error::{ErrorKind, RigError};
use radioctl_core::op::Op;
use radioctl_core::transcript::Panel;
use radioctl_core::units::{Hz, Mode, Vfo};

use crate::serial_vendor::{vendor_of, RxBuffer, TwinSnapshot, VendorTwin};

const CR: u8 = b'\r';

/// The filter widths in hertz, in the order of the radio's index byte.
pub const FILTERS: [u32; 34] = [
    6000, 5700, 5400, 5100, 4800, 4500, 4200, 3900, 3600, 3300, 3000, 2850, 2700, 2550, 2400, 2250,
    2100, 1950, 1800, 1650, 1500, 1350, 1200, 1050, 900, 750, 675, 600, 525, 450, 375, 330, 300,
    8000,
];

/// Which radio's formulas and frames.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Variant {
    /// RX-320 (Hamlib's formulas).
    Rx320,
    /// Pegasus TT-550 receive side (the programmer's guide's formulas).
    Tt550,
}

impl Variant {
    // [@ANCHOR: radioctl_tentec_rx320_variant_from_name]
    /// The variant of a dialect name.
    pub fn from_name(name: &str) -> Option<Variant> {
        match name {
            "rx320" => Some(Variant::Rx320),
            "tt550" => Some(Variant::Tt550),
            _ => None,
        }
    }
}

/// The CW BFO offset the Pegasus formulas use.
const PEGASUS_CW_BFO: i64 = 700;

fn unsupported(what: &str) -> RigError {
    RigError::new(ErrorKind::Unsupported, what.to_string())
}

// [@ANCHOR: radioctl_tentec_rx320_default_filter]
/// The filter index this engine selects for a mode.
pub fn default_filter(mode: Mode) -> u8 {
    let width = match mode {
        Mode::Cw | Mode::CwR => 600,
        Mode::Lsb | Mode::Usb => 2400,
        _ => 6000,
    };
    FILTERS
        .iter()
        .position(|w| *w == width)
        .and_then(|i| u8::try_from(i).ok())
        .unwrap_or(0)
}

// [@ANCHOR: radioctl_tentec_rx320_sideband]
/// The `m` of the tuning formula: -1 lower side (and CW), +1 upper side, 0 otherwise.
fn sideband(mode: Mode) -> i64 {
    match mode {
        Mode::Lsb | Mode::Cw | Mode::CwR => -1,
        Mode::Usb => 1,
        _ => 0,
    }
}

// [@ANCHOR: radioctl_tentec_rx320_correction]
/// The `c` of the tuning formula: half the filter width plus 200 Hz, or 0 in CW.
fn correction(variant: Variant, mode: Mode, width: u32) -> i64 {
    let cw = matches!(mode, Mode::Cw | Mode::CwR);
    match (variant, cw) {
        (Variant::Rx320, true) => 0,
        (Variant::Tt550, true) => i64::from(width / 2) + 200 + PEGASUS_CW_BFO,
        (_, false) => i64::from(width / 2) + 200,
    }
}

// [@ANCHOR: radioctl_tentec_rx320_tuning_words]
/// The coarse, fine and BFO words for `hz` in `mode` with the filter at `filter`, or `None` for an
/// unknown filter or a frequency the words cannot hold.
pub fn tuning_words(hz: Hz, mode: Mode, filter: u8) -> Option<[u16; 3]> {
    tuning_words_for(Variant::Rx320, hz, mode, filter)
}

// [@ANCHOR: radioctl_tentec_rx320_tuning_words_for]
/// The tuning words of a variant.
pub fn tuning_words_for(variant: Variant, hz: Hz, mode: Mode, filter: u8) -> Option<[u16; 3]> {
    let width = *FILTERS.get(usize::from(filter))?;
    let c = correction(variant, mode, width);
    let adjusted = i64::try_from(hz.0).ok()? - 1250 + sideband(mode) * c;
    if adjusted < 0 {
        return None;
    }
    let coarse = u16::try_from(adjusted / 2500 + 18000).ok()?;
    // 5.46 = 546 / 100, rounded down, in integers.
    let fine = u16::try_from((adjusted % 2500) * 546 / 100).ok()?;
    // 2.73 = 273 / 100.
    let bfo = u16::try_from((c + 8000) * 273 / 100).ok()?;
    Some([coarse, fine, bfo])
}

// [@ANCHOR: radioctl_tentec_rx320_frequency_of]
/// The frequency the words tune to in `mode` with the filter at `filter`.
pub fn frequency_of(words: [u16; 3], mode: Mode, filter: u8) -> Option<Hz> {
    frequency_of_for(Variant::Rx320, words, mode, filter)
}

// [@ANCHOR: radioctl_tentec_rx320_frequency_of_for]
/// The frequency tuning words of a variant tune to.
pub fn frequency_of_for(variant: Variant, words: [u16; 3], mode: Mode, filter: u8) -> Option<Hz> {
    let width = *FILTERS.get(usize::from(filter))?;
    let c = correction(variant, mode, width);
    // The fine word is floor(x * 5.46): x is the smallest integer with x * 5.46 >= fine.
    let fine = (i64::from(words[1]) * 100 + 545) / 546;
    let adjusted = (i64::from(words[0]) - 18000) * 2500 + fine;
    u64::try_from(adjusted + 1250 - sideband(mode) * c)
        .ok()
        .map(Hz)
}

// [@ANCHOR: radioctl_tentec_rx320_frame]
/// `N` and the three words, CR.
fn tune_frame(words: [u16; 3]) -> Vec<u8> {
    let mut out = vec![b'N'];
    for w in words {
        out.extend_from_slice(&w.to_be_bytes());
    }
    out.push(CR);
    out
}

/// The RX-320 client engine.
#[derive(Debug)]
pub struct Rx320Engine {
    desc: RigDescription,
    variant: Variant,
    hz: Hz,
    mode: Mode,
}

impl Rx320Engine {
    // [@ANCHOR: radioctl_tentec_rx320_engine_new]
    /// Builds an engine from an RX-320 description (`tentec`, dialect `rx320`).
    pub fn new(desc: RigDescription) -> Result<Rx320Engine, RigError> {
        let (_, name) = vendor_of(&desc)?;
        let variant =
            Variant::from_name(name).ok_or_else(|| unsupported("not an RX-320 or TT-550 file"))?;
        Ok(Rx320Engine {
            variant,
            hz: desc.default_frequency,
            mode: desc.default_mode,
            desc,
        })
    }
}

impl ProtocolEngine for Rx320Engine {
    fn description(&self) -> &RigDescription {
        &self.desc
    }

    // [@ANCHOR: radioctl_tentec_rx320_engine_encode]
    fn encode(&mut self, op: &Op) -> Result<Request, RigError> {
        if !self.desc.capabilities.has(op.required_capability()) {
            return Err(unsupported("capability not in the rig description"));
        }
        let bytes = match op {
            Op::SetFrequency(Vfo::A, hz) => {
                let words =
                    tuning_words_for(self.variant, *hz, self.mode, default_filter(self.mode))
                        .ok_or_else(|| unsupported("frequency outside the tuning words"))?;
                self.hz = *hz;
                tune_frame(words)
            }
            Op::SetMode(mode) => {
                let code = self
                    .desc
                    .mode_code(*mode)
                    .ok_or_else(|| unsupported("mode not in the description"))?;
                let filter = default_filter(*mode);
                let words = tuning_words_for(self.variant, self.hz, *mode, filter)
                    .ok_or_else(|| unsupported("frequency outside the tuning words"))?;
                self.mode = *mode;
                let mut out = vec![b'W', filter, CR];
                out.extend(tune_frame(words));
                if self.variant == Variant::Tt550 {
                    // Receive and transmit mode digits; the transmit side follows the receive.
                    out.extend_from_slice(&[b'M', code, code, CR]);
                } else {
                    out.extend_from_slice(&[b'M', code, CR]);
                }
                out
            }
            _ => return Err(unsupported("not implemented for the RX-320")),
        };
        Ok(Request {
            bytes,
            reply: ReplyKind::None,
        })
    }

    // [@ANCHOR: radioctl_tentec_rx320_engine_feed]
    fn feed(&mut self, bytes: &[u8], _now: Duration) -> Vec<Inbound> {
        if bytes.is_empty() {
            Vec::new()
        } else {
            vec![Inbound::Unrecognised(bytes.to_vec())]
        }
    }
}

/// A virtual RX-320.
#[derive(Debug)]
pub struct Rx320Twin {
    desc: RigDescription,
    variant: Variant,
    rx: RxBuffer,
    words: Option<[u16; 3]>,
    hz: Hz,
    mode: Mode,
    filter: u8,
}

impl Rx320Twin {
    // [@ANCHOR: radioctl_tentec_rx320_twin_new]
    /// A twin in the description's default state.
    pub fn new(desc: &RigDescription) -> Result<Rx320Twin, RigError> {
        let (_, name) = vendor_of(desc)?;
        let variant =
            Variant::from_name(name).ok_or_else(|| unsupported("not an RX-320 or TT-550 file"))?;
        Ok(Rx320Twin {
            variant,
            desc: desc.clone(),
            rx: RxBuffer::default(),
            words: None,
            hz: desc.default_frequency,
            mode: desc.default_mode,
            filter: default_filter(desc.default_mode),
        })
    }

    /// The frequency now: decoded from the last tuning words with the mode and filter held.
    fn current_hz(&self) -> Hz {
        self.words
            .and_then(|w| frequency_of_for(self.variant, w, self.mode, self.filter))
            .unwrap_or(self.hz)
    }
}

impl VendorTwin for Rx320Twin {
    // [@ANCHOR: radioctl_tentec_rx320_twin_snapshot]
    fn snapshot(&self) -> Option<TwinSnapshot> {
        Some(TwinSnapshot {
            frequency: self.current_hz(),
            mode_code: self.desc.mode_code(self.mode),
            ptt: false,
        })
    }

    // [@ANCHOR: radioctl_tentec_rx320_twin_feed]
    fn feed(&mut self, bytes: &[u8], now: Duration) -> Vec<u8> {
        self.rx.push(bytes, now);
        let mut out = Vec::new();
        loop {
            let held = self.rx.peek();
            let Some(&first) = held.first() else { break };
            let need = match first {
                b'M' if self.variant == Variant::Tt550 => 4,
                // Hamlib's Pegasus open restarts the radio (`XX`) and starts its radio program
                // (`P1`); the guide gives the answers " DSP START" and " RADIO START".
                b'X' | b'P' if self.variant == Variant::Tt550 => 3,
                b'W' | b'M' | b'G' => 3,
                b'N' => 8,
                _ => 1,
            };
            if held.len() < need {
                break;
            }
            let frame = self.rx.take(need).unwrap_or_default();
            if need > 1 && frame.last() != Some(&CR) {
                // Not a frame after all: resynchronise on the next byte.
                self.rx.push(&frame[1..], now);
                continue;
            }
            match (first, frame.as_slice()) {
                (b'X', b"XX\r") => out.extend_from_slice(b" DSP START"),
                (b'P', b"P1\r") => out.extend_from_slice(b" RADIO START"),
                (b'W', [_, n, _]) if usize::from(*n) < FILTERS.len() => self.filter = *n,
                (b'N', [_, a, b, c, d, e, f, _]) => {
                    self.words = Some([
                        u16::from_be_bytes([*a, *b]),
                        u16::from_be_bytes([*c, *d]),
                        u16::from_be_bytes([*e, *f]),
                    ]);
                }
                (b'M', [_, c, _] | [_, c, _, _]) => {
                    if let Some(m) = self.desc.code_mode(*c) {
                        self.mode = m;
                    }
                }
                _ => {}
            }
        }
        out
    }

    // [@ANCHOR: radioctl_tentec_rx320_twin_front_panel]
    fn front_panel(&mut self, panel: Panel) -> Result<Vec<u8>, String> {
        match panel {
            Panel::Frequency(hz) => {
                self.hz = hz;
                self.words = tuning_words_for(self.variant, hz, self.mode, self.filter);
            }
            Panel::Mode(mode) => self.mode = mode,
            other => return Err(format!("an RX-320 twin has no {other:?} knob")),
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
            "{}/../../rigs/tentec-rx-320.toml",
            env!("CARGO_MANIFEST_DIR")
        );
        let text = std::fs::read_to_string(path).expect("rig file");
        radioctl_core::rigfile::parse(&text)
            .expect("loads")
            .description
    }

    // Tests [@ANCHOR: radioctl_tentec_rx320_tuning_words]
    // Tests [@ANCHOR: radioctl_tentec_rx320_frequency_of]
    // Tests [@ANCHOR: radioctl_tentec_rx320_default_filter]
    // Tests [@ANCHOR: radioctl_tentec_rx320_sideband]
    // Tests [@ANCHOR: radioctl_tentec_rx320_correction]
    // Tests [@ANCHOR: radioctl_tentec_rx320_frame]
    #[test]
    fn the_tuning_words_round_trip_for_whole_hertz() {
        for mode in [Mode::Am, Mode::Usb, Mode::Lsb, Mode::Cw, Mode::Fm] {
            let filter = default_filter(mode);
            for hz in [
                100_000u64, 1_800_000, 7_074_000, 14_074_123, 29_999_999, 12_345_679,
            ] {
                let words = tuning_words(Hz(hz), mode, filter).expect("words");
                assert_eq!(
                    frequency_of(words, mode, filter),
                    Some(Hz(hz)),
                    "{mode:?} {hz}"
                );
            }
        }
        // USB at 14.074 MHz, filter index 14 (2400 Hz): c = 1400, a = 14_074_000 - 1250 + 1400.
        assert_eq!(
            tuning_words(Hz(14_074_000), Mode::Usb, 14),
            Some([23_629, 9_009, 25_662])
        );
        assert_eq!(tuning_words(Hz(10), Mode::Lsb, 14), None);
        assert_eq!(tuning_words(Hz(7_000_000), Mode::Usb, 99), None);
        assert_eq!(default_filter(Mode::Cw), 27);
        assert_eq!(default_filter(Mode::Am), 0);
        assert_eq!(default_filter(Mode::Usb), 14);
    }

    // Tests [@ANCHOR: radioctl_tentec_rx320_engine_new]
    #[test]
    fn engine_and_twin_refuse_other_dialects() {
        assert!(Rx320Engine::new(RigDescription::ts2000()).is_err());
        assert!(Rx320Twin::new(&RigDescription::ts2000()).is_err());
    }

    // Tests [@ANCHOR: radioctl_tentec_rx320_engine_encode]
    // Tests [@ANCHOR: radioctl_tentec_rx320_engine_feed]
    // Tests [@ANCHOR: radioctl_tentec_rx320_twin_new]
    // Tests [@ANCHOR: radioctl_tentec_rx320_twin_feed]
    // Tests [@ANCHOR: radioctl_tentec_rx320_twin_snapshot]
    // Tests [@ANCHOR: radioctl_tentec_rx320_twin_front_panel]
    #[test]
    fn the_engine_tunes_the_twin_within_a_hertz() {
        let desc = shipped();
        let mut e = Rx320Engine::new(desc.clone()).expect("engine");
        let mut t = Rx320Twin::new(&desc).expect("twin");
        let send = |e: &mut Rx320Engine, t: &mut Rx320Twin, op: Op| {
            let r = e.encode(&op).expect("encodes");
            assert_eq!(r.reply, ReplyKind::None);
            assert!(t.feed(&r.bytes, T0).is_empty());
            r.bytes
        };
        for (mode, hz) in [
            (Mode::Usb, 14_074_000),
            (Mode::Lsb, 3_573_000),
            (Mode::Cw, 7_030_000),
            (Mode::Am, 9_580_000),
            (Mode::Fm, 29_600_000),
        ] {
            let wire = send(&mut e, &mut t, Op::SetMode(mode));
            assert!(wire.starts_with(b"W") && wire.ends_with(b"\r"), "{wire:?}");
            send(&mut e, &mut t, Op::SetFrequency(Vfo::A, Hz(hz)));
            let snap = t.snapshot().expect("snapshot");
            assert_eq!(snap.frequency, Hz(hz), "{mode:?}");
            assert_eq!(snap.mode_code.and_then(|c| desc.code_mode(c)), Some(mode));
        }
        // A mode change keeps the frequency the radio is tuned to.
        send(&mut e, &mut t, Op::SetMode(Mode::Usb));
        assert_eq!(t.snapshot().expect("snapshot").frequency, Hz(29_600_000));
        // A write split across reads, and garbage between frames, still parse.
        let r = e
            .encode(&Op::SetFrequency(Vfo::A, Hz(5_000_000)))
            .expect("encodes");
        t.feed(b"?", T0);
        t.feed(&r.bytes[..3], T0);
        t.feed(&r.bytes[3..], T0);
        assert_eq!(t.snapshot().expect("snapshot").frequency, Hz(5_000_000));
        t.front_panel(Panel::Frequency(Hz(6_000_000)))
            .expect("tunes");
        assert_eq!(t.snapshot().expect("snapshot").frequency, Hz(6_000_000));
        assert!(t.front_panel(Panel::Ptt(true)).is_err());
        assert!(e.encode(&Op::GetFrequency(Vfo::A)).is_err());
        assert!(e.encode(&Op::SetPtt(true)).is_err());
        assert!(e.encode(&Op::SetMode(Mode::Rtty)).is_err());
        assert!(matches!(
            e.feed(b"x", T0).as_slice(),
            [Inbound::Unrecognised(_)]
        ));
        assert!(e.feed(b"", T0).is_empty());
    }

    // Tests [@ANCHOR: radioctl_tentec_rx320_variant_from_name]
    // Tests [@ANCHOR: radioctl_tentec_rx320_tuning_words_for]
    // Tests [@ANCHOR: radioctl_tentec_rx320_frequency_of_for]
    #[test]
    fn the_pegasus_adds_the_cw_offset_and_takes_two_mode_digits() {
        assert_eq!(Variant::from_name("tt550"), Some(Variant::Tt550));
        assert_eq!(Variant::from_name("rx320"), Some(Variant::Rx320));
        assert_eq!(Variant::from_name("x"), None);
        // CW at 7.030 MHz with the 600 Hz filter (index 27): the RX-320 adds nothing, the Pegasus
        // adds half the bandwidth plus 200 Hz plus the 700 Hz BFO offset (1200 Hz) to the
        // adjusted frequency, subtracted on the lower side: a = 7_030_000 - 1250 - 1200.
        let rx = tuning_words_for(Variant::Rx320, Hz(7_030_000), Mode::Cw, 27).expect("rx320");
        let pg = tuning_words_for(Variant::Tt550, Hz(7_030_000), Mode::Cw, 27).expect("tt550");
        assert_eq!(rx, [2811 + 18000, 6825, 21_840]);
        assert_eq!(pg, [2811 + 18000, 273, 25_116]);
        for hz in [100_000u64, 7_030_000, 14_074_321, 29_999_999] {
            for mode in [Mode::Am, Mode::Usb, Mode::Lsb, Mode::Cw, Mode::Fm] {
                let w = tuning_words_for(Variant::Tt550, Hz(hz), mode, default_filter(mode))
                    .expect("words");
                assert_eq!(
                    frequency_of_for(Variant::Tt550, w, mode, default_filter(mode)),
                    Some(Hz(hz)),
                    "{mode:?} {hz}"
                );
            }
        }
        let path = format!(
            "{}/../../rigs/tentec-tt-550.toml",
            env!("CARGO_MANIFEST_DIR")
        );
        let text = std::fs::read_to_string(path).expect("rig file");
        let desc = radioctl_core::rigfile::parse(&text)
            .expect("loads")
            .description;
        let mut e = Rx320Engine::new(desc.clone()).expect("engine");
        let mut t = Rx320Twin::new(&desc).expect("twin");
        let r = e.encode(&Op::SetMode(Mode::Cw)).expect("encodes");
        assert!(
            r.bytes.windows(4).any(|w| w == [b'M', b'3', b'3', CR]),
            "{:?}",
            r.bytes
        );
        t.feed(&r.bytes, T0);
        let r = e
            .encode(&Op::SetFrequency(Vfo::A, Hz(7_030_000)))
            .expect("encodes");
        t.feed(&r.bytes, T0);
        assert_eq!(t.feed(b"XX\r", T0), b" DSP START");
        assert_eq!(t.feed(b"P1\r", T0), b" RADIO START");
        let snap = t.snapshot().expect("snapshot");
        assert_eq!(snap.frequency, Hz(7_030_000));
        assert_eq!(
            snap.mode_code.and_then(|c| desc.code_mode(c)),
            Some(Mode::Cw)
        );
    }
}
