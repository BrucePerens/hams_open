// SPDX-License-Identifier: AGPL-3.0-or-later
//! The `radiod-channel` engine behind the real rig actor, bridged by `radiod_io` to a fake `radiod`
//! on loopback multicast (TTL 0, nothing leaves the host). The fake answers as the RX-888's `radiod`
//! did on 2026-11-05: one status per command echoing the tag, a front-end status (SSRC 0) with no tag,
//! and 16-bit PCM RTP at 12 kHz with a dynamic payload type. Checked: tuning, mode, passband width,
//! signal, the refusal of transmit, a lifetime on every channel command, the keep-alive, the audio
//! queue, the front-end read, and the channel's removal when the actor goes away.
#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use std::net::{Ipv4Addr, SocketAddrV4};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use radioctl_core::capability::Capability;
use radioctl_core::engine::ProtocolEngine;
use radioctl_core::error::RigError;
use radioctl_core::op::{Observation, Op};
use radioctl_core::rig::{spawn, Freshness, RigHandle, RigOptions};
use radioctl_core::rigfile;
use radioctl_core::units::{Hz, Mode, Vfo};
use radioctl_protocols::radiod_io::{serve_radiod, RadiodControl, RadiodLink};
use radioctl_protocols::radiod_net::{signal_raw, RadiodEngine};
use radiod_client::tlv::{parse, ty, Kind};
use socket2::{Domain, Protocol, Socket, Type};
use tokio::net::UdpSocket;
use tokio::runtime::Handle;
use tokio::time::sleep;

const SSRC: u32 = 4242;
const PT: u8 = 122;
const BASEBAND_DB: f32 = -127.1;

/// One command the fake received.
#[derive(Debug, Clone, PartialEq)]
struct Seen {
    ssrc: u32,
    lifetime: Option<u64>,
    frequency: Option<f64>,
    edges: Option<(f32, f32)>,
    /// `PLL_ENABLE` as sent, when the command carried it.
    pll: Option<bool>,
}

fn ports() -> (u16, u16) {
    ports_at(0)
}

/// A second pair of ports for a test that runs beside the first (a socket bound to a port gets every
/// group joined on it, so two concurrent fakes must not share one).
fn ports_at(pair: u16) -> (u16, u16) {
    let base = 30_000 + (std::process::id() % 7_000) as u16 * 4 + pair * 2;
    (base, base + 1)
}

fn join(group: Ipv4Addr, port: u16) -> UdpSocket {
    let s = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP)).expect("socket");
    s.set_reuse_address(true).expect("reuse");
    s.set_reuse_port(true).expect("reuse port");
    s.bind(&SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, port).into())
        .expect("bind");
    s.join_multicast_v4(&group, &Ipv4Addr::LOCALHOST)
        .expect("join");
    s.set_multicast_if_v4(&Ipv4Addr::LOCALHOST).expect("if");
    s.set_multicast_ttl_v4(0).expect("ttl 0");
    s.set_multicast_loop_v4(true).expect("loop");
    s.set_nonblocking(true).expect("nonblocking");
    UdpSocket::from_std(s.into()).expect("tokio")
}

fn tlv(out: &mut Vec<u8>, t: u8, v: &[u8]) {
    out.push(t);
    out.push(v.len() as u8);
    out.extend_from_slice(v);
}

