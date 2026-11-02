// SPDX-License-Identifier: AGPL-3.0-or-later
//! `ProtocolEngine::link_restored` behind the real rig actor: a virtual Kenwood TS-2000, a virtual
//! Elecraft KX3 and a virtual FLEX-6300 lose their link and get it back. The rig forgets the
//! session the controller set up (`AI2;` auto-information, Flex subscriptions); the actor unkeys
//! it as before and the engine sets the session up again exactly once. The TS-890S LAN session
//! (login in the framing, `AI2;` in the engine) is in `ts890_lan.rs`. Nothing leaves the machine.
#![allow(clippy::expect_used, clippy::panic)]

use std::path::PathBuf;
use std::time::Duration;

use radioctl_core::description::RigDescription;
use radioctl_core::error::ErrorKind;
use radioctl_core::op::{Alarm, Observation, Op};
use radioctl_core::rig::{spawn, spawn_current, Freshness, RigHandle, RigOptions, TransmitPermit};
use radioctl_core::rigfile;
use radioctl_core::serial::LinkStatus;
use radioctl_core::tcp::{connect_tcp, TcpConfig};
use radioctl_core::units::{Hz, Vfo};
use radioctl_protocols::elecraft::ElecraftEngine;
use radioctl_protocols::flex::FlexEngine;
use radioctl_protocols::flex_io::{serve_virtual_flex, VirtualFlexControl};
use radioctl_protocols::io::{spawn_virtual_rig, VirtualRigControl};
use radioctl_protocols::kenwood::KenwoodEngine;
use radioctl_protocols::virtual_flex::VirtualFlex;
use radioctl_protocols::virtual_rig::VirtualRig;
use tokio::net::TcpListener;
use tokio::runtime::Handle;
use tokio::sync::watch;
use tokio::time::{sleep, timeout};

fn connected(generation: u32) -> LinkStatus {
    LinkStatus::Connected {
        port: PathBuf::from("/dev/virtual"),
        generation,
    }
}

fn options(status: watch::Receiver<LinkStatus>) -> RigOptions {
    let mut options = RigOptions::default();
    options.poll.enabled = false;
    options.default_timeout = Duration::from_secs(2);
    options.link_watch = Some(status);
    options
}

