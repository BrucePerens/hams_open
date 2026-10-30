// SPDX-License-Identifier: AGPL-3.0-or-later
//! Behaviour of the rig actor against the real CI-V and Kenwood engines and virtual rigs, over
//! in-memory streams. Time is paused where a test depends on timing, so a "five second" radio
//! costs nothing and the assertions about delay are exact.
#![allow(clippy::expect_used, clippy::panic)]

use std::sync::mpsc as std_mpsc;
use std::sync::Arc;
use std::time::Duration;

use radioctl_core::description::RigDescription;
use radioctl_core::error::ErrorKind;
use radioctl_core::event::{RigEvent, Source};
use radioctl_core::op::{Field, Observation, Op};
use radioctl_core::poll::PollConfig;
use radioctl_core::rig::{spawn, spawn_current, Freshness, RigHandle, RigOptions, TransmitPermit};
use radioctl_core::units::{Hz, Mode, Vfo};
use radioctl_protocols::civ::CivEngine;
use radioctl_protocols::io::{spawn_virtual_rig, VirtualRigControl};
use radioctl_protocols::kenwood::KenwoodEngine;
use radioctl_protocols::virtual_rig::{PanelChange, VirtualRig};
use tokio::runtime::Handle;
use tokio::time::{sleep, timeout, Instant};

fn quiet() -> RigOptions {
    RigOptions {
        poll: PollConfig {
            enabled: false,
            ..PollConfig::default()
        },
        ..RigOptions::default()
    }
}

fn civ(options: RigOptions, delay: Duration) -> (RigHandle, VirtualRigControl) {
    let mut desc = RigDescription::ic7300();
    // A radio that takes seconds to answer needs a per-attempt timeout longer than its answer.
    desc.timing.timeout = Duration::from_secs(20);
    let rig = VirtualRig::from_description(&desc).expect("rig");
    let (stream, control) = spawn_virtual_rig(&Handle::current(), rig, delay);
    let handle =
        spawn_current(CivEngine::new(desc).expect("engine"), stream, options).expect("spawn");
    (handle, control)
}

fn kenwood(options: RigOptions, delay: Duration) -> (RigHandle, VirtualRigControl) {
    let desc = RigDescription::ts2000();
    let rig = VirtualRig::from_description(&desc).expect("rig");
    let (stream, control) = spawn_virtual_rig(&Handle::current(), rig, delay);
    let handle =
        spawn_current(KenwoodEngine::new(desc).expect("engine"), stream, options).expect("spawn");
    (handle, control)
}

fn permit() -> TransmitPermit {
    TransmitPermit::new("actor test")
}

