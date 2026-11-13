// SPDX-License-Identifier: AGPL-3.0-or-later
//! A large burst of short CI-V frames in one read must parse in linear time (found by the
//! all-rigs fuzz run: draining the front of the buffer per frame was quadratic).

use std::time::{Duration, Instant};

use radioctl_protocols::civ::{FrameParser, Parsed};

// Tests [@ANCHOR: radioctl_civ_parser_feed]
// Tests [@ANCHOR: radioctl_civ_parser_drop_leading_junk]
#[test]
fn civ_parser_handles_a_huge_burst_of_short_frames_quickly() {
    let frame = [0xFE, 0xFE, 0xE0, 0x94, 0x03, 0xFD];
    let burst: Vec<u8> = frame
        .iter()
        .copied()
        .cycle()
        .take(frame.len() * 300_000)
        .collect();
    let mut parser = FrameParser::default();
    let started = Instant::now();
    let parsed = parser.feed(&burst, Duration::from_secs(1));
    assert_eq!(parsed.len(), 300_000);
    assert!(parsed.iter().all(|p| matches!(p, Parsed::Frame(_))));
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "{:?}",
        started.elapsed()
    );
}
