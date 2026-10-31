// SPDX-License-Identifier: AGPL-3.0-or-later
//! End-to-end tests over socketpairs with the fake OS: peer rejection, policy denials, the
//! descriptor round trip, oversize and malformed frames, rate limiting, provisioning dry runs,
//! the audit log, and the accept loop with a real listener. Nothing here needs root; the
//! root-only paths (a real `/dev/ttyUSB0`, a real group change) go through the `Os` trait and
//! are covered by the fake, as the README documents.
#![allow(clippy::expect_used, clippy::unwrap_used)]

mod common;

use std::io::Write;
use std::os::fd::AsFd;
use std::os::unix::net::{UnixListener, UnixStream};
use std::time::{Duration, Instant};

use common::{broker_with, temp_dir, test_config, FakeOs, HAM_UID};
use relay_broker::client;
use relay_broker::os::{NodeKind, Os, Peer};
use relay_broker::policy::Denial;
use relay_broker::server::{run, serve_connection, Outcome};
use relay_broker::transport::recv_frame;
use relay_broker::wire::{DeviceClass, Group, Request, Response, Status, MAX_FRAME};

fn open(path: &str, class: DeviceClass) -> Request {
    Request::OpenDevice {
        class,
        read_only: false,
        path: path.into(),
    }
}

fn denied(resp: &Response, d: Denial) -> bool {
    matches!(resp, Response::Error { status: Status::Denied, reason } if reason == d.reason())
}

/// Serves exactly one connection on a socketpair and returns the client end and the outcome.
fn one_shot(
    mut broker: relay_broker::broker::Broker,
    req: &Request,
) -> (Response, Option<std::os::fd::OwnedFd>, Outcome) {
    let (client_end, server_end) = UnixStream::pair().unwrap();
    let t = std::thread::spawn(move || serve_connection(&mut broker, &server_end));
    let result = client::call(&client_end, req);
    let outcome = t.join().unwrap();
    let (resp, fd) = result.expect("call");
    (resp, fd, outcome)
}

// Tests [@ANCHOR: relay_broker_serve_connection]
// Tests [@ANCHOR: relay_broker_broker_new]
// Tests [@ANCHOR: relay_broker_broker_os]
#[test]
fn an_unlisted_peer_is_closed_before_any_byte_is_read() {
    let mut os = FakeOs::typical();
    os.peer = Peer {
        uid: 4242,
        gid: 4242,
        pid: 1,
    };
    let dir = temp_dir("peer");
    let log = dir.join("events.jsonl");
    let mut broker = broker_with(&test_config(), os, Some(&log));
    assert_eq!(broker.os().username(HAM_UID).as_deref(), Some("ham"));
    let (mut client_end, server_end) = UnixStream::pair().unwrap();
    client_end
        .write_all(&Request::ProvisionGroups.encode())
        .unwrap();
    assert_eq!(
        serve_connection(&mut broker, &server_end),
        Outcome::PeerRejected
    );
    drop(server_end);
    assert!(recv_frame(client_end.as_fd()).is_err(), "no answer at all");
    let text = std::fs::read_to_string(&log).unwrap();
    assert!(
        text.contains("\"action\":\"peer_rejected\"") && text.contains("\"uid\":4242"),
        "{text}"
    );
    // The same peer by GID is let in.
    let mut cfg = test_config();
    cfg.allow.gids.push(4242);
    let mut os = FakeOs::typical();
    os.peer.uid = 4242;
    os.peer.gid = 4242;
    let (resp, _, outcome) = one_shot(
        broker_with(&cfg, os, None),
        &Request::ListDevices { class: None },
    );
    assert_eq!(outcome, Outcome::Answered);
    assert!(matches!(resp, Response::Devices(_)));
    std::fs::remove_dir_all(dir).unwrap();
}

