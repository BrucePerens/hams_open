// SPDX-License-Identifier: AGPL-3.0-or-later
//! Dispatch for the `usb-device` family: from a description to its engine, its virtual twin (a
//! [`UsbDevice`] to run over [`radioctl_core::usb_device::FakeUsbBackend`]) and the
//! [`UsbTarget`] to look for on the bus.
//!
//! The Elektor SDR-USB has a description but no engine, for the reason [`engine_for`] states: it
//! needs an FTDI bit-bang I2C programmer for its Cypress PLL, which is not written.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use radioctl_core::description::{Family, RigDescription, UsbProtocol};
use radioctl_core::engine::ProtocolEngine;
use radioctl_core::error::{ErrorKind, RigError};
use radioctl_core::usb_device::{UsbBackend, UsbDevice, UsbTarget};
use radioctl_core::usb_wire::{execute, FrameError, UsbReply, UsbStatus, UsbTransfer};

// [@ANCHOR: radioctl_usb_family_usb_of]
fn usb_of(desc: &RigDescription) -> Result<&radioctl_core::description::UsbParams, RigError> {
    match (&desc.family, &desc.usb) {
        (Family::UsbDevice, Some(usb)) => Ok(usb),
        _ => Err(RigError::new(
            ErrorKind::Unsupported,
            "not a USB device description",
        )),
    }
}

fn no_engine(desc: &RigDescription, why: &str) -> RigError {
    RigError::new(
        ErrorKind::Unsupported,
        format!("{} has no engine yet: {why}", desc.name),
    )
}

// [@ANCHOR: radioctl_usb_family_engine_for]
/// The engine of a USB-device description, or why there is none.
pub fn engine_for(desc: &RigDescription) -> Result<Box<dyn ProtocolEngine>, RigError> {
    Ok(match usb_of(desc)?.protocol {
        UsbProtocol::Si570 | UsbProtocol::Fifisdr => {
            Box::new(crate::si570_usb::Si570UsbEngine::new(desc.clone())?)
        }
        UsbProtocol::Funcube => Box::new(crate::funcube::FuncubeEngine::new(desc.clone())?),
        UsbProtocol::Elektor507 => {
            return Err(no_engine(
                desc,
                "it programs a Cypress PLL through an FTDI chip in bit-bang mode, which is not implemented",
            ))
        }
        UsbProtocol::Dwt => Box::new(crate::dwt::DwtEngine::new(desc.clone())?),
        UsbProtocol::Perseus => Box::new(crate::perseus_engine::PerseusEngine::new(desc.clone())?),
    })
}

// [@ANCHOR: radioctl_usb_family_twin_for]
/// The virtual device of a USB-device description, for the fake backend.
pub fn twin_for(desc: &RigDescription) -> Result<Box<dyn UsbDevice>, RigError> {
    Ok(match usb_of(desc)?.protocol {
        UsbProtocol::Si570 | UsbProtocol::Fifisdr => {
            Box::new(crate::si570_usb::Si570Twin::new(desc)?)
        }
        UsbProtocol::Funcube => Box::new(crate::funcube::FuncubeTwin::new(desc, false)?),
        UsbProtocol::Dwt => Box::new(crate::dwt::DwtTwin::default()),
        UsbProtocol::Perseus => {
            crate::perseus::ReadyVirtualPerseus::new(1).open(&target_for(desc)?)?
        }
        UsbProtocol::Elektor507 => return Err(no_engine(desc, "no virtual twin")),
    })
}

// [@ANCHOR: radioctl_usb_family_target_for]
/// What to look for on the USB bus for a description (any serial number; the daemon's
/// configuration narrows it).
pub fn target_for(desc: &RigDescription) -> Result<UsbTarget, RigError> {
    let usb = usb_of(desc)?;
    Ok(UsbTarget {
        vendor_id: usb.vendor_id,
        product_id: usb.product_id,
        manufacturer: usb.manufacturer.clone(),
        product: usb.product.clone(),
        serial: None,
        interface: (usb.protocol == UsbProtocol::Funcube).then_some(crate::funcube::INTERFACE),
    })
}

// [@ANCHOR: radioctl_usb_family_backend_for]
/// The backend to open a description's device through: `inner` itself, except for a board that
/// needs the user's own firmware loaded when it is opened (the Perseus), whose backend loads it
/// from `firmware_dir`, else `env_dir` (`RADIOCTL_PERSEUS_FIRMWARE_DIR`), else
/// `$HOME/.config/radioctl/firmware/perseus` (`home` is `$HOME`). `real_time` false skips the
/// pauses of a real board (for a virtual one).
pub fn backend_for(
    desc: &RigDescription,
    inner: Arc<dyn UsbBackend>,
    firmware_dir: Option<&Path>,
    env_dir: Option<&str>,
    home: Option<&str>,
    real_time: bool,
) -> Result<Arc<dyn UsbBackend>, RigError> {
    if usb_of(desc)?.protocol != UsbProtocol::Perseus {
        return Ok(inner);
    }
    let dir = firmware_dir
        .map(Path::to_path_buf)
        .or_else(|| crate::perseus::PerseusFiles::default_dir(env_dir, home))
        .ok_or_else(|| {
            RigError::new(
                ErrorKind::Unsupported,
                "perseus: no firmware directory: set RADIOCTL_PERSEUS_FIRMWARE_DIR or HOME, or \
                 firmware_dir in the rig's transport"
                    .to_string(),
            )
        })?;
    Ok(Arc::new(crate::perseus::PerseusLoaderBackend::new(
        inner,
        crate::perseus::PerseusFiles::in_dir(&dir),
        real_time,
    )))
}

/// A virtual USB device as a byte-in, byte-out stand-in: request frames in, reply frames out.
/// The conformance harness replays transcripts through it, which is the wire the engine produces.
pub struct UsbFrameStandIn {
    device: Box<dyn UsbDevice>,
    pending: Vec<u8>,
}

impl UsbFrameStandIn {
    // [@ANCHOR: radioctl_usb_family_stand_in_new]
    /// The stand-in for a description.
    pub fn new(desc: &RigDescription) -> Result<UsbFrameStandIn, RigError> {
        Ok(UsbFrameStandIn {
            device: twin_for(desc)?,
            pending: Vec::new(),
        })
    }

    // [@ANCHOR: radioctl_usb_family_stand_in_feed]
    /// Performs every complete request frame in `bytes` on the device; returns the reply frames.
    pub fn feed(&mut self, bytes: &[u8]) -> Vec<u8> {
        self.pending.extend_from_slice(bytes);
        let mut out = Vec::new();
        loop {
            match UsbTransfer::decode(&self.pending) {
                Ok((transfer, used)) => {
                    self.pending.drain(..used);
                    out.extend(
                        execute(self.device.as_mut(), &transfer, Duration::from_secs(1)).encode(),
                    );
                }
                Err(FrameError::Incomplete) => break,
                Err(FrameError::Malformed) => {
                    self.pending.clear();
                    out.extend(
                        UsbReply {
                            op: 0,
                            status: UsbStatus::Other,
                            data: Vec::new(),
                        }
                        .encode(),
                    );
                    break;
                }
            }
        }
        out
    }
}