// Tests [@ANCHOR: radioctl_core_rig_spawn_current]
// Tests [@ANCHOR: radioctl_core_handle_frequency]
// Tests [@ANCHOR: radioctl_core_handle_set_frequency]
// Tests [@ANCHOR: radioctl_core_handle_mode]
// Tests [@ANCHOR: radioctl_core_handle_set_mode]
// Tests [@ANCHOR: radioctl_core_handle_cached]
// Tests [@ANCHOR: radioctl_core_handle_signal]
// Tests [@ANCHOR: radioctl_core_actor_run]
// Tests [@ANCHOR: radioctl_core_actor_serve]
// Tests [@ANCHOR: radioctl_core_actor_run_command]
// Tests [@ANCHOR: radioctl_core_actor_absorb]
// Tests [@ANCHOR: radioctl_core_actor_apply_set]
// Tests [@ANCHOR: radioctl_core_actor_await_reply]
// Tests [@ANCHOR: radioctl_core_actor_log]
// Tests [@ANCHOR: radioctl_core_handle_read]
#[tokio::test(start_paused = true)]
async fn commands_round_trip_and_the_cache_reports_age() {
    let (rig, control) = civ(quiet(), Duration::ZERO);
    let first = rig
        .frequency(Vfo::A, Freshness::Cached)
        .await
        .expect("read");
    assert_eq!(first.value, Hz(14_074_000));
    rig.set_frequency(Vfo::A, Hz(7_074_000)).await.expect("set");
    assert_eq!(control.state().freq_a, Hz(7_074_000));
    rig.set_mode(Mode::Cw).await.expect("mode");
    assert_eq!(
        rig.mode(Freshness::Cached).await.expect("mode").value,
        Mode::Cw
    );

    sleep(Duration::from_secs(3)).await;
    let aged = rig.cached(Field::Frequency(Vfo::A)).expect("cached");
    assert_eq!(aged.value, Observation::Frequency(Vfo::A, Hz(7_074_000)));
    assert!(aged.age >= Duration::from_secs(3));
    let stale = rig
        .frequency(Vfo::A, Freshness::MaxAge(Duration::from_secs(1)))
        .await
        .expect("refresh");
    assert_eq!(stale.age, Duration::ZERO);
    let still = rig
        .frequency(Vfo::A, Freshness::Cached)
        .await
        .expect("cached");
    assert!(still.age < Duration::from_secs(1));
    // The rig changed behind our back; Fresh sees it, Cached does not.
    control.front_panel(PanelChange::Frequency(Hz(10_136_000)));
    sleep(Duration::from_millis(10)).await;
    let cached = rig
        .frequency(Vfo::A, Freshness::Cached)
        .await
        .expect("cached");
    assert!(cached.value == Hz(10_136_000) || cached.value == Hz(7_074_000));
    let fresh = rig
        .frequency(Vfo::A, Freshness::Fresh)
        .await
        .expect("fresh");
    assert_eq!(fresh.value, Hz(10_136_000));
    assert_eq!(
        rig.signal(Freshness::Fresh).await.expect("signal").value,
        30
    );
    assert_eq!(rig.stats().commands_failed, 0);
}

// Tests [@ANCHOR: radioctl_core_handle_open_gate]
// Tests [@ANCHOR: radioctl_core_handle_close_gate]
// Tests [@ANCHOR: radioctl_core_handle_gate_state]
// Tests [@ANCHOR: radioctl_core_handle_set_ptt]
// Tests [@ANCHOR: radioctl_core_handle_ptt]
// Tests [@ANCHOR: radioctl_core_transmit_permit_new]
// Tests [@ANCHOR: radioctl_core_transmit_permit_granted_by]
#[tokio::test(start_paused = true)]
async fn transmit_fails_closed_until_the_gate_is_opened_per_rig() {
    let (rig, control) = civ(quiet(), Duration::ZERO);
    let (other, other_control) = civ(quiet(), Duration::ZERO);
    assert!(rig.transmit_permit().is_none());

    let denied = rig.set_ptt(true).await.expect_err("gate closed");
    assert_eq!(denied.kind, ErrorKind::TransmitGateClosed);
    let cw = rig
        .execute(Op::SendCw("CQ".into()))
        .await
        .expect_err("gate closed");
    assert_eq!(cw.kind, ErrorKind::TransmitGateClosed);
    assert_eq!(control.bytes_received(), 0, "nothing reached the rig");
    assert!(!control.state().ptt);

    // Unkeying is always allowed, whatever the gate says.
    rig.set_ptt(false).await.expect("unkey allowed");
    assert!(control.bytes_received() > 0);

    rig.open_transmit_gate(permit());
    assert_eq!(
        rig.transmit_permit().map(|p| p.granted_by().to_string()),
        Some("actor test".to_string())
    );
    rig.set_ptt(true).await.expect("keyed");
    assert!(control.state().ptt);
    assert!(rig.ptt(Freshness::Fresh).await.expect("ptt").value);

    // The other rig's gate is untouched.
    let denied = other.set_ptt(true).await.expect_err("other gate closed");
    assert_eq!(denied.kind, ErrorKind::TransmitGateClosed);
    assert!(!other_control.state().ptt);

    rig.set_ptt(false).await.expect("unkey");
    rig.close_transmit_gate();
    assert!(rig.transmit_permit().is_none());
    assert_eq!(
        rig.set_ptt(true).await.expect_err("closed again").kind,
        ErrorKind::TransmitGateClosed
    );
}

