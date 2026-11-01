// SPDX-License-Identifier: AGPL-3.0-or-later
//! The USB link against the fake backend: the frame wire, the device-access traits, the supervisor's
//! open, loss and reopen, and the unkey that follows a reopen. The virtual device is the Si570
//! twin; nothing here touches USB hardware.
#![allow(clippy::expect_used, clippy::panic)]

use std::sync::Arc;
use std::time::Duration;

use radioctl_core::backoff::Backoff;
use radioctl_core::error::ErrorKind;
use radioctl_core::rig::{spawn, Freshness, RigOptions, TransmitPermit};
use radioctl_core::rigfile;
use radioctl_core::serial::LinkStatus;
use radioctl_core::units::{Hz, Vfo};
use radioctl_core::usb_device::{FakeTransfer, FakeUsbBackend, UsbBackend, UsbDevice, UsbFailure, UsbTarget};
use radioctl_core::usb_link::{open_usb, UsbConfig};
use radioctl_core::usb_wire::{execute, FrameError, UsbReply, UsbStatus, UsbTransfer};
use radioctl_protocols::si570_usb::{Si570Twin, Si570UsbEngine};
use tokio::runtime::Handle;
use tokio::time::{sleep, timeout};

const SOFTROCK: &str = include_str!("../../../rigs/softrock-si570-avr-usb.toml");
const T: Duration = Duration::from_millis(200);

fn target() -> UsbTarget {
    UsbTarget {
        vendor_id: 0x16C0,
        product_id: 0x05DC,
        manufacturer: "www.obdev.at".into(),
        product: "DG8SAQ-I2C".into(),
        serial: None,
        interface: None,
    }
}

fn fast(target: UsbTarget) -> UsbConfig {
    let mut config = UsbConfig::new(target);
    config.transfer_timeout = Duration::from_millis(500);
    config.backoff = Backoff {
        initial: Duration::from_millis(20),
        max: Duration::from_millis(80),
        factor: 2,
    };
    config
}

fn softrock() -> (FakeUsbBackend, radioctl_core::description::RigDescription) {
    let desc = rigfile::parse(SOFTROCK).expect("file").description;
    let twin = Si570Twin::new(&desc).expect("twin");
    (FakeUsbBackend::new(Box::new(twin)), desc)
}

async fn eventually(what: &str, mut check: impl FnMut() -> bool) {
    for _ in 0..300 {
        if check() {
            return;
        }
        sleep(Duration::from_millis(20)).await;
    }
    panic!("never happened: {what}");
}

/// A device that fails every transfer in a chosen way.
struct Broken(UsbFailure);

impl UsbDevice for Broken {
    fn control_in(&mut self, _: u8, _: u8, _: u16, _: u16, _: u16, _: Duration) -> Result<Vec<u8>, UsbFailure> {
        Err(self.0.clone())
    }
    fn control_out(&mut self, _: u8, _: u8, _: u16, _: u16, _: &[u8], _: Duration) -> Result<usize, UsbFailure> {
        Ok(1)
    }
    fn bulk_write(&mut self, _: u8, _: &[u8], _: Duration) -> Result<usize, UsbFailure> {
        Err(self.0.clone())
    }
    fn bulk_read(&mut self, _: u8, _: usize, _: Duration) -> Result<Vec<u8>, UsbFailure> {
        Err(self.0.clone())
    }
    fn interrupt_write(&mut self, _: u8, _: &[u8], _: Duration) -> Result<usize, UsbFailure> {
        Err(self.0.clone())
    }
    fn interrupt_read(&mut self, _: u8, _: usize, _: Duration) -> Result<Vec<u8>, UsbFailure> {
        Err(self.0.clone())
    }
}

