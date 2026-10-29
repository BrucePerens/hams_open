// SPDX-License-Identifier: AGPL-3.0-or-later
//! The unit's own firmware version and internal temperature sensor ("Instrument Monitoring" in the vendor's Help file: VNWA3 firmware
//! v5.16 or later). Both are plain vendor reads; sources are in `docs/PROTOCOL.md` (usbmon captures of the vendor program).

use crate::usb::Vnwa;

/// The firmware version read by request 0xff (`wValue` 3): two bytes, minor then major. The unit under test returns `16 05`.
pub fn parse_version(b: &[u8]) -> Result<(u8, u8), String> {
    // [@ANCHOR: vnwa_open_firmware_version]
    match b {
        [minor, major] => Ok((*major, *minor)),
        _ => Err(format!(
            "firmware version: expected 2 bytes, got {}",
            b.len()
        )),
    }
}

/// The internal sensor read by request 0x85 (`wValue` 3): two bytes, little endian, in uncalibrated counts that rise as the unit warms.
pub fn parse_temperature(b: &[u8]) -> Result<u16, String> {
    // [@ANCHOR: vnwa_open_temperature]
    match b {
        [lo, hi] => Ok(u16::from_le_bytes([*lo, *hi])),
        _ => Err(format!("temperature: expected 2 bytes, got {}", b.len())),
    }
}

/// The oldest firmware the Help file says has the sensor.
pub const SENSOR_MIN_VERSION: (u8, u8) = (5, 16);

impl Vnwa {
    /// The firmware version as (major, minor): `(5, 22)` for v5.22.
    pub fn firmware_version(&self) -> Result<(u8, u8), String> {
        parse_version(&self.read(0xff, 0x0003, 0x7dc0, 2)?)
    }

    /// The internal temperature sensor in raw counts (uncalibrated; only differences are meaningful). An error if the firmware is older
    /// than the sensor.
    pub fn temperature_counts(&self) -> Result<u16, String> {
        let v = self.firmware_version()?;
        if v < SENSOR_MIN_VERSION {
            return Err(format!(
                "firmware v{}.{} has no temperature sensor (v{}.{} or later)",
                v.0, v.1, SENSOR_MIN_VERSION.0, SENSOR_MIN_VERSION.1
            ));
        }
        parse_temperature(&self.read(0x85, 0x0003, 0x0000, 2)?)
    }
}

impl crate::sweep::Vna {
    /// The unit's firmware version as (major, minor).
    pub fn firmware_version(&self) -> Result<(u8, u8), String> {
        self.session.usb.firmware_version()
    }

    /// The unit's internal temperature in raw counts (see `Vnwa::temperature_counts`).
    pub fn temperature_counts(&self) -> Result<u16, String> {
        self.session.usb.temperature_counts()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::{Scripted, TransferError, Transport};
    use std::sync::Arc;

    // Tests [@ANCHOR: vnwa_open_firmware_version]
    #[test]
    fn the_captured_version_reply_is_v5_22() {
        assert_eq!(parse_version(&[0x16, 0x05]).unwrap(), (5, 22));
        assert!(parse_version(&[0x16]).is_err());
        assert!((5, 22) >= SENSOR_MIN_VERSION && (5, 15) < SENSOR_MIN_VERSION);
    }

    // Tests [@ANCHOR: vnwa_open_temperature]
    #[test]
    fn captured_temperature_replies_are_little_endian_counts_that_rise_while_warming() {
        // Replies of request 0x85 from the vendor captures, in time order (v03: first minutes after power-on, then 11 minutes later).
        let seen: [[u8; 2]; 4] = [[0x3e, 0x59], [0x3d, 0x59], [0x02, 0x5a], [0x47, 0x5a]];
        let v: Vec<u16> = seen.iter().map(|b| parse_temperature(b).unwrap()).collect();
        assert_eq!(v, vec![0x593e, 0x593d, 0x5a02, 0x5a47]);
        assert!(v[3] > v[0]);
        assert!(parse_temperature(&[1, 2, 3]).is_err());
    }

    struct Shared(Arc<Scripted>);
    impl Transport for Shared {
        fn write(&self, t: u8, r: u8, v: u16, i: u16, d: &[u8]) -> Result<usize, TransferError> {
            self.0.write(t, r, v, i, d)
        }
        fn read(&self, r: u8, v: u16, i: u16, b: &mut [u8]) -> Result<usize, TransferError> {
            self.0.read(r, v, i, b)
        }
    }

    #[test]
    fn an_old_firmware_gives_a_clear_error_and_a_new_one_reads_the_sensor() {
        let old = Vnwa::with_transport(Box::new(Shared(Arc::new(Scripted::new(vec![Ok(vec![
            0x0f, 0x05,
        ])])))));
        assert!(old
            .temperature_counts()
            .unwrap_err()
            .contains("no temperature sensor"));
        let new = Vnwa::with_transport(Box::new(Shared(Arc::new(Scripted::new(vec![
            Ok(vec![0x16, 0x05]),
            Ok(vec![0x47, 0x5a]),
        ])))));
        assert_eq!(new.temperature_counts().unwrap(), 0x5a47);
    }
}
