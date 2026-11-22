// SPDX-License-Identifier: AGPL-3.0-or-later
//! TAPR DSP-10 2 m transceiver: sans-IO client engine and virtual twin: mode, frequency and PTT.
//!
//! **Source.** "Notes on the PC/DSP interface for the DSP-10 transceiver", Version 2.00, Bob Larkin
//! W7PUA, 10 Feb 2001 (`pc_dsp2.txt`, from http://janbob.com/electron/dsp10/pc_dsp2.txt, stored
//! outside the repositories, sha256 5bb540cfa331306c7fb768e13ce3fc2f0384823f640dba0bc72e138f35387746),
//! sections "PC to DSP Commands" (the six-byte frame and Command 77 'M') and "Receive status".
//! Valid for the DSP program UHF3.EXE 2.00, which the PC must have loaded first; the 9600 baud
//! serial port is the control path.
//!
//! * Every command is six bytes: ESC (`$`, 0x24), the command byte, four data bytes. The DSP does
//!   not answer a command; it streams status blocks (`ESC 'A'`, 20 bytes) that this engine ignores.
//! * Command 77 ('M'): `ESC 77 mode sb mikel mikeh`, `mode` 0 single sideband, 1 CW, 2 FM; `sb` 0 LSB,
//!   1 USB, 2 I-Q binaural; the microphone gain is 4 * (256 * mikeh + mikel).
//!
//! **Frequency** (not in pc_dsp2.txt, which does not say what the synthesizer word means). From the
//! GPL PC program source (`UHF396.ZIP`, from http://janbob.com/electron/dsp10/UHF396.ZIP, sha256
//! 482bf48728d980ae9633036871abf733d6ae57120264b615a1bb9a25025ef967, stored outside the
//! repositories), file `UHFA.C` v3.96, functions `new_freq` and `send_shift_data`, and `UHFA.H`
//! (`R10MHZ`): the receive frequency is quantised to a 5 kHz first-I-F step (rounding offset
//! +250 Hz for the DSP's USB side i.e. LSB on air, +4000 Hz for USB and CW, +2500 Hz for FM),
//! the 126 MHz synthesizer is `step - 19.665 MHz` in 5 kHz units `N`, sent as the 19-bit word
//! `(a << 1) | (b << 8)` with `b = N / 64`, `a = N % 64`, shifted out by three Command 8 words
//! (3 bits, 16 bits, then the control byte and the RF-gain byte) and latched by Command 7 with
//! `data` bit 0 low then high; the third LO is `349.5253333 * (step - f + 15000)` as a 24-bit
//! phase increment in Command 7 (`xl`, `lo`, `hi`; the v2.00 DSP takes `lo`, `hi`). The reference
//! divider word `R10MHZ` (0x08FA1) is sent once at open. 2 m has no transverter offset.
//!
//! **PTT** (`UHFA.C` `t_r_toggle`, hardware branch). Keying is a timed sequence ([`ProtocolEngine::encode_stages`]):
//! Command 10 audio gain 0, then the shift-register control byte `0x10` (antenna relay to transmit),
//! 50 ms, `0x18` (power amplifier on), 75 ms, `0x1A` (hardware into transmit), then Command 84 'T'
//! `ESC 84 3 2 pwrl pwrh` (DSP to transmit, power word 16421 = 32767 * 10^(-6/20), the PC program's
//! default `xmit_power` 94) and 10 ms. Each control byte goes out as `send_shift_data(R10MHZ)`: three
//! Command 8 words, Command 7 enable low, 10 ms, Command 7 enable high; the RF-gain byte is 0
//! while transmitting. Unkey: Command 84 `ESC 84 2 2 ...`, control bytes `0x18`, 25 ms, `0x10`, 25 ms,
//! `0x00`, 2 ms, then `0x04` with the RF-gain byte restored, then a retune to the current frequency.
//! The microphone-gain de-thump, the end beep, FM tones and the PC program's own sequencing delays
//! beyond these are not sent. The delays 50, 75, 25, 25 and 2 ms are the PC program's defaults
//! (`dly_ant2amp`, `dly_amp2xmit`, `dly_xmit2amp`, `dly_amp2ant`); no radio has run this.
//!
//! The mode codes in the description are `0` LSB, `1` USB, `2` CW, `3` FM (ours). The microphone gain word sent with every mode command is [`MIKE_WORD`] (ours; the
//! document gives no default).