// Tests [@ANCHOR: radioctl_core_usb_transfer_op]
// Tests [@ANCHOR: radioctl_core_usb_transfer_encode]
// Tests [@ANCHOR: radioctl_core_usb_transfer_decode]
// Tests [@ANCHOR: radioctl_core_usb_reply_encode]
// Tests [@ANCHOR: radioctl_core_usb_reply_decode]
// Tests [@ANCHOR: radioctl_core_usb_status_code]
// Tests [@ANCHOR: radioctl_core_usb_status_from_code]
#[test]
fn every_frame_round_trips_and_partial_or_bad_frames_are_told_apart() {
    let transfers = [
        UsbTransfer::ControlIn { request_type: 0xC0, request: 0x3A, value: 1, index: 2, length: 4 },
        UsbTransfer::ControlOut { request_type: 0x40, request: 0x32, value: 0x755, index: 0, data: vec![1, 2, 3, 4] },
        UsbTransfer::BulkWrite { endpoint: 2, data: vec![9; 10] },
        UsbTransfer::BulkRead { endpoint: 0x81, length: 512 },
        UsbTransfer::InterruptWrite { endpoint: 2, data: vec![7; 64] },
        UsbTransfer::InterruptRead { endpoint: 0x82, length: 64 },
        UsbTransfer::InterruptExchange { out_endpoint: 2, in_endpoint: 0x82, data: vec![0x65, 1, 2], read_length: 64 },
    ];
    for (n, transfer) in transfers.iter().enumerate() {
        let bytes = transfer.encode();
        assert_eq!(usize::from(transfer.op()), n + 1);
        assert_eq!(UsbTransfer::decode(&bytes), Ok((transfer.clone(), bytes.len())));
        // Every proper prefix is incomplete, and trailing bytes are left alone.
        for cut in 0..bytes.len() {
            assert_eq!(UsbTransfer::decode(bytes.get(..cut).unwrap_or(&[])), Err(FrameError::Incomplete), "{transfer:?} cut {cut}");
        }
        let mut two = bytes.clone();
        two.extend_from_slice(&bytes);
        assert_eq!(UsbTransfer::decode(&two).map(|(_, used)| used), Ok(bytes.len()));
    }
    assert_eq!(UsbTransfer::decode(&[99, 0, 0, 0, 0, 0, 0, 0, 0]), Err(FrameError::Malformed));
    // A data length over the limit is refused, not waited for.
    assert_eq!(UsbTransfer::decode(&[3, 1, 0, 0, 0, 0, 0, 0xFF, 0xFF]), Err(FrameError::Malformed));
    let reply = UsbReply { op: 4, status: UsbStatus::Stall, data: vec![5, 6, 7] };
    let bytes = reply.encode();
    assert_eq!(UsbReply::decode(&bytes), Ok((reply, bytes.len())));
    assert_eq!(UsbReply::decode(bytes.get(..bytes.len() - 1).unwrap_or(&[])), Err(FrameError::Incomplete));
    assert_eq!(UsbReply::decode(&[1, 0, 0xFF, 0xFF]), Err(FrameError::Malformed));
    for status in [UsbStatus::Ok, UsbStatus::Timeout, UsbStatus::NoDevice, UsbStatus::Stall, UsbStatus::Other] {
        assert_eq!(UsbStatus::from_code(status.code()), status);
    }
    assert_eq!(UsbStatus::from_code(200), UsbStatus::Other);
}

// Tests [@ANCHOR: radioctl_core_usb_execute]
#[test]
fn a_transfer_failure_or_a_short_write_never_reads_as_success() {
    let mut gone = Broken(UsbFailure::NoDevice);
    let read = UsbTransfer::ControlIn { request_type: 0xC0, request: 1, value: 0, index: 0, length: 4 };
    assert_eq!(execute(&mut gone, &read, T).status, UsbStatus::NoDevice);
    assert_eq!(execute(&mut Broken(UsbFailure::Timeout), &read, T).status, UsbStatus::Timeout);
    assert_eq!(execute(&mut Broken(UsbFailure::Stall), &read, T).status, UsbStatus::Stall);
    assert_eq!(execute(&mut Broken(UsbFailure::Other("x".into())), &read, T).status, UsbStatus::Other);
    // `Broken` accepts one byte of any control write: a two-byte write is short.
    let write = UsbTransfer::ControlOut { request_type: 0x40, request: 1, value: 0, index: 0, data: vec![1, 2] };
    assert_eq!(execute(&mut gone, &write, T).status, UsbStatus::Other);
    let one = UsbTransfer::ControlOut { request_type: 0x40, request: 1, value: 0, index: 0, data: vec![1] };
    assert_eq!(execute(&mut gone, &one, T).status, UsbStatus::Ok);
    // An exchange whose write fails never reads.
    let exchange = UsbTransfer::InterruptExchange { out_endpoint: 2, in_endpoint: 0x82, data: vec![1], read_length: 64 };
    assert_eq!(execute(&mut Broken(UsbFailure::Stall), &exchange, T).status, UsbStatus::Stall);
}

