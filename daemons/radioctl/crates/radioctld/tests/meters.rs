// SPDX-License-Identifier: AGPL-3.0-or-later
//! The Hamlib level names for the raw CI-V transmitter meters, served by radioctld from a keyed
//! virtual IC-7300: `ALC`, `SWR`, `COMP_METER`, `VD_METER` and `ID_METER`. The raw deflections
//! are 0 to 255 integers from the radio; Hamlib's levels are floats (SWR ratio, ALC 0 to 1,
//! decibels, volts, amperes) and radioctld converts through `meter_cal`'s UNVERIFIED scale
//! points. The pinned numbers are what Hamlib 4.6.2 itself answers for the same deflections
//! (`rigctld_oracle.rs` in radioctl-protocols asserts them against the live Hamlib).
//! A virtual rig only; the transmit gate is opened in the configuration for a rig in memory.
#![allow(clippy::expect_used, clippy::panic)]

mod common;

use common::{config_text, eventually, start, Client};

/// Hamlib's answers for the keyed virtual IC-7300: SWR 12, ALC 80, COMP 0, Vd 130, Id 90 (raw).
const HAMLIB_KEYED_METERS: [f64; 5] = [1.125, 80.0 / 120.0, 0.0, 13.078947, 9.278351];

// Tests [@ANCHOR: radioctld_service_get_level]
// Tests [@ANCHOR: radioctld_service_meter]
// Tests [@ANCHOR: radioctld_meter_cal_level]
// Tests [@ANCHOR: radioctld_meter_cal_clamped]
// Tests [@ANCHOR: radioctld_caps_granularity]
// Tests [@ANCHOR: radioctld_names_served_levels]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn radioctld_serves_the_meters_the_way_hamlib_reads_them() {
    let daemon = start(&config_text("", "ic7300", "icom-ic-7300.toml", true, false)).await;
    let mut client = Client::connect(&daemon).await;
    // The unkeyed rig: only the supply voltage reads; the others read zero (SWR 1:1).
    assert_eq!(client.ask("l SWR").await, "1\n");
    assert_eq!(client.ask("l ID_METER").await, "0\n");
    assert_eq!(client.ask("l VD_METER").await, "13.078947\n");
    // Keyed (virtual rig, gate opened by the configuration).
    assert_eq!(client.ask("T 1").await, "RPRT 0\n");
    let rig = daemon.rigs[0].virtual_control.clone().expect("virtual rig");
    eventually("the virtual rig keyed", || rig.state().ptt).await;
    // The service caches a reading for a short time: each meter is read until the keyed value
    // shows, and then it must be Hamlib's.
    for (name, want) in ["SWR", "ALC", "COMP_METER", "VD_METER", "ID_METER"]
        .into_iter()
        .zip(HAMLIB_KEYED_METERS)
    {
        let started = std::time::Instant::now();
        loop {
            let reply = client.ask(&format!("l {name}")).await;
            let got: f64 = reply
                .trim()
                .parse()
                .unwrap_or_else(|_| panic!("{name}: {reply:?}"));
            if (got - want).abs() < 1e-6 {
                break;
            }
            assert!(
                started.elapsed() < std::time::Duration::from_secs(5),
                "{name}: {got} against Hamlib's {want}"
            );
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
    }
    // Read-only levels, and the speech-compressor level COMP is not the meter.
    assert_eq!(client.ask("L COMP_METER 1").await, "RPRT -11\n");
    assert_eq!(client.ask("l COMP").await, "RPRT -11\n");
    assert_eq!(client.ask("T 0").await, "RPRT 0\n");
    daemon.stop().await;
}
