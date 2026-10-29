// SPDX-License-Identifier: AGPL-3.0-or-later
//! Hub capabilities and per-port status (`bench_usb_hubs`), the protection report, and port power control (`bench_usb_port_power`).

use super::power::{self, Opts, PortRef};
use super::protect::{self, BenchSerial};
use super::sysfs::{Device, Tree};
use super::uhubctl::{self, Runner, Switching, UHub};
use super::Config;
use serde_json::{json, Value};
use std::path::Path;
use std::time::Duration;

/// What a hub's port power switching has been shown to do. The levels build on each other; a port cannot be verified from
/// software alone, so the output says which level each statement is.
pub fn verification(cfg: &Config, key: &str, claimed: bool) -> Value {
    // [@ANCHOR: usb_hub_verification_levels]
    let c = cfg.ports.get(key);
    let mut level = 0;
    let mut evidence: Vec<String> = Vec::new();
    if claimed {
        level = 1;
        evidence
            .push("1 claimed: uhubctl lists the hub as switching this port individually".into());
    }
    if let Some(e) = power::proven(key) {
        level = level.max(2);
        evidence.push(format!(
            "2 re-enumerated by this server: the device left the tree {:.1} s after power-off and returned {:.1} s after power-on, device number {} to {}",
            e.gone_after_s, e.back_after_s, e.old_devnum, e.new_devnum
        ));
    }
    if let Some(t) = c.and_then(|c| c.confirmed_by_hand.as_ref()) {
        level = level.max(2);
        evidence.push(format!(
            "2 plus manually confirmed (by eye, recorded in the config): {t}"
        ));
    }
    if let Some(t) = c.and_then(|c| c.state_cleared.as_ref()) {
        level = level.max(3);
        evidence.push(format!("3 state cleared (recorded in the config): {t}"));
    }
    if let Some(t) = c.and_then(|c| c.vbus_measured.as_ref()) {
        level = level.max(4);
        evidence.push(format!("4 VBUS measured (manual entry in the config): {t}"));
    }
    let name = [
        "not switchable by uhubctl",
        "claimed",
        "re-enumerated",
        "state cleared",
        "VBUS measured",
    ][level];
    json!({
        "level": level, "name": name, "evidence": evidence,
        "note": "level 2 shows the port dropped the device, not that VBUS fell (some hubs disable the data lines and leave VBUS on); only level 4 is a measurement of VBUS",
    })
}

fn hub_json(
    t: &Tree,
    cfg: &Config,
    d: &Device,
    u: Option<&UHub>,
    desc: Option<&Result<uhubctl::HubDescriptor, String>>,
    ur: &Result<Vec<UHub>, String>,
) -> Value {
    let loc = d.path.strip_prefix("usb").unwrap_or(&d.path);
    let usb3 = d.speed_mbps() >= 5000.0;
    let switching = match (u, ur) {
        (Some(h), _) => h.switching.name(),
        (None, Ok(_)) => "not listed by uhubctl (it does not treat this hub as switchable)".into(),
        (None, Err(_)) => "unknown (uhubctl could not be run)".into(),
    };
    let claimed = u.is_some_and(|h| h.switching == Switching::PerPort);
    let ports: Vec<Value> = (1..=d.maxchild)
        .map(|n| {
            let sp = d.ports.iter().find(|p| p.number == n);
            let up = u.and_then(|h| h.port_list.iter().find(|p| p.number == n));
            let key = format!("{}/{}", d.path, n);
            json!({
                "port": n, "label": cfg.label(&d.path, n), "device": sp.and_then(|p| p.child.clone()),
                "sysfs_state": sp.map(|p| p.state.clone()), "connect_type": sp.map(|p| p.connect_type.clone()),
                "over_current_count": sp.map(|p| p.over_current_count), "disabled_by_software": sp.map(|p| p.disabled), "peer_port": sp.and_then(|p| p.peer.clone()),
                "status_bits": up.map(|p| p.bits.json()),
                "downstream_hub_switching": up.and_then(|p| p.device_text.as_ref()).filter(|t| t.contains("ports")).and_then(|t| t.rsplit(", ").next()).map(String::from),
                "power_switching": if claimed { "per-port" } else { "not per-port" },
                "power_control_verification": verification(cfg, &key, claimed),
            })
        })
        .collect();
    let descriptor = match desc {
        Some(Ok(h)) => json!({
            "wHubCharacteristics": format!("{:#06x}", h.characteristics), "power_switching": h.switching(), "over_current_protection": h.over_current(),
            "compound_device": h.compound(), "port_indicators": h.port_indicators(), "power_on_to_power_good_ms": h.power_on_to_good_ms,
            "hub_controller_current_ma": h.controller_current_ma,
            "tt_think_time_fs_bits": if usb3 { Value::Null } else { json!(h.think_time_fs_bits()) },
            "note": "these are what the hub reports about itself, not what it does",
        }),
        Some(Err(e)) => json!({"unavailable": e}),
        None => Value::Null,
    };
    json!({
        "hub": d.path, "label": d.parent.as_ref().zip(d.port).and_then(|(h, p)| cfg.label(h, p)), "uhubctl_location": loc,
        "vidpid": d.vidpid(), "product": d.product, "usb_version": d.bcd_usb, "speed": d.speed_name(), "bus": d.bus, "devnum": d.devnum,
        "ports": d.maxchild, "root_hub": d.is_root_hub(), "bus_powered": !d.self_powered, "tt": d.tt_mode(),
        "uplink": d.parent.as_ref().zip(d.port).map(|(h, p)| json!({"hub": h, "port": p})),
        "controller": d.controller.as_ref().map(|c| json!({"pci": c.pci, "driver": c.driver})),
        "uhubctl_power_switching": switching, "hub_descriptor": descriptor, "port_status": ports,
        "devices_below": t.subtree(&d.path).len() - 1,
    })
}

