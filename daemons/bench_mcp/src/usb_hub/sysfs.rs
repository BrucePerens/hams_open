// SPDX-License-Identifier: AGPL-3.0-or-later
//! The USB tree read from sysfs (`/sys/bus/usb/devices`), without root. Every reader takes the sysfs root as a
//! path: production passes `/sys`, the tests pass a recording of this development machine's `/sys`.

use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::Path;

/// One endpoint of the active alternate setting of an interface.
#[derive(Clone, Debug, PartialEq)]
pub struct Endpoint {
    pub address: u8,
    pub kind: String,
    pub max_packet: u32,
    pub interval_us: u32,
    pub direction: String,
}

/// A device node (or network interface, sound card) an interface exposes.
#[derive(Clone, Debug, PartialEq)]
pub struct Node {
    pub class: String,
    pub name: String,
    pub dev: Option<String>,
    pub path: Option<String>,
    pub id: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Interface {
    pub name: String,
    pub number: u32,
    pub alt: u32,
    pub class: u8,
    pub subclass: u8,
    pub protocol: u8,
    pub driver: Option<String>,
    pub nodes: Vec<Node>,
    pub endpoints: Vec<Endpoint>,
}

/// The state of one downstream port, from the hub's `<hub>-portN` directory.
#[derive(Clone, Debug, PartialEq)]
pub struct Port {
    pub hub: String,
    pub number: u32,
    pub state: String,
    pub connect_type: String,
    pub over_current_count: u64,
    pub disabled: bool,
    pub peer: Option<String>,
    pub child: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Controller {
    pub pci: String,
    pub driver: String,
    pub vendor: String,
    pub device: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Device {
    /// The sysfs name: `usb3` for a root hub, `3-2.2` for the device on port 2 of the hub `3-2`.
    pub path: String,
    pub parent: Option<String>,
    /// The port number on the parent (None for a root hub).
    pub port: Option<u32>,
    pub bus: u32,
    pub devnum: u32,
    pub vid: String,
    pub pid: String,
    pub manufacturer: Option<String>,
    pub product: Option<String>,
    pub serial: Option<String>,
    pub bcd_usb: String,
    pub class: u8,
    pub protocol: u8,
    pub speed: String,
    pub max_power_ma: u32,
    pub self_powered: bool,
    pub configuration: Option<String>,
    pub authorized: bool,
    pub removable: String,
    pub power_control: Option<String>,
    pub autosuspend_delay_ms: Option<i64>,
    pub runtime_status: Option<String>,
    pub maxchild: u32,
    pub interfaces: Vec<Interface>,
    pub ports: Vec<Port>,
    pub hub_interface_protocol: Option<u8>,
    pub controller: Option<Controller>,
}

impl Device {
    pub fn vidpid(&self) -> String {
        format!("{}:{}", self.vid, self.pid)
    }

    pub fn is_hub(&self) -> bool {
        self.class == 9 || (self.class == 0 && self.interfaces.iter().any(|i| i.class == 9))
    }

    pub fn is_root_hub(&self) -> bool {
        self.parent.is_none()
    }

    pub fn speed_mbps(&self) -> f64 {
        self.speed.parse().unwrap_or(0.0)
    }

    /// The speed class the device was negotiated at.
    pub fn speed_name(&self) -> &'static str {
        match self.speed_mbps() {
            s if s >= 20000.0 => "SuperSpeed+ 20 Gb/s",
            s if s >= 10000.0 => "SuperSpeed+ 10 Gb/s",
            s if s >= 5000.0 => "SuperSpeed 5 Gb/s",
            s if s >= 480.0 => "high-speed",
            s if s >= 12.0 => "full-speed",
            s if s > 0.0 => "low-speed",
            _ => "unknown",
        }
    }

    /// The transaction translator arrangement of a hub: the device protocol of a high-speed hub is 1 for a single TT and 2 for one
    /// per port; the multi-TT mode is the interface's alternate setting 1 (protocol 2), which the kernel selects when it can.
    pub fn tt_mode(&self) -> &'static str {
        if !self.is_hub() {
            return "not a hub";
        }
        if self.speed_mbps() >= 5000.0 {
            return "none (SuperSpeed hubs route without translators)";
        }
        if self.speed_mbps() < 480.0 {
            return "none (a full-speed hub)";
        }
        match (self.protocol, self.hub_interface_protocol) {
            (2, Some(2)) | (2, None) => "multi-TT (one per port)",
            (2, Some(_)) => "multi-TT hub running in single-TT mode",
            (1, _) => "single TT",
            _ => "none declared",
        }
    }
}

fn read(p: &Path) -> Option<String> {
    std::fs::read_to_string(p)
        .ok()
        .map(|s| s.trim().to_string())
}

fn hex_u8(s: Option<String>) -> u8 {
    s.and_then(|s| u8::from_str_radix(s.trim_start_matches("0x"), 16).ok())
        .unwrap_or(0)
}

fn link_name(p: &Path) -> Option<String> {
    std::fs::read_link(p)
        .ok()
        .and_then(|t| t.file_name().map(|n| n.to_string_lossy().to_string()))
}

pub fn is_device_name(n: &str) -> bool {
    if let Some(rest) = n.strip_prefix("usb") {
        return !rest.is_empty() && rest.bytes().all(|b| b.is_ascii_digit());
    }
    let Some((bus, path)) = n.split_once('-') else {
        return false;
    };
    !bus.is_empty()
        && bus.bytes().all(|b| b.is_ascii_digit())
        && !path.is_empty()
        && path
            .split('.')
            .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
}

/// The hub and port a device path hangs from: `3-2.2.1` is port 1 of `3-2.2`; `3-2` is port 2 of root hub `usb3`.
pub fn parent_of(path: &str) -> Option<(String, u32)> {
    if path.starts_with("usb") {
        return None;
    }
    let (bus, rest) = path.split_once('-')?;
    match rest.rsplit_once('.') {
        Some((up, port)) => Some((format!("{bus}-{up}"), port.parse().ok()?)),
        None => Some((format!("usb{bus}"), rest.parse().ok()?)),
    }
}

/// The path of the port-`port` child of `hub`: the inverse of `parent_of`.
pub fn child_path(hub: &str, port: u32) -> String {
    match hub.strip_prefix("usb") {
        Some(bus) => format!("{bus}-{port}"),
        None => format!("{hub}.{port}"),
    }
}

fn parse_interval_us(s: &str) -> u32 {
    let s = s.trim();
    if let Some(v) = s.strip_suffix("us") {
        return v.parse().unwrap_or(0);
    }
    if let Some(v) = s.strip_suffix("ms") {
        return v.parse::<u32>().unwrap_or(0) * 1000;
    }
    0
}

fn endpoints_of(dir: &Path) -> Vec<Endpoint> {
    let mut out = Vec::new();
    let Ok(rd) = std::fs::read_dir(dir) else {
        return out;
    };
    for e in rd.flatten() {
        let n = e.file_name().to_string_lossy().to_string();
        if !n.starts_with("ep_") {
            continue;
        }
        let d = e.path();
        out.push(Endpoint {
            address: hex_u8(read(&d.join("bEndpointAddress"))),
            kind: read(&d.join("type")).unwrap_or_default(),
            max_packet: read(&d.join("wMaxPacketSize"))
                .and_then(|s| u32::from_str_radix(&s, 16).ok())
                .unwrap_or(0),
            interval_us: read(&d.join("interval"))
                .map(|s| parse_interval_us(&s))
                .unwrap_or(0),
            direction: read(&d.join("direction")).unwrap_or_default(),
        });
    }
    out.sort_by_key(|e| e.address);
    out
}

const NODE_CLASSES: &[&str] = &[
    "tty",
    "block",
    "net",
    "sound",
    "hidraw",
    "input",
    "usbmisc",
    "video4linux",
    "misc",
    "scsi_generic",
];

fn dev_path(class: &str, name: &str) -> Option<String> {
    match class {
        "tty" | "block" | "hidraw" | "scsi_generic" | "usbmisc" | "video4linux" | "misc" => {
            Some(format!("/dev/{name}"))
        }
        "input" if name.starts_with("event") => Some(format!("/dev/input/{name}")),
        _ => None,
    }
}

fn collect_nodes(dir: &Path, depth: u32, out: &mut Vec<Node>) {
    if depth > 7 {
        return;
    }
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        let path = e.path();
        let is_dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
        if !is_dir || name == "power" || name.starts_with("ep_") {
            continue;
        }
        if NODE_CLASSES.contains(&name.as_str()) {
            class_nodes(&name, &path, out);
        } else {
            collect_nodes(&path, depth + 1, out);
        }
    }
}