use std::time::Duration;

use radioctl_core::description::RigDescription;
use radioctl_core::engine::{Inbound, ProtocolEngine, ReplyKind, Request, Stage};
use radioctl_core::error::{ErrorKind, RigError};
use radioctl_core::op::Op;
use radioctl_core::transcript::Panel;
use radioctl_core::units::{Hz, Vfo};

use crate::serial_vendor::{vendor_of, TwinSnapshot, VendorTwin};

/// The escape byte that starts every command (document: "ESC ... is '$' or 24 hex").
pub const ESC: u8 = 0x24;
/// Command 77 ('M'): mode, sideband and microphone gain.
pub const CMD_MODE: u8 = 77;
/// Frame length: ESC, command, four data bytes.
pub const FRAME_LEN: usize = 6;
/// The microphone gain word `256 * mikeh + mikel` we send (gain 4 * this).
pub const MIKE_WORD: u16 = 1;
/// The synthesizer reference-divider word sent at open (`UHFA.H` `R10MHZ`).
pub const R10MHZ: u32 = 0x08FA1;
/// Command 7: third LO phase increment and the synthesizer enable (`PF7`).
pub const CMD_LO: u8 = 7;
/// Command 8: shifts a 16-bit word into the synthesizer shift registers.
pub const CMD_SHIFT: u8 = 8;
/// Control byte of the shift registers in receive (`UHFA.C` `shift_reg` 0x04).
pub const SHIFT_RECEIVE: u8 = 0x04;
/// RF-gain byte of the shift registers at 0 dB (`UHFA.C` `rfg[0].gainbyte`).
pub const RF_GAIN_BYTE: u8 = 0x88;
/// Third-LO increment sent while keyed or unkeyed when no frequency was set yet (`UHFA.C` defaults).
const DPH_DEFAULT: u32 = 0x4AA6 << 8;
/// Shift-register control bytes of the transmit sequence (`UHFA.C` `t_r_toggle`).
const TX_ANTENNA: u8 = 0x10;
const TX_AMP: u8 = 0x18;
const TX_ON: u8 = 0x1A;
const RX_ANTENNA_OFF: u8 = 0x00;
/// Command 84 'T': DSP transmit/receive and power.
pub const CMD_TX: u8 = 84;
/// Transmit power word 4 * ... `fgain[94] * 32767` (`UHFA.C` default `xmit_power` 94).
const POWER_WORD: u16 = 16421;
/// The radio's range, 144 to 148 MHz.
pub const RANGE: std::ops::RangeInclusive<u64> = 144_000_000..=148_000_000;
/// Third LO phase increment per Hz (2^32 / 24000 / 2^8 ... `UHFA.C` `new_freq`: 349.525333).
const DPH_PER_HZ: f64 = 349.525_333_333_333;

fn unsupported(what: &str) -> RigError {
    RigError::new(ErrorKind::Unsupported, what.to_string())
}

// [@ANCHOR: radioctl_dsp10_mode_bytes]
/// The `mode` and `sb` bytes of Command 77 for a description mode code; `None` for a code the
/// DSP has no mode for. On 2 m the DSP's I-F sideband is the opposite of the one on the air
/// (the upper-side 19.68 MHz conversion): USB, CW and FM send `sb` 0 and LSB sends 1, as the PC
/// program does for its 144 MHz band (UHFA.C v3.96, `set_mode`, the `case USB`, `LSB`, `CW` and
/// `FM` arms where `tvtr[band].sb` is -1).
pub fn mode_bytes(code: u8) -> Option<(u8, u8)> {
    match code {
        b'0' => Some((0, 1)),
        b'1' => Some((0, 0)),
        b'2' => Some((1, 0)),
        b'3' => Some((2, 0)),
        _ => None,
    }
}

