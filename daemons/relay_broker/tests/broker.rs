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
}

// Tests [@ANCHOR: relay_broker_run]
// Tests [@ANCHOR: relay_broker_client_connect_and_call_as]
// Tests [@ANCHOR: relay_broker_activated_listener]
#[test]
fn the_accept_loop_serves_a_real_listener_filters_protected_devices_and_exits_when_idle() {
    let dir = temp_dir("run");
    let sock = dir.join("broker.sock");
    let listener = UnixListener::bind(&sock).unwrap();
    let mut broker = broker_with(&test_config(), FakeOs::typical(), None);
    let path = sock.clone();
    let client = std::thread::spawn(move || {
        client::connect_and_call_as(
            &path,
            &Request::ListDevices {
                class: Some(DeviceClass::Serial),
            },
            &[nix::unistd::getuid().as_raw()],
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
}

/// An impostor that listens on the broker's path as the test's own account, answers any request
/// with a device list, and records whether it ever received a byte.
fn impostor(sock: &std::path::Path) -> std::thread::JoinHandle<bool> {
    use std::io::Read;
    let listener = UnixListener::bind(sock).unwrap();
    std::thread::spawn(move || {
        let (mut conn, _) = listener.accept().unwrap();
        conn.set_read_timeout(Some(Duration::from_millis(500)))
            .unwrap();
        let mut byte = [0u8; 1];
        // The client verifies and hangs up without sending anything: EOF (0) or a timeout.
        matches!(conn.read(&mut byte), Ok(n) if n > 0)
    })
}

// Tests [@ANCHOR: relay_broker_client_verify_server]
// Tests [@ANCHOR: relay_broker_client_connect_and_call_as]
// Tests [@ANCHOR: relay_broker_client_connect_and_call]
// Tests [@ANCHOR: relay_broker_client_default_server_uids]
#[test]
fn the_client_refuses_a_server_that_is_not_an_expected_account_and_sends_it_nothing() {
    let me = nix::unistd::getuid().as_raw();
    let req = Request::ListDevices { class: None };
    let dir = temp_dir("impostor");
    let sock = dir.join("broker.sock");
    let spy = impostor(&sock);
    // An account that is not the one serving the socket: refused before any byte is sent.
    let err = client::connect_and_call_as(&sock, &req, &[me.wrapping_add(1)]).unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::PermissionDenied, "{err}");
    assert!(
        !spy.join().unwrap(),
        "the impostor must not receive the request"
    );
    // An empty list permits no one.
    let spy = impostor(&dir.join("empty.sock"));
    let err = client::connect_and_call_as(&dir.join("empty.sock"), &req, &[]).unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::PermissionDenied);
    assert!(!spy.join().unwrap());
    // The default expects root: a server run by any other account is refused (when the tests
    // themselves run as root there is no other account to be, so only the named case is checked).
    if me != 0 {
        let spy = impostor(&dir.join("default.sock"));
        let err = client::connect_and_call(&dir.join("default.sock"), &req).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::PermissionDenied);
        assert!(!spy.join().unwrap());
    }
    assert_eq!(client::DEFAULT_SERVER_UIDS, &[0]);
    // The expected account is accepted: verification passes and the request is sent.
    let listener = UnixListener::bind(dir.join("ok.sock")).unwrap();
    let mut broker = broker_with(&test_config(), FakeOs::typical(), None);
    let path = dir.join("ok.sock");
    let worker = std::thread::spawn(move || client::connect_and_call_as(&path, &req, &[me]));
    assert_eq!(
        run(&mut broker, &listener, Duration::from_secs(10), Some(1)).unwrap(),
        1
    );
    assert!(matches!(
        worker.join().unwrap().unwrap().0,
        Response::Devices(_)
    ));
}

// Tests [@ANCHOR: relay_broker_broker_bind_udp]
#[test]
fn bind_udp_serves_only_listed_ports_and_passes_a_descriptor() {
    let os = FakeOs::typical();
    let binds = os.binds.clone();
    let peer = os.peer;
    let mut broker = broker_with(&test_config(), os, None);
    let now = Instant::now();
    let (resp, fd) = broker.handle(peer, &Request::BindUdp { port: 123 }, now);
    assert_eq!(resp, Response::Opened);
    assert!(fd.is_some(), "the descriptor is passed");
    for port in [22, 80, 0, 1123] {
        let (resp, fd) = broker.handle(peer, &Request::BindUdp { port }, now);
        assert!(
            matches!(
                resp,
                Response::Error {
                    status: Status::Denied,
                    ..
                }
            ) && fd.is_none(),
            "port {port}: {resp:?}"
        );
    }
    assert_eq!(
        *binds.lock().unwrap(),
        vec![123],
        "only the listed port reached the OS"
    );
}

