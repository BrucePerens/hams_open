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

/// Amplitude modulation as the vendor program switches it on at its defaults, replayed from the usbmon capture of 2026-10-28: 500 Hz, 100 %,
/// at -20 dB (amplitude 1638). Only that setting is reproduced: the fields that set the modulation frequency and depth (requests 0x94, 0x9c,
/// 0x9d) and the amplitude-related ones (0x92, 0x93) have not been decoded, and a scaled copy of the sequence at another level gave a
/// different, weak modulation. The generator must already be on at `freq_hz` and -20 dB. The unit's microcontroller then modulates in
/// real time and stalls most control requests (0x91 among them), so the only way to stop it is a power cycle. The last request usually
/// stalls too, which is not an error.
pub fn am_captured_500hz_100pct(usb: &Vnwa, freq_hz: f64) -> Result<(), String> {
    // [@ANCHOR: vnwa_open_generator_am_captured]
    let level: u16 = 1638;
    usb.read(0x91, 0, 0, 64)?;
    usb.read(0x0e, 0, 0, 64)?;
    usb.send(&generator_frequency(freq_hz, 0))?;
    usb.read(0x04, 0x090a, 0, 8)?;
    usb.read(0x92, 0x06d4, 0, 64)?;
    usb.read(0x93, level, 0, 64)?;
    usb.read(0x58, level, 0, 64)?;
    usb.read(0x94, 0x661a, 0x0006, 64)?;
    usb.read(0x94, 0x661a, 0x0006, 64)?;
    usb.read(0x95, 0, 0, 64)?;
    usb.read(0x9f, 0, 0, 64)?;
    usb.read(0x9c, 0x0100, 0, 64)?;
    usb.read(0x9d, 0x0080, 0, 64)?;
    usb.read(0x0e, 0, 0, 64)?;
    usb.send(&generator_frequency(freq_hz, 0x0080))?;
    usb.read(0x04, 0x090a, 0, 8)?;
    usb.read(0x92, 0x06d4, 0, 64)?;
    usb.read(0x94, 0x661a, 0x0006, 64)?;
    usb.read(0x95, 0, 0, 64)?;
    usb.read(0x58, level, 0, 64)?;
    usb.read(0x66, 0, 0, 64)?;
    usb.read(0x9e, 0, 0, 64)?;
    // Starts the modulation; its status stage is often stalled by the busy unit.
    let _ = usb.read(0x90, 0x000a, 0, 64);
    Ok(())
}

/// Switches the generator off. Safe to call at any time and when it is already off.
pub fn off(usb: &Vnwa) -> Result<(), String> {
    run(usb, &LEAVE)
}

#[cfg(test)]
mod tests {
    use super::*;

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
