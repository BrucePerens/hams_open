// SPDX-License-Identifier: AGPL-3.0-or-later
//! The splitter in front of a virtual KX3 driven by the real rig actor and the Elecraft engine,
//! with simulated external programs: CI-V and TS-2000 clients over in-memory streams, TCP and
//! pseudo-terminals. Nothing is keyed that is not a virtual rig.
#![allow(clippy::expect_used, clippy::panic)]

use std::io::{Read, Write};
use std::path::Path;
use std::time::Duration;

use radioctl_core::op::Op;
use radioctl_core::rig::{spawn, RigHandle, RigOptions, TransmitPermit};
use radioctl_core::rigfile;
use radioctl_core::units::{Hz, Mode};
use radioctl_protocols::elecraft::ElecraftEngine;
use radioctl_protocols::io::{spawn_virtual_rig, VirtualRigControl};
use radioctl_protocols::virtual_rig::{PanelChange, VirtualRig};
use radioctl_splitter::pty::attach_pty;
use radioctl_splitter::session::{Protocol, Splitter, SplitterConfig};
use serialport::SerialPort;
use tokio::io::{duplex, AsyncReadExt, AsyncWriteExt, DuplexStream};
use tokio::runtime::Handle;
use tokio::time::{sleep, timeout};

const READ_FREQ: [u8; 6] = [0xFE, 0xFE, 0x94, 0xE0, 0x03, 0xFD];
const OK: [u8; 6] = [0xFE, 0xFE, 0xE0, 0x94, 0xFB, 0xFD];
const NG: [u8; 6] = [0xFE, 0xFE, 0xE0, 0x94, 0xFA, 0xFD];
const PTT_ON: [u8; 8] = [0xFE, 0xFE, 0x94, 0xE0, 0x1C, 0x00, 0x01, 0xFD];
const PTT_OFF: [u8; 8] = [0xFE, 0xFE, 0x94, 0xE0, 0x1C, 0x00, 0x00, 0xFD];

async fn virtual_kx3() -> (RigHandle, VirtualRigControl) {
    let desc = rigfile::parse(include_str!("../../../rigs/elecraft-kx3.toml"))
        .expect("kx3 file")
        .description;
    let radio = VirtualRig::from_description(&desc).expect("virtual rig");
    let (stream, control) = spawn_virtual_rig(&Handle::current(), radio, Duration::ZERO);
    let engine = ElecraftEngine::new(desc).expect("engine");
    let mut options = RigOptions::default();
    options.poll.enabled = false;
    let rig = spawn(&Handle::current(), engine, stream, options).expect("spawn");
    rig.execute(Op::SetTransceive(true)).await.expect("AI2");
    (rig, control)
}

fn splitter(rig: &RigHandle, protocol: Protocol) -> Splitter {
    Splitter::new(rig.clone(), SplitterConfig::new(protocol))
}

fn civ(echo: bool) -> Protocol {
    Protocol::Civ {
        address: 0x94,
        echo,
    }
}

/// A simulated external program on an in-memory stream.
fn client(splitter: &Splitter) -> DuplexStream {
    let (near, far) = duplex(4096);
    splitter.attach_stream(&Handle::current(), far);
    near
}

async fn read_n(stream: &mut DuplexStream, n: usize) -> Vec<u8> {
    let mut out = vec![0u8; n];
    timeout(Duration::from_secs(3), stream.read_exact(&mut out))
        .await
        .expect("bytes in time")
        .expect("read");
    out
}

async fn nothing_within(stream: &mut DuplexStream, wait: Duration) -> bool {
    let mut byte = [0u8; 1];
    timeout(wait, stream.read(&mut byte)).await.is_err()
}

fn freq_frame(to: u8, cmd: u8, bcd: [u8; 5]) -> Vec<u8> {
    let mut frame = vec![0xFE, 0xFE, to, 0x94, cmd];
    frame.extend(bcd);
    frame.push(0xFD);
    frame
}

