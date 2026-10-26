// SPDX-License-Identifier: AGPL-3.0-or-later
//! The VNWA's control requests, all vendor control transfers on endpoint 0 (no bulk or interrupt
//! traffic; the measured signal arrives as audio from the two USB codecs).
//!
//! Everything here was read off the wire while the owner ran the vendor program and set known
//! frequencies; see `docs/PROTOCOL.md` for what is confirmed and what is not.

/// Core clock of the DDS chips in the stock (non-overclocked) setting: about 36 MHz crystal times
/// ten. Confirmed by the tuning words seen for 1, 2, 10 and 20 MHz.
pub const DDS_CLOCK_HZ: f64 = 360_000_000.0;

/// A vendor control transfer, host to device (`bmRequestType` 0x40) or device to host (0xC0).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Request {
    pub request_type: u8,
    pub request: u8,
    pub value: u16,
    pub index: u16,
    pub data: Vec<u8>,
}

/// `bmRequestType` of a vendor request to the device.
pub const VENDOR_OUT: u8 = 0x40;

/// The DDS tuning word for a frequency: `round(f * 2^32 / clock)`.
pub fn tuning_word(freq_hz: f64) -> u32 {
    // [@ANCHOR: vnwa_open_tuning_word]
    (freq_hz * 4_294_967_296.0 / DDS_CLOCK_HZ).round() as u32
}

/// The sweep-frequency word carried in bytes 4 to 8 of request 0x49's payload (a 40-bit little-endian
/// number): `round(f / 9 * 2^40 / clock)`. The divisor 9 is what the wire shows (10 MHz gave 10/9 MHz worth,
/// 25 MHz gave 25/9 MHz worth); why the program divides by nine is not known.
pub fn sweep_word(freq_hz: f64) -> u64 {
    // [@ANCHOR: vnwa_open_sweep_word]
    (freq_hz / 9.0 * 1_099_511_627_776.0 / DDS_CLOCK_HZ).round() as u64
}

/// The signed 48-bit offset carried in the tail of request 0x49's payload (little endian):
/// `round((previous - new) / divisor * 2^48 / clock)`, where `previous` is the frequency before the sweep
/// started and `divisor` is 6 when the payload's first byte is 3 and 9 when it is 4 (that is, 3 x (byte - 1);
/// not explained). Returns the six bytes.
pub fn sweep_offset_tail(previous_hz: f64, new_hz: f64, counter: u8) -> [u8; 6] {
    // [@ANCHOR: vnwa_open_sweep_offset]
    let divisor = 3.0 * f64::from(counter.saturating_sub(1).max(1));
    let word =
        ((previous_hz - new_hz) / divisor * 281_474_976_710_656.0 / DDS_CLOCK_HZ).round() as i64;
    let b = word.to_le_bytes();
    [b[0], b[1], b[2], b[3], b[4], b[5]]
}

/// Set the generator frequency (request 0x4c): eight bytes, four zero bytes and then the tuning
/// word, little endian. `value` is 0x090a in the form the vendor program sends while it is
/// stepping the generator and 0x0000 or 0x00c8 in the first set-up; its meaning is not decoded.
pub fn generator_frequency(freq_hz: f64, value: u16) -> Request {
    // [@ANCHOR: vnwa_open_generator_frequency]
    let mut data = vec![0u8; 4];
    data.extend_from_slice(&tuning_word(freq_hz).to_le_bytes());
    Request {
        request_type: VENDOR_OUT,
        request: 0x4c,
        value,
        index: 0,
        data,
    }
}

/// The RF level lives in the `wValue` of request 0x58 (a device-to-host read with no data stage):
/// 14 bits, 0x3fff the maximum.
pub fn rf_level_value(level: u16) -> Result<u16, String> {
    // [@ANCHOR: vnwa_open_rf_level]
    if level <= 0x3fff {
        Ok(level)
    } else {
        Err(format!(
            "RF level {level:#x} is above the 14-bit maximum 0x3fff"
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Tests [@ANCHOR: vnwa_open_tuning_word]
    #[test]
    fn tuning_words_match_what_the_vendor_program_sent() {
        assert_eq!(tuning_word(1.0e6), 0x00b6_0b61);
        assert_eq!(tuning_word(2.0e6), 0x016c_16c1);
        assert_eq!(tuning_word(10.0e6), 0x071c_71c7);
        assert_eq!(tuning_word(20.0e6), 0x0e38_e38e);
    }

    // Tests [@ANCHOR: vnwa_open_sweep_word]
    #[test]
    fn sweep_words_match_the_two_single_point_sweeps() {
        assert_eq!(sweep_word(10.0e6), 0x0000_ca45_87e7);
        assert_eq!(sweep_word(25.0e6), 0x0001_f9ad_d3c1);
        // The two other words seen in the 25 MHz sweep: 1 MHz and 2 MHz.
        assert_eq!(sweep_word(1.0e6), 0x143a_2731);
        assert_eq!(sweep_word(2.0e6), 0x2874_4e61);
    }

    // Tests [@ANCHOR: vnwa_open_sweep_offset]
    #[test]
    fn offset_tails_match_the_captured_sweeps() {
        let hex = |t: [u8; 6]| t.iter().map(|b| format!("{b:02x}")).collect::<String>();
        // 1 -> 4 MHz, 1 -> 5 MHz, 5 -> 50 MHz at counter 3.
        assert_eq!(hex(sweep_offset_tail(1.0e6, 4.0e6, 3)), "faa44ffaa4ff");
        assert_eq!(hex(sweep_offset_tail(1.0e6, 5.0e6, 3)), "f8db14a386ff");
        assert_eq!(hex(sweep_offset_tail(5.0e6, 50.0e6, 3)), "abaaaaaaaafa");
        // 25 -> 10 MHz at counter 3 is +2.5 MHz; at counter 4 it is +5/3 MHz.
        assert_eq!(hex(sweep_offset_tail(25.0e6, 10.0e6, 4)), "13da4b682f01");
        // No change means a zero tail.
        assert_eq!(sweep_offset_tail(25.0e6, 25.0e6, 3), [0; 6]);
    }

    // Tests [@ANCHOR: vnwa_open_generator_frequency]
    #[test]
    fn generator_payload_is_four_zero_bytes_then_the_little_endian_word() {
        let r = generator_frequency(10.0e6, 0x00c8);
        assert_eq!(r.request, 0x4c);
        assert_eq!(r.request_type, VENDOR_OUT);
        assert_eq!(r.data, [0, 0, 0, 0, 0xc7, 0x71, 0x1c, 0x07]);
        assert_eq!(
            generator_frequency(20.0e6, 0x090a).data,
            [0, 0, 0, 0, 0x8e, 0xe3, 0x38, 0x0e]
        );
        assert_eq!(
            generator_frequency(1.0e6, 0x090a).data,
            [0, 0, 0, 0, 0x61, 0x0b, 0xb6, 0x00]
        );
    }

    // Tests [@ANCHOR: vnwa_open_rf_level]
    #[test]
    fn rf_level_is_fourteen_bits() {
        assert_eq!(rf_level_value(0x3fff), Ok(0x3fff));
        assert!(rf_level_value(0x4000).is_err());
    }
}
