// SPDX-License-Identifier: AGPL-3.0-or-later
//! N2ADR HiQSDR: sans-IO client engine and virtual twin for its UDP control frame.
//!
//! **Sources.** Facts only, read from Hamlib 4.6.2 (released tarball, sha256
//! b2ac73f44dd1161e95fdee6c95276144757647bf92d7fdb369ee2fe41ed47ae8): `rigs/kit/hiqsdr.c`. No
//! HiQSDR document was read; nothing is verified against a radio.
//!
//! The radio takes control frames over UDP (Hamlib's default address is 192.168.2.196, port
//! 48248). A frame is 22 bytes and carries all the state, so every command sends the whole frame:
//!
//! * bytes 0 and 1: `S` and `t`;
//! * bytes 2 to 5: the receive tuning phase word, little-endian, `frequency / reference clock *
//!   2^32` rounded (reference clock 122 880 000 Hz unless configured); bytes 6 to 9 the transmit
//!   phase word, the same as the receive one (split is not supported here);
//! * byte 10: the transmit level (120 at open); byte 11: `0x01` CW, `0x02` otherwise, and `0x08`
//!   keyed (refused in CW mode); byte 12: the sample-rate divisor, 4 for 48 kHz at the default
//!   clock; byte 13: firmware version, 0; byte 14: X1 connector and attenuator bits; byte 15:
//!   attenuator; byte 16: the antenna switch, bit 0 for antenna 2; bytes 17 to 21 unused.
//!
//! Hamlib cannot read anything back (`get_freq` is not implemented), so the description is set
//! only. Hamlib lists the modes CW and DSB; radioctl has no DSB, so AM stands in for it. Not in
//! the engine: the attenuator, preamplifier and transmit-level settings, the antenna switch, split
//! and the I/Q receive stream.

use std::time::Duration;

use radioctl_core::description::RigDescription;
use radioctl_core::engine::{Inbound, ProtocolEngine, ReplyKind, Request};
use radioctl_core::error::{ErrorKind, RigError};
use radioctl_core::op::Op;
use radioctl_core::transcript::Panel;
use radioctl_core::units::{Hz, Mode, Vfo};

use crate::serial_vendor::{vendor_of, TwinSnapshot, VendorTwin};

/// Length of the control frame.
pub const FRAME_LEN: usize = 22;
/// The reference clock, hertz (Hamlib's default).
pub const REFERENCE_CLOCK: u64 = 122_880_000;
/// Lowest frequency, hertz (Hamlib's range).
pub const MIN_HZ: u64 = 100_000;
/// Highest frequency, hertz (Hamlib's range).
pub const MAX_HZ: u64 = 66_000_000;
/// Mode byte: CW.
pub const MODE_CW: u8 = 0x01;
/// Mode byte: other than CW.
pub const MODE_OTHER: u8 = 0x02;
/// Mode byte bit: keyed.
pub const KEYED: u8 = 0x08;

fn unsupported(what: &str) -> RigError {
    RigError::new(ErrorKind::Unsupported, what.to_string())
}

// [@ANCHOR: radioctl_hiqsdr_phase_word]
/// The tuning phase word of `hz`: `hz / reference clock * 2^32`, rounded.
pub fn phase_word(hz: Hz) -> u32 {
    let phase = (u128::from(hz.0) << 32) + u128::from(REFERENCE_CLOCK / 2);
    u32::try_from(phase / u128::from(REFERENCE_CLOCK)).unwrap_or(u32::MAX)
}

// [@ANCHOR: radioctl_hiqsdr_word_frequency]
/// The frequency a phase word tunes to, rounded to the hertz.
pub fn word_frequency(word: u32) -> Hz {
    let product = u128::from(word) * u128::from(REFERENCE_CLOCK) + (1u128 << 31);
    Hz(u64::try_from(product >> 32).unwrap_or(u64::MAX))
}

// [@ANCHOR: radioctl_hiqsdr_initial_frame]
/// The frame Hamlib builds at open: zero tuning, level 120, not CW, 48 kHz.
pub fn initial_frame() -> [u8; FRAME_LEN] {
    let mut frame = [0u8; FRAME_LEN];
    frame[0] = b'S';
    frame[1] = b't';
    frame[10] = 120;
    frame[11] = MODE_OTHER;
    frame[12] = 4;
    frame
}

/// The HiQSDR client engine.
#[derive(Debug)]
pub struct HiqsdrEngine {
    desc: RigDescription,
    frame: [u8; FRAME_LEN],
}

impl HiqsdrEngine {
    // [@ANCHOR: radioctl_hiqsdr_engine_new]
    /// Builds an engine from a HiQSDR vendor description.
    pub fn new(desc: RigDescription) -> Result<HiqsdrEngine, RigError> {
        let (_, name) = vendor_of(&desc)?;
        if name != "hiqsdr" {
            return Err(unsupported("unknown HiQSDR dialect in the description"));
        }
        Ok(HiqsdrEngine {
            desc,
            frame: initial_frame(),
        })
    }