// [@ANCHOR: radioctl_dsp10_mode_frame]
/// Command 77 for `mode` and `sb`.
pub fn mode_frame(mode: u8, sb: u8) -> Vec<u8> {
    let [lo, hi] = MIKE_WORD.to_le_bytes();
    vec![ESC, CMD_MODE, mode, sb, lo, hi]
}

/// Which sideband the first conversion is quantised for.
// [@ANCHOR: radioctl_dsp10_side]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    /// USB or CW on air (the DSP's LSB side).
    Usb,
    /// LSB on air.
    Lsb,
    /// FM.
    Fm,
}

/// The words that tune the radio to one frequency.
// [@ANCHOR: radioctl_dsp10_tuning_struct]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tuning {
    /// The 19-bit synthesizer word.
    pub synth_word: u32,
    /// The 24-bit third-LO phase increment.
    pub dph: u32,
}

// [@ANCHOR: radioctl_dsp10_tuning]
/// The synthesizer word and third-LO increment for `hz`, or `None` outside 144 to 148 MHz
/// (`UHFA.C` `new_freq`).
pub fn tuning(hz: u64, side: Side) -> Option<Tuning> {
    if !RANGE.contains(&hz) {
        return None;
    }
    let round = match side {
        Side::Usb => 4000,
        Side::Lsb => 250,
        Side::Fm => 2500,
    };
    let step = 5000 * ((hz + round) / 5000);
    let n = (step - 19_665_000) / 5000;
    let b = (n / 64) & 0x7FF;
    let a = (n - 64 * b) & 0x7F;
    let dph = (DPH_PER_HZ * (step as f64 - hz as f64 + 15000.0) + 0.5) as u32;
    Some(Tuning {
        synth_word: ((a << 1) | (b << 8)) as u32,
        dph,
    })
}

fn lo_frame(data: u8, dph: u32) -> [u8; 6] {
    let [xl, lo, hi, _] = dph.to_le_bytes();
    [ESC, CMD_LO, data, xl, lo, hi]
}

// [@ANCHOR: radioctl_dsp10_synth_frames]
/// The frames that load a synthesizer word and set the third LO: the LO increment, three
/// shift-register words, then the enable low and high (`UHFA.C` `new_freq`, `send_shift_data`).
pub fn synth_frames(t: &Tuning) -> Vec<u8> {
    shift_frames(t.synth_word, t.dph, true)
}

// [@ANCHOR: radioctl_dsp10_shift_stages]
/// One `send_shift_data(R10MHZ)` of control byte `control` and RF-gain byte `gain`, as two stages
/// (the 10 ms between enable low and high), the second waiting `after`; `prefix` is sent first.
fn shift_stages(prefix: &[u8], control: u8, gain: u8, dph: u32, after: u64) -> [Stage; 2] {
    let mut frames = prefix.to_vec();
    frames.extend(shift_bytes(R10MHZ, control, gain, dph, false));
    let tail = frames.split_off(frames.len() - FRAME_LEN);
    [
        Stage {
            bytes: frames,
            wait: Duration::from_millis(10),
        },
        Stage {
            bytes: tail,
            wait: Duration::from_millis(after),
        },
    ]
}

