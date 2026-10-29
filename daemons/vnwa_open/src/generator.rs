// SPDX-License-Identifier: AGPL-3.0-or-later
//! The CW signal generator on the TX port, as the vendor program drives it ("Signal Generator" mode, remote commands `sg on`,
//! `sg_freq`, `sg_atten`, `sg off`). The sequences below are the control transfers seen in a usbmon capture of that program
//! (2026-10-26, `docs/PROTOCOL.md`, "Signal generator"). The generator needs neither the codecs nor the reference channels.

use crate::protocol::generator_frequency;
use crate::usb::Vnwa;

/// Largest 14-bit amplitude; the generator's attenuation is relative to it.
const FULL_SCALE: f64 = 16383.0;
/// The vendor program allows 0 dB down to -50 dB.
pub const MIN_ATTENUATION_DB: f64 = -50.0;

/// One device-to-host request of a sequence. The unit answers with no data of interest; the transfer itself is the command.
struct Read {
    request: u8,
    value: u16,
    len: usize,
}

/// The amplitude the unit takes for an attenuation in dB (0 to -50): `round(16383 x 10^(dB/20))`.
/// The vendor program sent 52, 163 and 518 for -50, -40 and -30 dB (within one count; its rounding is not known).
pub fn level_for_attenuation(db: f64) -> Result<u16, String> {
    // [@ANCHOR: vnwa_open_generator_level]
    if !(MIN_ATTENUATION_DB..=0.0).contains(&db) {
        return Err(format!(
            "generator attenuation {db} dB is outside {MIN_ATTENUATION_DB} to 0 dB"
        ));
    }
    Ok((FULL_SCALE * 10f64.powf(db / 20.0)).round() as u16)
}

/// The reads that switch the generator mode on, before the first frequency (vendor order).
const ENTER: [Read; 4] = [
    Read {
        request: 0x62,
        value: 0x0002,
        len: 64,
    },
    Read {
        request: 0x61,
        value: 0x0000,
        len: 64,
    },
    Read {
        request: 0x98,
        value: 0x0815,
        len: 64,
    },
    Read {
        request: 0x9a,
        value: 0x00c8,
        len: 64,
    },
];

/// The reads that switch it off: the same three the program sends when the generator is switched off or its attenuation is
/// set while it is off.
const LEAVE: [Read; 3] = [
    Read {
        request: 0x91,
        value: 0x0000,
        len: 64,
    },
    Read {
        request: 0x58,
        value: 0x0000,
        len: 64,
    },
    Read {
        request: 0x66,
        value: 0x0000,
        len: 64,
    },
];

fn run(usb: &Vnwa, reads: &[Read]) -> Result<(), String> {
    for r in reads {
        usb.read(r.request, r.value, 0, r.len)?;
    }
    Ok(())
}

/// Sets the generator frequency (the unit must be in generator mode): read 0x0e, write 0x4c, read 0x04.
fn set_frequency(usb: &Vnwa, freq_hz: f64, first: bool) -> Result<(), String> {
    usb.read(0x0e, 0, 0, 64)?;
    usb.send(&generator_frequency(
        freq_hz,
        if first { 0x00c8 } else { 0 },
    ))?;
    usb.read(0x04, 0x090a, 0, 8)?;
    Ok(())
}

/// Sets the output amplitude (request 0x58, the amplitude in `wValue`).
fn set_level(usb: &Vnwa, attenuation_db: f64) -> Result<(), String> {
    usb.read(0x58, level_for_attenuation(attenuation_db)?, 0, 64)?;
    Ok(())
}

/// Switches the generator on at a frequency and an attenuation (0 to -50 dB below full scale) and leaves it running.
pub fn on(usb: &Vnwa, freq_hz: f64, attenuation_db: f64) -> Result<(), String> {
    level_for_attenuation(attenuation_db)?;
    run(usb, &ENTER)?;
    set_frequency(usb, freq_hz, true)?;
    set_level(usb, attenuation_db)
}

/// Changes the frequency of a running generator.
pub fn retune(usb: &Vnwa, freq_hz: f64) -> Result<(), String> {
    set_frequency(usb, freq_hz, false)
}

