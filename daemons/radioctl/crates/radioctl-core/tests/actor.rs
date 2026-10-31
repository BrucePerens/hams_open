// SPDX-License-Identifier: AGPL-3.0-or-later
//! Behaviour of the rig actor against the real CI-V and Kenwood engines and virtual rigs, over
//! in-memory streams. Time is paused where a test depends on timing, so a "five second" radio
//! costs nothing and the assertions about delay are exact.
#![allow(clippy::expect_used, clippy::panic)]

use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use futures_core::Stream;
use radioctl_core::capability::{Capability, CapabilitySet};
use radioctl_core::description::RigDescription;
use radioctl_core::error::ErrorKind;
use radioctl_core::event::{RigEvent, Source};
use radioctl_core::op::{Alarm, Field, Observation, Op};
use radioctl_core::poll::PollConfig;
use radioctl_core::rig::{
    spawn, spawn_current, EventStream, Freshness, RigHandle, RigOptions, TransmitPermit,
};
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
    // The late answer to the abandoned read does not confuse the next command: the frequency
    // moves on the front panel in between, so a stale answer taken for the fresh one would show.
    control.set_reply_delay(Duration::ZERO);
    control.front_panel(PanelChange::Frequency(Hz(7_030_000)));
    let next = rig
        .frequency(Vfo::A, Freshness::Fresh)
        .await
        .expect("next read");
    assert_eq!(next.value, Hz(7_030_000));
}

// Tests [@ANCHOR: radioctl_core_op_is_emergency]
// Tests [@ANCHOR: radioctl_core_op_is_unkey]
// Tests [@ANCHOR: radioctl_core_actor_serve]
// Tests [@ANCHOR: radioctl_core_actor_apply_set]
#[tokio::test(start_paused = true)]
async fn stop_morse_jumps_the_queue_needs_no_gate_and_leaves_the_key_alone() {
    let (rig, control) = civ(quiet(), Duration::ZERO);
    rig.open_transmit_gate(permit());
    rig.set_ptt(true).await.expect("keyed");
    rig.execute(Op::SendCw("CQ".into()))
        .await
        .expect("morse sent");
    assert_eq!(control.state().cw_sent, vec!["CQ".to_string()]);
    // The gate closes and the rig becomes slow: a stop must still get through at once.
    rig.close_transmit_gate();
    control.set_reply_delay(Duration::from_secs(3));
    let read = {
        let rig = rig.clone();
        tokio::spawn(async move {
            rig.execute_within(Op::GetFrequency(Vfo::A), Duration::from_secs(30))
                .await
        })
    };
    sleep(Duration::from_millis(10)).await;
    let stop = {
        let rig = rig.clone();
        tokio::spawn(async move {
            rig.execute_within(Op::StopCw, Duration::from_secs(30))
                .await
        })
    };
    sleep(Duration::from_millis(10)).await;
    assert_eq!(
        control.state().cw_stops,
        1,
        "the stop reached the rig at once"
    );
    assert!(control.state().cw_sent.is_empty());
    assert!(
        read.is_finished(),
        "the slow read was abandoned for the stop"
    );
    assert_eq!(
        read.await.expect("join").expect_err("abandoned").kind,
        ErrorKind::Cancelled
    );
    stop.await.expect("join").expect("stop acknowledged");
    assert_eq!(rig.stats().preempted, 1);
    // A stop is not an unkey: the rig stays keyed, the cache still says so, and nothing was
    // counted as an unkey failure.
    assert!(control.state().ptt);
    assert_eq!(
        rig.cached(Field::Ptt).map(|c| c.value),
        Some(Observation::Ptt(true))
    );
    assert_eq!(rig.stats().unkey_failures, 0);
    control.set_reply_delay(Duration::ZERO);
    rig.set_ptt(false).await.expect("unkeyed");
    assert!(!control.state().ptt);
}

// Tests [@ANCHOR: radioctl_core_actor_absorb]
// Tests [@ANCHOR: radioctl_core_actor_claim_reply]
// Tests [@ANCHOR: radioctl_core_actor_note_written]
// Tests [@ANCHOR: radioctl_core_actor_next_seq]
#[tokio::test(start_paused = true)]
async fn a_late_ack_for_a_timed_out_key_is_not_taken_for_the_unkeys_answer() {
    // Reply timeout 1 s, no retries; the radio answers 1.5 s late. The key times out at 1 s and
    // the fail-closed unkey goes out; the key's late OK arrives at 1.5 s while the unkey waits for
    // an OK of its own. By order it is the key's: the unkey's answer comes at 2.5 s.
    let (rig, control) = civ_with_timing(Duration::from_secs(1), 0);
    rig.open_transmit_gate(permit());
    control.set_reply_delay(Duration::from_millis(1500));
    let started = Instant::now();
    let error = rig.set_ptt(true).await.expect_err("the key times out");
    assert_eq!(error.kind, ErrorKind::Timeout);
    let waited = started.elapsed();
    assert!(
        waited >= Duration::from_millis(2400),
        "the unkey took the key's late OK as its own answer after {waited:?}"
    );
    assert!(!control.state().ptt);
    let stats = rig.stats();
    assert_eq!(stats.stale_replies, 1, "the key's late OK was told apart");
    assert_eq!(stats.late_replies, 1);
    assert_eq!(
        stats.unkey_failures, 0,
        "the unkey was answered, by its own OK"
    );
}