// Tests [@ANCHOR: radioctl_core_usb_target_matches]
// Tests [@ANCHOR: radioctl_core_usb_target_label]
#[test]
fn a_target_is_told_apart_by_ids_strings_and_serial() {
    let t = target();
    assert!(t.matches(0x16C0, 0x05DC, "www.obdev.at", "DG8SAQ-I2C", ""));
    assert!(!t.matches(0x16C0, 0x05DC, "www.obdev.at", "KTH-SDR-KIT", ""));
    assert!(!t.matches(0x16C0, 0x05DD, "www.obdev.at", "DG8SAQ-I2C", ""));
    assert!(!t.matches(0x16C1, 0x05DC, "www.obdev.at", "DG8SAQ-I2C", ""));
    let any = UsbTarget { manufacturer: String::new(), product: String::new(), ..target() };
    assert!(any.matches(0x16C0, 0x05DC, "AE9RB", "Peaberry SDR", "x"));
    let one = UsbTarget { serial: Some("A1".into()), ..target() };
    assert!(one.matches(0x16C0, 0x05DC, "www.obdev.at", "DG8SAQ-I2C", "A1"));
    assert!(!one.matches(0x16C0, 0x05DC, "www.obdev.at", "DG8SAQ-I2C", "A2"));
    assert_eq!(t.label(), "usb:16c0:05dc");
    assert_eq!(one.label(), "usb:16c0:05dc:A1");
}

// Tests [@ANCHOR: radioctl_core_fake_usb_new]
// Tests [@ANCHOR: radioctl_core_fake_usb_unplug]
// Tests [@ANCHOR: radioctl_core_fake_usb_plug]
// Tests [@ANCHOR: radioctl_core_fake_usb_opens]
// Tests [@ANCHOR: radioctl_core_fake_usb_log]
// Tests [@ANCHOR: radioctl_core_fake_usb_open]
// Tests [@ANCHOR: radioctl_core_fake_usb_handle_transfers]
#[test]
fn the_fake_bus_logs_transfers_and_fails_when_unplugged() {
    let (bus, _) = softrock();
    let mut device = bus.open(&target()).expect("open");
    assert_eq!(bus.opens(), 1);
    let word = device.control_in(0xC0, 0x3A, 0, 0, 4, T).expect("read");
    assert_eq!(word.len(), 4);
    assert_eq!(device.control_out(0x40, 0x32, 0x755, 0, &word, T), Ok(4));
    assert_eq!(device.bulk_write(2, &[1], T), Err(UsbFailure::Stall));
    assert_eq!(device.bulk_read(0x81, 4, T), Err(UsbFailure::Stall));
    assert_eq!(device.interrupt_write(2, &[1], T), Err(UsbFailure::Stall));
    assert_eq!(device.interrupt_read(0x82, 4, T), Err(UsbFailure::Stall));
    assert_eq!(bus.log().len(), 6);
    assert_eq!(bus.log().first(), Some(&FakeTransfer::ControlIn(0xC0, 0x3A, 0, 0, 4)));
    bus.unplug();
    assert_eq!(device.control_in(0xC0, 0x3A, 0, 0, 4, T), Err(UsbFailure::NoDevice));
    assert_eq!(bus.log().len(), 6, "an unplugged device logs nothing");
    let refused = bus.open(&target());
    assert_eq!(refused.err().map(|e| e.kind), Some(ErrorKind::LinkDown));
    bus.plug();
    assert!(bus.open(&target()).is_ok());
    assert_eq!(bus.opens(), 2);
}

// Tests [@ANCHOR: radioctl_core_usb_config_new]
// Tests [@ANCHOR: radioctl_core_usb_open]
// Tests [@ANCHOR: radioctl_core_usb_supervisor_run]
// Tests [@ANCHOR: radioctl_core_usb_sleep_dropping]
#[tokio::test]
async fn the_first_open_fails_cleanly_when_the_device_is_not_there() {
    let (bus, _) = softrock();
    bus.unplug();
    let mut config = fast(target());
    config.first_open_attempts = 3;
    let err = open_usb(&Handle::current(), Arc::new(bus.clone()), config)
        .await
        .err()
        .expect("no device");
    assert_eq!(err.kind, ErrorKind::LinkDown);
    assert!(err.detail.contains("not plugged in"), "{}", err.detail);
    assert_eq!(bus.opens(), 0);
}

