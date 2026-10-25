// SPDX-License-Identifier: AGPL-3.0-or-later
//! `rsp_open`: an open driver for the SDRplay RSP2pro (USB 1df7:3010).
//!
//! Written from the private wire-protocol description, a description of the receiver's USB traffic recorded
//! from Bruce Perens's own unit. See README.md for provenance.
//!
//! The driver supports only a receiver whose firmware is already running (bcdDevice 2.07,
//! i.e. SDRplay's software has opened it since power-up). A receiver in its boot-ROM state
//! (bcdDevice 2.00) is reported as [`Error::NotRunning`]; this crate contains no firmware and
//! no firmware-loading code.
//!
//! ```no_run
//! use rsp_open::{Rsp2, Antenna};
//! let mut rx = Rsp2::open()?;
//! rx.set_frequency(95_800_000)?;
//! rx.set_gain(40, 0)?;
//! rx.select_antenna(Antenna::A)?;
//! let it = rx.start_stream_iter(64)?;
//! for chunk in it.take(10) { println!("{} samples", chunk.len()); }
//! rx.stop_stream()?;
//! rx.close()?;
//! # Ok::<(), rsp_open::Error>(())
//! ```

pub mod device;
pub mod dsp;
pub mod encode;
pub mod stream;
pub mod transport;
pub mod usb;

pub use device::{Rsp2, SampleIter, Settings};
pub use encode::{Antenna, Bandwidth, IfMode};
pub use stream::{IqF32, IqI16, StreamStats};

/// SDRplay's USB vendor ID.
pub const VID_SDRPLAY: u16 = 0x1df7;
/// RSP2 / RSP2pro product ID.
pub const PID_RSP2: u16 = 0x3010;
/// bcdDevice of the boot-ROM personality (no firmware loaded).
pub const BCD_BOOT_ROM: u16 = 0x0200;
/// bcdDevice once SDRplay's software has loaded the firmware.
pub const BCD_RUNNING: u16 = 0x0207;

/// Receiver models by USB product ID (public USB ID lists; only the RSP2 family is driven).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Model {
    Rsp1,
    Rsp1a,
    Rsp2,
    RspDuo,
    RspDx,
    Rsp1b,
    RspDxR2,
    Unknown,
}

impl Model {
    pub fn name(self) -> &'static str {
        match self {
            Model::Rsp1 => "RSP1",
            Model::Rsp1a => "RSP1A",
            Model::Rsp2 => "RSP2/RSP2pro",
            Model::RspDuo => "RSPduo",
            Model::RspDx => "RSPdx",
            Model::Rsp1b => "RSP1B (unconfirmed PID)",
            Model::RspDxR2 => "RSPdx-R2",
            Model::Unknown => "unknown SDRplay device",
        }
    }
}

pub fn model_for_pid(pid: u16) -> Model {
    match pid {
        0x2500 => Model::Rsp1,
        0x3000 => Model::Rsp1a,
        0x3010 => Model::Rsp2,
        0x3020 => Model::RspDuo,
        0x3030 => Model::RspDx,
        0x3050 => Model::Rsp1b,
        0x3060 => Model::RspDxR2,
        _ => Model::Unknown,
    }
}

/// A receiver found on the bus (from its device descriptor only).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeviceInfo {
    pub bus: u8,
    pub address: u8,
    pub vid: u16,
    pub pid: u16,
    pub bcd_device: u16,
    pub model: Model,
}

impl DeviceInfo {
    pub fn is_running(&self) -> bool {
        self.bcd_device == BCD_RUNNING
    }
}

/// Errors.
#[derive(Debug)]
pub enum Error {
    /// No RSP found.
    NotFound,
    /// The receiver is in its boot-ROM state (bcdDevice 2.00).
    NotRunning,
    /// bcdDevice is neither 2.00 nor 2.07: an unknown personality, not driven.
    UnknownState {
        bcd_device: u16,
    },
    /// An SDRplay receiver other than the RSP2 family.
    UnsupportedModel {
        pid: u16,
        name: &'static str,
    },
    Usb {
        what: String,
        code: i32,
        name: String,
    },
    InvalidArgument(String),
    /// A setting this driver does not implement (usually because its wire behaviour is not
    /// fully documented).
    Unsupported(String),
    Stream(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::NotFound => write!(f, "no SDRplay RSP2/RSP2pro found"),
            Error::NotRunning => write!(
                f,
                "device has no firmware loaded; load it with SDRplay's software first"
            ),
            Error::UnknownState { bcd_device } => {
                write!(f, "device reports bcdDevice {bcd_device:04x}, which this driver does not know; refusing to drive it")
            }
            Error::UnsupportedModel { pid, name } => write!(
                f,
                "{name} (PID {pid:04x}) is not supported yet; only the RSP2/RSP2pro is"
            ),
            Error::Usb { what, code, name } => write!(f, "USB error in {what}: {name} ({code})"),
            Error::InvalidArgument(s) => write!(f, "invalid argument: {s}"),
            Error::Unsupported(s) => write!(f, "unsupported: {s}"),
            Error::Stream(s) => write!(f, "stream error: {s}"),
        }
    }
}

impl std::error::Error for Error {}

/// Check a device descriptor's state before anything is sent to it.
pub fn check_state(pid: u16, bcd_device: u16) -> Result<(), Error> {
    if pid != PID_RSP2 {
        return Err(Error::UnsupportedModel {
            pid,
            name: model_for_pid(pid).name(),
        });
    }
    match bcd_device {
        BCD_RUNNING => Ok(()),
        BCD_BOOT_ROM => Err(Error::NotRunning),
        other => Err(Error::UnknownState { bcd_device: other }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_detection() {
        assert!(check_state(0x3010, 0x0207).is_ok());
        let e = check_state(0x3010, 0x0200).unwrap_err();
        assert!(matches!(e, Error::NotRunning));
        assert_eq!(
            e.to_string(),
            "device has no firmware loaded; load it with SDRplay's software first"
        );
        assert!(matches!(
            check_state(0x3010, 0x0208),
            Err(Error::UnknownState { bcd_device: 0x0208 })
        ));
        assert!(matches!(
            check_state(0x3000, 0x0207),
            Err(Error::UnsupportedModel { pid: 0x3000, .. })
        ));
    }
}
