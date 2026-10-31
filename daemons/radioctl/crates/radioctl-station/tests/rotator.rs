// SPDX-License-Identifier: AGPL-3.0-or-later
//! The rotator handle against virtual rotators on loopback sockets and in-memory streams, over the
//! real reconnecting network link: both protocols, slewing and waiting for arrival, stop in
//! mid-slew, positions outside the travel refused before anything is written, a silent or stuck
//! rotator ending in bounded errors, a late reply not mistaken for the next one, concurrent
//! commands serialised, and a down link failing at once without writing.
#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use std::sync::Arc;
use std::time::Duration;

use radioctl_core::net::{open_link, NetConfig, RawFraming};
use radioctl_core::serial::LinkStatus;
use radioctl_station::error::StationErrorKind;
use radioctl_station::rotator::{Rotator, RotatorDescription};
use radioctl_station::virtual_rotator::{serve_virtual_rotator, VirtualRotator};
use tokio::io::{duplex, AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::runtime::Handle;
use tokio::sync::watch;
use tokio::time::{sleep, timeout};

fn desc(id: &str, slew: f32, reply_ms: u64) -> RotatorDescription {
    let mut d = RotatorDescription::builtin()
        .into_iter()
        .find(|d| d.id == id)
        .unwrap();
    d.slew_deg_per_s = slew;
    d.reply_timeout_ms = reply_ms;
    d
}

async fn over_net(
    d: &RotatorDescription,
) -> (
    Rotator,
    radioctl_station::virtual_rotator::VirtualRotatorControl,
) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let control =
        serve_virtual_rotator(&Handle::current(), listener, VirtualRotator::new(d.clone()));
    let link = open_link(
        &Handle::current(),
        NetConfig::new(addr.to_string()),
        Box::new(RawFraming),
    )
    .await
    .expect("link");
    let watch = link.watch();
    let (transport, monitor) = link.into_parts();
    std::mem::forget(monitor);
    (
        Rotator::new(d.engine().unwrap(), transport, Some(watch)),
        control,
    )
}

// Tests [@ANCHOR: radioctl_station_rotator_new]
// Tests [@ANCHOR: radioctl_station_rotator_description]
// Tests [@ANCHOR: radioctl_station_rotator_set_position]
// Tests [@ANCHOR: radioctl_station_rotator_position]
// Tests [@ANCHOR: radioctl_station_rotator_wait_for]
// Tests [@ANCHOR: radioctl_station_rotator_stop]
// Tests [@ANCHOR: radioctl_station_rotator_transact]
// Tests [@ANCHOR: radioctl_station_rotator_link_ok]
// Tests [@ANCHOR: radioctl_station_rotator_drain]
// Tests [@ANCHOR: radioctl_station_vrot_serve]
// Tests [@ANCHOR: radioctl_station_vrot_lock]
// Tests [@ANCHOR: radioctl_station_vrot_control_state]
// Tests [@ANCHOR: radioctl_station_vrot_control_received]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rotctld_slews_arrives_and_stops_mid_move() {
    let d = desc("generic-rotctld", 100.0, 1500);
    let (rot, control) = over_net(&d).await;
    assert_eq!(rot.description().id, "generic-rotctld");
    assert_eq!(rot.position().await.unwrap(), (0.0, 0.0));
    rot.set_position(90.0, 45.0).await.unwrap();
    rot.wait_for(90.0, 45.0, 1.0, Duration::from_secs(5))
        .await
        .unwrap();
    assert!((control.state().az - 90.0).abs() <= 1.5);

    // Stop in mid-slew: it freezes short of the target.
    rot.set_position(350.0, 10.0).await.unwrap();
    sleep(Duration::from_millis(300)).await;
    rot.stop().await.unwrap();
    let (az, _) = rot.position().await.unwrap();
    assert!(az > 90.0 && az < 350.0, "stopped en route at {az}");
    sleep(Duration::from_millis(200)).await;
    assert_eq!(rot.position().await.unwrap().0, az, "still frozen");
    assert!(control.received().iter().any(|l| l == "S"));
}