// Tests [@ANCHOR: radioctl_core_actor_run_command]
#[tokio::test(start_paused = true)]
async fn a_transmit_command_already_queued_is_refused_when_the_gate_closes() {
    let (rig, control) = civ(quiet(), Duration::from_secs(5));
    rig.open_transmit_gate(permit());
    let slow = {
        let rig = rig.clone();
        tokio::spawn(async move {
            rig.execute_within(Op::GetMode, Duration::from_secs(30))
                .await
        })
    };
    sleep(Duration::from_millis(10)).await;
    let queued = {
        let rig = rig.clone();
        tokio::spawn(async move {
            rig.execute_within(Op::SetPtt(true), Duration::from_secs(30))
                .await
        })
    };
    sleep(Duration::from_millis(10)).await;
    rig.close_transmit_gate();
    slow.await.expect("join").expect("slow read finished");
    let refused = queued
        .await
        .expect("join")
        .expect_err("refused at run time");
    assert_eq!(refused.kind, ErrorKind::TransmitGateClosed);
    assert!(!control.state().ptt);
}

// Tests [@ANCHOR: radioctl_core_handle_execute_within]
// Tests [@ANCHOR: radioctl_core_handle_execute]
#[tokio::test(start_paused = true)]
async fn a_slow_rig_does_not_delay_another_rigs_ptt_off() {
    let (slow, _slow_control) = civ(quiet(), Duration::from_secs(5));
    let (fast, fast_control) = civ(quiet(), Duration::ZERO);
    fast.open_transmit_gate(permit());
    fast.set_ptt(true).await.expect("keyed");
    assert!(fast_control.state().ptt);

    let pending = {
        let slow = slow.clone();
        tokio::spawn(async move {
            slow.execute_within(Op::GetFrequency(Vfo::A), Duration::from_secs(30))
                .await
        })
    };
    sleep(Duration::from_millis(10)).await;
    let started = Instant::now();
    fast.set_ptt(false).await.expect("unkeyed");
    assert!(Instant::now() - started < Duration::from_millis(100));
    assert!(!fast_control.state().ptt);
    assert!(!pending.is_finished(), "the slow rig is still waiting");
    pending
        .await
        .expect("join")
        .expect("slow read completes in the end");
}

// Tests [@ANCHOR: radioctl_core_actor_run]
// Tests [@ANCHOR: radioctl_core_actor_run_command]
#[tokio::test(start_paused = true)]
async fn ptt_off_abandons_a_slow_read_on_the_same_rig() {
    let (rig, control) = civ(quiet(), Duration::ZERO);
    rig.open_transmit_gate(permit());
    rig.set_ptt(true).await.expect("keyed");
    control.set_reply_delay(Duration::from_secs(3));

    let read = {
        let rig = rig.clone();
        tokio::spawn(async move {
            rig.execute_within(Op::GetFrequency(Vfo::A), Duration::from_secs(30))
                .await
        })
    };
    sleep(Duration::from_millis(10)).await;
    let unkey = {
        let rig = rig.clone();
        tokio::spawn(async move {
            rig.execute_within(Op::SetPtt(false), Duration::from_secs(30))
                .await
        })
    };
    sleep(Duration::from_millis(10)).await;
    // The rig has already been told to unkey, although its answers take three seconds.
    assert!(!control.state().ptt);
    assert!(read.is_finished(), "the read was abandoned at once");
    let cancelled = read.await.expect("join").expect_err("abandoned");
    assert_eq!(cancelled.kind, ErrorKind::Cancelled);
    unkey
        .await
        .expect("join")
        .expect("unkey acknowledged in the end");
    assert_eq!(rig.stats().preempted, 1);
    // The late answer to the abandoned read does not confuse the next command.
    control.set_reply_delay(Duration::ZERO);
    let next = rig
        .frequency(Vfo::A, Freshness::Fresh)
        .await
        .expect("next read");
    assert_eq!(next.value, Hz(14_074_000));
}