// Tests [@ANCHOR: relay_broker_broker_handle]
// Tests [@ANCHOR: relay_broker_broker_open_device]
#[test]
fn policy_refuses_disks_protected_devices_traversal_and_symlink_games() {
    let mut broker = broker_with(&test_config(), FakeOs::typical(), None);
    let peer = FakeOs::typical().peer;
    let now = Instant::now();
    let cases: Vec<(Request, Denial)> = vec![
        (open("/dev/sda", DeviceClass::Serial), Denial::NotCharDevice),
        (
            open("/dev/ttyUSB1", DeviceClass::Serial),
            Denial::ProtectedUsb,
        ),
        (
            open("/dev/../etc/passwd", DeviceClass::Serial),
            Denial::Traversal,
        ),
        (
            open("/dev/snd/../sda", DeviceClass::Audio),
            Denial::Traversal,
        ),
        (
            open("/etc/passwd", DeviceClass::Serial),
            Denial::OutsideRoot,
        ),
        (
            open("dev/ttyUSB0", DeviceClass::Serial),
            Denial::NotAbsolute,
        ),
        (open("/dev/stdin", DeviceClass::Serial), Denial::Symlink),
        (open("/dev/ttyUSB7", DeviceClass::Serial), Denial::Symlink),
        (open("/dev/vnwa", DeviceClass::Audio), Denial::Symlink),
        (
            open("/dev/serial/by-id/usb-FTDI-if00-port0", DeviceClass::Serial),
            Denial::Symlink,
        ),
        (open("/dev/mem", DeviceClass::Serial), Denial::DenyListed),
        (open("/dev/ttyUSB0", DeviceClass::Audio), Denial::WrongClass),
        (
            open("/dev/snd/pcmC1D0c", DeviceClass::Serial),
            Denial::WrongClass,
        ),
        (
            open("/dev/ttyUSB9", DeviceClass::Serial),
            Denial::NotCharDevice,
        ),
        (
            open(&format!("/dev/{}", "x".repeat(260)), DeviceClass::Serial),
            Denial::TooLong,
        ),
    ];
    for (req, want) in cases {
        let (resp, fd) = broker.handle(peer, &req, now);
        assert!(
            denied(&resp, want),
            "{req:?}: got {resp:?}, wanted {want:?}"
        );
        assert!(fd.is_none());
    }
    // A node that changes between the walk and the open is caught by the post-open check.
    let mut os = FakeOs::typical();
    os.nodes.insert(
        "/dev/ttyACM0".into(),
        NodeKind::Char {
            major: 166,
            minor: 0,
        },
    );
    let mut broker = broker_with(&test_config(), os, None);
    let (resp, fd) = broker.handle(peer, &open("/dev/ttyACM0", DeviceClass::Serial), now);
    assert_eq!(resp, Response::Opened);
    assert!(fd.is_some());
}

// Tests [@ANCHOR: relay_broker_client_call]
// Tests [@ANCHOR: relay_broker_policy_check_fd]
#[test]
fn an_allowed_open_passes_a_working_descriptor_and_never_the_path() {
    let broker = broker_with(&test_config(), FakeOs::typical(), None);
    let (resp, fd, outcome) = one_shot(broker, &open("/dev/ttyUSB0", DeviceClass::Serial));
    assert_eq!(outcome, Outcome::Answered);
    assert_eq!(resp, Response::Opened);
    let fd = fd.expect("descriptor passed");
    let kind = relay_broker::os::LinuxOs::default().fd_kind(fd.as_fd());
    assert_eq!(
        kind,
        NodeKind::Char { major: 1, minor: 3 },
        "the fake opened /dev/null"
    );
    let mut file = std::fs::File::from(fd);
    file.write_all(b"data to the device").expect("writable");
    assert!(
        !resp.encode().windows(4).any(|w| w == b"/dev"),
        "no path in the answer"
    );
    // A read-only open of an audio node works the same way.
    let broker = broker_with(&test_config(), FakeOs::typical(), None);
    let req = Request::OpenDevice {
        class: DeviceClass::Audio,
        read_only: true,
        path: "/dev/snd/pcmC1D0c".into(),
    };
    let (resp, fd, _) = one_shot(broker, &req);
    assert_eq!(resp, Response::Opened);
    assert!(fd.is_some());
}

