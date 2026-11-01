// SPDX-License-Identifier: AGPL-3.0-or-later
//! Watkins-Johnson WJ-8888 HF receiver: sans-IO client engine and virtual twin.
//!
//! **Sources.** Facts only, read from Hamlib 4.6.2 (released tarball, sha256
//! b2ac73f44dd1161e95fdee6c95276144757647bf92d7fdb369ee2fe41ed47ae8): `rigs/wj/wj.c`. No
//! Watkins-Johnson document was found (premium-rx.org's copies return 404); nothing is verified
//! against a radio.
//!
//! Every command is ten bytes carrying all the settings, so each one sends the whole frame:
//!
//! * byte 0: `0x48` (command, receiver id 0 in the low nibble);
//! * bytes 1 to 4: the frequency as seven BCD digits of 10 Hz units, split across the bytes seven
//!   bits at a time; the least significant bit of the 100 Hz digit is not sent, so odd 100 Hz
//!   digits are lost (tuning is exact for frequencies whose 100 Hz digit is even);
//! * byte 4 bit 0 and byte 5 bit 6: the AGC (slow 0, off 1, fast 2, manual 3; slow here);
//! * byte 5: bits 3 to 5 the IF bandwidth (code 4, 8 kHz, until a mode is set, then code 2, which
//!   is what Hamlib's wire shows), bits 0 to 2 the mode (AM 0, FM 1, CW 2,
//!   LSB 5, USB 6);
//! * bytes 6 and 7: the BFO word (`0x400` for zero shift); bytes 7 and 8: the RF gain (`0x7f`
//!   at full gain); byte 9: zero.
//!
//! Hamlib's own `get_freq` and `get_mode` ask the radio with a monitor frame but then return
//! without filling in the result, so the description is set only. Not in the engine: filter width,
//! AGC, RF gain, BFO shift, the independent sideband mode and the receiver id.

use std::time::Duration;

use radioctl_core::description::RigDescription;
use radioctl_core::engine::{Inbound, ProtocolEngine, ReplyKind, Request};
use radioctl_core::error::{ErrorKind, RigError};
use radioctl_core::op::Op;
use radioctl_core::transcript::Panel;
use radioctl_core::units::{Hz, Mode, Vfo};

use crate::serial_vendor::{vendor_of, RxBuffer, TwinSnapshot, VendorTwin};

/// Length of a command frame.
pub const FRAME_LEN: usize = 10;
/// Lowest frequency, hertz (Hamlib's range).
pub const MIN_HZ: u64 = 5_000;
/// Highest frequency, hertz (Hamlib's range).
pub const MAX_HZ: u64 = 30_000_000;
/// IF bandwidth code of 8 kHz, where Hamlib starts.
const WIDTH_8K: u8 = 4;
/// IF bandwidth code of 2 kHz, which Hamlib's wire shows after every mode write (it picks the
/// mode's "normal" width; seen for USB and AM, assumed for the others).
const WIDTH_MODE: u8 = 2;
/// BFO word for no shift.
const BFO_ZERO: u16 = 0x400;
/// RF gain word at full gain.
const RF_FULL: u8 = 0x7f;

fn unsupported(what: &str) -> RigError {
    RigError::new(ErrorKind::Unsupported, what.to_string())
}

// [@ANCHOR: radioctl_wj_mode_code]
/// The mode bits of a mode, or `None` for one the radio has not got in this engine.
pub fn mode_code(mode: Mode) -> Option<u8> {
    Some(match mode {
        Mode::Am => 0,
        Mode::Fm => 1,
        Mode::Cw => 2,
        Mode::Lsb => 5,
        Mode::Usb => 6,
        _ => return None,
    })
}