/// Changes the attenuation of a running generator.
pub fn set_attenuation(usb: &Vnwa, attenuation_db: f64) -> Result<(), String> {
    set_level(usb, attenuation_db)
}

/// The modulation clock the unit's firmware runs its software DDS at: the modulating sine advances `65536 * f / clock` per step. Measured from
/// the 0x92 field of vendor captures at 500, 1000 and 2000 Hz (AM: 1748, 3495, 6991) and 500 and 1000 Hz (FM: 2097, 4194): the AM clock is
/// 18.75 kHz (12 MHz / 640) and the FM clock 15.625 kHz (12 MHz / 768). The Help file says "about 16 kHz".
pub const AM_CLOCK_HZ: f64 = 18_750.0;
pub const FM_CLOCK_HZ: f64 = 15_625.0;
/// The modulation frequency range: the Help file gives up to about 3 kHz (Nyquist about 8 kHz); the vendor program's captured values are 500 to 3000 Hz.
pub const MIN_MOD_HZ: f64 = 1.0;
pub const MAX_MOD_HZ: f64 = 3_000.0;
/// The DDS clock of the generator chip and the number of frequency steps on each side of the carrier in FM (the 0x95 field is the tuning-word
/// change of the deviation divided by 127: 470, 939 and 1879 for 5, 10 and 20 kHz).
const DDS_CLOCK_HZ: f64 = 360e6;
const FM_STEPS: f64 = 127.0;
/// The largest FM deviation accepted (kHz in the Help file's example is 300; the field is 16 bits).
pub const MAX_FM_DEVIATION_HZ: f64 = 300_000.0;

/// The fields of an AM start that depend on the settings (all others are constants of the sequence).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AmFields {
    /// 0x92 wValue: the modulation phase step.
    pub phase_step: u16,
    /// 0x94: the modulation amplitude as a 32-bit number, wIndex the high half and wValue the low half.
    pub amplitude_high: u16,
    pub amplitude_low: u16,
}

/// The AM fields for an output amplitude word `level` (the 0x58 value), a modulation frequency and a depth in percent (1 to 100).
/// The amplitude is the peak of the modulating wave in the DDS's amplitude units, which for a carrier of amplitude A (1 + m sin) peaking at
/// `level` is `level * m / (1 + m)`, scaled by `2^23 / 16383`: this reproduces the 0x94 fields of seven captures (depth 10, 25, 50, 75, 80, 100 % at
/// -20 dB and 50 % at -30 dB) to the last count or one count.
pub fn am_fields(level: u16, mod_hz: f64, depth_pct: f64) -> Result<AmFields, String> {
    // [@ANCHOR: vnwa_open_am_fields]
    if !(MIN_MOD_HZ..=MAX_MOD_HZ).contains(&mod_hz) {
        return Err(format!(
            "modulation frequency {mod_hz} Hz is outside {MIN_MOD_HZ} to {MAX_MOD_HZ} Hz"
        ));
    }
    if !(1.0..=100.0).contains(&depth_pct) {
        return Err(format!("AM depth {depth_pct} % is outside 1 to 100 %"));
    }
    let m = depth_pct / 100.0;
    let amp = (f64::from(level) / FULL_SCALE * m / (1.0 + m) * 8_388_608.0).round() as u32;
    Ok(AmFields {
        phase_step: (mod_hz * 65536.0 / AM_CLOCK_HZ).round() as u16,
        amplitude_high: (amp >> 16) as u16,
        amplitude_low: (amp & 0xffff) as u16,
    })
}

/// The FM fields: 0x92 wValue (phase step) and 0x95 wValue (the deviation per step).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FmFields {
    pub phase_step: u16,
    pub step_word: u16,
}

pub fn fm_fields(mod_hz: f64, deviation_hz: f64) -> Result<FmFields, String> {
    // [@ANCHOR: vnwa_open_fm_fields]
    if !(MIN_MOD_HZ..=MAX_MOD_HZ).contains(&mod_hz) {
        return Err(format!(
            "modulation frequency {mod_hz} Hz is outside {MIN_MOD_HZ} to {MAX_MOD_HZ} Hz"
        ));
    }
    if !(100.0..=MAX_FM_DEVIATION_HZ).contains(&deviation_hz) {
        return Err(format!(
            "FM deviation {deviation_hz} Hz is outside 100 Hz to {MAX_FM_DEVIATION_HZ} Hz"
        ));
    }
    Ok(FmFields {
        phase_step: (mod_hz * 65536.0 / FM_CLOCK_HZ).round() as u16,
        step_word: (deviation_hz * 4_294_967_296.0 / DDS_CLOCK_HZ / FM_STEPS).round() as u16,
    })
}