/// The fake radiod's status socket loop: records every command and answers it. A channel whose PLL
/// was turned on (`PLL_ENABLE`) reports `PLL_ENABLE` and a `PLL_LOCK` that follows `locked`, as the
/// documented status fields say (ka9q-radio `docs/ka9q-radio-3.md`, `status.h`).
async fn fake_radiod(
    sock: UdpSocket,
    dest: SocketAddrV4,
    seen: Arc<Mutex<Vec<Seen>>>,
    locked: Arc<AtomicBool>,
) {
    let mut buf = vec![0u8; 65_536];
    let mut edges = (50.0f32, 3000.0f32);
    let mut freq = 0.0f64;
    let mut pll = false;
    loop {
        let Ok(n) = sock.recv(&mut buf).await else {
            return;
        };
        let Ok((Kind::Command, entries)) = parse(&buf[..n]) else {
            continue;
        };
        let find = |t: u8| entries.iter().find(|e| e.ty == t);
        let ssrc = find(ty::OUTPUT_SSRC)
            .and_then(|e| e.uint().ok())
            .unwrap_or(0) as u32;
        let tag = find(ty::COMMAND_TAG)
            .and_then(|e| e.uint().ok())
            .unwrap_or(0);
        let lo = find(ty::LOW_EDGE).and_then(|e| e.f32().ok());
        let hi = find(ty::HIGH_EDGE).and_then(|e| e.f32().ok());
        let f = find(ty::RADIO_FREQUENCY).and_then(|e| e.f64().ok());
        let pll_sent = find(ty::PLL_ENABLE).and_then(|e| e.bool().ok());
        seen.lock().unwrap().push(Seen {
            ssrc,
            lifetime: find(ty::LIFETIME).and_then(|e| e.uint().ok()),
            frequency: f,
            edges: lo.zip(hi),
            pll: pll_sent,
        });
        let mut reply = vec![0u8];
        if ssrc == 0 {
            // The front end: no tag, as observed.
            tlv(&mut reply, ty::OUTPUT_SSRC, &[]);
            tlv(&mut reply, ty::AD_OVER, &[7]);
            tlv(&mut reply, ty::INPUT_SAMPRATE, &64_800_000u32.to_be_bytes());
        } else {
            if let (Some(lo), Some(hi)) = (lo, hi) {
                edges = (lo, hi);
            }
            if let Some(f) = f {
                freq = f;
            }
            if let Some(p) = pll_sent {
                pll = p;
            }
            tlv(&mut reply, ty::OUTPUT_SSRC, &ssrc.to_be_bytes());
            tlv(&mut reply, ty::COMMAND_TAG, &(tag as u32).to_be_bytes());
            tlv(
                &mut reply,
                ty::RADIO_FREQUENCY,
                &freq.to_bits().to_be_bytes(),
            );
            tlv(&mut reply, ty::LOW_EDGE, &edges.0.to_bits().to_be_bytes());
            tlv(&mut reply, ty::HIGH_EDGE, &edges.1.to_bits().to_be_bytes());
            tlv(
                &mut reply,
                ty::BASEBAND_POWER,
                &BASEBAND_DB.to_bits().to_be_bytes(),
            );
            tlv(&mut reply, ty::RTP_PT, &[PT]);
            tlv(&mut reply, ty::OUTPUT_ENCODING, &[2]);
            tlv(&mut reply, ty::OUTPUT_CHANNELS, &[1]);
            tlv(&mut reply, ty::OUTPUT_SAMPRATE, &12_000u32.to_be_bytes());
            tlv(&mut reply, ty::PLL_ENABLE, &[u8::from(pll)]);
            if pll {
                tlv(
                    &mut reply,
                    ty::PLL_LOCK,
                    &[u8::from(locked.load(Ordering::SeqCst))],
                );
            }
        }
        reply.push(ty::EOL);
        let _ = sock.send_to(&reply, dest).await;
    }
}

/// RTP every 20 ms: this channel's PCM, plus another channel's and a wrong payload type to ignore.
/// At sequence 160 the channel goes quiet for 400 ms and comes back 200 lower, as `radiod` does when the
/// channel is re-created (RX-888, 2026-11-05).
async fn fake_rtp(sock: UdpSocket, dest: SocketAddrV4) {
    let mut seq: u16 = 0;
    loop {
        if seq == 160 {
            seq = 60;
            sleep(Duration::from_millis(400)).await;
        }
        for (ssrc, pt) in [(SSRC, PT), (SSRC + 1, PT), (SSRC, 99)] {
            let mut p = vec![0x80, pt];
            p.extend_from_slice(&seq.to_be_bytes());
            p.extend_from_slice(&(u32::from(seq) * 240).to_be_bytes());
            p.extend_from_slice(&ssrc.to_be_bytes());
            for i in 0..240 {
                let v = (16_384.0 * (i as f32 * std::f32::consts::TAU / 12.0).sin()) as i16;
                p.extend_from_slice(&v.to_be_bytes());
            }
            let _ = sock.send_to(&p, dest).await;
        }
        seq = seq.wrapping_add(1);
        sleep(Duration::from_millis(20)).await;
    }
}

async fn eventually(what: &str, mut check: impl FnMut() -> bool) {
    for _ in 0..250 {
        if check() {
            return;
        }
        sleep(Duration::from_millis(20)).await;
    }
    panic!("never happened: {what}");
}

fn observation(reply: Result<Option<Observation>, RigError>) -> Observation {
    match reply {
        Ok(Some(o)) => o,
        other => panic!("no observation: {other:?}"),
    }
}

