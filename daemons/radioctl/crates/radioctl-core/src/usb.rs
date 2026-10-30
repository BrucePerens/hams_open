// SPDX-License-Identifier: AGPL-3.0-or-later
//! Finding a serial port by what it is, not where it happens to be today.
//!
//! USB serial devices get a different `/dev/ttyUSBn` each time they are plugged in, so a rig is
//! named by USB vendor id, product id and (when the chip has one) serial number. The matching and
//! chip classification here are pure functions; [`list_ports`] reads the system. On Linux the
//! list comes from sysfs (no libudev needed); elsewhere from the `serialport` crate.
//!
//! The chip table lists the USB-to-serial families rig interfaces use: FTDI (`0403`), Silicon
//! Labs CP210x (`10c4`), WCH CH340/CH341/CH9102 (`1a86`). A device that implements the USB CDC
//! ACM class has no vendor in common, so it is matched by explicit vid/pid (or by path).

use std::fs;
use std::path::{Path, PathBuf};

use crate::error::{ErrorKind, RigError};

/// Vendor id of FTDI.
pub const VID_FTDI: u16 = 0x0403;
/// Vendor id of Silicon Labs (CP210x).
pub const VID_SILABS: u16 = 0x10C4;
/// Vendor id of WCH (CH340, CH341, CH9102).
pub const VID_WCH: u16 = 0x1A86;

/// The family of the USB-to-serial chip behind a port.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Chip {
    /// FTDI FT232, FT2232 and relatives.
    Ftdi,
    /// Silicon Labs CP210x.
    Cp210x,
    /// WCH CH340, CH341, CH9102.
    Ch340,
    /// A USB CDC ACM device (native USB serial of the radio or a microcontroller).
    CdcAcm,
    /// Not recognised from the ids alone.
    Unknown,
}

/// What is known about one serial port.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortInfo {
    /// The device path to open.
    pub path: PathBuf,
    /// USB vendor id, if the port is USB.
    pub vid: Option<u16>,
    /// USB product id, if the port is USB.
    pub pid: Option<u16>,
    /// USB serial number string, if the device has one.
    pub serial: Option<String>,
    /// Product string, if known.
    pub product: Option<String>,
    /// True when the kernel driver for the port is `cdc_acm` (Linux).
    pub cdc_acm: bool,
}

/// What to look for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsbMatcher {
    /// Required vendor id.
    pub vid: u16,
    /// Required product id.
    pub pid: u16,
    /// Required serial number; `None` accepts any (then more than one match is an error).
    pub serial: Option<String>,
}

// [@ANCHOR: radioctl_core_usb_classify]
/// The chip family for a USB id, or [`Chip::CdcAcm`] when the kernel bound the CDC ACM driver.
pub fn classify(vid: u16, pid: u16, cdc_acm: bool) -> Chip {
    match (vid, pid) {
        (VID_FTDI, _) => Chip::Ftdi,
        (VID_SILABS, 0xEA60..=0xEA71) => Chip::Cp210x,
        (VID_WCH, 0x7522 | 0x7523 | 0x5523 | 0x55D4 | 0x55D3) => Chip::Ch340,
        _ if cdc_acm => Chip::CdcAcm,
        _ => Chip::Unknown,
    }
}

impl PortInfo {
    // [@ANCHOR: radioctl_core_usb_port_chip]
    /// The chip family of this port ([`Chip::Unknown`] for a non-USB port).
    pub fn chip(&self) -> Chip {
        match (self.vid, self.pid) {
            (Some(vid), Some(pid)) => classify(vid, pid, self.cdc_acm),
            _ => Chip::Unknown,
        }
    }
}

// [@ANCHOR: radioctl_core_usb_select_port]
/// The one port that matches, or an error that says whether none or several matched. Several
/// matches are never resolved by guessing: two identical interfaces need their serial numbers.
pub fn select_port<'a>(
    matcher: &UsbMatcher,
    ports: &'a [PortInfo],
) -> Result<&'a PortInfo, RigError> {
    let found: Vec<&PortInfo> = ports
        .iter()
        .filter(|p| p.vid == Some(matcher.vid) && p.pid == Some(matcher.pid))
        .filter(|p| matcher.serial.is_none() || p.serial == matcher.serial)
        .collect();
    match found.as_slice() {
        [one] => Ok(one),
        [] => Err(RigError::new(
            ErrorKind::LinkDown,
            format!(
                "no serial port with USB id {:04x}:{:04x}{}",
                matcher.vid,
                matcher.pid,
                matcher
                    .serial
                    .as_ref()
                    .map(|s| format!(" serial {s}"))
                    .unwrap_or_default()
            ),
        )),
        many => Err(RigError::new(
            ErrorKind::Unsupported,
            format!(
                "{} serial ports match USB id {:04x}:{:04x}; give the serial number",
                many.len(),
                matcher.vid,
                matcher.pid
            ),
        )),
    }
}

// [@ANCHOR: radioctl_core_usb_read_trimmed]
/// The trimmed text of a sysfs attribute, or `None` if it is missing or empty.
fn read_trimmed(path: &Path) -> Option<String> {
    let text = fs::read_to_string(path).ok()?;
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_string())
}

fn read_hex16(path: &Path) -> Option<u16> {
    u16::from_str_radix(&read_trimmed(path)?, 16).ok()
}