// Tests [@ANCHOR: radioctl_core_actor_absorb]
// Tests [@ANCHOR: radioctl_core_actor_drain_stale]
#[tokio::test(start_paused = true)]
async fn a_late_frequency_is_not_taken_for_the_next_reads_answer() {
    let (rig, control) = civ_with_timing(Duration::from_secs(1), 0);
    control.set_reply_delay(Duration::from_millis(1500));
    let first = rig.frequency(Vfo::A, Freshness::Fresh).await;
    assert_eq!(first.expect_err("times out").kind, ErrorKind::Timeout);
    // The dial moves; the late answer to the first read still carries the old frequency and is
    // on its way. The next read must report the new one, not the old answer.
    control.front_panel(PanelChange::Frequency(Hz(7_030_000)));
    control.set_reply_delay(Duration::ZERO);
    let started = Instant::now();
    let next = rig
        .frequency(Vfo::A, Freshness::Fresh)
        .await
        .expect("the next read is answered");
    assert_eq!(next.value, Hz(7_030_000));
    assert!(
        started.elapsed() >= Duration::from_millis(400),
        "the actor waited for the late answer before asking again"
    );
    let stats = rig.stats();
    assert_eq!(stats.late_replies, 1);
    assert_eq!(stats.stale_drains, 1);
    assert_eq!(
        stats.stale_replies, 0,
        "the late answer arrived before the new read went out"
    );
}

// Tests [@ANCHOR: radioctl_core_actor_drain_stale]
// Tests [@ANCHOR: radioctl_core_actor_claim_reply]
// Tests [@ANCHOR: radioctl_virtual_io_drop_replies]
#[tokio::test(start_paused = true)]
async fn a_reply_that_never_comes_stops_being_expected_and_does_not_eat_the_next_answer() {
    let (rig, control) = civ_with_timing(Duration::from_secs(1), 0);
    // The radio's answer to the first read is lost (a corrupted frame): it never comes.
    control.drop_replies(1);
    assert_eq!(
        rig.frequency(Vfo::A, Freshness::Fresh)
            .await
            .expect_err("times out")
            .kind,
        ErrorKind::Timeout
    );
    let started = Instant::now();
    let next = rig
        .frequency(Vfo::A, Freshness::Fresh)
        .await
        .expect("answered: the expectation of a late reply lapsed");
    assert_eq!(next.value, Hz(14_074_000));
    let waited = started.elapsed();
    assert!(
        waited >= Duration::from_millis(900) && waited < Duration::from_millis(1300),
        "waited one more timeout for the late reply, then asked: {waited:?}"
    );
    assert_eq!(rig.stats().stale_replies, 0);
    // An emergency command does not wait for a late reply.
    control.drop_replies(1);
    let _ = rig.frequency(Vfo::A, Freshness::Fresh).await;
    let started = Instant::now();
    rig.set_ptt(false).await.expect("unkeyed");
    assert!(started.elapsed() < Duration::from_millis(100));
}

// Tests [@ANCHOR: radioctl_core_actor_poll_reserved]
// Tests [@ANCHOR: radioctl_core_actor_poll_entry]
// Tests [@ANCHOR: radioctl_core_poll_take_starved]
#[tokio::test(start_paused = true)]
async fn the_transmit_state_is_still_polled_under_a_flood_of_client_commands() {
    let options = RigOptions {
        poll: PollConfig {
            ptt_reserve: Duration::from_secs(2),
            ..PollConfig::default()
        },
        ..RigOptions::default()
    };
    // The radio answers in a millisecond (so that paused time advances); four clients reading as
    // fast as the actor answers keep the queue from ever being empty, so ordinary polling, which
    // runs only when nothing is queued, never gets a turn.
    let (rig, control) = civ(options, Duration::from_millis(1));
    let mut events = rig.events();
    let stop = Arc::new(AtomicBool::new(false));
    let flooders: Vec<_> = (0..4)
        .map(|_| {
            let rig = rig.clone();
            let stop = Arc::clone(&stop);
            tokio::spawn(async move {
                while !stop.load(Ordering::Relaxed) {
                    let _ = rig.frequency(Vfo::A, Freshness::Fresh).await;
                }
            })
        })
        .collect();
    sleep(Duration::from_millis(500)).await;
    let polls_before = rig.stats().polls;
    // The operator presses the microphone PTT. Nobody asks the actor about PTT; it must find out.
    control.front_panel(PanelChange::Ptt(true));
    let seen = timeout(Duration::from_secs(3), async {
        loop {
            if let Some(event) = events.next().await {
                if event.source == Source::Poll
                    && event
                        .changes
                        .iter()
                        .any(|c| c.new == Observation::Ptt(true))
                {
                    return true;
                }
            } else {
                return false;
            }
        }
    })
    .await;
    stop.store(true, Ordering::Relaxed);
    for flooder in flooders {
        let _ = flooder.await;
    }
    assert_eq!(
        seen,
        Ok(true),
        "the keyed radio was never noticed under load"
    );
    let stats = rig.stats();
    assert!(stats.reserved_polls >= 1, "{stats:?}");
    assert!(stats.polls > polls_before);
    assert_eq!(
        rig.cached(Field::Ptt).map(|c| c.value),
        Some(Observation::Ptt(true))
    );
}