async fn rig(status: Ipv4Addr, data: Ipv4Addr) -> (RigHandle, RadiodControl) {
    rig_on(status, data, ports()).await
}

async fn rig_on(
    status: Ipv4Addr,
    data: Ipv4Addr,
    (status_port, data_port): (u16, u16),
) -> (RigHandle, RadiodControl) {
    let mut link = RadiodLink::new(status, data, SSRC);
    link.status_port = status_port;
    link.data_port = data_port;
    link.lifetime_frames = 50;
    link.refresh = Duration::from_millis(300);
    link.ack_wait = Duration::from_millis(500);
    link.mode = Mode::Usb;
    link.frequency_hz = 10_000_000;
    let (stream, control) = serve_radiod(&Handle::current(), link).expect("serve");
    let desc = rigfile::parse(include_str!("../../../rigs/radiod-channel.toml"))
        .expect("file")
        .description;
    let mut ro = RigOptions::default();
    ro.poll.enabled = false;
    ro.default_timeout = Duration::from_secs(3);
    let engine = RadiodEngine::new(desc).expect("engine");
    (
        spawn(&Handle::current(), engine, stream, ro).expect("spawn"),
        control,
    )
}

// Tests [@ANCHOR: radioctl_radiod_serve]
// Tests [@ANCHOR: radioctl_radiod_link_new]
// Tests [@ANCHOR: radioctl_radiod_link_validate]
// Tests [@ANCHOR: radioctl_radiod_run_control]
// Tests [@ANCHOR: radioctl_radiod_run_audio]
// Tests [@ANCHOR: radioctl_radiod_stall]
// Tests [@ANCHOR: radioctl_radiod_bridge_send]
// Tests [@ANCHOR: radioctl_radiod_bridge_assert]
// Tests [@ANCHOR: radioctl_radiod_bridge_refresh]
// Tests [@ANCHOR: radioctl_radiod_bridge_handle_line]
// Tests [@ANCHOR: radioctl_radiod_state_commands]
// Tests [@ANCHOR: radioctl_radiod_format_of]
// Tests [@ANCHOR: radioctl_radiod_radiod_mode]
// Tests [@ANCHOR: radioctl_radiod_lock]
// Tests [@ANCHOR: radioctl_radiod_control_take_audio]
// Tests [@ANCHOR: radioctl_radiod_control_stats]
// Tests [@ANCHOR: radioctl_radiod_control_front_end]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_radiod_channel_is_a_receive_only_virtual_rig() {
    let (status, data) = (
        Ipv4Addr::new(239, 255, 88, 1),
        Ipv4Addr::new(239, 255, 88, 2),
    );
    let (status_port, data_port) = ports();
    let seen = Arc::new(Mutex::new(Vec::<Seen>::new()));
    tokio::spawn(fake_radiod(
        join(status, status_port),
        SocketAddrV4::new(status, status_port),
        Arc::clone(&seen),
        Arc::new(AtomicBool::new(true)),
    ));
    tokio::spawn(fake_rtp(
        join(data, data_port),
        SocketAddrV4::new(data, data_port),
    ));

    let (handle, control) = rig(status, data).await;
    let caps = handle.capabilities();
    assert!(caps.iter().all(|c| !c.transmits()), "receive only");
    assert!(!caps.has(Capability::SetPtt) && !caps.has(Capability::ReadPtt));

    // The channel was created at the starting state, with a lifetime.
    eventually("the channel is created", || {
        seen.lock()
            .unwrap()
            .iter()
            .any(|s| s.ssrc == SSRC && s.frequency == Some(10e6))
    })
    .await;

    handle
        .set_frequency(Vfo::A, Hz(7_074_000))
        .await
        .expect("tune");
    let f = handle
        .frequency(Vfo::A, Freshness::Fresh)
        .await
        .expect("read");
    assert_eq!(f.value, Hz(7_074_000));
    handle.set_mode(Mode::Lsb).await.expect("mode");
    assert_eq!(
        handle.mode(Freshness::Fresh).await.expect("mode").value,
        Mode::Lsb
    );
    assert_eq!(
        seen.lock().unwrap().last().and_then(|s| s.edges),
        Some((-3000.0, -50.0)),
        "the mode's own passband"
    );
    handle
        .execute(Op::SetFilterWidth(Hz(2400)))
        .await
        .expect("width");
    assert_eq!(
        seen.lock().unwrap().last().and_then(|s| s.edges),
        Some((-2450.0, -50.0))
    );
    assert_eq!(
        observation(handle.execute(Op::GetFilterWidth).await),
        Observation::FilterWidth(Hz(2400))
    );
    let s = handle.signal(Freshness::Fresh).await.expect("signal");
    assert_eq!(s.value, signal_raw(BASEBAND_DB));

    // Transmit is refused, and so is a frequency outside the policy (10 kHz to 30 MHz).
    assert!(handle.set_ptt(true).await.is_err());
    assert!(handle.set_frequency(Vfo::A, Hz(100_000_000)).await.is_err());
    assert_eq!(
        handle
            .frequency(Vfo::A, Freshness::Fresh)
            .await
            .expect("read")
            .value,
        Hz(7_074_000),
        "a refused tune changes nothing"
    );

    // Audio: this channel's packets only, decoded to 240 mono samples at 12 kHz.
    let mut audio = control.take_audio().expect("audio once");
    assert!(control.take_audio().is_none());
    let frame = tokio::time::timeout(Duration::from_secs(3), audio.recv())
        .await
        .expect("a frame in time")
        .expect("open");
    assert_eq!((frame.samples.len(), frame.rate_hz), (240, 12_000));
    assert!(frame.samples.iter().all(|v| v.abs() <= 0.51));
    eventually("a wrong payload type is counted", || {
        control.stats().rtp_bad > 0
    })
    .await;

    // The front end is read at the refresh.
    eventually("the front end was read", || {
        control.front_end().is_some_and(|fe| fe.ad_over == Some(7))
    })
    .await;

    // Keep-alive: the state is re-asserted without any request from the actor.
    let before = seen.lock().unwrap().len();
    sleep(Duration::from_millis(800)).await;
    let after = seen.lock().unwrap().clone();
    let fresh = &after[before..];
    assert!(
        fresh
            .iter()
            .any(|s| s.ssrc == SSRC && s.frequency == Some(7_074_000.0))
            && fresh.iter().any(|s| s.ssrc == SSRC
                && s.frequency.is_none()
                && s.edges == Some((-2450.0, -50.0))),
        "refresh re-asserted the channel (Audio) and the width (Filter): {fresh:?}"
    );
    // Every command to the channel carried a lifetime; front-end polls carried none.
    for s in after.iter() {
        if s.ssrc == SSRC {
            assert!(s.lifetime.is_some_and(|l| l >= 1), "{s:?}");
        } else {
            assert_eq!((s.ssrc, s.lifetime), (0, None), "{s:?}");
        }
    }
    let stats = control.stats();
    assert!(stats.acks > 5 && stats.audio_frames > 0, "{stats:?}");

    // The channel restart: counted once per restart, with its outage, and the new stream is in order.
    eventually("the restart is counted", || {
        control.stats().rtp_restarts > 0
    })
    .await;
    let stats = control.stats();
    assert_eq!(
        (stats.rtp_lost, stats.rtp_out_of_order),
        (0, 0),
        "{stats:?}"
    );
    assert!(
        stats.rtp_stalls >= 1 && stats.rtp_stall_ms >= 300,
        "{stats:?}"
    );

    // The actor goes away: the channel is removed (lifetime 1).
    drop(handle);
    eventually("the channel is removed", || {
        seen.lock()
            .unwrap()
            .iter()
            .any(|s| s.ssrc == SSRC && s.lifetime == Some(1))
    })
    .await;
}