fn class_nodes(class: &str, dir: &Path, out: &mut Vec<Node>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    let mut names: Vec<_> = rd
        .flatten()
        .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .collect();
    names.sort_by_key(|e| e.file_name());
    for e in names {
        let name = e.file_name().to_string_lossy().to_string();
        let p = e.path();
        if class == "input" {
            // inputN holds its eventN / mouseN children and a `name` file.
            let label = read(&p.join("name"));
            let mut kids = Vec::new();
            if let Ok(k) = std::fs::read_dir(&p) {
                for c in k.flatten() {
                    let cn = c.file_name().to_string_lossy().to_string();
                    if cn.starts_with("event") {
                        kids.push(cn);
                    }
                }
            }
            kids.sort();
            for kid in kids {
                out.push(Node {
                    class: "input".into(),
                    path: dev_path("input", &kid),
                    dev: read(&p.join(&kid).join("dev")),
                    id: label.clone(),
                    name: kid,
                });
            }
            continue;
        }
        out.push(Node {
            class: class.to_string(),
            path: dev_path(class, &name),
            dev: read(&p.join("dev")),
            id: read(&p.join("id")),
            name,
        });
    }
}

fn interface_of(dir: &Path, name: &str) -> Interface {
    let num = |f: &str| read(&dir.join(f)).and_then(|s| s.parse().ok()).unwrap_or(0);
    let mut nodes = Vec::new();
    collect_nodes(dir, 0, &mut nodes);
    Interface {
        name: name.to_string(),
        number: num("bInterfaceNumber"),
        alt: num("bAlternateSetting"),
        class: hex_u8(read(&dir.join("bInterfaceClass"))),
        subclass: hex_u8(read(&dir.join("bInterfaceSubClass"))),
        protocol: hex_u8(read(&dir.join("bInterfaceProtocol"))),
        driver: link_name(&dir.join("driver")),
        nodes,
        endpoints: endpoints_of(dir),
    }
}

