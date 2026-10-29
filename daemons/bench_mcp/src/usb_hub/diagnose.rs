// SPDX-License-Identifier: AGPL-3.0-or-later
//! `bench_usb_diagnose`: why a device is slow, misbehaving or missing. It gathers what the other tools read (the sysfs tree, the port's
//! state and status word, the power budget, translator sharing, the kernel history) and turns it into findings ranked problem, warning
//! or note, each with the evidence and a next step. It reports what it can see and says what it could not read.

use super::bandwidth;
use super::history;
use super::sysfs::{child_path, parent_of, speed_finding, Device, Tree};
use super::uhubctl::{self, Runner};
use super::Config;
use serde_json::{json, Value};
use std::path::Path;

/// Whether the device's configured current fits the port and the hub's supply.
pub fn power_budget(t: &Tree, d: &Device) -> Value {
    // [@ANCHOR: usb_hub_power_budget]
    let Some(parent) = d.parent.as_deref().and_then(|p| t.get(p)) else {
        return Value::Null;
    };
    let ss = parent.speed_mbps() >= 5000.0;
    let port_budget: u32 = if ss { 900 } else { 500 };
    let bus_powered_hub = !parent.is_root_hub() && !parent.self_powered;
    let siblings: u32 = t
        .children(&parent.path)
        .iter()
        .map(|c| c.max_power_ma)
        .sum();
    let available = port_budget.saturating_sub(if bus_powered_hub {
        parent.max_power_ma
    } else {
        0
    });
    let over_port = d.max_power_ma > port_budget;
    let over_hub = bus_powered_hub && siblings > available;
    json!({
        "device_ma": d.max_power_ma, "port_budget_ma": port_budget, "parent_hub_bus_powered": bus_powered_hub,
        "parent_hub_left_for_its_ports_ma": if bus_powered_hub { Some(available) } else { None }, "all_devices_on_that_hub_ma": siblings,
        "over_budget": over_port || over_hub,
        "note": "bMaxPower is what the device asked for in its configuration, not what it draws",
    })
}

struct Finding {
    level: &'static str,
    what: String,
    evidence: String,
    next: String,
}

impl Finding {
    fn json(&self) -> Value {
        json!({"level": self.level, "what": self.what, "evidence": self.evidence, "next_step": self.next})
    }
}

fn f(level: &'static str, what: &str, evidence: impl Into<String>, next: &str) -> Finding {
    Finding {
        level,
        what: what.to_string(),
        evidence: evidence.into(),
        next: next.to_string(),
    }
}

fn rank(l: &str) -> u8 {
    match l {
        "problem" => 0,
        "warning" => 1,
        _ => 2,
    }
}

fn driverless_class_matters(class: u8) -> bool {
    matches!(class, 0x01 | 0x02 | 0x03 | 0x08 | 0x0a | 0x0e)
}

/// The port a missing device would be on, from the arguments.
fn missing_target(t: &Tree, cfg: &Config, args: &Value) -> Result<(String, u32), String> {
    if let Some(l) = args.get("device").and_then(|v| v.as_str()) {
        if let Some((hub, port)) = parent_of(l) {
            return Ok((hub, port));
        }
        let by_label = cfg
            .ports
            .iter()
            .find(|(_, p)| p.label.as_ref().is_some_and(|x| x.eq_ignore_ascii_case(l)));
        if let Some((k, _)) = by_label {
            let (h, p) = k.split_once('/').unwrap_or_default();
            return Ok((h.to_string(), p.parse().unwrap_or(0)));
        }
        return Err(format!("{l:?} is neither a device path nor a port label"));
    }
    match (
        args.get("hub").and_then(|v| v.as_str()),
        args.get("port").and_then(|v| v.as_u64()),
    ) {
        (Some(h), Some(p)) if t.get(h).is_some() => Ok((h.to_string(), p as u32)),
        (Some(h), Some(_)) => Err(format!("hub {h} is not in the USB tree")),
        _ => Err("name the device (path or port label), or hub and port".into()),
    }
}

