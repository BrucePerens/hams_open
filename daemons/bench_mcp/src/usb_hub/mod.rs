// SPDX-License-Identifier: AGPL-3.0-or-later
//! USB hub tools for bench_mcp: the USB tree as the kernel sees it (read from sysfs without root), hub capabilities and
//! per-port status, port power control through `sudo -n uhubctl`, the escalation ladder from a device reset to a port
//! power cycle, the kernel's USB event history, bus bandwidth sharing and a `diagnose` tool.
//!
//! The tools belong to one instrument entry with `"driver": "usb_hub"` in the bench config. Its tier decides what is
//! listed and allowed: `read_only` (listing, status, history), `read_config` (device reset, authorize, driver unbind
//! and bind, autosuspend, and switching a port back on), `full` (switching a port off, and power cycles).

pub mod sysfs;

use crate::config::{Instrument, Kind, Tier};
use rmcp::model::Tool;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

/// What the config file says about one port of one hub (`"3-2/2"`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PortCfg {
    pub label: Option<String>,
    /// A port whose device must never be switched off or reset by a tool.
    pub critical: bool,
    /// The operator says the hub plugged into this port is the intended target of power control.
    pub hub_uplink_ok: bool,
    /// How power clearing was confirmed by hand ("goes dark and keeps no settings"), if it was.
    pub confirmed_by_hand: Option<String>,
    /// A device-specific check or note that the device loses its settings (verification level 3).
    pub state_cleared: Option<String>,
    /// A manual VBUS measurement (verification level 4).
    pub vbus_measured: Option<String>,
}

/// The `usb_hub` entry of the bench config.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Config {
    pub ports: BTreeMap<String, PortCfg>,
    /// `vid:pid` values whose devices are critical wherever they are plugged in.
    pub critical_devices: Vec<String>,
}

impl Config {
    pub fn label(&self, hub: &str, port: u32) -> Option<String> {
        self.ports
            .get(&format!("{hub}/{port}"))
            .and_then(|p| p.label.clone())
    }
}

fn port_key(k: &str) -> Result<(), String> {
    let ok = k.split_once('/').is_some_and(|(h, p)| {
        !h.is_empty()
            && !p.is_empty()
            && p.bytes().all(|b| b.is_ascii_digit())
            && (h.starts_with("usb") || h.contains('-'))
    });
    if ok {
        Ok(())
    } else {
        Err(format!(
            "port key {k:?} must look like \"3-2/2\" (hub path, slash, port number)"
        ))
    }
}

/// Reads the `usb_hub` settings of an instrument entry: `ports` (a map of `"hub/port"` to `label`, `critical`,
/// `hub_uplink_ok`, `confirmed_by_hand`, `state_cleared`, `vbus_measured`) and `critical_devices` (`vid:pid` list).
pub fn parse_config(c: &Value) -> Result<Config, String> {
    // [@ANCHOR: usb_hub_config]
    let mut cfg = Config::default();
    if let Some(ports) = c.get("ports") {
        let map = ports.as_object().ok_or("ports must be an object")?;
        for (k, v) in map {
            port_key(k)?;
            let text = |f: &str| v.get(f).and_then(|x| x.as_str()).map(String::from);
            let flag = |f: &str| v.get(f).and_then(|x| x.as_bool()).unwrap_or(false);
            let label = text("label");
            if let Some(l) = &label {
                if l.trim().is_empty() {
                    return Err(format!("port {k}: label is empty"));
                }
                let dup = cfg
                    .ports
                    .iter()
                    .find(|(_, p)| p.label.as_ref().is_some_and(|x| x.eq_ignore_ascii_case(l)));
                if let Some((other, _)) = dup {
                    return Err(format!("label {l:?} is on both {other} and {k}"));
                }
            }
            cfg.ports.insert(
                k.clone(),
                PortCfg {
                    label,
                    critical: flag("critical"),
                    hub_uplink_ok: flag("hub_uplink_ok"),
                    confirmed_by_hand: text("confirmed_by_hand"),
                    state_cleared: text("state_cleared"),
                    vbus_measured: text("vbus_measured"),
                },
            );
        }
    }
    if let Some(list) = c.get("critical_devices") {
        for x in list
            .as_array()
            .ok_or("critical_devices must be a list of \"vid:pid\"")?
        {
            let s = x.as_str().unwrap_or_default().to_ascii_lowercase();
            let ok = s.len() == 9
                && s.as_bytes()[4] == b':'
                && s.replace(':', "").bytes().all(|b| b.is_ascii_hexdigit());
            if !ok {
                return Err(format!(
                    "critical_devices entry {x} is not a \"vid:pid\" like \"0451:2036\""
                ));
            }
            cfg.critical_devices.push(s);
        }
    }
    Ok(cfg)
}