fn ports_of(hub: &str, dir: &Path, nports: u32) -> Vec<Port> {
    let mut out = Vec::new();
    let Ok(rd) = std::fs::read_dir(dir) else {
        return out;
    };
    for e in rd.flatten() {
        let iface = e.path();
        let is_iface = e.file_name().to_string_lossy().contains(':') && iface.is_dir();
        if !is_iface {
            continue;
        }
        let Ok(inner) = std::fs::read_dir(&iface) else {
            continue;
        };
        for p in inner.flatten() {
            let pn = p.file_name().to_string_lossy().to_string();
            let Some((_, num)) = pn.rsplit_once("-port") else {
                continue;
            };
            let Ok(number) = num.parse::<u32>() else {
                continue;
            };
            if number > nports.max(255) {
                continue;
            }
            let pd = p.path();
            let child = child_path(hub, number);
            out.push(Port {
                hub: hub.to_string(),
                number,
                state: read(&pd.join("state")).unwrap_or_default(),
                connect_type: read(&pd.join("connect_type")).unwrap_or_default(),
                over_current_count: read(&pd.join("over_current_count"))
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(0),
                disabled: read(&pd.join("disable")).as_deref() == Some("1"),
                peer: std::fs::read_link(pd.join("peer"))
                    .ok()
                    .and_then(|t| t.file_name().map(|n| n.to_string_lossy().to_string())),
                child: Some(child),
            });
        }
    }
    out.sort_by_key(|p| p.number);
    out
}