/// Takes the link down and brings it back, and waits until the actor has finished restoring it
/// (the `LinkRestored` event comes after the unkey and the session set-up).
async fn drop_and_restore(
    rig: &RigHandle,
    status: &watch::Sender<LinkStatus>,
    generation: u32,
    forget: impl FnOnce(),
) {
    let mut events = rig.events();
    status
        .send(LinkStatus::Reconnecting {
            attempt: 1,
            last_error: "cable pulled".into(),
        })
        .expect("send");
    timeout(Duration::from_secs(5), async {
        while rig.link_up() {
            sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("link down");
    forget();
    status.send(connected(generation)).expect("send");
    timeout(Duration::from_secs(5), async {
        while let Some(event) = events.next().await {
            if event
                .changes
                .iter()
                .any(|c| c.new == Observation::Alarm(Alarm::LinkRestored))
            {
                return;
            }
        }
        panic!("event stream ended");
    })
    .await
    .expect("link restored event");
    // The virtual radio reads what the actor wrote on a task of its own: give it time to take
    // the session set-up in, so that a count taken next is a count of everything sent.
    sleep(Duration::from_millis(100)).await;
}

async fn ascii_rig(
    description: RigDescription,
    engine: impl radioctl_core::engine::ProtocolEngine,
) -> (RigHandle, VirtualRigControl, watch::Sender<LinkStatus>) {
    let rig = VirtualRig::from_description(&description).expect("virtual rig");
    let (stream, control) = spawn_virtual_rig(&Handle::current(), rig, Duration::ZERO);
    let (status, watcher) = watch::channel(connected(1));
    let handle = spawn_current(engine, stream, options(watcher)).expect("spawn");
    (handle, control, status)
}

// Tests [@ANCHOR: radioctl_kenwood_engine_link_restored]
// Tests [@ANCHOR: radioctl_core_actor_restore_session]
// Tests [@ANCHOR: radioctl_core_actor_link_status_changed]
// Tests [@ANCHOR: radioctl_virtual_forget_session]
// Tests [@ANCHOR: radioctl_virtual_io_forget_session]
#[tokio::test]
async fn kenwood_auto_information_comes_back_once_and_the_rig_is_unkeyed() {
    let description = RigDescription::ts2000();
    let engine = KenwoodEngine::new(description.clone()).expect("engine");
    let (rig, control, status) = ascii_rig(description, engine).await;
    rig.open_transmit_gate(TransmitPermit::new("link test"));
    rig.execute(Op::SetTransceive(true)).await.expect("ai");
    rig.set_ptt(true).await.expect("key");
    assert!(control.state().ptt && control.state().transceive);
    let before = control.bytes_received();

    drop_and_restore(&rig, &status, 2, || control.forget_session()).await;
    let state = control.state();
    assert!(!state.ptt, "unkeyed on restore, as before");
    assert!(state.transceive, "AI2; sent again");
    // `RX;` (3 bytes) and `AI2;` (4 bytes): each once.
    assert_eq!(control.bytes_received() - before, 3 + 4);

    // The gate is as it was: open, so the operator can key; and the unkey still works.
    rig.set_ptt(true).await.expect("gate unchanged");
    rig.set_ptt(false).await.expect("unkey");
    assert!(!control.state().ptt);

    // A second restore sends it once more, not twice.
    let before = control.bytes_received();
    drop_and_restore(&rig, &status, 3, || control.forget_session()).await;
    assert_eq!(control.bytes_received() - before, 3 + 4);
}

// Tests [@ANCHOR: radioctl_kenwood_engine_link_restored]
// Tests [@ANCHOR: radioctl_core_actor_restore_session]
#[tokio::test]
async fn a_kenwood_client_that_never_asked_for_auto_information_gets_none_back() {
    let description = RigDescription::ts2000();
    let engine = KenwoodEngine::new(description.clone()).expect("engine");
    let (rig, control, status) = ascii_rig(description, engine).await;
    rig.frequency(Vfo::A, Freshness::Fresh).await.expect("read");
    let before = control.bytes_received();
    drop_and_restore(&rig, &status, 2, || control.forget_session()).await;
    assert_eq!(control.bytes_received() - before, 3, "only the unkey");
    assert!(!control.state().transceive);
    // A closed gate stays closed across a restore.
    assert_eq!(
        rig.set_ptt(true).await.expect_err("gate").kind,
        ErrorKind::TransmitGateClosed
    );
}

// Tests [@ANCHOR: radioctl_elecraft_engine_link_restored]
// Tests [@ANCHOR: radioctl_core_actor_restore_session]
#[tokio::test]
async fn elecraft_auto_information_comes_back_once_and_the_rig_is_unkeyed() {
    let description = rigfile::parse(include_str!("../../../rigs/elecraft-kx3.toml"))
        .expect("kx3 file")
        .description;
    let engine = ElecraftEngine::new(description.clone()).expect("engine");
    let (rig, control, status) = ascii_rig(description, engine).await;
    rig.open_transmit_gate(TransmitPermit::new("link test"));
    rig.execute(Op::SetTransceive(true)).await.expect("ai");
    rig.set_ptt(true).await.expect("key");
    assert_eq!(control.state().auto_info, 2);
    let before = control.bytes_received();

    drop_and_restore(&rig, &status, 2, || control.forget_session()).await;
    let state = control.state();
    assert!(!state.ptt, "unkeyed on restore, as before");
    assert_eq!(state.auto_info, 2, "AI2; sent again");
    assert_eq!(control.bytes_received() - before, 3 + 4);
    rig.set_ptt(true).await.expect("gate unchanged");
    rig.set_ptt(false).await.expect("unkey");
}

// Tests [@ANCHOR: radioctl_flex_engine_link_restored]
// Tests [@ANCHOR: radioctl_core_actor_restore_session]
// Tests [@ANCHOR: radioctl_virtual_flex_forget_session]
// Tests [@ANCHOR: radioctl_virtual_flex_subscribed]
// Tests [@ANCHOR: radioctl_virtual_flex_control_forget_session]
// Tests [@ANCHOR: radioctl_virtual_flex_control_subscribed]
#[tokio::test]
async fn flex_subscriptions_come_back_once_and_the_gate_stays_closed() {
    let description = rigfile::parse(include_str!("../../../rigs/flex-6300.toml"))
        .expect("flex file")
        .description;
    let radio = VirtualFlex::from_description(&description).expect("virtual flex");
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("addr");
    let control: VirtualFlexControl = serve_virtual_flex(&Handle::current(), listener, radio);
    let stream = connect_tcp(addr, &TcpConfig::default())
        .await
        .expect("connect");
    let (status, watcher) = watch::channel(connected(1));
    let engine = FlexEngine::new(description).expect("engine");
    let rig = spawn(&Handle::current(), engine, stream, options(watcher)).expect("spawn");

    rig.frequency(Vfo::A, Freshness::Fresh).await.expect("read");
    assert_eq!(control.subscribed(), (true, true, true));
    let count = |text: &str| control.received().iter().filter(|l| *l == text).count();
    assert_eq!(count("sub slice all"), 1);

    drop_and_restore(&rig, &status, 2, || control.forget_session()).await;
    assert_eq!(control.subscribed(), (true, true, true), "subscribed again");
    assert_eq!(count("sub slice all"), 2, "once per link, no more");
    assert_eq!(count("sub tx all"), 2);
    assert_eq!(count("sub pan all"), 2);
    assert_eq!(count("info"), 2);
    // The unkey went out, and the next command does not repeat the subscriptions.
    assert!(count("xmit 0") >= 1);
    rig.frequency(Vfo::A, Freshness::Fresh).await.expect("read");
    assert_eq!(count("sub slice all"), 2);
    assert!(!control.state().keyed);
    rig.set_frequency(Vfo::A, Hz(7_074_000)).await.expect("set");
    assert_eq!(
        rig.set_ptt(true).await.expect_err("gate").kind,
        ErrorKind::TransmitGateClosed
    );
}

// Tests [@ANCHOR: radioctl_core_actor_open_session]
// Tests [@ANCHOR: radioctl_core_actor_write_session_requests]
// Tests [@ANCHOR: radioctl_receiver_engine_open_requests]
// Tests [@ANCHOR: radioctl_receiver_engine_link_restored]
// Tests [@ANCHOR: radioctl_pcr_dialect_open_commands]
#[tokio::test]
async fn an_icom_pcr_is_powered_on_when_the_actor_starts_and_again_after_a_link_loss() {
    let description = rigfile::parse(include_str!("../../../rigs/icom-ic-pcr1500.toml"))
        .expect("pcr file")
        .description;
    let engine = radioctl_protocols::icom_pcr::engine(description.clone()).expect("engine");
    let (rig, control, status) = ascii_rig(description, engine).await;
    // The first read waits for the open requests: the actor serves nothing before they are out.
    rig.frequency(Vfo::A, Freshness::Fresh).await.expect("read");
    // `H101\n` and `G300\n` (5 bytes each), then the `H1?\n` liveness read (4 bytes).
    assert_eq!(control.bytes_received(), 5 + 5 + 4);
    let before = control.bytes_received();
    drop_and_restore(&rig, &status, 2, || control.forget_session()).await;
    // A receiver cannot transmit, so the restore's unkey puts nothing on the wire; the two open
    // requests do.
    assert_eq!(control.bytes_received() - before, 5 + 5);
    rig.frequency(Vfo::A, Freshness::Fresh)
        .await
        .expect("read again");
}

// Tests [@ANCHOR: radioctl_core_engine_box_open_requests]
// Tests [@ANCHOR: radioctl_core_engine_box_link_restored]
// Tests [@ANCHOR: radioctl_core_op_transmits]
#[test]
fn no_engines_session_set_up_can_put_a_transmitter_on_the_air() {
    // For every shipped description: whatever the open and link-restored hooks return (on a fresh
    // engine, and again after the controller's usual session commands) differs from the bytes
    // that key the rig (PTT on, CW) or contains none of them.
    use radioctl_core::units::Mode;
    use radioctl_protocols::conformance::engine_for;
    let rigs = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../rigs");
    let mut checked = 0;
    for entry in std::fs::read_dir(rigs).expect("rigs dir") {
        let path = entry.expect("entry").path();
        if path.extension().is_none_or(|x| x != "toml") {
            continue;
        }
        let description = rigfile::parse(&std::fs::read_to_string(&path).expect("read"))
            .unwrap_or_else(|e| panic!("{}: {e}", path.display()))
            .description;
        let Ok(mut engine) = engine_for(&description) else {
            continue;
        };
        let keying: Vec<Vec<u8>> = [Op::SetPtt(true), Op::SendCw("E".into())]
            .iter()
            .filter_map(|op| engine.encode(op).ok().map(|r| r.bytes))
            .collect();
        // The session the controller sets up, so link_restored has something to send again.
        let _ = engine.encode(&Op::SetTransceive(true));
        let _ = engine.encode(&Op::SetMode(Mode::Usb));
        let mut hooks = engine.open_requests();
        hooks.extend(engine.link_restored());
        hooks.extend(engine.link_restored());
        for request in hooks {
            for key in &keying {
                assert!(
                    !key.is_empty() && !request.bytes.windows(key.len()).any(|w| w == &key[..]),
                    "{}: a session set-up request {:?} carries a keying command {:?}",
                    description.id,
                    request.bytes,
                    key
                );
            }
        }
        checked += 1;
    }
    assert!(checked >= 20, "only {checked} engines checked");
}
