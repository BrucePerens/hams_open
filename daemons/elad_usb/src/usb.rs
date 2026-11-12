// SPDX-License-Identifier: AGPL-3.0-or-later
//! The real USB transport (libusb through rusb) and read-only discovery.

use crate::protocol::{Model, EP_SAMPLES, VENDOR_ID};
use crate::transport::Transport;
use crate::Error;
use rusb::{request_type, Direction, Recipient, RequestType, UsbContext};
use std::time::Duration;

const CONTROL_TIMEOUT: Duration = Duration::from_millis(1000);

// [@ANCHOR: elad_usb_err]
fn err(e: rusb::Error) -> Error {
    match e {
        rusb::Error::Timeout => Error::Timeout,
        other => Error::Usb(other.to_string()),
    }
}

/// One ELAD receiver found on the bus.
// [@ANCHOR: usb_found_struct]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Found {
    /// Bus number.
    pub bus: u8,
    /// Device address.
    pub address: u8,
    /// Product id.
    pub pid: u16,
    /// The model.
    pub model: Model,
}

/// Lists ELAD receivers (vendor id 0x1721 and a known product id) without opening any of them.
// [@ANCHOR: elad_usb_find]
pub fn find() -> Result<Vec<Found>, Error> {
    let mut out = Vec::new();
    for d in rusb::devices().map_err(err)?.iter() {
        let Ok(desc) = d.device_descriptor() else {
            continue;
        };
        if desc.vendor_id() != VENDOR_ID {
            continue;
        }
        if let Ok(model) = Model::from_pid(desc.product_id()) {
            out.push(Found {
                bus: d.bus_number(),
                address: d.address(),
                pid: desc.product_id(),
                model,
            });
        }
    }
    Ok(out)
}

/// Whether a device with these USB ids is the receiver `found` names.
// [@ANCHOR: elad_usb_is_found]
fn is_found(vendor_id: u16, product_id: u16, found: &Found) -> bool {
    vendor_id == VENDOR_ID && product_id == found.pid
}

/// An open, claimed receiver.
// [@ANCHOR: usb_usb_transport_struct]
pub struct UsbTransport {
    handle: rusb::DeviceHandle<rusb::Context>,
}

impl UsbTransport {
    /// Opens `found`, selects configuration 1 and claims interface 0.
    // [@ANCHOR: elad_usb_open]
    pub fn open(found: &Found) -> Result<UsbTransport, Error> {
        let ctx = rusb::Context::new().map_err(err)?;
        for d in ctx.devices().map_err(err)?.iter() {
            if d.bus_number() == found.bus && d.address() == found.address {
                // The address can be reused after an unplug: only the receiver `find` listed there is opened and claimed.
                let desc = d.device_descriptor().map_err(err)?;
                if !is_found(desc.vendor_id(), desc.product_id(), found) {
                    return Err(Error::Usb(
                        "the device at that bus address is no longer the receiver that was found there".into(),
                    ));
                }
                let handle = d.open().map_err(err)?;
                let _ = handle.set_auto_detach_kernel_driver(true);
                handle.claim_interface(0).map_err(err)?;
                return Ok(UsbTransport { handle });
            }
        }
        Err(Error::Usb("the device is no longer on the bus".into()))
    }
}

impl Transport for UsbTransport {
    fn control_in(
        &mut self,
        request: u8,
        value: u16,
        index: u16,
        buf: &mut [u8],
    ) -> Result<usize, Error> {
        let rt = request_type(Direction::In, RequestType::Vendor, Recipient::Device);
        self.handle
            .read_control(rt, request, value, index, buf, CONTROL_TIMEOUT)
            .map_err(err)
    }

    fn control_out(
        &mut self,
        request: u8,
        value: u16,
        index: u16,
        data: &[u8],
    ) -> Result<usize, Error> {
        let rt = request_type(Direction::Out, RequestType::Vendor, Recipient::Device);
        self.handle
            .write_control(rt, request, value, index, data, CONTROL_TIMEOUT)
            .map_err(err)
    }

    fn bulk_in(&mut self, buf: &mut [u8], timeout: Duration) -> Result<usize, Error> {
        self.handle.read_bulk(EP_SAMPLES, buf, timeout).map_err(err)
    }
}

#[cfg(test)]
mod tests {
    // Tests [@ANCHOR: usb_found_struct]
    // Tests [@ANCHOR: usb_usb_transport_struct]
    use super::*;

    #[test]
    fn only_the_receiver_that_was_found_matches() {
        // Tests [@ANCHOR: elad_usb_is_found]
        let found = Found {
            bus: 1,
            address: 8,
            pid: Model::S2.pid(),
            model: Model::S2,
        };
        assert!(is_found(VENDOR_ID, Model::S2.pid(), &found));
        assert!(!is_found(VENDOR_ID, Model::Duo.pid(), &found));
        assert!(!is_found(0x20b7, Model::S2.pid(), &found));
        assert!(!is_found(0x20b7, 0x9db1, &found));
    }

    #[test]
    fn a_libusb_timeout_stays_a_timeout() {
        // Tests [@ANCHOR: elad_usb_err]
        assert_eq!(err(rusb::Error::Timeout), Error::Timeout);
        assert!(matches!(err(rusb::Error::NoDevice), Error::Usb(_)));
    }
}
