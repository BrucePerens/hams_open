// SPDX-License-Identifier: AGPL-3.0-or-later
//! The control-transfer layer under `usb::Vnwa`. `RusbTransport` is the production implementation (libusb); `Scripted` plays
//! back a fixed list of outcomes (including the transient errors the real unit produces) and logs what was sent. The driver
//! runs the same code over either, so the retry logic is tested on the path production uses.

use std::collections::VecDeque;
use std::sync::Mutex;
use std::time::Duration;

use rusb::UsbContext;

/// Why a control transfer failed, as far as recovery is concerned.
#[derive(Clone, Debug, PartialEq)]
pub enum TransferError {
    /// Worth sending again: the intermittent EPROTO (-71, libusb reports it as an I/O error), a stall, a timeout.
    Transient(String),
    /// The unit is gone from the bus (unplugged, or its hub port was switched off).
    NoDevice(String),
    /// Anything else (permission, invalid argument): sending again will not help.
    Other(String),
}

impl std::fmt::Display for TransferError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TransferError::Transient(m) => write!(f, "{m} (transient)"),
            TransferError::NoDevice(m) | TransferError::Other(m) => write!(f, "{m}"),
        }
    }
}

/// One control transfer, in either direction.
pub trait Transport: Send + Sync {
    fn write(
        &self,
        request_type: u8,
        request: u8,
        value: u16,
        index: u16,
        data: &[u8],
    ) -> Result<usize, TransferError>;
    fn read(
        &self,
        request: u8,
        value: u16,
        index: u16,
        buf: &mut [u8],
    ) -> Result<usize, TransferError>;
}

const TIMEOUT: Duration = Duration::from_millis(1000);
/// `bmRequestType` of a vendor read (device to host).
const VENDOR_IN: u8 = 0xC0;

/// Classifies a libusb error. libusb maps the kernel's -EPROTO to `Io`.
pub fn classify(e: rusb::Error) -> TransferError {
    // [@ANCHOR: vnwa_open_transfer_classes]
    match e {
        rusb::Error::Io | rusb::Error::Pipe | rusb::Error::Timeout | rusb::Error::Overflow => {
            TransferError::Transient(e.to_string())
        }
        rusb::Error::NoDevice | rusb::Error::NotFound => TransferError::NoDevice(e.to_string()),
        _ => TransferError::Other(e.to_string()),
    }
}

pub struct RusbTransport {
    handle: rusb::DeviceHandle<rusb::Context>,
}

impl RusbTransport {
    pub fn open(vid: u16, pid: u16) -> Result<RusbTransport, String> {
        let context = rusb::Context::new().map_err(|e| format!("libusb: {e}"))?;
        let handle = context.open_device_with_vid_pid(vid, pid).ok_or_else(|| {
            format!("no VNWA ({vid:04x}:{pid:04x}) found, or no permission to open it")
        })?;
        Ok(RusbTransport { handle })
    }
}

impl Transport for RusbTransport {
    fn write(
        &self,
        request_type: u8,
        request: u8,
        value: u16,
        index: u16,
        data: &[u8],
    ) -> Result<usize, TransferError> {
        self.handle
            .write_control(request_type, request, value, index, data, TIMEOUT)
            .map_err(classify)
    }

    fn read(
        &self,
        request: u8,
        value: u16,
        index: u16,
        buf: &mut [u8],
    ) -> Result<usize, TransferError> {
        self.handle
            .read_control(VENDOR_IN, request, value, index, buf, TIMEOUT)
            .map_err(classify)
    }
}

/// One transfer seen by `Scripted`: the request number and whether it was a write.
#[derive(Clone, Debug, PartialEq)]
pub struct Seen {
    pub write: bool,
    pub request: u8,
}

/// Plays back outcomes in order; once the script is used up every transfer succeeds with the bytes of the last `Ok` (or none).
pub struct Scripted {
    outcomes: Mutex<VecDeque<Result<Vec<u8>, TransferError>>>,
    seen: Mutex<Vec<Seen>>,
}

impl Scripted {
    pub fn new(outcomes: Vec<Result<Vec<u8>, TransferError>>) -> Scripted {
        Scripted {
            outcomes: Mutex::new(outcomes.into()),
            seen: Mutex::new(Vec::new()),
        }
    }

    pub fn seen(&self) -> Vec<Seen> {
        self.seen.lock().unwrap().clone()
    }

    fn next(&self, write: bool, request: u8) -> Result<Vec<u8>, TransferError> {
        self.seen.lock().unwrap().push(Seen { write, request });
        self.outcomes
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or(Ok(Vec::new()))
    }
}

impl Transport for Scripted {
    fn write(
        &self,
        _request_type: u8,
        request: u8,
        _value: u16,
        _index: u16,
        data: &[u8],
    ) -> Result<usize, TransferError> {
        self.next(true, request).map(|_| data.len())
    }

    fn read(
        &self,
        request: u8,
        _value: u16,
        _index: u16,
        buf: &mut [u8],
    ) -> Result<usize, TransferError> {
        let bytes = self.next(false, request)?;
        let n = bytes.len().min(buf.len());
        buf[..n].copy_from_slice(&bytes[..n]);
        Ok(n)
    }
}