// [@ANCHOR: radioctl_dsp10_ptt_stages]
/// The staged key-on (`on`) or unkey sequence for the third-LO increment `dph`, with the retune
/// frames `retune` appended to an unkey.
pub fn ptt_stages(on: bool, dph: u32, retune: &[u8]) -> Vec<Stage> {
    let [pl, ph] = POWER_WORD.to_le_bytes();
    let dsp_tx = [ESC, CMD_TX, 3, 2, pl, ph];
    let dsp_rx = [ESC, CMD_TX, 2, 2, pl, ph];
    let mut out = Vec::new();
    if on {
        let gain_off = [ESC, 10, 0, 0, 0, 4];
        out.extend(shift_stages(&gain_off, TX_ANTENNA, 0, dph, 50));
        out.extend(shift_stages(&[], TX_AMP, 0, dph, 75));
        out.extend(shift_stages(&[], TX_ON, 0, dph, 0));
        out.push(Stage {
            bytes: dsp_tx.to_vec(),
            wait: Duration::from_millis(10),
        });
    } else {
        out.extend(shift_stages(&dsp_rx, TX_AMP, 0, dph, 25));
        out.extend(shift_stages(&[], TX_ANTENNA, 0, dph, 25));
        out.extend(shift_stages(&[], RX_ANTENNA_OFF, 0, dph, 2));
        out.extend(shift_stages(&[], SHIFT_RECEIVE, RF_GAIN_BYTE, dph, 0));
        if !retune.is_empty() {
            out.push(Stage {
                bytes: retune.to_vec(),
                wait: Duration::ZERO,
            });
        }
    }
    out
}

// [@ANCHOR: radioctl_dsp10_shift_frames]
fn shift_frames(word: u32, dph: u32, lo_first: bool) -> Vec<u8> {
    shift_bytes(word, SHIFT_RECEIVE, RF_GAIN_BYTE, dph, lo_first)
}

// [@ANCHOR: radioctl_dsp10_shift_bytes]
fn shift_bytes(word: u32, control: u8, gain: u8, dph: u32, lo_first: bool) -> Vec<u8> {
    let first = (word >> 16) & 7;
    let mid = word & 0xFFFF;
    let mut out = Vec::new();
    if lo_first {
        out.extend(lo_frame(0, dph));
    }
    out.extend([ESC, CMD_SHIFT, first as u8, 0, 3, 4]);
    out.extend([ESC, CMD_SHIFT, mid as u8, (mid >> 8) as u8, 3, 4]);
    out.extend([ESC, CMD_SHIFT, control, gain, 3, 4]);
    out.extend(lo_frame(0, dph));
    out.extend(lo_frame(1, dph));
    out
}

/// The DSP-10 client engine.
// [@ANCHOR: radioctl_dsp10_dsp10_engine]
#[derive(Debug)]
pub struct Dsp10Engine {
    desc: RigDescription,
    side: Side,
    hz: Option<u64>,
}

impl Dsp10Engine {
    // [@ANCHOR: radioctl_dsp10_engine_new]
    /// Builds an engine from a DSP-10 vendor description.
    pub fn new(desc: RigDescription) -> Result<Dsp10Engine, RigError> {
        let (_, name) = vendor_of(&desc)?;
        if name != "dsp10" {
            return Err(unsupported("unknown DSP-10 dialect in the description"));
        }
        Ok(Dsp10Engine {
            desc,
            side: Side::Usb,
            hz: None,
        })
    }
}

impl ProtocolEngine for Dsp10Engine {
    fn description(&self) -> &RigDescription {
        &self.desc
    }

    // [@ANCHOR: radioctl_dsp10_engine_encode]
    fn encode(&mut self, op: &Op) -> Result<Request, RigError> {
        if !self.desc.capabilities.has(op.required_capability()) {
            return Err(unsupported("capability not in the rig description"));
        }
        let mut bytes = Vec::new();
        match op {
            Op::SetMode(mode) => {
                let code = self
                    .desc
                    .mode_code(*mode)
                    .ok_or_else(|| unsupported("mode not in the description"))?;
                let (m, sb) = mode_bytes(code)
                    .ok_or_else(|| unsupported("mode code unknown to the DSP-10 engine"))?;
                bytes.extend(mode_frame(m, sb));
                self.side = match code {
                    b'0' => Side::Lsb,
                    b'3' => Side::Fm,
                    _ => Side::Usb,
                };
                // The 5 kHz grid depends on the sideband: retune, as the PC program does.
                if let Some(t) = self.hz.and_then(|hz| tuning(hz, self.side)) {
                    bytes.extend(synth_frames(&t));
                }
            }
            Op::SetFrequency(Vfo::A, Hz(hz)) => {
                let t = tuning(*hz, self.side)
                    .ok_or_else(|| unsupported("frequency outside 144 to 148 MHz"))?;
                self.hz = Some(*hz);
                bytes.extend(synth_frames(&t));
            }
            _ => return Err(unsupported("not implemented for the DSP-10 engine")),
        }
        Ok(Request {
            bytes,
            reply: ReplyKind::None,
        })
    }

