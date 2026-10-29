// SPDX-License-Identifier: AGPL-3.0-or-later
//! Hardware test (dev box, unit on the bus): `cargo test --test hil_monitor -- --ignored`.
use vnwa_open::usb::Vnwa;

// Tests [@ANCHOR: vnwa_open_temperature]
// Tests [@ANCHOR: vnwa_open_temperature_counts]
// Tests [@ANCHOR: vnwa_open_rusb_transport_open]
#[test]
#[ignore = "hil: needs the VNWA"]
fn the_unit_reports_its_firmware_and_a_steady_temperature_reading() {
    let u = Vnwa::open().unwrap();
    let fw = u.firmware_version().unwrap();
    assert!(fw >= vnwa_open::monitor::SENSOR_MIN_VERSION, "{fw:?}");
    let (a, b) = (
        u.temperature_counts().unwrap(),
        u.temperature_counts().unwrap(),
    );
    // Counts near 23,000 on the unit under test; two reads a moment apart agree to a few counts.
    assert!((15_000..30_000).contains(&a), "{a}");
    assert!(a.abs_diff(b) < 50, "{a} {b}");
}
