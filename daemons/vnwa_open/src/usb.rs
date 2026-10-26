// SPDX-License-Identifier: AGPL-3.0-or-later
//! USB transfers to the VNWA: vendor control transfers on endpoint 0 only (`docs/PROTOCOL.md`). Nothing here claims an
//! interface, resets the device, or sends anything that is not a `Request` built by this crate, and no firmware is touched.

use std::time::Duration;

use rusb::UsbContext;

use crate::protocol::Request;
use crate::{PID, VID};

const TIMEOUT: Duration = Duration::from_millis(1000);
/// `bmRequestType` of a vendor read (device to host).
const VENDOR_IN: u8 = 0xC0;

pub struct Vnwa {
    handle: rusb::DeviceHandle<rusb::Context>,
}

impl Vnwa {
    /// Opens the first VNWA found on the USB bus.
    pub fn open() -> Result<Vnwa, String> {
        let context = rusb::Context::new().map_err(|e| format!("libusb: {e}"))?;
        let handle = context.open_device_with_vid_pid(VID, PID).ok_or_else(|| {
            format!("no VNWA ({VID:04x}:{PID:04x}) found, or no permission to open it")
        })?;
        Ok(Vnwa { handle })
    }

    /// One host-to-device vendor request.
    pub fn send(&self, r: &Request) -> Result<(), String> {
        let sent = self
            .handle
            .write_control(
                r.request_type,
                r.request,
                r.value,
                r.index,
                &r.data,
                TIMEOUT,
            )
            .map_err(|e| format!("request {:#04x}: {e}", r.request))?;
        if sent != r.data.len() {
            return Err(format!(
                "request {:#04x}: {sent} of {} bytes sent",
                r.request,
                r.data.len()
            ));
        }
        Ok(())
    }

    /// The requests of a block, in order.
    pub fn send_all(&self, block: &[Request]) -> Result<(), String> {
        block.iter().try_for_each(|r| self.send(r))
    }

    /// A device-to-host vendor request of at most `len` bytes (for example 0x3a, the generator's read-back).
    pub fn read(&self, request: u8, value: u16, index: u16, len: usize) -> Result<Vec<u8>, String> {
        let mut buf = vec![0u8; len];
        let got = self
            .handle
            .read_control(VENDOR_IN, request, value, index, &mut buf, TIMEOUT)
            .map_err(|e| format!("read request {request:#04x}: {e}"))?;
        buf.truncate(got);
        Ok(buf)
    }
}
