// SPDX-License-Identifier: AGPL-3.0-or-later
//! The direct libusb backend (cargo feature `usb`): it lists the bus and reports an absent device.
//! It is given ids no device has, so it opens nothing; the transfers themselves are exercised only
//! on a real board, which no test touches.
#![cfg(feature = "usb")]
#![allow(clippy::expect_used, clippy::panic)]

use radioctl_core::error::ErrorKind;
use radioctl_core::usb_device::{UsbBackend, UsbFailure, UsbTarget};
use radioctl_core::usb_libusb::{failure, read_into, LibusbBackend};

// Tests [@ANCHOR: radioctl_core_libusb_backend_new]
// Tests [@ANCHOR: radioctl_core_libusb_backend_open]
#[test]
fn an_absent_device_is_reported_as_not_plugged_in() {
    let backend = LibusbBackend::new().expect("libusb initialises");
    let target = UsbTarget {
        vendor_id: 0xFFFF,
        product_id: 0xFFFE,
        manufacturer: String::new(),
        product: String::new(),
        serial: None,
        interface: None,
    };
    let error = backend.open(&target).err().expect("absent");
    assert_eq!(error.kind, ErrorKind::LinkDown);
    assert!(error.detail.contains("not plugged in"), "{}", error.detail);
}

// Tests [@ANCHOR: radioctl_core_libusb_failure]
#[test]
fn libusb_errors_map_to_what_the_link_does_about_them() {
    assert_eq!(failure(rusb::Error::Timeout), UsbFailure::Timeout);
    assert_eq!(failure(rusb::Error::NoDevice), UsbFailure::NoDevice);
    assert_eq!(failure(rusb::Error::Io), UsbFailure::NoDevice);
    assert_eq!(failure(rusb::Error::Pipe), UsbFailure::Stall);
    assert!(matches!(failure(rusb::Error::Busy), UsbFailure::Other(_)));
}

// Tests [@ANCHOR: radioctl_core_libusb_read_into]
#[test]
fn a_read_returns_only_the_bytes_that_arrived() {
    let got = read_into(8, |buffer| {
        buffer.iter_mut().take(3).for_each(|b| *b = 7);
        Ok(3)
    });
    assert_eq!(got, Ok(vec![7, 7, 7]));
    assert_eq!(
        read_into(8, |_| Err(rusb::Error::Timeout)),
        Err(UsbFailure::Timeout)
    );
}