pub fn diagnose(
    run: &dyn Runner,
    sysfs: &Path,
    cfg: &Config,
    now: f64,
    args: &Value,
) -> Result<Value, String> {
    // [@ANCHOR: usb_hub_diagnose]
    let t = Tree::read(sysfs)?;
    let privileged = args
        .get("privileged")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    let mut findings: Vec<Finding> = Vec::new();
    let mut unread: Vec<String> = Vec::new();
    let requested = args.get("device").and_then(|v| v.as_str());
    let present = requested.and_then(|d| super::resolve_device(&t, cfg, d).ok());
    let (hub_path, port_no) = match &present {
        Some(p) => parent_of(p).ok_or("a root hub has no port to diagnose")?,
        None => missing_target(&t, cfg, args)?,
    };
    let dev_path = child_path(&hub_path, port_no);
    let hub = t.get(&hub_path);
    let label = cfg.label(&hub_path, port_no);
    let u = if privileged {
        match uhubctl::status(run) {
            Ok(v) => Some(v),
            Err(e) => {
                unread.push(format!("uhubctl status: {e}"));
                None
            }
        }
    } else {
        None
    };
    let loc = hub_path
        .strip_prefix("usb")
        .unwrap_or(&hub_path)
        .to_string();
    let uport = u
        .as_ref()
        .and_then(|v| v.iter().find(|h| h.location == loc))
        .and_then(|h| h.port_list.iter().find(|p| p.number == port_no))
        .cloned();
    let sport = hub.and_then(|h| h.ports.iter().find(|p| p.number == port_no));
    let history_events = match history::journal(run, Some(now - 86400.0), false, false) {
        Ok(text) => Some(history::parse(&text)),
        Err(e) => {
            unread.push(format!("kernel journal: {e}"));
            None
        }
    };
    let hist = history_events.as_ref().map(|ev| {
        history::analyze(
            ev,
            &history::Query {
                device: Some(dev_path.clone()),
                since: Some(now - 86400.0),
                limit: 0,
                flap_window_s: 120.0,
                flap_connects: 3,
            },
        )
    });

    let Some(hub) = hub else {
        findings.push(f(
            "problem",
            &format!("the hub {hub_path} that this port belongs to is not in the USB tree"),
            format!("{hub_path} is absent from /sys/bus/usb/devices"),
            "diagnose the hub's own uplink first: bench_usb_diagnose with device set to the hub",
        ));
        return Ok(finish(&dev_path, label, false, findings, unread, hist));
    };
    if port_no > hub.maxchild {
        return Err(format!(
            "{hub_path} has {} ports; {port_no} is not one of them",
            hub.maxchild
        ));
    }
    if !hub.authorized {
        findings.push(f(
            "problem",
            &format!("the hub {hub_path} is deauthorized"),
            "authorized is 0",
            "bench_usb_authorize with authorized true",
        ));
    }

    match t.get(&dev_path) {
        None => missing(&mut findings, hub, sport, uport.as_ref(), hist.as_ref()),
        Some(d) => present_checks(
            &mut findings,
            &t,
            cfg,
            sysfs,
            d,
            sport,
            uport.as_ref(),
            hist.as_ref(),
        ),
    }
    Ok(finish(
        &dev_path,
        label,
        t.get(&dev_path).is_some(),
        findings,
        unread,
        hist,
    ))
}

fn finish(
    dev: &str,
    label: Option<String>,
    present: bool,
    mut findings: Vec<Finding>,
    unread: Vec<String>,
    hist: Option<Value>,
) -> Value {
    findings.sort_by_key(|x| rank(x.level));
    let worst = findings.first().map(|x| x.level);
    let verdict = match (present, worst) {
        (_, Some("problem")) => "a problem was found",
        (_, Some("warning")) => "no outright fault, but something looks wrong",
        (true, _) => "nothing wrong was found in what can be read from software",
        (false, _) => {
            "the device is not on the bus and no cause was found in what can be read from software"
        }
    };
    json!({
        "device": dev, "port_label": label, "present_in_the_tree": present, "verdict": verdict,
        "findings": findings.iter().map(|x| x.json()).collect::<Vec<_>>(),
        "could_not_read": unread,
        "history_24h": hist.map(|h| json!({"totals": h["totals"], "per_device": h["per_device"], "flapping": h["flapping"]})),
    })
}

