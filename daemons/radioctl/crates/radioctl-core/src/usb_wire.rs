// SPDX-License-Identifier: AGPL-3.0-or-later
//! The frame the USB transport carries between a protocol engine and a [`crate::usb_device::UsbDevice`].
//!
//! A USB device has no byte stream, but the rig actor talks to every transport through one, and
//! the engines are sans-IO. So an engine encodes each operation as a *request frame* (a
//! [`UsbTransfer`]), the [`crate::usb_link`] decodes it, performs the transfer on the device and
//! writes a *reply frame* ([`UsbReply`]) back, which the engine decodes. Both frames are
//! little-endian and self-delimiting, so partial reads and writes are harmless.
//!
//! ```text
//! request: op, a, b, value(2), index(2), len(2), data(len, for the operations that send data)
//! reply:   op, status, len(2), data(len)
//! ```
//!
//! `op` is 1 control in, 2 control out, 3 bulk write, 4 bulk read, 5 interrupt write, 6 interrupt
//! read, 7 interrupt exchange (write a report, then read the answer, with no other transfer in
//! between: the FUNcube command and reply). For the control operations `a` is the request type
//! and `b` the request; for the others `a` is the endpoint (for an exchange the OUT endpoint)
//! and `b` the IN endpoint of an exchange. `len` is the data length to send or the length to
//! read; an exchange puts its read length in `value`. A reply carries the request's `op`.

use std::time::Duration;

use crate::usb_device::{UsbDevice, UsbFailure};

/// Longest data field either frame may carry; larger is a protocol error, not an allocation.
pub const MAX_DATA: usize = 4096;
const REQUEST_HEADER: usize = 9;
const REPLY_HEADER: usize = 4;

/// The status byte of a reply.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UsbStatus {
    /// The transfer completed.
    Ok,
    /// The device did not answer in time.
    Timeout,
    /// The device is gone.
    NoDevice,
    /// The device refused the request.
    Stall,
    /// Any other failure (including a short write).
    Other,
}

impl UsbStatus {
    // [@ANCHOR: radioctl_core_usb_status_code]
    /// The status byte.
    pub fn code(self) -> u8 {
        match self {
            UsbStatus::Ok => 0,
            UsbStatus::Timeout => 1,
            UsbStatus::NoDevice => 2,
            UsbStatus::Stall => 3,
            UsbStatus::Other => 4,
        }
    }

    // [@ANCHOR: radioctl_core_usb_status_from_code]
    /// The status of a byte; an unknown byte is [`UsbStatus::Other`].
    pub fn from_code(code: u8) -> UsbStatus {
        match code {
            0 => UsbStatus::Ok,
            1 => UsbStatus::Timeout,
            2 => UsbStatus::NoDevice,
            3 => UsbStatus::Stall,
            _ => UsbStatus::Other,
        }
    }
}

/// One USB transfer, as a request frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UsbTransfer {
    /// A control transfer from the device.
    ControlIn {
        /// Request type (direction, type and recipient bits).
        request_type: u8,
        /// Request number.
        request: u8,
        /// wValue.
        value: u16,
        /// wIndex.
        index: u16,
        /// Bytes to read.
        length: u16,
    },
    /// A control transfer to the device.
    ControlOut {
        /// Request type.
        request_type: u8,
        /// Request number.
        request: u8,
        /// wValue.
        value: u16,
        /// wIndex.
        index: u16,
        /// Data stage.
        data: Vec<u8>,
    },
    /// A bulk OUT transfer.
    BulkWrite {
        /// Endpoint address.
        endpoint: u8,
        /// Bytes to send.
        data: Vec<u8>,
    },
    /// A bulk IN transfer.
    BulkRead {
        /// Endpoint address.
        endpoint: u8,
        /// Bytes to read.
        length: u16,
    },
    /// An interrupt OUT transfer.
    InterruptWrite {
        /// Endpoint address.
        endpoint: u8,
        /// Bytes to send.
        data: Vec<u8>,
    },
    /// An interrupt IN transfer.
    InterruptRead {
        /// Endpoint address.
        endpoint: u8,
        /// Bytes to read.
        length: u16,
    },
    /// An interrupt OUT transfer followed at once by an interrupt IN transfer.
    InterruptExchange {
        /// OUT endpoint address.
        out_endpoint: u8,
        /// IN endpoint address.
        in_endpoint: u8,
        /// Bytes to send.
        data: Vec<u8>,
        /// Bytes to read back.
        read_length: u16,
    },
}

/// Why a frame could not be decoded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameError {
    /// More bytes are needed.
    Incomplete,
    /// The bytes are not a frame (unknown operation, or a data field over [`MAX_DATA`]).
    Malformed,
}

