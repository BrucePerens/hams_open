// SPDX-License-Identifier: AGPL-3.0-or-later
//! Hardware tests (dev box with the unit on hub 3-2 port 2): `cargo test --test hil_overrun -- --ignored`.
use std::time::Duration;

use vnwa_open::capture::Capture;
use vnwa_open::power::{cold_reset, HubPort};

// Tests [@ANCHOR: vnwa_open_capture_close]
#[test]
#[ignore = "hil: needs the VNWA on hub 3-2 port 2; power cycles it"]
fn closing_the_streams_before_a_power_cycle_reports_no_overruns_and_idle_capture_reports_none() {
    let c = vnwa_open::topology::codecs().unwrap();
    let (mut a, mut b) = (
        Capture::open(&c.s11).unwrap(),
        Capture::open(&c.s21).unwrap(),
    );
    std::thread::sleep(Duration::from_secs(20));
    assert_eq!(a.overrun_count() + b.overrun_count(), 0, "idle capture");
    a.close();
    b.close();
    assert!(!a.is_open());
    cold_reset(&HubPort {
        hub: "3-2".into(),
        port: 2,
    })
    .unwrap();
    assert_eq!(
        a.overrun_count() + b.overrun_count(),
        0,
        "during the power cycle"
    );
    assert!(a
        .frames_after(0, 0, 10, Duration::from_millis(50))
        .unwrap_err()
        .contains("closed"));
}
