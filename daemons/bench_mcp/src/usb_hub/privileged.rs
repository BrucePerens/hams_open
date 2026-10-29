// SPDX-License-Identifier: AGPL-3.0-or-later
//! The root helper behind the middle tier. The server runs `sudo -n <this binary> --usb-priv <operation> <arguments>`; this module is what
//! runs as root. It accepts only the operations below, checks every argument against the real sysfs before using it, and does nothing else:
//!
//! * `reset <device>`: USBDEVFS_RESET on the device's node (a port reset; the device keeps its number and its power)
//! * `authorize <device> <0|1>`: write the device's `authorized` file (0 is a software disconnect without losing power)
//! * `unbind <interface> <driver>` and `bind <interface> <driver>`: write the driver's `unbind` or `bind` file
//! * `autosuspend <device> <on|auto> <delay_ms|->`: write `power/control` and `power/autosuspend_delay_ms`
//!
//! A sudoers rule for this binary lets the user run exactly these operations as root, and nothing else; the binary must be owned by root
//! and not writable by the user, or the rule is a way to run anything.

use super::sysfs::is_device_name;
use std::path::Path;

pub const FLAG: &str = "--usb-priv";

fn device_dir(sysfs: &Path, dev: &str) -> Result<std::path::PathBuf, String> {
    // [@ANCHOR: bench_mcp_usb_hub_privileged_device_dir]
    if !is_device_name(dev) || dev.starts_with("usb") {
        return Err(format!(
            "{dev:?} is not the path of a USB device behind a hub port (root hubs are not allowed)"
        ));
    }
    let link = sysfs.join("bus/usb/devices").join(dev);
    let real = std::fs::canonicalize(&link).map_err(|e| format!("{dev}: {e}"))?;
    if !real.join("idVendor").is_file() {
        return Err(format!("{dev} is not a USB device"));
    }
    Ok(real)
}

fn interface_name(i: &str) -> Result<(), String> {
    // [@ANCHOR: bench_mcp_usb_hub_privileged_interface_name]
    let ok = i.split_once(':').is_some_and(|(d, rest)| {
        is_device_name(d)
            && !rest.is_empty()
            && rest.bytes().all(|b| b.is_ascii_digit() || b == b'.')
    });
    if ok {
        Ok(())
    } else {
        Err(format!(
            "{i:?} is not a USB interface name like 3-2.2.2:1.3"
        ))
    }
}

fn driver_dir(sysfs: &Path, driver: &str) -> Result<std::path::PathBuf, String> {
    // [@ANCHOR: bench_mcp_usb_hub_privileged_driver_dir]
    let ok = !driver.is_empty()
        && driver
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-' || b == b'.');
    if !ok {
        return Err(format!("{driver:?} is not a driver name"));
    }
    let d = sysfs.join("bus/usb/drivers").join(driver);
    if d.is_dir() {
        Ok(d)
    } else {
        Err(format!("no USB driver named {driver:?}"))
    }
}

fn write(path: &Path, text: &str) -> Result<(), String> {
    std::fs::write(path, text).map_err(|e| format!("write {}: {e}", path.display()))
}

fn read_num(dir: &Path, f: &str) -> Result<u32, String> {
    std::fs::read_to_string(dir.join(f))
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .ok_or_else(|| format!("cannot read {f} of {}", dir.display()))
}

const USBDEVFS_RESET: libc::c_ulong = 0x5514;

fn reset(sysfs: &Path, dev: &str) -> Result<String, String> {
    // [@ANCHOR: bench_mcp_usb_hub_privileged_reset]
    let dir = device_dir(sysfs, dev)?;
    let (bus, num) = (read_num(&dir, "busnum")?, read_num(&dir, "devnum")?);
    let node = format!("/dev/bus/usb/{bus:03}/{num:03}");
    let f = std::fs::OpenOptions::new()
        .write(true)
        .open(&node)
        .map_err(|e| format!("open {node}: {e}"))?;
    use std::os::fd::AsRawFd;
    // SAFETY: USBDEVFS_RESET takes no argument; the descriptor is open for the length of the call.
    let r = unsafe { libc::ioctl(f.as_raw_fd(), USBDEVFS_RESET, 0) };
    if r < 0 {
        return Err(format!(
            "USBDEVFS_RESET on {node}: {}",
            std::io::Error::last_os_error()
        ));
    }
    Ok(format!("reset {dev} ({node})"))
}

/// Runs one operation against the sysfs at `sysfs`; the arguments are those after `--usb-priv`.
pub fn run_at(sysfs: &Path, args: &[String]) -> Result<String, String> {
    // [@ANCHOR: usb_hub_privileged_ops]
    let a: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    match a.as_slice() {
        ["reset", dev] => reset(sysfs, dev),
        ["authorize", dev, v @ ("0" | "1")] => {
            let dir = device_dir(sysfs, dev)?;
            write(&dir.join("authorized"), v)?;
            Ok(format!("{dev} authorized={v}"))
        }
        [op @ ("unbind" | "bind"), iface, driver] => {
            interface_name(iface)?;
            let dir = driver_dir(sysfs, driver)?;
            write(&dir.join(op), iface)?;
            Ok(format!("{op} {iface} {driver}"))
        }
        ["autosuspend", dev, control @ ("on" | "auto"), delay] => {
            let dir = device_dir(sysfs, dev)?;
            if *delay != "-" {
                let ms: i64 = delay
                    .parse()
                    .map_err(|_| format!("{delay:?} is not a delay in ms"))?;
                if !(-1..=3_600_000).contains(&ms) {
                    return Err("delay_ms must be -1 to 3600000".into());
                }
                write(&dir.join("power/autosuspend_delay_ms"), &ms.to_string())?;
            }
            write(&dir.join("power/control"), control)?;
            Ok(format!("{dev} power/control={control} delay={delay}"))
        }
        _ => Err(format!("not an allowed operation: {}", a.join(" "))),
    }
}