fn alarms(events: &[RigEvent]) -> Vec<(Source, Alarm)> {
    events
        .iter()
        .flat_map(|e| {
            e.changes.iter().filter_map(move |c| match c.new {
                Observation::Alarm(alarm) => Some((e.source, alarm)),
                _ => None,
            })
        })
        .collect()
}

async fn drain(events: &mut EventStream) -> Vec<RigEvent> {
    let mut seen = Vec::new();
    while let Ok(Some(event)) = timeout(Duration::from_millis(10), events.next()).await {
        seen.push(event);
    }
    seen
}

// Tests [@ANCHOR: radioctl_core_actor_serve]
// Tests [@ANCHOR: radioctl_core_actor_alarm]
#[tokio::test(start_paused = true)]
async fn an_unkey_the_rig_does_not_answer_is_reported_as_an_alarm() {
    let (rig, control) = civ_with_timing(Duration::from_secs(1), 0);
    let mut events = rig.events();
    rig.open_transmit_gate(permit());
    rig.set_ptt(true).await.expect("keyed");
    drain(&mut events).await;
    // The radio stops answering; the caller's unkey times out and, as `RigService::disconnect`
    // and `Splitter::cleanup` do, the caller drops the result.
    control.set_reply_delay(Duration::from_secs(5));
    let _ = rig
        .execute_from(Op::SetPtt(false), Duration::from_secs(2), Source::Splitter)
        .await;
    assert_eq!(rig.stats().unkey_failures, 1);
    assert_eq!(
        alarms(&drain(&mut events).await),
        vec![(Source::Splitter, Alarm::UnkeyFailed)]
    );
}

// Tests [@ANCHOR: radioctl_core_actor_run_command]
// Tests [@ANCHOR: radioctl_core_actor_spacing]
#[tokio::test(start_paused = true)]
async fn an_emergency_unkey_does_not_wait_for_inter_command_spacing() {
    let mut desc = RigDescription::ic7300();
    desc.timing.inter_command = Duration::from_secs(10);
    let rig_model = VirtualRig::from_description(&desc).expect("rig");
    let (stream, control) = spawn_virtual_rig(&Handle::current(), rig_model, Duration::ZERO);
    let rig = spawn_current(CivEngine::new(desc).expect("engine"), stream, quiet()).expect("spawn");
    rig.open_transmit_gate(permit());
    rig.set_ptt(true).await.expect("keyed");
    let started = Instant::now();
    rig.set_ptt(false).await.expect("unkeyed");
    assert!(!control.state().ptt);
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "the unkey waited {:?} for inter-command spacing",
        started.elapsed()
    );
    // An ordinary command still keeps the gap.
    let started = Instant::now();
    rig.execute_within(Op::GetFrequency(Vfo::A), Duration::from_secs(30))
        .await
        .expect("read");
    assert!(started.elapsed() >= Duration::from_secs(9));
}

// Tests [@ANCHOR: radioctl_core_actor_note_unsolicited]
// Tests [@ANCHOR: radioctl_core_actor_absorb]
#[tokio::test(start_paused = true)]
async fn a_late_reply_on_a_polled_rig_is_not_taken_for_transceive() {
    let mut desc = RigDescription::ts2000();
    desc.timing.timeout = Duration::from_secs(1);
    desc.timing.retries = 0;
    desc.capabilities = CapabilitySet::of(&[
        Capability::ReadFrequency,
        Capability::SetFrequency,
        Capability::ReadMode,
        Capability::SetMode,
        Capability::SetPtt,
        Capability::ReadSignal,
    ]);
    let rig_model = VirtualRig::from_description(&desc).expect("rig");
    let (stream, control) = spawn_virtual_rig(&Handle::current(), rig_model, Duration::ZERO);
    let rig =
        spawn_current(KenwoodEngine::new(desc).expect("engine"), stream, quiet()).expect("spawn");
    // One answer arrives after its command was given up.
    control.set_reply_delay(Duration::from_secs(3));
    let failed = rig
        .execute_within(Op::GetFrequency(Vfo::A), Duration::from_secs(2))
        .await
        .expect_err("timed out");
    assert_eq!(failed.kind, ErrorKind::Timeout);
    sleep(Duration::from_secs(4)).await;
    let stats = rig.stats();
    assert_eq!(
        stats.late_replies, 1,
        "the late answer is counted as a late reply"
    );
    assert_eq!(
        stats.unsolicited, 0,
        "a rig without Transceive never reports on its own: nothing may slow its polling"
    );
}

// Tests [@ANCHOR: radioctl_core_actor_link_down]
#[tokio::test(start_paused = true)]
async fn link_loss_while_keyed_is_a_visible_alarm() {
    let (near, far) = tokio::io::duplex(64);
    let rig = spawn_current(
        CivEngine::new(RigDescription::ic7300()).expect("engine"),
        near,
        quiet(),
    )
    .expect("spawn");
    let mut events = rig.events();
    drop(far);
    sleep(Duration::from_millis(10)).await;
    assert!(!rig.link_up());
    assert_eq!(
        alarms(&drain(&mut events).await),
        vec![(Source::Api, Alarm::LinkDown)]
    );
    // A second failure of the same dead link is not reported again.
    let _ = rig.mode(Freshness::Fresh).await;
    assert!(alarms(&drain(&mut events).await).is_empty());
}

async fn stream_next(stream: &mut EventStream) -> Option<RigEvent> {
    std::future::poll_fn(|cx| Pin::new(&mut *stream).poll_next(cx)).await
}