fn hub_settings() -> relay_broker::hub::HubSettings {
    relay_broker::hub::HubSettings {
        interface: "wlan0".into(),
        ssid: "HAMS-EMERGENCY".into(),
        passphrase: "correct horse".into(),
        channel: 6,
        five_ghz: false,
        hub_address: std::net::Ipv4Addr::new(192, 168, 50, 1),
        relay_name: "relay".into(),
        uplink_dns: None,
        lease_minutes: 60,
    }
}

// Tests [@ANCHOR: relay_broker_broker_hub_start]
// Tests [@ANCHOR: relay_broker_broker_hub_stop]
#[test]
fn the_hub_is_refused_unless_enabled_and_then_written_from_the_templates() {
    let now = Instant::now();
    // Off by default.
    let os = FakeOs::typical();
    let peer = os.peer;
    let mut broker = broker_with(&test_config(), os, None);
    let (resp, _) = broker.handle(peer, &Request::HubStart(hub_settings()), now);
    assert!(
        matches!(
            resp,
            Response::Error {
                status: Status::Denied,
                ..
            }
        ),
        "{resp:?}"
    );
    // Enabled.
    let mut cfg = test_config();
    cfg.hub_enabled = true;
    let os = FakeOs::typical();
    let hub = os.hub.clone();
    let mut broker = broker_with(&cfg, os, None);
    let (resp, _) = broker.handle(peer, &Request::HubStart(hub_settings()), now);
    assert_eq!(
        resp,
        Response::Hub {
            access_point: true,
            dhcp: true
        }
    );
    let started = hub.lock().unwrap().clone().expect("started");
    assert!(
        started.0.contains("wpa_passphrase=correct horse")
            && started.1.contains("dhcp-option=42,192.168.50.1")
    );
    // A setting that would add a line or open the network never reaches the OS.
    let mut bad = hub_settings();
    bad.ssid = "x\nwpa=0".into();
    let (resp, _) = broker.handle(peer, &Request::HubStart(bad), now);
    assert!(
        matches!(
            resp,
            Response::Error {
                status: Status::Denied,
                ..
            }
        ),
        "{resp:?}"
    );
    let (resp, _) = broker.handle(peer, &Request::HubStatus, now);
    assert_eq!(
        resp,
        Response::Hub {
            access_point: true,
            dhcp: true
        }
    );
    let (resp, _) = broker.handle(peer, &Request::HubStop, now);
    assert_eq!(
        resp,
        Response::Hub {
            access_point: false,
            dhcp: false
        }
    );
    assert!(hub.lock().unwrap().is_none());
}

// Tests [@ANCHOR: relay_broker_broker_update_package]
#[test]
fn update_package_runs_only_when_the_config_enables_it() {
    let peer = Peer {
        uid: HAM_UID,
        gid: 1000,
        pid: 4242,
    };
    let now = Instant::now();
    // Off by default: refused, the package manager is never asked.
    let os = FakeOs::typical();
    let updates = os.updates.clone();
    let mut broker = broker_with(&test_config(), os, None);
    let (resp, _) = broker.handle(peer, &Request::UpdatePackage, now);
    assert!(
        matches!(
            resp,
            Response::Error {
                status: Status::Denied,
                ..
            }
        ),
        "{resp:?}"
    );
    assert_eq!(updates.load(std::sync::atomic::Ordering::SeqCst), 0);
    // Enabled: one fixed upgrade per request.
    let mut cfg = test_config();
    cfg.update_package = true;
    let os = FakeOs::typical();
    let updates = os.updates.clone();
    let mut broker = broker_with(&cfg, os, None);
    let (resp, _) = broker.handle(peer, &Request::UpdatePackage, now);
    assert_eq!(resp, Response::Updated);
    assert_eq!(updates.load(std::sync::atomic::Ordering::SeqCst), 1);
}