pub struct Spec {
    pub name: &'static str,
    pub tier: Tier,
    pub desc: &'static str,
    pub props: &'static str,
    pub required: &'static [&'static str],
}

macro_rules! props {
    ($($rest:tt)*) => { concat!("{", $($rest)*, "}") };
}

pub const SPECS: &[Spec] = &[
    Spec {
        name: "bench_usb_topology",
        tier: Tier::ReadOnly,
        desc: "The USB tree from sysfs (no root needed): per device its bus, controller (PCI address and driver), hub path and port, port label, vid:pid, strings, class, USB version, NEGOTIATED speed with a finding when it is below what the device can do, configured power (bMaxPower), authorized flag, autosuspend state, interfaces with bound drivers and the device nodes they expose (tty, block, net, sound card, hidraw, input event), and for hubs the port count, TT mode and per-port state (state, connect type, over-current count). `device` (a path such as 3-2.2.1, or a port label) returns that device and everything below it; `bus` limits to one bus.",
        props: props!(r#""instrument":{"type":"string"},"device":{"type":"string","description":"device path or port label"},"bus":{"type":"integer"},"flat":{"type":"boolean","description":"a list instead of nesting"}"#),
        required: &[],
    },
    Spec {
        name: "bench_usb_find",
        tier: Tier::ReadOnly,
        desc: "Find the device behind a node or a name: give `node` (for example /dev/ttyUSB0, ttyACM1, hidraw3, eth1, a sound card id such as CODEC, or a sdX block name) or `vidpid`, `product` (substring) or `label`, and get the USB path, port label and the node, so that 'which device is /dev/ttyUSB0' has an answer.",
        props: props!(r#""instrument":{"type":"string"},"node":{"type":"string"},"vidpid":{"type":"string"},"product":{"type":"string"},"label":{"type":"string"}"#),
        required: &[],
    },
];

fn instrument<'a>(insts: &'a [Instrument], args: &Value) -> Result<&'a Instrument, String> {
    let hubs: Vec<&Instrument> = insts.iter().filter(|i| i.kind == Kind::UsbHub).collect();
    if let Some(n) = args.get("instrument").and_then(|n| n.as_str()) {
        return hubs
            .iter()
            .copied()
            .find(|i| i.name == n)
            .ok_or_else(|| format!("no usb_hub instrument named {n:?}"));
    }
    match hubs.as_slice() {
        [one] => Ok(one),
        [] => Err("no usb_hub instrument is configured".into()),
        many => Err(format!(
            "name the instrument: {}",
            many.iter()
                .map(|i| i.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )),
    }
}

pub fn tool_defs(insts: &[Instrument]) -> Vec<Tool> {
    SPECS
        .iter()
        .filter(|s| insts.iter().any(|i| i.kind == Kind::UsbHub && i.tier >= s.tier))
        .map(|s| {
            let schema = json!({"type": "object", "properties": serde_json::from_str::<Value>(s.props).unwrap(), "required": s.required});
            let Value::Object(schema) = schema else { unreachable!() };
            Tool::new(s.name, format!("[{:?}] {}", s.tier, s.desc), Arc::new(schema))
        })
        .collect()
}

/// Resolves a device argument: a sysfs path in the tree, or a port label.
pub fn resolve_device(t: &sysfs::Tree, cfg: &Config, s: &str) -> Result<String, String> {
    if t.get(s).is_some() {
        return Ok(s.to_string());
    }
    for (key, p) in &cfg.ports {
        if p.label.as_ref().is_some_and(|l| l.eq_ignore_ascii_case(s)) {
            let (hub, port) = key.split_once('/').unwrap_or_default();
            let child = sysfs::child_path(hub, port.parse().unwrap_or(0));
            return if t.get(&child).is_some() {
                Ok(child)
            } else {
                Err(format!(
                    "port {key} is labelled {s:?} but nothing is on it now"
                ))
            };
        }
    }
    Err(format!(
        "{s:?} is neither a device path in the tree nor a port label"
    ))
}

fn labeler(cfg: &Config) -> impl Fn(&str, u32) -> Option<String> + '_ {
    move |h, p| cfg.label(h, p)
}

