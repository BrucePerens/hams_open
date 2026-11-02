// SPDX-License-Identifier: AGPL-3.0-or-later
//! SAT-Schneider DRT1 DRM receiver: sans-IO client engine and virtual twin.
//!
//! **Sources.** Facts only, read from Hamlib 4.6.2 (released tarball, sha256
//! b2ac73f44dd1161e95fdee6c95276144757647bf92d7fdb369ee2fe41ed47ae8): `rigs/kit/drt1.c`
//! (`drt1_caps`, `drt1_set_freq`, `ad_write_reg`). No SAT-Schneider document was read; nothing is
//! verified against a radio, and no code or comment was copied.
//!
//! The DRT1 has no byte stream. Its local oscillator is an AD9951 DDS that the host programs with
//! the serial port's control lines, bit by bit: RTS carries the data (inverted: the line is
//! asserted for a 0 bit, because the level converter inverts), the break state of the transmit
//! line is the serial clock (a bit is taken when break is set, then it is cleared), and DTR
//! frames one register write (raised before the instruction byte, dropped after the last data
//! bit, which latches the register). A register write is the instruction byte (a 5-bit register
//! address, most significant bit first, bit 7 zero for a write) and then 3 or 4 data bytes, most
//! significant bit first. A tune writes CFR2 (address 1, 3 bytes: reference multiplier in bits
//! 7..3, the VCO gain bit 2, the charge pump current in bits 1..0), then FTW0 (address 4, 4
//! bytes): `(f + IF) / (osc * multiplier) * 2^32`, truncated. The defaults are the oscillator at
//! 45.012 MHz, a 45 MHz IF offset, multiplier 8 and 150 uA, which is 0x47 in CFR2. The receive
//! range is 50 kHz to 30 MHz, AM only; there is no read-back and no PTT.

use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use radioctl_core::description::RigDescription;
use radioctl_core::engine::{Inbound, LineStep, ProtocolEngine, Request};
use radioctl_core::error::{ErrorKind, RigError};
use radioctl_core::keyer::{ControlLine, ControlLines, KeyLine};
use radioctl_core::op::Op;
use radioctl_core::transcript::Panel;
use radioctl_core::units::{Hz, Vfo};

use crate::serial_vendor::{vendor_of, TwinSnapshot, VendorTwin};

/// The DDS reference oscillator, Hz (Hamlib's default).
pub const OSC_HZ: f64 = 45_012_000.0;
/// The IF offset added to the receive frequency, Hz (Hamlib's default).
pub const IF_HZ: f64 = 45_000_000.0;
/// The reference clock multiplier (Hamlib's default).
pub const REF_MULT: u32 = 8;
/// The charge pump current in microamps (Hamlib's default).
pub const PUMP_UA: u32 = 150;
/// Lowest and highest receive frequency, Hz (Hamlib's range).
pub const RANGE_HZ: (u64, u64) = (50_000, 30_000_000);
/// How long each line change is held. Our choice: Hamlib waits with a short spin loop of no
/// stated duration, and the DDS pins sit behind 1 kOhm resistors, so a slow clock is wanted; a
/// USB serial adapter adds about a millisecond of its own to every change.
pub const STEP_HOLD: Duration = Duration::from_micros(20);

/// CFR2 register address.
const CFR2: u32 = 1;
/// FTW0 register address.
const FTW0: u32 = 4;

fn unsupported(what: &str) -> RigError {
    RigError::new(ErrorKind::Unsupported, what.to_string())
}

// [@ANCHOR: radioctl_drt1_cfr2_word]
/// The CFR2 data word for a reference multiplier and a charge pump current in microamps (75 to
/// 150 in steps of 25).
pub fn cfr2_word(ref_mult: u32, pump_ua: u32) -> u32 {
    ((ref_mult << 3) & 0xf8) | 0x4 | ((pump_ua.saturating_sub(75) / 25) & 0x3)
}