fn controller_of(real_root_hub: &Path) -> Option<Controller> {
    let pci = real_root_hub.parent()?;
    let driver = link_name(&pci.join("driver"))?;
    Some(Controller {
        pci: pci.file_name()?.to_string_lossy().to_string(),
        driver,
        vendor: read(&pci.join("vendor")).unwrap_or_default(),
        device: read(&pci.join("device")).unwrap_or_default(),
    })
}

fn device_at(dir: &Path, path: &str) -> Option<Device> {
    let vid = read(&dir.join("idVendor"))?;
    let pid = read(&dir.join("idProduct"))?;
    let parent = parent_of(path);
    let attr = |f: &str| read(&dir.join(f));
    let num = |f: &str| {
        attr(f)
            .and_then(|s| s.trim().parse::<u32>().ok())
            .unwrap_or(0)
    };
    let mut interfaces = Vec::new();
    let prefix = format!("{path}:");
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let n = e.file_name().to_string_lossy().to_string();
            if n.starts_with(&prefix) {
                interfaces.push(interface_of(&e.path(), &n));
            }
        }
    }
    interfaces.sort_by_key(|i| (i.number, i.alt));
    let maxchild = num("maxchild");
    let bm = attr("bmAttributes").map(|s| u8::from_str_radix(s.trim(), 16).unwrap_or(0));
    let hub_interface_protocol = interfaces.iter().find(|i| i.class == 9).map(|i| i.protocol);
    let ports = if maxchild > 0 {
        ports_of(path, dir, maxchild)
    } else {
        Vec::new()
    };
    Some(Device {
        path: path.to_string(),
        port: parent.as_ref().map(|p| p.1),
        parent: parent.map(|p| p.0),
        bus: num("busnum"),
        devnum: num("devnum"),
        vid,
        pid,
        manufacturer: attr("manufacturer"),
        product: attr("product"),
        serial: attr("serial"),
        bcd_usb: attr("version").unwrap_or_default(),
        class: hex_u8(attr("bDeviceClass")),
        protocol: hex_u8(attr("bDeviceProtocol")),
        speed: attr("speed").unwrap_or_default(),
        max_power_ma: attr("bMaxPower")
            .map(|s| s.trim_end_matches("mA").trim().parse().unwrap_or(0))
            .unwrap_or(0),
        self_powered: bm.is_some_and(|b| b & 0x40 != 0),
        configuration: attr("configuration"),
        authorized: attr("authorized").as_deref() != Some("0"),
        removable: attr("removable").unwrap_or_default(),
        power_control: attr("power/control"),
        autosuspend_delay_ms: attr("power/autosuspend_delay_ms").and_then(|s| s.parse().ok()),
        runtime_status: attr("power/runtime_status"),
        maxchild,
        interfaces,
        ports,
        hub_interface_protocol,
        controller: None,
    })
}

/// The whole tree: every device by sysfs name.
#[derive(Clone, Debug, Default)]
pub struct Tree {
    pub devices: BTreeMap<String, Device>,
}

fn path_key(p: &str) -> (u32, Vec<u32>, bool) {
    let digits = |s: &str| s.parse::<u32>().unwrap_or(0);
    match p.strip_prefix("usb") {
        Some(b) => (digits(b), vec![], false),
        None => {
            let (bus, rest) = p.split_once('-').unwrap_or((p, ""));
            (digits(bus), rest.split('.').map(digits).collect(), true)
        }
    }
}

impl Tree {
    /// Reads `<sysfs>/bus/usb/devices`.
    pub fn read(sysfs: &Path) -> Result<Tree, String> {
        // [@ANCHOR: usb_hub_sysfs_tree]
        let dir = sysfs.join("bus/usb/devices");
        let rd = std::fs::read_dir(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let mut tree = Tree::default();
        let mut controllers: BTreeMap<u32, Controller> = BTreeMap::new();
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if !is_device_name(&name) {
                continue;
            }
            let Ok(real) = std::fs::canonicalize(e.path()) else {
                continue;
            };
            let Some(mut dev) = device_at(&real, &name) else {
                continue;
            };
            if dev.is_root_hub() {
                if let Some(c) = controller_of(&real) {
                    controllers.insert(dev.bus, c);
                }
            }
            dev.controller = None;
            tree.devices.insert(name, dev);
        }
        for d in tree.devices.values_mut() {
            d.controller = controllers.get(&d.bus).cloned();
        }
        let present: Vec<String> = tree.devices.keys().cloned().collect();
        for d in tree.devices.values_mut() {
            for p in d.ports.iter_mut() {
                if !p.child.as_ref().is_some_and(|c| present.contains(c)) {
                    p.child = None;
                }
            }
        }
        if tree.devices.is_empty() {
            return Err(format!("no USB devices found under {}", dir.display()));
        }
        Ok(tree)
    }