// Tests [@ANCHOR: radioctl_core_rig_options_with_callback]
// Tests [@ANCHOR: radioctl_core_handle_stats]
// Tests [@ANCHOR: radioctl_core_actor_observe]
#[tokio::test(start_paused = true)]
async fn a_blocked_callback_never_delays_ptt_off_and_events_coalesce() {
    let (release_tx, release_rx) = std_mpsc::channel::<()>();
    let release_rx = std::sync::Mutex::new(release_rx);
    let (seen_tx, seen_rx) = std_mpsc::channel::<RigEvent>();
    let options = RigOptions {
        dispatcher_capacity: 1,
        ..quiet()
    }
    .with_callback(move |event| {
        let _ = seen_tx.send(event);
        let held = release_rx.lock().unwrap_or_else(|p| p.into_inner());
        let _ = held.recv_timeout(Duration::from_secs(30));
    });
    let (rig, control) = civ(options, Duration::ZERO);
    rig.open_transmit_gate(permit());
    rig.set_ptt(true).await.expect("keyed");
    seen_seq(&seen_rx);
    let started = Instant::now();
    for step in 0..6u64 {
        rig.set_frequency(Vfo::A, Hz(14_000_000 + step))
            .await
            .expect("set");
    }
    rig.set_ptt(false).await.expect("unkeyed");
    assert!(
        Instant::now() - started < Duration::from_millis(100),
        "virtual time barely moved"
    );
    assert!(!control.state().ptt);
    let stats = rig.stats();
    assert!(stats.dispatcher.queue_full > 0, "{stats:?}");
    assert!(stats.dispatcher.values_coalesced > 0, "{stats:?}");
    for _ in 0..8 {
        let _ = release_tx.send(());
    }
}

fn seen_seq(rx: &std_mpsc::Receiver<RigEvent>) {
    let first = rx
        .recv_timeout(Duration::from_secs(10))
        .expect("the first event reaches the callback");
    assert_eq!(first.changes.first().map(|c| c.field), Some(Field::Ptt));
}

// Tests [@ANCHOR: radioctl_core_event_stream_next]
// Tests [@ANCHOR: radioctl_core_handle_events]
#[tokio::test(start_paused = true)]
async fn a_panicking_callback_leaves_the_rig_and_later_events_alone() {
    let (seen_tx, seen_rx) = std_mpsc::channel::<RigEvent>();
    let calls = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let counter = Arc::clone(&calls);
    let options = quiet().with_callback(move |event| {
        if counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0 {
            panic!("application callback bug");
        }
        let _ = seen_tx.send(event);
    });
    let (rig, _control) = civ(options, Duration::ZERO);
    let mut stream = rig.events();
    rig.set_frequency(Vfo::A, Hz(7_000_000))
        .await
        .expect("first change");
    let panicked = timeout(Duration::from_secs(1), stream.next())
        .await
        .expect("stream event")
        .expect("some");
    assert_eq!(panicked.source, Source::Api);
    rig.set_frequency(Vfo::A, Hz(7_100_000))
        .await
        .expect("second change");
    let later = seen_rx
        .recv_timeout(Duration::from_secs(10))
        .expect("delivered after the panic");
    assert_eq!(
        later.changes.first().map(|c| c.new),
        Some(Observation::Frequency(Vfo::A, Hz(7_100_000)))
    );
    assert_eq!(rig.stats().dispatcher.callback_panics, 1);
    assert_eq!(
        rig.frequency(Vfo::A, Freshness::Fresh)
            .await
            .expect("rig works")
            .value,
        Hz(7_100_000)
    );
}

