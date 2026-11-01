// SPDX-License-Identifier: AGPL-3.0-or-later
//! The TS-890S bandscope: the core scope operations against the virtual TS-890S built from
//! `rigs/kenwood-ts-890s.toml`, the LAN-only `##DD2` frame through the engine, and the places the
//! frame cannot be placed. Virtual rig only; nothing here transmits.
#![allow(clippy::expect_used, clippy::panic)]

use std::time::Duration;

use radioctl_core::description::RigDescription;
use radioctl_core::engine::{Inbound, ProtocolEngine};
use radioctl_core::error::ErrorKind;
use radioctl_core::op::{Observation, Op};
use radioctl_core::rigfile;
use radioctl_core::scope::{FilterScopeFrame, ScopeAction, ScopeAxis, ScopeFrame, ScopeSetting};
use radioctl_core::units::{Hz, Vfo};
use radioctl_protocols::kenwood::{Token, Tokenizer};
use radioctl_protocols::kenwood_ts890::Ts890Engine;
use radioctl_protocols::virtual_rig::VirtualRig;

const NOW: Duration = Duration::from_secs(3);

fn description() -> RigDescription {
    rigfile::parse(include_str!("../../../rigs/kenwood-ts-890s.toml"))
        .expect("loads")
        .description
}

fn pair() -> (Ts890Engine, VirtualRig) {
    let desc = description();
    (
        Ts890Engine::new(desc.clone()).expect("engine"),
        VirtualRig::from_description(&desc).expect("rig"),
    )
}

fn exchange(engine: &mut Ts890Engine, rig: &mut VirtualRig, op: Op) -> Vec<Inbound> {
    let request = engine.encode(&op).expect("encode");
    let answer = rig.feed(&request.bytes, NOW);
    engine.feed(&answer, NOW)
}

fn observed(inbound: Vec<Inbound>) -> Observation {
    match inbound.as_slice() {
        [Inbound::Observation(o)] => *o,
        other => panic!("expected one observation, got {other:?}"),
    }
}

fn frames(inbound: Vec<Inbound>) -> Vec<ScopeFrame> {
    inbound
        .into_iter()
        .filter_map(|i| match i {
            Inbound::ScopeFrame(frame) => Some(frame),
            _ => None,
        })
        .collect()
}

/// A `##DD2` frame whose 640 samples are all `value` (two hexadecimal digits).
fn dd2(value: &str) -> Vec<u8> {
    format!("##DD2{};", value.repeat(640)).into_bytes()
}

/// Turns the output on the way a client does and returns what the rig answered, fed to the engine.
fn start_output(engine: &mut Ts890Engine, rig: &mut VirtualRig) {
    exchange(engine, rig, Op::SetTransceive(true));
    let inbound = exchange(engine, rig, Op::SetScopeOutput(true));
    assert!(
        inbound.iter().any(|i| matches!(
            i,
            Inbound::Observation(Observation::ScopeSpan(0, Hz(50_000)))
        )),
        "{inbound:?}"
    );
}

