// SPDX-License-Identifier: AGPL-3.0-or-later
//! AOR AR7030 and AR7030 Plus: the binary memory-access remote protocol. Sans-IO client engine and
//! virtual twin.
//!
//! **Sources.** AOR's "AR-7030 Computer remote control protocol" text (firmware 1.1A to 1.4B;
//! `~/manuals/aor/textfiles_ar7030_rs232_command_list.pdf`, sha256
//! 88a9c5e27d0abd8854bf8cf11b1e18bd563e98cf5bd835f75231529453e00edf): serial line (section 3,
//! PDF page 1), operation codes (section 6, page 2), memory pages and lock levels (sections 7 and 8),
//! routines (page 3 to 4), working memory addresses (section 14: frequency at 0x1A, mode at 0x1D).
//! The document says nothing about the AR7030 Plus; Hamlib 4.6.2's `rigs/aor/ar7030.c` and
//! `ar7030p.c` (released source tarball, sha256
//! b2ac73f44dd1161e95fdee6c95276144757647bf92d7fdb369ee2fe41ed47ae8) were read for facts only and use
//! the same operation codes and addresses for the Plus. No code or comment was copied.
//!
//! * 1200 baud, 8N1, no flow control. Every byte sent is one command: the high nibble is the
//!   operation, the low nibble its data. `5p` selects page p, `3h` loads the H register, `4a` sets the
//!   low address byte from H and a, `1a` the high address nibble, `6d` writes the byte H:d at the
//!   address and increments it, `7x` reads the byte at the address and advances by x (the radio
//!   sends one byte), `8l` sets the lock level, `2r` executes routine r (4 sets everything from
//!   memory, 14 sends the signal strength as one byte), `0` does nothing.
//! * Frequency: 24 bits at page 0, address 0x1A, most significant byte first; the step is
//!   44.545 MHz / 2^24 (about 2.655 Hz). Mode: one byte at 0x1D (1 AM, 2 synchronous AM, 3 narrow
//!   FM, 4 data, 5 CW, 6 LSB, 7 USB).
//! * Setting: lock level 1, set the address, write the bytes, execute routine 4, unlock (Hamlib's
//!   order; the document's finer routines 1 and 2 would also do).
//!
//! Not implemented: the second VFO of the Plus, memories, filters, AGC, volume, the buttons, the
//! real-time clock, power on and off.

use std::time::Duration;

use radioctl_core::description::{Family, ReceiverProtocol, RigDescription};
use radioctl_core::engine::{Inbound, ProtocolEngine, ReplyKind, Request, PARTIAL_FRAME_TIMEOUT};
use radioctl_core::error::{ErrorKind, RigError};
use radioctl_core::op::{Field, Observation, Op};
use radioctl_core::units::{Hz, Vfo};

use crate::receiver::ReceiverTwin;
use crate::virtual_rig::RigState;

/// Page 0 address of the 24-bit tuned frequency.
const FREQU: u16 = 0x1A;

/// Page 0 address of the mode byte.
const MODE: u16 = 0x1D;

/// The DDS step numerator and denominator: hertz per count is 44 545 000 / 2^24.
const STEP_NUM: u64 = 44_545_000;
const STEP_DEN: u64 = 1 << 24;

/// The highest frequency Hamlib's AR7030 code accepts (counts 12 058 624).
const MAX_HZ: u64 = 32_000_000;

/// True when the description's dialect is `ar7030` or `ar7030p`.
// [@ANCHOR: radioctl_ar7030_handles]
pub fn handles(desc: &RigDescription) -> bool {
    matches!(
        desc.receiver.as_ref().map(|r| r.dialect.as_str()),
        Some("ar7030" | "ar7030p")
    )
}

// [@ANCHOR: radioctl_ar7030_counts_of]
/// The 24-bit count for a frequency, rounded to the nearest step.
fn counts_of(hz: u64) -> u32 {
    let hz = hz.min(1_000_000_000);
    let n = (hz * STEP_DEN * 2 + STEP_NUM) / (2 * STEP_NUM);
    u32::try_from(n.min(0xFF_FFFF)).unwrap_or(0xFF_FFFF)
}

// [@ANCHOR: radioctl_ar7030_hz_of]
/// The frequency of a 24-bit count, rounded to the nearest hertz.
fn hz_of(counts: u32) -> u64 {
    (u64::from(counts) * STEP_NUM + STEP_DEN / 2) / STEP_DEN
}

