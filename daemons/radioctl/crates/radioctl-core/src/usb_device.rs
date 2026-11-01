// SPDX-License-Identifier: AGPL-3.0-or-later
//! The device-access seam of the USB transport: what radioctl needs from a USB device, behind
//! two traits, so the code that opens and drives the device can be replaced without touching an
//! engine.
//!
//! * [`UsbBackend`] opens a device found by vendor id, product id, the manufacturer and product
//!   strings and an optional serial number (the V-USB boards of the Si570 kits all share
//!   16C0:05DC and differ only by their strings). It is called again after every loss, which is
//!   the hot-plug reconnect: a backend that cannot find the device returns an error and the link
//!   retries with backoff.
//! * [`UsbDevice`] is one open device: control transfers in and out, bulk and interrupt transfers.
//!
//! Three implementations exist. [`crate::usb_libusb::LibusbBackend`] (cargo feature `usb`) opens
//! the device directly through libusb. [`FakeUsbBackend`] serves a scripted or virtual device and
//! is what every test uses: no test touches real USB hardware. The relay's zero-privilege broker
//! (`hams_com` `relay_broker`) will implement the same two traits over a file descriptor it
//! hands over, or over a proxy, and the engines and [`crate::usb_link`] do not change.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use crate::error::{ErrorKind, RigError};

/// How to find one USB device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsbTarget {
    /// USB vendor id.
    pub vendor_id: u16,
    /// USB product id.
    pub product_id: u16,
    /// The manufacturer string the device must report; empty matches any.
    pub manufacturer: String,
    /// The product string the device must report; empty matches any.
    pub product: String,
    /// The serial number string the device must report; `None` matches any (two identical
    /// boards on one host need it).
    pub serial: Option<String>,
    /// The interface to claim after opening (FUNcube: 2), when the device has one to claim.
    pub interface: Option<u8>,
}

impl UsbTarget {
    // [@ANCHOR: radioctl_core_usb_target_matches]
    /// Whether a device that reports these ids and strings is the one asked for.
    pub fn matches(
        &self,
        vid: u16,
        pid: u16,
        manufacturer: &str,
        product: &str,
        serial: &str,
    ) -> bool {
        self.vendor_id == vid
            && self.product_id == pid
            && (self.manufacturer.is_empty() || self.manufacturer == manufacturer)
            && (self.product.is_empty() || self.product == product)
            && self.serial.as_deref().is_none_or(|s| s == serial)
    }

    // [@ANCHOR: radioctl_core_usb_target_label]
    /// `usb:VVVV:PPPP` (with `:serial` when one is set), for status and logs.
    pub fn label(&self) -> String {
        match &self.serial {
            Some(serial) => format!(
                "usb:{:04x}:{:04x}:{serial}",
                self.vendor_id, self.product_id
            ),
            None => format!("usb:{:04x}:{:04x}", self.vendor_id, self.product_id),
        }
    }
}

/// Why one transfer failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UsbFailure {
    /// The device did not answer in time.
    Timeout,
    /// The device is gone (unplugged, reset, permission revoked): the link must reopen it.
    NoDevice,
    /// The device refused the request (a stall, "pipe" error): the link stays up.
    Stall,
    /// Anything else, with the backend's wording.
    Other(String),
}

/// One open USB device. All transfers block up to `timeout`; the link calls them from its own
/// thread, never from a runtime worker.
pub trait UsbDevice: Send {
    /// A control transfer from the device (`request_type` has the direction bit set).
    fn control_in(
        &mut self,
        request_type: u8,
        request: u8,
        value: u16,
        index: u16,
        length: u16,
        timeout: Duration,
    ) -> Result<Vec<u8>, UsbFailure>;

    /// A control transfer to the device; returns the number of bytes accepted.
    fn control_out(
        &mut self,
        request_type: u8,
        request: u8,
        value: u16,
        index: u16,
        data: &[u8],
        timeout: Duration,
    ) -> Result<usize, UsbFailure>;

    /// Writes `data` to a bulk OUT endpoint.
    fn bulk_write(
        &mut self,
        endpoint: u8,
        data: &[u8],
        timeout: Duration,
    ) -> Result<usize, UsbFailure>;

    /// Reads up to `length` bytes from a bulk IN endpoint.
    fn bulk_read(
        &mut self,
        endpoint: u8,
        length: usize,
        timeout: Duration,
    ) -> Result<Vec<u8>, UsbFailure>;

    /// Writes `data` to an interrupt OUT endpoint (a HID output report).
    fn interrupt_write(
        &mut self,
        endpoint: u8,
        data: &[u8],
        timeout: Duration,
    ) -> Result<usize, UsbFailure>;

    /// Reads up to `length` bytes from an interrupt IN endpoint (a HID input report).
    fn interrupt_read(
        &mut self,
        endpoint: u8,
        length: usize,
        timeout: Duration,
    ) -> Result<Vec<u8>, UsbFailure>;
}

/// Finds and opens USB devices.
pub trait UsbBackend: Send + Sync + 'static {
    /// Opens the one device that matches `target`, claiming its interface when the target names
    /// one. Called again after every loss. Errors are [`ErrorKind::LinkDown`] when the device
    /// is absent and [`ErrorKind::Io`]-like failures otherwise; the link treats all as "retry".
    fn open(&self, target: &UsbTarget) -> Result<Box<dyn UsbDevice>, RigError>;
}

