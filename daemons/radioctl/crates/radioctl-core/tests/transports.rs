// SPDX-License-Identifier: AGPL-3.0-or-later
//! The real transports against pseudo-terminal pairs and loopback sockets: serial bytes in both
//! directions, a virtual KX3 behind a pty driven by the rig actor, hot-plug reconnect with
//! backoff, bounded failures, line keying behind the transmit gate, and TCP. No radio, no
//! transmitter: the keying tests use a recording set of lines.
#![allow(clippy::expect_used, clippy::panic)]

use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use radioctl_core::backoff::Backoff;
use radioctl_core::description::RigDescription;
use radioctl_core::error::{ErrorKind, RigError};
use radioctl_core::keyer::{ControlLines, KeyLine, LineKeyer};
use radioctl_core::rig::{spawn, Freshness, RigOptions, TransmitPermit};
use radioctl_core::rigfile;
use radioctl_core::serial::{
    open_serial, resolve, LineControl, LinkStatus, PortTarget, SerialConfig,
};

use radioctl_core::tcp::{connect_tcp, TcpConfig};
use radioctl_core::units::{Hz, Vfo};
use radioctl_core::usb::{list_ports, list_sysfs, PortInfo, UsbMatcher};
use radioctl_protocols::elecraft::ElecraftEngine;
use radioctl_protocols::virtual_rig::VirtualRig;
use serialport::{SerialPort, TTYPort};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::runtime::Handle;
use tokio::time::{sleep, timeout};

fn kx3() -> RigDescription {
    rigfile::parse(include_str!("../../../rigs/elecraft-kx3.toml"))
        .expect("kx3 file")
        .description
}

/// A pty pair: the master plays the radio side, the slave path is what a rig would be on.
fn pty() -> (TTYPort, PathBuf) {
    let (master, slave) = TTYPort::pair().expect("pty pair");
    let path = PathBuf::from(slave.name().expect("slave name"));
    drop(slave);
    (master, path)
}

fn fast_config(path: &Path) -> SerialConfig {
    let mut config = SerialConfig::new(PortTarget::Path(path.to_path_buf()), 38400);
    config.lines = LineControl::Unmanaged;
    config.backoff = Backoff {
        initial: Duration::from_millis(20),
        max: Duration::from_millis(80),
        factor: 2,
    };
    config
}

/// Reads from the master until `want` bytes arrived or two seconds passed.
fn master_read(master: &mut TTYPort, want: usize) -> Vec<u8> {
    master
        .set_timeout(Duration::from_millis(100))
        .expect("timeout");
    let mut out = Vec::new();
    let mut buffer = [0u8; 64];
    for _ in 0..20 {
        if out.len() >= want {
            break;
        }
        if let Ok(n) = master.read(&mut buffer) {
            out.extend_from_slice(buffer.get(..n).unwrap_or(&[]));
        }
    }
    out
}

async fn read_exactly(link: &mut tokio::io::DuplexStream, want: usize) -> Vec<u8> {
    let mut out = vec![0u8; want];
    timeout(Duration::from_secs(3), link.read_exact(&mut out))
        .await
        .expect("bytes arrive in time")
        .expect("read");
    out
}

// Tests [@ANCHOR: radioctl_core_serial_open]
// Tests [@ANCHOR: radioctl_core_serial_open_port]
// Tests [@ANCHOR: radioctl_core_serial_resolve]
// Tests [@ANCHOR: radioctl_core_serial_config_new]
// Tests [@ANCHOR: radioctl_core_serial_supervisor_run]
// Tests [@ANCHOR: radioctl_core_serial_session]
// Tests [@ANCHOR: radioctl_core_serial_read_loop]
// Tests [@ANCHOR: radioctl_core_serial_write_fully]
// Tests [@ANCHOR: radioctl_core_serial_link_status]
// Tests [@ANCHOR: radioctl_core_serial_link_stats]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn bytes_flow_both_ways_over_a_pty() {
    let (mut master, path) = pty();
    let mut link = open_serial(&Handle::current(), fast_config(&path))
        .await
        .expect("open");
    assert!(matches!(
        link.status(),
        LinkStatus::Connected { generation: 1, .. }
    ));
    master.write_all(b"FA00014060000;").expect("master write");
    assert_eq!(
        read_exactly(&mut link.transport, 14).await,
        b"FA00014060000;"
    );
    link.transport.write_all(b"FA;").await.expect("write");
    let heard = tokio::task::spawn_blocking(move || master_read(&mut master, 3))
        .await
        .expect("join");
    assert_eq!(heard, b"FA;");
    let stats = link.stats();
    assert_eq!((stats.bytes_in, stats.bytes_out), (14, 3));
    assert_eq!(
        resolve(&PortTarget::Path(path.clone())).expect("path"),
        path
    );
}