fn put_header(op: u8, a: u8, b: u8, value: u16, index: u16, len: u16, data: &[u8]) -> Vec<u8> {
    let mut out = vec![op, a, b];
    out.extend_from_slice(&value.to_le_bytes());
    out.extend_from_slice(&index.to_le_bytes());
    out.extend_from_slice(&len.to_le_bytes());
    out.extend_from_slice(data);
    out
}

fn data_len(data: &[u8]) -> u16 {
    u16::try_from(data.len()).unwrap_or(u16::MAX)
}

impl UsbTransfer {
    // [@ANCHOR: radioctl_core_usb_transfer_op]
    /// The operation byte of the frame.
    pub fn op(&self) -> u8 {
        match self {
            UsbTransfer::ControlIn { .. } => 1,
            UsbTransfer::ControlOut { .. } => 2,
            UsbTransfer::BulkWrite { .. } => 3,
            UsbTransfer::BulkRead { .. } => 4,
            UsbTransfer::InterruptWrite { .. } => 5,
            UsbTransfer::InterruptRead { .. } => 6,
            UsbTransfer::InterruptExchange { .. } => 7,
        }
    }

    // [@ANCHOR: radioctl_core_usb_transfer_encode]
    /// The request frame. Data longer than [`MAX_DATA`] is cut, which [`UsbTransfer::decode`]
    /// then reads back as a shorter transfer; engines never build such a transfer.
    pub fn encode(&self) -> Vec<u8> {
        let op = self.op();
        let cut = |data: &[u8]| data.get(..MAX_DATA).unwrap_or(data).to_vec();
        match self {
            UsbTransfer::ControlIn {
                request_type,
                request,
                value,
                index,
                length,
            } => put_header(op, *request_type, *request, *value, *index, *length, &[]),
            UsbTransfer::ControlOut {
                request_type,
                request,
                value,
                index,
                data,
            } => {
                let data = cut(data);
                put_header(
                    op,
                    *request_type,
                    *request,
                    *value,
                    *index,
                    data_len(&data),
                    &data,
                )
            }
            UsbTransfer::BulkWrite { endpoint, data }
            | UsbTransfer::InterruptWrite { endpoint, data } => {
                let data = cut(data);
                put_header(op, *endpoint, 0, 0, 0, data_len(&data), &data)
            }
            UsbTransfer::BulkRead { endpoint, length }
            | UsbTransfer::InterruptRead { endpoint, length } => {
                put_header(op, *endpoint, 0, 0, 0, *length, &[])
            }
            UsbTransfer::InterruptExchange {
                out_endpoint,
                in_endpoint,
                data,
                read_length,
            } => {
                let data = cut(data);
                put_header(
                    op,
                    *out_endpoint,
                    *in_endpoint,
                    *read_length,
                    0,
                    data_len(&data),
                    &data,
                )
            }
        }
    }

    // [@ANCHOR: radioctl_core_usb_transfer_decode]
    /// Decodes the first request frame in `bytes`; returns it and the number of bytes it used.
    pub fn decode(bytes: &[u8]) -> Result<(UsbTransfer, usize), FrameError> {
        let head = bytes.get(..REQUEST_HEADER).ok_or(FrameError::Incomplete)?;
        let word = |at: usize| {
            u16::from_le_bytes([
                head.get(at).copied().unwrap_or(0),
                head.get(at + 1).copied().unwrap_or(0),
            ])
        };
        let (op, a, b) = (
            head.first().copied().unwrap_or(0),
            head.get(1).copied().unwrap_or(0),
            head.get(2).copied().unwrap_or(0),
        );
        let (value, index, len) = (word(3), word(5), word(7));
        if usize::from(len) > MAX_DATA {
            return Err(FrameError::Malformed);
        }
        let sends = matches!(op, 2 | 3 | 5 | 7);
        let used = REQUEST_HEADER + if sends { usize::from(len) } else { 0 };
        let data = || -> Result<Vec<u8>, FrameError> {
            bytes
                .get(REQUEST_HEADER..used)
                .map(<[u8]>::to_vec)
                .ok_or(FrameError::Incomplete)
        };
        let transfer = match op {
            1 => UsbTransfer::ControlIn {
                request_type: a,
                request: b,
                value,
                index,
                length: len,
            },
            2 => UsbTransfer::ControlOut {
                request_type: a,
                request: b,
                value,
                index,
                data: data()?,
            },
            3 => UsbTransfer::BulkWrite {
                endpoint: a,
                data: data()?,
            },
            4 => UsbTransfer::BulkRead {
                endpoint: a,
                length: len,
            },
            5 => UsbTransfer::InterruptWrite {
                endpoint: a,
                data: data()?,
            },
            6 => UsbTransfer::InterruptRead {
                endpoint: a,
                length: len,
            },
            7 => UsbTransfer::InterruptExchange {
                out_endpoint: a,
                in_endpoint: b,
                data: data()?,
                read_length: value,
            },
            _ => return Err(FrameError::Malformed),
        };
        Ok((transfer, used))
    }
}

