// SPDX-License-Identifier: AGPL-3.0-or-later
//! Is a Linux kernel driver (the in-kernel `msi2500`) bound to a receiver, and is anyone using
//! it? Only `sysfs` and `/proc` are read; nothing is opened, detached or changed here.
//!
//! Detaching a bound kernel driver while another program streams from its `/dev/swradio*`
//! node removes the device from under that program, so [`inspect`] lets the opener refuse
//! (see README "Linux: the kernel msi2500 driver and opening a receiver").

use crate::{DeviceInfo, Error};
use std::path::{Path, PathBuf};

/// A process that has the kernel driver's device node open.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Holder {
    pub pid: u32,
    pub name: String,
}

/// A kernel driver bound to a receiver's interface 0.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KernelDriverUse {
    /// Driver name, e.g. `msi2500`.
    pub driver: String,
    /// `/dev` nodes created by that driver for this receiver, e.g. `/dev/swradio0`.
    pub nodes: Vec<PathBuf>,
    /// Processes that have one of those nodes open (only processes this user may inspect).
    pub holders: Vec<Holder>,
    /// False when some process could not be inspected (another user's), so `holders` may be
    /// incomplete.
    pub holders_complete: bool,
}

/// Look at the receiver `info` (bus number and device address from libusb). `Ok(None)` when no
/// kernel driver is bound to it or the platform has no sysfs.
pub fn inspect(info: &DeviceInfo) -> Result<Option<KernelDriverUse>, Error> {
    inspect_in(
        Path::new("/sys"),
        Path::new("/dev"),
        Path::new("/proc"),
        info.bus,
        info.address,
    )
}

fn read_trim(p: &Path) -> Option<String> {
    std::fs::read_to_string(p)
        .ok()
        .map(|s| s.trim().to_string())
}

/// As [`inspect`] with the roots of `sys`, `dev` and `proc` given (for tests).
pub(crate) fn inspect_in(
    sys: &Path,
    dev: &Path,
    proc_: &Path,
    bus: u8,
    address: u8,
) -> Result<Option<KernelDriverUse>, Error> {
    // [@ANCHOR: rsp_kernel_inspect_in]
    let devices = sys.join("bus/usb/devices");
    let Ok(rd) = std::fs::read_dir(&devices) else {
        return Ok(None);
    };
    // The USB device directory whose busnum/devnum match (names without ':').
    let mut found: Option<PathBuf> = None;
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        if name.contains(':') {
            continue;
        }
        let p = e.path();
        let b: Option<u32> = read_trim(&p.join("busnum")).and_then(|s| s.parse().ok());
        let a: Option<u32> = read_trim(&p.join("devnum")).and_then(|s| s.parse().ok());
        if b == Some(bus as u32) && a == Some(address as u32) {
            found = Some(p);
            break;
        }
    }
    let Some(dir) = found else {
        return Ok(None);
    };
    let dirname = dir.file_name().unwrap().to_string_lossy().to_string();
    let iface = devices.join(format!("{dirname}:1.0"));
    let Ok(drv) = std::fs::read_link(iface.join("driver")) else {
        return Ok(None);
    };
    let Some(driver) = drv.file_name().map(|s| s.to_string_lossy().to_string()) else {
        return Ok(None);
    };
    // Device nodes of that interface: /sys/class/video4linux/<node>/device -> the interface.
    let iface_real = std::fs::canonicalize(&iface).unwrap_or(iface.clone());
    let mut nodes = Vec::new();
    if let Ok(rd) = std::fs::read_dir(sys.join("class/video4linux")) {
        for e in rd.flatten() {
            let dev_link = e.path().join("device");
            if std::fs::canonicalize(&dev_link).ok().as_deref() == Some(iface_real.as_path()) {
                nodes.push(dev.join(e.file_name()));
            }
        }
    }
    nodes.sort();
    let (holders, holders_complete) = holders_of(proc_, &nodes);
    Ok(Some(KernelDriverUse {
        driver,
        nodes,
        holders,
        holders_complete,
    }))
}

