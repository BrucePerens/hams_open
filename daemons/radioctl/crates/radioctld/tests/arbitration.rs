// SPDX-License-Identifier: AGPL-3.0-or-later
//! Several clients through the one rig actor, PTT ownership, and the closed transmit gate, on
//! virtual rigs through the production path. A refused transmit command must leave no trace at
//! the rig: not a byte on its transport, not a state change.
#![allow(clippy::expect_used, clippy::panic)]

mod common;

use std::time::Duration;

use common::{config_text, eventually, start, Client};
use radioctld::daemon::Daemon;
use tokio::time::sleep;

fn bytes(daemon: &Daemon) -> usize {
    daemon.rigs[0]
        .virtual_control
        .as_ref()
        .expect("virtual rig")
        .bytes_received()
}

fn ptt(daemon: &Daemon) -> bool {
    daemon.rigs[0]
        .virtual_control
        .as_ref()
        .expect("virtual rig")
        .state()
        .ptt
}

// Tests [@ANCHOR: radioctld_service_check_transmit]
// Tests [@ANCHOR: radioctld_service_refuse]
// Tests [@ANCHOR: radioctld_service_key]
// Tests [@ANCHOR: radioctld_service_send_morse]
// Tests [@ANCHOR: radioctld_service_unkey]
// Tests [@ANCHOR: radioctld_service_stats]
// Tests [@ANCHOR: radioctld_service_rig]
// Tests [@ANCHOR: radioctld_service_exec]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_refused_transmit_command_never_reaches_the_rig() {
    let daemon = start(&config_text("", "kx3", "elecraft-kx3.toml", false, false)).await;
    let mut client = Client::connect(&daemon).await;
    assert_eq!(client.ask("f").await, "14060000\n");
    let before = bytes(&daemon);
    let refused = [
        "T 1",
        "T 2",
        "T 3",
        "+T 1",
        "b CQ TEST",
        "\\send_morse CQ",
        "+b CQ",
    ];
    for line in refused {
        let reply = client.ask(line).await;
        assert!(reply.ends_with("RPRT -9\n"), "{line}: {reply:?}");
    }
    sleep(Duration::from_millis(100)).await;
    assert_eq!(
        bytes(&daemon),
        before,
        "a refused command put bytes on the rig's transport"
    );
    let control = daemon.rigs[0]
        .virtual_control
        .as_ref()
        .expect("virtual rig");
    assert!(!control.state().ptt);
    assert!(control.state().cw_sent.is_empty());
    assert_eq!(
        daemon.rigs[0].service.stats().transmit_refused,
        refused.len() as u64
    );
    assert_eq!(
        daemon.rigs[0].service.ptt_owner(),
        None,
        "a refused key claims nothing"
    );
    assert!(daemon.rigs[0].service.rig().transmit_permit().is_none());
    assert_eq!(
        client.ask("T 0").await,
        "RPRT 0\n",
        "unkeying is always allowed"
    );
    assert!(bytes(&daemon) > before, "the unkey did reach the rig");
    daemon.stop().await;
}

// Tests [@ANCHOR: radioctld_caps_dump_state]
// Tests [@ANCHOR: radioctld_service_caps_view]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_dump_state_of_a_closed_rig_advertises_no_transmit() {
    let closed = start(&config_text("", "kx3", "elecraft-kx3.toml", false, false)).await;
    let open = start(&config_text("", "kx3", "elecraft-kx3.toml", true, false)).await;
    let closed_state = Client::connect(&closed).await.ask("\\dump_state").await;
    let open_state = Client::connect(&open).await.ask("\\dump_state").await;
    assert!(closed_state.contains("\nptt_type=0x0\n"), "{closed_state}");
    assert!(open_state.contains("\nptt_type=0x1\n"), "{open_state}");
    assert_eq!(open_state.lines().count(), closed_state.lines().count() + 1);
    assert!(closed_state.ends_with("done\n"));
    closed.stop().await;
    open.stop().await;
}