/// The reply frame to one [`UsbTransfer`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsbReply {
    /// The operation of the request this answers.
    pub op: u8,
    /// How the transfer ended.
    pub status: UsbStatus,
    /// Bytes read from the device (empty for a write, or on failure).
    pub data: Vec<u8>,
}

impl UsbReply {
    // [@ANCHOR: radioctl_core_usb_reply_encode]
    /// The reply frame.
    pub fn encode(&self) -> Vec<u8> {
        let data = self.data.get(..MAX_DATA).unwrap_or(&self.data);
        let mut out = vec![self.op, self.status.code()];
        out.extend_from_slice(&data_len(data).to_le_bytes());
        out.extend_from_slice(data);
        out
    }

    // [@ANCHOR: radioctl_core_usb_reply_decode]
    /// Decodes the first reply frame in `bytes`; returns it and the number of bytes it used.
    pub fn decode(bytes: &[u8]) -> Result<(UsbReply, usize), FrameError> {
        let head = bytes.get(..REPLY_HEADER).ok_or(FrameError::Incomplete)?;
        let len = usize::from(u16::from_le_bytes([
            head.get(2).copied().unwrap_or(0),
            head.get(3).copied().unwrap_or(0),
        ]));
        if len > MAX_DATA {
            return Err(FrameError::Malformed);
        }
        let data = bytes
            .get(REPLY_HEADER..REPLY_HEADER + len)
            .ok_or(FrameError::Incomplete)?;
        Ok((
            UsbReply {
                op: head.first().copied().unwrap_or(0),
                status: UsbStatus::from_code(head.get(1).copied().unwrap_or(0)),
                data: data.to_vec(),
            },
            REPLY_HEADER + len,
        ))
    }
}

fn status_of(failure: &UsbFailure) -> UsbStatus {
    match failure {
        UsbFailure::Timeout => UsbStatus::Timeout,
        UsbFailure::NoDevice => UsbStatus::NoDevice,
        UsbFailure::Stall => UsbStatus::Stall,
        UsbFailure::Other(_) => UsbStatus::Other,
    }
}

// [@ANCHOR: radioctl_core_usb_execute]
/// Performs `transfer` on `device` and returns its reply. A write that moves fewer bytes than it
/// was given is [`UsbStatus::Other`]: a half-sent command must not read as success.
pub fn execute(device: &mut dyn UsbDevice, transfer: &UsbTransfer, timeout: Duration) -> UsbReply {
    let op = transfer.op();
    let wrote = |result: Result<usize, UsbFailure>, wanted: usize| match result {
        Ok(n) if n == wanted => (UsbStatus::Ok, Vec::new()),
        Ok(_) => (UsbStatus::Other, Vec::new()),
        Err(failure) => (status_of(&failure), Vec::new()),
    };
    let read = |result: Result<Vec<u8>, UsbFailure>| match result {
        Ok(data) => (UsbStatus::Ok, data),
        Err(failure) => (status_of(&failure), Vec::new()),
    };
    let (status, data) = match transfer {
        UsbTransfer::ControlIn {
            request_type,
            request,
            value,
            index,
            length,
        } => read(device.control_in(*request_type, *request, *value, *index, *length, timeout)),
        UsbTransfer::ControlOut {
            request_type,
            request,
            value,
            index,
            data,
        } => wrote(
            device.control_out(*request_type, *request, *value, *index, data, timeout),
            data.len(),
        ),
        UsbTransfer::BulkWrite { endpoint, data } => {
            wrote(device.bulk_write(*endpoint, data, timeout), data.len())
        }
        UsbTransfer::BulkRead { endpoint, length } => {
            read(device.bulk_read(*endpoint, usize::from(*length), timeout))
        }
        UsbTransfer::InterruptWrite { endpoint, data } => {
            wrote(device.interrupt_write(*endpoint, data, timeout), data.len())
        }
        UsbTransfer::InterruptRead { endpoint, length } => {
            read(device.interrupt_read(*endpoint, usize::from(*length), timeout))
        }
        UsbTransfer::InterruptExchange {
            out_endpoint,
            in_endpoint,
            data,
            read_length,
        } => {
            match wrote(
                device.interrupt_write(*out_endpoint, data, timeout),
                data.len(),
            ) {
                (UsbStatus::Ok, _) => {
                    read(device.interrupt_read(*in_endpoint, usize::from(*read_length), timeout))
                }
                failed => failed,
            }
        }
    };
    UsbReply { op, status, data }
}