fn locked<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// A transfer a [`FakeUsbBackend`] device received, as the tests inspect it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FakeTransfer {
    /// `control_in`: request type, request, value, index, length.
    ControlIn(u8, u8, u16, u16, u16),
    /// `control_out`: request type, request, value, index, data.
    ControlOut(u8, u8, u16, u16, Vec<u8>),
    /// `bulk_write`: endpoint, data.
    BulkWrite(u8, Vec<u8>),
    /// `bulk_read`: endpoint, length.
    BulkRead(u8, usize),
    /// `interrupt_write`: endpoint, data.
    InterruptWrite(u8, Vec<u8>),
    /// `interrupt_read`: endpoint, length.
    InterruptRead(u8, usize),
}

struct FakeShared {
    present: AtomicBool,
    opens: AtomicU32,
    log: Mutex<Vec<FakeTransfer>>,
    device: Mutex<Box<dyn UsbDevice>>,
}

/// A backend that serves one in-memory device (a virtual twin, or a scripted fake) that can be
/// unplugged and plugged back in. Clone it: all clones are the same bus.
#[derive(Clone)]
pub struct FakeUsbBackend {
    shared: Arc<FakeShared>,
}

impl FakeUsbBackend {
    // [@ANCHOR: radioctl_core_fake_usb_new]
    /// A bus with `device` plugged in. The device keeps its state across unplug and replug, as a
    /// real board with its own power does.
    pub fn new(device: Box<dyn UsbDevice>) -> FakeUsbBackend {
        FakeUsbBackend {
            shared: Arc::new(FakeShared {
                present: AtomicBool::new(true),
                opens: AtomicU32::new(0),
                log: Mutex::new(Vec::new()),
                device: Mutex::new(device),
            }),
        }
    }

    // [@ANCHOR: radioctl_core_fake_usb_unplug]
    /// Unplugs the device: open handles fail with [`UsbFailure::NoDevice`] and `open` fails.
    pub fn unplug(&self) {
        self.shared.present.store(false, Ordering::SeqCst);
    }

    // [@ANCHOR: radioctl_core_fake_usb_plug]
    /// Plugs the device back in.
    pub fn plug(&self) {
        self.shared.present.store(true, Ordering::SeqCst);
    }

    // [@ANCHOR: radioctl_core_fake_usb_opens]
    /// How many times the device has been opened.
    pub fn opens(&self) -> u32 {
        self.shared.opens.load(Ordering::SeqCst)
    }

    // [@ANCHOR: radioctl_core_fake_usb_log]
    /// Every transfer the device has received, oldest first.
    pub fn log(&self) -> Vec<FakeTransfer> {
        locked(&self.shared.log).clone()
    }
}

impl UsbBackend for FakeUsbBackend {
    // [@ANCHOR: radioctl_core_fake_usb_open]
    fn open(&self, target: &UsbTarget) -> Result<Box<dyn UsbDevice>, RigError> {
        if !self.shared.present.load(Ordering::SeqCst) {
            return Err(RigError::new(
                ErrorKind::LinkDown,
                format!("{} is not plugged in", target.label()),
            ));
        }
        self.shared.opens.fetch_add(1, Ordering::SeqCst);
        Ok(Box::new(FakeHandle {
            shared: Arc::clone(&self.shared),
        }))
    }
}

struct FakeHandle {
    shared: Arc<FakeShared>,
}

impl FakeHandle {
    // [@ANCHOR: radioctl_core_fake_usb_handle_enter]
    fn enter(
        &self,
        transfer: FakeTransfer,
    ) -> Result<MutexGuard<'_, Box<dyn UsbDevice>>, UsbFailure> {
        if !self.shared.present.load(Ordering::SeqCst) {
            return Err(UsbFailure::NoDevice);
        }
        locked(&self.shared.log).push(transfer);
        Ok(locked(&self.shared.device))
    }
}

impl UsbDevice for FakeHandle {
    // [@ANCHOR: radioctl_core_fake_usb_handle_transfers]
    fn control_in(
        &mut self,
        request_type: u8,
        request: u8,
        value: u16,
        index: u16,
        length: u16,
        timeout: Duration,
    ) -> Result<Vec<u8>, UsbFailure> {
        self.enter(FakeTransfer::ControlIn(
            request_type,
            request,
            value,
            index,
            length,
        ))?
        .control_in(request_type, request, value, index, length, timeout)
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
        self.enter(FakeTransfer::ControlOut(
            request_type,
            request,
            value,
            index,
            data.to_vec(),
        ))?
        .control_out(request_type, request, value, index, data, timeout)
    }

    fn bulk_write(
        &mut self,
        endpoint: u8,
        data: &[u8],
        timeout: Duration,
    ) -> Result<usize, UsbFailure> {
        self.enter(FakeTransfer::BulkWrite(endpoint, data.to_vec()))?
            .bulk_write(endpoint, data, timeout)
    }

    fn bulk_read(
        &mut self,
        endpoint: u8,
        length: usize,
        timeout: Duration,
    ) -> Result<Vec<u8>, UsbFailure> {
        self.enter(FakeTransfer::BulkRead(endpoint, length))?
            .bulk_read(endpoint, length, timeout)
    }

    fn interrupt_write(
        &mut self,
        endpoint: u8,
        data: &[u8],
        timeout: Duration,
    ) -> Result<usize, UsbFailure> {
        self.enter(FakeTransfer::InterruptWrite(endpoint, data.to_vec()))?
            .interrupt_write(endpoint, data, timeout)
    }

    fn interrupt_read(
        &mut self,
        endpoint: u8,
        length: usize,
        timeout: Duration,
    ) -> Result<Vec<u8>, UsbFailure> {
        self.enter(FakeTransfer::InterruptRead(endpoint, length))?
            .interrupt_read(endpoint, length, timeout)
    }
}