// [@ANCHOR: radioctl_ar7030_set_address]
/// The bytes that select a page and address.
fn set_address(page: u8, address: u16) -> Vec<u8> {
    let mut out = vec![
        0x50 | (page & 0x0F),
        0x30 | u8::try_from((address >> 4) & 0x0F).unwrap_or(0),
        0x40 | u8::try_from(address & 0x0F).unwrap_or(0),
    ];
    if address > 0xFF {
        out.push(0x10 | u8::try_from(address >> 8).unwrap_or(0));
    }
    out
}

// [@ANCHOR: radioctl_ar7030_write_byte]
/// The bytes that write one byte at the current address.
fn write_byte(value: u8) -> [u8; 2] {
    [0x30 | (value >> 4), 0x60 | (value & 0x0F)]
}

/// The engine of the AR7030 and AR7030 Plus.
#[derive(Debug)]
pub struct Ar7030Engine {
    desc: RigDescription,
    expecting: ReplyKind,
    want: usize,
    buffer: Vec<u8>,
    last_byte_at: Duration,
}

// [@ANCHOR: radioctl_ar7030_engine_new]
/// The engine for an AR7030 or AR7030 Plus description.
pub fn engine(desc: RigDescription) -> Result<Ar7030Engine, RigError> {
    let right = desc.family == Family::Receiver
        && handles(&desc)
        && desc
            .receiver
            .as_ref()
            .is_some_and(|r| r.protocol == ReceiverProtocol::Aor);
    if !right {
        return Err(RigError::new(
            ErrorKind::Unsupported,
            "not an AR7030 or AR7030 Plus description",
        ));
    }
    Ok(Ar7030Engine {
        desc,
        expecting: ReplyKind::None,
        want: 0,
        buffer: Vec::new(),
        last_byte_at: Duration::ZERO,
    })
}

impl ProtocolEngine for Ar7030Engine {
    // [@ANCHOR: radioctl_ar7030_engine_description]
    fn description(&self) -> &RigDescription {
        &self.desc
    }

    // [@ANCHOR: radioctl_ar7030_engine_encode]
    fn encode(&mut self, op: &Op) -> Result<Request, RigError> {
        if !self.desc.capabilities.has(op.required_capability()) {
            return Err(RigError::new(
                ErrorKind::Unsupported,
                "capability not in the rig description",
            ));
        }
        let unsupported = |why: &str| RigError::new(ErrorKind::Unsupported, why.to_string());
        let (bytes, reply, want) = match op {
            Op::GetFrequency(Vfo::A) => {
                let mut b = set_address(0, FREQU);
                b.extend_from_slice(&[0x71, 0x71, 0x71]);
                (b, ReplyKind::Observation(Field::Frequency(Vfo::A)), 3)
            }
            Op::GetMode => {
                let mut b = set_address(0, MODE);
                b.push(0x71);
                (b, ReplyKind::Observation(Field::Mode), 1)
            }
            Op::GetSignal => (vec![0x2E], ReplyKind::Observation(Field::Signal), 1),
            Op::SetFrequency(Vfo::A, hz) => {
                if hz.0 > MAX_HZ {
                    return Err(unsupported("frequency out of range"));
                }
                let counts = counts_of(hz.0);
                let mut b = vec![0x81];
                b.extend(set_address(0, FREQU));
                for byte in counts.to_be_bytes().iter().skip(1) {
                    b.extend_from_slice(&write_byte(*byte));
                }
                b.extend_from_slice(&[0x24, 0x80]);
                (b, ReplyKind::None, 0)
            }
            Op::SetMode(mode) => {
                let code = self
                    .desc
                    .mode_code(*mode)
                    .ok_or_else(|| unsupported("mode not in the rig description"))?;
                let mut b = vec![0x81];
                b.extend(set_address(0, MODE));
                b.extend_from_slice(&write_byte(code));
                b.extend_from_slice(&[0x24, 0x80]);
                (b, ReplyKind::None, 0)
            }
            _ => return Err(unsupported("not implemented for the AR7030 engine")),
        };
        self.expecting = reply;
        self.want = want;
        self.buffer.clear();
        Ok(Request { bytes, reply })
    }