// Tests [@ANCHOR: radioctl_core_event_stream_next]
#[tokio::test(start_paused = true)]
async fn a_lagging_stream_subscriber_skips_and_counts() {
    let options = RigOptions {
        stream_capacity: 2,
        ..quiet()
    };
    let (rig, _control) = civ(options, Duration::ZERO);
    let mut stream = rig.events();
    for step in 0..6u64 {
        rig.set_frequency(Vfo::A, Hz(7_000_000 + step))
            .await
            .expect("set");
    }
    let event = stream.next().await.expect("an event");
    assert!(matches!(
        event.changes.first().map(|c| c.new),
        Some(Observation::Frequency(_, _))
    ));
    assert!(rig.stats().stream_lagged >= 1);
}

// Tests [@ANCHOR: radioctl_core_actor_poll_once]
#[tokio::test(start_paused = true)]
async fn polling_finds_a_front_panel_change_and_reports_old_and_new() {
    let (rig, control) = kenwood(RigOptions::default(), Duration::ZERO);
    let mut stream = rig.events();
    // Let the first round of polls fill the cache.
    sleep(Duration::from_secs(1)).await;
    let first = rig.cached(Field::Frequency(Vfo::A)).expect("polled");
    assert_eq!(first.value, Observation::Frequency(Vfo::A, Hz(14_074_000)));
    while timeout(Duration::from_millis(1), stream.next())
        .await
        .is_ok()
    {}

    control.front_panel(PanelChange::Frequency(Hz(7_100_000)));
    let event = timeout(Duration::from_secs(5), stream.next())
        .await
        .expect("event in time")
        .expect("some");
    assert_eq!(event.source, Source::Poll);
    let change = event.changes.first().expect("a change");
    assert_eq!(
        change.old,
        Some(Observation::Frequency(Vfo::A, Hz(14_074_000)))
    );
    assert_eq!(change.new, Observation::Frequency(Vfo::A, Hz(7_100_000)));
    assert!(rig.stats().polls > 0);
}

// Tests [@ANCHOR: radioctl_core_actor_poll_once]
// Tests [@ANCHOR: radioctl_core_wait_until]
#[tokio::test(start_paused = true)]
async fn polling_backs_off_when_nothing_changes() {
    let (rig, _control) = kenwood(RigOptions::default(), Duration::ZERO);
    sleep(Duration::from_secs(60)).await;
    let polls = rig.stats().polls;
    // Fixed 250 ms polling of three fields would be about 720.
    assert!(polls > 10 && polls < 120, "{polls} polls in 60 s");
}

// Tests [@ANCHOR: radioctl_core_actor_run]
#[tokio::test(start_paused = true)]
async fn transceive_frames_become_events_and_slow_the_polling() {
    let (rig, control) = civ(RigOptions::default(), Duration::ZERO);
    let mut stream = rig.events();
    sleep(Duration::from_secs(1)).await;
    while timeout(Duration::from_millis(1), stream.next())
        .await
        .is_ok()
    {}
    control.front_panel(PanelChange::Frequency(Hz(21_074_000)));
    let event = timeout(Duration::from_millis(100), stream.next())
        .await
        .expect("prompt")
        .expect("some");
    assert_eq!(event.source, Source::Transceive);
    assert_eq!(
        event.changes.first().map(|c| c.new),
        Some(Observation::Frequency(Vfo::A, Hz(21_074_000)))
    );
    assert_eq!(rig.stats().unsolicited, 1);
}