// [@ANCHOR: radioctl_drt1_tuning_word]
/// The FTW0 tuning word for a receive frequency (truncated, as the DDS arithmetic is).
pub fn tuning_word(hz: Hz) -> u32 {
    let ratio = (hz.0 as f64 + IF_HZ) / (OSC_HZ * f64::from(REF_MULT));
    (ratio * 4_294_967_296.0) as u64 as u32
}

// [@ANCHOR: radioctl_drt1_word_frequency]
/// The receive frequency a tuning word and a CFR2 word stand for, to the nearest hertz.
pub fn word_frequency(ftw: u32, cfr2: u32) -> Hz {
    let mult = f64::from((cfr2 >> 3) & 0x1f);
    let lo = f64::from(ftw) / 4_294_967_296.0 * OSC_HZ * mult;
    Hz((lo - IF_HZ).round().max(0.0) as u64)
}

fn step(line: ControlLine, asserted: bool) -> LineStep {
    LineStep {
        line,
        asserted,
        hold: STEP_HOLD,
    }
}

// [@ANCHOR: radioctl_drt1_write_register]
/// The line steps of one register write: DTR up, the instruction byte, `bytes` data bytes of
/// `data`, DTR down. Each bit sets RTS (asserted for a 0), then sets and clears break.
pub fn write_register(addr: u32, bytes: u32, data: u32) -> Vec<LineStep> {
    let mut steps = vec![
        step(ControlLine::Break, false),
        step(ControlLine::Dtr, true),
    ];
    let bit = |value: bool, steps: &mut Vec<LineStep>| {
        steps.push(step(ControlLine::Rts, !value));
        steps.push(step(ControlLine::Break, true));
        steps.push(step(ControlLine::Break, false));
    };
    let instruction = addr & 0x1f;
    for i in (0..8).rev() {
        bit(instruction & (1 << i) != 0, &mut steps);
    }
    for i in (0..bytes * 8).rev() {
        bit(data & (1 << i) != 0, &mut steps);
    }
    steps.push(step(ControlLine::Dtr, false));
    steps
}

// [@ANCHOR: radioctl_drt1_tune_steps]
/// The whole tune: the lines to idle (DTR down, RTS down, break clear), CFR2, then FTW0.
pub fn tune_steps(hz: Hz) -> Vec<LineStep> {
    let mut steps = vec![
        step(ControlLine::Dtr, false),
        step(ControlLine::Rts, false),
        step(ControlLine::Break, false),
    ];
    steps.extend(write_register(CFR2, 3, cfr2_word(REF_MULT, PUMP_UA)));
    steps.extend(write_register(FTW0, 4, tuning_word(hz)));
    steps
}

/// The DRT1 client engine. It has no bytes: its commands are line sequences.
#[derive(Debug)]
pub struct Drt1Engine {
    desc: RigDescription,
}

impl Drt1Engine {
    // [@ANCHOR: radioctl_drt1_engine_new]
    /// Builds an engine from a DRT1 vendor description.
    pub fn new(desc: RigDescription) -> Result<Drt1Engine, RigError> {
        let (_, dialect) = vendor_of(&desc)?;
        if dialect != "drt1" {
            return Err(unsupported("unknown DRT1 dialect in the description"));
        }
        Ok(Drt1Engine { desc })
    }
}

impl ProtocolEngine for Drt1Engine {
    fn description(&self) -> &RigDescription {
        &self.desc
    }

    // [@ANCHOR: radioctl_drt1_engine_encode_lines]
    fn encode_lines(&mut self, op: &Op) -> Result<Option<Vec<LineStep>>, RigError> {
        if !self.desc.capabilities.has(op.required_capability()) {
            return Err(unsupported("capability not in the rig description"));
        }
        match op {
            Op::SetFrequency(Vfo::A, hz) if (RANGE_HZ.0..=RANGE_HZ.1).contains(&hz.0) => {
                Ok(Some(tune_steps(*hz)))
            }
            Op::SetFrequency(Vfo::A, _) => Err(RigError::new(
                ErrorKind::Malformed,
                "the DRT1 receives 50 kHz to 30 MHz",
            )),
            _ => Err(unsupported("not implemented for the DRT1 engine")),
        }
    }