// Tests [@ANCHOR: radioctld_service_key]
// Tests [@ANCHOR: radioctld_service_unkey]
// Tests [@ANCHOR: radioctld_service_disconnect]
// Tests [@ANCHOR: radioctld_service_ptt_owner]
// Tests [@ANCHOR: radioctld_service_send_morse]
// Tests [@ANCHOR: radioctld_rigs_build_rig]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn one_transmitter_one_owner_and_a_departing_owner_unkeys() {
    let daemon = start(&config_text("", "kx3", "elecraft-kx3.toml", true, false)).await;
    let mut a = Client::connect(&daemon).await;
    let mut b = Client::connect(&daemon).await;
    assert_eq!(a.ask("T 1").await, "RPRT 0\n");
    eventually("the rig to be keyed", || ptt(&daemon)).await;
    assert_eq!(daemon.rigs[0].service.ptt_owner(), Some(1));
    let before = bytes(&daemon);
    assert_eq!(
        b.ask("T 1").await,
        "RPRT -9\n",
        "another client may not key"
    );
    assert_eq!(b.ask("b CQ").await, "RPRT -9\n", "nor send Morse");
    sleep(Duration::from_millis(60)).await;
    assert_eq!(
        bytes(&daemon),
        before,
        "the conflicting commands never reached the rig"
    );
    assert!(ptt(&daemon));
    assert_eq!(a.ask("t").await, "1\n");
    assert_eq!(b.ask("T 0").await, "RPRT 0\n", "anybody may unkey");
    eventually("the rig to be unkeyed", || !ptt(&daemon)).await;
    assert_eq!(daemon.rigs[0].service.ptt_owner(), None);
    assert_eq!(b.ask("T 1").await, "RPRT 0\n");
    eventually("the rig to be keyed by b", || ptt(&daemon)).await;
    assert_eq!(a.ask("T 1").await, "RPRT -9\n");
    drop(b);
    eventually("the rig to be unkeyed after its owner left", || {
        !ptt(&daemon)
    })
    .await;
    assert_eq!(daemon.rigs[0].service.ptt_owner(), None);
    assert_eq!(a.ask("T 1").await, "RPRT 0\n");
    assert!(daemon.rigs[0].service.stats().ptt_conflicts >= 3);
    daemon.stop().await;
}

// Tests [@ANCHOR: radioctld_service_shutdown]
// Tests [@ANCHOR: radioctld_daemon_stop]
// Tests [@ANCHOR: radioctld_server_shutdown]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn stopping_the_daemon_unkeys_a_rig_a_client_was_keying() {
    let mut daemon = start(&config_text("", "kx3", "elecraft-kx3.toml", true, false)).await;
    let mut a = Client::connect(&daemon).await;
    assert_eq!(a.ask("T 1").await, "RPRT 0\n");
    eventually("the rig to be keyed", || ptt(&daemon)).await;
    let control = daemon.rigs[0].virtual_control.clone().expect("virtual rig");
    let served = daemon.rigs.remove(0);
    served.server.shutdown().await;
    served.service.shutdown().await;
    eventually("unkey on shutdown", || !control.state().ptt).await;
}

// Tests [@ANCHOR: radioctld_service_read]
// Tests [@ANCHOR: radioctld_service_set]
// Tests [@ANCHOR: radioctld_service_frequency]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_second_client_reads_what_the_first_set_from_the_cache() {
    let daemon = start(&config_text("", "kx3", "elecraft-kx3.toml", false, false)).await;
    let mut a = Client::connect(&daemon).await;
    let mut b = Client::connect(&daemon).await;
    assert_eq!(a.ask("F 7074000").await, "RPRT 0\n");
    let control = daemon.rigs[0].virtual_control.clone().expect("virtual rig");
    eventually("the rig to see the new frequency", || {
        control.state().freq_a.0 == 7_074_000
    })
    .await;
    let (bytes_before, hits_before) = (bytes(&daemon), daemon.rigs[0].service.stats().cache_hits);
    assert_eq!(b.ask("f").await, "7074000\n");
    assert_eq!(a.ask("f").await, "7074000\n");
    assert_eq!(
        bytes(&daemon),
        bytes_before,
        "cached reads cost the radio nothing"
    );
    assert_eq!(daemon.rigs[0].service.stats().cache_hits, hits_before + 2);
    daemon.stop().await;
}