fn missing(
    out: &mut Vec<Finding>,
    hub: &Device,
    sport: Option<&super::sysfs::Port>,
    up: Option<&uhubctl::UPort>,
    hist: Option<&Value>,
) {
    if let Some(p) = up {
        let b = &p.bits;
        if !b.powered {
            out.push(f("problem", "the port is powered off", format!("status word {:04x}: power bit clear", b.word), "bench_usb_port_power with action on (a tool or a person switched it off, or a hub reset left it off)"));
        }
        if b.over_current {
            out.push(f("problem", "the hub reports over-current on this port", format!("status word {:04x}: over-current bit set", b.word), "unplug what is on the port; a short or an overloaded device; the hub removes power until the condition clears"));
        }
        if b.powered && !b.connected {
            out.push(f("warning", "the port is powered but nothing is connected", format!("status word {:04x}: power set, connect clear", b.word), "check the cable and that the device has power (a bus-powered device on a bus-powered hub may be starved); try another port"));
        }
        if b.connected && !b.enabled {
            out.push(f("problem", "a device is connected but the port is not enabled", format!("status word {:04x}: connect set, enable clear", b.word), "enumeration failed: see the history below; bench_usb_port_power cycle, a shorter cable, a powered hub"));
        }
    }
    if let Some(p) = sport {
        if p.over_current_count > 0 {
            out.push(f(
                "warning",
                "this port has seen over-current events",
                format!("over_current_count is {}", p.over_current_count),
                "something drew too much current on this port since boot",
            ));
        }
        if p.disabled {
            out.push(f("problem", "the port is disabled by software", "the port's disable file is 1", "re-enable it by writing 0 to the port's disable attribute as root, or cycle the port"));
        }
        if up.is_none() {
            out.push(f("note", &format!("the port state in sysfs is {:?}", p.state), "uhubctl status was not available, so the status word could not be read", "run with sudo -n uhubctl available (privileged true) for the power, connect and over-current bits"));
        }
    }
    if let Some(h) = hist {
        let d = h["per_device"].as_array().and_then(|a| a.first());
        if let Some(d) = d {
            let n = |k: &str| d[k].as_u64().unwrap_or(0);
            if n("enumeration_failures") + n("descriptor_read_errors") + n("address_errors") > 0 {
                out.push(f("problem", "the kernel tried to enumerate a device here and failed", format!("{} enumeration failures, {} descriptor read errors, {} address errors in 24 h", n("enumeration_failures"), n("descriptor_read_errors"), n("address_errors")), "a device that fails to enumerate is usually a bad cable, an underpowered port, or the device in a wedged state: bench_usb_port_power cycle with a longer off_time_s"));
            }
        }
    }
    if hub.self_powered {
        return;
    }
    out.push(f(
        "note",
        &format!("the hub {} is bus powered", hub.path),
        "bmAttributes says it is not self powered",
        "devices that draw much current may not start on a bus-powered hub; use a powered hub",
    ));
}

