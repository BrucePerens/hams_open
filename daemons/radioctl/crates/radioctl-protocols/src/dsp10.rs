// SPDX-License-Identifier: AGPL-3.0-or-later
//! TAPR DSP-10 2 m transceiver: sans-IO client engine and virtual twin, mode setting only.
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
//! **Not covered, on purpose.** The document gives no frequency command: Command 7 sets the third
//! LO and the synthesizer enable, and Command 8 shifts a 16-bit word to the synthesizer shift
//! registers, but the meaning of that word (the 126 MHz synthesizer divider) is hardware knowledge
//! the document does not contain, so there is no set-frequency. Command 84 ('T') only switches the
//! DSP between receive and transmit; the transmit hardware is steered by Commands 7 and 8, so
//! there is no PTT either and the transmit gate has nothing to guard here.
//!
//! The mode codes in the description are `0` LSB, `1` USB, `2` CW, `3` FM (ours). The microphone gain word sent with every mode command is [`MIKE_WORD`] (ours; the
//! document gives no default).

use std::time::Duration;

use radioctl_core::description::RigDescription;
use radioctl_core::engine::{Inbound, ProtocolEngine, ReplyKind, Request};
use radioctl_core::error::{ErrorKind, RigError};
use radioctl_core::op::Op;
use radioctl_core::transcript::Panel;

use crate::serial_vendor::{vendor_of, TwinSnapshot, VendorTwin};

/// The escape byte that starts every command (document: "ESC ... is '$' or 24 hex").
pub const ESC: u8 = 0x24;
/// Command 77 ('M'): mode, sideband and microphone gain.
pub const CMD_MODE: u8 = 77;
/// Frame length: ESC, command, four data bytes.
pub const FRAME_LEN: usize = 6;
/// The microphone gain word `256 * mikeh + mikel` we send (gain 4 * this).
pub const MIKE_WORD: u16 = 1;

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

/// The DSP-10 client engine.
// [@ANCHOR: radioctl_dsp10_dsp10_engine]
#[derive(Debug)]
pub struct Dsp10Engine {
    desc: RigDescription,
}

impl Dsp10Engine {
    // [@ANCHOR: radioctl_dsp10_engine_new]
    /// Builds an engine from a DSP-10 vendor description.
    pub fn new(desc: RigDescription) -> Result<Dsp10Engine, RigError> {
        let (_, name) = vendor_of(&desc)?;
        if name != "dsp10" {
            return Err(unsupported("unknown DSP-10 dialect in the description"));
        }
        Ok(Dsp10Engine { desc })
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
        let Op::SetMode(mode) = op else {
            return Err(unsupported("not implemented for the DSP-10 engine"));
        };
        let code = self
            .desc
            .mode_code(*mode)
            .ok_or_else(|| unsupported("mode not in the description"))?;
        let (m, sb) = mode_bytes(code)
            .ok_or_else(|| unsupported("mode code unknown to the DSP-10 engine"))?;
        Ok(Request {
            bytes: mode_frame(m, sb),
            reply: ReplyKind::None,
        })
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
    hz: radioctl_core::units::Hz,
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
        })
    }

    /// The microphone gain the last mode command set, 4 * (256 * mikeh + mikel).
    pub fn mike_gain(&self) -> u32 {
        4 * u32::from(self.mike)
    }

    /// The last command byte received (status byte +2 of the receive status).
    pub fn last_command(&self) -> u8 {
        self.last_cmd
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
            frequency: self.hz,
            mode_code: Some(code),
            ptt: false,
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
                self.last_cmd = f[1];
                if f[1] == CMD_MODE && f[2] <= 2 && f[3] <= 2 {
                    self.mode = f[2];
                    self.sb = f[3];
                    self.mike = u16::from_le_bytes([f[4], f[5]]);
                }
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
        // No frequency, no PTT: the document has no command for either.
        assert!(e
            .encode(&Op::SetFrequency(Vfo::A, Hz(145_000_000)))
            .is_err());
        assert!(e.encode(&Op::SetPtt(true)).is_err());
        assert!(e.encode(&Op::SetMode(Mode::Am)).is_err());
        assert!(t.front_panel(Panel::Ptt(true)).is_err());
    }
}
