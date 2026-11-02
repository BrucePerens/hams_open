// SPDX-License-Identifier: AGPL-3.0-or-later
//! The SAT-Schneider DRT1 behind the real rig actor: a tune is a control-line sequence driven on
//! the transport's lines (here a virtual DRT1 that decodes them), never bytes, and the closed
//! transmit gate and the missing-lines case are checked. No hardware is touched.
#![allow(clippy::expect_used, clippy::panic)]

use std::sync::Arc;
use std::time::Duration;

use radioctl_core::description::RigDescription;
use radioctl_core::error::ErrorKind;
use radioctl_core::event::Source;
use radioctl_core::op::Op;
use radioctl_core::poll::PollConfig;
use radioctl_core::rig::{spawn_current, RigHandle, RigOptions};
use radioctl_core::rigfile;
use radioctl_core::units::{Hz, Vfo};
use radioctl_protocols::drt1::{Drt1Engine, Drt1Twin};
use tokio::io::AsyncReadExt;

fn description() -> RigDescription {
    rigfile::parse(include_str!("../../../rigs/sat-schneider-drt1.toml"))
        .expect("DRT1 file")
        .description
}

fn options(twin: Option<Drt1Twin>) -> RigOptions {
    RigOptions {
        poll: PollConfig {
            enabled: false,
            ..PollConfig::default()
        },
        control_lines: twin.map(|t| Arc::new(t) as Arc<_>),
        ..RigOptions::default()
    }
}

/// A DRT1 actor over an idle byte stream; the far end reports every byte the actor writes.
fn drt1(twin: Option<Drt1Twin>) -> (RigHandle, tokio::task::JoinHandle<usize>) {
    let desc = description();
    let (near, mut far) = tokio::io::duplex(64);
    let counter = tokio::spawn(async move {
        let mut seen = 0;
        let mut buffer = [0u8; 64];
        while let Ok(n) = far.read(&mut buffer).await {
            if n == 0 {
                break;
            }
            seen += n;
        }
        seen
    });
    let handle =
        spawn_current(Drt1Engine::new(desc).expect("engine"), near, options(twin)).expect("spawn");
    (handle, counter)
}

// Tests [@ANCHOR: radioctl_core_actor_run_line_steps]
// Tests [@ANCHOR: radioctl_core_rig_drive_lines]
// Tests [@ANCHOR: radioctl_drt1_engine_encode_lines]
// Tests [@ANCHOR: radioctl_drt1_twin_set_control]
#[tokio::test(start_paused = true)]
async fn a_tune_reaches_the_dds_through_the_control_lines_and_writes_no_bytes() {
    let twin = Drt1Twin::new(&description()).expect("twin");
    let (handle, _counter) = drt1(Some(twin.clone()));
    assert_eq!(twin.frequency(), description().default_frequency);
    handle
        .set_frequency(Vfo::A, Hz(7_074_000))
        .await
        .expect("tunes");
    assert_eq!(twin.frequency(), Hz(7_074_000));
    handle
        .set_frequency(Vfo::A, Hz(15_000_000))
        .await
        .expect("tunes again");
    assert_eq!(twin.frequency(), Hz(15_000_000));
    drop(handle);
    assert_eq!(_counter.await.expect("far end"), 0, "no byte is written");
}

#[tokio::test(start_paused = true)]
async fn out_of_range_and_unsupported_commands_touch_no_line() {
    let twin = Drt1Twin::new(&description()).expect("twin");
    let (handle, _counter) = drt1(Some(twin.clone()));
    let error = handle
        .set_frequency(Vfo::A, Hz(31_000_000))
        .await
        .expect_err("above the range");
    assert_eq!(error.kind, ErrorKind::Malformed);
    assert_eq!(twin.registers(), (None, None));
    // The transmit gate is closed and the description has no PTT.
    let error = handle.set_ptt(true).await.expect_err("receiver");
    assert!(
        matches!(
            error.kind,
            ErrorKind::TransmitGateClosed | ErrorKind::Unsupported
        ),
        "{error:?}"
    );
    assert_eq!(twin.registers(), (None, None));
}

#[tokio::test(start_paused = true)]
async fn a_transport_without_control_lines_refuses_the_tune() {
    let (handle, _counter) = drt1(None);
    let error = handle
        .set_frequency(Vfo::A, Hz(7_074_000))
        .await
        .expect_err("no lines");
    assert_eq!(error.kind, ErrorKind::Unsupported);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_tune_that_outlasts_its_deadline_stops_between_steps_with_a_timeout() {
    let twin = Drt1Twin::new(&description()).expect("twin");
    let (handle, _counter) = drt1(Some(twin.clone()));
    let error = handle
        .execute_from(
            Op::SetFrequency(Vfo::A, Hz(7_074_000)),
            Duration::from_micros(300),
            Source::Api,
        )
        .await
        .expect_err("225 steps of 20 us cannot fit");
    assert_eq!(error.kind, ErrorKind::Timeout);
    assert_eq!(
        twin.frequency(),
        description().default_frequency,
        "an unfinished tune is not applied"
    );
}