    // [@ANCHOR: radioctl_dsp10_engine_encode_stages]
    fn encode_stages(&mut self, op: &Op) -> Result<Option<Vec<Stage>>, RigError> {
        let Op::SetPtt(on) = op else {
            return Ok(None);
        };
        if !self.desc.capabilities.has(op.required_capability()) {
            return Err(unsupported("capability not in the rig description"));
        }
        let tuning = self.hz.and_then(|hz| tuning(hz, self.side));
        let dph = tuning.map_or(DPH_DEFAULT, |t| t.dph);
        let retune = tuning.map(|t| synth_frames(&t)).unwrap_or_default();
        Ok(Some(ptt_stages(*on, dph, &retune)))
    }

    // [@ANCHOR: radioctl_dsp10_engine_open_requests]
    fn open_requests(&mut self) -> Vec<Request> {
        // Loads the synthesizer reference divider (UHFA.C initialize); keeps the receive state.
        vec![Request {
            bytes: shift_frames(R10MHZ, 0x4AA6 << 8, false),
            reply: ReplyKind::None,
        }]
    }

    // [@ANCHOR: radioctl_dsp10_engine_feed]
    fn feed(&mut self, _bytes: &[u8], _now: Duration) -> Vec<Inbound> {
        // The status stream (ESC 'A' blocks) carries nothing this engine reads.
        Vec::new()
    }
}

/// A virtual DSP-10.
// [@ANCHOR: radioctl_dsp10_dsp10_twin]
#[derive(Debug)]
pub struct Dsp10Twin {
    frame: Vec<u8>,
    mode: u8,
    sb: u8,
    mike: u16,
    last_cmd: u8,
    hz: Hz,
    shift: Vec<u16>,
    dph: u32,
    pf7: bool,
    tx_hw: bool,
    tx_dsp: bool,
    synth_hz: Option<u64>,
    r_word: Option<u32>,
}

impl Dsp10Twin {
    // [@ANCHOR: radioctl_dsp10_twin_new]
    /// A twin in the DSP's start state (SSB, USB).
    pub fn new(desc: &RigDescription) -> Result<Dsp10Twin, RigError> {
        let (_, name) = vendor_of(desc)?;
        if name != "dsp10" {
            return Err(unsupported("unknown DSP-10 dialect in the description"));
        }
        Ok(Dsp10Twin {
            frame: Vec::new(),
            mode: 0,
            sb: 0,
            mike: 0,
            last_cmd: 0,
            hz: desc.default_frequency,
            shift: Vec::new(),
            dph: 0,
            pf7: false,
            tx_hw: false,
            tx_dsp: false,
            synth_hz: None,
            r_word: None,
        })
    }

    /// The microphone gain the last mode command set, 4 * (256 * mikeh + mikel).
    pub fn mike_gain(&self) -> u32 {
        4 * u32::from(self.mike)
    }

    /// The reference-divider word the synthesizer was last loaded with, if any.
    // [@ANCHOR: radioctl_dsp10_twin_reference_word]
    pub fn reference_word(&self) -> Option<u32> {
        self.r_word
    }

    /// The last command byte received (status byte +2 of the receive status).
    pub fn last_command(&self) -> u8 {
        self.last_cmd
    }
}

impl Dsp10Twin {
    // [@ANCHOR: radioctl_dsp10_twin_tuned]
    /// The receive frequency the synthesizer and third LO make, if both were loaded.
    fn tuned(&self) -> Option<Hz> {
        let step = self.synth_hz? + 19_665_000;
        let f = step as f64 + 15000.0 - f64::from(self.dph) / DPH_PER_HZ;
        Some(Hz(f.round() as u64))
    }