const BCD_14060000: [u8; 5] = [0x00, 0x00, 0x06, 0x14, 0x00];
const BCD_7030000: [u8; 5] = [0x00, 0x00, 0x03, 0x07, 0x00];
const BCD_7100000: [u8; 5] = [0x00, 0x00, 0x10, 0x07, 0x00];

async fn wait_for(mut condition: impl FnMut() -> bool) {
    for _ in 0..100 {
        if condition() {
            return;
        }
        sleep(Duration::from_millis(20)).await;
    }
    panic!("condition not met in time");
}

// Tests [@ANCHOR: radioctl_splitter_new]
// Tests [@ANCHOR: radioctl_splitter_config_new]
// Tests [@ANCHOR: radioctl_splitter_attach_stream]
// Tests [@ANCHOR: radioctl_splitter_attach_channels]
// Tests [@ANCHOR: radioctl_splitter_run_client]
// Tests [@ANCHOR: radioctl_splitter_execute]
// Tests [@ANCHOR: radioctl_splitter_read]
// Tests [@ANCHOR: radioctl_splitter_stats]
// Tests [@ANCHOR: radioctl_core_handle_execute_from]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn two_civ_clients_share_one_rig_one_queue_and_cached_reads() {
    let (rig, control) = virtual_kx3().await;
    let splitter = splitter(&rig, civ(false));
    let mut a = client(&splitter);
    let mut b = client(&splitter);
    a.write_all(&READ_FREQ).await.expect("write");
    assert_eq!(
        read_n(&mut a, 11).await,
        freq_frame(0xE0, 0x03, BCD_14060000)
    );
    let after_first = control.bytes_received();
    b.write_all(&READ_FREQ).await.expect("write");
    assert_eq!(
        read_n(&mut b, 11).await,
        freq_frame(0xE0, 0x03, BCD_14060000)
    );
    assert_eq!(
        control.bytes_received(),
        after_first,
        "the second program's read came from the cache, not the radio"
    );
    assert!(splitter.stats().cache_hits >= 1);
    // A sets the frequency; B sees it, and both are told by the radio's transceive frame.
    let mut set = vec![0xFE, 0xFE, 0x94, 0xE0, 0x05];
    set.extend(BCD_7030000);
    set.push(0xFD);
    a.write_all(&set).await.expect("write");
    assert_eq!(read_n(&mut a, 6).await, OK);
    assert_eq!(
        read_n(&mut a, 11).await,
        freq_frame(0x00, 0x00, BCD_7030000)
    );
    assert_eq!(
        read_n(&mut b, 11).await,
        freq_frame(0x00, 0x00, BCD_7030000)
    );
    assert_eq!(control.state().freq_a, Hz(7_030_000));
    b.write_all(&READ_FREQ).await.expect("write");
    assert_eq!(
        read_n(&mut b, 11).await,
        freq_frame(0xE0, 0x03, BCD_7030000)
    );
    // Mode through B, read by A in the IC-7300's table (CW is 03).
    b.write_all(&[0xFE, 0xFE, 0x94, 0xE0, 0x06, 0x01, 0x01, 0xFD])
        .await
        .expect("write");
    assert_eq!(read_n(&mut b, 6).await, OK);
    assert_eq!(control.state().mode, Mode::Usb);
    a.write_all(&[0xFE, 0xFE, 0x94, 0xE0, 0x04, 0xFD])
        .await
        .expect("write");
    assert_eq!(
        read_n(&mut a, 8).await,
        vec![0xFE, 0xFE, 0xE0, 0x94, 0x04, 0x01, 0x01, 0xFD]
    );
    assert_eq!(rig.stats().commands_failed, 0);
    assert!(splitter.stats().clients == 2);
}

