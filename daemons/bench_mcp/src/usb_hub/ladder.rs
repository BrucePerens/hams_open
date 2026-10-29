// SPDX-License-Identifier: AGPL-3.0-or-later
//! The escalation ladder below a port power cycle, each rung its own tool at the middle tier: a soft reset of a device
//! (USBDEVFS_RESET), deauthorize and authorize (a software disconnect that keeps the power), driver unbind and bind, and autosuspend
//! control. The privileged part is the `--usb-priv` helper of this same binary run through `sudo -n`; see `privileged`.

use super::hubs::flag;
use super::power::{probe, wait_until};
use super::privileged::FLAG;
use super::protect::{self, BenchRef};
use super::sysfs::Tree;
use super::uhubctl::{explain_failure, Runner};
use super::Config;
use serde_json::{json, Value};
use std::path::Path;
use std::time::Duration;

/// The helper command: `sudo -n <this binary> --usb-priv <args>`.
pub fn helper_command(exe: &str, args: &[&str]) -> Vec<String> {
    let mut v: Vec<String> = vec!["-n".into(), exe.into(), FLAG.into()];
    v.extend(args.iter().map(|s| s.to_string()));
    v
}

fn run_helper(run: &dyn Runner, exe: &str, args: &[&str]) -> Result<String, String> {
    let (ok, text) = run.run("sudo", &helper_command(exe, args)).map_err(|e| {
        if e.starts_with("sudo") {
            format!("sudo is not installed: {e}")
        } else {
            e
        }
    })?;
    if ok {
        Ok(text.trim().to_string())
    } else {
        Err(explain_failure(exe, &text))
    }
}

/// What every rung needs: the tree, the device named by the arguments, and the protection verdict for acting on it.
struct Target {
    tree: Tree,
    device: String,
    verdict: protect::Verdict,
    dry: bool,
    over: bool,
}

fn target(
    sysfs: &Path,
    proc_root: &Path,
    cfg: &Config,
    bench: &[BenchRef],
    args: &Value,
) -> Result<Target, String> {
    let tree = Tree::read(sysfs)?;
    let name = args
        .get("device")
        .and_then(|v| v.as_str())
        .ok_or("name the device: its path (for example 3-2.2.1) or its port label")?;
    let device = super::resolve_device(&tree, cfg, name)?;
    if tree.get(&device).is_some_and(|d| d.is_root_hub()) {
        return Err(format!(
            "{device} is a root hub: these tools act on devices behind a port"
        ));
    }
    let crit = protect::critical_set(&tree, cfg, bench, sysfs, proc_root);
    let verdict = protect::check_device(&tree, cfg, &crit, &device);
    Ok(Target {
        tree,
        device,
        verdict,
        dry: flag(args, "dry_run")?,
        over: flag(args, "override_protection")?,
    })
}

fn dry(t: &Target, what: &str, commands: Vec<String>, extra: Value) -> Value {
    let affected: Vec<String> = t
        .tree
        .subtree(&t.device)
        .iter()
        .map(|d| d.path.clone())
        .collect();
    json!({
        "dry_run": true, "device": t.device, "would": what, "commands": commands, "devices_affected": affected,
        "protection": t.verdict.json(), "would_be_refused": t.verdict.permit(&format!("{}:", t.device), t.over).err(),
        "detail": extra, "nothing_was_changed": true,
    })
}

fn exe() -> String {
    std::env::current_exe()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| "bench_mcp".into())
}

fn cmd_text(args: &[&str]) -> String {
    format!("sudo {}", helper_command(&exe(), args).join(" "))
}

/// `bench_usb_reset`: a soft reset of one device (and, for a hub, everything below it re-enumerates).
pub fn reset(
    run: &dyn Runner,
    sysfs: &Path,
    proc_root: &Path,
    cfg: &Config,
    bench: &[BenchRef],
    args: &Value,
) -> Result<Value, String> {
    // [@ANCHOR: usb_hub_ladder]
    let t = target(sysfs, proc_root, cfg, bench, args)?;
    let a = ["reset", t.device.as_str()];
    if t.dry {
        return Ok(dry(&t, "a USBDEVFS_RESET of the device: it keeps its power and its device number and its drivers are rebound", vec![cmd_text(&a)], Value::Null));
    }
    t.verdict
        .permit(&format!("resetting {}:", t.device), t.over)?;
    let before = probe(sysfs, &t.device);
    let started = std::time::Instant::now();
    run_helper(run, &exe(), &a)?;
    let back = wait_until(Duration::from_secs(10), || {
        probe(sysfs, &t.device).is_some()
    });
    Ok(json!({
        "reset": t.device, "back_in_the_tree": back, "after_s": started.elapsed().as_secs_f64(),
        "device_number_before": before.as_ref().map(|b| b.devnum), "device_number_after": probe(sysfs, &t.device).map(|b| b.devnum),
        "note": "a reset keeps the power: the device is re-initialised by the host, not power cycled",
    }))
}