/// `bench_usb_hubs`: every hub with its sysfs facts, what uhubctl says about its switching, its class descriptor, and its ports.
pub fn hubs(
    run: &dyn Runner,
    sysfs_root: &Path,
    cfg: &Config,
    args: &Value,
) -> Result<Value, String> {
    let t = Tree::read(sysfs_root)?;
    let only = args.get("hub").and_then(|v| v.as_str());
    let privileged = args
        .get("privileged")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    let ur = if privileged {
        uhubctl::status(run)
    } else {
        Err("not asked for (privileged: false)".into())
    };
    let mut out = Vec::new();
    for d in t
        .sorted()
        .into_iter()
        .filter(|d| d.is_hub() && only.is_none_or(|h| h == d.path))
    {
        let loc = d.path.strip_prefix("usb").unwrap_or(&d.path);
        let u = ur
            .as_ref()
            .ok()
            .and_then(|v| v.iter().find(|h| h.location == loc));
        let desc = privileged.then(|| uhubctl::hub_descriptor(run, d.bus, d.devnum));
        out.push(hub_json(&t, cfg, d, u, desc.as_ref(), &ur));
    }
    if out.is_empty() {
        return Err(format!("no hub {}", only.unwrap_or("found")));
    }
    Ok(json!({
        "hubs": out,
        "uhubctl": match &ur { Ok(v) => json!({"available": true, "hubs_listed": v.iter().map(|h| h.location.clone()).collect::<Vec<_>>()}), Err(e) => json!({"available": false, "error": e}) },
        "needs_sudo": "uhubctl status and the hub class descriptor are readable only by root, so this tool runs `sudo -n uhubctl` and `sudo -n lsusb -v`; the sysfs facts need no privilege and are given even when those fail",
    }))
}

pub fn bench_serials(insts: &[crate::config::Instrument]) -> Vec<BenchSerial> {
    insts
        .iter()
        .filter_map(|i| {
            i.serial.as_ref().map(|s| BenchSerial {
                instrument: i.name.clone(),
                path: s.clone(),
            })
        })
        .collect()
}

/// The port an argument set names: `hub` and `port`, a port `label`, or a `device` path (the port it is plugged into).
pub fn resolve_port(t: &Tree, cfg: &Config, args: &Value) -> Result<PortRef, String> {
    if let Some(l) = args.get("label").and_then(|v| v.as_str()) {
        let (key, _) = cfg
            .ports
            .iter()
            .find(|(_, p)| p.label.as_ref().is_some_and(|x| x.eq_ignore_ascii_case(l)))
            .ok_or_else(|| format!("no port is labelled {l:?}"))?;
        let (h, p) = key.split_once('/').unwrap_or_default();
        return Ok(port_ref(t, h, p.parse().unwrap_or(0)));
    }
    if let Some(d) = args.get("device").and_then(|v| v.as_str()) {
        let path = super::resolve_device(t, cfg, d)?;
        let dev = t.get(&path).ok_or("no such device")?;
        return match dev.parent.as_ref().zip(dev.port) {
            Some((h, p)) => Ok(port_ref(t, h, p)),
            None => Err(format!("{path} is a root hub and has no port")),
        };
    }
    match (
        args.get("hub").and_then(|v| v.as_str()),
        args.get("port").and_then(|v| v.as_u64()),
    ) {
        (Some(h), Some(p)) => Ok(port_ref(
            t,
            h,
            u32::try_from(p).map_err(|_| "port is too large")?,
        )),
        _ => Err(
            "name the port: hub and port (for example hub \"3-2\", port 2), or label, or device"
                .into(),
        ),
    }
}