// Tests [@ANCHOR: radioctl_ts890_engine_encode]
// Tests [@ANCHOR: radioctl_ts890_engine_decode]
// Tests [@ANCHOR: radioctl_ts890_decode_scope]
// Tests [@ANCHOR: radioctl_ts890_scope_axis_code]
// Tests [@ANCHOR: radioctl_ts890_scope_axis_of]
// Tests [@ANCHOR: radioctl_ts890_scope_span_code]
// Tests [@ANCHOR: radioctl_ts890_scope_reference_code]
// Tests [@ANCHOR: radioctl_virtual_ts890_scope_command]
// Tests [@ANCHOR: radioctl_virtual_ts890_scope_default]
// Tests [@ANCHOR: radioctl_virtual_ts890_command]
#[test]
fn scope_settings_round_trip_through_the_virtual_rig_and_bad_ones_never_reach_the_wire() {
    let (mut engine, mut rig) = pair();
    assert_eq!(
        observed(exchange(&mut engine, &mut rig, Op::GetScopeAxis(0))),
        Observation::ScopeAxis(0, ScopeAxis::Center)
    );
    for axis in [ScopeAxis::Fixed, ScopeAxis::ScrollFixed, ScopeAxis::Center] {
        exchange(&mut engine, &mut rig, Op::SetScopeAxis(0, axis));
        assert_eq!(
            observed(exchange(&mut engine, &mut rig, Op::GetScopeAxis(0))),
            Observation::ScopeAxis(0, axis)
        );
    }
    for hz in [5_000, 10_000, 25_000, 50_000, 100_000, 200_000, 500_000] {
        exchange(&mut engine, &mut rig, Op::SetScopeSpan(0, Hz(hz)));
        assert_eq!(
            observed(exchange(&mut engine, &mut rig, Op::GetScopeSpan(0))),
            Observation::ScopeSpan(0, Hz(hz))
        );
    }
    for hundredths in [-2000, -50, 0, 50, 1000] {
        exchange(&mut engine, &mut rig, Op::SetScopeReference(0, hundredths));
        assert_eq!(
            observed(exchange(&mut engine, &mut rig, Op::GetScopeReference(0))),
            Observation::ScopeReference(0, hundredths)
        );
    }
    let refused = [
        Op::SetScopeAxis(0, ScopeAxis::ScrollCenter),
        Op::SetScopeAxis(1, ScopeAxis::Center),
        Op::SetScopeSpan(0, Hz(7_000)),
        Op::SetScopeSpan(1, Hz(5_000)),
        Op::SetScopeReference(0, -2050),
        Op::SetScopeReference(0, 1050),
        Op::SetScopeReference(0, 25),
        Op::SetScopeEdges(0, 1, Hz(1), Hz(2)),
        Op::GetScopeSpan(1),
    ];
    for op in refused {
        let error = engine.encode(&op).expect_err("refused before the wire");
        assert_eq!(error.kind, ErrorKind::Unsupported, "{op:?}");
    }
    // The rig itself refuses what the guide has no digit for, and answers the other BS commands.
    for (command, answer) in [
        ("BS47;", "?;"),
        ("BS39;", "?;"),
        ("BSC061;", "?;"),
        ("BSC6;", "?;"),
        ("BSB0;", "?;"),
        ("BSZ;", "?;"),
        ("BS0;", "BS01;"),
        ("BS9;", "BS90;"),
        ("BS49;BS4;", "BS43;"),
        ("BS81;BS8;", "BS81;"),
        ("DD09;", "?;"),
        ("DD0;", "DD00;"),
    ] {
        assert_eq!(
            String::from_utf8(rig.feed(command.as_bytes(), NOW)).expect("text"),
            answer,
            "{command}"
        );
    }
}