    // [@ANCHOR: radioctl_ar7030_engine_feed]
    fn feed(&mut self, bytes: &[u8], now: Duration) -> Vec<Inbound> {
        let mut out = Vec::new();
        if !self.buffer.is_empty() && now.saturating_sub(self.last_byte_at) > PARTIAL_FRAME_TIMEOUT
        {
            out.push(Inbound::Unrecognised(std::mem::take(&mut self.buffer)));
        }
        self.last_byte_at = now;
        let ReplyKind::Observation(field) = self.expecting else {
            // The radio sends nothing unasked; stray bytes are reported, not guessed at.
            if !bytes.is_empty() {
                out.push(Inbound::Unrecognised(bytes.to_vec()));
            }
            return out;
        };
        self.buffer.extend_from_slice(bytes);
        if self.buffer.len() < self.want {
            return out;
        }
        let frame: Vec<u8> = self.buffer.drain(..self.want).collect();
        self.buffer.clear();
        self.expecting = ReplyKind::None;
        let found = match (field, frame.as_slice()) {
            (Field::Frequency(_), [a, b, c]) => Some(Observation::Frequency(
                Vfo::A,
                Hz(hz_of(u32::from_be_bytes([0, *a, *b, *c]))),
            )),
            (Field::Mode, [code]) => self.desc.code_mode(*code).map(Observation::Mode),
            (Field::Signal, [level]) => Some(Observation::Signal(u16::from(*level))),
            _ => None,
        };
        match found {
            Some(o) => out.push(Inbound::Observation(o)),
            None => out.push(Inbound::Unrecognised(frame)),
        }
        out
    }
}

/// The virtual AR7030 or AR7030 Plus: the radio's address, H and lock registers and page 0.
#[derive(Debug)]
pub struct Ar7030Twin {
    desc: RigDescription,
    page: u8,
    address: u16,
    h: u8,
    lock: u8,
    working: [u8; 256],
}

impl Ar7030Twin {
    // [@ANCHOR: radioctl_ar7030_twin_new]
    /// The twin of an AR7030 or AR7030 Plus description.
    pub fn new(desc: &RigDescription) -> Result<Ar7030Twin, RigError> {
        if !handles(desc) {
            return Err(RigError::new(
                ErrorKind::Unsupported,
                "not an AR7030 or AR7030 Plus dialect",
            ));
        }
        Ok(Ar7030Twin {
            desc: desc.clone(),
            page: 0,
            address: 0,
            h: 0,
            lock: 0,
            working: [0; 256],
        })
    }

    // [@ANCHOR: radioctl_ar7030_twin_refresh]
    /// Copies the front-panel state into the working memory the controller reads.
    fn refresh(&mut self, state: &RigState) {
        let counts = counts_of(state.freq_a.0).to_be_bytes();
        self.working[usize::from(FREQU)..usize::from(FREQU) + 3].copy_from_slice(&counts[1..]);
        self.working[usize::from(MODE)] = self.desc.mode_code(state.mode).unwrap_or(1);
    }

    // [@ANCHOR: radioctl_ar7030_twin_read]
    /// The byte at the current page and address.
    fn read(&self) -> u8 {
        match self.page {
            0 => self
                .working
                .get(usize::from(self.address))
                .copied()
                .unwrap_or(0),
            15 => b"7030_14A"
                .get(usize::from(self.address))
                .copied()
                .unwrap_or(0),
            _ => 0,
        }
    }

    // [@ANCHOR: radioctl_ar7030_twin_execute]
    /// Runs a routine; the bytes it sends, if any.
    fn execute(&mut self, state: &mut RigState, routine: u8) -> Vec<u8> {
        match routine {
            // Set frequency, set mode, set all: apply working memory to the radio.
            1 | 2 | 4 => {
                let f = &self.working[usize::from(FREQU)..usize::from(FREQU) + 3];
                let counts = u32::from_be_bytes([0, f[0], f[1], f[2]]);
                let hz = hz_of(counts);
                if hz <= MAX_HZ {
                    state.freq_a = Hz(hz);
                }
                if let Some(mode) = self.desc.code_mode(self.working[usize::from(MODE)]) {
                    state.mode = mode;
                }
                Vec::new()
            }
            14 => vec![u8::try_from(state.signal.min(255)).unwrap_or(255)],
            _ => Vec::new(),
        }
    }
}

