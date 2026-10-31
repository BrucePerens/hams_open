// SPDX-License-Identifier: AGPL-3.0-or-later
//! Protocol transcripts against virtual rigs through the production path (configuration text,
//! description files, real TCP). The transcripts in `tests/transcripts/` follow the formats
//! observed from Hamlib 4.6.2's `rigctld`: `>` lines are requests, `<` lines the exact reply.
#![allow(clippy::expect_used, clippy::panic)]

mod common;

use common::{config_text, start, start_with, Client};
use radioctl_core::description::RigDescription;

/// Runs a transcript file: every `>` request, then the `<` lines that must come back.
async fn replay(daemon_text: &str, descriptions: Option<Vec<RigDescription>>, transcript: &str) {
    let daemon = match descriptions {
        Some(descriptions) => start_with(daemon_text, descriptions).await,
        None => start(daemon_text).await,
    };
    let mut client = Client::connect(&daemon).await;
    let mut request: Option<String> = None;
    let mut expected = String::new();
    let mut steps = 0;
    let finish = |request: &mut Option<String>, expected: &mut String| {
        request.take().map(|r| (r, std::mem::take(expected)))
    };
    let mut pending = Vec::new();
    for line in transcript.lines() {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(r) = line.strip_prefix("> ") {
            if let Some(step) = finish(&mut request, &mut expected) {
                pending.push(step);
            }
            request = Some(r.to_string());
        } else if let Some(l) = line.strip_prefix("< ") {
            expected.push_str(l);
            expected.push('\n');
        } else if line == "<" {
            expected.push('\n');
        } else {
            panic!("bad transcript line {line:?}");
        }
    }
    if let Some(step) = finish(&mut request, &mut expected) {
        pending.push(step);
    }
    for (request, expected) in pending {
        let got = client.ask(&request).await;
        assert_eq!(got, expected, "request {request:?}");
        steps += 1;
    }
    assert!(steps >= 15, "transcript too thin: {steps}");
    let config = daemon.rigs[0].service.config();
    assert_eq!(config.vfo_mode, daemon_text.contains("vfo_mode = true"));
    daemon.stop().await;
}

// Tests [@ANCHOR: radioctld_command_parse_line]
// Tests [@ANCHOR: radioctld_reply_render]
// Tests [@ANCHOR: radioctld_service_execute]
// Tests [@ANCHOR: radioctld_server_answer]
// Tests [@ANCHOR: radioctld_server_run_client]
// Tests [@ANCHOR: radioctld_server_accept_loop]
// Tests [@ANCHOR: radioctld_daemon_start]
// Tests [@ANCHOR: radioctld_daemon_start_one]
// Tests [@ANCHOR: radioctld_rigs_build_rig]
// Tests [@ANCHOR: radioctld_rigs_boxed_encode]
// Tests [@ANCHOR: radioctld_rigs_boxed_feed]
// Tests [@ANCHOR: radioctld_rigs_service_config]
// Tests [@ANCHOR: radioctld_server_write]
// Tests [@ANCHOR: radioctld_service_new]
// Tests [@ANCHOR: radioctld_service_config]
// Tests [@ANCHOR: radioctld_service_tx_state]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_virtual_kx3_transcript() {
    let text = config_text("", "kx3", "elecraft-kx3.toml", false, false);
    replay(&text, None, include_str!("transcripts/kx3.txt")).await;
}

// Tests [@ANCHOR: radioctld_service_execute]
// Tests [@ANCHOR: radioctld_service_split_vfo]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_virtual_ic7300_transcript() {
    let text = config_text("", "ic7300", "icom-ic-7300.toml", false, false);
    replay(&text, None, include_str!("transcripts/ic7300.txt")).await;
}

// Tests [@ANCHOR: radioctld_service_execute]
// Tests [@ANCHOR: radioctld_service_set_split_vfo]
// Tests [@ANCHOR: radioctld_service_split_vfo]
// Tests [@ANCHOR: radioctld_service_served_level]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_rig_without_a_second_vfo_split_filter_or_levels_says_so() {
    // The built-in IC-7300 description is the small one; the daemon takes descriptions from its
    // caller exactly as the binary passes the ones it read from files.
    let text = config_text("", "ic7300", "icom-ic-7300.toml", false, false);
    let descriptions = vec![RigDescription::ic7300()];
    replay(
        &text,
        Some(descriptions),
        include_str!("transcripts/ic7300_basic.txt"),
    )
    .await;
}

// Tests [@ANCHOR: radioctld_service_execute]
// Tests [@ANCHOR: radioctld_service_resolve_vfo]
// Tests [@ANCHOR: radioctld_reply_vfo_echo]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn vfo_mode_transcript() {
    let text = config_text("", "kx3", "elecraft-kx3.toml", false, true);
    replay(&text, None, include_str!("transcripts/kx3_vfo_mode.txt")).await;
}