// [@ANCHOR: radioctl_wj_bcd_digits]
/// The four bytes Hamlib's BCD routine makes of a frequency: seven digits of 10 Hz units, the
/// first six two to a byte and the seventh in the high nibble of the last byte.
pub fn bcd_digits(hz: Hz) -> [u8; 4] {
    let mut units = hz.0 / 10;
    let mut digits = [0u8; 7];
    for slot in digits.iter_mut().rev() {
        *slot = u8::try_from(units % 10).unwrap_or(0);
        units /= 10;
    }
    [
        digits[0] << 4 | digits[1],
        digits[2] << 4 | digits[3],
        digits[4] << 4 | digits[5],
        digits[6] << 4,
    ]
}

// [@ANCHOR: radioctl_wj_command_frame]
/// The command frame for a frequency, a mode and a width code (slow AGC, no shift, full gain).
pub fn command_frame(hz: Hz, mode: u8, width: u8) -> [u8; FRAME_LEN] {
    let f = bcd_digits(hz);
    let mut buf = [0u8; FRAME_LEN];
    buf[0] = 0x48;
    buf[1] = f[0] & 0x3f;
    buf[2] = f[1] >> 1;
    buf[3] = ((f[1] & 1) << 6) | (f[2] >> 2);
    buf[4] = ((f[2] & 2) << 5) | (f[3] >> 3);
    buf[5] = ((width & 7) << 3) | (mode & 7);
    buf[6] = u8::try_from((BFO_ZERO >> 5) & 0x3f).unwrap_or(0);
    buf[7] = u8::try_from((BFO_ZERO & 0x1f) << 2).unwrap_or(0) | ((RF_FULL >> 6) & 1);
    buf[8] = (RF_FULL & 0x3f) << 1;
    buf
}

// [@ANCHOR: radioctl_wj_decode_frame]
/// The frequency and mode bits a command frame carries (the lost bit of the 100 Hz digit reads
/// as zero), or `None` for a frame that is not a command.
pub fn decode_frame(buf: &[u8]) -> Option<(Hz, u8)> {
    if buf.len() != FRAME_LEN || buf.first()? & 0x40 == 0 {
        return None;
    }
    let b = |i: usize| buf.get(i).copied().unwrap_or(0);
    let f0 = b(1) & 0x3f;
    let f1 = (b(2) << 1) | ((b(3) >> 6) & 1);
    let f2 = ((b(3) & 0x3f) << 2) | (((b(4) >> 6) & 1) << 1);
    let f3 = (b(4) & 0x1e) << 3;
    let digit = |n: u8| u64::from(n & 0x0f);
    let units = [f0 >> 4, f0, f1 >> 4, f1, f2 >> 4, f2, f3 >> 4]
        .iter()
        .fold(0u64, |a, n| a * 10 + digit(*n));
    Some((Hz(units * 10), b(5) & 7))
}

/// The WJ-8888 client engine.
#[derive(Debug)]
pub struct WjEngine {
    desc: RigDescription,
    hz: Hz,
    mode: u8,
    width: u8,
}

impl WjEngine {
    // [@ANCHOR: radioctl_wj_engine_new]
    /// Builds an engine from a WJ vendor description (starting, like Hamlib, at 500 kHz AM).
    pub fn new(desc: RigDescription) -> Result<WjEngine, RigError> {
        let (_, name) = vendor_of(&desc)?;
        if name != "wj8888" {
            return Err(unsupported("unknown WJ dialect in the description"));
        }
        Ok(WjEngine {
            desc,
            hz: Hz(500_000),
            mode: 0,
            width: WIDTH_8K,
        })
    }
}

impl ProtocolEngine for WjEngine {
    fn description(&self) -> &RigDescription {
        &self.desc
    }

    // [@ANCHOR: radioctl_wj_engine_encode]
    fn encode(&mut self, op: &Op) -> Result<Request, RigError> {
        if !self.desc.capabilities.has(op.required_capability()) {
            return Err(unsupported("capability not in the rig description"));
        }
        match op {
            Op::SetFrequency(Vfo::A, hz) => {
                if hz.0 < MIN_HZ || hz.0 > MAX_HZ {
                    return Err(unsupported("frequency outside 5 kHz to 30 MHz"));
                }
                self.hz = *hz;
            }
            Op::SetMode(mode) => {
                self.mode =
                    mode_code(*mode).ok_or_else(|| unsupported("mode the engine has not got"))?;
                self.width = WIDTH_MODE;
            }
            _ => return Err(unsupported("not implemented for the WJ engine")),
        }
        Ok(Request {
            bytes: command_frame(self.hz, self.mode, self.width).to_vec(),
            reply: ReplyKind::None,
        })
    }