// Tests [@ANCHOR: radioctl_core_actor_run_command]
#[tokio::test(start_paused = true)]
async fn typed_errors_carry_kind_retry_hint_and_transcript() {
    let (rig, _control) = civ(quiet(), Duration::from_secs(100));
    let error = rig
        .execute_within(Op::GetFrequency(Vfo::A), Duration::from_millis(500))
        .await
        .expect_err("times out");
    assert_eq!(error.kind, ErrorKind::Timeout);
    assert_eq!(error.retry, radioctl_core::error::RetryHint::Now);
    assert_eq!(
        error.transcript_tail.first().map(String::as_str),
        Some("TX fe fe 94 e0 03 fd")
    );

    // Refused by the rig: the virtual rig lacks a mode the engine's description has.
    let mut rig_desc = RigDescription::ic7300();
    rig_desc.mode_codes.retain(|(mode, _)| *mode != Mode::Wfm);
    let virtual_rig = VirtualRig::from_description(&rig_desc).expect("rig");
    let (stream, _control) = spawn_virtual_rig(&Handle::current(), virtual_rig, Duration::ZERO);
    let refusing = spawn_current(
        CivEngine::new(RigDescription::ic7300()).expect("engine"),
        stream,
        quiet(),
    )
    .expect("spawn");
    let refused = refusing.set_mode(Mode::Wfm).await.expect_err("NG");
    assert_eq!(refused.kind, ErrorKind::Rejected);
    assert_eq!(refused.retry, radioctl_core::error::RetryHint::Never);

    // Not in the description: refused before any I/O.
    let (ken, ken_control) = kenwood(quiet(), Duration::ZERO);
    assert_eq!(
        ken.ptt(Freshness::Fresh)
            .await
            .expect_err("unsupported")
            .kind,
        ErrorKind::Unsupported
    );
    assert_eq!(ken_control.bytes_received(), 0);
}

// Tests [@ANCHOR: radioctl_core_handle_execute]
// Tests [@ANCHOR: radioctl_core_handle_submit]
#[tokio::test(start_paused = true)]
async fn a_full_queue_is_a_typed_error_not_a_wait() {
    let options = RigOptions {
        queue_capacity: 1,
        ..quiet()
    };
    let (rig, _control) = civ(options, Duration::from_secs(10));
    let mut running = Vec::new();
    for _ in 0..2 {
        let rig = rig.clone();
        running.push(tokio::spawn(async move {
            rig.execute_within(Op::GetMode, Duration::from_secs(60))
                .await
        }));
        sleep(Duration::from_millis(10)).await;
    }
    let overflow = rig.execute(Op::GetMode).await.expect_err("queue is full");
    assert_eq!(overflow.kind, ErrorKind::QueueFull);
    for task in running {
        task.await
            .expect("join")
            .expect("queued commands still complete");
    }
}

// Tests [@ANCHOR: radioctl_core_handle_link_up]
#[tokio::test(start_paused = true)]
async fn a_dead_transport_ends_in_link_down() {
    let (near, far) = tokio::io::duplex(64);
    let rig = spawn_current(
        CivEngine::new(RigDescription::ic7300()).expect("engine"),
        near,
        quiet(),
    )
    .expect("spawn");
    assert!(rig.link_up());
    drop(far);
    sleep(Duration::from_millis(10)).await;
    assert!(!rig.link_up());
    assert_eq!(
        rig.mode(Freshness::Fresh).await.expect_err("down").kind,
        ErrorKind::LinkDown
    );
}