fn port_ref(t: &Tree, hub: &str, port: u32) -> PortRef {
    let exact = t.get(hub).is_some_and(|h| h.speed_mbps() >= 5000.0);
    PortRef {
        hub: hub.to_string(),
        port,
        exact,
    }
}

fn secs(
    args: &Value,
    key: &str,
    default: Duration,
    min: Duration,
    max: Duration,
) -> Result<Duration, String> {
    let Some(v) = args.get(key).filter(|v| !v.is_null()) else {
        return Ok(default);
    };
    let n = v
        .as_f64()
        .filter(|n| n.is_finite())
        .ok_or_else(|| format!("{key} must be a number of seconds"))?;
    let d = Duration::from_secs_f64(n.max(0.0));
    if d < min || d > max {
        return Err(format!(
            "{key} must be between {} and {} seconds",
            min.as_secs(),
            max.as_secs()
        ));
    }
    Ok(d)
}

fn flag(args: &Value, key: &str) -> Result<bool, String> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(false),
        Some(v) => v
            .as_bool()
            .ok_or_else(|| format!("{key} must be true or false")),
    }
}

/// Everything about a port action that can be decided before touching it: the target, who is affected, the verdict and what the hub is.
pub struct Plan {
    pub affected: Vec<String>,
    pub verdict: protect::Verdict,
    pub switching: Result<(), String>,
}

pub fn plan(
    t: &Tree,
    cfg: &Config,
    crit: &protect::Critical,
    run: &dyn Runner,
    port: &PortRef,
) -> Result<Plan, String> {
    let hub = t
        .get(&port.hub)
        .ok_or_else(|| format!("no hub {} in the USB tree", port.hub))?;
    if !hub.is_hub() || port.port == 0 || port.port > hub.maxchild {
        return Err(format!(
            "{} has {} ports; {} is not one of them",
            port.hub, hub.maxchild, port.port
        ));
    }
    let verdict = protect::check_port(t, cfg, crit, &port.hub, port.port);
    let affected = t
        .get(&port.child())
        .map(|d| t.subtree(&d.path).iter().map(|x| x.path.clone()).collect())
        .unwrap_or_default();
    let switching = match uhubctl::status(run) {
        Err(e) => Err(e),
        Ok(list) => match list.iter().find(|h| h.location == port.location()) {
            None => Err(format!("uhubctl does not list hub {} as a switchable hub, so its ports cannot be switched", port.hub)),
            Some(h) if h.switching != Switching::PerPort => Err(format!(
                "hub {} switches power {} according to uhubctl, not per port: switching one port would not affect only that port",
                port.hub,
                h.switching.name()
            )),
            Some(_) => Ok(()),
        },
    };
    Ok(Plan {
        affected,
        verdict,
        switching,
    })
}