// Tests [@ANCHOR: radioctl_ts890_engine_scope_start]
// Tests [@ANCHOR: radioctl_ts890_engine_scope_frame]
// Tests [@ANCHOR: radioctl_ts890_engine_note]
// Tests [@ANCHOR: radioctl_ts890_scope_samples]
// Tests [@ANCHOR: radioctl_ts890_engine_feed]
// Tests [@ANCHOR: radioctl_virtual_ts890_sweep]
// Tests [@ANCHOR: radioctl_kenwood_tokenizer_with_max_pending]
#[test]
fn a_lan_bandscope_frame_is_placed_from_what_the_engine_read_first() {
    let (mut engine, mut rig) = pair();
    assert!(rig.scope_sweep().is_empty(), "output starts off");
    exchange(&mut engine, &mut rig, Op::SetTransceive(true));
    assert!(rig.scope_sweep().is_empty(), "off until DD0 is set");
    start_output(&mut engine, &mut rig);
    let wire = rig.scope_sweep();
    assert_eq!((wire.len(), wire.starts_with(b"##DD2")), (1286, true));
    // The 1286-byte frame arrives in small pieces, as a TCP stream delivers it.
    let mut got = Vec::new();
    for piece in wire.chunks(100) {
        got.extend(frames(engine.feed(piece, NOW)));
    }
    let [frame] = got.as_slice() else {
        panic!("expected one frame, got {got:?}");
    };
    assert_eq!(frame.axis, ScopeAxis::Center);
    assert_eq!((frame.low, frame.high), (Hz(14_049_000), Hz(14_099_000)));
    assert_eq!((frame.receiver, frame.amplitude_max), (0, 140));
    assert_eq!(frame.samples.len(), 640);
    // Sweep 1, pixel 0 is synthetic value 1; the core's bottom is 0, the rig's is 8C.
    assert_eq!(frame.samples[0], 140 - 1);
    assert!(frame.samples.iter().all(|s| *s <= 140));
    assert_eq!(engine.scope_frames_dropped(), 0);
    // A different dial and span move the edges.
    exchange(
        &mut engine,
        &mut rig,
        Op::SetFrequency(radioctl_core::units::Vfo::A, Hz(7_074_000)),
    );
    exchange(&mut engine, &mut rig, Op::SetScopeSpan(0, Hz(10_000)));
    let moved = frames(engine.feed(&rig.scope_sweep(), NOW));
    assert_eq!(
        moved.first().map(|f| (f.low, f.high)),
        Some((Hz(7_069_000), Hz(7_079_000)))
    );
    // Output off: the rig stops sending.
    exchange(&mut engine, &mut rig, Op::SetScopeOutput(false));
    assert!(rig.scope_sweep().is_empty());
    assert_eq!(
        observed(exchange(&mut engine, &mut rig, Op::GetScopeOutput)),
        Observation::ScopeOutput(false)
    );
}

// Tests [@ANCHOR: radioctl_ts890_engine_scope_frames_dropped]
// Tests [@ANCHOR: radioctl_ts890_engine_scope_frame]
// Tests [@ANCHOR: radioctl_ts890_scope_samples]
#[test]
fn sweeps_that_cannot_be_placed_or_are_malformed_are_counted_and_not_published() {
    let (mut engine, mut rig) = pair();
    // Nothing known yet: no frequency, span or axis.
    assert!(frames(engine.feed(&dd2("40"), NOW)).is_empty());
    assert_eq!(engine.scope_frames_dropped(), 1);
    start_output(&mut engine, &mut rig);
    assert_eq!(frames(engine.feed(&dd2("40"), NOW)).len(), 1);
    // A value past 8C, a non-hexadecimal pair and a short frame are malformed.
    for bad in [dd2("8D"), dd2("GG"), b"##DD240;".to_vec()] {
        assert!(frames(engine.feed(&bad, NOW)).is_empty());
    }
    assert_eq!(engine.scope_frames_dropped(), 4);
    // Expand state not read yet (an engine that only saw the axis, span and frequency): dropped.
    let (mut blind, _rig) = pair();
    blind.feed(b"BS30;BS43;FA00014074000;FR0;", NOW);
    assert!(frames(blind.feed(&dd2("40"), NOW)).is_empty());
    assert_eq!(blind.scope_frames_dropped(), 1);
    // Expand on with a span whose expanded width the guide does not give: dropped.
    blind.feed(b"BSO1;", NOW);
    assert!(frames(blind.feed(&dd2("40"), NOW)).is_empty());
    assert_eq!(blind.scope_frames_dropped(), 2);
    // Auto scroll with its edges not read yet: dropped.
    let (mut scroll, _rig) = pair();
    scroll.feed(b"BS32;BS43;BSO0;FA00014074000;", NOW);
    assert!(frames(scroll.feed(&dd2("40"), NOW)).is_empty());
    assert_eq!(scroll.scope_frames_dropped(), 1);
    // Fixed mode with edges not yet read (a new engine that only saw the axis answer) is dropped.
    let (mut fresh, _rig) = pair();
    fresh.feed(b"BS31;", NOW);
    fresh.feed(b"FA00014074000;", NOW);
    assert!(frames(fresh.feed(&dd2("40"), NOW)).is_empty());
    assert_eq!(fresh.scope_frames_dropped(), 1);
}