// Tests [@ANCHOR: radioctl_station_rotator_park]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn gs232_moves_parks_and_refuses_out_of_travel_before_writing() {
    let d = desc("yaesu-g-5500-gs232b", 500.0, 1500);
    let (rot, control) = over_net(&d).await;
    rot.set_position(200.0, 60.0).await.unwrap();
    rot.wait_for(200.0, 60.0, 1.0, Duration::from_secs(5))
        .await
        .unwrap();
    rot.park().await.unwrap();
    rot.wait_for(0.0, 0.0, 1.0, Duration::from_secs(5))
        .await
        .unwrap();

    let before = control.received().len();
    for (az, el) in [(451.0, 0.0), (-1.0, 0.0), (10.0, 181.0), (f32::NAN, 0.0)] {
        let e = rot.set_position(az, el).await.unwrap_err();
        assert_eq!(e.kind, StationErrorKind::Refused, "{az} {el}");
    }
    sleep(Duration::from_millis(100)).await;
    assert_eq!(
        control.received().len(),
        before,
        "nothing reached the rotator"
    );
    // Overlap travel (past 360) is allowed on this rotator.
    rot.set_position(440.0, 0.0).await.unwrap();
}

// Tests [@ANCHOR: radioctl_station_vrot_control_set_silent]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_silent_rotator_times_out_and_a_late_reply_is_not_taken_for_the_next() {
    let d = desc("generic-rotctld", 100.0, 300);
    let (rot, control) = over_net(&d).await;
    rot.set_position(10.0, 0.0).await.unwrap();
    control.set_silent(true);
    let started = std::time::Instant::now();
    let e = rot.position().await.unwrap_err();
    assert_eq!(e.kind, StationErrorKind::Timeout);
    assert!(started.elapsed() < Duration::from_secs(2), "bounded");
    control.set_silent(false);
    // The next command gets its own answer, not a leftover.
    rot.set_position(20.0, 0.0).await.unwrap();
    rot.wait_for(20.0, 0.0, 1.0, Duration::from_secs(5))
        .await
        .unwrap();
}

// Tests [@ANCHOR: radioctl_station_vrot_control_set_stuck]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_stuck_rotator_is_reported_with_where_it_was_seen() {
    let d = desc("generic-rotctld", 100.0, 1000);
    let (rot, control) = over_net(&d).await;
    control.set_stuck(true);
    rot.set_position(90.0, 0.0).await.unwrap();
    let e = rot
        .wait_for(90.0, 0.0, 1.0, Duration::from_millis(400))
        .await
        .unwrap_err();
    assert_eq!(e.kind, StationErrorKind::Timeout);
    assert!(e.detail.contains("last seen 0.0, 0.0"), "{}", e.detail);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn concurrent_commands_are_serialised_and_never_mixed() {
    let d = desc("generic-rotctld", 1000.0, 2000);
    let (rot, _control) = over_net(&d).await;
    let rot = Arc::new(rot);
    let mut tasks = Vec::new();
    for i in 0..20u32 {
        let rot = Arc::clone(&rot);
        tasks.push(tokio::spawn(async move {
            if i % 2 == 0 {
                rot.set_position(i as f32 * 10.0, 0.0).await.map(|()| None)
            } else {
                rot.position().await.map(Some)
            }
        }));
    }
    for t in tasks {
        let r = t.await.unwrap().unwrap();
        if let Some((az, el)) = r {
            assert!((0.0..=360.0).contains(&az) && el == 0.0);
        }
    }
}

// Tests [@ANCHOR: radioctl_station_rotator_stop]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_down_link_fails_at_once_without_writing_and_recovers() {
    let d = desc("generic-rotctld", 100.0, 500);
    let (near, mut far) = duplex(4096);
    let (tx, rx) = watch::channel(LinkStatus::Connected {
        port: "tcp://test".into(),
        generation: 1,
    });
    let rot = Rotator::new(d.engine().unwrap(), near, Some(rx));
    // The far end answers "RPRT 0" to anything while the link is up.
    let echo = tokio::spawn(async move {
        let mut seen = Vec::new();
        let mut buf = [0u8; 128];
        while let Ok(n) = far.read(&mut buf).await {
            if n == 0 {
                break;
            }
            seen.extend_from_slice(&buf[..n]);
            let _ = far.write_all(b"RPRT 0\n").await;
        }
        seen
    });
    rot.stop().await.unwrap();
    tx.send(LinkStatus::Reconnecting {
        attempt: 1,
        last_error: "cable".into(),
    })
    .unwrap();
    let started = std::time::Instant::now();
    let e = rot.stop().await.unwrap_err();
    assert!(e.detail.contains("link"), "{}", e.detail);
    assert!(
        started.elapsed() < Duration::from_millis(100),
        "fails at once"
    );
    tx.send(LinkStatus::Connected {
        port: "tcp://test".into(),
        generation: 2,
    })
    .unwrap();
    rot.stop().await.unwrap();
    drop(rot);
    let seen = timeout(Duration::from_secs(2), echo)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        seen, b"S\nS\n",
        "the command on the down link was never written"
    );
}