fn freq_of(event: &RigEvent) -> Option<Hz> {
    match event.changes.first().map(|c| c.new) {
        Some(Observation::Frequency(_, hz)) => Some(hz),
        _ => None,
    }
}

// Tests [@ANCHOR: radioctl_core_event_stream_poll_next]
// Tests [@ANCHOR: radioctl_core_event_stream_poll]
// Tests [@ANCHOR: radioctl_core_handle_events]
#[tokio::test(start_paused = true)]
async fn stream_trait_delivers_events_in_order_with_old_and_new() {
    let (rig, _control) = civ(quiet(), Duration::ZERO);
    let mut stream = rig.events();
    for step in 0..5u64 {
        rig.set_frequency(Vfo::A, Hz(7_000_000 + step))
            .await
            .expect("set");
    }
    let mut seen = Vec::new();
    while let Ok(Some(event)) = timeout(Duration::from_millis(10), stream_next(&mut stream)).await {
        assert_eq!(event.source, Source::Api);
        seen.extend(freq_of(&event));
    }
    let expected: Vec<Hz> = (0..5u64).map(|s| Hz(7_000_000 + s)).collect();
    assert_eq!(seen, expected);
    assert_eq!(stream.lagged(), 0);
}

// Tests [@ANCHOR: radioctl_core_event_stream_next]
// Tests [@ANCHOR: radioctl_core_event_stream_poll]
#[tokio::test(start_paused = true)]
async fn the_stream_ends_when_the_rig_stops() {
    let (rig, _control) = civ(quiet(), Duration::ZERO);
    let mut stream = rig.events();
    let mut scope = rig.scope_frames();
    drop(rig);
    let ended = timeout(Duration::from_secs(5), stream.next())
        .await
        .expect("the stream ends instead of hanging");
    assert!(ended.is_none());
    assert!(stream_next(&mut stream).await.is_none(), "stays ended");
    let ended = timeout(Duration::from_secs(5), scope.next())
        .await
        .expect("the scope stream ends too");
    assert!(ended.is_none());
}

// Tests [@ANCHOR: radioctl_core_event_stream_poll]
// Tests [@ANCHOR: radioctl_core_handle_events]
#[tokio::test(start_paused = true)]
async fn many_subscribers_each_get_every_event() {
    let (rig, _control) = civ(quiet(), Duration::ZERO);
    let mut tasks = Vec::new();
    for _ in 0..8 {
        let mut stream = rig.events();
        tasks.push(tokio::spawn(async move {
            let mut seen = Vec::new();
            while seen.len() < 5 {
                match stream.next().await {
                    Some(event) => seen.extend(freq_of(&event)),
                    None => break,
                }
            }
            seen
        }));
    }
    for step in 0..5u64 {
        rig.set_frequency(Vfo::A, Hz(3_500_000 + step))
            .await
            .expect("set");
    }
    let expected: Vec<Hz> = (0..5u64).map(|s| Hz(3_500_000 + s)).collect();
    for task in tasks {
        assert_eq!(task.await.expect("subscriber task"), expected);
    }
}

// Tests [@ANCHOR: radioctl_core_handle_events]
// Tests [@ANCHOR: radioctl_core_event_stream_lagged]
// Tests [@ANCHOR: radioctl_core_handle_stats]
#[tokio::test(start_paused = true)]
async fn a_stalled_subscriber_never_delays_ptt_off_and_counts_its_lag() {
    let options = RigOptions {
        stream_capacity: 1,
        ..quiet()
    };
    let (rig, control) = civ(options, Duration::ZERO);
    // A subscriber task that takes one event and then never reads again.
    let mut stalled = rig.events();
    let stall = tokio::spawn(async move {
        let _ = stalled.next().await;
        std::future::pending::<()>().await;
    });
    let kept = rig.events();
    rig.open_transmit_gate(permit());
    rig.set_ptt(true).await.expect("keyed");
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
    // A subscriber that did not read skips to the newest events and counts what it missed.
    let mut kept = kept;
    assert!(kept.next().await.is_some());
    assert!(kept.lagged() > 0);
    assert!(rig.stats().stream_lagged >= kept.lagged());
    stall.abort();
}

