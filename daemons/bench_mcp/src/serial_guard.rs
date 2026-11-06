// SPDX-License-Identifier: AGPL-3.0-or-later
//! Which serial devices a bench tool may open. A supply (ATM6301) and a NanoVNA are both CH340/CDC adapters, and `/dev/ttyUSBn` numbers follow
//! plug-in order, so a bare `ttyUSB0` can silently become the SainSmart relay board or another instrument. Two rules:
//! * `reserved_serial` (config, top level): devices no bench tool opens. An entry is a device path (`/dev/serial/by-id/...`, compared after
//!   resolving symlinks) or `usb:<bus-port>` (for example `usb:1-4.2`, the device's place in the USB tree, found through sysfs).
//! * a supply is opened by a bare `/dev/ttyUSBn` or `/dev/ttyACMn` only when no `/dev/serial/by-id` name points at that device.
//!
//! The checks only look at the file system (they never open the device).

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// The two directories the checks read, so tests can use a fake tree.
pub struct Roots<'a> {
    pub by_id: &'a Path,
    pub sys: &'a Path,
}

/// The real `/dev/serial/by-id` and `/sys`.
pub fn real_roots() -> Roots<'static> {
    Roots {
        by_id: Path::new("/dev/serial/by-id"),
        sys: Path::new("/sys"),
    }
}

static RESERVED: OnceLock<Vec<String>> = OnceLock::new();

/// Reads the config's `reserved_serial` list: strings that are an absolute `/dev/` path or `usb:<bus-port>`; anything else is an error.
pub fn parse_reserved(top: &serde_json::Value) -> Result<Vec<String>, String> {
    // [@ANCHOR: bench_mcp_serial_guard_parse_reserved]
    let Some(v) = top.get("reserved_serial") else {
        return Ok(Vec::new());
    };
    let list = v
        .as_array()
        .ok_or("config: reserved_serial must be a list of strings")?;
    let mut out = Vec::new();
    for e in list {
        let s = e
            .as_str()
            .ok_or("config: reserved_serial must be a list of strings")?;
        let usb_ok = s.strip_prefix("usb:").is_some_and(|p| {
            !p.is_empty()
                && p.chars()
                    .all(|c| c.is_ascii_digit() || c == '-' || c == '.')
        });
        if !(usb_ok || (s.starts_with("/dev/") && !s.contains(".."))) {
            return Err(format!(
                "config: reserved_serial entry {s:?} must be a /dev/ path or usb:<bus-port> such as usb:1-4.2"
            ));
        }
        out.push(s.to_string());
    }
    Ok(out)
}

/// Remembers the reserved list for the whole process (`config::load` calls it once, before any device is opened).
pub fn set_reserved(list: Vec<String>) {
    // [@ANCHOR: bench_mcp_serial_guard_set_reserved]
    RESERVED.set(list).ok();
}

fn real(p: &Path) -> PathBuf {
    std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf())
}

/// The USB bus-port of a tty (`1-4.2` for `.../1-4.2/1-4.2:1.0/ttyUSB0`), from its sysfs entry.
fn usb_port_of(dev: &Path, sys: &Path) -> Option<String> {
    // [@ANCHOR: bench_mcp_serial_guard_usb_port_of]
    let name = dev.file_name()?;
    let link = std::fs::canonicalize(sys.join("class/tty").join(name).join("device")).ok()?;
    link.components()
        .filter_map(|c| c.as_os_str().to_str())
        .filter(|c| c.contains('-') && c.chars().next().is_some_and(|f| f.is_ascii_digit()))
        .filter_map(|c| c.split(':').next())
        .next_back()
        .map(String::from)
}