fn filter_frames(inbound: Vec<Inbound>) -> Vec<FilterScopeFrame> {
    inbound
        .into_iter()
        .filter_map(|i| match i {
            Inbound::FilterScopeFrame(frame) => Some(frame),
            _ => None,
        })
        .collect()
}

// Tests [@ANCHOR: radioctl_ts890_engine_feed]
// Tests [@ANCHOR: radioctl_ts890_engine_filter_scope_frame]
// Tests [@ANCHOR: radioctl_ts890_scope_samples]
// Tests [@ANCHOR: radioctl_virtual_ts890_filter_sweep]
// Tests [@ANCHOR: radioctl_virtual_filter_scope_sweep]
#[test]
fn the_filter_scope_frame_is_published_flipped_and_a_bad_one_is_counted() {
    let (mut engine, mut rig) = pair();
    assert!(rig.filter_scope_sweep().is_empty(), "output starts off");
    let good = format!("##DD3{}32;", "00".repeat(212));
    let got = filter_frames(engine.feed(good.as_bytes(), NOW));
    let [frame] = got.as_slice() else {
        panic!("expected one frame, got {got:?}");
    };
    assert_eq!((frame.receiver, frame.amplitude_max), (0, 50));
    assert_eq!(frame.samples.len(), 213);
    // 00 is the strongest, so it is the top of the core's scale; 32 is the bottom.
    assert_eq!((frame.samples[0], frame.samples[212]), (50, 0));
    assert_eq!(engine.scope_frames_dropped(), 0);
    let bad = format!("##DD3{};", "33".repeat(213));
    assert!(engine.feed(bad.as_bytes(), NOW).is_empty());
    assert_eq!(engine.scope_frames_dropped(), 1);
    // The virtual rig's own frame is one the engine accepts.
    start_output(&mut engine, &mut rig);
    let wire = rig.filter_scope_sweep();
    assert_eq!((wire.len(), wire.starts_with(b"##DD3")), (432, true));
    assert_eq!(filter_frames(engine.feed(&wire, NOW)).len(), 1);
    assert_eq!(engine.scope_frames_dropped(), 1);
}

// Tests [@ANCHOR: radioctl_ts890_engine_encode]
// Tests [@ANCHOR: radioctl_ts890_decode_scope]
// Tests [@ANCHOR: radioctl_ts890_scope_setting_wire]
// Tests [@ANCHOR: radioctl_ts890_scope_setting_of]
// Tests [@ANCHOR: radioctl_virtual_ts890_scope_command]
#[test]
fn scope_settings_round_trip_and_codes_past_the_guide_never_reach_the_wire() {
    let (mut engine, mut rig) = pair();
    for (setting, max) in [
        (ScopeSetting::Display, 1),
        (ScopeSetting::Layout, 4),
        (ScopeSetting::Range, 3),
        (ScopeSetting::Pause, 1),
        (ScopeSetting::Marker, 1),
        (ScopeSetting::Attenuator, 3),
        (ScopeSetting::MaxHold, 1),
    ] {
        for code in 0..=max {
            exchange(&mut engine, &mut rig, Op::SetScopeSetting(0, setting, code));
            assert_eq!(
                observed(exchange(
                    &mut engine,
                    &mut rig,
                    Op::GetScopeSetting(0, setting)
                )),
                Observation::ScopeSetting(0, setting, code),
                "{setting:?}"
            );
        }
        for bad in [
            Op::SetScopeSetting(0, setting, max + 1),
            Op::SetScopeSetting(1, setting, 0),
            Op::GetScopeSetting(1, setting),
        ] {
            let error = engine.encode(&bad).expect_err("refused before the wire");
            assert_eq!(error.kind, ErrorKind::Unsupported, "{bad:?}");
        }
    }
    // An answer past the guide's range or of the wrong length is not an observation.
    for answer in [&b"BS92;"[..], b"BS61;BS7;"] {
        let seen = engine.feed(answer, NOW);
        assert!(
            seen.iter().all(|i| !matches!(
                i,
                Inbound::Observation(Observation::ScopeSetting(0, ScopeSetting::MaxHold, 2))
            )),
            "{seen:?}"
        );
    }
    assert!(matches!(
        engine.feed(b"BS12;BS5X;", NOW).last(),
        Some(Inbound::Unrecognised(_))
    ));
}

