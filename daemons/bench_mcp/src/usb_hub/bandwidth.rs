// SPDX-License-Identifier: AGPL-3.0-or-later
//! Bus bandwidth sharing: which devices share a controller, a root hub or a transaction translator, and how much periodic
//! (isochronous and interrupt) bandwidth they use now and could use. Full- and low-speed devices behind a high-speed hub share that
//! hub's translator (one per port on a multi-TT hub, one for the whole hub on a single-TT hub) and its 1 ms frame; that is where
//! several full-speed audio codecs on one busy bus run out of room.
//!
//! The figures are the host's reservation arithmetic (packet size plus protocol overhead per interval), the same way the kernel counts
//! periodic transfers; they are an estimate, not a measurement of what the devices send.

use super::sysfs::{Device, Tree};
use super::Config;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::Path;

/// Full-speed frame: 12 Mb/s is 1500 bytes per 1 ms frame, of which periodic transfers may take 90 %.
const FS_PERIODIC_BYTES_PER_MS: f64 = 1350.0;
/// High-speed: 7500 bytes per 125 us microframe, 80 % periodic, 8 microframes a millisecond.
const HS_PERIODIC_BYTES_PER_MS: f64 = 48000.0;

/// Protocol overhead per packet in bytes on the wire.
fn overhead(speed: f64, iso: bool) -> f64 {
    match (speed >= 480.0, iso) {
        (true, true) => 38.0,
        (true, false) => 55.0,
        (false, true) => 9.0,
        (false, false) => 13.0,
    }
}

/// Bytes per millisecond a periodic endpoint reserves. `interval_us` is the polling interval; `mps` the raw wMaxPacketSize
/// (bits 11 and 12 add transactions per microframe at high speed).
pub fn endpoint_load(speed: f64, iso: bool, mps: u32, interval_us: u32) -> Option<f64> {
    if speed >= 5000.0 || interval_us == 0 {
        return None;
    }
    let size = f64::from(mps & 0x7ff);
    let mult = if speed >= 480.0 {
        f64::from(1 + ((mps >> 11) & 3))
    } else {
        1.0
    };
    let per_ms = 1000.0 / f64::from(interval_us);
    Some((size + overhead(speed, iso)) * mult * per_ms)
}