/// `bench_usb_authorize`: deauthorize (the kernel disconnects the device's drivers and the device leaves the tree) or authorize it again.
pub fn authorize(
    run: &dyn Runner,
    sysfs: &Path,
    proc_root: &Path,
    cfg: &Config,
    bench: &[BenchRef],
    args: &Value,
) -> Result<Value, String> {
    let t = target(sysfs, proc_root, cfg, bench, args)?;
    let on = args
        .get("authorized")
        .and_then(|v| v.as_bool())
        .ok_or("authorized must be true or false")?;
    let v = if on { "1" } else { "0" };
    let a = ["authorize", t.device.as_str(), v];
    if t.dry {
        let what = if on {
            "authorize the device: it is configured again and its drivers bind"
        } else {
            "deauthorize the device: its drivers unbind and it is no longer used, but it keeps its power"
        };
        return Ok(dry(&t, what, vec![cmd_text(&a)], Value::Null));
    }
    if !on {
        t.verdict
            .permit(&format!("deauthorizing {}:", t.device), t.over)?;
    }
    let before = probe(sysfs, &t.device);
    run_helper(run, &exe(), &a)?;
    let after = probe(sysfs, &t.device);
    let dev = Tree::read(sysfs).ok().and_then(|tr| {
        tr.get(&t.device).map(|d| {
            (
                d.authorized,
                d.interfaces.iter().filter(|i| i.driver.is_some()).count(),
            )
        })
    });
    Ok(json!({
        "device": t.device, "authorized": on, "device_number_before": before.map(|b| b.devnum), "device_number_after": after.map(|b| b.devnum),
        "now": dev.map(|(a, bound)| json!({"authorized": a, "interfaces_with_a_driver": bound})),
        "note": "deauthorizing is a software disconnect: the device stays powered. Authorize it again with authorized: true",
    }))
}

/// `bench_usb_driver`: unbind or bind one interface from or to a driver.
pub fn driver(
    run: &dyn Runner,
    sysfs: &Path,
    proc_root: &Path,
    cfg: &Config,
    bench: &[BenchRef],
    args: &Value,
) -> Result<Value, String> {
    let tree = Tree::read(sysfs)?;
    let iface_name = args
        .get("interface")
        .and_then(|v| v.as_str())
        .ok_or("name the interface, for example 3-2.2.2:1.3")?;
    let (dev_path, _) = iface_name
        .split_once(':')
        .ok_or("an interface name looks like 3-2.2.2:1.3")?;
    let dev = tree
        .get(dev_path)
        .ok_or_else(|| format!("no device {dev_path} in the tree"))?;
    let iface = dev
        .interfaces
        .iter()
        .find(|i| i.name == iface_name)
        .ok_or_else(|| format!("{dev_path} has no interface {iface_name}"))?;
    let action = args
        .get("action")
        .and_then(|v| v.as_str())
        .ok_or("action must be unbind or bind")?;
    let drv = match (action, args.get("driver").and_then(|v| v.as_str())) {
        ("unbind", d) => d
            .map(String::from)
            .or_else(|| iface.driver.clone())
            .ok_or_else(|| format!("{iface_name} has no driver bound"))?,
        ("bind", Some(d)) => d.to_string(),
        ("bind", None) => return Err("bind needs a driver name".into()),
        (o, _) => return Err(format!("unknown action {o:?} (unbind, bind)")),
    };
    let crit = protect::critical_set(&tree, cfg, bench, sysfs, proc_root);
    let verdict = protect::check_device(&tree, cfg, &crit, dev_path);
    let (dry_run, over) = (flag(args, "dry_run")?, flag(args, "override_protection")?);
    let a = [action, iface_name, drv.as_str()];
    if dry_run {
        return Ok(json!({
            "dry_run": true, "interface": iface_name, "would": format!("{action} {iface_name} {} driver {drv}", if action == "unbind" { "from" } else { "to" }),
            "commands": [cmd_text(&a)], "currently_bound": iface.driver, "nodes_that_go_away": iface.nodes.iter().map(|n| n.name.clone()).collect::<Vec<_>>(),
            "protection": verdict.json(), "would_be_refused": verdict.permit(&format!("{action} on {dev_path}:"), over).err(), "nothing_was_changed": true,
        }));
    }
    if action == "unbind" {
        verdict.permit(&format!("unbinding {iface_name}:"), over)?;
    }
    run_helper(run, &exe(), &a)?;
    let now = Tree::read(sysfs).ok().and_then(|t| {
        t.get(dev_path).and_then(|d| {
            d.interfaces
                .iter()
                .find(|i| i.name == iface_name)
                .map(|i| i.driver.clone())
        })
    });
    Ok(json!({"interface": iface_name, "action": action, "driver": drv, "driver_bound_now": now}))
}