// Tests [@ANCHOR: radioctl_ts890_engine_encode]
// Tests [@ANCHOR: radioctl_ts890_scope_edges_of]
// Tests [@ANCHOR: radioctl_ts890_engine_scope_frame]
// Tests [@ANCHOR: radioctl_ts890_engine_feed]
// Tests [@ANCHOR: radioctl_virtual_ts890_edges_command]
#[test]
fn fixed_mode_edges_are_stored_read_back_and_place_the_sweep() {
    let (mut engine, mut rig) = pair();
    start_output(&mut engine, &mut rig);
    exchange(&mut engine, &mut rig, Op::SetScopeAxis(0, ScopeAxis::Fixed));
    // The default range is the rig's own: 14.000 to 14.350 MHz, the dial at 14.074 inside it.
    let got = frames(engine.feed(&rig.scope_sweep(), NOW));
    let [frame] = got.as_slice() else {
        panic!("expected one frame, got {got:?}");
    };
    assert_eq!(frame.axis, ScopeAxis::Fixed);
    assert_eq!((frame.low, frame.high), (Hz(14_000_000), Hz(14_350_000)));
    assert!(!frame.out_of_range);
    // Range 2 holds 7.100 to 7.200 MHz; selecting it moves the edges and the dial is outside.
    let set = Op::SetScopeEdges(2, 1, Hz(7_100_000), Hz(7_200_000));
    assert_eq!(
        String::from_utf8(engine.encode(&set).expect("encode").bytes).expect("text"),
        "BSM20710000007200000;BSM;"
    );
    exchange(&mut engine, &mut rig, set);
    exchange(
        &mut engine,
        &mut rig,
        Op::SetScopeSetting(0, ScopeSetting::Range, 2),
    );
    let got = frames(engine.feed(&rig.scope_sweep(), NOW));
    let [moved] = got.as_slice() else {
        panic!("expected one frame, got {got:?}");
    };
    assert_eq!((moved.low, moved.high), (Hz(7_100_000), Hz(7_200_000)));
    assert!(moved.out_of_range);
    // Bad edges never reach the wire.
    for bad in [
        Op::SetScopeEdges(0, 1, Hz(1), Hz(2)),
        Op::SetScopeEdges(4, 1, Hz(1), Hz(2)),
        Op::SetScopeEdges(1, 2, Hz(1), Hz(2)),
        Op::SetScopeEdges(1, 1, Hz(2), Hz(2)),
        Op::SetScopeEdges(1, 1, Hz(1), Hz(100_000_000)),
    ] {
        let error = engine.encode(&bad).expect_err("refused before the wire");
        assert_eq!(error.kind, ErrorKind::Unsupported, "{bad:?}");
    }
    // The rig refuses a set of range 0, edges that are not low to high, and a short command.
    for command in [
        "BSM00000000100000002;",
        "BSM10000200000001000;",
        "BSM1000010;",
    ] {
        let text = String::from_utf8(rig.feed(command.as_bytes(), NOW)).expect("text");
        assert_eq!(text, "?;", "{command}");
    }
    assert_eq!(
        String::from_utf8(rig.feed(b"BSM;", NOW)).expect("text"),
        "BSM00710000007200000;"
    );
    // Two 99999999 put the initial edges of that range back.
    rig.feed(b"BSM29999999999999999;", NOW);
    assert_eq!(
        String::from_utf8(rig.feed(b"BSM;", NOW)).expect("text"),
        "BSM00350000004000000;"
    );
}