    pub fn get(&self, path: &str) -> Option<&Device> {
        self.devices.get(path)
    }

    /// Children of a device, in port order.
    pub fn children(&self, path: &str) -> Vec<&Device> {
        let mut v: Vec<&Device> = self
            .devices
            .values()
            .filter(|d| d.parent.as_deref() == Some(path))
            .collect();
        v.sort_by_key(|d| d.port);
        v
    }

    /// The root hubs, by bus number.
    pub fn roots(&self) -> Vec<&Device> {
        let mut v: Vec<&Device> = self.devices.values().filter(|d| d.is_root_hub()).collect();
        v.sort_by_key(|d| d.bus);
        v
    }

    /// The device and everything above it, root hub last.
    pub fn ancestry(&self, path: &str) -> Vec<&Device> {
        let mut out = Vec::new();
        let mut cur = self.get(path);
        while let Some(d) = cur {
            out.push(d);
            cur = d.parent.as_deref().and_then(|p| self.get(p));
        }
        out
    }

    /// `path` itself and every device below it.
    pub fn subtree(&self, path: &str) -> Vec<&Device> {
        let mut out: Vec<&Device> = self.get(path).into_iter().collect();
        let mut i = 0;
        while i < out.len() {
            let more = self.children(&out[i].path.clone());
            out.extend(more);
            i += 1;
        }
        out
    }

    pub fn sorted(&self) -> Vec<&Device> {
        let mut v: Vec<&Device> = self.devices.values().collect();
        v.sort_by_key(|d| path_key(&d.path));
        v
    }
}

/// What the device descriptor's USB version says the device can do, in Mb/s, and what to make of the negotiated speed.
/// A `bcdUSB` of 2.x does not prove high-speed capability (many full-speed devices report 2.0), so that case is a note and
/// only a USB 3 device on a slower link is a warning.
pub fn speed_finding(d: &Device, parent: Option<&Device>) -> Option<Value> {
    let ver: f64 = d.bcd_usb.parse().unwrap_or(0.0);
    let speed = d.speed_mbps();
    if d.is_root_hub() || speed == 0.0 {
        return None;
    }
    let parent_speed = parent.map(|p| p.speed_mbps()).unwrap_or(0.0);
    let (level, text) = if ver >= 3.0 && speed < 5000.0 {
        let why = if parent_speed > 0.0 && parent_speed < 5000.0 {
            format!(
                "its parent hub {} is itself only {} Mb/s",
                parent.map(|p| p.path.as_str()).unwrap_or("?"),
                parent_speed
            )
        } else {
            "it is on a USB 2.0 port, a USB 2.0 cable, or a USB 3 link that failed to train"
                .to_string()
        };
        (
            "warn",
            format!(
                "a USB {} device running at {} Mb/s instead of SuperSpeed: {why}",
                d.bcd_usb, speed
            ),
        )
    } else if (2.0..3.0).contains(&ver) && speed <= 12.0 && speed > 1.5 {
        let why = if parent_speed > 0.0 && parent_speed <= 12.0 {
            format!(
                "its parent hub {} is full-speed only",
                parent.map(|p| p.path.as_str()).unwrap_or("?")
            )
        } else {
            "the device may simply be full-speed only".to_string()
        };
        (
            "note",
            format!(
                "reports USB {} but runs at {} Mb/s: {why}",
                d.bcd_usb, speed
            ),
        )
    } else {
        return None;
    };
    Some(json!({"level": level, "text": text}))
}