impl ReceiverTwin for Ar7030Twin {
    // [@ANCHOR: radioctl_ar7030_twin_line]
    fn line(&mut self, _state: &mut RigState, _line: &str) -> Vec<u8> {
        // The protocol has no lines; bytes arrive through `raw`.
        Vec::new()
    }

    // [@ANCHOR: radioctl_ar7030_twin_raw]
    fn raw(&mut self, state: &mut RigState, bytes: &[u8]) -> Option<Vec<u8>> {
        let mut out = Vec::new();
        for byte in bytes {
            let x = byte & 0x0F;
            match byte >> 4 {
                0x1 => self.address = (self.address & 0x0FF) | (u16::from(x) << 8),
                0x2 => out.extend(self.execute(state, x)),
                0x3 => self.h = x,
                0x4 => {
                    self.address = (self.address & 0xF00) | (u16::from(self.h) << 4) | u16::from(x);
                    self.h = 0;
                }
                0x5 => self.page = x,
                0x6 => {
                    if self.page == 0 {
                        if let Some(slot) = self.working.get_mut(usize::from(self.address)) {
                            *slot = (self.h << 4) | x;
                        }
                    }
                    self.address = self.address.wrapping_add(1);
                    self.h = 0;
                }
                0x7 => {
                    self.refresh(state);
                    out.push(self.read());
                    self.address = self.address.wrapping_add(u16::from(x));
                }
                0x8 => self.lock = x,
                _ => {}
            }
        }
        Some(out)
    }
}

#[cfg(test)]
mod tests {
    use radioctl_core::units::Mode;

    use super::*;

    const T0: Duration = Duration::from_secs(1);

    fn shipped(id: &str) -> RigDescription {
        let path = format!("{}/../../rigs/{id}.toml", env!("CARGO_MANIFEST_DIR"));
        let text = std::fs::read_to_string(path).expect("rig file");
        radioctl_core::rigfile::parse(&text)
            .expect("loads")
            .description
    }

    fn fresh_state(desc: &RigDescription) -> RigState {
        crate::virtual_rig::VirtualRig::from_description(desc)
            .expect("rig")
            .state()
            .clone()
    }

    // Tests [@ANCHOR: radioctl_ar7030_handles]
    // Tests [@ANCHOR: radioctl_ar7030_counts_of]
    // Tests [@ANCHOR: radioctl_ar7030_hz_of]
    // Tests [@ANCHOR: radioctl_ar7030_set_address]
    // Tests [@ANCHOR: radioctl_ar7030_write_byte]
    // Tests [@ANCHOR: radioctl_ar7030_engine_new]
    // Tests [@ANCHOR: radioctl_ar7030_engine_description]
    // Tests [@ANCHOR: radioctl_ar7030_engine_encode]
    #[test]
    fn commands_are_nibble_operations() {
        let mut e = engine(shipped("aor-ar7030")).expect("engine");
        assert_eq!(e.description().id, "aor-ar7030");
        // Frequency: 7 074 000 Hz is 2 664 318 counts (0x28A77E).
        assert_eq!(counts_of(7_074_000), 0x28_A7_7E);
        assert_eq!(hz_of(counts_of(7_174_000)), 7_174_000);
        assert_eq!(
            e.encode(&Op::GetFrequency(Vfo::A)).expect("get").bytes,
            [0x50, 0x31, 0x4A, 0x71, 0x71, 0x71]
        );
        assert_eq!(
            e.encode(&Op::GetMode).expect("get").bytes,
            [0x50, 0x31, 0x4D, 0x71]
        );
        assert_eq!(e.encode(&Op::GetSignal).expect("get").bytes, [0x2E]);
        let set = e.encode(&Op::SetMode(Mode::Usb)).expect("set").bytes;
        assert_eq!(set, [0x81, 0x50, 0x31, 0x4D, 0x30, 0x67, 0x24, 0x80]);
        let freq = e
            .encode(&Op::SetFrequency(Vfo::A, Hz(7_074_000)))
            .expect("set")
            .bytes;
        assert_eq!(
            freq,
            [0x81, 0x50, 0x31, 0x4A, 0x32, 0x68, 0x3A, 0x67, 0x37, 0x6E, 0x24, 0x80]
        );
        assert!(e.encode(&Op::SetFrequency(Vfo::A, Hz(33_000_000))).is_err());
        assert!(e.encode(&Op::SetMode(Mode::Wfm)).is_err());
        assert!(e.encode(&Op::SetPtt(true)).is_err());
        assert_eq!(set_address(2, 0x1F4), [0x52, 0x3F, 0x44, 0x11]);
        assert!(handles(&shipped("aor-ar7030p")) && !handles(&shipped("aor-ar5000")));
        assert!(engine(shipped("aor-ar5000")).is_err());
    }