/// The entry point for `--usb-priv`: prints the outcome and returns the exit code.
pub fn main(args: &[String]) -> i32 {
    // [@ANCHOR: bench_mcp_usb_hub_privileged_main]
    // SAFETY: geteuid has no preconditions.
    if unsafe { libc::geteuid() } != 0 {
        eprintln!("{FLAG} must run as root (through sudo -n)");
        return 2;
    }
    match run_at(Path::new("/sys"), args) {
        Ok(s) => {
            println!("{s}");
            0
        }
        Err(e) => {
            eprintln!("{e}");
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::usb_hub::testkit::{writable_sys, Dir};

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    // Tests [@ANCHOR: usb_hub_privileged_ops]
    // Tests [@ANCHOR: bench_mcp_usb_hub_privileged_device_dir]
    // Tests [@ANCHOR: bench_mcp_usb_hub_privileged_driver_dir]
    // Tests [@ANCHOR: bench_mcp_usb_hub_privileged_interface_name]
    // Tests [@ANCHOR: bench_mcp_usb_hub_privileged_reset]
    #[test]
    fn authorize_and_autosuspend_write_only_their_files_after_checking_the_device() {
        let (_d, sys) = writable_sys();
        run_at(&sys, &s(&["authorize", "3-2.2.1", "0"])).unwrap();
        let dev = std::fs::canonicalize(sys.join("bus/usb/devices/3-2.2.1")).unwrap();
        assert_eq!(
            std::fs::read_to_string(dev.join("authorized")).unwrap(),
            "0"
        );
        run_at(&sys, &s(&["autosuspend", "3-2.2.1", "auto", "2000"])).unwrap();
        assert_eq!(
            std::fs::read_to_string(dev.join("power/control")).unwrap(),
            "auto"
        );
        assert_eq!(
            std::fs::read_to_string(dev.join("power/autosuspend_delay_ms")).unwrap(),
            "2000"
        );
        run_at(&sys, &s(&["autosuspend", "3-2.2.1", "on", "-"])).unwrap();
        assert_eq!(
            std::fs::read_to_string(dev.join("power/control")).unwrap(),
            "on"
        );
    }

    #[test]
    fn bind_and_unbind_write_the_interface_to_the_drivers_file() {
        let (d, sys) = writable_sys();
        std::fs::create_dir_all(sys.join("bus/usb/drivers/snd-usb-audio")).unwrap();
        run_at(&sys, &s(&["unbind", "3-2.2.2:1.0", "snd-usb-audio"])).unwrap();
        assert_eq!(
            std::fs::read_to_string(sys.join("bus/usb/drivers/snd-usb-audio/unbind")).unwrap(),
            "3-2.2.2:1.0"
        );
        run_at(&sys, &s(&["bind", "3-2.2.2:1.0", "snd-usb-audio"])).unwrap();
        assert!(sys.join("bus/usb/drivers/snd-usb-audio/bind").is_file());
        drop(d);
    }

    #[test]
    fn anything_outside_the_allowlist_is_refused_before_a_write() {
        let (_d, sys) = writable_sys();
        let bad: &[&[&str]] = &[
            &["authorize", "../../etc", "1"],
            &["authorize", "3-2.2.1", "2"],
            &["authorize", "9-9", "1"],
            &["unbind", "3-2.2.2:1.0", "../x"],
            &["unbind", "../../x:1.0", "snd-usb-audio"],
            &["unbind", "3-2.2.2:1.0", "no-such-driver"],
            &["autosuspend", "3-2.2.1", "auto", "99999999"],
            &["autosuspend", "3-2.2.1", "suspend", "-"],
            &["rm", "-rf", "/"],
            &["reset"],
            &["reset", "3-2.2.1/../.."],
        ];
        for b in bad {
            assert!(run_at(&sys, &s(b)).is_err(), "{b:?} must be refused");
        }
        let scratch = Dir::new();
        assert!(run_at(scratch.path(), &s(&["authorize", "3-2", "1"])).is_err());
    }

    // Tests [@ANCHOR: bench_mcp_usb_hub_privileged_main]
    #[test]
    fn the_entry_point_refuses_a_non_root_caller_and_an_operation_outside_the_allowlist() {
        // An operation outside the allowlist writes nothing in either case: a non-root caller is stopped first (exit 2), and root
        // reaches the allowlist check, which refuses it (exit 1). The accepted operations are tested through `run_at` on a copy
        // of the sysfs; `main` itself always acts on the real /sys, so its success path is not run here.
        // SAFETY: geteuid has no preconditions.
        let root = unsafe { libc::geteuid() } == 0;
        let code = main(&s(&["rm", "-rf", "/"]));
        assert_eq!(code, if root { 1 } else { 2 });
        assert_eq!(main(&[]), if root { 1 } else { 2 });
    }
}
