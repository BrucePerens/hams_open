// SPDX-License-Identifier: AGPL-3.0-or-later
//! The one-call helpers of `radioctl_core::prelude` against the virtual IC-7300 and KX3 built
//! from the shipped rig descriptions. Time is paused where a lease timer matters. Nothing here
//! touches hardware.
#![allow(clippy::expect_used, clippy::panic)]

use std::time::Duration;

use radioctl_core::poll::PollConfig;
use radioctl_core::prelude::*;
use radioctl_protocols::civ::CivEngine;
use radioctl_protocols::io::{spawn_virtual_rig, VirtualRigControl};
use radioctl_protocols::virtual_rig::VirtualRig;
use tokio::runtime::Handle;
use tokio::time::{sleep, timeout};

fn quiet() -> RigOptions {
    RigOptions {
        poll: PollConfig {
            enabled: false,
            ..PollConfig::default()
        },
        ..RigOptions::default()
    }
}

fn ic7300() -> (RigHandle, VirtualRigControl) {
    let description = rigfile::load("icom-ic-7300")
        .expect("description")
        .description;
    let radio = VirtualRig::from_description(&description).expect("virtual rig");
    let (stream, control) = spawn_virtual_rig(&Handle::current(), radio, Duration::ZERO);
    let rig = spawn_current(
        CivEngine::new(description).expect("engine"),
        stream,
        quiet(),
    )
    .expect("spawn");
    (rig, control)
}

fn permit() -> TransmitPermit {
    TransmitPermit::new("easy test")
}

// Tests [@ANCHOR: radioctl_core_rigfile_rigs_dir]
// Tests [@ANCHOR: radioctl_core_rigfile_load]
#[test]
fn a_description_loads_by_id_or_by_path_and_a_miss_names_the_file() {
    assert!(rigfile::rigs_dir().join("elecraft-kx3.toml").is_file());
    let by_id = rigfile::load("elecraft-kx3").expect("by id");
    let path = rigfile::rigs_dir().join("elecraft-kx3.toml");
    let by_path = rigfile::load(path.to_str().expect("utf-8")).expect("by path");
    assert_eq!(by_id.description.id, "elecraft-kx3");
    assert_eq!(by_id.description, by_path.description);
    let miss = rigfile::load("no-such-rig").expect_err("a miss");
    assert!(miss.detail.contains("no-such-rig.toml"), "{}", miss.detail);
    let miss = rigfile::load("/nonexistent/dir/rig.toml").expect_err("a path miss");
    assert!(
        miss.detail.contains("/nonexistent/dir/rig.toml"),
        "{}",
        miss.detail
    );
}

// Tests [@ANCHOR: radioctl_core_easy_level]
// Tests [@ANCHOR: radioctl_core_easy_set_level]
#[tokio::test(start_paused = true)]
async fn a_level_is_set_and_read_back_typed_and_from_the_cache() {
    let (rig, _control) = ic7300();
    rig.set_level(Level::AfGain, 120).await.expect("set");
    let fresh = rig
        .level(Level::AfGain, Freshness::Fresh)
        .await
        .expect("fresh");
    assert_eq!(fresh.value, 120);
    let cached = rig
        .level(Level::AfGain, Freshness::Cached)
        .await
        .expect("cached");
    assert_eq!(cached.value, 120);
    assert!(cached.age < Duration::from_secs(1));
}

// Tests [@ANCHOR: radioctl_core_easy_meter]
#[tokio::test(start_paused = true)]
async fn a_raw_meter_is_read_typed() {
    let (rig, _control) = ic7300();
    let swr = rig.meter(Meter::Swr, Freshness::Fresh).await.expect("swr");
    assert_eq!(swr.age, Duration::ZERO);
    let again = rig
        .meter(Meter::Swr, Freshness::Cached)
        .await
        .expect("cached");
    assert_eq!(again.value, swr.value);
}

// Tests [@ANCHOR: radioctl_core_easy_next_change]
#[tokio::test(start_paused = true)]
async fn next_change_skips_other_fields_and_reports_the_source() {
    let (rig, _control) = ic7300();
    let mut events = rig.events();
    rig.set_mode(Mode::Cw).await.expect("mode");
    rig.set_frequency(Vfo::A, Hz(7_074_000))
        .await
        .expect("frequency");
    let (change, source) = timeout(
        Duration::from_secs(5),
        events.next_change(Field::Frequency(Vfo::A)),
    )
    .await
    .expect("in time")
    .expect("a change");
    assert_eq!(change.new, Observation::Frequency(Vfo::A, Hz(7_074_000)));
    assert_eq!(source, Source::Api);
}

// Tests [@ANCHOR: radioctl_core_easy_next_change]
#[tokio::test(start_paused = true)]
async fn next_change_ends_with_none_when_the_rig_stops() {
    let (rig, control) = ic7300();
    let mut events = rig.events();
    drop(rig);
    drop(control);
    let ended = timeout(Duration::from_secs(5), events.next_change(Field::Mode))
        .await
        .expect("in time");
    assert!(ended.is_none());
}

// Tests [@ANCHOR: radioctl_core_easy_start_scope]
// Tests [@ANCHOR: radioctl_core_easy_stop_scope]
#[tokio::test(start_paused = true)]
async fn a_scope_stream_starts_subscribed_and_stops() {
    let (rig, control) = ic7300();
    let mut frames = rig.start_scope().await.expect("start");
    control.scope_sweep();
    let frame = timeout(Duration::from_secs(5), frames.next())
        .await
        .expect("in time")
        .expect("a sweep");
    assert!(frame.high > frame.low);
    rig.stop_scope().await.expect("stop");
    let off = rig.execute(Op::GetScopeOutput).await.expect("read");
    assert_eq!(off, Some(Observation::ScopeOutput(false)));
}