// Tests [@ANCHOR: radioctl_ts890_engine_link_restored]
// Tests [@ANCHOR: radioctl_ts890_engine_encode]
#[test]
fn a_restored_link_asks_for_the_waveform_output_again() {
    let (mut engine, _rig) = pair();
    assert!(engine.link_restored().is_empty());
    engine.encode(&Op::SetTransceive(true)).expect("encode");
    engine.encode(&Op::SetScopeOutput(true)).expect("encode");
    let texts: Vec<String> = engine
        .link_restored()
        .into_iter()
        .map(|r| String::from_utf8(r.bytes).expect("text"))
        .collect();
    assert_eq!(texts, ["AI2;", "FA;FB;FR;BS3;BS4;BSO;BSM;DD02;"]);
    engine.encode(&Op::SetScopeOutput(false)).expect("encode");
    assert_eq!(engine.link_restored().len(), 1);
}

// Tests [@ANCHOR: radioctl_kenwood_tokenizer_with_max_pending]
#[test]
fn a_tokenizer_with_a_larger_limit_holds_a_long_command() {
    let body = "A".repeat(1000);
    let mut small = Tokenizer::default();
    let tokens = small.feed(body.as_bytes(), NOW);
    assert!(
        tokens.iter().all(|t| matches!(t, Token::Junk(_))) && !tokens.is_empty(),
        "{tokens:?}"
    );
    let mut large = Tokenizer::with_max_pending(2048);
    assert!(large.feed(body.as_bytes(), NOW).is_empty());
    assert_eq!(large.feed(b";", NOW), vec![Token::Command(body)]);
}

// Tests [@ANCHOR: radioctl_ts890_engine_encode]
// Tests [@ANCHOR: radioctl_ts890_scope_setting_wire]
// Tests [@ANCHOR: radioctl_ts890_scope_setting_of]
// Tests [@ANCHOR: radioctl_ts890_decode_scope]
// Tests [@ANCHOR: radioctl_virtual_ts890_scope_command]
#[test]
fn averaging_waterfall_speed_audio_scope_and_the_other_settings_follow_the_guide() {
    let (mut engine, mut rig) = pair();
    for (setting, min, max, letter) in [
        (ScopeSetting::Averaging, 0, 3, 'A'),
        (ScopeSetting::WaterfallSpeed, 1, 4, 'B'),
        (ScopeSetting::AudioAttenuator, 0, 3, 'G'),
        (ScopeSetting::AudioSpan, 0, 1, 'H'),
        (ScopeSetting::OscilloscopeLevel, 0, 3, 'I'),
        (ScopeSetting::OscilloscopeSweep, 0, 5, 'J'),
        (ScopeSetting::ShiftPosition, 0, 8, 'K'),
        (ScopeSetting::AudioPause, 0, 1, 'N'),
        (ScopeSetting::Expand, 0, 1, 'O'),
    ] {
        for code in min..=max {
            let op = Op::SetScopeSetting(0, setting, code);
            assert_eq!(
                String::from_utf8(engine.encode(&op).expect("encode").bytes).expect("text"),
                format!("BS{letter}{code};")
            );
            exchange(&mut engine, &mut rig, op);
            assert_eq!(
                observed(exchange(
                    &mut engine,
                    &mut rig,
                    Op::GetScopeSetting(0, setting)
                )),
                Observation::ScopeSetting(0, setting, code),
                "{setting:?}"
            );
        }
        for bad in [
            Op::SetScopeSetting(0, setting, max + 1),
            Op::SetScopeSetting(0, setting, min.wrapping_sub(1)),
        ] {
            let error = engine.encode(&bad).expect_err("refused before the wire");
            assert_eq!(error.kind, ErrorKind::Unsupported, "{bad:?}");
        }
    }
    // The waterfall speed starts at 1; 0 is not in the guide, on the wire or in an answer.
    assert!(engine
        .encode(&Op::SetScopeSetting(0, ScopeSetting::WaterfallSpeed, 0))
        .is_err());
    assert!(matches!(
        engine.feed(b"BSB0;", NOW).last(),
        Some(Inbound::Unrecognised(_))
    ));
    // The overflow state can be read but has no set form.
    assert_eq!(
        observed(exchange(
            &mut engine,
            &mut rig,
            Op::GetScopeSetting(0, ScopeSetting::Overflow)
        )),
        Observation::ScopeSetting(0, ScopeSetting::Overflow, 0)
    );
    let error = engine
        .encode(&Op::SetScopeSetting(0, ScopeSetting::Overflow, 0))
        .expect_err("no set form");
    assert_eq!(error.kind, ErrorKind::Unsupported);
    assert_eq!(rig.feed(b"BSL1;", NOW), b"?;");
}