fn node_json(n: &Node) -> Value {
    json!({"class": n.class, "name": n.name, "dev": n.dev, "path": n.path, "id": n.id})
}

fn iface_json(i: &Interface) -> Value {
    json!({
        "name": i.name, "number": i.number, "alt": i.alt,
        "class": format!("{:02x}", i.class), "subclass": format!("{:02x}", i.subclass), "protocol": format!("{:02x}", i.protocol),
        "driver": i.driver, "nodes": i.nodes.iter().map(node_json).collect::<Vec<_>>(),
    })
}

fn port_json(p: &Port, label: Option<&str>) -> Value {
    json!({
        "port": p.number, "label": label, "state": p.state, "connect_type": p.connect_type,
        "over_current_count": p.over_current_count, "disabled_by_software": p.disabled, "peer_port": p.peer, "device": p.child,
    })
}

/// One device as JSON. `labels` maps a port key (`hub/port`) to its label.
pub fn device_json(t: &Tree, d: &Device, labels: &dyn Fn(&str, u32) -> Option<String>) -> Value {
    let parent = d.parent.as_deref().and_then(|p| t.get(p));
    let label = d
        .parent
        .as_ref()
        .zip(d.port)
        .and_then(|(h, p)| labels(h, p));
    let ctrl = d
        .controller
        .as_ref()
        .map(|c| json!({"pci": c.pci, "driver": c.driver, "vendor": c.vendor, "device": c.device}));
    json!({
        "path": d.path, "label": label, "bus": d.bus, "devnum": d.devnum, "vidpid": d.vidpid(),
        "manufacturer": d.manufacturer, "product": d.product, "serial": d.serial,
        "parent": d.parent, "port": d.port,
        "usb_version": d.bcd_usb, "speed_mbps": d.speed, "speed": d.speed_name(), "speed_finding": speed_finding(d, parent),
        "class": format!("{:02x}", d.class), "is_hub": d.is_hub(),
        "max_power_ma": d.max_power_ma, "self_powered": d.self_powered, "power_budget": super::diagnose::power_budget(t, d),
        "authorized": d.authorized, "removable": d.removable,
        "power": {"control": d.power_control, "autosuspend_delay_ms": d.autosuspend_delay_ms, "runtime_status": d.runtime_status},
        "controller": ctrl,
        "hub": if d.is_hub() { Some(json!({
            "ports": d.maxchild, "tt": d.tt_mode(),
            "port_status": d.ports.iter().map(|p| port_json(p, labels(&d.path, p.number).as_deref())).collect::<Vec<_>>(),
        })) } else { None },
        "interfaces": d.interfaces.iter().map(iface_json).collect::<Vec<_>>(),
    })
}

