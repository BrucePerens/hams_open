// SPDX-License-Identifier: AGPL-3.0-or-later
//! The SAT-Schneider DRT1 as the daemon uses it: a virtual DRT1 (its twin as the control lines
//! behind the real actor) served over rigctld's text protocol. The DRT1 is a receiver with no
//! byte stream, so a tune is a control-line sequence. No hardware is touched.
#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

mod common;

use common::{config_text, start, Client};

// Tests [@ANCHOR: radioctld_rigs_build_rig]
// Tests [@ANCHOR: radioctld_rigs_is_line_driven]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_virtual_drt1_is_tuned_by_control_lines_and_has_no_transmitter() {
    let text = config_text("", "drt1", "sat-schneider-drt1.toml", false, false);
    let daemon = start(&text).await;
    let mut client = Client::connect(&daemon).await;
    assert_eq!(client.ask("F 7074000").await, "RPRT 0\n");
    assert_eq!(client.ask("f").await, "7074000\n");
    assert_eq!(client.ask("F 14074000").await, "RPRT 0\n");
    assert_eq!(client.ask("f").await, "14074000\n");
    // Out of range (above 30 MHz) and no PTT: refused, nothing driven.
    assert_ne!(client.ask("F 145000000").await, "RPRT 0\n");
    assert_ne!(client.ask("T 1").await, "RPRT 0\n");
    assert_eq!(client.ask("f").await, "14074000\n");
    daemon.stop().await;
}