// Tests [@ANCHOR: radioctld_service_set_mode]
// Tests [@ANCHOR: radioctld_service_mode_and_width]
// Tests [@ANCHOR: radioctld_service_mode_fields]
// Tests [@ANCHOR: radioctld_service_require_mode]
// Tests [@ANCHOR: radioctld_service_set_split_vfo]
// Tests [@ANCHOR: radioctld_service_split_vfo]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn many_clients_hammering_one_rig_all_get_consistent_answers() {
    let daemon = start(&config_text("", "kx3", "elecraft-kx3.toml", false, false)).await;
    let addr = daemon.rigs[0].server.local_addr();
    let mut tasks = Vec::new();
    for worker in 0..8u64 {
        tasks.push(tokio::spawn(async move {
            let mut client = Client::connect_to(addr).await;
            for round in 0..25u64 {
                let hz = 7_000_000 + worker * 1000 + round;
                assert_eq!(client.ask(&format!("F {hz}")).await, "RPRT 0\n");
                let read = client.ask("f").await;
                let value: u64 = read.trim().parse().expect("a frequency");
                assert!((7_000_000..7_010_000).contains(&value), "{read:?}");
                assert_eq!(client.ask("M USB 2400").await, "RPRT 0\n");
                assert_eq!(client.ask("X USB 2400").await, "RPRT 0\n");
                assert!(client.ask("m").await.starts_with("USB\n"));
                assert_eq!(client.ask("S 0 VFOA").await, "RPRT 0\n");
                assert!(client.ask("s").await.starts_with("0\n"));
            }
        }));
    }
    for task in tasks {
        task.await.expect("client task");
    }
    assert_eq!(daemon.rigs[0].server.stats().accepted, 8);
    assert_eq!(daemon.rigs[0].service.rig().stats().commands_failed, 0);
    daemon.stop().await;
}

// Tests [@ANCHOR: radioctld_command_morse_text]
// Tests [@ANCHOR: radioctld_service_send_morse]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn morse_text_cannot_smuggle_a_cat_command_through_an_open_gate() {
    let daemon = start(&config_text("", "kx3", "elecraft-kx3.toml", true, false)).await;
    let mut client = Client::connect(&daemon).await;
    for line in ["b CQ;TX", "b CQ;TX;", "\\send_morse ;TX;", "b CQ\u{7f}"] {
        let reply = client.ask(line).await;
        assert!(
            reply == "RPRT -11\n" || reply == "RPRT -1\n",
            "{line}: {reply:?}"
        );
    }
    sleep(Duration::from_millis(100)).await;
    assert!(!ptt(&daemon), "a Morse text keyed the transmitter");
    let control = daemon.rigs[0]
        .virtual_control
        .as_ref()
        .expect("virtual rig");
    assert!(control.state().cw_sent.is_empty());
    assert_eq!(client.ask("b CQ").await, "RPRT 0\n", "plain text is fine");
    eventually("the Morse to reach the rig", || {
        control.state().cw_sent == vec!["CQ".to_string()]
    })
    .await;
    daemon.stop().await;
}

// Tests [@ANCHOR: radioctld_service_send_morse]
// Tests [@ANCHOR: radioctld_service_disconnect]
// Tests [@ANCHOR: radioctld_service_ptt_owner]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_morse_sender_owns_the_transmitter_and_its_departure_unkeys() {
    let daemon = start(&config_text("", "kx3", "elecraft-kx3.toml", true, false)).await;
    let mut a = Client::connect(&daemon).await;
    let mut b = Client::connect(&daemon).await;
    assert_eq!(a.ask("b CQ").await, "RPRT 0\n");
    assert_eq!(
        daemon.rigs[0].service.ptt_owner(),
        Some(1),
        "Morse puts RF on the air, so the sender holds the transmitter"
    );
    assert_eq!(
        b.ask("T 1").await,
        "RPRT -9\n",
        "nobody else may key meanwhile"
    );
    assert_eq!(b.ask("b DE").await, "RPRT -9\n");
    let before = bytes(&daemon);
    drop(a);
    eventually("the departing sender to give up and unkey the rig", || {
        daemon.rigs[0].service.ptt_owner().is_none() && bytes(&daemon) > before
    })
    .await;
    assert_eq!(b.ask("T 1").await, "RPRT 0\n");
    daemon.stop().await;
}

// Tests [@ANCHOR: radioctld_service_shutdown]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn shutdown_unkeys_even_when_no_owner_is_recorded() {
    let mut daemon = start(&config_text("", "kx3", "elecraft-kx3.toml", true, false)).await;
    let control = daemon.rigs[0].virtual_control.clone().expect("virtual rig");
    let before = control.bytes_received();
    assert_eq!(daemon.rigs[0].service.ptt_owner(), None);
    let served = daemon.rigs.remove(0);
    served.server.shutdown().await;
    served.service.shutdown().await;
    eventually("an unkey to reach the rig on shutdown", || {
        control.bytes_received() > before && !control.state().ptt
    })
    .await;
}