/// `bench_usb_autosuspend`: runtime power management of one device.
pub fn autosuspend(
    run: &dyn Runner,
    sysfs: &Path,
    proc_root: &Path,
    cfg: &Config,
    bench: &[BenchRef],
    args: &Value,
) -> Result<Value, String> {
    let t = target(sysfs, proc_root, cfg, bench, args)?;
    let control = args
        .get("control")
        .and_then(|v| v.as_str())
        .ok_or("control must be on (never suspend) or auto (may suspend)")?;
    if !matches!(control, "on" | "auto") {
        return Err(format!("unknown control {control:?} (on, auto)"));
    }
    let delay = match args.get("delay_ms") {
        None | Some(Value::Null) => "-".to_string(),
        Some(v) => {
            let n = v
                .as_i64()
                .ok_or("delay_ms must be a whole number of milliseconds")?;
            if !(-1..=3_600_000).contains(&n) {
                return Err("delay_ms must be -1 to 3600000".into());
            }
            n.to_string()
        }
    };
    let a = ["autosuspend", t.device.as_str(), control, delay.as_str()];
    if t.dry {
        let d = t.tree.get(&t.device).map(|d| json!({"control": d.power_control, "delay_ms": d.autosuspend_delay_ms, "runtime_status": d.runtime_status}));
        return Ok(dry(
            &t,
            "set the device's runtime power management",
            vec![cmd_text(&a)],
            json!({"now": d}),
        ));
    }
    if control == "auto" {
        t.verdict
            .permit(&format!("allowing {} to autosuspend:", t.device), t.over)?;
    }
    run_helper(run, &exe(), &a)?;
    let now = Tree::read(sysfs).ok().and_then(|tr| tr.get(&t.device).map(|d| json!({"control": d.power_control, "delay_ms": d.autosuspend_delay_ms, "runtime_status": d.runtime_status})));
    Ok(json!({"device": t.device, "now": now}))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::usb_hub::testkit::{writable_sys, Dir};
    use std::cell::RefCell;

    /// Runs the helper operation in-process against the copy of sysfs, as the root helper would, and records the command.
    struct InProcess {
        sys: std::path::PathBuf,
        calls: RefCell<Vec<String>>,
    }
    impl Runner for InProcess {
        fn run(&self, program: &str, args: &[String]) -> Result<(bool, String), String> {
            self.calls
                .borrow_mut()
                .push(format!("{program} {}", args.join(" ")));
            assert_eq!(
                (program, args[0].as_str(), args[2].as_str()),
                ("sudo", "-n", FLAG)
            );
            match crate::usb_hub::privileged::run_at(&self.sys, &args[3..]) {
                Ok(s) => Ok((true, s)),
                Err(e) => Ok((false, e)),
            }
        }
    }

    fn cfg() -> Config {
        crate::usb_hub::parse_config(
            &json!({"ports": {"3-2.2/1": {"label": "SDR kit"}}, "critical_devices": ["08bb:29c0"]}),
        )
        .unwrap()
    }

    // Tests [@ANCHOR: usb_hub_ladder]
    #[test]
    fn deauthorize_and_authorize_go_through_the_helper_and_report_the_state() {
        let (_d, sys) = writable_sys();
        let run = InProcess {
            sys: sys.clone(),
            calls: RefCell::new(vec![]),
        };
        let proc = Dir::new();
        let v = authorize(
            &run,
            &sys,
            proc.path(),
            &cfg(),
            &[],
            &json!({"device": "sdr kit", "authorized": false}),
        )
        .unwrap();
        assert_eq!(v["device"], "3-2.2.1");
        assert!(
            run.calls.borrow()[0].contains("--usb-priv authorize 3-2.2.1 0"),
            "{:?}",
            run.calls.borrow()
        );
        assert_eq!(v["now"]["authorized"], false);
        let v = authorize(
            &run,
            &sys,
            proc.path(),
            &cfg(),
            &[],
            &json!({"device": "3-2.2.1", "authorized": true}),
        )
        .unwrap();
        assert_eq!(v["now"]["authorized"], true);
    }

    #[test]
    fn protected_devices_are_refused_before_the_helper_runs_and_dry_run_changes_nothing() {
        let (_d, sys) = writable_sys();
        let run = InProcess {
            sys: sys.clone(),
            calls: RefCell::new(vec![]),
        };
        let proc = Dir::new();
        let e = reset(
            &run,
            &sys,
            proc.path(),
            &cfg(),
            &[],
            &json!({"device": "3-2.2.2"}),
        )
        .unwrap_err();
        assert!(
            e.contains("critical_devices") && e.contains("no argument overrides"),
            "{e}"
        );
        let e = authorize(
            &run,
            &sys,
            proc.path(),
            &cfg(),
            &[],
            &json!({"device": "3-2.2", "authorized": false}),
        )
        .unwrap_err();
        assert!(
            e.contains("08bb:29c0"),
            "the hub on the path to a critical device: {e}"
        );
        assert!(run.calls.borrow().is_empty());
        let d = reset(
            &run,
            &sys,
            proc.path(),
            &cfg(),
            &[],
            &json!({"device": "3-2.2.2", "dry_run": true}),
        )
        .unwrap();
        assert!(
            d["would_be_refused"].as_str().unwrap().contains("refused"),
            "{d}"
        );
        assert!(d["commands"][0]
            .as_str()
            .unwrap()
            .contains("--usb-priv reset 3-2.2.2"));
        let ok = reset(
            &run,
            &sys,
            proc.path(),
            &cfg(),
            &[],
            &json!({"device": "3-2.2.1", "dry_run": true}),
        )
        .unwrap();
        assert!(ok["would_be_refused"].is_null() && ok["nothing_was_changed"] == true);
        assert!(
            run.calls.borrow().is_empty(),
            "a dry run never calls the helper"
        );
        let e = reset(
            &run,
            &sys,
            proc.path(),
            &cfg(),
            &[],
            &json!({"device": "usb3"}),
        )
        .unwrap_err();
        assert!(e.contains("root hub"), "{e}");
    }

    #[test]
    fn driver_unbind_and_bind_name_the_interface_and_autosuspend_reads_back() {
        let (_d, sys) = writable_sys();
        std::fs::create_dir_all(sys.join("bus/usb/drivers/usbhid")).unwrap();
        let run = InProcess {
            sys: sys.clone(),
            calls: RefCell::new(vec![]),
        };
        let proc = Dir::new();
        let c = crate::usb_hub::parse_config(&json!({})).unwrap();
        let d = driver(
            &run,
            &sys,
            proc.path(),
            &c,
            &[],
            &json!({"interface": "3-2.2.1:1.0", "action": "unbind", "dry_run": true}),
        );
        assert!(
            d.unwrap_err().contains("no driver bound"),
            "the recorded SDR kit interface has no driver"
        );
        let d = driver(
            &run,
            &sys,
            proc.path(),
            &c,
            &[],
            &json!({"interface": "3-2.2.2:1.3", "action": "unbind", "dry_run": true}),
        )
        .unwrap();
        assert_eq!(d["currently_bound"], "usbhid");
        assert!(d["commands"][0]
            .as_str()
            .unwrap()
            .ends_with("unbind 3-2.2.2:1.3 usbhid"));
        let v = driver(
            &run,
            &sys,
            proc.path(),
            &c,
            &[],
            &json!({"interface": "3-2.2.2:1.3", "action": "unbind"}),
        )
        .unwrap();
        assert_eq!(v["driver"], "usbhid");
        let e = driver(
            &run,
            &sys,
            proc.path(),
            &c,
            &[],
            &json!({"interface": "3-2.2.2:1.3", "action": "bind"}),
        )
        .unwrap_err();
        assert!(e.contains("driver name"), "{e}");
        let a = autosuspend(
            &run,
            &sys,
            proc.path(),
            &c,
            &[],
            &json!({"device": "3-2.2.1", "control": "auto", "delay_ms": 3000}),
        )
        .unwrap();
        assert_eq!(
            (a["now"]["control"].as_str(), a["now"]["delay_ms"].as_i64()),
            (Some("auto"), Some(3000))
        );
        assert!(autosuspend(
            &run,
            &sys,
            proc.path(),
            &c,
            &[],
            &json!({"device": "3-2.2.1", "control": "sleep"})
        )
        .is_err());
        assert!(autosuspend(
            &run,
            &sys,
            proc.path(),
            &c,
            &[],
            &json!({"device": "3-2.2.1", "control": "on", "delay_ms": 1e9})
        )
        .is_err());
    }

    #[test]
    fn a_failing_sudo_is_explained() {
        struct NoSudo;
        impl Runner for NoSudo {
            fn run(&self, _: &str, _: &[String]) -> Result<(bool, String), String> {
                Ok((false, "sudo: a password is required".into()))
            }
        }
        let (_d, sys) = writable_sys();
        let proc = Dir::new();
        let c = crate::usb_hub::parse_config(&json!({})).unwrap();
        let e = authorize(
            &NoSudo,
            &sys,
            proc.path(),
            &c,
            &[],
            &json!({"device": "3-2.2.1", "authorized": true}),
        )
        .unwrap_err();
        assert!(e.contains("NOPASSWD"), "{e}");
    }
}