// Tests [@ANCHOR: relay_broker_recv_frame]
#[test]
fn oversize_and_malformed_frames_are_refused_without_a_panic() {
    let peer_ok = |b: &mut relay_broker::broker::Broker, raw: &[u8]| {
        let (mut client_end, server_end) = UnixStream::pair().unwrap();
        client_end.write_all(raw).unwrap();
        let outcome = serve_connection(b, &server_end);
        drop(server_end);
        (
            outcome,
            recv_frame(client_end.as_fd())
                .ok()
                .map(|(body, _)| Response::decode(&body)),
        )
    };
    let mut broker = broker_with(&test_config(), FakeOs::typical(), None);
    // Length above MAX_FRAME: dropped after four bytes.
    let mut huge = ((MAX_FRAME + 1) as u32).to_be_bytes().to_vec();
    huge.extend_from_slice(&[1u8; 64]);
    assert_eq!(peer_ok(&mut broker, &huge).0, Outcome::IoError);
    assert_eq!(
        peer_ok(&mut broker, &0xffff_ffffu32.to_be_bytes()).0,
        Outcome::IoError
    );
    assert_eq!(peer_ok(&mut broker, &[0, 0, 0, 0]).0, Outcome::IoError);
    // Garbage body: Malformed answer.
    let (outcome, resp) = peer_ok(&mut broker, &[0, 0, 0, 3, 1, 99, 0]);
    assert_eq!(outcome, Outcome::Malformed);
    assert!(
        matches!(
            resp,
            Some(Ok(Response::Error {
                status: Status::Malformed,
                ..
            }))
        ),
        "{resp:?}"
    );
    // Wrong version: UnsupportedVersion answer.
    let (outcome, resp) = peer_ok(&mut broker, &[0, 0, 0, 3, 2, 1, 0]);
    assert_eq!(outcome, Outcome::Malformed);
    assert!(matches!(
        resp,
        Some(Ok(Response::Error {
            status: Status::UnsupportedVersion,
            ..
        }))
    ));
    // Trailing bytes after a valid request.
    let mut body = Request::ProvisionGroups.encode();
    body.push(0);
    let mut raw = (body.len() as u32).to_be_bytes().to_vec();
    raw.extend_from_slice(&body);
    assert_eq!(peer_ok(&mut broker, &raw).0, Outcome::Malformed);
    // A peer that connects and sends nothing times out, it does not hang the broker.
    let (client_end, server_end) = UnixStream::pair().unwrap();
    let started = Instant::now();
    assert_eq!(serve_connection(&mut broker, &server_end), Outcome::IoError);
    assert!(started.elapsed() < Duration::from_secs(30));
    drop(client_end);
}

// Tests [@ANCHOR: relay_broker_rate_limiter_allow]
#[test]
fn a_peer_over_its_rate_is_refused_and_logged() {
    let mut cfg = test_config();
    cfg.rate_per_minute = 2;
    let dir = temp_dir("rate");
    let log = dir.join("events.jsonl");
    let mut broker = broker_with(&cfg, FakeOs::typical(), Some(&log));
    let peer = FakeOs::typical().peer;
    let now = Instant::now();
    let req = Request::ListDevices { class: None };
    assert!(matches!(
        broker.handle(peer, &req, now).0,
        Response::Devices(_)
    ));
    assert!(matches!(
        broker.handle(peer, &req, now).0,
        Response::Devices(_)
    ));
    let (resp, _) = broker.handle(peer, &req, now);
    assert!(
        matches!(
            resp,
            Response::Error {
                status: Status::RateLimited,
                ..
            }
        ),
        "{resp:?}"
    );
    let later = now + Duration::from_secs(31);
    assert!(matches!(
        broker.handle(peer, &req, later).0,
        Response::Devices(_)
    ));
    let text = std::fs::read_to_string(&log).unwrap();
    assert_eq!(
        text.matches("\"outcome\":\"rate_limited\"").count(),
        1,
        "{text}"
    );
    std::fs::remove_dir_all(dir).unwrap();
}