// Tests [@ANCHOR: radioctl_core_handle_events]
// Tests [@ANCHOR: radioctl_core_event_stream_poll]
#[tokio::test(start_paused = true)]
async fn a_panicking_subscriber_task_leaves_the_rig_and_other_subscribers_alone() {
    let (rig, _control) = civ(quiet(), Duration::ZERO);
    let mut bad = rig.events();
    let bad_task = tokio::spawn(async move {
        let _ = bad.next().await;
        panic!("application subscriber bug");
    });
    let mut good = rig.events();
    rig.set_frequency(Vfo::A, Hz(7_000_000))
        .await
        .expect("first change");
    assert!(bad_task.await.expect_err("the task panicked").is_panic());
    rig.set_frequency(Vfo::A, Hz(7_100_000))
        .await
        .expect("second change");
    let first = good.next().await.expect("first event");
    let second = good.next().await.expect("second event");
    assert_eq!(freq_of(&first), Some(Hz(7_000_000)));
    assert_eq!(freq_of(&second), Some(Hz(7_100_000)));
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

fn civ_with_timing(timeout: Duration, retries: u8) -> (RigHandle, VirtualRigControl) {
    let mut desc = RigDescription::ic7300();
    desc.timing.timeout = timeout;
    desc.timing.retries = retries;
    let rig = VirtualRig::from_description(&desc).expect("rig");
    let (stream, control) = spawn_virtual_rig(&Handle::current(), rig, Duration::ZERO);
    let handle =
        spawn_current(CivEngine::new(desc).expect("engine"), stream, quiet()).expect("spawn");
    (handle, control)
}

// Tests [@ANCHOR: radioctl_core_actor_unkey_fail_closed]
// Tests [@ANCHOR: radioctl_core_actor_run_command]
#[tokio::test(start_paused = true)]
async fn a_key_the_rig_never_answers_is_unkeyed_fail_closed() {
    let (rig, control) = civ_with_timing(Duration::from_secs(1), 0);
    rig.open_transmit_gate(permit());
    // The radio keys on the command but its answer is lost for three seconds.
    control.set_reply_delay(Duration::from_secs(3));
    let failed = rig
        .execute_within(Op::SetPtt(true), Duration::from_secs(2))
        .await
        .expect_err("no answer within the deadline");
    assert_eq!(failed.kind, ErrorKind::Timeout);
    assert!(
        !control.state().ptt,
        "a key whose outcome is unknown is followed by an unkey, never left on the air"
    );
    assert!(failed
        .transcript_tail
        .iter()
        .any(|line| line.starts_with("FAIL CLOSED unkey")));
}

// Tests [@ANCHOR: radioctl_core_actor_unkey_on_stop]
// Tests [@ANCHOR: radioctl_core_actor_run]
#[tokio::test(start_paused = true)]
async fn dropping_the_last_handle_unkeys_a_keyed_rig() {
    let (rig, control) = civ(quiet(), Duration::ZERO);
    rig.open_transmit_gate(permit());
    rig.set_ptt(true).await.expect("keyed");
    assert!(control.state().ptt);
    drop(rig);
    sleep(Duration::from_millis(50)).await;
    assert!(
        !control.state().ptt,
        "the actor unkeys the rig before it ends"
    );
}

// Tests [@ANCHOR: radioctl_core_deadline_after]
// Tests [@ANCHOR: radioctl_core_actor_spacing]
// Tests [@ANCHOR: radioctl_core_actor_poll_once]
#[tokio::test(start_paused = true)]
async fn absurd_timeouts_saturate_instead_of_panicking() {
    let (rig, _control) = civ(quiet(), Duration::ZERO);
    let read = rig
        .execute_within(Op::GetMode, Duration::MAX)
        .await
        .expect("a read with no practical deadline still works");
    assert_eq!(read, Some(Observation::Mode(Mode::Usb)));

    // A rig file with an absurd per-command timeout and command spacing, polled.
    let mut desc = RigDescription::ic7300();
    desc.timing.timeout = Duration::MAX;
    desc.timing.inter_command = Duration::MAX;
    let virtual_rig = VirtualRig::from_description(&desc).expect("rig");
    let (stream, control) = spawn_virtual_rig(&Handle::current(), virtual_rig, Duration::ZERO);
    let polled = spawn_current(
        CivEngine::new(desc).expect("engine"),
        stream,
        RigOptions::default(),
    )
    .expect("spawn");
    sleep(Duration::from_millis(500)).await;
    assert!(polled.stats().polls > 0, "polling ran");
    assert!(control.bytes_received() > 0);
    let first = polled
        .frequency(Vfo::A, Freshness::Cached)
        .await
        .expect("cached frequency");
    assert_eq!(first.value, Hz(14_074_000));
}

// Tests [@ANCHOR: radioctl_core_actor_watchdog_due]
// Tests [@ANCHOR: radioctl_core_actor_watchdog_fire]
// Tests [@ANCHOR: radioctl_core_actor_arm_watchdog]
// Tests [@ANCHOR: radioctl_core_actor_alarm]
// Tests [@ANCHOR: radioctl_core_actor_emit]
#[tokio::test(start_paused = true)]
async fn transmitting_past_the_limit_is_unkeyed_by_the_actor_and_reported() {
    let options = RigOptions {
        max_transmit: Duration::from_secs(5),
        ..quiet()
    };
    let (rig, control) = civ(options, Duration::ZERO);
    let mut events = rig.events();
    rig.open_transmit_gate(permit());
    rig.set_ptt(true).await.expect("keyed");
    sleep(Duration::from_secs(4)).await;
    assert!(control.state().ptt, "within the limit the rig stays keyed");
    sleep(Duration::from_millis(1100)).await;
    assert!(
        !control.state().ptt,
        "past the limit the actor unkeys the rig on its own"
    );
    assert_eq!(rig.stats().watchdog_unkeys, 1);
    assert_eq!(rig.stats().unkey_failures, 0);
    let mut seen = Vec::new();
    while let Ok(Some(event)) = timeout(Duration::from_millis(10), events.next()).await {
        seen.push(event);
    }
    assert!(seen.iter().any(|e| e.source == Source::Watchdog
        && e.changes
            .iter()
            .any(|c| c.new == Observation::Alarm(Alarm::TransmitTimeout))));
    assert!(seen.iter().any(|e| e.source == Source::Watchdog
        && e.changes.iter().any(|c| c.new == Observation::Ptt(false))));
    // Keying again starts a fresh limit.
    rig.set_ptt(true).await.expect("keyed again");
    sleep(Duration::from_secs(4)).await;
    assert!(control.state().ptt);
    rig.set_ptt(false).await.expect("unkeyed by the caller");
}

// Tests [@ANCHOR: radioctl_core_actor_watchdog_due]
// Tests [@ANCHOR: radioctl_core_actor_await_reply]
#[tokio::test(start_paused = true)]
async fn the_watchdog_preempts_a_slow_read_to_unkey_in_time() {
    let options = RigOptions {
        max_transmit: Duration::from_secs(5),
        ..quiet()
    };
    let (rig, control) = civ(options, Duration::ZERO);
    rig.open_transmit_gate(permit());
    rig.set_ptt(true).await.expect("keyed");
    // The radio now takes a minute to answer a read that was started while keyed.
    control.set_reply_delay(Duration::from_secs(60));
    let read = {
        let rig = rig.clone();
        tokio::spawn(async move {
            rig.execute_within(Op::GetFrequency(Vfo::A), Duration::from_secs(120))
                .await
        })
    };
    sleep(Duration::from_millis(10)).await;
    control.set_reply_delay(Duration::ZERO);
    sleep(Duration::from_secs(6)).await;
    assert!(
        !control.state().ptt,
        "the slow read did not hold the actor past the transmit limit"
    );
    let cancelled = read.await.expect("join").expect_err("abandoned");
    assert_eq!(cancelled.kind, ErrorKind::Cancelled);
}

// Tests [@ANCHOR: radioctl_core_handle_heartbeat]
// Tests [@ANCHOR: radioctl_core_actor_watchdog_due]
#[tokio::test(start_paused = true)]
async fn a_lapsed_heartbeat_unkeys_only_while_transmitting() {
    let options = RigOptions {
        heartbeat: Some(Duration::from_secs(1)),
        ..quiet()
    };
    let (rig, control) = civ(options, Duration::ZERO);
    rig.open_transmit_gate(permit());
    // Receiving without a heartbeat is fine.
    sleep(Duration::from_secs(10)).await;
    assert_eq!(rig.stats().watchdog_unkeys, 0);
    rig.set_ptt(true).await.expect("keyed");
    for _ in 0..5 {
        sleep(Duration::from_millis(600)).await;
        rig.heartbeat();
    }
    assert!(control.state().ptt, "a beating caller keeps the rig keyed");
    sleep(Duration::from_millis(1100)).await;
    assert!(!control.state().ptt, "a lapsed heartbeat unkeys the rig");
    assert_eq!(rig.stats().watchdog_unkeys, 1);
    let alarm = rig.cached(Field::Alarm);
    assert!(alarm.is_none(), "alarms are not cached");
}

// Tests [@ANCHOR: radioctl_core_actor_unkey_fail_closed]
// Tests [@ANCHOR: radioctl_core_actor_alarm]
#[tokio::test(start_paused = true)]
async fn an_unkey_the_rig_never_answers_is_retried_then_reported() {
    let (rig, control) = civ_with_timing(Duration::from_secs(1), 0);
    let mut events = rig.events();
    rig.open_transmit_gate(permit());
    rig.set_ptt(true).await.expect("keyed");
    let before = control.bytes_received();
    // The radio goes deaf: nothing it is sent is answered.
    control.set_reply_delay(Duration::from_secs(3600));
    sleep(Duration::from_secs(181)).await;
    sleep(Duration::from_secs(5)).await;
    let stats = rig.stats();
    assert_eq!(stats.watchdog_unkeys, 1);
    assert_eq!(
        stats.unkey_failures, 1,
        "the failure is counted, not swallowed"
    );
    assert!(
        control.bytes_received() >= before + 3 * 8,
        "the unkey was written three times"
    );
    let mut alarms = Vec::new();
    while let Ok(Some(event)) = timeout(Duration::from_millis(10), events.next()).await {
        alarms.extend(event.changes.into_iter().map(|c| c.new));
    }
    assert!(alarms.contains(&Observation::Alarm(Alarm::TransmitTimeout)));
    assert!(alarms.contains(&Observation::Alarm(Alarm::UnkeyFailed)));
}

// Tests [@ANCHOR: radioctl_core_handle_submit]
#[tokio::test(start_paused = true)]
async fn an_unkey_is_never_refused_when_the_emergency_queue_is_full() {
    let (rig, control) = civ(quiet(), Duration::ZERO);
    rig.open_transmit_gate(permit());
    rig.set_ptt(true).await.expect("keyed");
    control.set_reply_delay(Duration::from_secs(2));
    let mut unkeys = Vec::new();
    for _ in 0..40 {
        let rig = rig.clone();
        unkeys.push(tokio::spawn(async move {
            rig.execute_within(Op::SetPtt(false), Duration::from_secs(60))
                .await
        }));
    }
    for unkey in unkeys {
        unkey
            .await
            .expect("join")
            .expect("every unkey is accepted and answered");
    }
    assert!(!control.state().ptt);
    assert!(
        rig.stats().unkeys_coalesced > 0,
        "the overflow rode on a queued unkey"
    );
}

// Tests [@ANCHOR: radioctl_core_actor_link_status_changed]
// Tests [@ANCHOR: radioctl_core_link_changed]
// Tests [@ANCHOR: radioctl_core_handle_link_up]
#[tokio::test(start_paused = true)]
async fn a_port_loss_takes_the_link_down_and_a_reopen_unkeys_before_serving_the_queue() {
    use radioctl_core::serial::LinkStatus;
    let (status_tx, status_rx) = tokio::sync::watch::channel(LinkStatus::Connected {
        port: std::path::PathBuf::from("/dev/ttyUSB0"),
        generation: 1,
    });
    let options = RigOptions {
        link_watch: Some(status_rx),
        ..quiet()
    };
    let (rig, control) = civ(options, Duration::from_millis(5));
    let mut events = rig.events();
    rig.open_transmit_gate(permit());
    rig.set_ptt(true).await.expect("key");
    assert!(control.state().ptt);
    assert!(rig.link_up());

    // The USB adapter is unplugged: the supervisor reconnects underneath and drops writes
    // meanwhile. The actor must know, refuse commands with LinkDown (not a misleading
    // Timeout) and raise the alarm.
    status_tx
        .send(LinkStatus::Reconnecting {
            attempt: 1,
            last_error: "no such device".into(),
        })
        .expect("send");
    sleep(Duration::from_millis(10)).await;
    assert!(!rig.link_up());
    assert_eq!(
        rig.frequency(Vfo::A, Freshness::Fresh)
            .await
            .expect_err("down")
            .kind,
        ErrorKind::LinkDown
    );
    let mut alarms = Vec::new();
    while let Ok(Some(event)) = timeout(Duration::from_millis(50), events.next()).await {
        for change in event.changes {
            if let Observation::Alarm(alarm) = change.new {
                alarms.push(alarm);
            }
        }
    }
    assert_eq!(alarms, vec![Alarm::LinkDown]);

    // A read arrives in the same instant as the reopen; the actor must run the unkey pass
    // before serving it. With the rig answering in 5 ms (paused time), the unkey's round trip
    // ends at +5 ms and the read's, written only after it, at +10 ms.
    status_tx
        .send(LinkStatus::Connected {
            port: std::path::PathBuf::from("/dev/ttyUSB0"),
            generation: 2,
        })
        .expect("send");
    let reader = {
        let rig = rig.clone();
        tokio::spawn(async move { rig.frequency(Vfo::A, Freshness::Fresh).await })
    };
    sleep(Duration::from_millis(7)).await;
    assert!(rig.link_up());
    assert!(!control.state().ptt, "the reopen must unkey the rig first");
    assert!(
        !reader.is_finished(),
        "the queued read must wait for the unkey pass"
    );
    sleep(Duration::from_millis(10)).await;
    reader
        .await
        .expect("task")
        .expect("the read is served after the unkey");
    assert_eq!(
        rig.cached(Field::Ptt).map(|c| c.value),
        Some(Observation::Ptt(false))
    );
    let mut alarms = Vec::new();
    while let Ok(Some(event)) = timeout(Duration::from_millis(50), events.next()).await {
        for change in event.changes {
            if let Observation::Alarm(alarm) = change.new {
                alarms.push(alarm);
            }
        }
    }
    assert_eq!(alarms, vec![Alarm::LinkRestored]);
}

// Timing policy under injected faults, on the virtual IC-7300 (nothing is keyed).

// Tests [@ANCHOR: radioctl_virtual_io_set_reply_jitter]
// Tests [@ANCHOR: radioctl_virtual_io_next_jitter]
// Tests [@ANCHOR: radioctl_core_actor_run_command]
#[tokio::test(start_paused = true)]
async fn jitter_inside_the_timeout_never_costs_a_command() {
    let (rig, control) = civ_with_timing(Duration::from_secs(1), 0);
    control.set_reply_delay(Duration::from_millis(200));
    control.set_reply_jitter(Duration::from_millis(500));
    for _ in 0..20 {
        let read = rig
            .frequency(Vfo::A, Freshness::Fresh)
            .await
            .expect("answered");
        assert_eq!(read.value, Hz(14_074_000));
    }
    let stats = rig.stats();
    assert_eq!(stats.commands_ok, 20);
    assert_eq!(stats.commands_failed, 0);
    assert_eq!(stats.late_replies, 0);
    // Jitter that can exceed the timeout costs commands but never answers: a late answer
    // is drained, the next command is answered with the right value.
    control.set_reply_jitter(Duration::from_millis(1500));
    let mut failed = 0;
    for _ in 0..20 {
        match rig.frequency(Vfo::A, Freshness::Fresh).await {
            Ok(read) => assert_eq!(read.value, Hz(14_074_000)),
            Err(e) => {
                assert_eq!(e.kind, ErrorKind::Timeout);
                failed += 1;
            }
        }
    }
    assert!(
        failed > 0,
        "the jitter reached past the timeout at least once"
    );
    assert_eq!(
        rig.stats().stale_replies,
        0,
        "no late answer was taken for a newer one"
    );
}

// Tests [@ANCHOR: radioctl_virtual_io_set_echo]
// Tests [@ANCHOR: radioctl_civ_engine_decode]
#[tokio::test(start_paused = true)]
async fn a_bus_echo_of_our_own_frames_is_skipped_not_misread() {
    let (rig, control) = civ_with_timing(Duration::from_secs(1), 0);
    control.set_echo(true);
    for _ in 0..5 {
        let read = rig
            .frequency(Vfo::A, Freshness::Fresh)
            .await
            .expect("answered");
        assert_eq!(read.value, Hz(14_074_000));
    }
    rig.set_frequency(Vfo::A, Hz(7_030_000)).await.expect("set");
    let read = rig
        .frequency(Vfo::A, Freshness::Fresh)
        .await
        .expect("answered");
    assert_eq!(read.value, Hz(7_030_000));
    let stats = rig.stats();
    assert_eq!(stats.commands_failed, 0);
    assert_eq!(stats.unrecognised, 0, "an echo is never junk");
    assert_eq!(stats.unsolicited, 0, "an echo is never a transceive report");
}

// Tests [@ANCHOR: radioctl_virtual_io_set_reply_chunk]
// Tests [@ANCHOR: radioctl_civ_parser_feed]
#[tokio::test(start_paused = true)]
async fn an_answer_split_across_reads_is_still_one_answer() {
    let (rig, control) = civ_with_timing(Duration::from_secs(1), 0);
    control.set_reply_chunk(1);
    for _ in 0..5 {
        let read = rig
            .frequency(Vfo::A, Freshness::Fresh)
            .await
            .expect("answered");
        assert_eq!(read.value, Hz(14_074_000));
    }
    control.set_reply_chunk(3);
    let mode = rig.mode(Freshness::Fresh).await.expect("answered");
    assert_eq!(mode.value, Mode::Usb);
    let stats = rig.stats();
    assert_eq!(stats.commands_failed, 0);
    assert_eq!(stats.unrecognised, 0);
}

// Tests [@ANCHOR: radioctl_virtual_io_truncate_replies]
// Tests [@ANCHOR: radioctl_core_actor_run_command]
#[tokio::test(start_paused = true)]
async fn a_frame_cut_short_times_out_and_the_retry_is_answered_cleanly() {
    let (rig, control) = civ_with_timing(Duration::from_millis(500), 1);
    control.truncate_replies(1);
    let started = Instant::now();
    let read = rig
        .frequency(Vfo::A, Freshness::Fresh)
        .await
        .expect("the retry is answered");
    assert_eq!(read.value, Hz(14_074_000));
    let waited = started.elapsed();
    assert!(
        waited >= Duration::from_millis(500) && waited < Duration::from_millis(1100),
        "one timeout, then the retry: {waited:?}"
    );
    // The partial frame left in the parser is restarted by the next preamble, not glued to
    // the retry's answer.
    let stats = rig.stats();
    assert_eq!(stats.commands_failed, 0);
    assert_eq!(stats.commands_ok, 1);
    // Without a retry the caller sees the timeout and nothing else is poisoned.
    let (rig, control) = civ_with_timing(Duration::from_millis(500), 0);
    control.truncate_replies(1);
    let err = rig
        .frequency(Vfo::A, Freshness::Fresh)
        .await
        .expect_err("times out");
    assert_eq!(err.kind, ErrorKind::Timeout);
    let read = rig
        .frequency(Vfo::A, Freshness::Fresh)
        .await
        .expect("answered");
    assert_eq!(read.value, Hz(14_074_000));
}

// Tests [@ANCHOR: radioctl_virtual_io_drop_replies]
// Tests [@ANCHOR: radioctl_core_actor_run_command]
#[tokio::test(start_paused = true)]
async fn a_radio_that_never_answers_fails_within_timeout_times_attempts() {
    let (rig, control) = civ_with_timing(Duration::from_millis(500), 2);
    control.drop_replies(3);
    let started = Instant::now();
    let err = rig
        .frequency(Vfo::A, Freshness::Fresh)
        .await
        .expect_err("times out");
    assert_eq!(err.kind, ErrorKind::Timeout);
    let waited = started.elapsed();
    assert!(
        waited >= Duration::from_millis(1500) && waited < Duration::from_millis(2100),
        "three attempts of 500 ms: {waited:?}"
    );
    assert_eq!(
        control.bytes_received(),
        6 * 3,
        "one frame per attempt, nothing more"
    );
    let read = rig
        .frequency(Vfo::A, Freshness::Fresh)
        .await
        .expect("answered again");
    assert_eq!(read.value, Hz(14_074_000));
}

// Tests [@ANCHOR: radioctl_virtual_io_front_panel]
// Tests [@ANCHOR: radioctl_core_actor_claim_reply]
#[tokio::test(start_paused = true)]
async fn a_transceive_frame_arriving_before_a_pending_answer_is_an_event_not_the_answer() {
    let (rig, control) = civ_with_timing(Duration::from_secs(2), 0);
    let mut events = rig.events();
    control.set_reply_delay(Duration::from_millis(600));
    let pending = {
        let rig = rig.clone();
        tokio::spawn(async move { rig.mode(Freshness::Fresh).await })
    };
    sleep(Duration::from_millis(100)).await;
    // The dial moves while the mode read is in flight: its unsolicited frame reaches the
    // engine before the mode answer does.
    control.front_panel(PanelChange::Frequency(Hz(7_030_000)));
    let mode = pending
        .await
        .expect("task")
        .expect("the mode read is answered");
    assert_eq!(mode.value, Mode::Usb);
    let seen = drain(&mut events).await;
    assert!(
        seen.iter().any(|e| e.source == Source::Transceive
            && e.changes
                .iter()
                .any(|c| c.new == Observation::Frequency(Vfo::A, Hz(7_030_000)))),
        "the knob is an event of its own: {seen:?}"
    );
    let stats = rig.stats();
    assert_eq!(stats.unsolicited, 1);
    assert_eq!(stats.commands_failed, 0);
    assert_eq!(stats.stale_replies, 0);
}
