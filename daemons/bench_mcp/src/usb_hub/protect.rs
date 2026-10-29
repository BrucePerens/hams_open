// SPDX-License-Identifier: AGPL-3.0-or-later
//! The protection list: devices and hubs a tool must not power off, reset, unbind or deauthorize by default.
//!
//! A device is critical when it carries the root filesystem or another mounted filesystem, is a boot-protocol keyboard or
//! mouse, is named by the config (`critical` ports and `critical_devices`), or is the serial adapter of a bench instrument.
//! Switching or resetting a port is refused when the device on it, or anything below it, is critical, and when the device on
//! it is a hub (its uplink: everything below it goes too) unless the port is marked `hub_uplink_ok` in the config.
//! Reasons from the config and from bench instruments cannot be overridden by a tool argument; the rest can, with
//! `override_protection: true`.

use super::sysfs::{child_path, Tree};
use super::Config;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

#[derive(Clone, Debug, PartialEq)]
pub struct Why {
    pub text: String,
    pub overridable: bool,
}

/// Device path to the reasons it is critical.
pub type Critical = BTreeMap<String, Vec<Why>>;

/// What a bench instrument uses on the USB tree: a serial device path (a power supply), the hub port its unit hangs from and its
/// vid:pid (the VNA).
pub struct BenchRef {
    pub instrument: String,
    pub path: Option<String>,
    pub hub_port: Option<(String, u32)>,
    pub vidpid: Option<String>,
}

fn add(c: &mut Critical, dev: &str, text: String, overridable: bool) {
    // [@ANCHOR: bench_mcp_usb_hub_protect_add]
    let list = c.entry(dev.to_string()).or_default();
    if !list.iter().any(|w| w.text == text) {
        list.push(Why { text, overridable });
    }
}

/// The names a block device is known by to the USB tree: itself, its whole disk and, for device-mapper or md devices, the
/// devices underneath it.
fn block_names(sysfs: &Path, name: &str, out: &mut BTreeSet<String>, depth: u32) {
    // [@ANCHOR: bench_mcp_usb_hub_protect_block_names]
    if depth > 6 || !out.insert(name.to_string()) {
        return;
    }
    let base = name.trim_end_matches(|c: char| c.is_ascii_digit());
    let base = base
        .strip_suffix('p')
        .filter(|b| b.chars().last().is_some_and(|c| c.is_ascii_digit()))
        .unwrap_or(base);
    if base != name && !base.is_empty() {
        block_names(sysfs, base, out, depth + 1);
    }
    let slaves = sysfs.join("class/block").join(name).join("slaves");
    if let Ok(rd) = std::fs::read_dir(slaves) {
        for e in rd.flatten() {
            block_names(sysfs, &e.file_name().to_string_lossy(), out, depth + 1);
        }
    }
}

fn mount_sources(proc_root: &Path) -> Vec<(String, String)> {
    let text = std::fs::read_to_string(proc_root.join("mounts")).unwrap_or_default();
    text.lines()
        .filter_map(|l| {
            let mut f = l.split_whitespace();
            let (src, mp) = (f.next()?, f.next()?);
            src.starts_with("/dev/")
                .then(|| (src.to_string(), mp.replace("\\040", " ")))
        })
        .collect()
}