fn topology(root: &Path, cfg: &Config, args: &Value) -> Result<Value, String> {
    let t = sysfs::Tree::read(root)?;
    let lab = labeler(cfg);
    let flat = args.get("flat").and_then(|v| v.as_bool()).unwrap_or(false);
    if let Some(d) = args.get("device").and_then(|v| v.as_str()) {
        let path = resolve_device(&t, cfg, d)?;
        return Ok(if flat {
            json!(t
                .subtree(&path)
                .iter()
                .map(|d| sysfs::device_json(&t, d, &lab))
                .collect::<Vec<_>>())
        } else {
            sysfs::tree_json(&t, &path, &lab)
        });
    }
    let bus = args.get("bus").and_then(|v| v.as_u64());
    let roots: Vec<&sysfs::Device> = t
        .roots()
        .into_iter()
        .filter(|r| bus.is_none_or(|b| u64::from(r.bus) == b))
        .collect();
    if roots.is_empty() {
        return Err(format!("no USB bus {}", bus.unwrap_or(0)));
    }
    if flat {
        let all: Vec<Value> = roots
            .iter()
            .flat_map(|r| t.subtree(&r.path))
            .map(|d| sysfs::device_json(&t, d, &lab))
            .collect();
        return Ok(json!(all));
    }
    Ok(json!(roots
        .iter()
        .map(|r| sysfs::tree_json(&t, &r.path, &lab))
        .collect::<Vec<_>>()))
}

fn find(root: &Path, cfg: &Config, args: &Value) -> Result<Value, String> {
    let t = sysfs::Tree::read(root)?;
    let text = |k: &str| {
        args.get(k)
            .and_then(|v| v.as_str())
            .map(|s| s.to_ascii_lowercase())
    };
    let (node, vidpid, product, label) =
        (text("node"), text("vidpid"), text("product"), text("label"));
    if node.is_none() && vidpid.is_none() && product.is_none() && label.is_none() {
        return Err("give node, vidpid, product or label".into());
    }
    let node = node.map(|n| {
        n.trim_start_matches("/dev/")
            .trim_start_matches("input/")
            .to_string()
    });
    let mut hits = Vec::new();
    for d in t.sorted() {
        let port_label = d
            .parent
            .as_ref()
            .zip(d.port)
            .and_then(|(h, p)| cfg.label(h, p));
        let mut why = Vec::new();
        if let Some(n) = &node {
            for i in &d.interfaces {
                for nd in i.nodes.iter().filter(|nd| {
                    nd.name.eq_ignore_ascii_case(n)
                        || nd.id.as_ref().is_some_and(|x| x.eq_ignore_ascii_case(n))
                }) {
                    why.push(json!({"interface": i.name, "driver": i.driver, "node": nd.name, "class": nd.class, "path": nd.path}));
                }
            }
            if why.is_empty() {
                continue;
            }
        }
        if vidpid.as_ref().is_some_and(|v| *v != d.vidpid()) {
            continue;
        }
        if product.as_ref().is_some_and(|p| {
            !d.product
                .as_deref()
                .unwrap_or_default()
                .to_ascii_lowercase()
                .contains(p.as_str())
        }) {
            continue;
        }
        if label.as_ref().is_some_and(|l| {
            !port_label
                .as_deref()
                .unwrap_or_default()
                .to_ascii_lowercase()
                .contains(l.as_str())
        }) {
            continue;
        }
        hits.push(json!({
            "device": d.path, "label": port_label, "vidpid": d.vidpid(), "product": d.product, "bus": d.bus, "devnum": d.devnum,
            "chain": t.ancestry(&d.path).iter().rev().map(|a| a.path.clone()).collect::<Vec<_>>(), "nodes": why,
        }));
    }
    if hits.is_empty() {
        return Err("nothing in the USB tree matches".into());
    }
    Ok(json!(hits))
}