// Tests [@ANCHOR: radioctl_radiod_control_carrier_lock]
// Tests [@ANCHOR: radioctl_radiod_radiod_mode]
// Tests [@ANCHOR: radioctl_radiod_bridge_handle_line]
// Tests [@ANCHOR: radioctl_radiod_mode_spec_edges]
// Tests [@ANCHOR: radioctl_radiod_sam_passband]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn synchronous_am_turns_radiods_pll_on_and_reports_its_lock() {
    let (status, data) = (
        Ipv4Addr::new(239, 255, 88, 3),
        Ipv4Addr::new(239, 255, 88, 4),
    );
    let pair = ports_at(1);
    let seen = Arc::new(Mutex::new(Vec::<Seen>::new()));
    let locked = Arc::new(AtomicBool::new(true));
    tokio::spawn(fake_radiod(
        join(status, pair.0),
        SocketAddrV4::new(status, pair.0),
        Arc::clone(&seen),
        Arc::clone(&locked),
    ));
    let (handle, control) = rig_on(status, data, pair).await;
    let last = |seen: &Arc<Mutex<Vec<Seen>>>| {
        seen.lock()
            .unwrap()
            .iter()
            .rev()
            .find(|s| s.ssrc == SSRC && s.frequency.is_some())
            .cloned()
    };

    // USB: no PLL, and no lock to report.
    eventually("the channel is created", || last(&seen).is_some()).await;
    assert_eq!(last(&seen).and_then(|s| s.pll), Some(false));
    assert_eq!(control.carrier_lock(), None);

    // SAM: the PLL goes on, the mode's own passband comes back, and the lock is reported.
    handle.set_mode(Mode::Sam).await.expect("sam");
    assert_eq!(
        handle.mode(Freshness::Fresh).await.expect("mode").value,
        Mode::Sam
    );
    let cmd = last(&seen).expect("audio command");
    assert_eq!((cmd.pll, cmd.edges), (Some(true), Some((-5000.0, 5000.0))));
    assert_eq!(control.carrier_lock(), Some(true));

    // The carrier is lost: the next status (a signal read polls the channel) says unlocked.
    locked.store(false, Ordering::SeqCst);
    handle.signal(Freshness::Fresh).await.expect("signal");
    assert_eq!(control.carrier_lock(), Some(false));
    locked.store(true, Ordering::SeqCst);
    handle.signal(Freshness::Fresh).await.expect("signal");
    assert_eq!(control.carrier_lock(), Some(true));

    // SAM-U and SAM-L keep 100 Hz across the carrier, and a width keeps it inside.
    handle.set_mode(Mode::SamU).await.expect("sam-u");
    assert_eq!(last(&seen).and_then(|s| s.edges), Some((-100.0, 5000.0)));
    handle
        .execute(Op::SetFilterWidth(Hz(3000)))
        .await
        .expect("width");
    assert_eq!(
        seen.lock().unwrap().last().and_then(|s| s.edges),
        Some((-100.0, 2900.0))
    );
    handle
        .execute(Op::SetFilterWidth(Hz(50)))
        .await
        .expect("narrow width");
    assert_eq!(
        seen.lock().unwrap().last().and_then(|s| s.edges),
        Some((-100.0, 100.0)),
        "never narrower than 200 Hz, so the carrier is not at an edge"
    );
    handle.set_mode(Mode::SamL).await.expect("sam-l");
    assert_eq!(last(&seen).and_then(|s| s.edges), Some((-5000.0, 100.0)));
    assert_eq!(control.carrier_lock(), Some(true));

    // Back to AM (envelope detection): the PLL is turned off and no lock is reported.
    handle.set_mode(Mode::Am).await.expect("am");
    assert_eq!(last(&seen).and_then(|s| s.pll), Some(false));
    assert_eq!(control.carrier_lock(), None);
    drop(handle);
}