// Tests [@ANCHOR: radioctl_core_rig_spawn]
// Tests [@ANCHOR: radioctl_core_handle_id]
// Tests [@ANCHOR: radioctl_core_handle_capabilities]
#[test]
fn two_rigs_on_two_runtimes_work_and_a_handle_works_with_no_runtime_at_all() {
    let build = || {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_time()
            .build()
            .expect("runtime")
    };
    let (runtime_a, runtime_b) = (build(), build());
    let start = |runtime: &tokio::runtime::Runtime, desc: RigDescription| {
        let rig = VirtualRig::from_description(&desc).expect("rig");
        let (stream, control) = spawn_virtual_rig(runtime.handle(), rig, Duration::from_millis(5));
        let handle = spawn(
            runtime.handle(),
            CivEngine::new(desc).expect("engine"),
            stream,
            quiet(),
        )
        .expect("spawn");
        (handle, control)
    };
    let (rig_a, control_a) = start(&runtime_a, RigDescription::ic7300());
    let mut other = RigDescription::ic7300();
    other.id = "second-ic7300".into();
    let (rig_b, control_b) = start(&runtime_b, other);
    assert_eq!(rig_a.id(), "icom-ic-7300");
    assert_eq!(rig_b.id(), "second-ic7300");
    assert!(rig_a
        .capabilities()
        .has(radioctl_core::capability::Capability::SetFrequency));

    // This thread has no tokio runtime; the futures only wait on channels.
    assert!(Handle::try_current().is_err());
    futures::executor::block_on(async {
        rig_a
            .set_frequency(Vfo::A, Hz(3_573_000))
            .await
            .expect("set on runtime A");
        rig_b
            .set_frequency(Vfo::A, Hz(7_074_000))
            .await
            .expect("set on runtime B");
        let a = rig_a
            .frequency(Vfo::A, Freshness::Fresh)
            .await
            .expect("read A");
        let b = rig_b
            .frequency(Vfo::A, Freshness::Fresh)
            .await
            .expect("read B");
        assert_eq!((a.value, b.value), (Hz(3_573_000), Hz(7_074_000)));
    });
    assert_eq!(control_a.state().freq_a, Hz(3_573_000));
    assert_eq!(control_b.state().freq_a, Hz(7_074_000));

    // A handle also works from a task on a third runtime, other than the one running its actor.
    let runtime_c = build();
    let read = runtime_c.block_on(async {
        rig_a
            .frequency(Vfo::A, Freshness::Fresh)
            .await
            .expect("cross read")
    });
    assert_eq!(read.value, Hz(3_573_000));
    // And from a plain OS thread.
    let from_thread = rig_b.clone();
    let joined = std::thread::spawn(move || {
        futures::executor::block_on(async {
            from_thread
                .set_mode(Mode::Lsb)
                .await
                .expect("mode from a thread")
        })
    });
    joined.join().expect("thread");
    assert_eq!(control_b.state().mode, Mode::Lsb);
}

#[cfg(feature = "owned-runtime")]
// Tests [@ANCHOR: radioctl_core_rig_spawn_with_owned_runtime]
// Tests [@ANCHOR: radioctl_core_rig_owned_shutdown]
#[test]
fn the_owned_runtime_constructor_works_without_any_caller_runtime() {
    let desc = RigDescription::ic7300();
    let rig_model = VirtualRig::from_description(&desc).expect("rig");
    let mut control_slot = None;
    let owned = radioctl_core::rig::spawn_with_owned_runtime(
        || {
            let (stream, control) =
                spawn_virtual_rig(&Handle::current(), rig_model, Duration::ZERO);
            control_slot = Some(control);
            stream
        },
        CivEngine::new(desc).expect("engine"),
        quiet(),
    )
    .expect("owned rig");
    let value = futures::executor::block_on(owned.handle.frequency(Vfo::A, Freshness::Fresh))
        .expect("read");
    assert_eq!(value.value, Hz(14_074_000));
    assert!(control_slot.is_some());
    owned.shutdown();
}

// Tests [@ANCHOR: radioctl_core_actor_spacing]
#[tokio::test(start_paused = true)]
async fn the_description_s_inter_command_gap_is_kept() {
    let mut desc = RigDescription::ts2000();
    desc.timing.inter_command = Duration::from_millis(100);
    let rig_model = VirtualRig::from_description(&desc).expect("rig");
    let (stream, _control) = spawn_virtual_rig(&Handle::current(), rig_model, Duration::ZERO);
    let rig =
        spawn_current(KenwoodEngine::new(desc).expect("engine"), stream, quiet()).expect("spawn");
    rig.set_frequency(Vfo::A, Hz(7_000_000))
        .await
        .expect("first");
    let started = Instant::now();
    rig.set_frequency(Vfo::A, Hz(7_001_000))
        .await
        .expect("second");
    assert!(Instant::now() - started >= Duration::from_millis(90));
}