    fn request(&self) -> Request {
        Request {
            bytes: self.frame.to_vec(),
            reply: ReplyKind::None,
        }
    }
}

impl ProtocolEngine for HiqsdrEngine {
    fn description(&self) -> &RigDescription {
        &self.desc
    }

    // [@ANCHOR: radioctl_hiqsdr_engine_encode]
    fn encode(&mut self, op: &Op) -> Result<Request, RigError> {
        if !self.desc.capabilities.has(op.required_capability()) {
            return Err(unsupported("capability not in the rig description"));
        }
        match op {
            Op::SetFrequency(Vfo::A, hz) => {
                if hz.0 < MIN_HZ || hz.0 > MAX_HZ {
                    return Err(unsupported("frequency outside 100 kHz to 66 MHz"));
                }
                let word = phase_word(*hz).to_le_bytes();
                self.frame[2..6].copy_from_slice(&word);
                self.frame[6..10].copy_from_slice(&word);
            }
            Op::SetMode(mode) => {
                // Hamlib's mode write replaces the whole mode byte, which un-keys.
                self.frame[11] = match mode {
                    Mode::Cw => MODE_CW,
                    Mode::Am => MODE_OTHER,
                    _ => return Err(unsupported("mode the HiQSDR has not got")),
                };
            }
            Op::SetPtt(on) => {
                if self.frame[11] & MODE_CW != 0 {
                    return Err(RigError::new(
                        ErrorKind::Rejected,
                        "the HiQSDR does not key in CW mode".to_string(),
                    ));
                }
                if *on {
                    self.frame[11] |= KEYED;
                } else {
                    self.frame[11] &= !KEYED;
                }
            }
            _ => return Err(unsupported("not implemented for the HiQSDR engine")),
        }
        Ok(self.request())
    }

    // [@ANCHOR: radioctl_hiqsdr_engine_feed]
    fn feed(&mut self, bytes: &[u8], _now: Duration) -> Vec<Inbound> {
        // The radio sends nothing back on the control port.
        if bytes.is_empty() {
            Vec::new()
        } else {
            vec![Inbound::Unrecognised(bytes.to_vec())]
        }
    }
}

/// A virtual HiQSDR: it keeps the last control frame it was sent.
#[derive(Debug)]
pub struct HiqsdrTwin {
    frame: [u8; FRAME_LEN],
    default_hz: Hz,
    tuned: bool,
}

impl HiqsdrTwin {
    // [@ANCHOR: radioctl_hiqsdr_twin_new]
    /// A twin in the description's default state.
    pub fn new(desc: &RigDescription) -> Result<HiqsdrTwin, RigError> {
        let (_, name) = vendor_of(desc)?;
        if name != "hiqsdr" {
            return Err(unsupported("unknown HiQSDR dialect in the description"));
        }
        Ok(HiqsdrTwin {
            frame: initial_frame(),
            default_hz: desc.default_frequency,
            tuned: false,
        })
    }
}

impl VendorTwin for HiqsdrTwin {
    // [@ANCHOR: radioctl_hiqsdr_twin_snapshot]
    fn snapshot(&self) -> Option<TwinSnapshot> {
        let word = u32::from_le_bytes([self.frame[2], self.frame[3], self.frame[4], self.frame[5]]);
        Some(TwinSnapshot {
            frequency: if self.tuned {
                word_frequency(word)
            } else {
                self.default_hz
            },
            mode_code: Some(self.frame[11] & MODE_CW | (self.frame[11] & MODE_OTHER)),
            ptt: self.frame[11] & KEYED != 0,
        })
    }

    // [@ANCHOR: radioctl_hiqsdr_twin_feed]
    fn feed(&mut self, bytes: &[u8], _now: Duration) -> Vec<u8> {
        // One datagram is one frame; anything else is ignored, as a radio ignores junk.
        if bytes.len() == FRAME_LEN && bytes.starts_with(b"St") {
            if let Ok(frame) = <[u8; FRAME_LEN]>::try_from(bytes) {
                self.tuned = self.tuned || frame[2..6] != [0, 0, 0, 0];
                self.frame = frame;
            }
        }
        Vec::new()
    }