// Tests [@ANCHOR: radioctl_radiod_link_validate]
#[test]
fn links_that_would_let_a_channel_lapse_or_misbehave_are_refused() {
    let g = Ipv4Addr::new(239, 1, 2, 3);
    let ok = RadiodLink::new(g, g, 7);
    assert!(ok.validate().is_ok());
    let mut l = ok.clone();
    l.ssrc = 0;
    assert!(l.validate().is_err());
    let mut l = ok.clone();
    l.ssrc = u32::MAX;
    assert!(l.validate().is_err());
    let mut l = ok.clone();
    l.mode = Mode::Rtty;
    assert!(l.validate().is_err());
    let mut l = ok.clone();
    l.lifetime_frames = 0;
    assert!(l.validate().is_err());
    let mut l = ok.clone();
    l.refresh = Duration::from_secs(4); // lifetime 10 s: more than a third
    assert!(l.validate().is_err());
    let mut l = ok.clone();
    l.refresh = Duration::ZERO;
    assert!(l.validate().is_err());
    let mut l = ok;
    l.audio_queue = 0;
    assert!(l.validate().is_err());
}

#[test]
fn the_engine_refuses_transmit_before_the_bridge_sees_it() {
    let desc = rigfile::parse(include_str!("../../../rigs/radiod-channel.toml"))
        .expect("file")
        .description;
    let mut e = RadiodEngine::new(desc).expect("engine");
    assert!(e.encode(&Op::SetPtt(true)).is_err());
    assert!(e.encode(&Op::SendCw("CQ".into())).is_err());
}
