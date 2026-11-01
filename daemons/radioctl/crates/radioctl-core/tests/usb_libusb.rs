// SPDX-License-Identifier: AGPL-3.0-or-later
//! The direct libusb backend (cargo feature `usb`): it lists the bus and reports an absent device.
//! It is given ids no device has, so it opens nothing; the transfers themselves are exercised only
//! on a real board, which no test touches.
#![cfg(feature = "usb")]
#![allow(clippy::expect_used, clippy::panic)]

use radioctl_core::error::ErrorKind;
use radioctl_core::usb_device::{UsbBackend, UsbTarget};
use radioctl_core::usb_libusb::LibusbBackend;

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