// [@ANCHOR: radioctl_core_usb_list_sysfs]
/// Lists `ttyUSB*` and `ttyACM*` ports from a sysfs tree rooted at `sys_root` (normally
/// `/sys`), with device nodes under `dev_root` (normally `/dev`). For each port the USB device is
/// the nearest ancestor of the tty's `device` link that has `idVendor` and `idProduct`. Entries
/// that cannot be read are skipped; the result is sorted by path.
pub fn list_sysfs(sys_root: &Path, dev_root: &Path) -> Vec<PortInfo> {
    let mut ports = Vec::new();
    let Ok(entries) = fs::read_dir(sys_root.join("class/tty")) else {
        return ports;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !(name.starts_with("ttyUSB") || name.starts_with("ttyACM")) {
            continue;
        }
        let Ok(device) = fs::canonicalize(entry.path().join("device")) else {
            continue;
        };
        let cdc_acm = fs::canonicalize(device.join("driver"))
            .ok()
            .and_then(|d| d.file_name().map(|n| n == "cdc_acm"))
            .unwrap_or(false);
        let usb = device
            .ancestors()
            .find(|dir| dir.join("idVendor").is_file() && dir.join("idProduct").is_file());
        let (vid, pid, serial, product) = match usb {
            Some(dir) => (
                read_hex16(&dir.join("idVendor")),
                read_hex16(&dir.join("idProduct")),
                read_trimmed(&dir.join("serial")),
                read_trimmed(&dir.join("product")),
            ),
            None => (None, None, None, None),
        };
        ports.push(PortInfo {
            path: dev_root.join(&name),
            vid,
            pid,
            serial,
            product,
            cdc_acm,
        });
    }
    ports.sort_by(|a, b| a.path.cmp(&b.path));
    ports
}

// [@ANCHOR: radioctl_core_usb_list_ports]
/// Lists the serial ports of this machine that can be told apart by USB id.
pub fn list_ports() -> Vec<PortInfo> {
    #[cfg(target_os = "linux")]
    {
        list_sysfs(Path::new("/sys"), Path::new("/dev"))
    }
    #[cfg(not(target_os = "linux"))]
    {
        serialport::available_ports()
            .unwrap_or_default()
            .into_iter()
            .map(|info| {
                let usb = match info.port_type {
                    serialport::SerialPortType::UsbPort(usb) => Some(usb),
                    _ => None,
                };
                PortInfo {
                    path: PathBuf::from(info.port_name),
                    vid: usb.as_ref().map(|u| u.vid),
                    pid: usb.as_ref().map(|u| u.pid),
                    serial: usb.as_ref().and_then(|u| u.serial_number.clone()),
                    product: usb.as_ref().and_then(|u| u.product.clone()),
                    cdc_acm: false,
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn port(vid: u16, pid: u16, serial: Option<&str>) -> PortInfo {
        PortInfo {
            path: PathBuf::from(format!("/dev/ttyUSB{pid}")),
            vid: Some(vid),
            pid: Some(pid),
            serial: serial.map(String::from),
            product: None,
            cdc_acm: false,
        }
    }

    // Tests [@ANCHOR: radioctl_core_usb_classify]
    // Tests [@ANCHOR: radioctl_core_usb_port_chip]
    #[test]
    fn known_vendor_ids_name_their_chip() {
        assert_eq!(classify(0x0403, 0x6001, false), Chip::Ftdi);
        assert_eq!(classify(0x0403, 0x6015, false), Chip::Ftdi);
        assert_eq!(classify(0x10C4, 0xEA60, false), Chip::Cp210x);
        assert_eq!(classify(0x10C4, 0x1234, false), Chip::Unknown);
        assert_eq!(classify(0x1A86, 0x7523, false), Chip::Ch340);
        assert_eq!(classify(0x2341, 0x0043, true), Chip::CdcAcm);
        assert_eq!(classify(0x2341, 0x0043, false), Chip::Unknown);
        assert_eq!(port(0x0403, 0x6001, None).chip(), Chip::Ftdi);
        let mut plain = port(0x0403, 0x6001, None);
        plain.vid = None;
        assert_eq!(plain.chip(), Chip::Unknown);
    }

    // Tests [@ANCHOR: radioctl_core_usb_select_port]
    #[test]
    fn selection_needs_exactly_one_match() {
        let ports = vec![
            port(0x0403, 0x6001, Some("A1")),
            port(0x0403, 0x6001, Some("A2")),
            port(0x10C4, 0xEA60, None),
        ];
        let by_serial = UsbMatcher {
            vid: 0x0403,
            pid: 0x6001,
            serial: Some("A2".into()),
        };
        assert_eq!(
            select_port(&by_serial, &ports)
                .expect("one")
                .serial
                .as_deref(),
            Some("A2")
        );
        let ambiguous = UsbMatcher {
            serial: None,
            ..by_serial.clone()
        };
        let error = select_port(&ambiguous, &ports).expect_err("two match");
        assert_eq!(error.kind, ErrorKind::Unsupported);
        let missing = UsbMatcher {
            serial: Some("ZZ".into()),
            ..by_serial
        };
        let error = select_port(&missing, &ports).expect_err("none match");
        assert_eq!(error.kind, ErrorKind::LinkDown);
        assert!(error.detail.contains("0403:6001"), "{}", error.detail);
        let sole = UsbMatcher {
            vid: 0x10C4,
            pid: 0xEA60,
            serial: None,
        };
        assert!(select_port(&sole, &ports).is_ok());
    }
}