// Tests [@ANCHOR: radioctl_ts890_engine_encode]
// Tests [@ANCHOR: radioctl_ts890_scope_action_wire]
// Tests [@ANCHOR: radioctl_ts890_engine_feed]
// Tests [@ANCHOR: radioctl_virtual_ts890_marker_shift]
// Tests [@ANCHOR: radioctl_virtual_ts890_current_edges]
// Tests [@ANCHOR: radioctl_virtual_ts890_scope_command]
#[test]
fn waterfall_clear_and_marker_shift_are_commands_and_marker_shift_is_not_for_center_mode() {
    let (mut engine, mut rig) = pair();
    let text = |engine: &mut Ts890Engine, op: Op| {
        String::from_utf8(engine.encode(&op).expect("encode").bytes).expect("text")
    };
    let clear = Op::ScopeAction(0, ScopeAction::ClearWaterfall);
    assert_eq!(text(&mut engine, clear.clone()), "BSD;");
    // With auto information off the rig is silent; with it on it confirms, and the engine
    // swallows the confirmation.
    assert!(exchange(&mut engine, &mut rig, clear.clone()).is_empty());
    exchange(&mut engine, &mut rig, Op::SetTransceive(true));
    assert_eq!(rig.feed(b"BSD;", NOW), b"BSD;");
    assert!(exchange(&mut engine, &mut rig, clear).is_empty());
    assert!(engine.feed(b"BSD;", NOW).is_empty());
    // Marker shift is invalid in center mode, so it is not sent once the engine knows the mode.
    let shift = Op::ScopeAction(0, ScopeAction::MarkerShift);
    exchange(
        &mut engine,
        &mut rig,
        Op::SetScopeAxis(0, ScopeAxis::Center),
    );
    assert_eq!(
        engine.encode(&shift).expect_err("refused").kind,
        ErrorKind::Unsupported
    );
    assert_eq!(rig.feed(b"BSE;", NOW), b"?;");
    // In fixed mode it re-centres the range on the receive frequency (14.074 MHz); the 350 kHz width stays.
    exchange(&mut engine, &mut rig, Op::SetScopeAxis(0, ScopeAxis::Fixed));
    assert_eq!(text(&mut engine, shift.clone()), "BSE;");
    exchange(&mut engine, &mut rig, shift);
    assert_eq!(
        String::from_utf8(rig.feed(b"BSM;", NOW)).expect("text"),
        "BSM01389900014249000;"
    );
    // Other receivers are not on this rig.
    let sub = Op::ScopeAction(1, ScopeAction::ClearWaterfall);
    assert_eq!(
        engine.encode(&sub).expect_err("refused").kind,
        ErrorKind::Unsupported
    );
}