/// Which modulation.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Modulation {
    Am { mod_hz: f64, depth_pct: f64 },
    Fm { mod_hz: f64, deviation_hz: f64 },
}

/// The requests of a modulation start, in the vendor's order, as (request, wValue, wIndex) reads, with the 0x4c frequency writes marked by `None`
/// (first with `wValue` 0, second with 0x0080). The last is the start request 0x90 (0x0a for AM, 0x0c for FM), after which the unit modulates.
pub fn modulation_reads(level: u16, m: Modulation) -> Result<Vec<(u8, u16, u16)>, String> {
    // [@ANCHOR: vnwa_open_modulation_sequence]
    let (v92, v93, v94, v95, start): (u16, u16, (u16, u16), u16, u16) = match m {
        Modulation::Am { mod_hz, depth_pct } => {
            let f = am_fields(level, mod_hz, depth_pct)?;
            (
                f.phase_step,
                level,
                (f.amplitude_low, f.amplitude_high),
                0,
                0x000a,
            )
        }
        Modulation::Fm {
            mod_hz,
            deviation_hz,
        } => {
            let f = fm_fields(mod_hz, deviation_hz)?;
            (f.phase_step, level, (0, 0), f.step_word, 0x000c)
        }
    };
    Ok(vec![
        (0x91, 0, 0),
        (0x0e, 0, 0),
        (0x4c, 0, 0), // write: carrier word, wValue 0
        (0x04, 0x090a, 0),
        (0x92, v92, 0),
        (0x93, v93, 0),
        (0x58, level, 0),
        (0x94, v94.0, v94.1),
        (0x94, v94.0, v94.1),
        (0x95, v95, 0),
        (0x9f, 0, 0),
        (0x9c, 0x0100, 0),
        (0x9d, 0x0080, 0),
        (0x0e, 0, 0),
        (0x4c, 0x0080, 0), // write: carrier word, wValue 0x0080
        (0x04, 0x090a, 0),
        (0x92, v92, 0),
        (0x94, v94.0, v94.1),
        (0x95, v95, 0),
        (0x58, level, 0),
        (0x66, 0, 0),
        (0x9e, 0, 0),
        (0x90, start, 0),
    ])
}

/// Starts AM or FM on a running generator (`on` first, at the carrier frequency and attenuation given here again). After the last request the
/// unit's microcontroller modulates in real time and stalls control requests, including the ones that switch the generator off, so **the only way
/// to end the modulation is a power cycle** (`power::cold_reset`); the last request's own status stage is often stalled, which is not an error.
pub fn modulate(
    usb: &Vnwa,
    carrier_hz: f64,
    attenuation_db: f64,
    m: Modulation,
) -> Result<(), String> {
    let level = level_for_attenuation(attenuation_db)?;
    let seq = modulation_reads(level, m)?;
    let last = seq.len() - 1;
    for (i, (req, v, idx)) in seq.into_iter().enumerate() {
        if req == 0x4c {
            usb.send(&generator_frequency(carrier_hz, v))?;
        } else if i == last {
            let _ = usb.read(req, v, idx, 64);
        } else {
            usb.read(req, v, idx, if req == 0x04 { 8 } else { 64 })?;
        }
    }
    Ok(())
}

/// Whether the unit still answers the request that switches the generator off (0x91). During AM or FM it does not (a stall), and then the
/// unit has to be power cycled to leave the modulation.
pub fn answers(usb: &Vnwa) -> bool {
    usb.read(0x91, 0, 0, 64).is_ok()
}