/// The reason `path` is reserved, or None. A device that cannot be resolved is compared by its literal name.
pub fn reserved_reason(path: &str, reserved: &[String], r: &Roots) -> Option<String> {
    // [@ANCHOR: bench_mcp_serial_guard_reserved_reason]
    let dev = real(Path::new(path));
    let port = usb_port_of(&dev, r.sys);
    reserved
        .iter()
        .find(|e| match e.strip_prefix("usb:") {
            Some(p) => port.as_deref() == Some(p),
            None => real(Path::new(e.as_str())) == dev,
        })
        .map(|e| format!("{path} is reserved by the config's reserved_serial entry {e:?} (another device, not an instrument bench tools may open)"))
}

/// The `/dev/serial/by-id` name that points at `path`, if there is one.
pub fn by_id_name(path: &str, r: &Roots) -> Option<PathBuf> {
    // [@ANCHOR: bench_mcp_serial_guard_by_id_name]
    let dev = real(Path::new(path));
    let mut names: Vec<PathBuf> = std::fs::read_dir(r.by_id)
        .ok()?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| real(p) == dev)
        .collect();
    names.sort();
    names.into_iter().next()
}

/// Whether a supply may be opened at `path`: refused when reserved, and refused when it is a bare `ttyUSBn` or `ttyACMn` that has a by-id name
/// (the error gives the by-id path to use).
pub fn check_supply(path: &str, reserved: &[String], r: &Roots) -> Result<(), String> {
    // [@ANCHOR: bench_mcp_serial_guard_check_supply]
    if let Some(why) = reserved_reason(path, reserved, r) {
        return Err(why);
    }
    let p = Path::new(path);
    let bare = !p.starts_with(r.by_id)
        && p.file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.starts_with("ttyUSB") || n.starts_with("ttyACM"));
    if bare {
        if let Some(id) = by_id_name(path, r) {
            return Err(format!(
                "{path} is a bare tty name, and tty numbers follow plug-in order (they can become another device, such as a relay board); use the stable path {} in the config",
                id.display()
            ));
        }
    }
    Ok(())
}

/// `check_supply` against the process's reserved list and the real file system.
pub fn check_supply_open(path: &str) -> Result<(), String> {
    check_supply(path, RESERVED.get().map_or(&[], |v| v), &real_roots())
}