// Tests [@ANCHOR: radioctl_ts890_engine_scope_frame]
// Tests [@ANCHOR: radioctl_ts890_expanded_span]
// Tests [@ANCHOR: radioctl_ts890_engine_note]
// Tests [@ANCHOR: radioctl_ts890_engine_scope_start]
#[test]
fn an_expanded_sweep_is_placed_over_the_wider_range_the_guide_gives() {
    let (mut engine, mut rig) = pair();
    start_output(&mut engine, &mut rig);
    exchange(&mut engine, &mut rig, Op::SetScopeSpan(0, Hz(100_000)));
    let width = |engine: &mut Ts890Engine| {
        let got = frames(engine.feed(&dd2("40"), NOW));
        let [frame] = got.as_slice() else {
            panic!("expected one frame, got {got:?}");
        };
        (frame.low, frame.high)
    };
    assert_eq!(width(&mut engine), (Hz(14_024_000), Hz(14_124_000)));
    exchange(
        &mut engine,
        &mut rig,
        Op::SetScopeSetting(0, ScopeSetting::Expand, 1),
    );
    assert_eq!(width(&mut engine), (Hz(13_924_000), Hz(14_224_000)));
    exchange(&mut engine, &mut rig, Op::SetScopeSpan(0, Hz(200_000)));
    assert_eq!(width(&mut engine), (Hz(13_874_000), Hz(14_274_000)));
    // 50 kHz expanded: the guide gives no width, so the sweep is not placed.
    exchange(&mut engine, &mut rig, Op::SetScopeSpan(0, Hz(50_000)));
    let before = engine.scope_frames_dropped();
    assert!(frames(engine.feed(&dd2("40"), NOW)).is_empty());
    assert_eq!(engine.scope_frames_dropped(), before + 1);
}

// Tests [@ANCHOR: radioctl_ts890_engine_scope_frame]
// Tests [@ANCHOR: radioctl_ts890_engine_encode]
// Tests [@ANCHOR: radioctl_virtual_ts890_current_edges]
#[test]
fn an_auto_scroll_sweep_is_placed_from_the_edges_read_and_they_are_read_after_each_set_frequency() {
    let (mut engine, mut rig) = pair();
    start_output(&mut engine, &mut rig);
    exchange(
        &mut engine,
        &mut rig,
        Op::SetScopeAxis(0, ScopeAxis::ScrollFixed),
    );
    let one = |engine: &mut Ts890Engine| {
        let got = frames(engine.feed(&dd2("40"), NOW));
        let [frame] = got.as_slice() else {
            panic!("expected one frame, got {got:?}");
        };
        frame.clone()
    };
    // The virtual rig scrolls in pages of the span's width: 14.050 to 14.100 MHz holds 14.074.
    let frame = one(&mut engine);
    assert_eq!(frame.axis, ScopeAxis::ScrollFixed);
    assert_eq!((frame.low, frame.high), (Hz(14_050_000), Hz(14_100_000)));
    assert!(!frame.out_of_range);
    // Setting a frequency in scroll mode reads the edges again, so the placement follows the page.
    let set = Op::SetFrequency(Vfo::A, Hz(14_120_000));
    assert_eq!(
        String::from_utf8(engine.encode(&set).expect("encode").bytes).expect("text"),
        "FA00014120000;BSM;"
    );
    exchange(&mut engine, &mut rig, set.clone());
    let frame = one(&mut engine);
    assert_eq!((frame.low, frame.high), (Hz(14_100_000), Hz(14_150_000)));
    assert!(!frame.out_of_range);
    // A dial turn on the rig is not followed by a read: the old edges stay and the sweep says the
    // receive frequency is outside them.
    engine.feed(b"FA00014300000;", NOW);
    let frame = one(&mut engine);
    assert_eq!((frame.low, frame.high), (Hz(14_100_000), Hz(14_150_000)));
    assert!(frame.out_of_range);
    // Outside scroll mode no read is added.
    exchange(
        &mut engine,
        &mut rig,
        Op::SetScopeAxis(0, ScopeAxis::Center),
    );
    assert_eq!(
        String::from_utf8(engine.encode(&set).expect("encode").bytes).expect("text"),
        "FA00014120000;"
    );
}
