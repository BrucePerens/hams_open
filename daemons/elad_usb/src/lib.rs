// SPDX-License-Identifier: AGPL-3.0-or-later
//! A receive-only driver for ELAD USB receivers (FDM-DUO, FDM-S2, FDM-S1).
//!
//! Design rules:
//! * **Receive only, by construction.** The only requests this crate can put on the wire are the ones in
//!   [`protocol::is_receive_request`]: firmware version, EEPROM read, tune, front-end filter and attenuator,
//!   and the sample FIFO. There is no CAT write, no PTT, no transmit anything.
//! * **No vendor code and no blob.** An FDM-S1 or FDM-S2 needs its FPGA loaded by a vendor image before it
//!   sends samples. This crate never loads one: [`fpga::FpgaState`] says so and [`radio::Radio::open`]
//!   refuses such a unit until the caller states that the FPGA is already loaded
//!   (`hams_com/night_shift_questions/open/elad-fpga-image-for-fdm-s1-s2-*.md`).
//! * **Sans-IO core.** [`protocol`] and [`convert`] build and parse bytes; [`transport::Transport`] is the one
//!   seam to the device, with [`transport::FakeTransport`] for tests and `usb::UsbTransport` (feature `usb`).
//! * **Everything is `verified=unverified`** until it has run against a real unit (see README.md).
#![doc = "Modules: [`protocol`], [`convert`], [`fpga`], [`transport`], [`radio`], `usb` (feature)."]

pub mod convert;
pub mod fpga;
pub mod protocol;
pub mod radio;
#[cfg(test)]
mod tests;
pub mod transport;
#[cfg(feature = "usb")]
pub mod usb;

/// Everything that can go wrong.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// A USB id that is not a known ELAD receiver.
    UnknownDevice {
        /// USB product id.
        pid: u16,
    },
    /// The unit needs a vendor FPGA image and the caller did not say it is loaded.
    FpgaImageRequired,
    /// No sample arrived: on an FDM-S1/S2 this is what an unloaded FPGA looks like.
    NoSamples,
    /// A bulk read timed out.
    Timeout,
    /// The request is not one of the receive-only requests.
    NotReceiveOnly(u8),
    /// A value outside what the unit accepts.
    OutOfRange(String),
    /// The unit answered with something this crate cannot read.
    BadReply(String),
    /// The USB layer failed.
    Usb(String),
}

impl std::fmt::Display for Error {
    // [@ANCHOR: elad_error_display]
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::UnknownDevice { pid } => write!(f, "USB product id {pid:#06x} is not a known ELAD receiver"),
            Error::FpgaImageRequired => write!(
                f,
                "this ELAD receiver (FDM-S1/S2) needs its FPGA image loaded by ELAD's own loader, which this software does not include or run; load it yourself and say so, or use an FDM-DUO"
            ),
            Error::NoSamples => write!(f, "the receiver delivered no samples (an FDM-S1/S2 with no FPGA image loaded does this)"),
            Error::Timeout => write!(f, "the USB read timed out"),
            Error::NotReceiveOnly(r) => write!(f, "request {r:#04x} is not a receive-only request and is refused"),
            Error::OutOfRange(m) => write!(f, "out of range: {m}"),
            Error::BadReply(m) => write!(f, "unreadable reply from the receiver: {m}"),
            Error::Usb(m) => write!(f, "USB error: {m}"),
        }
    }
}

impl std::error::Error for Error {}
