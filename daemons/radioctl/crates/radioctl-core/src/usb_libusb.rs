// SPDX-License-Identifier: AGPL-3.0-or-later
//! The direct libusb backend (cargo feature `usb`, off by default): opens the device in this
//! process through the `rusb` crate. The relay's zero-privilege broker will provide its own
//! [`UsbBackend`] instead; engines and the link do not know which one they have.
//!
//! Matching reads the manufacturer, product and serial strings of each candidate with the
//! right vendor and product id, so devices on the V-USB shared id are told apart. A device that
//! cannot be opened (no permission) is skipped; if none opens, the error says whether any
//! candidate was seen, so a missing udev rule reads differently from an unplugged board. The
//! kernel driver is detached while the interface is claimed and reattached on close
//! (libusb's auto-detach), which the FUNcube's HID interface needs.

use std::time::Duration;

use rusb::{Context, DeviceHandle, UsbContext};

use crate::error::{ErrorKind, RigError};
use crate::usb_device::{UsbBackend, UsbDevice, UsbFailure, UsbTarget};

/// Opens devices through libusb.
pub struct LibusbBackend {
    context: Context,
}

impl LibusbBackend {
    // [@ANCHOR: radioctl_core_libusb_backend_new]
    /// A libusb context. Fails when libusb cannot initialise.
    pub fn new() -> Result<LibusbBackend, RigError> {
        let context = Context::new()
            .map_err(|e| RigError::new(ErrorKind::Unsupported, format!("libusb: {e}")))?;
        Ok(LibusbBackend { context })
    }
}

fn failure(error: rusb::Error) -> UsbFailure {
    match error {
        rusb::Error::Timeout => UsbFailure::Timeout,
        rusb::Error::NoDevice | rusb::Error::Io => UsbFailure::NoDevice,
        rusb::Error::Pipe => UsbFailure::Stall,
        other => UsbFailure::Other(other.to_string()),
    }
}

impl UsbBackend for LibusbBackend {
    // [@ANCHOR: radioctl_core_libusb_backend_open]
    fn open(&self, target: &UsbTarget) -> Result<Box<dyn UsbDevice>, RigError> {
        let down = |detail: String| RigError::new(ErrorKind::LinkDown, detail);
        let devices = self
            .context
            .devices()
            .map_err(|e| down(format!("libusb: list devices: {e}")))?;
        let mut candidates = 0u32;
        let mut last_open_error = None;
        for device in devices.iter() {
            let Ok(descriptor) = device.device_descriptor() else {
                continue;
            };
            if descriptor.vendor_id() != target.vendor_id
                || descriptor.product_id() != target.product_id
            {
                continue;
            }
            candidates += 1;
            let handle = match device.open() {
                Ok(handle) => handle,
                Err(e) => {
                    last_open_error = Some(e.to_string());
                    continue;
                }
            };
            let text = |read: Result<String, rusb::Error>| read.unwrap_or_default();
            let manufacturer = text(handle.read_manufacturer_string_ascii(&descriptor));
            let product = text(handle.read_product_string_ascii(&descriptor));
            let serial = text(handle.read_serial_number_string_ascii(&descriptor));
            if !target.matches(
                descriptor.vendor_id(),
                descriptor.product_id(),
                &manufacturer,
                &product,
                &serial,
            ) {
                continue;
            }
            // Not every platform supports auto-detach; a failure only matters if the claim fails.
            let _ = handle.set_auto_detach_kernel_driver(true);
            if let Some(interface) = target.interface {
                handle
                    .claim_interface(interface)
                    .map_err(|e| down(format!("claim interface {interface}: {e}")))?;
            }
            return Ok(Box::new(LibusbDevice { handle }));
        }
        Err(down(match (candidates, last_open_error) {
            (0, _) => format!("{} is not plugged in", target.label()),
            (_, Some(why)) => format!(
                "{} cannot be opened ({why}); check the udev rule",
                target.label()
            ),
            (_, None) => format!(
                "no {} has the expected strings or serial number",
                target.label()
            ),
        }))
    }
}

struct LibusbDevice {
    handle: DeviceHandle<Context>,
}

impl UsbDevice for LibusbDevice {
    // The transfers below are one-line calls into rusb; they can only be exercised on a real device,
    // which no test touches, so they carry no anchor.
    fn control_in(
        &mut self,
        request_type: u8,
        request: u8,
        value: u16,
        index: u16,
        length: u16,
        timeout: Duration,
    ) -> Result<Vec<u8>, UsbFailure> {
        let mut buffer = vec![0u8; usize::from(length)];
        let n = self
            .handle
            .read_control(request_type, request, value, index, &mut buffer, timeout)
            .map_err(failure)?;
        buffer.truncate(n);
        Ok(buffer)
    }

    fn control_out(
        &mut self,
        request_type: u8,
        request: u8,
        value: u16,
        index: u16,
        data: &[u8],
        timeout: Duration,
    ) -> Result<usize, UsbFailure> {
        self.handle
            .write_control(request_type, request, value, index, data, timeout)
            .map_err(failure)
    }

    fn bulk_write(
        &mut self,
        endpoint: u8,
        data: &[u8],
        timeout: Duration,
    ) -> Result<usize, UsbFailure> {
        self.handle
            .write_bulk(endpoint, data, timeout)
            .map_err(failure)
    }

    fn bulk_read(
        &mut self,
        endpoint: u8,
        length: usize,
        timeout: Duration,
    ) -> Result<Vec<u8>, UsbFailure> {
        let mut buffer = vec![0u8; length];
        let n = self
            .handle
            .read_bulk(endpoint, &mut buffer, timeout)
            .map_err(failure)?;
        buffer.truncate(n);
        Ok(buffer)
    }

    fn interrupt_write(
        &mut self,
        endpoint: u8,
        data: &[u8],
        timeout: Duration,
    ) -> Result<usize, UsbFailure> {
        self.handle
            .write_interrupt(endpoint, data, timeout)
            .map_err(failure)
    }

    fn interrupt_read(
        &mut self,
        endpoint: u8,
        length: usize,
        timeout: Duration,
    ) -> Result<Vec<u8>, UsbFailure> {
        let mut buffer = vec![0u8; length];
        let n = self
            .handle
            .read_interrupt(endpoint, &mut buffer, timeout)
            .map_err(failure)?;
        buffer.truncate(n);
        Ok(buffer)
    }
}