    // Tests [@ANCHOR: radioctl_ar7030_engine_feed]
    #[test]
    fn replies_are_raw_bytes_counted_out() {
        let mut e = engine(shipped("aor-ar7030p")).expect("engine");
        e.encode(&Op::GetFrequency(Vfo::A)).expect("get");
        let counts = counts_of(14_200_000).to_be_bytes();
        assert!(
            e.feed(&counts[1..3], T0).is_empty(),
            "waits for the third byte"
        );
        assert_eq!(
            e.feed(&counts[3..], T0),
            vec![Inbound::Observation(Observation::Frequency(
                Vfo::A,
                Hz(hz_of(counts_of(14_200_000)))
            ))]
        );
        e.encode(&Op::GetMode).expect("get");
        assert_eq!(
            e.feed(&[6], T0),
            vec![Inbound::Observation(Observation::Mode(Mode::Lsb))]
        );
        e.encode(&Op::GetMode).expect("get");
        assert!(matches!(
            e.feed(&[9], T0).as_slice(),
            [Inbound::Unrecognised(_)]
        ));
        e.encode(&Op::GetSignal).expect("get");
        assert_eq!(
            e.feed(&[200], T0),
            vec![Inbound::Observation(Observation::Signal(200))]
        );
        e.encode(&Op::SetMode(Mode::Am)).expect("set");
        assert!(matches!(
            e.feed(&[1], T0).as_slice(),
            [Inbound::Unrecognised(_)]
        ));
        let later = T0 + Duration::from_secs(5);
        e.encode(&Op::GetFrequency(Vfo::A)).expect("get");
        assert!(e.feed(&[1], later).is_empty());
        let seen = e.feed(&[2, 3], later + Duration::from_secs(5));
        assert!(matches!(seen.as_slice(), [Inbound::Unrecognised(_), ..]));
    }

    // Tests [@ANCHOR: radioctl_ar7030_twin_new]
    // Tests [@ANCHOR: radioctl_ar7030_twin_refresh]
    // Tests [@ANCHOR: radioctl_ar7030_twin_read]
    // Tests [@ANCHOR: radioctl_ar7030_twin_execute]
    // Tests [@ANCHOR: radioctl_ar7030_twin_line]
    // Tests [@ANCHOR: radioctl_ar7030_twin_raw]
    #[test]
    fn the_twin_runs_the_documented_operations() {
        let desc = shipped("aor-ar7030");
        let mut twin = Ar7030Twin::new(&desc).expect("twin");
        let mut state = fresh_state(&desc);
        let mut e = engine(desc.clone()).expect("engine");
        // Set 7 174 000 Hz and USB through the engine's own bytes.
        for op in [
            Op::SetFrequency(Vfo::A, Hz(7_174_000)),
            Op::SetMode(Mode::Usb),
        ] {
            let bytes = e.encode(&op).expect("encode").bytes;
            assert_eq!(twin.raw(&mut state, &bytes), Some(Vec::new()));
        }
        assert_eq!(state.freq_a, Hz(7_174_000));
        assert_eq!(state.mode, Mode::Usb);
        let bytes = e.encode(&Op::GetFrequency(Vfo::A)).expect("get").bytes;
        let answer = twin.raw(&mut state, &bytes).expect("answer");
        assert_eq!(answer.len(), 3);
        assert_eq!(
            e.feed(&answer, T0),
            vec![Inbound::Observation(Observation::Frequency(
                Vfo::A,
                Hz(7_174_000)
            ))]
        );
        state.signal = 77;
        assert_eq!(twin.raw(&mut state, &[0x2E]), Some(vec![77]));
        // The identity page.
        let mut ident = Vec::new();
        ident.extend(
            twin.raw(&mut state, &[0x5F, 0x30, 0x40, 0x71, 0x71])
                .expect("ident"),
        );
        assert_eq!(ident, b"70");
        assert!(twin.line(&mut state, "D").is_empty());
        assert!(Ar7030Twin::new(&shipped("aor-ar5000")).is_err());
    }
}