pub fn call(insts: &[Instrument], name: &str, args: &Value) -> Result<Value, String> {
    let spec = SPECS
        .iter()
        .find(|s| s.name == name)
        .ok_or_else(|| format!("unknown tool {name}"))?;
    let inst = instrument(insts, args)?;
    if inst.tier < spec.tier {
        return Err(format!(
            "{name} needs tier {:?}; {:?} is configured at {:?}",
            spec.tier, inst.name, inst.tier
        ));
    }
    let root = sysfs::sysfs_root();
    match name {
        "bench_usb_topology" => topology(&root, &inst.usb, args),
        "bench_usb_find" => find(&root, &inst.usb, args),
        _ => Err(format!("{name} does not apply to {:?}", inst.name)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/usb_fixtures/devbox/sys")
    }

    fn cfg() -> Config {
        parse_config(&json!({"ports": {"3-2/2": {"label": "VNWA 3E", "hub_uplink_ok": true}}, "critical_devices": ["0451:2036"]})).unwrap()
    }

    // Tests [@ANCHOR: usb_hub_config]
    #[test]
    fn config_keeps_labels_flags_and_refuses_bad_entries() {
        let c = cfg();
        assert_eq!(c.label("3-2", 2).as_deref(), Some("VNWA 3E"));
        assert!(c.ports["3-2/2"].hub_uplink_ok && !c.ports["3-2/2"].critical);
        assert_eq!(c.critical_devices, ["0451:2036"]);
        let bad = |v: Value| parse_config(&v).unwrap_err();
        assert!(bad(json!({"ports": {"32": {}}})).contains("port key"));
        assert!(
            bad(json!({"ports": {"3-2/2": {"label": "a"}, "3-2/3": {"label": "A"}}}))
                .contains("both")
        );
        assert!(bad(json!({"critical_devices": ["zzzz:0000"]})).contains("vid:pid"));
        assert!(bad(json!({"ports": {"3-2/2": {"label": " "}}})).contains("empty"));
    }

    #[test]
    fn topology_lists_buses_a_subtree_by_label_and_refuses_unknowns() {
        let all = topology(&fixture(), &cfg(), &json!({})).unwrap();
        assert_eq!(all.as_array().unwrap().len(), 4);
        let sub = topology(
            &fixture(),
            &cfg(),
            &json!({"device": "vnwa 3e", "flat": true}),
        )
        .unwrap();
        let paths: Vec<_> = sub
            .as_array()
            .unwrap()
            .iter()
            .map(|d| d["path"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(paths, ["3-2.2", "3-2.2.1", "3-2.2.2", "3-2.2.3"]);
        assert!(topology(&fixture(), &cfg(), &json!({"device": "nope"})).is_err());
        assert!(topology(&fixture(), &cfg(), &json!({"bus": 9})).is_err());
        let one = topology(&fixture(), &cfg(), &json!({"bus": 3})).unwrap();
        assert_eq!(one.as_array().unwrap().len(), 1);
    }

    #[test]
    fn find_answers_which_device_is_this_node() {
        let hit = find(&fixture(), &cfg(), &json!({"node": "/dev/hidraw3"}));
        let hits = hit
            .or_else(|_| find(&fixture(), &cfg(), &json!({"node": "CODEC"})))
            .unwrap();
        assert!(hits[0]["device"].as_str().unwrap().starts_with("3-2.2."));
        let v = find(&fixture(), &cfg(), &json!({"label": "vnwa"})).unwrap();
        assert_eq!(v[0]["device"], "3-2.2");
        let by_id = find(&fixture(), &cfg(), &json!({"vidpid": "0451:2036"})).unwrap();
        assert_eq!(by_id[0]["label"], "VNWA 3E");
        assert_eq!(by_id[0]["chain"], json!(["usb3", "3-2", "3-2.2"]));
        assert!(find(&fixture(), &cfg(), &json!({})).is_err());
    }

    #[test]
    fn tools_are_listed_by_the_tier_of_the_instrument() {
        let c = |tier: &str| {
            crate::config::parse(&format!(
                r#"{{"instruments":{{"usb":{{"driver":"usb_hub","tier":"{tier}"}}}}}}"#
            ))
            .unwrap()
        };
        assert!(!tool_defs(&c("read_only")).is_empty());
        assert!(SPECS
            .iter()
            .all(|s| serde_json::from_str::<Value>(s.props).is_ok()));
        let e = call(&c("read_only"), "bench_usb_nope", &json!({})).unwrap_err();
        assert!(e.contains("unknown"), "{e}");
    }
}