/// `bench_usb_port_power`: off, on or cycle one port.
pub fn port_power(
    run: &dyn Runner,
    sysfs_root: &Path,
    proc_root: &Path,
    cfg: &Config,
    bench: &[BenchSerial],
    tier: crate::config::Tier,
    args: &Value,
) -> Result<Value, String> {
    // [@ANCHOR: usb_hub_port_power]
    use crate::config::Tier;
    let action = args
        .get("action")
        .and_then(|v| v.as_str())
        .ok_or("action must be off, on or cycle")?;
    if !matches!(action, "off" | "on" | "cycle") {
        return Err(format!("unknown action {action:?} (off, on, cycle)"));
    }
    if action != "on" && tier < Tier::Full {
        return Err(format!("{action} needs tier Full; this instrument is configured at {tier:?} (switching a port back on needs read_config)"));
    }
    let dry = flag(args, "dry_run")?;
    let over = flag(args, "override_protection")?;
    let off_time = secs(
        args,
        "off_time_s",
        power::DEFAULT_OFF_TIME,
        power::MIN_OFF_TIME,
        power::MAX_OFF_TIME,
    )?;
    let back = secs(
        args,
        "back_timeout_s",
        power::DEFAULT_BACK_TIMEOUT,
        Duration::from_secs(5),
        Duration::from_secs(300),
    )?;
    let expect = args
        .get("expect_vidpid")
        .and_then(|v| v.as_str())
        .map(|s| s.to_ascii_lowercase());
    let t = Tree::read(sysfs_root)?;
    let port = resolve_port(&t, cfg, args)?;
    let crit = protect::critical_set(&t, cfg, bench, sysfs_root, proc_root);
    let p = plan(&t, cfg, &crit, run, &port)?;
    let label = cfg.label(&port.hub, port.port);
    let name = match &label {
        Some(l) => format!("port {} ({l})", port.key()),
        None => format!("port {}", port.key()),
    };
    let loc = port.location();
    let cmd = |a: &str| {
        format!(
            "sudo -n uhubctl -N -l {loc} -p {} -a {a}{}",
            port.port,
            if port.exact { " -e" } else { "" }
        )
    };
    let mut steps = Vec::new();
    if action != "on" {
        steps.push(cmd("off"));
        steps.push(format!(
            "wait up to {} s for {} to leave the USB tree",
            power::GONE_TIMEOUT.as_secs(),
            port.child()
        ));
    }
    if action == "cycle" {
        steps.push(format!(
            "keep the port off for at least {} s",
            off_time.as_secs_f64()
        ));
    }
    if action != "off" {
        steps.push(cmd("on"));
        steps.push(format!(
            "wait up to {} s for {} to enumerate again with a new device number",
            back.as_secs(),
            port.child()
        ));
    }
    if dry {
        let blocked = if action == "on" {
            None
        } else {
            p.verdict.permit(&name, over).err()
        };
        return Ok(json!({
            "dry_run": true, "action": action, "port": port.key(), "label": label, "devices_affected": p.affected, "steps": steps,
            "protection": p.verdict.json(),
            "hub_switching": p.switching.clone().err().unwrap_or_else(|| "per-port (as uhubctl reports it)".into()),
            "would_be_refused": blocked.or(p.switching.clone().err()),
            "nothing_was_changed": true,
        }));
    }
    if action != "on" {
        p.verdict.permit(&name, over)?;
    }
    p.switching.clone()?;
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _held = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    match action {
        "on" => power::set(run, sysfs_root, &port, true, back),
        "off" => {
            let mut v = power::set(run, sysfs_root, &port, false, back)?;
            v["warning"] = json!("the port is now OFF; switch it on again with action on");
            Ok(v)
        }
        _ => {
            let opts = Opts {
                expect_vidpid: expect,
                ..Opts::new(off_time, back)
            };
            let r = power::cycle(run, sysfs_root, &port, &opts, None)?;
            let mut v = r.json();
            v["devices_affected"] = json!(p.affected);
            v["power_control_verification"] = verification(cfg, &port.key(), true);
            Ok(v)
        }
    }
}

