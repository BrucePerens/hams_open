// SPDX-License-Identifier: AGPL-3.0-or-later
//! The scope operations of the core API on the Icom CI-V engine and the virtual IC-7300 and
//! IC-7610: settings round-trip, waveform sweeps arrive as typed frames, and the actor publishes
//! them on the scope stream. All virtual rigs; nothing here can transmit.
#![allow(clippy::expect_used, clippy::panic)]

use std::time::Duration;

use radioctl_core::capability::Capability;
use radioctl_core::description::RigDescription;
use radioctl_core::engine::{Inbound, ProtocolEngine, ReplyKind};
use radioctl_core::op::{Observation, Op};
use radioctl_core::rig::{spawn, RigOptions};
use radioctl_core::rigfile;
use radioctl_core::scope::ScopeAxis;
use radioctl_core::units::Hz;
use radioctl_protocols::civ::CivEngine;
use radioctl_protocols::io::spawn_virtual_rig;
use radioctl_protocols::kenwood::KenwoodEngine;
use radioctl_protocols::virtual_rig::VirtualRig;
use tokio::runtime::Handle;

const NOW: Duration = Duration::from_secs(10);

fn description(file: &str) -> RigDescription {
    let text = std::fs::read_to_string(format!(
        "{}/../../rigs/{file}.toml",
        env!("CARGO_MANIFEST_DIR")
    ))
    .expect("rig file");
    rigfile::parse(&text).expect("rig description").description
}

fn exchange(engine: &mut CivEngine, rig: &mut VirtualRig, op: Op) -> Vec<Inbound> {
    let request = engine.encode(&op).expect("encode");
    let answer = rig.feed(&request.bytes, NOW);
    match request.reply {
        ReplyKind::None => assert!(answer.is_empty(), "unexpected answer to {op:?}"),
        _ => assert!(!answer.is_empty(), "no answer to {op:?}"),
    }
    engine.feed(&answer, NOW)
}

fn observed(inbound: Vec<Inbound>) -> Observation {
    match inbound.as_slice() {
        [Inbound::Observation(observation)] => *observation,
        other => panic!("expected one observation, got {other:?}"),
    }
}

fn frames(engine: &mut CivEngine, rig: &mut VirtualRig) -> Vec<radioctl_core::scope::ScopeFrame> {
    engine
        .feed(&rig.scope_sweep(), NOW)
        .into_iter()
        .filter_map(|i| match i {
            Inbound::ScopeFrame(frame) => Some(frame),
            _ => None,
        })
        .collect()
}

// Tests [@ANCHOR: radioctl_civ_engine_encode_scope]
// Tests [@ANCHOR: radioctl_scope_axis_from_mode]
// Tests [@ANCHOR: radioctl_scope_mode_from_axis]
// Tests [@ANCHOR: radioctl_civ_engine_decode_scope]
// Tests [@ANCHOR: radioctl_scope_sim_try_handle]
// Tests [@ANCHOR: radioctl_scope_sim_sweep]
#[test]
fn scope_settings_round_trip_and_sweeps_follow_them_on_the_virtual_ic7300_and_ic7610() {
    for file in ["icom-ic-7300", "icom-ic-7610"] {
        let desc = description(file);
        assert!(desc.capabilities.has(Capability::Scope), "{file}");
        let spans = desc.scope.as_ref().expect("scope").spans_hz.clone();
        let (span, wide) = (spans[0], spans[spans.len() - 1]);
        let mut engine = CivEngine::new(desc.clone()).expect("engine");
        let mut rig = VirtualRig::from_description(&desc).expect("rig");

        // Output is off until asked for.
        assert_eq!(
            observed(exchange(&mut engine, &mut rig, Op::GetScopeOutput)),
            Observation::ScopeOutput(false)
        );
        assert!(frames(&mut engine, &mut rig).is_empty());
        assert_eq!(
            exchange(&mut engine, &mut rig, Op::SetScopeOutput(true)),
            vec![Inbound::Ack]
        );
        assert_eq!(
            observed(exchange(&mut engine, &mut rig, Op::GetScopeOutput)),
            Observation::ScopeOutput(true)
        );

        // Centred on the tuned frequency, at a span the menu offers.
        for op in [
            Op::SetScopeAxis(0, ScopeAxis::Center),
            Op::SetScopeSpan(0, Hz(wide)),
            Op::SetScopeSpan(0, Hz(span)),
            Op::SetScopeReference(0, -2000),
        ] {
            assert_eq!(exchange(&mut engine, &mut rig, op), vec![Inbound::Ack]);
        }
        assert_eq!(
            observed(exchange(&mut engine, &mut rig, Op::GetScopeAxis(0))),
            Observation::ScopeAxis(0, ScopeAxis::Center)
        );
        assert_eq!(
            observed(exchange(&mut engine, &mut rig, Op::GetScopeSpan(0))),
            Observation::ScopeSpan(0, Hz(span))
        );
        assert_eq!(
            observed(exchange(&mut engine, &mut rig, Op::GetScopeReference(0))),
            Observation::ScopeReference(0, -2000)
        );
        let tuned = rig.state().freq_a.0;
        let seen = frames(&mut engine, &mut rig);
        let [frame] = seen.as_slice() else {
            panic!("{file}: one frame, got {}", seen.len());
        };
        assert_eq!(frame.axis, ScopeAxis::Center);
        assert_eq!(frame.centre(), Hz(tuned), "{file}");
        assert_eq!(frame.span(), Hz(span), "{file}");
        assert_eq!(frame.receiver, 0);
        assert!(!frame.samples.is_empty());
        assert!(frame
            .samples
            .iter()
            .all(|s| u16::from(*s) <= frame.amplitude_max));

        // Fixed edges.
        for op in [
            Op::SetScopeAxis(0, ScopeAxis::Fixed),
            Op::SetScopeEdges(1, 1, Hz(14_000_000), Hz(14_350_000)),
        ] {
            assert_eq!(exchange(&mut engine, &mut rig, op), vec![Inbound::Ack]);
        }
        let seen = frames(&mut engine, &mut rig);
        let [fixed] = seen.as_slice() else {
            panic!("{file}: one fixed frame");
        };
        assert_eq!(fixed.axis, ScopeAxis::Fixed);
        assert_eq!((fixed.low, fixed.high), (Hz(14_000_000), Hz(14_350_000)));

        // Refused before the wire: a span the scope does not offer, a receiver that is not there.
        assert!(engine.encode(&Op::SetScopeSpan(0, Hz(12_345))).is_err());
        assert!(engine.encode(&Op::GetScopeSpan(2)).is_err());
        // Switching output off stops the sweeps.
        exchange(&mut engine, &mut rig, Op::SetScopeOutput(false));
        assert!(frames(&mut engine, &mut rig).is_empty());
    }
}