/// Ends AM or FM without a power cycle: a USB reset of the control chip (which the vendor program does not try), then the usual switch-off. Measured
/// 2026-10-29: after the reset the unit answered every request again (0x91 gave its usual 0x73) and the switch-off worked. The reset sometimes makes the
/// unit re-enumerate (kernel: "device not responding to setup address", then a new device number a second later), after which the old handle is gone,
/// so the unit is opened again. Takes the open unit and a way to open it again. An error says a power cycle is needed if the unit still does not answer.
pub fn end_modulation_with(
    usb: Vnwa,
    mut reopen: impl FnMut() -> Result<Vnwa, String>,
    tries: u32,
    pause: std::time::Duration,
) -> Result<(), String> {
    // [@ANCHOR: vnwa_open_end_modulation]
    let reset = usb.reset_device();
    drop(usb);
    for _ in 0..tries {
        std::thread::sleep(pause);
        if let Ok(u) = reopen() {
            if answers(&u) {
                return off(&u);
            }
        }
    }
    Err(format!("the unit still refuses requests {:.0} s after a USB reset (reset: {reset:?}): it needs a power cycle", f64::from(tries) * pause.as_secs_f64()))
}

/// `end_modulation_with` on the unit on the bus.
pub fn end_modulation() -> Result<(), String> {
    end_modulation_with(
        Vnwa::open()?,
        Vnwa::open,
        24,
        std::time::Duration::from_millis(500),
    )
}

/// The captured AM at the vendor's defaults (500 Hz, 100 %, -20 dB) with the carrier frequency of the caller; kept for the tests that compare the
/// decoded sequence with the capture.
pub fn am_captured_500hz_100pct(usb: &Vnwa, freq_hz: f64) -> Result<(), String> {
    // [@ANCHOR: vnwa_open_generator_am_captured]
    modulate(
        usb,
        freq_hz,
        -20.0,
        Modulation::Am {
            mod_hz: 500.0,
            depth_pct: 100.0,
        },
    )
}