    // [@ANCHOR: radioctl_wj_engine_feed]
    fn feed(&mut self, bytes: &[u8], _now: Duration) -> Vec<Inbound> {
        // Only monitor frames answer, and the engine sends none.
        if bytes.is_empty() {
            Vec::new()
        } else {
            vec![Inbound::Unrecognised(bytes.to_vec())]
        }
    }
}

/// A virtual WJ-8888: it decodes the command frames.
#[derive(Debug)]
pub struct WjTwin {
    rx: RxBuffer,
    hz: Hz,
    mode: u8,
}

impl WjTwin {
    // [@ANCHOR: radioctl_wj_twin_new]
    /// A twin in the description's default state.
    pub fn new(desc: &RigDescription) -> Result<WjTwin, RigError> {
        let (_, name) = vendor_of(desc)?;
        if name != "wj8888" {
            return Err(unsupported("unknown WJ dialect in the description"));
        }
        Ok(WjTwin {
            rx: RxBuffer::default(),
            hz: desc.default_frequency,
            mode: desc.mode_code(desc.default_mode).unwrap_or(0),
        })
    }
}

impl VendorTwin for WjTwin {
    // [@ANCHOR: radioctl_wj_twin_snapshot]
    fn snapshot(&self) -> Option<TwinSnapshot> {
        Some(TwinSnapshot {
            frequency: self.hz,
            mode_code: Some(self.mode),
            ptt: false,
        })
    }

    // [@ANCHOR: radioctl_wj_twin_feed]
    fn feed(&mut self, bytes: &[u8], now: Duration) -> Vec<u8> {
        self.rx.push(bytes, now);
        while let Some(frame) = self.rx.take(FRAME_LEN) {
            if let Some((hz, mode)) = decode_frame(&frame) {
                self.hz = hz;
                self.mode = mode;
            }
        }
        Vec::new()
    }

    // [@ANCHOR: radioctl_wj_twin_front_panel]
    fn front_panel(&mut self, panel: Panel) -> Result<Vec<u8>, String> {
        match panel {
            Panel::Frequency(hz) => self.hz = hz,
            Panel::Mode(mode) => {
                self.mode = mode_code(mode).ok_or_else(|| format!("no {mode:?} mode"))?;
            }
            other => return Err(format!("a WJ twin has no {other:?} knob")),
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
            "{}/../../rigs/watkins-johnson-wj-8888.toml",
            env!("CARGO_MANIFEST_DIR")
        );
        let text = std::fs::read_to_string(path).expect("rig file");
        radioctl_core::rigfile::parse(&text)
            .expect("loads")
            .description
    }