pub fn critical_set(
    t: &Tree,
    cfg: &Config,
    bench: &[BenchRef],
    sysfs: &Path,
    proc_root: &Path,
) -> Critical {
    // [@ANCHOR: usb_hub_protection]
    let mut c = Critical::new();
    let node_owner = |name: &str| -> Option<String> {
        t.devices
            .values()
            .find(|d| {
                d.interfaces
                    .iter()
                    .flat_map(|i| &i.nodes)
                    .any(|n| n.name == name)
            })
            .map(|d| d.path.clone())
    };
    for (src, mp) in mount_sources(proc_root) {
        let dev = std::fs::canonicalize(&src)
            .ok()
            .and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_string()));
        let dev = dev.unwrap_or_else(|| src.trim_start_matches("/dev/").to_string());
        let mut names = BTreeSet::new();
        block_names(sysfs, &dev, &mut names, 0);
        for n in &names {
            let Some(owner) = node_owner(n) else { continue };
            let root = mp == "/";
            let text = if root {
                format!("carries the root filesystem ({src})")
            } else {
                format!("has {src} mounted at {mp}")
            };
            add(&mut c, &owner, text, !root);
        }
    }
    for d in t.devices.values() {
        for i in d
            .interfaces
            .iter()
            .filter(|i| i.class == 3 && i.subclass == 1)
        {
            let kind = match i.protocol {
                1 => "keyboard",
                2 => "mouse",
                _ => continue,
            };
            add(
                &mut c,
                &d.path,
                format!("is a USB {kind} (boot protocol)"),
                true,
            );
        }
        if cfg.critical_devices.contains(&d.vidpid()) {
            add(
                &mut c,
                &d.path,
                format!("{} is listed in critical_devices", d.vidpid()),
                false,
            );
        }
        if let Some((hub, port)) = d.parent.as_ref().zip(d.port) {
            let key = format!("{hub}/{port}");
            if let Some(p) = cfg.ports.get(&key).filter(|p| p.critical) {
                let name = p.label.clone().unwrap_or(key);
                add(
                    &mut c,
                    &d.path,
                    format!("is on the critical port {name}"),
                    false,
                );
            }
        }
    }
    for b in bench {
        if let Some(path) = &b.path {
            let real = std::fs::canonicalize(path)
                .ok()
                .and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_string()));
            let name = real.unwrap_or_else(|| path.trim_start_matches("/dev/").to_string());
            if let Some(owner) = node_owner(&name) {
                let text = format!(
                    "is the serial adapter of bench instrument {:?}",
                    b.instrument
                );
                add(&mut c, &owner, text, false);
            }
        }
        // The VNA is measured through its hub port: a reset or power cycle by a hub tool would spoil a measurement, so it is protected
        // unless the caller says override_protection (its own power cycle tool does not go through this list).
        if let Some((hub, port)) = &b.hub_port {
            for d in t.subtree(&child_path(hub, *port)) {
                let text = format!(
                    "is on the hub port {hub}/{port} of bench instrument {:?}",
                    b.instrument
                );
                add(&mut c, &d.path, text, true);
            }
        }
        if let Some(v) = &b.vidpid {
            for d in t.devices.values().filter(|d| d.vidpid() == *v) {
                let text = format!("is the unit of bench instrument {:?}", b.instrument);
                add(&mut c, &d.path, text, true);
            }
        }
    }
    c
}

#[derive(Clone, Debug, PartialEq)]
pub struct Verdict {
    pub reasons: Vec<Why>,
}

impl Verdict {
    pub fn clear(&self) -> bool {
        self.reasons.is_empty()
    }
    pub fn overridable(&self) -> bool {
        self.reasons.iter().all(|r| r.overridable)
    }
    /// Ok to go ahead, given the caller's `override_protection` argument; the error says what blocks and whether it can be overridden.
    pub fn permit(&self, what: &str, override_protection: bool) -> Result<(), String> {
        // [@ANCHOR: bench_mcp_usb_hub_protect_verdict_permit]
        if self.clear() {
            return Ok(());
        }
        let list = self
            .reasons
            .iter()
            .map(|r| r.text.as_str())
            .collect::<Vec<_>>()
            .join("; ");
        if !self.overridable() {
            return Err(format!("refused: {what} {list}. This protection comes from the config or a bench instrument and no argument overrides it"));
        }
        if override_protection {
            return Ok(());
        }
        Err(format!(
            "refused: {what} {list}. Add override_protection: true if this is really intended"
        ))
    }
    pub fn json(&self) -> Value {
        json!({"protected": !self.clear(), "overridable": self.overridable(), "reasons": self.reasons.iter().map(|r| r.text.clone()).collect::<Vec<_>>()})
    }
}

