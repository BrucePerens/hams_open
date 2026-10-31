// SPDX-License-Identifier: AGPL-3.0-or-later
//! The FlexRadio engine behind the real rig actor and the real TCP transport, talking to a
//! virtual FLEX-6300 served on a real loopback listener: reads, sets, status pushed from the
//! radio, both orders of reply and status, and the closed transmit gate checked on the wire.
#![allow(clippy::expect_used, clippy::panic)]

use std::time::Duration;

use radioctl_core::description::RigDescription;
use radioctl_core::error::ErrorKind;
use radioctl_core::op::{Field, Observation, Op};
use radioctl_core::rig::{spawn, Freshness, RigHandle, RigOptions, TransmitPermit};
use radioctl_core::rigfile;
use radioctl_core::tcp::{connect_tcp, TcpConfig};
use radioctl_core::units::{Hz, Level, Mode, Vfo};
use radioctl_protocols::flex::FlexEngine;
use radioctl_protocols::flex_io::{serve_virtual_flex, VirtualFlexControl};
use radioctl_protocols::virtual_flex::{FlexPanel, Order, VirtualFlex};
use tokio::net::TcpListener;
use tokio::runtime::Handle;
use tokio::time::{sleep, timeout};

fn description() -> RigDescription {
    rigfile::parse(include_str!("../../../rigs/flex-6300.toml"))
        .expect("flex file")
        .description
}

async fn flex(order: Order) -> (RigHandle, VirtualFlexControl) {
    let desc = description();
    let mut rig = VirtualFlex::from_description(&desc).expect("virtual flex");
    rig.order = order;
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("addr");
    let control = serve_virtual_flex(&Handle::current(), listener, rig);
    let stream = connect_tcp(addr, &TcpConfig::default())
        .await
        .expect("connect");
    let mut options = RigOptions::default();
    options.poll.enabled = false;
    options.default_timeout = Duration::from_secs(3);
    let engine = FlexEngine::new(desc).expect("engine");
    let handle = spawn(&Handle::current(), engine, stream, options).expect("spawn");
    (handle, control)
}

async fn eventually(what: &str, mut check: impl FnMut() -> bool) {
    for _ in 0..100 {
        if check() {
            return;
        }
        sleep(Duration::from_millis(20)).await;
    }
    panic!("never happened: {what}");
}

// Tests [@ANCHOR: radioctl_virtual_flex_serve]
// Tests [@ANCHOR: radioctl_virtual_flex_new]
// Tests [@ANCHOR: radioctl_virtual_flex_feed]
// Tests [@ANCHOR: radioctl_virtual_flex_command]
// Tests [@ANCHOR: radioctl_virtual_flex_control_state]
// Tests [@ANCHOR: radioctl_flex_engine_encode]
// Tests [@ANCHOR: radioctl_core_tcp_connect]
async fn reads_and_sets_work_in(order: Order) {
    let (rig, control) = flex(order).await;
    let first = timeout(
        Duration::from_secs(5),
        rig.frequency(Vfo::A, Freshness::Fresh),
    )
    .await
    .expect("no hang")
    .expect("frequency");
    assert_eq!(first.value, Hz(14_074_000));
    let received = control.received();
    assert_eq!(
        received.get(..4),
        Some(
            &[
                "sub slice all".to_string(),
                "sub tx all".to_string(),
                "sub pan all".to_string(),
                "info".to_string()
            ][..]
        ),
        "{received:?}"
    );
    rig.set_frequency(Vfo::A, Hz(7_100_500)).await.expect("set");
    rig.set_mode(Mode::Lsb).await.expect("mode");
    rig.execute(Op::SetFilterWidth(Hz(2_400)))
        .await
        .expect("filter");
    rig.execute(Op::SetLevel(Level::AfGain, 35))
        .await
        .expect("af");
    rig.execute(Op::SetLevel(Level::RfPower, 25))
        .await
        .expect("power");
    rig.execute(Op::SetLevel(Level::MicGain, 55))
        .await
        .expect("mic");
    let state = control.state();
    assert_eq!(state.frequency, Hz(7_100_500));
    assert_eq!(state.mode, Mode::Lsb);
    assert_eq!((state.filter_lo_hz, state.filter_hi_hz), (100, 2_500));
    assert_eq!(
        (state.audio_level, state.rf_power, state.mic_level),
        (35, 25, 55)
    );
    let fresh = |op: Op| {
        let rig = rig.clone();
        async move { rig.execute(op).await.expect("read").expect("value") }
    };
    assert_eq!(
        fresh(Op::GetFrequency(Vfo::A)).await,
        Observation::Frequency(Vfo::A, Hz(7_100_500))
    );
    assert_eq!(fresh(Op::GetMode).await, Observation::Mode(Mode::Lsb));
    assert_eq!(
        fresh(Op::GetFilterWidth).await,
        Observation::FilterWidth(Hz(2_400))
    );
    assert_eq!(
        fresh(Op::GetLevel(Level::AfGain)).await,
        Observation::Level(Level::AfGain, 35)
    );
    assert_eq!(fresh(Op::GetFirmware).await, Observation::Firmware(3, 2));
    assert_eq!(fresh(Op::GetPtt).await, Observation::Ptt(false));
    // Refused by the radio: the typed error, not a hang.
    assert!(rig.execute(Op::SetLevel(Level::AfGain, 101)).await.is_err());
}