    // [@ANCHOR: radioctl_dsp10_twin_apply]
    /// Applies one complete command frame.
    fn apply(&mut self, f: &[u8]) {
        self.last_cmd = f[1];
        match f[1] {
            CMD_MODE if f[2] <= 2 && f[3] <= 2 => {
                self.mode = f[2];
                self.sb = f[3];
                self.mike = u16::from_le_bytes([f[4], f[5]]);
            }
            CMD_TX => self.tx_dsp = f[2] & 1 == 1,
            CMD_SHIFT => self.shift.push(u16::from_le_bytes([f[2], f[3]])),
            CMD_LO => {
                self.dph = u32::from_le_bytes([f[3], f[4], f[5], 0]);
                let on = f[2] & 1 == 1;
                if on && !self.pf7 {
                    self.latch();
                }
                self.pf7 = on;
            }
            _ => {}
        }
    }

    // [@ANCHOR: radioctl_dsp10_twin_latch]
    /// The enable line rose: the three shifted words become the synthesizer register.
    fn latch(&mut self) {
        let words = std::mem::take(&mut self.shift);
        let [first, mid, control] = words.as_slice() else {
            return;
        };
        self.tx_hw = control & 0x02 != 0;
        let w = (u32::from(*first & 7) << 16) | u32::from(*mid);
        if w & 1 == 1 {
            self.r_word = Some(w);
        } else {
            let n = 64 * u64::from((w >> 8) & 0x7FF) + u64::from((w >> 1) & 0x7F);
            self.synth_hz = Some(n * 5000);
        }
    }
}

impl VendorTwin for Dsp10Twin {
    // [@ANCHOR: radioctl_dsp10_twin_snapshot]
    fn snapshot(&self) -> Option<TwinSnapshot> {
        let code = match (self.mode, self.sb) {
            (0, 1) => b'0',
            (0, _) => b'1',
            (1, _) => b'2',
            _ => b'3',
        };
        Some(TwinSnapshot {
            frequency: self.tuned().unwrap_or(self.hz),
            mode_code: Some(code),
            ptt: self.tx_hw && self.tx_dsp,
        })
    }

    // [@ANCHOR: radioctl_dsp10_twin_feed]
    fn feed(&mut self, bytes: &[u8], _now: Duration) -> Vec<u8> {
        for &b in bytes {
            // Resynchronise on ESC: bytes before the first ESC are not a frame.
            if self.frame.is_empty() && b != ESC {
                continue;
            }
            self.frame.push(b);
            if self.frame.len() == FRAME_LEN {
                let f = std::mem::take(&mut self.frame);
                self.apply(&f);
            }
        }
        Vec::new()
    }

    // [@ANCHOR: radioctl_dsp10_twin_front_panel]
    fn front_panel(&mut self, panel: Panel) -> Result<Vec<u8>, String> {
        Err(format!("a DSP-10 has no {panel:?} knob"))
    }
}

#[cfg(test)]
mod tests {
    // Tests [@ANCHOR: radioctl_dsp10_dsp10_engine]
    // Tests [@ANCHOR: radioctl_dsp10_dsp10_twin]
    use super::*;
    use radioctl_core::units::{Hz, Mode, Vfo};

    const T0: Duration = Duration::from_secs(1);

    fn shipped() -> RigDescription {
        let path = format!("{}/../../rigs/tapr-dsp-10.toml", env!("CARGO_MANIFEST_DIR"));
        let text = std::fs::read_to_string(path).expect("rig file");
        radioctl_core::rigfile::parse(&text)
            .expect("loads")
            .description
    }