    // [@ANCHOR: radioctl_drt1_engine_encode]
    fn encode(&mut self, _op: &Op) -> Result<Request, RigError> {
        Err(unsupported(
            "the DRT1 is commanded by control-line sequences, not bytes",
        ))
    }

    // [@ANCHOR: radioctl_drt1_engine_feed]
    fn feed(&mut self, bytes: &[u8], _now: Duration) -> Vec<Inbound> {
        if bytes.is_empty() {
            Vec::new()
        } else {
            vec![Inbound::Unrecognised(bytes.to_vec())]
        }
    }
}

#[derive(Debug)]
struct DdsState {
    rts: bool,
    dtr: bool,
    brk: bool,
    /// Bits clocked in since DTR went up, oldest first (the value is the data bit).
    bits: Vec<bool>,
    cfr2: Option<u32>,
    ftw0: Option<u32>,
    hz: Hz,
}

impl DdsState {
    // [@ANCHOR: radioctl_drt1_dds_latch]
    fn latch(&mut self) {
        let Some((instruction, data)) = self.bits.split_at_checked(8) else {
            return;
        };
        let word = |bits: &[bool]| bits.iter().fold(0u32, |acc, b| (acc << 1) | u32::from(*b));
        let instruction = word(instruction);
        if instruction & 0x80 != 0 {
            return;
        }
        match (instruction & 0x1f, data.len()) {
            (CFR2, 24) => self.cfr2 = Some(word(data)),
            (FTW0, 32) => self.ftw0 = Some(word(data)),
            _ => return,
        }
        if let (Some(ftw), Some(cfr2)) = (self.ftw0, self.cfr2) {
            self.hz = word_frequency(ftw, cfr2);
        }
    }
}

/// A virtual DRT1: the AD9951 serial port, decoded from the line changes. Clone it to hand one
/// handle to the actor as its control lines and keep another to look at the tuning.
#[derive(Debug, Clone)]
pub struct Drt1Twin {
    state: Arc<Mutex<DdsState>>,
}

fn locked(state: &Mutex<DdsState>) -> MutexGuard<'_, DdsState> {
    state
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

impl Drt1Twin {
    // [@ANCHOR: radioctl_drt1_twin_new]
    /// A twin in the description's default state.
    pub fn new(desc: &RigDescription) -> Result<Drt1Twin, RigError> {
        vendor_of(desc)?;
        Ok(Drt1Twin {
            state: Arc::new(Mutex::new(DdsState {
                rts: false,
                dtr: false,
                brk: false,
                bits: Vec::new(),
                cfr2: None,
                ftw0: None,
                hz: desc.default_frequency,
            })),
        })
    }

    // [@ANCHOR: radioctl_drt1_twin_frequency]
    /// The frequency the twin is tuned to (the default until both registers were written).
    pub fn frequency(&self) -> Hz {
        locked(&self.state).hz
    }

    // [@ANCHOR: radioctl_drt1_twin_registers]
    /// The CFR2 and FTW0 words last latched, if any.
    pub fn registers(&self) -> (Option<u32>, Option<u32>) {
        let state = locked(&self.state);
        (state.cfr2, state.ftw0)
    }
}

impl ControlLines for Drt1Twin {
    // [@ANCHOR: radioctl_drt1_twin_set_line]
    fn set_line(&self, _line: KeyLine, _asserted: bool) -> Result<(), RigError> {
        Err(unsupported("a DRT1 has no keying line"))
    }