// Tests [@ANCHOR: radioctl_splitter_civ_parse]
// Tests [@ANCHOR: radioctl_splitter_civ_reply]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn civ_echo_semantics_follow_the_endpoint_setting() {
    let (rig, _control) = virtual_kx3().await;
    let echoing = splitter(&rig, civ(true));
    let mut with_echo = client(&echoing);
    with_echo.write_all(&READ_FREQ).await.expect("write");
    let heard = read_n(&mut with_echo, 6 + 11).await;
    assert_eq!(heard.get(..6), Some(&READ_FREQ[..]));
    assert_eq!(
        heard.get(6..),
        Some(freq_frame(0xE0, 0x03, BCD_14060000).as_slice())
    );
    let quiet = splitter(&rig, civ(false));
    let mut without = client(&quiet);
    without.write_all(&READ_FREQ).await.expect("write");
    let heard = read_n(&mut without, 11).await;
    assert_eq!(heard, freq_frame(0xE0, 0x03, BCD_14060000));
    assert!(nothing_within(&mut without, Duration::from_millis(150)).await);
}

// Tests [@ANCHOR: radioctl_splitter_civ_unsolicited]
// Tests [@ANCHOR: radioctl_splitter_ts2000_unsolicited]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn front_panel_changes_reach_every_client_as_that_clients_unsolicited_frames() {
    let (rig, control) = virtual_kx3().await;
    let ci_v = splitter(&rig, civ(false));
    let cat = splitter(&rig, Protocol::Ts2000);
    let mut icom = client(&ci_v);
    let mut kenwood_off = client(&cat);
    let mut kenwood_on = client(&cat);
    kenwood_on.write_all(b"AI1;").await.expect("write");
    sleep(Duration::from_millis(100)).await;
    control.front_panel(PanelChange::Frequency(Hz(7_100_000)));
    assert_eq!(
        read_n(&mut icom, 11).await,
        freq_frame(0x00, 0x00, BCD_7100000)
    );
    assert_eq!(read_n(&mut kenwood_on, 14).await, b"FA00007100000;");
    assert!(nothing_within(&mut kenwood_off, Duration::from_millis(200)).await);
    control.front_panel(PanelChange::Mode(Mode::Lsb));
    assert_eq!(
        read_n(&mut icom, 8).await,
        vec![0xFE, 0xFE, 0x00, 0x94, 0x01, 0x00, 0x01, 0xFD]
    );
    assert_eq!(read_n(&mut kenwood_on, 4).await, b"MD1;");
}

// Tests [@ANCHOR: radioctl_splitter_key]
// Tests [@ANCHOR: radioctl_splitter_cleanup]
// Tests [@ANCHOR: radioctl_splitter_ptt_owner]
// Tests [@ANCHOR: radioctl_splitter_arbiter_try_key]
// Tests [@ANCHOR: radioctl_splitter_arbiter_release]
// Tests [@ANCHOR: radioctl_splitter_arbiter_clear]
// Tests [@ANCHOR: radioctl_splitter_arbiter_owner]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn one_client_owns_the_transmitter_and_its_departure_unkeys_the_rig() {
    let (rig, control) = virtual_kx3().await;
    let splitter = splitter(&rig, civ(false));
    let mut a = client(&splitter);
    let mut b = client(&splitter);
    // The gate is closed: nothing keys, nobody owns anything.
    a.write_all(&PTT_ON).await.expect("write");
    assert_eq!(read_n(&mut a, 6).await, NG);
    assert!(!control.state().ptt);
    assert_eq!(splitter.ptt_owner(), None);
    rig.open_transmit_gate(TransmitPermit::new("test: virtual rig only"));
    a.write_all(&PTT_ON).await.expect("write");
    assert_eq!(read_n(&mut a, 6).await, OK);
    assert!(control.state().ptt);
    let owner = splitter.ptt_owner().expect("a owns it");
    // B cannot key while A holds it, and the rig stays as A left it.
    b.write_all(&PTT_ON).await.expect("write");
    assert_eq!(read_n(&mut b, 6).await, NG);
    assert_eq!(splitter.stats().ptt_conflicts, 1);
    assert_eq!(splitter.ptt_owner(), Some(owner));
    // CW from B is refused while A transmits.
    b.write_all(&[0xFE, 0xFE, 0x94, 0xE0, 0x17, b'C', b'Q', 0xFD])
        .await
        .expect("write");
    assert_eq!(read_n(&mut b, 6).await, NG);
    assert!(control.state().cw_sent.is_empty());
    // Anyone may unkey.
    b.write_all(&PTT_OFF).await.expect("write");
    assert_eq!(read_n(&mut b, 6).await, OK);
    assert!(!control.state().ptt);
    assert_eq!(splitter.ptt_owner(), None);
    // B keys; B vanishing unkeys the rig.
    b.write_all(&PTT_ON).await.expect("write");
    assert_eq!(read_n(&mut b, 6).await, OK);
    assert!(control.state().ptt);
    drop(b);
    wait_for(|| !control.state().ptt && splitter.ptt_owner().is_none()).await;
    // A third client reads the transmit state through the cache.
    a.write_all(&[0xFE, 0xFE, 0x94, 0xE0, 0x1C, 0x00, 0xFD])
        .await
        .expect("write");
    let state = read_n(&mut a, 8).await;
    assert_eq!(state, vec![0xFE, 0xFE, 0xE0, 0x94, 0x1C, 0x00, 0x00, 0xFD]);
}

