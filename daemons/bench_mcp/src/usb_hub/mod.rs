// SPDX-License-Identifier: AGPL-3.0-or-later
//! USB hub tools for bench_mcp: the USB tree as the kernel sees it (read from sysfs without root), hub capabilities and
//! per-port status, port power control through `sudo -n uhubctl`, the escalation ladder from a device reset to a port
//! power cycle, the kernel's USB event history, bus bandwidth sharing and a `diagnose` tool.
//!
//! The tools belong to one instrument entry with `"driver": "usb_hub"` in the bench config. Its tier decides what is
//! listed and allowed: `read_only` (listing, status, history), `read_config` (device reset, authorize, driver unbind
//! and bind, autosuspend, and switching a port back on), `full` (switching a port off, and power cycles).

pub mod bandwidth;
pub mod diagnose;
pub mod history;
pub mod hubs;
pub mod ladder;
pub mod power;
pub mod privileged;
pub mod protect;
pub mod sysfs;
#[cfg(test)]
pub mod testkit;
pub mod uhubctl;

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
    Spec {
        name: "bench_usb_hubs",
        tier: Tier::ReadOnly,
        desc: "Hub capabilities and per-port status: for each hub its port count, TT mode, uplink, what uhubctl says about power switching (per-port, ganged, or not listed), the hub class descriptor (power-switching mode, over-current protection mode, TT think time, as the hub CLAIMS them), and per port the sysfs state, over-current count, the decoded status word (powered, connected, enabled, suspended, over-current, reset, speed, USB 3 link state) and the power-control verification level: 1 claimed (uhubctl reports per-port switching), 2 re-enumerated (a power cycle by this server showed the device leave and return with a new device number, or it was confirmed by hand in the config), 3 state cleared, 4 VBUS measured (both from the config). Level 2 shows the port dropped the device, not that VBUS fell. Runs `sudo -n uhubctl` and `sudo -n lsusb -v` for what only root can read; `privileged: false` skips them and `hub` limits to one hub.",
        props: props!(r#""instrument":{"type":"string"},"hub":{"type":"string","description":"one hub path, for example 3-2"},"privileged":{"type":"boolean","description":"false skips the sudo reads"}"#),
        required: &[],
    },
    Spec {
        name: "bench_usb_protection",
        tier: Tier::ReadOnly,
        desc: "The protection list: devices that power-off, reset and unbind tools refuse by default (the root filesystem and other mounted USB storage, boot-protocol keyboards and mice, config-critical ports and devices, the serial adapters of bench instruments, every hub on the path to them, and the uplink of any hub), with the reasons and which of them cannot be overridden. Give hub and port, label or device to see the verdict for one target.",
        props: props!(r#""instrument":{"type":"string"},"hub":{"type":"string"},"port":{"type":"integer"},"label":{"type":"string"},"device":{"type":"string"}"#),
        required: &[],
    },
    Spec {
        name: "bench_usb_port_power",
        tier: Tier::ReadConfig,
        desc: "Switch one hub port off, on, or cycle it (off, wait, on) through `sudo -n uhubctl`, only on ports of hubs uhubctl reports as per-port switching. off and cycle need tier full; on needs read_config. A cycle waits for the device to leave the USB tree, keeps the port off for off_time_s (default 5, 1 to 60) and reports the ACTUAL off time, waits up to back_timeout_s (default 45) for the expected vid:pid to enumerate with a new device number, and reports how long that took and the verification level reached (2 when the leave-and-return was seen). The port is named by hub and port, label, or device. Refused by default for the protected devices and hub uplinks (see bench_usb_protection); override_protection: true overrides only the overridable reasons. dry_run: true says what would happen and changes nothing.",
        props: props!(r#""instrument":{"type":"string"},"action":{"type":"string","enum":["off","on","cycle"]},"hub":{"type":"string"},"port":{"type":"integer"},"label":{"type":"string"},"device":{"type":"string"},"off_time_s":{"type":"number"},"back_timeout_s":{"type":"number"},"expect_vidpid":{"type":"string"},"dry_run":{"type":"boolean"},"override_protection":{"type":"boolean"}"#),
        required: &["action"],
    },
    Spec {
        name: "bench_usb_reset",
        tier: Tier::ReadConfig,
        desc: "Soft reset of one USB device (USBDEVFS_RESET): the host re-initialises it, it keeps its power and device number, its drivers are rebound. First rung of the escalation ladder: reset, then deauthorize/authorize, then driver unbind/bind, then bench_usb_port_power. Needs root, which it gets by running this binary's --usb-priv helper through `sudo -n`. Refused for protected devices and hub uplinks (see bench_usb_protection); override_protection overrides only the overridable reasons. dry_run says what would happen.",
        props: props!(r#""instrument":{"type":"string"},"device":{"type":"string","description":"device path such as 3-2.2.1, or a port label"},"dry_run":{"type":"boolean"},"override_protection":{"type":"boolean"}"#),
        required: &["device"],
    },
    Spec {
        name: "bench_usb_authorize",
        tier: Tier::ReadConfig,
        desc: "Deauthorize (authorized: false) or authorize (true) a USB device: a software disconnect that keeps the device powered. Deauthorizing unbinds its drivers; the device stays in the tree but unused. Same privilege route and protection rules as bench_usb_reset; authorizing is never refused. dry_run says what would happen.",
        props: props!(r#""instrument":{"type":"string"},"device":{"type":"string"},"authorized":{"type":"boolean"},"dry_run":{"type":"boolean"},"override_protection":{"type":"boolean"}"#),
        required: &["device", "authorized"],
    },
    Spec {
        name: "bench_usb_driver",
        tier: Tier::ReadConfig,
        desc: "Unbind a USB interface from its driver, or bind it to a named driver (the interface name is like 3-2.2.2:1.3, as bench_usb_topology lists it). Unbinding removes the nodes the driver made (tty, sound card, input). Same privilege route and protection rules as bench_usb_reset. dry_run lists the nodes that would go away.",
        props: props!(r#""instrument":{"type":"string"},"interface":{"type":"string"},"action":{"type":"string","enum":["unbind","bind"]},"driver":{"type":"string","description":"required for bind; unbind defaults to the bound driver"},"dry_run":{"type":"boolean"},"override_protection":{"type":"boolean"}"#),
        required: &["interface", "action"],
    },
    Spec {
        name: "bench_usb_autosuspend",
        tier: Tier::ReadConfig,
        desc: "Runtime power management of one USB device: control on (never autosuspend) or auto (may suspend after delay_ms, -1 to 3600000). A device that misbehaves when suspended is fixed with on. Setting auto on a protected device is refused. dry_run shows the current setting.",
        props: props!(r#""instrument":{"type":"string"},"device":{"type":"string"},"control":{"type":"string","enum":["on","auto"]},"delay_ms":{"type":"integer"},"dry_run":{"type":"boolean"},"override_protection":{"type":"boolean"}"#),
        required: &["device", "control"],
    },
    Spec {
        name: "bench_usb_history",
        tier: Tier::ReadOnly,
        desc: "USB event history from the kernel journal (`journalctl -k -o short-unix`): the connect, disconnect, reset and enumeration timeline, per-device counters (connects, disconnects, resets, enumeration failures, device descriptor read failures, address errors, kernel power-cycle attempts, over-current, -19 'no device' urb lines, xhci errors, errno counts such as -71 EPROTO) and flapping detection (flap_connects or more connects of one device within flap_window_s, defaults 3 and 120 s). `device` (path or label) limits to a device and everything below it; since_s limits to the last N seconds; max_events caps the timeline (default 100). The journal needs root or the group systemd-journal/adm; otherwise this fails naming the fix, and use_sudo: true runs `sudo -n journalctl -k` for the call.",
        props: props!(r#""instrument":{"type":"string"},"device":{"type":"string"},"since_s":{"type":"number"},"max_events":{"type":"integer"},"flap_window_s":{"type":"number"},"flap_connects":{"type":"integer"},"all_boots":{"type":"boolean"},"use_sudo":{"type":"boolean"}"#),
        required: &[],
    },
    Spec {
        name: "bench_usb_watch",
        tier: Tier::ReadOnly,
        desc: "Watch the USB tree for `seconds` (1 to 120) by polling sysfs and report every change with its time: devices appearing, disappearing or re-enumerating (new device number), speed or authorized changes, port state changes and over-current count rises, with the number of enumerations per device and which devices flapped. Polling can miss a change shorter than interval_ms (default 200); bench_usb_history from the journal is exact.",
        props: props!(r#""instrument":{"type":"string"},"seconds":{"type":"number"},"interval_ms":{"type":"integer"}"#),
        required: &[],
    },
    Spec {
        name: "bench_usb_bandwidth",
        tier: Tier::ReadOnly,
        desc: "Which devices share a controller, a root hub or a transaction translator, and how much periodic (isochronous and interrupt) bandwidth they use now and could reserve in their busiest alternate settings: per domain (a TT of a high-speed hub, one per port on a multi-TT hub; a high-speed bus; a SuperSpeed bus) the budget in bytes per ms, active and worst-case load and the devices in it, plus findings such as several full-speed isochronous codecs behind one translator. Also lists the root hubs of each controller (USB 2 and USB 3 companions). The figures are the host's reservation arithmetic, an estimate and not a measurement. `bus` limits to one bus.",
        props: props!(r#""instrument":{"type":"string"},"bus":{"type":"integer"}"#),
        required: &[],
    },
    Spec {
        name: "bench_usb_diagnose",
        tier: Tier::ReadOnly,
        desc: "Explain why a USB device is slow, misbehaving or missing. Give `device` (a path such as 3-2.2.1, or a port label) or `hub` and `port`. It checks the port (powered off, over-current, powered with nothing connected, connected but not enabled, disabled), the device (deauthorized, running below its capability, interfaces without a driver, runtime suspended), the power budget (bMaxPower against the hub's supply), translator bandwidth sharing, and the last 24 h of the kernel journal (enumeration failures, -71 errors, resets, flapping). Findings are ranked problem, warning, note, each with evidence and a next step; `could_not_read` lists what needed a privilege it did not have (the status word uses `sudo -n uhubctl`; `privileged: false` skips it).",
        props: props!(r#""instrument":{"type":"string"},"device":{"type":"string"},"hub":{"type":"string"},"port":{"type":"integer"},"privileged":{"type":"boolean"}"#),
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
    call_with(
        &uhubctl::ProcessRunner,
        Path::new("/sys"),
        Path::new("/proc"),
        insts,
        name,
        args,
    )
}

/// `call` with the program runner and the sysfs and procfs roots given: production passes the process runner, `/sys` and `/proc`.
pub fn call_with(
    run: &dyn uhubctl::Runner,
    root: &Path,
    proc_root: &Path,
    insts: &[Instrument],
    name: &str,
    args: &Value,
) -> Result<Value, String> {
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
    match name {
        "bench_usb_topology" => topology(root, &inst.usb, args),
        "bench_usb_find" => find(root, &inst.usb, args),
        "bench_usb_hubs" => hubs::hubs(run, root, &inst.usb, args),
        "bench_usb_protection" => {
            let t = sysfs::Tree::read(root)?;
            hubs::protection(
                &t,
                &inst.usb,
                &hubs::bench_refs(insts),
                root,
                proc_root,
                args,
            )
        }
        "bench_usb_reset" => ladder::reset(
            run,
            root,
            proc_root,
            &inst.usb,
            &hubs::bench_refs(insts),
            args,
        ),
        "bench_usb_authorize" => ladder::authorize(
            run,
            root,
            proc_root,
            &inst.usb,
            &hubs::bench_refs(insts),
            args,
        ),
        "bench_usb_driver" => ladder::driver(
            run,
            root,
            proc_root,
            &inst.usb,
            &hubs::bench_refs(insts),
            args,
        ),
        "bench_usb_autosuspend" => ladder::autosuspend(
            run,
            root,
            proc_root,
            &inst.usb,
            &hubs::bench_refs(insts),
            args,
        ),
        "bench_usb_history" => {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs_f64())
                .unwrap_or(0.0);
            history::history(run, root, &inst.usb, now, args)
        }
        "bench_usb_bandwidth" => {
            bandwidth::bandwidth(&sysfs::Tree::read(root)?, &inst.usb, root, args)
        }
        "bench_usb_diagnose" => {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs_f64())
                .unwrap_or(0.0);
            diagnose::diagnose(run, root, &inst.usb, now, args)
        }
        "bench_usb_watch" => history::watch(root, args),
        "bench_usb_port_power" => hubs::port_power(
            run,
            root,
            proc_root,
            &inst.usb,
            &hubs::bench_refs(insts),
            inst.tier,
            args,
        ),
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