// Tests [@ANCHOR: radioctl_core_usb_session]
// Tests [@ANCHOR: radioctl_core_usb_serve]
// Tests [@ANCHOR: radioctl_core_usb_link_status]
// Tests [@ANCHOR: radioctl_core_usb_link_watch]
// Tests [@ANCHOR: radioctl_core_usb_link_stats]
// Tests [@ANCHOR: radioctl_core_usb_link_into_parts]
// Tests [@ANCHOR: radioctl_core_usb_monitor_status]
// Tests [@ANCHOR: radioctl_core_usb_monitor_watch]
// Tests [@ANCHOR: radioctl_core_usb_monitor_stats]
#[tokio::test]
async fn a_rig_tunes_over_the_usb_link_and_garbage_does_not_wedge_it() {
    let (bus, desc) = softrock();
    let link = open_usb(&Handle::current(), Arc::new(bus.clone()), fast(target())).await.expect("link");
    assert!(matches!(link.status(), LinkStatus::Connected { generation: 1, .. }));
    assert_eq!(link.stats().reconnects, 0);
    let _ = link.watch();
    let (stream, monitor) = link.into_parts();
    let mut options = RigOptions::default();
    options.poll.enabled = false;
    options.link_watch = Some(monitor.watch());
    let rig = spawn(&Handle::current(), Si570UsbEngine::new(desc).expect("engine"), stream, options).expect("spawn");
    assert_eq!(rig.frequency(Vfo::A, Freshness::Fresh).await.expect("read").value, Hz(7_074_000));
    rig.set_frequency(Vfo::A, Hz(14_074_000)).await.expect("set");
    assert_eq!(rig.frequency(Vfo::A, Freshness::Fresh).await.expect("read").value, Hz(14_074_000));
    assert!(monitor.stats().bytes_out > 0 && monitor.stats().bytes_in > 0);
    assert!(matches!(monitor.status(), LinkStatus::Connected { generation: 1, .. }));
    assert_eq!(bus.opens(), 1);
}

// Tests [@ANCHOR: radioctl_core_usb_session]
#[tokio::test]
async fn a_lost_device_is_reopened_and_the_rig_is_unkeyed_before_anything_else() {
    let (bus, desc) = softrock();
    let link = open_usb(&Handle::current(), Arc::new(bus.clone()), fast(target())).await.expect("link");
    let (stream, monitor) = link.into_parts();
    let mut options = RigOptions::default();
    options.poll.enabled = false;
    options.link_watch = Some(monitor.watch());
    options.default_timeout = Duration::from_secs(2);
    let rig = spawn(&Handle::current(), Si570UsbEngine::new(desc).expect("engine"), stream, options).expect("spawn");
    // The gate is closed: a key request is refused before it reaches the device.
    let refused = rig.set_ptt(true).await.expect_err("gate closed");
    assert_eq!(refused.kind, ErrorKind::TransmitGateClosed);
    assert!(!bus.log().iter().any(|t| matches!(t, FakeTransfer::ControlIn(_, 0x50, _, _, _))));
    rig.open_transmit_gate(TransmitPermit::new("usb link test"));
    rig.set_ptt(true).await.expect("keys");
    bus.unplug();
    // The next command finds the link gone.
    let err = rig.frequency(Vfo::A, Freshness::Fresh).await.expect_err("link down");
    assert!(matches!(err.kind, ErrorKind::LinkDown | ErrorKind::Timeout), "{err:?}");
    eventually("reconnecting", || matches!(monitor.status(), LinkStatus::Reconnecting { .. })).await;
    let before = bus.log().len();
    bus.plug();
    eventually("reconnected", || matches!(monitor.status(), LinkStatus::Connected { generation: 2, .. })).await;
    assert_eq!(monitor.stats().reconnects, 1);
    // The first thing the reopened device hears is the unkey.
    eventually("unkey after reopen", || {
        bus.log().iter().skip(before).any(|t| matches!(t, FakeTransfer::ControlIn(0xC0, 0x50, 0, 0, 3)))
    })
    .await;
    let after = bus.log();
    let first = after.get(before..).and_then(|l| l.first());
    assert_eq!(first, Some(&FakeTransfer::ControlIn(0xC0, 0x50, 0, 0, 3)));
    timeout(Duration::from_secs(3), rig.frequency(Vfo::A, Freshness::Fresh))
        .await
        .expect("answered")
        .expect("works again");
}