// Tests [@ANCHOR: radioctl_core_easy_transmit_lease]
#[tokio::test(start_paused = true)]
async fn a_lease_opens_the_gate_only_for_its_permit_and_refuses_what_is_unsafe() {
    let (rig, _control) = ic7300();
    assert_eq!(
        rig.transmit_lease(permit(), Duration::ZERO)
            .err()
            .map(|e| e.kind),
        Some(ErrorKind::Rejected)
    );
    assert_eq!(
        rig.transmit_permit(),
        None,
        "a refused lease leaves the gate closed"
    );
    let lease = rig
        .transmit_lease(permit(), Duration::from_secs(30))
        .expect("lease");
    assert_eq!(rig.transmit_permit(), Some(permit()));
    assert_eq!(
        rig.transmit_lease(permit(), Duration::from_secs(30))
            .err()
            .map(|e| e.kind),
        Some(ErrorKind::Rejected),
        "a second lease on an open gate"
    );
    drop(lease);
}

// Tests [@ANCHOR: radioctl_core_easy_lease_remaining]
// Tests [@ANCHOR: radioctl_core_easy_lease_key]
// Tests [@ANCHOR: radioctl_core_easy_lease_unkey]
#[tokio::test(start_paused = true)]
async fn a_lease_keys_and_unkeys_and_counts_down() {
    let (rig, control) = ic7300();
    let lease = rig
        .transmit_lease(permit(), Duration::from_secs(30))
        .expect("lease");
    assert_eq!(lease.remaining(), Duration::from_secs(30));
    lease.key().await.expect("key");
    assert!(control.state().ptt);
    sleep(Duration::from_secs(10)).await;
    assert_eq!(lease.remaining(), Duration::from_secs(20));
    lease.unkey().await.expect("unkey");
    assert!(!control.state().ptt);
    assert!(
        rig.transmit_permit().is_some(),
        "unkeying leaves the lease open"
    );
}

// Tests [@ANCHOR: radioctl_core_easy_lease_transmit_for]
#[tokio::test(start_paused = true)]
async fn transmit_for_keys_holds_and_unkeys() {
    let (rig, control) = ic7300();
    let lease = rig
        .transmit_lease(permit(), Duration::from_secs(30))
        .expect("lease");
    let hold = lease.transmit_for(Duration::from_secs(3));
    tokio::pin!(hold);
    assert!(
        timeout(Duration::from_secs(1), &mut hold).await.is_err(),
        "still holding"
    );
    assert!(control.state().ptt, "keyed while holding");
    hold.await.expect("transmit_for");
    assert!(!control.state().ptt);
}

// Tests [@ANCHOR: radioctl_core_easy_lease_transmit_for]
#[tokio::test(start_paused = true)]
async fn transmit_for_is_cut_short_by_the_lease() {
    let (rig, control) = ic7300();
    let lease = rig
        .transmit_lease(permit(), Duration::from_secs(5))
        .expect("lease");
    let started = tokio::time::Instant::now();
    lease
        .transmit_for(Duration::from_secs(60))
        .await
        .expect("transmit_for");
    assert!(started.elapsed() <= Duration::from_secs(6));
    assert!(!control.state().ptt);
}

// Tests [@ANCHOR: radioctl_core_easy_lease_release]
#[tokio::test(start_paused = true)]
async fn releasing_a_lease_closes_the_gate_and_unkeys() {
    let (rig, control) = ic7300();
    let lease = rig
        .transmit_lease(permit(), Duration::from_secs(30))
        .expect("lease");
    lease.key().await.expect("key");
    lease.release().await.expect("release");
    assert!(!control.state().ptt);
    assert_eq!(rig.transmit_permit(), None);
    assert_eq!(
        rig.set_ptt(true).await.err().map(|e| e.kind),
        Some(ErrorKind::TransmitGateClosed)
    );
}

// Tests [@ANCHOR: radioctl_core_easy_lease_drop]
#[tokio::test(start_paused = true)]
async fn dropping_a_keyed_lease_closes_the_gate_and_unkeys() {
    let (rig, control) = ic7300();
    let lease = rig
        .transmit_lease(permit(), Duration::from_secs(30))
        .expect("lease");
    lease.key().await.expect("key");
    assert!(control.state().ptt);
    drop(lease);
    sleep(Duration::from_millis(500)).await;
    assert!(!control.state().ptt, "the drop unkeyed the radio");
    assert_eq!(rig.transmit_permit(), None);
}

// Tests [@ANCHOR: radioctl_core_easy_transmit_lease]
#[tokio::test(start_paused = true)]
async fn an_expired_lease_unkeys_closes_the_gate_and_refuses_to_key() {
    let (rig, control) = ic7300();
    let lease = rig
        .transmit_lease(permit(), Duration::from_secs(5))
        .expect("lease");
    lease.key().await.expect("key");
    sleep(Duration::from_secs(6)).await;
    assert!(!control.state().ptt, "the lease's timer unkeyed the radio");
    assert_eq!(rig.transmit_permit(), None);
    assert_eq!(lease.remaining(), Duration::ZERO);
    assert_eq!(
        lease.key().await.err().map(|e| e.kind),
        Some(ErrorKind::TransmitGateClosed)
    );
    drop(lease);
    // The expired lease's drop must not close a gate someone opened since.
    rig.open_transmit_gate(TransmitPermit::new("someone else"));
    assert!(rig.transmit_permit().is_some());
}