    // [@ANCHOR: radioctl_drt1_twin_set_control]
    fn set_control(&self, line: ControlLine, asserted: bool) -> Result<(), RigError> {
        let mut s = locked(&self.state);
        match line {
            ControlLine::Rts => s.rts = asserted,
            ControlLine::Dtr => {
                if asserted && !s.dtr {
                    s.bits.clear();
                }
                if !asserted && s.dtr {
                    s.latch();
                }
                s.dtr = asserted;
            }
            ControlLine::Break => {
                if asserted && !s.brk && s.dtr {
                    let bit = !s.rts;
                    s.bits.push(bit);
                }
                s.brk = asserted;
            }
        }
        Ok(())
    }
}

impl VendorTwin for Drt1Twin {
    // [@ANCHOR: radioctl_drt1_twin_snapshot]
    fn snapshot(&self) -> Option<TwinSnapshot> {
        Some(TwinSnapshot {
            frequency: self.frequency(),
            mode_code: None,
            ptt: false,
        })
    }

    // [@ANCHOR: radioctl_drt1_twin_feed]
    fn feed(&mut self, _bytes: &[u8], _now: Duration) -> Vec<u8> {
        Vec::new()
    }

    // [@ANCHOR: radioctl_drt1_twin_front_panel]
    fn front_panel(&mut self, panel: Panel) -> Result<Vec<u8>, String> {
        match panel {
            Panel::Frequency(hz) => locked(&self.state).hz = hz,
            other => return Err(format!("a DRT1 twin has no {other:?} knob")),
        }
        Ok(Vec::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shipped() -> RigDescription {
        let path = format!(
            "{}/../../rigs/sat-schneider-drt1.toml",
            env!("CARGO_MANIFEST_DIR")
        );
        let text = std::fs::read_to_string(path).expect("rig file");
        radioctl_core::rigfile::parse(&text)
            .expect("loads")
            .description
    }

    fn drive(twin: &Drt1Twin, steps: &[LineStep]) {
        for s in steps {
            twin.set_control(s.line, s.asserted).expect("line");
        }
    }

    // Tests [@ANCHOR: radioctl_drt1_cfr2_word]
    // Tests [@ANCHOR: radioctl_drt1_tuning_word]
    // Tests [@ANCHOR: radioctl_drt1_word_frequency]
    #[test]
    fn the_words_follow_the_dds_arithmetic() {
        assert_eq!(cfr2_word(8, 150), 0x47);
        assert_eq!(cfr2_word(8, 75), 0x44);
        // 7 MHz: (7 000 000 + 45 000 000) / (45 012 000 * 8) * 2^32, truncated.
        assert_eq!(tuning_word(Hz(7_000_000)), 620_218_773);
        for hz in [50_000, 7_074_000, 14_200_123, 30_000_000] {
            assert_eq!(word_frequency(tuning_word(Hz(hz)), 0x47), Hz(hz));
        }
    }

    // Tests [@ANCHOR: radioctl_drt1_write_register]
    #[test]
    fn a_register_write_is_framed_by_dtr_and_clocked_by_break() {
        let steps = write_register(CFR2, 3, 0x47);
        assert_eq!(steps.len(), 2 + (8 + 24) * 3 + 1);
        assert_eq!(steps[1], step(ControlLine::Dtr, true));
        assert_eq!(steps.last(), Some(&step(ControlLine::Dtr, false)));
        // First instruction bit is 0 (write): RTS asserted, then one clock pulse.
        assert_eq!(steps[2], step(ControlLine::Rts, true));
        assert_eq!(steps[3], step(ControlLine::Break, true));
        assert_eq!(steps[4], step(ControlLine::Break, false));
    }

    // Tests [@ANCHOR: radioctl_drt1_dds_latch]
    // Tests [@ANCHOR: radioctl_drt1_tune_steps]
    // Tests [@ANCHOR: radioctl_drt1_twin_set_control]
    // Tests [@ANCHOR: radioctl_drt1_twin_registers]
    // Tests [@ANCHOR: radioctl_drt1_twin_frequency]
    #[test]
    fn the_twin_decodes_a_tune_from_the_lines_alone() {
        let desc = shipped();
        let twin = Drt1Twin::new(&desc).expect("twin");
        assert_eq!(twin.frequency(), desc.default_frequency);
        let steps = tune_steps(Hz(7_000_000));
        assert_eq!(steps.len(), 3 + 99 + 123);
        drive(&twin, &steps);
        assert_eq!(twin.registers(), (Some(0x47), Some(620_218_773)));
        assert_eq!(twin.frequency(), Hz(7_000_000));
        drive(&twin, &tune_steps(Hz(14_074_000)));
        assert_eq!(twin.frequency(), Hz(14_074_000));
    }

    // Tests [@ANCHOR: radioctl_drt1_twin_set_control]
    #[test]
    fn clock_pulses_outside_a_framed_write_are_ignored() {
        let twin = Drt1Twin::new(&shipped()).expect("twin");
        drive(
            &twin,
            &[
                step(ControlLine::Break, true),
                step(ControlLine::Break, false),
            ],
        );
        drive(&twin, &write_register(CFR2, 2, 0x1234));
        assert_eq!(
            twin.registers(),
            (None, None),
            "a short write is not latched"
        );
    }

    // Tests [@ANCHOR: radioctl_drt1_engine_new]
    #[test]
    fn engine_refuses_other_families() {
        assert!(Drt1Engine::new(RigDescription::ts2000()).is_err());
        assert!(Drt1Twin::new(&RigDescription::ts2000()).is_err());
    }

    // Tests [@ANCHOR: radioctl_drt1_engine_encode_lines]
    // Tests [@ANCHOR: radioctl_drt1_engine_encode]
    // Tests [@ANCHOR: radioctl_drt1_engine_feed]
    #[test]
    fn the_engine_has_only_a_set_frequency_line_sequence() {
        let mut e = Drt1Engine::new(shipped()).expect("engine");
        let steps = e
            .encode_lines(&Op::SetFrequency(Vfo::A, Hz(9_500_000)))
            .expect("encodes")
            .expect("a sequence");
        assert_eq!(steps, tune_steps(Hz(9_500_000)));
        assert!(e
            .encode_lines(&Op::SetFrequency(Vfo::A, Hz(10_000)))
            .is_err());
        assert!(e
            .encode_lines(&Op::SetFrequency(Vfo::A, Hz(31_000_000)))
            .is_err());
        assert!(e.encode_lines(&Op::GetFrequency(Vfo::A)).is_err());
        assert!(e.encode_lines(&Op::SetPtt(true)).is_err());
        assert!(e.encode(&Op::SetFrequency(Vfo::A, Hz(9_500_000))).is_err());
        assert!(e.feed(&[], Duration::ZERO).is_empty());
        assert_eq!(
            e.feed(b"x", Duration::ZERO),
            vec![Inbound::Unrecognised(vec![b'x'])]
        );
    }

    // Tests [@ANCHOR: radioctl_drt1_twin_new]
    // Tests [@ANCHOR: radioctl_drt1_twin_set_line]
    // Tests [@ANCHOR: radioctl_drt1_twin_snapshot]
    // Tests [@ANCHOR: radioctl_drt1_twin_feed]
    // Tests [@ANCHOR: radioctl_drt1_twin_front_panel]
    #[test]
    fn the_twin_has_one_knob_and_no_keying_line() {
        let mut t = Drt1Twin::new(&shipped()).expect("twin");
        assert!(t.set_line(KeyLine::Rts, true).is_err());
        assert!(t.feed(b"abc", Duration::ZERO).is_empty());
        t.front_panel(Panel::Frequency(Hz(5_000_000)))
            .expect("tunes");
        assert_eq!(t.snapshot().expect("snapshot").frequency, Hz(5_000_000));
        assert!(t.front_panel(Panel::Ptt(true)).is_err());
    }
}