    // [@ANCHOR: radioctl_hiqsdr_twin_front_panel]
    fn front_panel(&mut self, panel: Panel) -> Result<Vec<u8>, String> {
        Err(format!("a HiQSDR twin has no front panel: {panel:?}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const T0: Duration = Duration::from_secs(1);

    fn shipped() -> RigDescription {
        let path = format!(
            "{}/../../rigs/n2adr-hiqsdr.toml",
            env!("CARGO_MANIFEST_DIR")
        );
        let text = std::fs::read_to_string(path).expect("rig file");
        radioctl_core::rigfile::parse(&text)
            .expect("loads")
            .description
    }

    // Tests [@ANCHOR: radioctl_hiqsdr_phase_word]
    // Tests [@ANCHOR: radioctl_hiqsdr_word_frequency]
    // Tests [@ANCHOR: radioctl_hiqsdr_initial_frame]
    #[test]
    fn the_phase_word_round_trips_to_the_hertz() {
        // 122.88 MHz / 2^32 is 0.0286 Hz per step: every hertz of the range survives.
        for hz in [
            100_000, 7_074_000, 14_074_321, 28_000_001, 65_999_999, 66_000_000,
        ] {
            assert_eq!(word_frequency(phase_word(Hz(hz))), Hz(hz), "{hz}");
        }
        // 1/12 of the clock is exactly 2^32 / 12 (rounded).
        assert_eq!(phase_word(Hz(10_240_000)), 357_913_941);
        let frame = initial_frame();
        assert_eq!(&frame[..2], b"St");
        assert_eq!((frame[10], frame[11], frame[12]), (120, 0x02, 4));
    }

    // Tests [@ANCHOR: radioctl_hiqsdr_engine_new]
    #[test]
    fn other_families_are_refused() {
        assert!(HiqsdrEngine::new(RigDescription::ts2000()).is_err());
        assert!(HiqsdrTwin::new(&RigDescription::ts2000()).is_err());
    }

    // Tests [@ANCHOR: radioctl_hiqsdr_engine_encode]
    // Tests [@ANCHOR: radioctl_hiqsdr_engine_feed]
    // Tests [@ANCHOR: radioctl_hiqsdr_twin_new]
    // Tests [@ANCHOR: radioctl_hiqsdr_twin_feed]
    // Tests [@ANCHOR: radioctl_hiqsdr_twin_snapshot]
    // Tests [@ANCHOR: radioctl_hiqsdr_twin_front_panel]
    #[test]
    fn every_command_sends_the_whole_frame_and_the_twin_keeps_it() {
        let desc = shipped();
        let mut e = HiqsdrEngine::new(desc.clone()).expect("engine");
        let mut t = HiqsdrTwin::new(&desc).expect("twin");
        assert_eq!(
            t.snapshot().expect("snapshot").frequency,
            desc.default_frequency
        );
        let r = e
            .encode(&Op::SetFrequency(Vfo::A, Hz(7_074_000)))
            .expect("encodes");
        assert_eq!(r.bytes.len(), FRAME_LEN);
        assert_eq!(r.reply, ReplyKind::None);
        assert_eq!(&r.bytes[2..6], &r.bytes[6..10], "split is not supported");
        assert!(t.feed(&r.bytes, T0).is_empty());
        let snap = t.snapshot().expect("snapshot");
        assert_eq!(snap.frequency, Hz(7_074_000));
        assert!(!snap.ptt);
        // Key, then change mode: Hamlib's mode write replaces the byte, which un-keys.
        let r = e.encode(&Op::SetPtt(true)).expect("keys");
        assert_eq!(r.bytes[11], 0x0a);
        t.feed(&r.bytes, T0);
        assert!(t.snapshot().expect("snapshot").ptt);
        let r = e.encode(&Op::SetMode(Mode::Cw)).expect("cw");
        assert_eq!(r.bytes[11], 0x01);
        t.feed(&r.bytes, T0);
        let snap = t.snapshot().expect("snapshot");
        assert!(!snap.ptt);
        assert_eq!(snap.mode_code, Some(1));
        // No keying in CW mode; back to the other mode, unkey.
        assert_eq!(
            e.encode(&Op::SetPtt(true)).expect_err("cw").kind,
            ErrorKind::Rejected
        );
        let r = e.encode(&Op::SetMode(Mode::Am)).expect("am");
        t.feed(&r.bytes, T0);
        assert_eq!(t.snapshot().expect("snapshot").mode_code, Some(2));
        let r = e.encode(&Op::SetPtt(true)).expect("keys");
        let r2 = e.encode(&Op::SetPtt(false)).expect("unkeys");
        assert_eq!(r.bytes[11], 0x0a);
        assert_eq!(r2.bytes[11], 0x02);
        assert_eq!(&r2.bytes[2..6], &r.bytes[2..6], "tuning is kept");
        // Refusals and junk.
        assert!(e.encode(&Op::SetFrequency(Vfo::A, Hz(99_999))).is_err());
        assert!(e.encode(&Op::SetFrequency(Vfo::A, Hz(66_000_001))).is_err());
        assert!(e.encode(&Op::SetMode(Mode::Usb)).is_err());
        assert!(e.encode(&Op::GetFrequency(Vfo::A)).is_err());
        assert!(t.feed(b"St", T0).is_empty());
        assert!(t.feed(&[0u8; FRAME_LEN], T0).is_empty());
        assert!(matches!(
            e.feed(b"?", T0).as_slice(),
            [Inbound::Unrecognised(_)]
        ));
        assert!(e.feed(b"", T0).is_empty());
        assert!(t.front_panel(Panel::Ptt(true)).is_err());
    }
}