    // Tests [@ANCHOR: radioctl_dsp10_mode_bytes]
    // Tests [@ANCHOR: radioctl_dsp10_mode_frame]
    #[test]
    fn command_77_bytes_follow_the_document() {
        assert_eq!(mode_bytes(b'0'), Some((0, 1)));
        assert_eq!(mode_bytes(b'1'), Some((0, 0)));
        assert_eq!(mode_bytes(b'2'), Some((1, 0)));
        assert_eq!(mode_bytes(b'3'), Some((2, 0)));
        assert_eq!(mode_bytes(b'9'), None);
        assert_eq!(mode_frame(2, 0), vec![0x24, 77, 2, 0, 1, 0]);
    }

    // Tests [@ANCHOR: radioctl_dsp10_engine_new]
    #[test]
    fn other_families_are_refused() {
        assert!(Dsp10Engine::new(RigDescription::ts2000()).is_err());
        assert!(Dsp10Twin::new(&RigDescription::ts2000()).is_err());
    }

    // Tests [@ANCHOR: radioctl_dsp10_engine_encode]
    // Tests [@ANCHOR: radioctl_dsp10_engine_feed]
    // Tests [@ANCHOR: radioctl_dsp10_twin_new]
    // Tests [@ANCHOR: radioctl_dsp10_twin_feed]
    // Tests [@ANCHOR: radioctl_dsp10_twin_snapshot]
    // Tests [@ANCHOR: radioctl_dsp10_twin_front_panel]
    #[test]
    fn modes_reach_the_virtual_dsp_and_nothing_else_is_encoded() {
        let desc = shipped();
        let mut e = Dsp10Engine::new(desc.clone()).expect("engine");
        let mut t = Dsp10Twin::new(&desc).expect("twin");
        for (mode, wire, code) in [
            (Mode::Lsb, [0x24, 77, 0, 1, 1, 0], b'0'),
            (Mode::Usb, [0x24, 77, 0, 0, 1, 0], b'1'),
            (Mode::Cw, [0x24, 77, 1, 0, 1, 0], b'2'),
            (Mode::Fm, [0x24, 77, 2, 0, 1, 0], b'3'),
        ] {
            let r = e.encode(&Op::SetMode(mode)).expect("encodes");
            assert_eq!(r.bytes, wire, "{mode:?}");
            assert_eq!(r.reply, ReplyKind::None);
            assert!(t.feed(&r.bytes, T0).is_empty());
            assert!(e.feed(&[], T0).is_empty());
            assert_eq!(t.snapshot().expect("snapshot").mode_code, Some(code));
        }
        assert_eq!(t.last_command(), 77);
        assert_eq!(t.mike_gain(), 4);
        // A frame split across reads, and junk before ESC, still lands (sb 1 is LSB on 2 m).
        t.feed(&[0, 0x24, 77, 0], T0);
        t.feed(&[1, 1, 0], T0);
        assert_eq!(t.snapshot().expect("snapshot").mode_code, Some(b'0'));
        // Out of range mode and sideband bytes are ignored.
        t.feed(&[0x24, 77, 5, 0, 1, 0], T0);
        assert_eq!(t.snapshot().expect("snapshot").mode_code, Some(b'0'));
        // Only VFO A and 144 to 148 MHz; no PTT.
        assert!(e
            .encode(&Op::SetFrequency(Vfo::B, Hz(145_000_000)))
            .is_err());
        assert!(e.encode(&Op::SetPtt(true)).is_err());
        assert!(e.encode(&Op::SetMode(Mode::Am)).is_err());
        assert!(t.front_panel(Panel::Ptt(true)).is_err());
    }