// Tests [@ANCHOR: relay_broker_broker_set_system_time]
// Tests [@ANCHOR: relay_broker_broker_time_status]
// Tests [@ANCHOR: relay_broker_os_system_time]
#[test]
fn the_clock_is_stepped_only_when_enabled_and_inside_the_ceilings() {
    use relay_broker::wire::TimeSource;
    let peer = Peer {
        uid: HAM_UID,
        gid: 1000,
        pid: 4242,
    };
    let now = Instant::now();
    let step = |target: u64, source: TimeSource, agreeing: u8| Request::SetSystemTime {
        target_unix_secs: target,
        source,
        agreeing,
    };
    let denied = |r: &Response| {
        matches!(
            r,
            Response::Error {
                status: Status::Denied,
                ..
            }
        )
    };
    // Off by default: refused and the clock untouched; the read-only status is always served.
    let os = FakeOs::typical();
    let clock = os.clock.clone();
    let mut broker = broker_with(&test_config(), os, None);
    let (resp, _) = broker.handle(peer, &step(1_793_500_001, TimeSource::Gps, 0), now);
    assert!(denied(&resp), "{resp:?}");
    assert_eq!(
        clock.load(std::sync::atomic::Ordering::SeqCst),
        1_793_500_000
    );
    let (resp, _) = broker.handle(peer, &Request::TimeStatus, now);
    assert!(
        matches!(
            resp,
            Response::TimeStatus {
                now_unix_secs: 1_793_500_000,
                breaker_open: false,
                ..
            }
        ),
        "{resp:?}"
    );

    // Enabled: a small step, a GPS-proven large step, an unproven large step and a pre-build target.
    let mut cfg = test_config();
    cfg.set_time = true;
    cfg.max_time_steps_per_hour = 2;
    let os = FakeOs::typical();
    let clock = os.clock.clone();
    let fails = os.clock_fails.clone();
    let audit_dir = temp_dir("time");
    let audit_path = audit_dir.join("events.jsonl");
    let mut broker = broker_with(&cfg, os, Some(&audit_path));
    let (resp, _) = broker.handle(peer, &step(1_793_500_010, TimeSource::Network, 1), now);
    assert_eq!(
        resp,
        Response::TimeSet {
            before_unix_secs: 1_793_500_000,
            after_unix_secs: 1_793_500_010
        }
    );
    let (resp, _) = broker.handle(peer, &step(1_793_600_000, TimeSource::Network, 1), now);
    assert!(denied(&resp), "unproven large step: {resp:?}");
    let (resp, _) = broker.handle(peer, &step(1_000, TimeSource::Gps, 3), now);
    assert!(denied(&resp), "before the build date: {resp:?}");
    assert_eq!(
        clock.load(std::sync::atomic::Ordering::SeqCst),
        1_793_500_010
    );
    let (resp, _) = broker.handle(peer, &step(1_793_600_000, TimeSource::Gps, 0), now);
    assert!(matches!(resp, Response::TimeSet { .. }), "{resp:?}");
    // The hour's two steps are used: the next trips the breaker, and the status says so.
    let (resp, _) = broker.handle(peer, &step(1_793_600_001, TimeSource::Gps, 0), now);
    assert!(denied(&resp), "{resp:?}");
    let (resp, _) = broker.handle(peer, &Request::TimeStatus, now);
    assert!(
        matches!(
            resp,
            Response::TimeStatus {
                steps_last_hour: 2,
                breaker_open: true,
                ..
            }
        ),
        "{resp:?}"
    );
    // An OS failure is Failed, not Denied, and is not counted as a step.
    let os = FakeOs::typical();
    os.clock_fails
        .store(true, std::sync::atomic::Ordering::SeqCst);
    let mut cfg = test_config();
    cfg.set_time = true;
    let mut broker = broker_with(&cfg, os, None);
    let (resp, _) = broker.handle(peer, &step(1_793_500_001, TimeSource::Gps, 0), now);
    assert!(
        matches!(
            resp,
            Response::Error {
                status: Status::Failed,
                ..
            }
        ),
        "{resp:?}"
    );
    let (resp, _) = broker.handle(peer, &Request::TimeStatus, now);
    assert!(
        matches!(
            resp,
            Response::TimeStatus {
                steps_last_hour: 0,
                ..
            }
        ),
        "{resp:?}"
    );
    let _ = fails;
    // Every attempt is in the audit log with its evidence.
    let log = std::fs::read_to_string(&audit_path).expect("audit");
    assert!(
        log.contains("set_system_time") && log.contains("source=Gps") && log.contains("breaker"),
        "{log}"
    );
}
