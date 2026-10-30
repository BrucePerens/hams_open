// SPDX-License-Identifier: AGPL-3.0-or-later
//! Property tests for the request parser and the response renderer (the parser is the attack
//! surface of a network server), plus a whole-pipeline fuzz: arbitrary lines through the real
//! service on a virtual rig whose transmit gate is closed must never reach the rig as a
//! transmission.
#![allow(clippy::expect_used, clippy::panic)]

mod common;

use common::{config_text, start};
use proptest::prelude::*;
use proptest::test_runner::{Config as ProptestConfig, TestRunner};
use radioctld::command::{parse_hz, parse_line, Line, LineBuffer, MAX_HZ};
use radioctld::reply::{render, Body, Header};
use radioctld::server::answer;

/// Lines that are mostly almost-valid commands: the parser's interesting region.
fn command_ish() -> impl Strategy<Value = String> {
    let word = prop_oneof![
        Just("f"),
        Just("F"),
        Just("m"),
        Just("M"),
        Just("t"),
        Just("T"),
        Just("s"),
        Just("S"),
        Just("b"),
        Just("w"),
        Just("l"),
        Just("L"),
        Just("u"),
        Just("U"),
        Just("j"),
        Just("J"),
        Just("\\send_morse"),
        Just("\\set_ptt"),
        Just("\\dump_state"),
        Just("\\chk_vfo"),
        Just("+T"),
        Just("+b"),
        Just("+\\set_ptt"),
        Just("\\set_split_freq_mode"),
        Just("q"),
        Just("VFOA"),
        Just("VFOB"),
        Just("currVFO"),
        Just("USB"),
        Just("PKTUSB"),
        Just("1"),
        Just("0"),
        Just("2"),
        Just("3"),
        Just("KEYSPD"),
        Just("AF"),
        Just("RIT"),
        Just("-5"),
        Just("14074000"),
        Just("14074000.000000"),
        Just("1e999"),
        Just("nan"),
        Just("CQ"),
    ];
    prop::collection::vec(word, 0..6).prop_map(|words| words.join(" "))
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(2000))]

    // Tests [@ANCHOR: radioctld_command_parse_line]
    // Tests [@ANCHOR: radioctld_command_build]
    // Tests [@ANCHOR: radioctld_command_resolve]
    #[test]
    fn the_parser_never_panics_and_always_answers_with_a_known_shape(line in ".{0,200}") {
        if let Some(parsed) = parse_line(&line) {
            match parsed.command {
                Ok(_) => prop_assert!(!parsed.name.is_empty()),
                Err(code) => prop_assert!(code == -1 || code == -4, "code {code}"),
            }
            prop_assert!(parsed.args.chars().all(|c| !c.is_control()));
        }
    }

    // Tests [@ANCHOR: radioctld_command_parse_hz]
    #[test]
    fn frequencies_are_bounded(text in "[0-9.+eE-]{0,24}") {
        if let Some(hz) = parse_hz(&text) {
            prop_assert!(hz <= MAX_HZ);
        }
    }

    // Tests [@ANCHOR: radioctld_command_linebuffer_push]
    // Tests [@ANCHOR: radioctld_command_linebuffer_has_partial]
    // Tests [@ANCHOR: radioctld_command_into_line]
    #[test]
    fn line_assembly_is_bounded_and_independent_of_how_the_bytes_are_cut(
        bytes in prop::collection::vec(any::<u8>(), 0..600),
        cuts in prop::collection::vec(1usize..40, 1..30),
        max in 1usize..80,
    ) {
        let mut whole = LineBuffer::new(max);
        let all = whole.push(&bytes);
        let mut pieces = LineBuffer::new(max);
        let mut got = Vec::new();
        let mut failed = false;
        let mut rest = bytes.as_slice();
        let mut cut_iter = cuts.iter().cycle();
        while !rest.is_empty() {
            let take = (*cut_iter.next().unwrap_or(&1)).min(rest.len());
            let (chunk, tail) = rest.split_at(take);
            rest = tail;
            match pieces.push(chunk) {
                Ok(lines) => got.extend(lines),
                Err(_) => { failed = true; break; }
            }
        }
        match all {
            Ok(lines) => { prop_assert!(!failed); prop_assert_eq!(lines, got); }
            Err(_) => prop_assert!(failed),
        }
        if let Ok(lines) = pieces.push(b"") {
            for line in lines {
                if let Line::Text(text) = line { prop_assert!(text.len() <= max); }
            }
        }
    }

    // Tests [@ANCHOR: radioctld_reply_render]
    // Tests [@ANCHOR: radioctld_reply_header_line]
    // Tests [@ANCHOR: radioctld_command_sanitise]
    #[test]
    fn rendered_replies_hold_no_control_characters_whatever_the_client_sent(line in ".{0,120}") {
        if let Some(parsed) = parse_line(&format!("+{line}")) {
            let header = Header {
                extended: true,
                name: parsed.name,
                vfo: parsed.vfo.as_ref().map(|(text, _)| text.clone()),
                args: parsed.args.clone(),
            };
            for outcome in [Ok(Body::Done), Err(-1), Ok(Body::Fields(vec![("X", "1".to_string())]))] {
                let text = render(&header, &outcome);
                prop_assert!(text.ends_with('\n'));
                prop_assert!(text.chars().all(|c| c == '\n' || !c.is_control()));
            }
        }
    }
}

// Tests [@ANCHOR: radioctld_server_answer]
// Tests [@ANCHOR: radioctld_service_check_transmit]
// Tests [@ANCHOR: radioctld_service_execute]
#[test]
fn fuzzed_lines_through_the_whole_service_never_transmit_through_a_closed_gate() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .expect("runtime");
    let handle = runtime.handle().clone();
    let (daemon, kept) = runtime.block_on(async {
        let daemon = start(&config_text("", "kx3", "elecraft-kx3.toml", false, false)).await;
        let control = daemon.rigs[0].virtual_control.clone().expect("virtual rig");
        (daemon, control)
    });
    let service = daemon.rigs[0].service.clone();
    let mut runner = TestRunner::new(ProptestConfig::with_cases(600));
    let any_line = prop_oneof![command_ish(), ".{0,80}".prop_map(|s| s)];
    runner
        .run(&any_line, |line| {
            let service = service.clone();
            let line = Line::Text(line);
            let (reply, _) = handle.block_on(async move { answer(&service, 77, &line).await });
            prop_assert!(reply.is_empty() || reply.ends_with('\n'));
            prop_assert!(reply
                .chars()
                .all(|c| c == '\n' || c == '\t' || !c.is_control()));
            prop_assert!(
                !kept.state().ptt,
                "the transmitter was keyed through a closed gate"
            );
            prop_assert!(kept.state().cw_sent.is_empty(), "Morse reached the rig");
            Ok(())
        })
        .expect("no fuzzed line transmits");
    runtime.block_on(daemon.stop());
}