// Tests [@ANCHOR: radioctl_core_serial_open]
// Tests [@ANCHOR: radioctl_core_rig_spawn]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_rig_actor_reads_a_virtual_kx3_through_a_pty() {
    let (mut master, path) = pty();
    let desc = kx3();
    let mut radio = VirtualRig::from_description(&desc).expect("virtual rig");
    let done = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let done_in_thread = Arc::clone(&done);
    let radio_thread = std::thread::spawn(move || {
        master
            .set_timeout(Duration::from_millis(50))
            .expect("timeout");
        let mut buffer = [0u8; 128];
        let started = std::time::Instant::now();
        while !done_in_thread.load(std::sync::atomic::Ordering::Acquire)
            && started.elapsed() < Duration::from_secs(8)
        {
            if let Ok(n) = master.read(&mut buffer) {
                let reply = radio.feed(buffer.get(..n).unwrap_or(&[]), started.elapsed());
                if !reply.is_empty() {
                    master.write_all(&reply).expect("reply");
                }
            }
        }
    });
    let link = open_serial(&Handle::current(), fast_config(&path))
        .await
        .expect("open");
    let mut options = RigOptions::default();
    options.poll.enabled = false;
    let engine = ElecraftEngine::new(desc).expect("engine");
    let rig = spawn(&Handle::current(), engine, link.transport, options).expect("spawn");
    let read = rig.frequency(Vfo::A, Freshness::Fresh).await.expect("read");
    assert_eq!(read.value, Hz(14_060_000));
    rig.set_frequency(Vfo::A, Hz(7_030_000)).await.expect("set");
    let back = rig.frequency(Vfo::A, Freshness::Fresh).await.expect("read");
    assert_eq!(back.value, Hz(7_030_000));
    assert_eq!(rig.stats().commands_failed, 0);
    drop(rig);
    done.store(true, std::sync::atomic::Ordering::Release);
    radio_thread.join().expect("radio thread");
}

// Tests [@ANCHOR: radioctl_core_serial_open]
// Tests [@ANCHOR: radioctl_core_serial_sleep_dropping]
// Tests [@ANCHOR: radioctl_core_serial_link_watch]
// Tests [@ANCHOR: radioctl_core_backoff_delay]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_vanished_port_is_reopened_with_backoff_and_bytes_in_between_are_dropped() {
    let dir =
        Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("hotplug-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("dir");
    let link_path = dir.join("rig");
    let relink = |target: &Path| {
        let _ = std::fs::remove_file(&link_path);
        std::os::unix::fs::symlink(target, &link_path).expect("symlink");
    };
    let (first_master, first_path) = pty();
    relink(&first_path);
    let mut link = open_serial(&Handle::current(), fast_config(&link_path))
        .await
        .expect("open");
    let mut watch = link.watch();
    // Unplug: the master goes away, the slave hangs up, the supervisor starts retrying.
    drop(first_master);
    timeout(Duration::from_secs(3), async {
        loop {
            if matches!(*watch.borrow_and_update(), LinkStatus::Reconnecting { .. }) {
                break;
            }
            watch.changed().await.expect("status");
        }
    })
    .await
    .expect("noticed the unplug");
    link.transport
        .write_all(b"lost;")
        .await
        .expect("write while down");
    sleep(Duration::from_millis(100)).await;
    assert!(link.stats().bytes_dropped >= 5, "{:?}", link.stats());
    // Plug in again, at a different device node.
    let (mut second_master, second_path) = pty();
    relink(&second_path);
    timeout(Duration::from_secs(5), async {
        loop {
            if matches!(
                *watch.borrow_and_update(),
                LinkStatus::Connected { generation: 2, .. }
            ) {
                break;
            }
            watch.changed().await.expect("status");
        }
    })
    .await
    .expect("reconnected");
    assert_eq!(link.stats().reconnects, 1);
    second_master.write_all(b"MD3;").expect("master write");
    assert_eq!(read_exactly(&mut link.transport, 4).await, b"MD3;");
    link.transport.write_all(b"MD;").await.expect("write");
    let heard = tokio::task::spawn_blocking(move || master_read(&mut second_master, 3))
        .await
        .expect("join");
    assert_eq!(heard, b"MD;");
    drop(link.transport);
    timeout(Duration::from_secs(3), async {
        while !matches!(*watch.borrow_and_update(), LinkStatus::Closed) {
            watch.changed().await.expect("status");
        }
    })
    .await
    .expect("closed after the stream was dropped");
    let _ = std::fs::remove_dir_all(&dir);
}