// Tests [@ANCHOR: radioctl_splitter_ts2000_parse]
// Tests [@ANCHOR: radioctl_splitter_ts2000_reply]
// Tests [@ANCHOR: radioctl_splitter_ts2000_classify]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_ts2000_client_reads_sets_and_is_refused_what_it_cannot_have() {
    let (rig, control) = virtual_kx3().await;
    let splitter = splitter(&rig, Protocol::Ts2000);
    let mut a = client(&splitter);
    a.write_all(b"FA;MD;ID;").await.expect("write");
    assert_eq!(
        read_n(&mut a, 14 + 4 + 6).await,
        b"FA00014060000;MD3;ID019;"
    );
    a.write_all(b"FA00007074000;MD2;").await.expect("write");
    sleep(Duration::from_millis(100)).await;
    assert_eq!(control.state().freq_a, Hz(7_074_000));
    assert_eq!(control.state().mode, Mode::Usb);
    a.write_all(b"IF;ZZ;SM0;").await.expect("write");
    assert_eq!(read_n(&mut a, 2 + 2 + 8).await, b"?;?;SM00015;");
    a.write_all(b"TX;").await.expect("write");
    assert_eq!(read_n(&mut a, 2).await, b"?;");
    assert!(!control.state().ptt);
}

// Tests [@ANCHOR: radioctl_splitter_serve_tcp]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tcp_clients_are_clients_too() {
    let (rig, _control) = virtual_kx3().await;
    let splitter = splitter(&rig, Protocol::Ts2000);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");
    let task = splitter.serve_tcp(&Handle::current(), listener);
    let mut first = tokio::net::TcpStream::connect(addr).await.expect("connect");
    let mut second = tokio::net::TcpStream::connect(addr).await.expect("connect");
    for stream in [&mut first, &mut second] {
        stream.write_all(b"FA;").await.expect("write");
        let mut reply = [0u8; 14];
        timeout(Duration::from_secs(3), stream.read_exact(&mut reply))
            .await
            .expect("in time")
            .expect("read");
        assert_eq!(&reply, b"FA00014060000;");
    }
    task.abort();
}

fn external_program(path: &Path) -> serialport::TTYPort {
    let mut port = serialport::new(path.to_string_lossy(), 9600)
        .open_native()
        .expect("open the virtual port as a program would");
    port.set_timeout(Duration::from_millis(100))
        .expect("timeout");
    port
}