pub fn protection(
    t: &Tree,
    cfg: &Config,
    bench: &[BenchSerial],
    sysfs_root: &Path,
    proc_root: &Path,
    args: &Value,
) -> Result<Value, String> {
    let crit = protect::critical_set(t, cfg, bench, sysfs_root, proc_root);
    let mut v = protect::report(t, cfg, &crit);
    if args.get("hub").is_some() || args.get("label").is_some() || args.get("device").is_some() {
        let port = resolve_port(t, cfg, args)?;
        v["port_verdict"] = json!({"port": port.key(), "verdict": protect::check_port(t, cfg, &crit, &port.hub, port.port).json()});
    }
    Ok(v)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Tier;
    use crate::usb_hub::testkit::{recorded_sys, writable_sys, Behaviour, Dir, FakeHub};

    fn cfg() -> Config {
        crate::usb_hub::parse_config(&json!({"ports": {"3-2/2": {"label": "VNWA 3E", "hub_uplink_ok": true, "confirmed_by_hand": "goes dark and keeps no settings"}, "3-2/4": {"label": "spare"}}})).unwrap()
    }

    #[test]
    fn hubs_merge_sysfs_uhubctl_and_descriptor_and_claim_only_what_uhubctl_says() {
        let hub = FakeHub::new(&recorded_sys(), Behaviour::Switches);
        let v = hubs(&hub, &recorded_sys(), &cfg(), &json!({"privileged": false})).unwrap();
        assert_eq!(v["uhubctl"]["available"], false);
        let h = v["hubs"]
            .as_array()
            .unwrap()
            .iter()
            .find(|h| h["hub"] == "3-2")
            .unwrap();
        assert_eq!(h["ports"], 5);
        assert!(h["uhubctl_power_switching"]
            .as_str()
            .unwrap()
            .starts_with("unknown"));
        assert_eq!(h["port_status"][1]["label"], "VNWA 3E");
    }

    // Tests [@ANCHOR: usb_hub_verification_levels]
    #[test]
    fn verification_levels_are_claimed_rechecked_and_manual() {
        let c = cfg();
        let a = verification(&c, "3-2/4", true);
        assert_eq!(
            (a["level"].as_u64(), a["name"].as_str()),
            (Some(1), Some("claimed"))
        );
        let none = verification(&c, "3-2/4", false);
        assert_eq!(none["level"], 0);
        let b = verification(&c, "3-2/2", true);
        assert_eq!(b["level"], 2);
        assert!(b["evidence"][1]
            .as_str()
            .unwrap()
            .contains("manually confirmed"));
        assert!(b["note"].as_str().unwrap().contains("not that VBUS fell"));
        let c4 = crate::usb_hub::parse_config(&json!({"ports": {"3-2/2": {"state_cleared": "VNA reverts to defaults", "vbus_measured": "0.02 V"}}})).unwrap();
        assert_eq!(verification(&c4, "3-2/2", true)["level"], 4);
    }

    #[test]
    fn dry_run_says_what_would_happen_and_changes_nothing() {
        let (_d, sys) = writable_sys();
        let hub = FakeHub::new(&sys, Behaviour::Switches);
        let proc = Dir::new();
        let v = port_power(
            &hub,
            &sys,
            proc.path(),
            &cfg(),
            &[],
            Tier::Full,
            &json!({"label": "vnwa 3e", "action": "cycle", "dry_run": true, "off_time_s": 7}),
        )
        .unwrap();
        assert_eq!(v["dry_run"], true);
        assert!(v["steps"][0]
            .as_str()
            .unwrap()
            .ends_with("-l 3-2 -p 2 -a off"));
        assert_eq!(v["devices_affected"].as_array().unwrap().len(), 4);
        assert!(v["would_be_refused"].is_null(), "{v}");
        assert!(
            hub.calls.borrow().iter().all(|c| !c.contains("-a ")),
            "only status was read: {:?}",
            hub.calls.borrow()
        );
        assert!(crate::usb_hub::power::probe(&sys, "3-2.2").is_some());
    }

    #[test]
    fn power_off_and_cycle_need_tier_full_and_on_needs_read_config() {
        let (_d, sys) = writable_sys();
        let hub = FakeHub::new(&sys, Behaviour::Switches);
        let proc = Dir::new();
        let go = |tier, action: &str| {
            port_power(
                &hub,
                &sys,
                proc.path(),
                &cfg(),
                &[],
                tier,
                &json!({"hub": "3-2", "port": 4, "action": action, "dry_run": true}),
            )
        };
        assert!(go(Tier::ReadConfig, "off")
            .unwrap_err()
            .contains("needs tier Full"));
        assert!(go(Tier::ReadConfig, "cycle")
            .unwrap_err()
            .contains("needs tier Full"));
        assert!(go(Tier::ReadConfig, "on").is_ok());
        assert!(go(Tier::Full, "bogus")
            .unwrap_err()
            .contains("unknown action"));
    }

    // Tests [@ANCHOR: usb_hub_port_power]
    #[test]
    fn a_protected_hub_uplink_is_refused_and_the_override_works_only_where_allowed() {
        let (_d, sys) = writable_sys();
        let hub = FakeHub::new(&sys, Behaviour::Switches);
        let proc = Dir::new();
        // No hub_uplink_ok and no label: the VNWA's hub is a hub's uplink.
        let plain = crate::usb_hub::parse_config(&json!({})).unwrap();
        let args = json!({"hub": "3-2", "port": 2, "action": "off", "off_time_s": 1});
        let e = port_power(&hub, &sys, proc.path(), &plain, &[], Tier::Full, &args).unwrap_err();
        assert!(
            e.contains("uplink") && e.contains("override_protection"),
            "{e}"
        );
        assert!(
            hub.calls.borrow().iter().all(|c| !c.contains("-a ")),
            "refused before any switching"
        );
        let crit =
            crate::usb_hub::parse_config(&json!({"critical_devices": ["08bb:29c0"]})).unwrap();
        let e = port_power(
            &hub,
            &sys,
            proc.path(),
            &crit,
            &[],
            Tier::Full,
            &json!({"hub": "3-2", "port": 2, "action": "off", "override_protection": true}),
        )
        .unwrap_err();
        assert!(e.contains("no argument overrides"), "{e}");
        let d = port_power(
            &hub,
            &sys,
            proc.path(),
            &plain,
            &[],
            Tier::Full,
            &json!({"hub": "3-2", "port": 2, "action": "cycle", "dry_run": true}),
        )
        .unwrap();
        assert!(d["would_be_refused"].as_str().unwrap().contains("uplink"));
    }

    #[test]
    fn a_cycle_on_a_labelled_port_runs_through_uhubctl_and_reports_levels_and_actual_off_time() {
        let (_d, sys) = writable_sys();
        let hub = FakeHub::new(&sys, Behaviour::Switches);
        let proc = Dir::new();
        let v = port_power(
            &hub,
            &sys,
            proc.path(),
            &cfg(),
            &[],
            Tier::Full,
            &json!({"label": "VNWA 3E", "action": "cycle", "off_time_s": 1, "back_timeout_s": 5}),
        )
        .unwrap();
        assert_eq!(v["verification_level"], 2);
        assert!(v["off_time_actual_s"].as_f64().unwrap() >= 0.99, "{v}");
        assert_eq!(v["power_control_verification"]["level"], 2);
        assert_eq!(v["device_after"]["vidpid"], "0451:2036");
    }

    #[test]
    fn ports_of_ganged_or_unlisted_hubs_are_refused() {
        let (_d, sys) = writable_sys();
        let hub = FakeHub::new(&sys, Behaviour::Switches);
        let proc = Dir::new();
        // 3-2.2 is listed by uhubctl only as a ganged device on port 2, and root hubs are not listed at all.
        for (h, p) in [("3-2.2", 1), ("usb3", 4)] {
            let e = port_power(
                &hub,
                &sys,
                proc.path(),
                &cfg(),
                &[],
                Tier::Full,
                &json!({"hub": h, "port": p, "action": "off"}),
            )
            .unwrap_err();
            assert!(e.contains("does not list hub"), "{e}");
        }
        let e = port_power(
            &hub,
            &sys,
            proc.path(),
            &cfg(),
            &[],
            Tier::Full,
            &json!({"hub": "3-2", "port": 9, "action": "off"}),
        )
        .unwrap_err();
        assert!(e.contains("not one of them"), "{e}");
    }

    #[test]
    fn a_superspeed_hub_is_addressed_with_exact_location() {
        let (_d, sys) = writable_sys();
        let hub = FakeHub::new(&sys, Behaviour::Ignores);
        let proc = Dir::new();
        let v = port_power(
            &hub,
            &sys,
            proc.path(),
            &cfg(),
            &[],
            Tier::Full,
            &json!({"hub": "2-3", "port": 4, "action": "cycle", "dry_run": true}),
        )
        .unwrap();
        assert_eq!(v["steps"][0], "sudo -n uhubctl -N -l 2-3 -p 4 -a off -e");
        let v = port_power(
            &hub,
            &sys,
            proc.path(),
            &cfg(),
            &[],
            Tier::Full,
            &json!({"hub": "3-2", "port": 4, "action": "cycle", "dry_run": true}),
        )
        .unwrap();
        assert_eq!(v["steps"][0], "sudo -n uhubctl -N -l 3-2 -p 4 -a off");
    }

    #[test]
    fn off_time_is_bounded_and_ports_resolve_by_device() {
        let (_d, sys) = writable_sys();
        let hub = FakeHub::new(&sys, Behaviour::Switches);
        let proc = Dir::new();
        let e = port_power(
            &hub,
            &sys,
            proc.path(),
            &cfg(),
            &[],
            Tier::Full,
            &json!({"hub": "3-2", "port": 4, "action": "cycle", "off_time_s": 0.1}),
        )
        .unwrap_err();
        assert!(e.contains("off_time_s must be between"), "{e}");
        let t = Tree::read(&sys).unwrap();
        let p = resolve_port(&t, &cfg(), &json!({"device": "3-2.2"})).unwrap();
        assert_eq!(p.key(), "3-2/2");
        assert!(resolve_port(&t, &cfg(), &json!({})).is_err());
    }
}