#[tokio::test]
async fn a_virtual_flex_over_tcp_answers_reads_and_sets_reply_first() {
    reads_and_sets_work_in(Order::ReplyFirst).await;
}

#[tokio::test]
async fn a_virtual_flex_over_tcp_answers_reads_and_sets_status_first() {
    reads_and_sets_work_in(Order::StatusFirst).await;
}

// Tests [@ANCHOR: radioctl_virtual_flex_front_panel]
// Tests [@ANCHOR: radioctl_flex_engine_feed]
#[tokio::test]
async fn a_change_at_the_radio_reaches_the_cache_and_the_event_stream() {
    let (rig, control) = flex(Order::ReplyFirst).await;
    let mut events = rig.events();
    rig.frequency(Vfo::A, Freshness::Fresh)
        .await
        .expect("first");
    control.front_panel(FlexPanel::Tune(Hz(10_136_000)));
    control.front_panel(FlexPanel::SetMode(Mode::Data));
    eventually("cache follows the radio", || {
        rig.cached(Field::Frequency(Vfo::A)).map(|c| c.value)
            == Some(Observation::Frequency(Vfo::A, Hz(10_136_000)))
            && rig.cached(Field::Mode).map(|c| c.value) == Some(Observation::Mode(Mode::Data))
    })
    .await;
    let event = timeout(Duration::from_secs(2), events.next())
        .await
        .expect("an event in time")
        .expect("an event");
    assert!(!event.changes.is_empty());
    control.front_panel(FlexPanel::Mox(true));
    eventually("ptt follows the interlock", || {
        rig.cached(Field::Ptt).map(|c| c.value) == Some(Observation::Ptt(true))
    })
    .await;
    control.front_panel(FlexPanel::Mox(false));
    eventually("ptt released", || {
        rig.cached(Field::Ptt).map(|c| c.value) == Some(Observation::Ptt(false))
    })
    .await;
}

#[tokio::test]
async fn nothing_that_can_key_reaches_the_wire_while_the_gate_is_closed() {
    let (rig, control) = flex(Order::ReplyFirst).await;
    rig.frequency(Vfo::A, Freshness::Fresh)
        .await
        .expect("link up");
    let ptt = rig.set_ptt(true).await.expect_err("gate closed");
    assert_eq!(ptt.kind, ErrorKind::TransmitGateClosed);
    let cw = rig
        .execute(Op::SendCw("CQ TEST".into()))
        .await
        .expect_err("gate closed");
    assert_eq!(cw.kind, ErrorKind::TransmitGateClosed);
    // Give a stray command time to arrive, then look at every line the radio saw.
    sleep(Duration::from_millis(200)).await;
    let received = control.received();
    assert!(
        !received
            .iter()
            .any(|line| radioctl_protocols::flex::line_may_transmit(line)),
        "{received:?}"
    );
    assert!(!control.state().keyed && control.state().cw_sent.is_empty());
    // Unkeying is always allowed.
    rig.set_ptt(false).await.expect("unkey");
    assert!(control.received().iter().any(|l| l == "xmit 0"));
    // Open the gate: now the same commands go out.
    rig.open_transmit_gate(TransmitPermit::new("flex test"));
    rig.set_ptt(true).await.expect("key virtual radio");
    assert!(control.state().keyed);
    rig.set_ptt(false).await.expect("unkey");
    rig.execute(Op::SendCw("CQ TEST".into())).await.expect("cw");
    assert_eq!(control.state().cw_sent, vec!["CQ TEST".to_string()]);
    rig.close_transmit_gate();
    let again = rig.set_ptt(true).await.expect_err("closed again");
    assert_eq!(again.kind, ErrorKind::TransmitGateClosed);
}