/// Switches the generator off. Safe to call at any time and when it is already off.
pub fn off(usb: &Vnwa) -> Result<(), String> {
    run(usb, &LEAVE)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Tests [@ANCHOR: vnwa_open_end_modulation]
    #[test]
    fn a_usb_reset_ends_the_modulation_stall_and_a_unit_that_stays_stalled_is_reported() {
        use crate::transport::{TransferError, Transport};
        use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
        use std::sync::Arc;
        use std::time::Duration;
        /// A unit that stalls every request until it has been reset (then it answers), like the unit during AM or FM.
        struct Modulating {
            stalled: AtomicBool,
            resets: Arc<AtomicU32>,
        }
        impl Transport for Modulating {
            fn reset(&self) -> Result<(), TransferError> {
                self.resets.fetch_add(1, Ordering::Relaxed);
                self.stalled.store(false, Ordering::Relaxed);
                Ok(())
            }
            fn write(
                &self,
                _t: u8,
                _r: u8,
                _v: u16,
                _i: u16,
                d: &[u8],
            ) -> Result<usize, TransferError> {
                Ok(d.len())
            }
            fn read(&self, _r: u8, _v: u16, _i: u16, b: &mut [u8]) -> Result<usize, TransferError> {
                if self.stalled.load(Ordering::Relaxed) {
                    Err(TransferError::Transient("Pipe error".into()))
                } else {
                    Ok(b.len().min(1))
                }
            }
        }
        let resets = Arc::new(AtomicU32::new(0));
        let unit = |stalled: bool| {
            Vnwa::with_transport(Box::new(Modulating {
                stalled: AtomicBool::new(stalled),
                resets: resets.clone(),
            }))
        };
        let pause = Duration::from_millis(1);
        let u = unit(true);
        assert!(!answers(&u), "a modulating unit does not answer");
        // The reset makes the unit re-enumerate: the old handle is gone and the reopen gives an answering one, after two failed opens.
        let mut opens = 0;
        let r = end_modulation_with(
            u,
            || {
                opens += 1;
                if opens < 3 {
                    Err("no VNWA found".into())
                } else {
                    Ok(unit(false))
                }
            },
            10,
            pause,
        );
        assert_eq!(r, Ok(()));
        assert_eq!((resets.load(Ordering::Relaxed), opens), (1, 3));
        // A unit that stays stalled (the reopen gives a stalled one again) is reported with the advice.
        let e = end_modulation_with(unit(true), || Ok(unit(true)), 4, pause).unwrap_err();
        assert!(e.contains("needs a power cycle"), "{e}");
    }

    // Tests [@ANCHOR: vnwa_open_am_fields]
    #[test]
    fn the_am_fields_reproduce_every_captured_start() {
        // (level, modulation Hz, depth %, 0x92, 0x94 wValue, 0x94 wIndex), from usbmon captures of the vendor program on 2026-10-29.
        for (level, hz, depth, v92, lo, hi) in [
            (1638u16, 500.0, 100.0, 0x06d4u16, 0x661au16, 6u16),
            (1638, 1000.0, 50.0, 0x0da7, 0x4411, 4),
            (1638, 2000.0, 80.0, 0x1b4f, 0xb017, 5),
            (1638, 1000.0, 10.0, 0x0da7, 0x29d6, 1),
            (1638, 1000.0, 25.0, 0x0da7, 0x8f3d, 2),
            (1638, 1000.0, 75.0, 0x0da7, 0x7c16, 5),
            (518, 1000.0, 50.0, 0x0da7, 0x595b, 1),
        ] {
            let f = am_fields(level, hz, depth).unwrap();
            assert_eq!(f.phase_step, v92, "{level} {hz} {depth}");
            let (got, want) = (
                (u32::from(f.amplitude_high) << 16) | u32::from(f.amplitude_low),
                (u32::from(hi) << 16) | u32::from(lo),
            );
            assert!(
                got.abs_diff(want) <= 1,
                "{level} {hz} {depth}: {got} vs {want}"
            );
        }
        assert!(am_fields(1638, 3500.0, 50.0).is_err() && am_fields(1638, 0.5, 50.0).is_err());
        assert!(am_fields(1638, 1000.0, 0.0).is_err() && am_fields(1638, 1000.0, 120.0).is_err());
    }

    // Tests [@ANCHOR: vnwa_open_fm_fields]
    #[test]
    fn the_fm_fields_reproduce_every_captured_start() {
        // (modulation Hz, deviation Hz, 0x92, 0x95)
        for (hz, dev, v92, v95) in [
            (1000.0, 10_000.0, 0x1062u16, 0x03abu16),
            (1000.0, 5_000.0, 0x1062, 0x01d6),
            (1000.0, 20_000.0, 0x1062, 0x0757),
            (500.0, 10_000.0, 0x0831, 0x03ab),
        ] {
            let f = fm_fields(hz, dev).unwrap();
            assert_eq!((f.phase_step, f.step_word), (v92, v95), "{hz} {dev}");
        }
        assert!(fm_fields(1000.0, 50.0).is_err() && fm_fields(1000.0, 400_000.0).is_err());
    }

    // Tests [@ANCHOR: vnwa_open_modulation_sequence]
    #[test]
    fn the_start_sequence_at_the_vendor_defaults_is_the_captured_one() {
        // The 23 requests of the capture of `sg_mod am` at 500 Hz, 100 %, -20 dB (usbmon 2026-10-29, wValue then wIndex).
        let captured: [(u8, u16, u16); 23] = [
            (0x91, 0, 0),
            (0x0e, 0, 0),
            (0x4c, 0, 0),
            (0x04, 0x090a, 0),
            (0x92, 0x06d4, 0),
            (0x93, 0x0666, 0),
            (0x58, 0x0666, 0),
            (0x94, 0x661a, 6),
            (0x94, 0x661a, 6),
            (0x95, 0, 0),
            (0x9f, 0, 0),
            (0x9c, 0x0100, 0),
            (0x9d, 0x0080, 0),
            (0x0e, 0, 0),
            (0x4c, 0x0080, 0),
            (0x04, 0x090a, 0),
            (0x92, 0x06d4, 0),
            (0x94, 0x661a, 6),
            (0x95, 0, 0),
            (0x58, 0x0666, 0),
            (0x66, 0, 0),
            (0x9e, 0, 0),
            (0x90, 0x000a, 0),
        ];
        // 0x94's 32-bit amplitude is 419,354 in the capture; ours may differ by one count in the low half.
        let ours = modulation_reads(
            1638,
            Modulation::Am {
                mod_hz: 500.0,
                depth_pct: 100.0,
            },
        )
        .unwrap();
        assert_eq!(ours.len(), captured.len());
        for (a, b) in ours.iter().zip(captured.iter()) {
            assert_eq!((a.0, a.2), (b.0, b.2));
            assert!(a.1.abs_diff(b.1) <= 1, "{a:?} vs {b:?}");
        }
        // FM ends in 0x0c and carries no AM amplitude.
        let fm = modulation_reads(
            1638,
            Modulation::Fm {
                mod_hz: 1000.0,
                deviation_hz: 10_000.0,
            },
        )
        .unwrap();
        assert_eq!(fm.last(), Some(&(0x90, 0x000c, 0)));
        assert!(fm
            .iter()
            .filter(|r| r.0 == 0x94)
            .all(|r| r.1 == 0 && r.2 == 0));
    }

    // Tests [@ANCHOR: vnwa_open_generator_level]
    #[test]
    fn attenuation_gives_the_amplitudes_the_vendor_program_sent() {
        // The vendor program's rounding differs by one count at -40 dB (it sent 163, the exact value is 163.8).
        let near = |db: f64, sent: u16| level_for_attenuation(db).unwrap().abs_diff(sent) <= 1;
        assert!(near(-50.0, 0x0034));
        assert!(near(-40.0, 0x00a3));
        assert!(near(-30.0, 0x0206));
        assert_eq!(level_for_attenuation(0.0), Ok(0x3fff));
        assert!(level_for_attenuation(-50.5).is_err());
        assert!(level_for_attenuation(0.5).is_err());
    }

    struct Shared(std::sync::Arc<crate::transport::Scripted>);
    impl crate::transport::Transport for Shared {
        fn reset(&self) -> Result<(), crate::transport::TransferError> {
            self.0.reset()
        }
        fn write(
            &self,
            t: u8,
            r: u8,
            v: u16,
            i: u16,
            d: &[u8],
        ) -> Result<usize, crate::transport::TransferError> {
            self.0.write(t, r, v, i, d)
        }
        fn read(
            &self,
            r: u8,
            v: u16,
            i: u16,
            b: &mut [u8],
        ) -> Result<usize, crate::transport::TransferError> {
            self.0.read(r, v, i, b)
        }
    }

    fn unit(
        outcomes: Vec<Result<Vec<u8>, crate::transport::TransferError>>,
    ) -> (Vnwa, std::sync::Arc<crate::transport::Scripted>) {
        let s = std::sync::Arc::new(crate::transport::Scripted::new(outcomes));
        (Vnwa::with_transport(Box::new(Shared(s.clone()))), s)
    }

    // Tests [@ANCHOR: vnwa_open_generator_am_captured]
    #[test]
    fn the_captured_am_sequence_is_sent_in_order_and_a_stalled_last_request_is_not_an_error() {
        use crate::transport::{Seen, TransferError};
        let stall = || Err(TransferError::Transient("Pipe error".into()));
        // The script is used up by the 22 requests before the last one; the last one (0x90) and its three resends all stall.
        let mut outcomes: Vec<_> = (0..22).map(|_| Ok(vec![0u8; 8])).collect();
        outcomes.extend((0..4).map(|_| stall()));
        let (u, s) = unit(outcomes);
        assert_eq!(am_captured_500hz_100pct(&u, 10e6), Ok(()));
        let seen = s.seen();
        let read = |r: u8| Seen {
            write: false,
            request: r,
        };
        assert_eq!(seen[0], read(0x91));
        assert_eq!(seen[1], read(0x0e));
        assert_eq!(
            seen.iter().filter(|t| t.write).count(),
            2,
            "two frequency writes"
        );
        assert_eq!(
            seen.iter()
                .filter(|t| !t.write && t.request == 0x9e)
                .count(),
            1
        );
        assert_eq!(seen.last(), Some(&read(0x90)));
        assert_eq!(seen.iter().filter(|t| t.request == 0x90).count(), 4);
    }

    #[test]
    fn a_failure_before_the_last_request_is_an_error() {
        use crate::transport::TransferError;
        let (u, s) = unit(vec![Err(TransferError::NoDevice("No such device".into()))]);
        let e = am_captured_500hz_100pct(&u, 10e6).unwrap_err();
        assert!(e.contains("No such device"), "{e}");
        assert_eq!(s.seen().len(), 1);
    }
}