/// Whether the config marks the hub's port as `hub_uplink_ok`.
fn uplink_ok(cfg: &Config, device: &str, t: &Tree) -> bool {
    t.get(device)
        .and_then(|d| d.parent.as_ref().zip(d.port))
        .is_some_and(|(h, p)| {
            cfg.ports
                .get(&format!("{h}/{p}"))
                .is_some_and(|c| c.hub_uplink_ok)
        })
}

/// The verdict for acting on `device` and everything below it (a power-off or cycle of its port, a reset, a deauthorize).
pub fn check_device(t: &Tree, cfg: &Config, crit: &Critical, device: &str) -> Verdict {
    // [@ANCHOR: bench_mcp_usb_hub_protect_check_device]
    let mut reasons = Vec::new();
    for d in t.subtree(device) {
        for w in crit.get(&d.path).into_iter().flatten() {
            let who = if d.path == device {
                format!("the device {} ", d.path)
            } else {
                format!("{} (below {device}) ", d.path)
            };
            reasons.push(Why {
                text: format!("{who}{}", w.text),
                overridable: w.overridable,
            });
        }
    }
    if let Some(d) = t.get(device) {
        let below = t.subtree(device).len() - 1;
        if d.is_hub() && !uplink_ok(cfg, device, t) {
            let text = format!("{device} is a hub and this is its uplink: acting on it takes down the {below} device(s) below it");
            reasons.push(Why {
                text,
                overridable: true,
            });
        }
    }
    Verdict { reasons }
}

/// The verdict for a port: its own config flag, then whatever is plugged into it.
pub fn check_port(t: &Tree, cfg: &Config, crit: &Critical, hub: &str, port: u32) -> Verdict {
    // [@ANCHOR: bench_mcp_usb_hub_protect_check_port]
    let mut v = match t.get(&child_path(hub, port)) {
        Some(d) => check_device(t, cfg, crit, &d.path),
        None => Verdict { reasons: vec![] },
    };
    let key = format!("{hub}/{port}");
    if cfg.ports.get(&key).is_some_and(|p| p.critical) {
        let name = cfg.ports[&key].label.clone().unwrap_or(key);
        v.reasons.insert(
            0,
            Why {
                text: format!("port {name} is marked critical in the config"),
                overridable: false,
            },
        );
    }
    v
}