#[allow(clippy::too_many_arguments)]
fn present_checks(
    out: &mut Vec<Finding>,
    t: &Tree,
    cfg: &Config,
    sysfs: &Path,
    d: &Device,
    sport: Option<&super::sysfs::Port>,
    up: Option<&uhubctl::UPort>,
    hist: Option<&Value>,
) {
    let parent = d.parent.as_deref().and_then(|p| t.get(p));
    if let Some(s) = speed_finding(d, parent) {
        let level = if s["level"] == "warn" {
            "warning"
        } else {
            "note"
        };
        out.push(f(level, "the device is running slower than its descriptor suggests", s["text"].as_str().unwrap_or_default(), "try a USB 3 port and cable directly on the host; for a USB 2.x report at 12 Mb/s it may simply be a full-speed device"));
    }
    if !d.authorized {
        out.push(f(
            "problem",
            "the device is deauthorized",
            "authorized is 0, so no driver binds",
            "bench_usb_authorize with authorized true",
        ));
    }
    if !d.is_hub() {
        for i in d.interfaces.iter().filter(|i| i.driver.is_none()) {
            let lvl = if driverless_class_matters(i.class) {
                "warning"
            } else {
                "note"
            };
            out.push(f(lvl, &format!("interface {} has no driver bound", i.name), format!("class {:02x} subclass {:02x}", i.class, i.subclass), if i.class == 0xff { "vendor-specific interfaces are often used from user space (libusb); otherwise the driver module is not loaded" } else { "the kernel module for this class is missing or blocked, or the interface was unbound by bench_usb_driver: modprobe it or bench_usb_driver bind" }));
        }
    }
    if d.power_control.as_deref() == Some("auto")
        && d.runtime_status.as_deref() == Some("suspended")
    {
        out.push(f(
            "note",
            "the device is runtime suspended",
            "power/control is auto and runtime_status is suspended",
            "a device that misbehaves after idle: bench_usb_autosuspend with control on",
        ));
    }
    let budget = power_budget(t, d);
    if budget["over_budget"] == true {
        out.push(f(
            "warning",
            "the devices on this hub ask for more current than it can supply",
            format!("{budget}"),
            "use a self-powered hub, or move a device to another port",
        ));
    }
    if let Some(p) = sport {
        if p.over_current_count > 0 {
            out.push(f(
                "warning",
                "this port has seen over-current events",
                format!("over_current_count is {}", p.over_current_count),
                "something drew too much current on this port since boot",
            ));
        }
    }
    if let Some(p) = up {
        if p.bits.suspended == Some(true) {
            out.push(f(
                "note",
                "the port is suspended",
                format!("status word {:04x}", p.bits.word),
                "normal for an idle autosuspended device",
            ));
        }
    }
    let dom = bandwidth::domain_of(t, d);
    if let Ok(b) = bandwidth::bandwidth(t, cfg, sysfs, &json!({"bus": d.bus})) {
        let key = if dom.port > 0 {
            format!("{} port {} of {}", dom.kind, dom.port, dom.hub)
        } else {
            format!("{} of {}", dom.kind, dom.hub)
        };
        if let Some(row) = b["domains"]
            .as_array()
            .and_then(|a| a.iter().find(|r| r["domain"] == key.as_str()))
        {
            let wc = row["worst_case_percent"].as_f64().unwrap_or(0.0);
            let n = row["devices"].as_array().map(|a| a.len()).unwrap_or(0);
            if wc > 100.0 {
                out.push(f(
                    "warning",
                    "the devices behind this translator cannot all stream at once",
                    format!("{key}: worst case {wc} % of the periodic budget, {n} devices"),
                    "move a device to another port of a multi-TT hub, or to another root hub",
                ));
            } else if wc > 50.0 && n > 2 {
                out.push(f("note", "this device shares a busy translator", format!("{key}: worst case {wc} % of the periodic budget, {n} devices"), "if streams drop out, check bench_usb_bandwidth; moving a device off the shared translator removes the contention"));
            }
        }
    }
    if let Some(h) = hist {
        if let Some(row) = h["per_device"]
            .as_array()
            .and_then(|a| a.iter().find(|r| r["device"] == d.path.as_str()))
        {
            let n = |k: &str| row[k].as_u64().unwrap_or(0);
            let e71 = row["errno_counts"]["-71"].as_u64().unwrap_or(0);
            if e71 > 0 || n("enumeration_failures") > 0 {
                out.push(f(
                    "warning",
                    "the kernel logged link errors for this device in the last 24 h",
                    format!(
                        "{e71} errors -71, {} enumeration failures, {} descriptor read errors",
                        n("enumeration_failures"),
                        n("descriptor_read_errors")
                    ),
                    "a marginal cable, port or hub; see bench_usb_history for when",
                ));
            }
            if n("resets") > 2 {
                out.push(f(
                    "warning",
                    "the device was reset repeatedly",
                    format!("{} resets in 24 h", n("resets")),
                    "something keeps resetting it: the driver, a tool, or a link fault",
                ));
            }
        }
        if let Some(fl) = h["flapping"]
            .as_array()
            .and_then(|a| a.iter().find(|r| r["device"] == d.path.as_str()))
        {
            out.push(f(
                "warning",
                "the device has connected several times in a short window",
                format!(
                    "{} connects within {} s",
                    fl["connects_in_window"], fl["window_s"]
                ),
                "if nobody power cycled it, suspect the cable, power or the device",
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::usb_hub::testkit::{recorded, writable_sys, Behaviour, FakeHub};

    struct Quiet(FakeHub);
    impl Runner for Quiet {
        fn run(&self, p: &str, a: &[String]) -> Result<(bool, String), String> {
            if p == "journalctl" {
                return Ok((
                    true,
                    std::fs::read_to_string(recorded().join("kernel_journal.txt")).unwrap(),
                ));
            }
            self.0.run(p, a)
        }
    }

    fn cfg() -> Config {
        crate::usb_hub::parse_config(&json!({"ports": {"3-2/4": {"label": "spare"}}})).unwrap()
    }

    fn levels(v: &Value) -> Vec<(String, String)> {
        v["findings"]
            .as_array()
            .unwrap()
            .iter()
            .map(|x| {
                (
                    x["level"].as_str().unwrap().to_string(),
                    x["what"].as_str().unwrap().to_string(),
                )
            })
            .collect()
    }

    // Tests [@ANCHOR: usb_hub_diagnose]
    #[test]
    fn an_empty_powered_port_is_a_powered_port_with_nothing_connected() {
        let (_d, sys) = writable_sys();
        let run = Quiet(FakeHub::new(&sys, Behaviour::Switches));
        let v = diagnose(
            &run,
            &sys,
            &cfg(),
            1793290000.0,
            &json!({"device": "spare"}),
        )
        .unwrap();
        assert_eq!(v["present_in_the_tree"], false);
        assert!(
            levels(&v)
                .iter()
                .any(|(l, w)| l == "warning" && w.contains("powered but nothing is connected")),
            "{v}"
        );
    }

    #[test]
    fn a_missing_device_on_a_port_the_kernel_failed_to_enumerate_is_reported_from_the_journal() {
        let (_d, sys) = writable_sys();
        let run = Quiet(FakeHub::new(&sys, Behaviour::Switches));
        let dev = "3-2.2.1";
        let hub = std::fs::canonicalize(sys.join("bus/usb/devices").join(dev)).unwrap();
        std::fs::remove_dir_all(&hub).unwrap();
        std::fs::remove_file(sys.join("bus/usb/devices").join(dev)).unwrap();
        let v = diagnose(&run, &sys, &cfg(), 1793290000.0, &json!({"device": dev})).unwrap();
        assert_eq!(v["present_in_the_tree"], false);
        assert!(
            levels(&v)
                .iter()
                .any(|(l, w)| l == "problem" && w.contains("failed")),
            "{:?}",
            levels(&v)
        );
        assert_eq!(v["verdict"], "a problem was found");
    }

    #[test]
    fn a_powered_off_port_and_a_deauthorized_device_are_found() {
        let (_d, sys) = writable_sys();
        struct Off(FakeHub);
        impl Runner for Off {
            fn run(&self, p: &str, a: &[String]) -> Result<(bool, String), String> {
                if p == "journalctl" {
                    return Err("none".into());
                }
                let t = std::fs::read_to_string(recorded().join("uhubctl_nodesc.txt"))
                    .unwrap()
                    .replace("Port 4: 0100 power", "Port 4: 0000 off");
                let _ = (&self.0, a);
                Ok((true, t))
            }
        }
        let run = Off(FakeHub::new(&sys, Behaviour::Switches));
        let v = diagnose(&run, &sys, &cfg(), 0.0, &json!({"device": "spare"})).unwrap();
        assert!(
            levels(&v)
                .iter()
                .any(|(l, w)| l == "problem" && w.contains("powered off")),
            "{v}"
        );
        assert!(v["could_not_read"][0].as_str().unwrap().contains("journal"));
        let dev = std::fs::canonicalize(sys.join("bus/usb/devices/3-2.2.3")).unwrap();
        std::fs::write(dev.join("authorized"), "0\n").unwrap();
        let v = diagnose(
            &run,
            &sys,
            &cfg(),
            0.0,
            &json!({"device": "3-2.2.3", "privileged": false}),
        )
        .unwrap();
        assert!(
            levels(&v)
                .iter()
                .any(|(l, w)| l == "problem" && w.contains("deauthorized")),
            "{v}"
        );
    }

    #[test]
    fn the_recorded_vnwa_chain_shows_the_shared_translator_and_its_history() {
        let (_d, sys) = writable_sys();
        let run = Quiet(FakeHub::new(&sys, Behaviour::Switches));
        let v = diagnose(
            &run,
            &sys,
            &cfg(),
            1793290000.0,
            &json!({"device": "3-2.2.2"}),
        )
        .unwrap();
        assert_eq!(v["present_in_the_tree"], true);
        let l = levels(&v);
        assert!(
            l.iter()
                .any(|(_, w)| w.contains("shares a busy translator")
                    || w.contains("cannot all stream")),
            "{l:?}"
        );
        assert!(
            l.iter()
                .any(|(_, w)| w.contains("full-speed") || w.contains("slower")),
            "{l:?}"
        );
        assert!(v["history_24h"]["totals"]["connect"].as_u64().unwrap() > 0);
    }

    // Tests [@ANCHOR: usb_hub_power_budget]
    #[test]
    fn the_power_budget_flags_a_bus_powered_hub_asked_for_more_than_it_has() {
        let t = Tree::read(&crate::usb_hub::testkit::recorded_sys()).unwrap();
        let b = power_budget(&t, t.get("3-2.2.1").unwrap());
        assert_eq!(b["port_budget_ma"], 500);
        assert!(b["all_devices_on_that_hub_ma"].as_u64().unwrap() > 0);
        let mut over = t.clone();
        for c in ["3-2.2.1", "3-2.2.2", "3-2.2.3"] {
            over.devices.get_mut(c).unwrap().max_power_ma = 300;
        }
        over.devices.get_mut("3-2.2").unwrap().self_powered = false;
        over.devices.get_mut("3-2.2").unwrap().max_power_ma = 100;
        let b = power_budget(&over, over.get("3-2.2.1").unwrap());
        assert_eq!(
            (
                b["over_budget"].clone(),
                b["parent_hub_left_for_its_ports_ma"].clone()
            ),
            (json!(true), json!(400))
        );
        assert!(power_budget(&t, t.get("usb3").unwrap()).is_null());
    }

    #[test]
    fn bad_targets_are_errors() {
        let (_d, sys) = writable_sys();
        let run = Quiet(FakeHub::new(&sys, Behaviour::Switches));
        assert!(diagnose(&run, &sys, &cfg(), 0.0, &json!({})).is_err());
        assert!(diagnose(&run, &sys, &cfg(), 0.0, &json!({"device": "nonsense"})).is_err());
        assert!(diagnose(&run, &sys, &cfg(), 0.0, &json!({"hub": "3-2", "port": 9})).is_err());
        assert!(diagnose(&run, &sys, &cfg(), 0.0, &json!({"device": "usb3"})).is_err());
    }
}