// Tests [@ANCHOR: radioctl_core_serial_open_port]
// Tests [@ANCHOR: radioctl_core_serial_control_set_line]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_port_without_modem_lines_cannot_be_opened_as_managed_and_is_honest_when_unmanaged() {
    let (_master, path) = pty();
    let mut managed = fast_config(&path);
    managed.lines = LineControl::Managed;
    let error = open_serial(&Handle::current(), managed)
        .await
        .err()
        .expect("a pty has no DTR");
    assert_eq!(error.kind, ErrorKind::LinkDown);
    assert!(error.detail.contains("DTR"), "{}", error.detail);
    let link = open_serial(&Handle::current(), fast_config(&path))
        .await
        .expect("unmanaged");
    let refused = link
        .control
        .set_line(KeyLine::Rts, true)
        .expect_err("no lines");
    assert_eq!(refused.kind, ErrorKind::Unsupported);
}

// Tests [@ANCHOR: radioctl_core_serial_open]
// Tests [@ANCHOR: radioctl_core_serial_resolve]
// Tests [@ANCHOR: radioctl_core_usb_select_port]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn opening_what_is_not_there_fails_after_the_stated_attempts_with_a_typed_error() {
    let mut config = fast_config(Path::new("/dev/does-not-exist-radioctl"));
    config.first_open_attempts = 3;
    let started = std::time::Instant::now();
    let error = open_serial(&Handle::current(), config)
        .await
        .err()
        .expect("must fail");
    assert_eq!(error.kind, ErrorKind::LinkDown);
    assert!(started.elapsed() >= Duration::from_millis(60));
    assert!(started.elapsed() < Duration::from_secs(3));
    let usb = SerialConfig::new(
        PortTarget::Usb(UsbMatcher {
            vid: 0xFFFF,
            pid: 0xFFFF,
            serial: None,
        }),
        9600,
    );
    let error = open_serial(&Handle::current(), usb)
        .await
        .err()
        .expect("must fail");
    assert_eq!(error.kind, ErrorKind::LinkDown);
    assert!(error.detail.contains("ffff:ffff"), "{}", error.detail);
}

#[derive(Clone, Default)]
struct Lines(Arc<Mutex<Vec<(KeyLine, bool)>>>);

impl ControlLines for Lines {
    fn set_line(&self, line: KeyLine, asserted: bool) -> Result<(), RigError> {
        self.0.lock().expect("lock").push((line, asserted));
        Ok(())
    }
}