    // Tests [@ANCHOR: radioctl_wj_mode_code]
    // Tests [@ANCHOR: radioctl_wj_bcd_digits]
    // Tests [@ANCHOR: radioctl_wj_command_frame]
    // Tests [@ANCHOR: radioctl_wj_decode_frame]
    #[test]
    fn frequencies_round_trip_except_the_lost_bit_of_the_hundred_hertz_digit() {
        assert_eq!(bcd_digits(Hz(7_074_000)), [0x07, 0x07, 0x40, 0x00]);
        assert_eq!(bcd_digits(Hz(29_999_990)), [0x29, 0x99, 0x99, 0x90]);
        for hz in [
            5_000, 500_000, 7_074_000, 10_000_000, 14_200_020, 29_999_880,
        ] {
            let frame = command_frame(Hz(hz), 6, 4);
            assert_eq!(frame.len(), FRAME_LEN);
            assert_eq!(frame[0], 0x48);
            assert_eq!(decode_frame(&frame), Some((Hz(hz), 6)), "{hz}");
            // No byte is above seven bits, as the radio expects.
            assert!(frame.iter().skip(1).all(|b| *b < 0x80), "{hz}");
        }
        // An odd 100 Hz digit loses its low bit: 14.2001 MHz arrives as 14.2000.
        assert_eq!(
            decode_frame(&command_frame(Hz(14_200_100), 0, 4)),
            Some((Hz(14_200_000), 0))
        );
        assert_eq!(
            decode_frame(&command_frame(Hz(14_200_300), 0, 4)),
            Some((Hz(14_200_200), 0))
        );
        assert_eq!(mode_code(Mode::Usb), Some(6));
        assert_eq!(mode_code(Mode::Wfm), None);
        assert_eq!(decode_frame(&[0u8; FRAME_LEN]), None);
        assert_eq!(decode_frame(&[0x48; 9]), None);
    }

    // Tests [@ANCHOR: radioctl_wj_engine_new]
    #[test]
    fn other_families_are_refused() {
        assert!(WjEngine::new(RigDescription::ts2000()).is_err());
        assert!(WjTwin::new(&RigDescription::ts2000()).is_err());
    }

    // Tests [@ANCHOR: radioctl_wj_engine_encode]
    // Tests [@ANCHOR: radioctl_wj_engine_feed]
    // Tests [@ANCHOR: radioctl_wj_twin_new]
    // Tests [@ANCHOR: radioctl_wj_twin_feed]
    // Tests [@ANCHOR: radioctl_wj_twin_snapshot]
    // Tests [@ANCHOR: radioctl_wj_twin_front_panel]
    #[test]
    fn every_command_sends_the_whole_frame() {
        let desc = shipped();
        let mut e = WjEngine::new(desc.clone()).expect("engine");
        let mut t = WjTwin::new(&desc).expect("twin");
        let r = e
            .encode(&Op::SetFrequency(Vfo::A, Hz(7_074_000)))
            .expect("encodes");
        assert_eq!(r.reply, ReplyKind::None);
        assert!(t.feed(&r.bytes[..4], T0).is_empty());
        assert!(t.feed(&r.bytes[4..], T0).is_empty());
        let snap = t.snapshot().expect("snapshot");
        assert_eq!(snap.frequency, Hz(7_074_000));
        let r = e.encode(&Op::SetMode(Mode::Usb)).expect("encodes");
        // The frame Hamlib's wire showed for this frequency and mode (oracle batch).
        assert_eq!(
            r.bytes,
            [0x48, 0x07, 0x03, 0x50, 0x00, 0x16, 0x20, 0x01, 0x7e, 0x00]
        );
        t.feed(&r.bytes, T0);
        let snap = t.snapshot().expect("snapshot");
        assert_eq!((snap.frequency, snap.mode_code), (Hz(7_074_000), Some(6)));
        assert!(e.encode(&Op::SetMode(Mode::Wfm)).is_err());
        assert!(e.encode(&Op::SetFrequency(Vfo::A, Hz(4_999))).is_err());
        assert!(e.encode(&Op::SetFrequency(Vfo::A, Hz(30_000_001))).is_err());
        assert!(e.encode(&Op::GetFrequency(Vfo::A)).is_err());
        assert!(e.encode(&Op::SetPtt(true)).is_err());
        assert!(e.feed(b"", T0).is_empty());
        assert!(matches!(
            e.feed(b">", T0).as_slice(),
            [Inbound::Unrecognised(_)]
        ));
        t.front_panel(Panel::Frequency(Hz(3_573_000)))
            .expect("tunes");
        t.front_panel(Panel::Mode(Mode::Lsb)).expect("mode");
        assert!(t.front_panel(Panel::Mode(Mode::Wfm)).is_err());
        assert!(t.front_panel(Panel::Ptt(true)).is_err());
    }
}