// Tests [@ANCHOR: relay_broker_broker_grant]
// Tests [@ANCHOR: relay_broker_broker_log]
#[test]
fn provisioning_reports_by_default_and_applies_only_when_configured() {
    let os = FakeOs::typical();
    let added = os.added.clone();
    let peer = os.peer;
    let now = Instant::now();
    let dir = temp_dir("grant");
    let log = dir.join("events.jsonl");
    let mut broker = broker_with(&test_config(), os, Some(&log));
    let (resp, _) = broker.handle(peer, &Request::ProvisionGroups, now);
    assert_eq!(
        resp,
        Response::GroupReport {
            applied: false,
            user: "ham".into(),
            groups: vec!["dialout".into(), "audio".into()],
        }
    );
    let (resp, _) = broker.handle(
        peer,
        &Request::GrantGroup {
            group: Group::Audio,
        },
        now,
    );
    assert_eq!(
        resp,
        Response::GroupReport {
            applied: false,
            user: "ham".into(),
            groups: vec!["audio".into()],
        }
    );
    assert!(added.lock().unwrap().is_empty(), "dry run changed nothing");
    let text = std::fs::read_to_string(&log).unwrap();
    assert_eq!(text.matches("\"outcome\":\"dry_run\"").count(), 2, "{text}");

    let mut cfg = test_config();
    cfg.apply_groups = true;
    let mut os = FakeOs::typical();
    os.groups.insert("audio".into(), vec!["ham".into()]);
    let added = os.added.clone();
    let mut broker = broker_with(&cfg, os, None);
    let (resp, _) = broker.handle(peer, &Request::ProvisionGroups, now);
    assert_eq!(
        resp,
        Response::GroupReport {
            applied: true,
            user: "ham".into(),
            groups: vec!["dialout".into()],
        }
    );
    assert_eq!(
        *added.lock().unwrap(),
        vec![("ham".to_string(), "dialout".to_string())]
    );
    // A peer whose uid has no account gets Failed, never a guess.
    let mut os = FakeOs::typical();
    os.users.clear();
    let mut broker = broker_with(&cfg, os, None);
    let (resp, _) = broker.handle(peer, &Request::ProvisionGroups, now);
    assert!(matches!(
        resp,
        Response::Error {
            status: Status::Failed,
            ..
        }
    ));
    std::fs::remove_dir_all(dir).unwrap();
}

// Tests [@ANCHOR: relay_broker_run]
// Tests [@ANCHOR: relay_broker_client_connect_and_call]
// Tests [@ANCHOR: relay_broker_activated_listener]
#[test]
fn the_accept_loop_serves_a_real_listener_filters_protected_devices_and_exits_when_idle() {
    let dir = temp_dir("run");
    let sock = dir.join("broker.sock");
    let listener = UnixListener::bind(&sock).unwrap();
    let mut broker = broker_with(&test_config(), FakeOs::typical(), None);
    let path = sock.clone();
    let client = std::thread::spawn(move || {
        client::connect_and_call(
            &path,
            &Request::ListDevices {
                class: Some(DeviceClass::Serial),
            },
        )
    });
    let served = run(&mut broker, &listener, Duration::from_secs(10), Some(1)).unwrap();
    assert_eq!(served, 1);
    let (resp, fd) = client.join().unwrap().unwrap();
    assert!(fd.is_none());
    match resp {
        Response::Devices(d) => {
            assert_eq!(
                d.len(),
                1,
                "the VNWA codec is filtered out of the listing: {d:?}"
            );
            assert_eq!(d[0].path, "/dev/ttyUSB0");
        }
        other => panic!("{other:?}"),
    }
    let started = Instant::now();
    assert_eq!(
        run(&mut broker, &listener, Duration::from_millis(200), None).unwrap(),
        0
    );
    assert!(
        started.elapsed() >= Duration::from_millis(150),
        "idle exit after the timeout"
    );
    // Without systemd's environment there is no activated listener.
    assert!(relay_broker::server::activated_listener().is_none());
    std::fs::remove_dir_all(dir).unwrap();
}