    // Tests [@ANCHOR: radioctl_dsp10_tuning]
    // Tests [@ANCHOR: radioctl_dsp10_shift_frames]
    // Tests [@ANCHOR: radioctl_dsp10_twin_tuned]
    // Tests [@ANCHOR: radioctl_dsp10_twin_apply]
    // Tests [@ANCHOR: radioctl_dsp10_twin_latch]
    // Tests [@ANCHOR: radioctl_dsp10_side]
    // Tests [@ANCHOR: radioctl_dsp10_tuning_struct]
    // Tests [@ANCHOR: radioctl_dsp10_engine_open_requests]
    // Tests [@ANCHOR: radioctl_dsp10_twin_reference_word]
    // Tests [@ANCHOR: radioctl_dsp10_synth_frames]
    #[test]
    fn tuning_words_follow_the_pc_program() {
        // 145.000000 MHz, USB on air (DSP LSB side): I-F 145.000 MHz, synthesizer 125.335 MHz,
        // N = 25067 = 64 * 391 + 43, third LO 15 kHz.
        let t = tuning(145_000_000, Side::Usb).expect("in range");
        assert_eq!(t.synth_word, (43 << 1) | (391 << 8));
        assert_eq!(t.dph, 0x50_0000);
        assert!(tuning(143_999_999, Side::Usb).is_none());
        assert!(tuning(148_000_001, Side::Usb).is_none());
        // The quantisation to 5 kHz depends on the sideband: the same 145.003 MHz lands on
        // 145.005 (USB) and 145.000 (LSB); FM rounds to nearest.
        assert_eq!(tuning(145_003_000, Side::Usb).expect("t").dph, 5_941_931);
        assert_eq!(tuning(145_003_000, Side::Lsb).expect("t").dph, 4_194_304);
        assert_eq!(tuning(145_002_000, Side::Fm).expect("t").dph, 4_543_829);
        assert_eq!(
            synth_frames(&t),
            vec![
                0x24, 7, 0, 0, 0, 0x50, //
                0x24, 8, 1, 0, 3, 4, //
                0x24, 8, 0x56, 0x87, 3, 4, //
                0x24, 8, 0x04, 0x88, 3, 4, //
                0x24, 7, 0, 0, 0, 0x50, //
                0x24, 7, 1, 0, 0, 0x50,
            ]
        );
    }

    #[test]
    fn frequency_reaches_the_virtual_dsp_and_follows_the_mode() {
        let desc = shipped();
        let mut e = Dsp10Engine::new(desc.clone()).expect("engine");
        let mut t = Dsp10Twin::new(&desc).expect("twin");
        // The reference divider goes out when the rig is opened (UHFA.C initialize).
        for r in e.open_requests() {
            t.feed(&r.bytes, T0);
        }
        assert_eq!(t.reference_word(), Some(R10MHZ));
        for (mode, hz) in [
            (Mode::Usb, 145_000_000),
            (Mode::Usb, 145_123_456),
            (Mode::Lsb, 144_000_000),
            (Mode::Lsb, 147_999_999),
            (Mode::Fm, 146_520_000),
            (Mode::Cw, 144_100_050),
        ] {
            let r = e.encode(&Op::SetMode(mode)).expect("mode");
            t.feed(&r.bytes, T0);
            let r = e.encode(&Op::SetFrequency(Vfo::A, Hz(hz))).expect("freq");
            assert_eq!(r.reply, ReplyKind::None);
            t.feed(&r.bytes, T0);
            assert_eq!(
                t.snapshot().expect("snapshot").frequency,
                Hz(hz),
                "{mode:?} {hz}"
            );
        }
        // Changing the sideband retunes (the 5 kHz grid moves), as the PC program does.
        let r = e
            .encode(&Op::SetFrequency(Vfo::A, Hz(145_003_000)))
            .expect("f");
        t.feed(&r.bytes, T0);
        let r = e.encode(&Op::SetMode(Mode::Usb)).expect("usb");
        assert_eq!(r.bytes.len(), 6 + 6 * 6);
        t.feed(&r.bytes, T0);
        assert_eq!(t.snapshot().expect("snapshot").frequency, Hz(145_003_000));
        // Out of the 144 to 148 MHz range, and no PTT.
        assert!(e
            .encode(&Op::SetFrequency(Vfo::A, Hz(143_000_000)))
            .is_err());
        assert!(e
            .encode(&Op::SetFrequency(Vfo::B, Hz(145_000_000)))
            .is_err());
        assert!(e.encode(&Op::SetPtt(true)).is_err());
    }
}