#[test]
fn a_rig_without_the_scope_capability_refuses_scope_operations() {
    let plain = description("icom-ic-9700");
    let mut engine = CivEngine::new(plain).expect("engine");
    // Whether or not the model has a scope description, it must name the capability.
    if !engine.description().capabilities.has(Capability::Scope) {
        assert!(engine.encode(&Op::GetScopeOutput).is_err());
    }
    let kenwood = description("kenwood-ts-590sg");
    let mut engine = KenwoodEngine::new(kenwood).expect("engine");
    assert!(engine.encode(&Op::SetScopeOutput(true)).is_err());
}

// Tests [@ANCHOR: radioctl_core_handle_scope_frames]
// Tests [@ANCHOR: radioctl_core_scope_stream_next]
// Tests [@ANCHOR: radioctl_virtual_io_scope_sweep]
#[tokio::test(start_paused = true)]
async fn the_actor_publishes_sweeps_on_the_scope_stream_and_counts_lag() {
    let desc = description("icom-ic-7300");
    let rig = VirtualRig::from_description(&desc).expect("rig");
    let (stream, control) = spawn_virtual_rig(&Handle::current(), rig, Duration::ZERO);
    let engine = CivEngine::new(desc).expect("engine");
    let mut options = RigOptions::default();
    options.poll.enabled = false;
    options.scope_capacity = 2;
    let handle = spawn(&Handle::current(), engine, stream, options).expect("spawn");
    let mut frames = handle.scope_frames();

    handle
        .execute(Op::SetScopeOutput(true))
        .await
        .expect("output on");
    assert_eq!(
        handle.execute(Op::GetScopeOutput).await.expect("read"),
        Some(Observation::ScopeOutput(true))
    );
    control.scope_sweep();
    let first = frames.next().await.expect("a frame");
    assert_eq!(first.seq, 1);
    assert_eq!(first.receiver, 0);
    assert_eq!(first.axis, ScopeAxis::Center);
    assert_eq!(first.centre(), control.state().freq_a);
    assert_eq!(handle.stats().scope_frames, 1);
    assert_eq!(handle.stats().unrecognised, 0);

    // A subscriber that does not read loses the oldest frames, and the loss is counted.
    for _ in 0..5 {
        control.scope_sweep();
    }
    tokio::time::sleep(Duration::from_millis(50)).await;
    let next = frames.next().await.expect("a frame after the lag");
    assert!(next.seq > 2, "skipped ahead, got seq {}", next.seq);
    assert!(handle.stats().scope_lagged >= 1);
    assert!(handle.transmit_permit().is_none());
}