/// The polling interval in us from the endpoint descriptor's bInterval.
fn interval_us(speed: f64, iso: bool, b: u8) -> u32 {
    let exp = |b: u8| 1u32 << (b.clamp(1, 16) - 1);
    if speed >= 480.0 {
        125 * exp(b)
    } else if iso {
        1000 * exp(b)
    } else {
        1000 * u32::from(b.max(1))
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Stream {
    pub interface: u32,
    pub alt: u32,
    pub address: u8,
    pub kind: &'static str,
    pub bytes_per_ms: f64,
}

/// Every periodic endpoint of every alternate setting in the device's configuration descriptors.
pub fn potential_streams(sysfs: &Path, d: &Device) -> Vec<Stream> {
    let bytes = std::fs::read(
        sysfs
            .join("bus/usb/devices")
            .join(&d.path)
            .join("descriptors"),
    )
    .unwrap_or_default();
    let speed = d.speed_mbps();
    let (mut i, mut ifnum, mut alt) = (0usize, 0u32, 0u32);
    let mut out = Vec::new();
    while i + 2 <= bytes.len() {
        let len = usize::from(bytes[i]);
        if len < 2 || i + len > bytes.len() {
            break;
        }
        match bytes[i + 1] {
            4 if len >= 9 => {
                ifnum = u32::from(bytes[i + 2]);
                alt = u32::from(bytes[i + 3]);
            }
            5 if len >= 7 => {
                let attr = bytes[i + 3] & 3;
                let mps = u32::from(u16::from_le_bytes([bytes[i + 4], bytes[i + 5]]));
                if attr == 1 || attr == 3 {
                    let iso = attr == 1;
                    if let Some(l) =
                        endpoint_load(speed, iso, mps, interval_us(speed, iso, bytes[i + 6]))
                    {
                        out.push(Stream {
                            interface: ifnum,
                            alt,
                            address: bytes[i + 2],
                            kind: if iso { "isochronous" } else { "interrupt" },
                            bytes_per_ms: l,
                        });
                    }
                }
            }
            _ => {}
        }
        i += len;
    }
    out
}

/// The load of the endpoints in use now (the active alternate setting of each interface).
pub fn active_load(d: &Device) -> f64 {
    let speed = d.speed_mbps();
    d.interfaces
        .iter()
        .flat_map(|i| &i.endpoints)
        .filter(|e| e.kind == "Isochronous" || e.kind == "Interrupt")
        .filter_map(|e| endpoint_load(speed, e.kind == "Isochronous", e.max_packet, e.interval_us))
        .fold(0.0, |a, b| a + b)
}

/// The worst case: for each interface the alternate setting with the largest periodic load, summed.
pub fn potential_load(streams: &[Stream]) -> f64 {
    let mut worst: BTreeMap<u32, BTreeMap<u32, f64>> = BTreeMap::new();
    for s in streams {
        *worst
            .entry(s.interface)
            .or_default()
            .entry(s.alt)
            .or_default() += s.bytes_per_ms;
    }
    worst
        .values()
        .map(|alts| alts.values().cloned().fold(0.0, f64::max))
        .fold(0.0, |a, b| a + b)
}

/// The domain a device's periodic traffic is counted in: its translator for a full- or low-speed device below a high-speed hub,
/// otherwise its bus at its own speed.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Domain {
    pub bus: u32,
    pub kind: &'static str,
    pub hub: String,
    pub port: u32,
}

pub fn domain_of(t: &Tree, d: &Device) -> Domain {
    let speed = d.speed_mbps();
    if speed >= 5000.0 {
        return Domain {
            bus: d.bus,
            kind: "SuperSpeed bus",
            hub: format!("usb{}", d.bus),
            port: 0,
        };
    }
    if speed >= 480.0 {
        return Domain {
            bus: d.bus,
            kind: "high-speed bus",
            hub: format!("usb{}", d.bus),
            port: 0,
        };
    }
    // Full or low speed: walk up to the nearest high-speed hub; the translator is on that hub.
    let mut child = d;
    while let Some(parent) = child.parent.as_deref().and_then(|p| t.get(p)) {
        if parent.speed_mbps() >= 480.0 && !parent.is_root_hub() {
            let multi = parent.tt_mode().starts_with("multi-TT")
                && parent.hub_interface_protocol != Some(1);
            return Domain {
                bus: d.bus,
                kind: if multi {
                    "TT (one per port)"
                } else {
                    "TT (single, shared by all ports)"
                },
                hub: parent.path.clone(),
                port: if multi { child.port.unwrap_or(0) } else { 0 },
            };
        }
        if parent.is_root_hub() {
            return Domain {
                bus: d.bus,
                kind: "full-speed root hub",
                hub: parent.path.clone(),
                port: 0,
            };
        }
        child = parent;
    }
    Domain {
        bus: d.bus,
        kind: "full-speed root hub",
        hub: format!("usb{}", d.bus),
        port: 0,
    }
}

fn capacity(kind: &str) -> Option<f64> {
    if kind.starts_with("TT") || kind.starts_with("full-speed") {
        Some(FS_PERIODIC_BYTES_PER_MS)
    } else if kind.starts_with("high-speed") {
        Some(HS_PERIODIC_BYTES_PER_MS)
    } else {
        None
    }
}

fn pct(x: f64, cap: Option<f64>) -> Value {
    cap.map(|c| json!((1000.0 * x / c).round() / 10.0))
        .unwrap_or(Value::Null)
}

/// `bench_usb_bandwidth`.
pub fn bandwidth(t: &Tree, cfg: &Config, sysfs: &Path, args: &Value) -> Result<Value, String> {
    // [@ANCHOR: usb_hub_bandwidth]
    let only_bus = args.get("bus").and_then(|v| v.as_u64());
    let mut domains: BTreeMap<Domain, Vec<&Device>> = BTreeMap::new();
    for d in t
        .sorted()
        .into_iter()
        .filter(|d| !d.is_root_hub() && only_bus.is_none_or(|b| u64::from(d.bus) == b))
    {
        domains.entry(domain_of(t, d)).or_default().push(d);
    }
    if domains.is_empty() {
        return Err("no devices on that bus".into());
    }
    let mut out = Vec::new();
    let mut findings = Vec::new();
    for (dom, devs) in &domains {
        let cap = capacity(dom.kind);
        let mut rows = Vec::new();
        let (mut act, mut pot) = (0.0, 0.0);
        let mut iso_devices = 0;
        for d in devs {
            let streams = potential_streams(sysfs, d);
            let (a, p) = (active_load(d), potential_load(&streams));
            if streams.iter().any(|s| s.kind == "isochronous") {
                iso_devices += 1;
            }
            act += a;
            pot += p;
            let label = d
                .parent
                .as_ref()
                .zip(d.port)
                .and_then(|(h, p)| cfg.label(h, p));
            rows.push(json!({
                "device": d.path, "label": label, "vidpid": d.vidpid(), "product": d.product, "speed_mbps": d.speed,
                "active_bytes_per_ms": a.round(), "worst_case_bytes_per_ms": p.round(),
                "periodic_endpoints_in_descriptors": streams.len(),
            }));
        }
        let id = if dom.port > 0 {
            format!("{} port {} of {}", dom.kind, dom.port, dom.hub)
        } else {
            format!("{} of {}", dom.kind, dom.hub)
        };
        if let Some(c) = cap {
            if pot > c {
                findings.push(format!("{id}: {} device(s) could reserve {:.0} bytes/ms in their busiest alternate settings, over the {:.0} bytes/ms periodic budget ({:.0} %): they cannot all stream at once", devs.len(), pot, c, 100.0 * pot / c));
            } else if iso_devices >= 2 && pot > 0.5 * c {
                findings.push(format!("{id}: {iso_devices} devices with isochronous endpoints share it and could reserve {:.0} % of the periodic budget", 100.0 * pot / c));
            }
            if act > 0.9 * c {
                findings.push(format!(
                    "{id}: {:.0} % of the periodic budget is in use now",
                    100.0 * act / c
                ));
            }
        }
        out.push(json!({
            "domain": id, "bus": dom.bus, "kind": dom.kind, "hub": dom.hub, "port": if dom.port > 0 { Some(dom.port) } else { None },
            "periodic_budget_bytes_per_ms": cap, "active_bytes_per_ms": act.round(), "active_percent": pct(act, cap),
            "worst_case_bytes_per_ms": pot.round(), "worst_case_percent": pct(pot, cap), "devices": rows,
        }));
    }
    let buses: Vec<Value> = t
        .roots()
        .iter()
        .filter(|r| only_bus.is_none_or(|b| u64::from(r.bus) == b))
        .map(|r| {
            let companions: Vec<String> = t.roots().iter().filter(|o| o.bus != r.bus && o.controller == r.controller && r.controller.is_some()).map(|o| o.path.clone()).collect();
            json!({
                "bus": r.bus, "root_hub": r.path, "speed": r.speed_name(), "controller": r.controller.as_ref().map(|c| json!({"pci": c.pci, "driver": c.driver})),
                "ports": r.maxchild, "devices": t.subtree(&r.path).len() - 1, "companion_root_hubs_on_the_same_controller": companions,
            })
        })
        .collect();
    Ok(json!({
        "buses": buses, "domains": out, "findings": findings,
        "method": "reservation arithmetic per periodic endpoint (packet size plus protocol overhead per polling interval); 'active' counts the endpoints of the current alternate settings, 'worst case' the busiest alternate setting of each interface from the descriptors. Full- and low-speed devices count against the translator of the high-speed hub they hang from (one per port on a multi-TT hub). SuperSpeed devices have their own links and are not counted here. A USB 3 port has a USB 2 companion on the same connector: a SuperSpeed device uses the SS bus, anything slower on the same connector uses the companion root hub.",
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::usb_hub::testkit::recorded_sys;

    #[test]
    fn endpoint_loads_follow_the_usb_arithmetic() {
        // A full-speed isochronous endpoint of 192 bytes every 1 ms: (192 + 9) * 1.
        assert_eq!(endpoint_load(12.0, true, 192, 1000), Some(201.0));
        // A full-speed interrupt endpoint of 8 bytes every 10 ms.
        assert_eq!(endpoint_load(12.0, false, 8, 10000), Some(2.1));
        // A high-speed isochronous endpoint with 2 extra transactions: 3 x (1024 + 38) every 125 us.
        assert_eq!(
            endpoint_load(480.0, true, 1024 | (2 << 11), 125),
            Some(3.0 * 1062.0 * 8.0)
        );
        assert_eq!(endpoint_load(5000.0, true, 1024, 125), None);
        assert_eq!(interval_us(12.0, true, 1), 1000);
        assert_eq!(interval_us(480.0, false, 4), 1000);
        assert_eq!(interval_us(12.0, false, 10), 10000);
    }

    // Tests [@ANCHOR: usb_hub_bandwidth]
    #[test]
    fn the_three_full_speed_devices_of_the_vnwa_share_one_translator_and_the_codecs_can_reserve_most_of_it(
    ) {
        let t = Tree::read(&recorded_sys()).unwrap();
        let v = bandwidth(
            &t,
            &Config::default(),
            &recorded_sys(),
            &serde_json::json!({"bus": 3}),
        )
        .unwrap();
        let tt = v["domains"]
            .as_array()
            .unwrap()
            .iter()
            .find(|d| d["hub"] == "3-2")
            .unwrap();
        assert_eq!(
            (tt["kind"].as_str(), tt["port"].as_u64()),
            (Some("TT (one per port)"), Some(2))
        );
        let devs: Vec<_> = tt["devices"]
            .as_array()
            .unwrap()
            .iter()
            .map(|d| d["device"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(devs, ["3-2.2", "3-2.2.1", "3-2.2.2", "3-2.2.3"]);
        let codec = tt["devices"]
            .as_array()
            .unwrap()
            .iter()
            .find(|d| d["device"] == "3-2.2.2")
            .unwrap();
        assert!(
            codec["worst_case_bytes_per_ms"].as_f64().unwrap() > 300.0,
            "{codec}"
        );
        assert!(
            tt["worst_case_bytes_per_ms"].as_f64().unwrap()
                > codec["worst_case_bytes_per_ms"].as_f64().unwrap() * 1.9
        );
        let f: Vec<_> = v["findings"]
            .as_array()
            .unwrap()
            .iter()
            .map(|x| x.as_str().unwrap().to_string())
            .collect();
        assert!(
            f.iter()
                .any(|x| x.contains("isochronous") || x.contains("over the")),
            "{f:?}"
        );
        assert!(v["buses"][0]["companion_root_hubs_on_the_same_controller"]
            .as_array()
            .unwrap()
            .iter()
            .any(|x| x == "usb4"));
    }

    #[test]
    fn the_domain_of_a_device_is_its_translator_or_its_bus() {
        let t = Tree::read(&recorded_sys()).unwrap();
        let d = |p: &str| domain_of(&t, t.get(p).unwrap());
        assert_eq!((d("3-2.2.2").hub.as_str(), d("3-2.2.2").port), ("3-2", 2));
        assert_eq!(
            d("3-2.2").port,
            2,
            "the full-speed hub itself is behind the same translator"
        );
        assert_eq!(d("3-2").kind, "high-speed bus");
        assert_eq!(d("3-9").kind, "full-speed root hub");
        assert_eq!(d("2-3").kind, "SuperSpeed bus");
    }

    #[test]
    fn descriptors_give_every_alternate_settings_periodic_endpoints() {
        let t = Tree::read(&recorded_sys()).unwrap();
        let s = potential_streams(&recorded_sys(), t.get("3-2.2.2").unwrap());
        assert!(s.iter().any(|x| x.kind == "isochronous"), "{s:?}");
        assert!(s.iter().any(|x| x.kind == "interrupt"));
        assert!(potential_load(&s) > 0.0);
        assert!(potential_streams(&recorded_sys(), t.get("3-9").unwrap()).len() <= 4);
    }
}
