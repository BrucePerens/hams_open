// SPDX-License-Identifier: AGPL-3.0-or-later
//! The bounds: loopback by default, the allow-list, the client limit, line length and the
//! timeouts. Each is a connection to a real listener started from configuration text.
#![allow(clippy::expect_used, clippy::panic)]

mod common;

use std::time::Duration;

use common::{config_text, start, Client};

// Tests [@ANCHOR: radioctld_server_serve]
// Tests [@ANCHOR: radioctld_server_local_addr]
// Tests [@ANCHOR: radioctld_server_stats]
// Tests [@ANCHOR: radioctl_d_server_server_handle]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_default_listener_is_on_loopback() {
    let daemon = start(&config_text("", "kx3", "elecraft-kx3.toml", false, false)).await;
    assert!(daemon.rigs[0].server.local_addr().ip().is_loopback());
    let mut client = Client::connect(&daemon).await;
    assert_eq!(client.ask("f").await, "14060000\n");
    assert_eq!(daemon.rigs[0].server.stats().accepted, 1);
    assert_eq!(daemon.rigs[0].server.stats().active, 1);
    daemon.stop().await;
}

// Tests [@ANCHOR: radioctl_d_server_box_future]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_peer_outside_the_allow_list_is_dropped_before_any_byte_is_read() {
    let text = config_text(
        "[server]\nallow = [\"10.0.0.0/8\"]",
        "kx3",
        "elecraft-kx3.toml",
        false,
        false,
    );
    let daemon = start(&text).await;
    let mut client = Client::connect(&daemon).await;
    client.send_raw(b"f\n").await;
    assert!(client.is_closed(Duration::from_secs(2)).await);
    let stats = daemon.rigs[0].server.stats();
    assert_eq!((stats.denied, stats.accepted), (1, 0));
    assert_eq!(daemon.rigs[0].service.stats().commands, 0);
    daemon.stop().await;
}

// Tests [@ANCHOR: radioctl_d_server_counters]
// Tests [@ANCHOR: radioctl_d_server_server_stats]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_client_limit_is_enforced_and_slots_are_freed() {
    let text = config_text(
        "[server]\nmax_clients = 2",
        "kx3",
        "elecraft-kx3.toml",
        false,
        false,
    );
    let daemon = start(&text).await;
    let mut a = Client::connect(&daemon).await;
    let mut b = Client::connect(&daemon).await;
    assert_eq!(a.ask("f").await, "14060000\n");
    assert_eq!(b.ask("f").await, "14060000\n");
    let mut c = Client::connect(&daemon).await;
    assert!(c.is_closed(Duration::from_secs(2)).await);
    assert_eq!(daemon.rigs[0].server.stats().busy, 1);
    drop(a);
    tokio::time::sleep(Duration::from_millis(200)).await;
    let mut d = Client::connect(&daemon).await;
    assert_eq!(d.ask("f").await, "14060000\n");
    daemon.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_over_long_line_gets_an_error_and_the_connection_is_closed() {
    let text = config_text(
        "[server]\nmax_line_bytes = 64",
        "kx3",
        "elecraft-kx3.toml",
        false,
        false,
    );
    let daemon = start(&text).await;
    let mut client = Client::connect(&daemon).await;
    client.send_raw(&[b'x'; 65]).await;
    assert_eq!(client.read_reply().await, "RPRT -1\n");
    assert!(client.is_closed(Duration::from_secs(2)).await);
    assert_eq!(daemon.rigs[0].server.stats().line_too_long, 1);
    let mut fine = Client::connect(&daemon).await;
    let sixty_four = format!("f {}\n", " ".repeat(61));
    fine.send_raw(sixty_four.as_bytes()).await;
    assert_eq!(fine.read_reply().await, "14060000\n");
    daemon.stop().await;
}

// Tests [@ANCHOR: radioctl_d_server_shared]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn silent_and_stalled_clients_are_dropped() {
    let text = config_text(
        "[server]\nidle_timeout_secs = 1\nline_timeout_secs = 1",
        "kx3",
        "elecraft-kx3.toml",
        false,
        false,
    );
    let daemon = start(&text).await;
    let mut idle = Client::connect(&daemon).await;
    assert_eq!(idle.ask("f").await, "14060000\n");
    assert!(
        idle.is_closed(Duration::from_secs(3)).await,
        "idle client kept"
    );
    let mut stalled = Client::connect(&daemon).await;
    stalled.send_raw(b"F 1407").await;
    assert!(
        stalled.is_closed(Duration::from_secs(3)).await,
        "half a line kept"
    );
    let mut trickle = Client::connect(&daemon).await;
    // Generous hang guard: the line timeout must cut the trickle off long before this many pulses.
    let mut cut_off = false;
    for _ in 0..100 {
        trickle.send_raw(b" ").await;
        if trickle.is_closed(Duration::from_millis(300)).await {
            cut_off = true;
            break;
        }
    }
    assert!(cut_off, "a trickle outlived the line timeout");
    assert!(daemon.rigs[0].server.stats().timeouts >= 2);
    daemon.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn garbage_on_the_wire_never_stops_the_server() {
    let daemon = start(&config_text("", "kx3", "elecraft-kx3.toml", false, false)).await;
    let mut noisy = Client::connect(&daemon).await;
    let garbage: Vec<u8> = (0..4000u32)
        .map(|i| (i.wrapping_mul(2_654_435_761) >> 13) as u8)
        .collect();
    for chunk in garbage.chunks(300) {
        noisy.send_raw(chunk).await;
    }
    noisy.send_raw(b"\n\\\n+\n\\\\\n\xff\xfe\n").await;
    let _ = noisy.read_reply().await;
    drop(noisy);
    let mut good = Client::connect(&daemon).await;
    assert_eq!(good.ask("f").await, "14060000\n");
    daemon.stop().await;
}

// Tests [@ANCHOR: radioctld_server_run_client]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_new_partial_line_gets_its_own_line_timeout() {
    let text = config_text(
        "[server]\nline_timeout_secs = 2",
        "kx3",
        "elecraft-kx3.toml",
        false,
        false,
    );
    let daemon = start(&text).await;
    let mut slow = Client::connect(&daemon).await;
    slow.send_raw(b"\n").await;
    // First line begins now and ends 1.4 s later, together with the start of the next one.
    slow.send_raw(b"f").await;
    tokio::time::sleep(Duration::from_millis(1400)).await;
    slow.send_raw(b"\nf").await;
    assert_eq!(slow.read_reply().await, "14060000\n");
    // The second line began 1.4 s ago too, 2.8 s after the first began; it still has 0.6 s.
    tokio::time::sleep(Duration::from_millis(1400)).await;
    slow.send_raw(b"\n").await;
    assert_eq!(
        slow.read_reply().await,
        "14060000\n",
        "the second line was timed from the start of the first"
    );
    daemon.stop().await;
}
