// SPDX-License-Identifier: AGPL-3.0-or-later
//! Hardware tests (dev box, unit on hub 3-2 port 2 with its TX output looped back to RX): `cargo test --test hil_sweep -- --ignored`.
//! The second test power cycles the unit.
use vnwa_open::power::HubPort;
use vnwa_open::sweep::{Config, Recoverable, Vna};

// Tests [@ANCHOR: vnwa_open_session_open]
// Tests [@ANCHOR: vnwa_open_trace_point]
// Tests [@ANCHOR: vnwa_open_meter_once]
#[test]
#[ignore = "hil: needs the VNWA with TX looped back to RX"]
fn a_session_opens_a_point_is_traced_and_a_loopback_tone_is_metered() {
    let mut vna = Vna::open(Config::default()).unwrap();
    let raw = vna.trace_point(10e6, 20.0, 420.0).unwrap();
    assert!(!raw.codec1.is_empty() && !raw.codec2.is_empty());
    assert!(raw.send_ms > 0.0);
    // On a TX-to-RX loopback the offset from the unit's own TX frequency is zero (the meter accepts up to 25 Hz).
    let r = vna.meter(10e6, 60.0).unwrap();
    assert!(r.offset_hz.abs() < 25.0, "{r:?}");
}

// Tests [@ANCHOR: vnwa_open_vna_cold_reset]
#[test]
#[ignore = "hil: needs the VNWA on hub 3-2 port 2; power cycles it"]
fn a_cold_reset_reopens_the_session_and_the_unit_measures_again() {
    let mut vna = Vna::open(Config {
        hub: Some(HubPort {
            hub: "3-2".into(),
            port: 2,
        }),
        ..Default::default()
    })
    .unwrap();
    vna.cold_reset().unwrap();
    vna.measure(10e6, 10e6).unwrap();
}