fn program_read(port: &mut serialport::TTYPort, want: usize) -> Vec<u8> {
    let mut out = Vec::new();
    let mut buffer = [0u8; 64];
    for _ in 0..40 {
        if out.len() >= want {
            break;
        }
        if let Ok(n) = port.read(&mut buffer) {
            out.extend_from_slice(buffer.get(..n).unwrap_or(&[]));
        }
    }
    out
}

// Tests [@ANCHOR: radioctl_splitter_pty_attach]
// Tests [@ANCHOR: radioctl_splitter_pty_path]
// Tests [@ANCHOR: radioctl_splitter_pty_slave_path]
// Tests [@ANCHOR: radioctl_splitter_pty_drop]
// Tests [@ANCHOR: radioctl_splitter_pty_read_master]
// Tests [@ANCHOR: radioctl_splitter_pty_write_master]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn two_programs_on_two_virtual_serial_ports_share_the_radio_and_the_transmitter() {
    let (rig, control) = virtual_kx3().await;
    let splitter = splitter(&rig, civ(false));
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("pty-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("dir");
    let runtime = Handle::current();
    let (wsjtx_port, _a) =
        attach_pty(&splitter, &runtime, Some(dir.join("wsjtx"))).expect("pty one");
    let (fldigi_port, _b) =
        attach_pty(&splitter, &runtime, Some(dir.join("fldigi"))).expect("pty two");
    assert_eq!(wsjtx_port.path(), dir.join("wsjtx"));
    assert!(wsjtx_port.slave_path().starts_with("/dev/"));
    rig.open_transmit_gate(TransmitPermit::new("test: virtual rig only"));
    let (wsjtx_path, fldigi_path) = (
        wsjtx_port.path().to_path_buf(),
        fldigi_port.path().to_path_buf(),
    );
    let outcome = tokio::task::spawn_blocking(move || {
        let mut wsjtx = external_program(&wsjtx_path);
        let mut fldigi = external_program(&fldigi_path);
        wsjtx.write_all(&READ_FREQ).expect("write");
        let freq = program_read(&mut wsjtx, 11);
        fldigi.write_all(&READ_FREQ).expect("write");
        let freq_two = program_read(&mut fldigi, 11);
        wsjtx.write_all(&PTT_ON).expect("write");
        let first = program_read(&mut wsjtx, 6);
        fldigi.write_all(&PTT_ON).expect("write");
        let second = program_read(&mut fldigi, 6);
        // The program using the first port goes away (closes it) while it holds the PTT.
        drop(wsjtx);
        (freq, freq_two, first, second)
    })
    .await
    .expect("join");
    assert_eq!(outcome.0, freq_frame(0xE0, 0x03, BCD_14060000));
    assert_eq!(outcome.1, freq_frame(0xE0, 0x03, BCD_14060000));
    assert_eq!(outcome.2, OK.to_vec());
    assert_eq!(outcome.3, NG.to_vec());
    wait_for(|| !control.state().ptt).await;
    assert_eq!(splitter.ptt_owner(), None);
    let link = wsjtx_port.path().to_path_buf();
    drop(wsjtx_port);
    assert!(!link.exists(), "the symlink is removed with the endpoint");
    let _ = std::fs::remove_dir_all(&dir);
}

// Tests [@ANCHOR: radioctl_splitter_client_wait]
// Tests [@ANCHOR: radioctl_splitter_client_abort]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_client_ends_when_its_stream_ends_or_when_aborted() {
    let (rig, _control) = virtual_kx3().await;
    let splitter = splitter(&rig, Protocol::Ts2000);
    let (near, far) = duplex(1024);
    let ended = splitter.attach_stream(&Handle::current(), far);
    drop(near);
    timeout(Duration::from_secs(3), ended.wait())
        .await
        .expect("the client ended with its stream");
    let (_near, far) = duplex(1024);
    let running = splitter.attach_stream(&Handle::current(), far);
    running.abort();
    timeout(Duration::from_secs(3), running.wait())
        .await
        .expect("the aborted client ended");
}