/// The tree as nested JSON: each device with its `children`.
pub fn tree_json(t: &Tree, root: &str, labels: &dyn Fn(&str, u32) -> Option<String>) -> Value {
    let Some(d) = t.get(root) else {
        return Value::Null;
    };
    let mut v = device_json(t, d, labels);
    let kids: Vec<Value> = t
        .children(root)
        .iter()
        .map(|c| tree_json(t, &c.path, labels))
        .collect();
    v["children"] = Value::Array(kids);
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    pub fn fixture() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/usb_fixtures/devbox/sys")
    }

    // Tests [@ANCHOR: usb_hub_sysfs_tree]
    #[test]
    fn the_recorded_tree_has_the_vnwa_chain_and_its_topology() {
        let t = Tree::read(&fixture()).unwrap();
        assert_eq!(t.roots().len(), 4);
        let hub = t.get("3-2").unwrap();
        assert_eq!(
            (hub.vidpid().as_str(), hub.maxchild, hub.parent.as_deref()),
            ("14b0:013a", 5, Some("usb3"))
        );
        assert!(hub.is_hub());
        assert_eq!(hub.tt_mode(), "multi-TT (one per port)");
        let inner = t.get("3-2.2").unwrap();
        assert_eq!(
            (inner.parent.as_deref(), inner.port, inner.speed.as_str()),
            (Some("3-2"), Some(2), "12")
        );
        assert_eq!(inner.tt_mode(), "none (a full-speed hub)");
        let kids: Vec<_> = t.children("3-2.2").iter().map(|d| d.path.clone()).collect();
        assert_eq!(kids, ["3-2.2.1", "3-2.2.2", "3-2.2.3"]);
        let chain: Vec<_> = t
            .ancestry("3-2.2.3")
            .iter()
            .map(|d| d.path.clone())
            .collect();
        assert_eq!(chain, ["3-2.2.3", "3-2.2", "3-2", "usb3"]);
        let c = hub.controller.as_ref().unwrap();
        assert_eq!(
            (c.pci.as_str(), c.driver.as_str()),
            ("0000:00:14.0", "xhci_hcd")
        );
        assert_eq!(hub.ports[1].child.as_deref(), Some("3-2.2"));
        assert_eq!(hub.ports[0].child, None);
    }

    #[test]
    fn a_codec_exposes_its_sound_card_hid_node_and_drivers() {
        let t = Tree::read(&fixture()).unwrap();
        let codec = t.get("3-2.2.2").unwrap();
        assert_eq!(codec.product.as_deref(), Some("USB AUDIO  CODEC"));
        let snd = codec
            .interfaces
            .iter()
            .find(|i| i.driver.as_deref() == Some("snd-usb-audio"))
            .unwrap();
        let card = snd.nodes.iter().find(|n| n.class == "sound").unwrap();
        assert!(
            card.name.starts_with("card")
                && card.id.as_deref().is_some_and(|i| i.starts_with("CODEC")),
            "{card:?}"
        );
        let hid = codec
            .interfaces
            .iter()
            .flat_map(|i| &i.nodes)
            .find(|n| n.class == "hidraw")
            .unwrap();
        assert!(hid.path.as_deref().unwrap().starts_with("/dev/hidraw"));
        assert_eq!(
            codec.serial.as_deref(),
            None,
            "recorded serial numbers are redacted or absent"
        );
    }

    #[test]
    fn path_arithmetic_round_trips() {
        assert_eq!(parent_of("3-2.2.1"), Some(("3-2.2".into(), 1)));
        assert_eq!(parent_of("3-2"), Some(("usb3".into(), 2)));
        assert_eq!(parent_of("usb3"), None);
        assert_eq!(child_path("usb3", 2), "3-2");
        assert_eq!(child_path("3-2.2", 3), "3-2.2.3");
        assert!(
            is_device_name("3-2.2.1")
                && is_device_name("usb4")
                && !is_device_name("3-2:1.0")
                && !is_device_name("3-0:1.0")
        );
    }

    #[test]
    fn speed_findings_separate_a_warning_from_a_note() {
        let t = Tree::read(&fixture()).unwrap();
        let codec = t.get("3-2.2.2").unwrap();
        let inner = t.get("3-2.2").unwrap();
        assert!(
            speed_finding(inner, t.get("3-2")).is_none(),
            "a USB 1.1 hub at 12 Mb/s is as designed"
        );
        let note = speed_finding(codec, Some(inner)).unwrap();
        assert!(
            note["level"] == "note" && note["text"].as_str().unwrap().contains("full-speed only"),
            "{note}"
        );
        let mut ss = t.get("2-3").unwrap().clone();
        ss.speed = "480".into();
        let f = speed_finding(&ss, t.get("usb2")).unwrap();
        assert_eq!(f["level"], "warn");
    }

    #[test]
    fn the_json_tree_nests_children_and_labels_ports() {
        let t = Tree::read(&fixture()).unwrap();
        let label = |h: &str, p: u32| (h == "3-2" && p == 2).then(|| "VNWA 3E".to_string());
        let v = tree_json(&t, "usb3", &label);
        let hub = &v["children"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["path"] == "3-2")
            .unwrap()
            .clone();
        assert_eq!(hub["hub"]["port_status"][1]["label"], "VNWA 3E");
        assert_eq!(hub["children"][0]["label"], "VNWA 3E");
        assert_eq!(hub["children"][0]["children"].as_array().unwrap().len(), 3);
    }
}