/// The reserved check alone (any serial device), against the process's list and the real file system.
pub fn check_reserved_open(path: &str) -> Result<(), String> {
    // [@ANCHOR: bench_mcp_serial_guard_check_reserved_open]
    match reserved_reason(path, RESERVED.get().map_or(&[], |v| v), &real_roots()) {
        Some(why) => Err(why),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::os::unix::fs::symlink;

    /// A fake /dev and /sys: `ttyUSB0` on usb 1-4.2 and `ttyUSB1` on usb 1-4.3, by-id names for ttyUSB0 only.
    fn tree(tag: &str) -> (PathBuf, PathBuf, PathBuf, PathBuf) {
        let root = std::env::temp_dir().join(format!("bench_guard_{tag}_{}", std::process::id()));
        std::fs::remove_dir_all(&root).ok();
        let dev = root.join("dev");
        let by_id = dev.join("serial/by-id");
        std::fs::create_dir_all(&by_id).unwrap();
        for (n, port) in [("ttyUSB0", "1-4.2"), ("ttyUSB1", "1-4.3")] {
            std::fs::write(dev.join(n), b"").unwrap();
            let iface = root.join(format!("sys/devices/usb1/{port}/{port}:1.0"));
            std::fs::create_dir_all(iface.join("tty").join(n)).unwrap();
            std::fs::create_dir_all(root.join("sys/class/tty").join(n)).unwrap();
            symlink(&iface, root.join("sys/class/tty").join(n).join("device")).unwrap();
        }
        symlink(
            dev.join("ttyUSB0"),
            by_id.join("usb-FAKE_supply-if00-port0"),
        )
        .unwrap();
        let sys = root.join("sys");
        (root, dev, by_id, sys)
    }

    // Tests [@ANCHOR: bench_mcp_serial_guard_parse_reserved]
    #[test]
    fn the_reserved_list_takes_dev_paths_and_usb_ports_and_refuses_anything_else() {
        assert_eq!(parse_reserved(&json!({})).unwrap(), Vec::<String>::new());
        let ok =
            parse_reserved(&json!({"reserved_serial": ["/dev/serial/by-id/usb-x", "usb:1-4.2"]}));
        assert_eq!(ok.unwrap().len(), 2);
        for bad in [
            json!("x"),
            json!([1]),
            json!(["ttyUSB0"]),
            json!(["/dev/../etc"]),
            json!(["usb:"]),
            json!(["usb:a b"]),
        ] {
            assert!(
                parse_reserved(&json!({"reserved_serial": bad})).is_err(),
                "{bad}"
            );
        }
    }

    // Tests [@ANCHOR: bench_mcp_serial_guard_reserved_reason]
    // Tests [@ANCHOR: bench_mcp_serial_guard_usb_port_of]
    // Tests [@ANCHOR: bench_mcp_serial_guard_by_id_name]
    #[test]
    fn a_reserved_usb_port_or_by_id_path_is_found_whatever_name_the_device_has() {
        let (root, dev, by_id, sys) = tree("reserved");
        let r = Roots {
            by_id: &by_id,
            sys: &sys,
        };
        let tty0 = dev.join("ttyUSB0").display().to_string();
        let tty1 = dev.join("ttyUSB1").display().to_string();
        let by_usb = vec!["usb:1-4.2".to_string()];
        assert!(reserved_reason(&tty0, &by_usb, &r)
            .unwrap()
            .contains("usb:1-4.2"));
        assert!(reserved_reason(&tty1, &by_usb, &r).is_none());
        // Through the by-id symlink the same device is found.
        let link = by_id
            .join("usb-FAKE_supply-if00-port0")
            .display()
            .to_string();
        assert!(reserved_reason(&link, &by_usb, &r).is_some());
        let by_path = vec![link.clone()];
        assert!(reserved_reason(&tty0, &by_path, &r).is_some());
        assert!(reserved_reason(&tty1, &by_path, &r).is_none());
        assert!(reserved_reason(&tty0, &[], &r).is_none());
        assert_eq!(
            by_id_name(&tty0, &r).unwrap(),
            by_id.join("usb-FAKE_supply-if00-port0")
        );
        assert!(by_id_name(&tty1, &r).is_none());
        std::fs::remove_dir_all(root).ok();
    }

    // Tests [@ANCHOR: bench_mcp_serial_guard_check_supply]
    #[test]
    fn a_supply_is_refused_on_a_reserved_device_and_on_a_bare_tty_that_has_a_by_id_name() {
        let (root, dev, by_id, sys) = tree("supply");
        let r = Roots {
            by_id: &by_id,
            sys: &sys,
        };
        let tty0 = dev.join("ttyUSB0").display().to_string();
        let tty1 = dev.join("ttyUSB1").display().to_string();
        let link = by_id
            .join("usb-FAKE_supply-if00-port0")
            .display()
            .to_string();
        let e = check_supply(&tty0, &[], &r).unwrap_err();
        assert!(
            e.contains("usb-FAKE_supply-if00-port0") && e.contains("bare"),
            "{e}"
        );
        assert!(check_supply(&link, &[], &r).is_ok());
        // No by-id name: nothing better to point at, so the bare name is allowed.
        assert!(check_supply(&tty1, &[], &r).is_ok());
        // Reserved wins, even through the stable name.
        let res = vec!["usb:1-4.3".to_string()];
        assert!(check_supply(&tty1, &res, &r)
            .unwrap_err()
            .contains("reserved"));
        let res = vec!["usb:1-4.2".to_string()];
        assert!(check_supply(&link, &res, &r)
            .unwrap_err()
            .contains("reserved"));
        // A missing by-id directory (no adapter attached) is not an error here.
        let none = Roots {
            by_id: &root.join("nope"),
            sys: &sys,
        };
        assert!(check_supply(&tty0, &[], &none).is_ok());
        std::fs::remove_dir_all(root).ok();
    }
}