fn holders_of(proc_: &Path, nodes: &[PathBuf]) -> (Vec<Holder>, bool) {
    // [@ANCHOR: rsp_kernel_holders_of]
    let mut holders = Vec::new();
    let mut complete = true;
    if nodes.is_empty() {
        return (holders, complete);
    }
    let Ok(rd) = std::fs::read_dir(proc_) else {
        return (holders, false);
    };
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        let Ok(pid) = name.parse::<u32>() else {
            continue;
        };
        let fds = match std::fs::read_dir(e.path().join("fd")) {
            Ok(f) => f,
            Err(err) => {
                // A process of another user (or one that just exited).
                if err.kind() == std::io::ErrorKind::PermissionDenied {
                    complete = false;
                }
                continue;
            }
        };
        let uses = fds
            .flatten()
            .filter_map(|f| std::fs::read_link(f.path()).ok())
            .any(|t| nodes.contains(&t));
        if uses {
            let comm = read_trim(&e.path().join("comm")).unwrap_or_default();
            holders.push(Holder { pid, name: comm });
        }
    }
    holders.sort_by_key(|h| h.pid);
    (holders, complete)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    struct Tmp(PathBuf);
    impl Tmp {
        fn new(tag: &str) -> Tmp {
            let p =
                std::env::temp_dir().join(format!("rsp_open_kernel_{}_{tag}", std::process::id()));
            let _ = std::fs::remove_dir_all(&p);
            std::fs::create_dir_all(&p).unwrap();
            Tmp(p)
        }
    }
    impl Drop for Tmp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// A fake sysfs/dev/proc: USB device 3-1 (bus 3, address 7) with interface 3-1:1.0, optionally
    /// bound to msi2500 with the V4L node swradio0, and optionally a process with it open.
    fn fake(t: &Tmp, bound: bool, open_pid: Option<(u32, &str)>) {
        let root = &t.0;
        let devs = root.join("sys/bus/usb/devices");
        std::fs::create_dir_all(devs.join("3-1/3-1:1.0")).unwrap();
        std::fs::write(devs.join("3-1/busnum"), "3\n").unwrap();
        std::fs::write(devs.join("3-1/devnum"), "7\n").unwrap();
        // The interface is also reachable as 3-1:1.0 directly under devices/.
        symlink(devs.join("3-1/3-1:1.0"), devs.join("3-1:1.0")).unwrap();
        std::fs::create_dir_all(root.join("sys/bus/usb/drivers/msi2500")).unwrap();
        if bound {
            symlink(
                root.join("sys/bus/usb/drivers/msi2500"),
                devs.join("3-1/3-1:1.0/driver"),
            )
            .unwrap();
            std::fs::create_dir_all(root.join("sys/class/video4linux/swradio0")).unwrap();
            symlink(
                devs.join("3-1/3-1:1.0"),
                root.join("sys/class/video4linux/swradio0/device"),
            )
            .unwrap();
        }
        std::fs::create_dir_all(root.join("dev")).unwrap();
        std::fs::create_dir_all(root.join("proc/1/fd")).unwrap();
        if let Some((pid, comm)) = open_pid {
            let fd = root.join(format!("proc/{pid}/fd"));
            std::fs::create_dir_all(&fd).unwrap();
            std::fs::write(root.join(format!("proc/{pid}/comm")), format!("{comm}\n")).unwrap();
            symlink(root.join("dev/swradio0"), fd.join("5")).unwrap();
        }
    }

    fn run(t: &Tmp, bus: u8, addr: u8) -> Option<KernelDriverUse> {
        inspect_in(
            &t.0.join("sys"),
            &t.0.join("dev"),
            &t.0.join("proc"),
            bus,
            addr,
        )
        .unwrap()
    }

    #[test]
    fn no_driver_bound_gives_none() {
        let t = Tmp::new("none");
        fake(&t, false, None);
        assert_eq!(run(&t, 3, 7), None);
    }

    #[test]
    fn other_device_gives_none() {
        let t = Tmp::new("other");
        fake(&t, true, None);
        assert_eq!(run(&t, 3, 8), None);
    }

    #[test]
    fn bound_driver_with_idle_node() {
        let t = Tmp::new("idle");
        fake(&t, true, None);
        let u = run(&t, 3, 7).unwrap();
        assert_eq!(u.driver, "msi2500");
        assert_eq!(u.nodes, vec![t.0.join("dev/swradio0")]);
        assert!(u.holders.is_empty());
    }

    #[test]
    // Tests [@ANCHOR: rsp_kernel_holders_of]
    // Tests [@ANCHOR: rsp_kernel_inspect_in]
    fn bound_driver_with_a_process_using_the_node() {
        let t = Tmp::new("busy");
        fake(&t, true, Some((4242, "gqrx")));
        let u = run(&t, 3, 7).unwrap();
        assert_eq!(
            u.holders,
            vec![Holder {
                pid: 4242,
                name: "gqrx".into()
            }]
        );
    }
}