pub fn report(t: &Tree, cfg: &Config, crit: &Critical) -> Value {
    // [@ANCHOR: bench_mcp_usb_hub_protect_report]
    let devices: Vec<Value> = crit
        .iter()
        .map(|(d, why)| {
            let hubs_above: Vec<String> = t.ancestry(d).iter().skip(1).filter(|a| !a.is_root_hub()).map(|a| a.path.clone()).collect();
            json!({"device": d, "vidpid": t.get(d).map(|x| x.vidpid()), "reasons": why.iter().map(|w| json!({"why": w.text, "overridable": w.overridable})).collect::<Vec<_>>(), "hubs_on_the_path": hubs_above})
        })
        .collect();
    let ports: Vec<&String> = cfg
        .ports
        .iter()
        .filter(|(_, p)| p.critical)
        .map(|(k, _)| k)
        .collect();
    json!({
        "critical_devices": devices, "critical_ports": ports,
        "rules": ["a port is refused when the device on it or any device below it is critical",
                  "a port whose device is a hub is refused (its uplink) unless the port has hub_uplink_ok in the config",
                  "reasons from the config and from bench instruments cannot be overridden; the others can with override_protection: true"],
    })
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::usb_hub::sysfs::Tree;
    use crate::usb_hub::testkit::{recorded_sys, Dir};
    use std::path::PathBuf;

    pub fn fixture() -> PathBuf {
        recorded_sys()
    }

    fn cfg(v: Value) -> Config {
        crate::usb_hub::parse_config(&v).unwrap()
    }

    fn none() -> Dir {
        Dir::new()
    }

    // Tests [@ANCHOR: bench_mcp_usb_hub_protect_verdict_permit]
    // Tests [@ANCHOR: bench_mcp_usb_hub_protect_add]
    // Tests [@ANCHOR: bench_mcp_usb_hub_protect_block_names]
    // Tests [@ANCHOR: bench_mcp_usb_hub_protect_check_port]
    #[test]
    fn a_hub_uplink_is_refused_unless_the_config_says_it_is_the_target() {
        let t = Tree::read(&fixture()).unwrap();
        let c = cfg(json!({}));
        let crit = critical_set(&t, &c, &[], &fixture(), none().path());
        let v = check_port(&t, &c, &crit, "3-2", 2);
        assert!(!v.clear() && v.overridable(), "{v:?}");
        assert!(v
            .permit("port 3-2/2", false)
            .unwrap_err()
            .contains("override_protection"));
        assert!(v.permit("port 3-2/2", true).is_ok());
        let ok = cfg(json!({"ports": {"3-2/2": {"hub_uplink_ok": true}}}));
        assert!(check_port(&t, &ok, &crit, "3-2", 2).clear());
        assert!(
            check_port(&t, &c, &crit, "3-2", 4).clear(),
            "an empty port has nothing to protect"
        );
    }

    // Tests [@ANCHOR: usb_hub_protection]
    #[test]
    fn config_critical_ports_and_devices_cannot_be_overridden_and_protect_the_hubs_above() {
        let t = Tree::read(&fixture()).unwrap();
        let c = cfg(
            json!({"ports": {"3-2.2/3": {"label": "scope codec", "critical": true}}, "critical_devices": ["08bb:29c0"]}),
        );
        let crit = critical_set(&t, &c, &[], &fixture(), none().path());
        assert!(
            crit.contains_key("3-2.2.3") && crit.contains_key("3-2.2.2"),
            "{crit:?}"
        );
        let hub = check_port(&t, &c, &crit, "3-2", 2);
        assert!(
            !hub.overridable(),
            "the hub on the path to a critical device: {hub:?}"
        );
        let e = hub.permit("port 3-2/2", true).unwrap_err();
        assert!(
            e.contains("no argument overrides") && e.contains("below 3-2.2"),
            "{e}"
        );
        let leaf = check_port(&t, &c, &crit, "3-2.2", 1);
        assert!(
            leaf.clear(),
            "a harmless sibling is not protected: {leaf:?}"
        );
        let flagged = check_port(&t, &c, &crit, "3-2.2", 3);
        assert!(flagged.reasons[0].text.contains("critical in the config"));
    }

    // Tests [@ANCHOR: bench_mcp_usb_hub_protect_check_device]
    #[test]
    fn the_root_filesystem_device_and_mounts_on_usb_storage_are_found_through_dm_and_partitions() {
        let t = Tree::read(&fixture()).unwrap();
        let dir = none();
        // A USB disk on 3-2.2.1 appears in the tree as a block node; the root filesystem is on a mapper device on top of it.
        let work = none();
        let mut tree_t = t.clone();
        let d = tree_t.devices.get_mut("3-2.2.1").unwrap();
        d.interfaces[0].nodes.push(crate::usb_hub::sysfs::Node {
            class: "block".into(),
            name: "sdb".into(),
            dev: None,
            path: Some("/dev/sdb".into()),
            id: None,
        });
        let d2 = tree_t.devices.get_mut("3-2.2.3").unwrap();
        d2.interfaces[0].nodes.push(crate::usb_hub::sysfs::Node {
            class: "block".into(),
            name: "sdc".into(),
            dev: None,
            path: Some("/dev/sdc".into()),
            id: None,
        });
        std::fs::create_dir_all(work.path().join("class/block/dm-0/slaves")).unwrap();
        std::fs::create_dir_all(work.path().join("class/block/sdb1")).unwrap();
        std::os::unix::fs::symlink("../sdb1", work.path().join("class/block/dm-0/slaves/sdb1"))
            .unwrap_or(());
        std::fs::write(dir.path().join("mounts"), "/dev/dm-0 / ext4 rw 0 0\n/dev/sdc1 /media/stick vfat rw 0 0\ntmpfs /run tmpfs rw 0 0\n").unwrap();
        let crit = critical_set(&tree_t, &cfg(json!({})), &[], work.path(), dir.path());
        let root = &crit["3-2.2.1"];
        assert!(
            root[0].text.contains("root filesystem") && !root[0].overridable,
            "{root:?}"
        );
        let stick = &crit["3-2.2.3"];
        assert!(
            stick[0].text.contains("mounted at /media/stick") && stick[0].overridable,
            "{stick:?}"
        );
        let v = check_device(&tree_t, &cfg(json!({})), &crit, "3-2.2");
        assert!(
            !v.overridable(),
            "root filesystem below the hub makes the hub's uplink unoverridable: {v:?}"
        );
    }

    // Tests [@ANCHOR: bench_mcp_usb_hub_protect_report]
    #[test]
    fn keyboards_and_bench_serial_adapters_are_critical() {
        let t = Tree::read(&fixture()).unwrap();
        let mut kb = t.clone();
        let d = kb.devices.get_mut("3-2.2.1").unwrap();
        d.interfaces[0].class = 3;
        d.interfaces[0].subclass = 1;
        d.interfaces[0].protocol = 1;
        d.interfaces[0].nodes.push(crate::usb_hub::sysfs::Node {
            class: "tty".into(),
            name: "ttyUSB7".into(),
            dev: None,
            path: Some("/dev/ttyUSB7".into()),
            id: None,
        });
        let bench = [BenchRef {
            instrument: "psu".into(),
            path: Some("/dev/ttyUSB7".into()),
            hub_port: None,
            vidpid: None,
        }];
        let crit = critical_set(&kb, &cfg(json!({})), &bench, &fixture(), none().path());
        let w = &crit["3-2.2.1"];
        assert!(w
            .iter()
            .any(|x| x.text.contains("keyboard") && x.overridable));
        assert!(w
            .iter()
            .any(|x| x.text.contains("bench instrument \"psu\"") && !x.overridable));
        let rep = report(&kb, &cfg(json!({})), &crit);
        assert_eq!(
            rep["critical_devices"][0]["hubs_on_the_path"],
            json!(["3-2.2", "3-2"])
        );
    }

    #[test]
    fn the_vna_hub_port_and_everything_below_it_is_protected_but_overridable() {
        let t = Tree::read(&fixture()).unwrap();
        let bench = [BenchRef {
            instrument: "vna".into(),
            path: None,
            hub_port: Some(("3-2".into(), 2)),
            vidpid: None,
        }];
        let c = cfg(json!({"ports": {"3-2/2": {"hub_uplink_ok": true}}}));
        let crit = critical_set(&t, &c, &bench, &fixture(), none().path());
        for d in ["3-2.2", "3-2.2.1", "3-2.2.2", "3-2.2.3"] {
            assert!(crit[d][0].text.contains("bench instrument \"vna\""), "{d}");
        }
        let leaf = check_device(&t, &c, &crit, "3-2.2.2");
        assert!(!leaf.clear() && leaf.overridable());
        assert!(leaf.permit("reset", false).is_err());
        assert!(leaf.permit("reset", true).is_ok());
        assert!(!check_port(&t, &c, &crit, "3-2", 2).clear());
        assert!(check_port(&t, &c, &crit, "3-2", 4).clear());
    }

    #[test]
    fn the_real_codecs_hid_interface_is_not_mistaken_for_a_keyboard() {
        let t = Tree::read(&fixture()).unwrap();
        let crit = critical_set(&t, &cfg(json!({})), &[], &fixture(), none().path());
        assert!(!crit.contains_key("3-2.2.2"), "{crit:?}");
    }
}