// Tests [@ANCHOR: radioctl_core_keyer_new]
// Tests [@ANCHOR: radioctl_core_keyer_key]
// Tests [@ANCHOR: radioctl_core_keyer_release]
// Tests [@ANCHOR: radioctl_core_serial_control_set_line]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn line_keying_needs_the_open_gate_but_release_never_does() {
    let desc = kx3();
    let radio = VirtualRig::from_description(&desc).expect("virtual rig");
    let (stream, _control) =
        radioctl_protocols::io::spawn_virtual_rig(&Handle::current(), radio, Duration::ZERO);
    let engine = ElecraftEngine::new(desc).expect("engine");
    let rig = spawn(&Handle::current(), engine, stream, RigOptions::default()).expect("spawn");
    let lines = Lines::default();
    let keyer = LineKeyer::new(lines.clone(), KeyLine::Rts, true).expect("keyer");
    let error = keyer.key(&rig).expect_err("gate is closed");
    assert_eq!(error.kind, ErrorKind::TransmitGateClosed);
    assert_eq!(*lines.0.lock().expect("lock"), vec![(KeyLine::Rts, false)]);
    keyer.release().expect("release works with the gate closed");
    rig.open_transmit_gate(TransmitPermit::new("test: recording lines, no radio"));
    keyer.key(&rig).expect("gate open");
    rig.close_transmit_gate();
    keyer.release().expect("release");
    assert_eq!(
        *lines.0.lock().expect("lock"),
        vec![
            (KeyLine::Rts, false),
            (KeyLine::Rts, false),
            (KeyLine::Rts, true),
            (KeyLine::Rts, false)
        ]
    );
    // A real link without an open port reports it instead of pretending.
    let mut config = fast_config(Path::new("/dev/does-not-exist-radioctl"));
    config.first_open_attempts = 1;
    assert!(open_serial(&Handle::current(), config).await.is_err());
}

// Tests [@ANCHOR: radioctl_core_tcp_connect]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tcp_connects_moves_bytes_and_fails_in_bounded_time() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");
    tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.expect("accept");
        let mut buffer = [0u8; 16];
        let n = socket.read(&mut buffer).await.expect("read");
        socket
            .write_all(buffer.get(..n).unwrap_or(&[]))
            .await
            .expect("echo");
    });
    let config = TcpConfig::default();
    let mut stream = connect_tcp(addr, &config).await.expect("connect");
    stream.write_all(b"ID;").await.expect("write");
    let mut echoed = [0u8; 3];
    stream.read_exact(&mut echoed).await.expect("read");
    assert_eq!(&echoed, b"ID;");
    // A port nobody listens on: the closed listener's address.
    let closed = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let dead = closed.local_addr().expect("addr");
    drop(closed);
    let quick = TcpConfig {
        connect_timeout: Duration::from_millis(200),
        attempts: 2,
        backoff: Backoff {
            initial: Duration::from_millis(20),
            max: Duration::from_millis(20),
            factor: 2,
        },
    };
    let started = std::time::Instant::now();
    let error = connect_tcp(dead, &quick).await.expect_err("refused");
    assert_eq!(error.kind, ErrorKind::LinkDown);
    assert!(error.detail.contains("2 attempts"), "{}", error.detail);
    assert!(started.elapsed() < Duration::from_secs(2));
}

// Tests [@ANCHOR: radioctl_core_usb_list_sysfs]
// Tests [@ANCHOR: radioctl_core_usb_list_ports]
#[test]
fn sysfs_lists_usb_ports_and_skips_the_rest() {
    let root = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("sysfs-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let usb = root.join("sys/devices/pci0/usb1/1-2");
    let interface = usb.join("1-2:1.0");
    let tty = interface.join("ttyUSB0");
    fs::create_dir_all(&tty).expect("tree");
    for (file, value) in [
        ("idVendor", "0403\n"),
        ("idProduct", "6001\n"),
        ("serial", "A50285BI\n"),
        ("product", "FT232R USB UART\n"),
    ] {
        fs::write(usb.join(file), value).expect("attribute");
    }
    let class = root.join("sys/class/tty");
    fs::create_dir_all(class.join("ttyUSB0")).expect("class");
    std::os::unix::fs::symlink(&interface, class.join("ttyUSB0/device")).expect("link");
    fs::create_dir_all(class.join("tty1")).expect("console");
    fs::create_dir_all(class.join("ttyACM3")).expect("acm without a device link");
    let ports = list_sysfs(&root.join("sys"), Path::new("/dev"));
    assert_eq!(
        ports,
        vec![PortInfo {
            path: PathBuf::from("/dev/ttyUSB0"),
            vid: Some(0x0403),
            pid: Some(0x6001),
            serial: Some("A50285BI".into()),
            product: Some("FT232R USB UART".into()),
            cdc_acm: false,
        }]
    );
    assert!(list_sysfs(&root.join("missing"), Path::new("/dev")).is_empty());
    let _ = fs::remove_dir_all(&root);
    // The real machine: whatever it has, listing must not panic.
    let _ = list_ports();
}
