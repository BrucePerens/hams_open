// SPDX-License-Identifier: AGPL-3.0-or-later
//! Port power off, on and cycle through `sudo -n uhubctl`, the one implementation behind the hub tools and the VNA's cold reset.
//!
//! Every cycle checks what it can from software: that the device on the port left the USB tree when the port was switched
//! off, and that it came back with a new device number and readable descriptors. That is verification level 2
//! ("re-enumerated"). It shows the port dropped the device; it does not show that VBUS fell, because some hubs disable the data
//! lines and leave VBUS on. Level 1 is only that uhubctl reports per-port switching ("claimed"), level 3 that the device lost its
//! settings (a device-specific check), level 4 a VBUS measurement (manual entry).

use super::sysfs::child_path;
use super::uhubctl::{sudo, Runner};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// The default time a port stays off, so the device's supply capacitors discharge.
pub const DEFAULT_OFF_TIME: Duration = Duration::from_secs(5);
pub const MIN_OFF_TIME: Duration = Duration::from_secs(1);
pub const MAX_OFF_TIME: Duration = Duration::from_secs(60);
/// How long a device may take to leave the tree after the port is switched off.
pub const GONE_TIMEOUT: Duration = Duration::from_secs(10);
pub const DEFAULT_BACK_TIMEOUT: Duration = Duration::from_secs(45);
const POLL: Duration = Duration::from_millis(100);

#[derive(Clone, Debug, PartialEq)]
pub struct PortRef {
    pub hub: String,
    pub port: u32,
    /// Pass `-e` to uhubctl: needed for a SuperSpeed hub, whose location uhubctl 2.6.0 does not find otherwise (observed on this
    /// machine's 14b0:0131 hub: `-l 2-3` answers "No compatible devices detected at location 2-3" and `-l 2-3 -e` works).
    pub exact: bool,
}

impl PortRef {
    pub fn key(&self) -> String {
        format!("{}/{}", self.hub, self.port)
    }
    /// The hub as uhubctl names it: `3-2`, or the bare bus number for a root hub.
    pub fn location(&self) -> String {
        self.hub
            .strip_prefix("usb")
            .unwrap_or(&self.hub)
            .to_string()
    }
    pub fn child(&self) -> String {
        child_path(&self.hub, self.port)
    }
}

/// A device as the tree shows it right now: its device number and ids, read straight from its sysfs directory.
#[derive(Clone, Debug, PartialEq)]
pub struct Seen {
    pub devnum: u32,
    pub vidpid: String,
}

pub fn probe(sysfs: &Path, path: &str) -> Option<Seen> {
    let d = sysfs.join("bus/usb/devices").join(path);
    let read = |f: &str| {
        std::fs::read_to_string(d.join(f))
            .ok()
            .map(|s| s.trim().to_string())
    };
    let (vid, pid) = (read("idVendor")?, read("idProduct")?);
    Some(Seen {
        devnum: read("devnum")?.parse().ok()?,
        vidpid: format!("{vid}:{pid}"),
    })
}

/// What a session has proved about a port by cycling it.
#[derive(Clone, Debug, PartialEq)]
pub struct Evidence {
    pub unix_time: u64,
    pub gone_after_s: f64,
    pub back_after_s: f64,
    pub old_devnum: u32,
    pub new_devnum: u32,
}

static PROVEN: Mutex<Option<HashMap<String, Evidence>>> = Mutex::new(None);

pub fn proven(key: &str) -> Option<Evidence> {
    PROVEN
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
        .and_then(|m| m.get(key).cloned())
}

fn remember(key: &str, e: Evidence) {
    PROVEN
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get_or_insert_with(HashMap::new)
        .insert(key.to_string(), e);
}

fn uhubctl(run: &dyn Runner, port: &PortRef, action: &str) -> Result<(), String> {
    let p = port.port.to_string();
    let loc = port.location();
    let mut args = vec!["-N", "-l", &loc, "-p", &p, "-a", action];
    if port.exact {
        args.push("-e");
    }
    sudo(run, "uhubctl", &args)
        .map(|_| ())
        .map_err(|e| format!("uhubctl {action} on hub {loc} port {p} failed: {e}"))
}

pub fn wait_until(timeout: Duration, mut done: impl FnMut() -> bool) -> bool {
    let end = Instant::now() + timeout;
    loop {
        if done() {
            return true;
        }
        if Instant::now() >= end {
            return false;
        }
        std::thread::sleep(POLL);
    }
}

/// The timing and expectation of one cycle.
#[derive(Clone, Debug, PartialEq)]
pub struct Opts {
    pub off_time: Duration,
    pub back: Duration,
    /// How long to wait for the device to leave the tree.
    pub gone: Duration,
    /// The vid:pid that must be back; the device's own when None.
    pub expect_vidpid: Option<String>,
}

impl Opts {
    pub fn new(off_time: Duration, back: Duration) -> Opts {
        Opts {
            off_time,
            back,
            gone: GONE_TIMEOUT,
            expect_vidpid: None,
        }
    }
}

/// The result of one off-wait-on cycle.
#[derive(Clone, Debug)]
pub struct Report {
    pub port: PortRef,
    pub device_before: Option<Seen>,
    pub device_after: Option<Seen>,
    pub off_time_requested_s: f64,
    pub off_time_actual_s: f64,
    pub gone_after_s: Option<f64>,
    pub back_after_s: Option<f64>,
    pub ready_after_s: Option<f64>,
    pub re_enumerated: bool,
    pub notes: Vec<String>,
}

impl Report {
    pub fn level(&self) -> u8 {
        if self.re_enumerated {
            2
        } else {
            1
        }
    }
    pub fn json(&self) -> Value {
        let seen = |s: &Option<Seen>| {
            s.as_ref()
                .map(|s| json!({"devnum": s.devnum, "vidpid": s.vidpid}))
        };
        json!({
            "port": self.port.key(), "device_before": seen(&self.device_before), "device_after": seen(&self.device_after),
            "off_time_requested_s": self.off_time_requested_s, "off_time_actual_s": self.off_time_actual_s,
            "left_the_tree_after_s": self.gone_after_s, "back_in_the_tree_after_s": self.back_after_s, "ready_after_s": self.ready_after_s,
            "re_enumerated": self.re_enumerated, "verification_level": self.level(),
            "verification": if self.re_enumerated {
                "level 2, re-enumerated: the device left the USB tree when the port went off and came back with a new device number and readable descriptors. This shows the port dropped the device, not that VBUS fell (some hubs disable the data lines and leave VBUS on)"
            } else {
                "level 1 only, claimed: this cycle did not show the device leaving and coming back, so it did not confirm that the port really removes power"
            },
            "notes": self.notes,
        })
    }
}

/// Switches the port off, waits for the device to leave the tree, keeps it off for `off_time` in all, switches it on, and waits for
/// the device to be back with a new device number (and, when `ready` is given, for `ready` to be true) within `back`.
/// An error says which step failed; a failure after the port was switched off says so, because the port is then still off.
pub fn cycle(
    run: &dyn Runner,
    sysfs: &Path,
    port: &PortRef,
    opts: &Opts,
    ready: Option<&mut dyn FnMut() -> bool>,
) -> Result<Report, String> {
    // [@ANCHOR: usb_hub_power_cycle]
    let (off_time, back) = (opts.off_time, opts.back);
    let child = port.child();
    let before = probe(sysfs, &child);
    let expect = opts
        .expect_vidpid
        .clone()
        .or_else(|| before.as_ref().map(|b| b.vidpid.clone()));
    let mut notes = Vec::new();
    let t0 = Instant::now();
    uhubctl(run, port, "off")?;
    let off_done = Instant::now();
    let mut gone = None;
    if before.is_some() {
        if wait_until(opts.gone, || {
            probe(sysfs, &child).is_none_or(|s| Some(s.devnum) != before.as_ref().map(|b| b.devnum))
        }) {
            gone = Some(t0.elapsed().as_secs_f64());
        } else {
            notes.push(format!(
                "the device stayed in the USB tree for {} s after the port was switched off: the hub accepted the command but the port did not drop the device (not a hub that really switches this port)",
                opts.gone.as_secs()
            ));
        }
    }
    let held = off_done.elapsed();
    if held < off_time {
        std::thread::sleep(off_time - held);
    }
    let on_start = Instant::now();
    let off_actual = on_start.duration_since(off_done).as_secs_f64();
    if let Err(first) = uhubctl(run, port, "on") {
        std::thread::sleep(Duration::from_millis(500));
        uhubctl(run, port, "on").map_err(|e| format!("{first}; second try: {e}. THE PORT IS STILL OFF: switch it on again with the port power tool"))?;
        notes.push(format!(
            "the first power-on failed ({first}) and the second worked"
        ));
    }
    let new_enum = |s: &Seen| {
        let changed = before.as_ref().is_none_or(|b| b.devnum != s.devnum);
        changed && expect.as_ref().is_none_or(|e| *e == s.vidpid)
    };
    let mut after = None;
    let back_ok = wait_until(back, || {
        after = probe(sysfs, &child).filter(&new_enum);
        after.is_some()
    });
    let back_after = back_ok.then(|| on_start.elapsed().as_secs_f64());
    let mut ready_after = None;
    if let (true, Some(r)) = (back_ok, ready) {
        let left = back.saturating_sub(on_start.elapsed());
        if wait_until(left, r) {
            ready_after = Some(on_start.elapsed().as_secs_f64());
        } else {
            return Err(format!(
                "the device was back in the USB tree {:.1} s after power-on but was not ready within {} s of power-on on hub {} port {}",
                back_after.unwrap_or(0.0),
                back.as_secs(),
                port.hub,
                port.port
            ));
        }
    }
    if !back_ok {
        let why = match (&before, probe(sysfs, &child)) {
            (Some(b), Some(s)) if s.devnum == b.devnum => {
                "the same device number is still there (the device never left)".to_string()
            }
            (_, Some(s)) => format!(
                "a device {} is there but it is not the expected {}",
                s.vidpid,
                expect.clone().unwrap_or_default()
            ),
            (_, None) => "nothing enumerated".to_string(),
        };
        return Err(format!(
            "the device did not come back within {} s of power-on on hub {} port {}: {why}",
            back.as_secs(),
            port.hub,
            port.port
        ));
    }
    let re_enumerated = gone.is_some() && before.is_some() && after.is_some();
    let report = Report {
        port: port.clone(),
        device_before: before,
        device_after: after,
        off_time_requested_s: off_time.as_secs_f64(),
        off_time_actual_s: off_actual,
        gone_after_s: gone,
        back_after_s: back_after,
        ready_after_s: ready_after,
        re_enumerated,
        notes,
    };
    if let (true, Some(b), Some(a), Some(g), Some(back_s)) = (
        report.re_enumerated,
        &report.device_before,
        &report.device_after,
        gone,
        back_after,
    ) {
        let unix_time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        remember(
            &port.key(),
            Evidence {
                unix_time,
                gone_after_s: g,
                back_after_s: back_s,
                old_devnum: b.devnum,
                new_devnum: a.devnum,
            },
        );
    }
    Ok(report)
}

/// Runs `cycle` for each (off time, wait) pair in turn until one succeeds; the error lists every failure. A unit that fails to
/// enumerate after a short power-off has come back after a longer one.
pub fn cycle_escalating(
    run: &dyn Runner,
    sysfs: &Path,
    port: &PortRef,
    schedule: &[Opts],
    mut ready: Option<&mut dyn FnMut() -> bool>,
) -> Result<Report, String> {
    // [@ANCHOR: usb_hub_power_escalation]
    let mut failures = Vec::new();
    for (i, opts) in schedule.iter().enumerate() {
        let again: Option<&mut dyn FnMut() -> bool> = match ready.as_mut() {
            Some(r) => Some(&mut **r),
            None => None,
        };
        match cycle(run, sysfs, port, opts, again) {
            Ok(mut r) => {
                if i > 0 {
                    r.notes.push(format!(
                        "it took cycle {} (off time {} s) after: {}",
                        i + 1,
                        opts.off_time.as_secs(),
                        failures.join("; ")
                    ));
                }
                return Ok(r);
            }
            Err(e) => failures.push(format!(
                "cycle {} (off {} s): {e}",
                i + 1,
                opts.off_time.as_secs()
            )),
        }
    }
    Err(failures.join("; "))
}

/// Switches a port off or on, and shows what the tree did.
pub fn set(
    run: &dyn Runner,
    sysfs: &Path,
    port: &PortRef,
    on: bool,
    back: Duration,
) -> Result<Value, String> {
    let child = port.child();
    let before = probe(sysfs, &child);
    let t0 = Instant::now();
    uhubctl(run, port, if on { "on" } else { "off" })?;
    let mut note = String::new();
    if !on && before.is_none() {
        note = "no device was on the port".to_string();
    }
    if !on && before.is_some() {
        let gone = wait_until(GONE_TIMEOUT, || {
            probe(sysfs, &child).is_none_or(|s| Some(s.devnum) != before.as_ref().map(|b| b.devnum))
        });
        note = if gone {
            format!(
                "the device left the USB tree {:.1} s after the command",
                t0.elapsed().as_secs_f64()
            )
        } else {
            "the device is still in the USB tree: the port did not drop it".to_string()
        };
    }
    if on {
        let up = wait_until(back, || probe(sysfs, &child).is_some());
        note = if up {
            format!(
                "a device enumerated {:.1} s after the command",
                t0.elapsed().as_secs_f64()
            )
        } else {
            "no device enumerated yet (the port may be empty)".to_string()
        };
    }
    Ok(
        json!({"port": port.key(), "powered": on, "device_before": before.map(|b| json!({"devnum": b.devnum, "vidpid": b.vidpid})), "observed": note}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::usb_hub::testkit::{writable_sys, Behaviour, FakeHub};

    fn port() -> PortRef {
        PortRef {
            hub: "3-2".into(),
            port: 2,
            exact: false,
        }
    }

    fn opts(off_ms: u64, back_ms: u64) -> Opts {
        Opts {
            gone: Duration::from_millis(300),
            ..Opts::new(
                Duration::from_millis(off_ms),
                Duration::from_millis(back_ms),
            )
        }
    }

    // Tests [@ANCHOR: usb_hub_power_cycle]
    #[test]
    fn a_cycle_is_off_then_on_through_sudo_uhubctl_and_proves_re_enumeration() {
        let (_d, sys) = writable_sys();
        let hub = FakeHub::new(&sys, Behaviour::Switches);
        let o = opts(300, 5000);
        let r = cycle(&hub, &sys, &port(), &o, None).unwrap();
        assert_eq!(
            *hub.calls.borrow(),
            [
                "sudo -n uhubctl -N -l 3-2 -p 2 -a off",
                "sudo -n uhubctl -N -l 3-2 -p 2 -a on"
            ]
        );
        let (b, a) = (
            r.device_before.as_ref().unwrap(),
            r.device_after.as_ref().unwrap(),
        );
        assert_eq!(
            (b.vidpid.as_str(), a.vidpid.as_str()),
            ("0451:2036", "0451:2036")
        );
        assert!(a.devnum != b.devnum && r.re_enumerated && r.level() == 2);
        assert!(
            r.off_time_actual_s >= 0.29 && r.off_time_requested_s == 0.3,
            "{r:?}"
        );
        assert!(
            r.gone_after_s.unwrap() <= r.back_after_s.unwrap() + o.off_time.as_secs_f64() + 1.0
        );
        let j = r.json();
        assert_eq!(j["verification_level"], 2);
        assert!(j["verification"]
            .as_str()
            .unwrap()
            .contains("not that VBUS fell"));
        let e = proven("3-2/2").unwrap();
        assert_eq!((e.old_devnum, e.new_devnum), (b.devnum, a.devnum));
    }

    #[test]
    fn a_hub_that_ignores_the_command_is_reported_and_the_cycle_fails_clearly() {
        let (_d, sys) = writable_sys();
        let hub = FakeHub::new(&sys, Behaviour::Ignores);
        let e = cycle(&hub, &sys, &port(), &opts(0, 300), None).unwrap_err();
        assert!(
            e.contains("never left") && e.contains("did not come back"),
            "{e}"
        );
    }

    #[test]
    fn a_failing_power_off_stops_before_power_on() {
        let (_d, sys) = writable_sys();
        let hub = FakeHub::new(&sys, Behaviour::Fails);
        let e = cycle(&hub, &sys, &port(), &opts(0, 300), None).unwrap_err();
        assert!(
            e.contains("uhubctl off") && e.contains("No compatible"),
            "{e}"
        );
        assert_eq!(
            hub.calls.borrow().len(),
            1,
            "no power-on after a failed power-off"
        );
    }

    #[test]
    fn a_ready_check_is_waited_for_after_the_device_is_back() {
        let (_d, sys) = writable_sys();
        let hub = FakeHub::new(&sys, Behaviour::Switches);
        let mut polls = 0;
        let mut ready = || {
            polls += 1;
            polls >= 3
        };
        let r = cycle(&hub, &sys, &port(), &opts(0, 5000), Some(&mut ready)).unwrap();
        assert!(r.ready_after_s.unwrap() >= r.back_after_s.unwrap());
        let (_d2, sys2) = writable_sys();
        let hub2 = FakeHub::new(&sys2, Behaviour::Switches);
        let mut never = || false;
        let e = cycle(&hub2, &sys2, &port(), &opts(0, 400), Some(&mut never)).unwrap_err();
        assert!(e.contains("not ready"), "{e}");
    }

    // Tests [@ANCHOR: usb_hub_power_escalation]
    #[test]
    fn a_unit_that_does_not_come_back_gets_a_second_cycle_with_a_longer_off_time() {
        let (_d, sys) = writable_sys();
        let hub = FakeHub::new(&sys, Behaviour::Switches);
        let mut polls = 0;
        // Not ready during the first cycle's short wait, ready in the second.
        let mut ready = || {
            polls += 1;
            polls >= 3
        };
        let sched = [opts(0, 50), opts(0, 5000)];
        let r = cycle_escalating(&hub, &sys, &port(), &sched, Some(&mut ready)).unwrap();
        assert_eq!(
            hub.calls
                .borrow()
                .iter()
                .filter(|c| c.ends_with("-a off"))
                .count(),
            2,
            "{:?}",
            hub.calls.borrow()
        );
        assert!(r.notes.iter().any(|n| n.contains("cycle 2")), "{r:?}");
        let (_d3, sys3) = writable_sys();
        let hub3 = FakeHub::new(&sys3, Behaviour::Ignores);
        let e = cycle_escalating(&hub3, &sys3, &port(), &sched[..1], None).unwrap_err();
        assert!(e.contains("cycle 1"), "{e}");
    }

    #[test]
    fn set_off_reports_the_device_leaving_the_tree() {
        let (_d, sys) = writable_sys();
        let hub = FakeHub::new(&sys, Behaviour::Switches);
        let v = set(&hub, &sys, &port(), false, DEFAULT_BACK_TIMEOUT).unwrap();
        assert_eq!(v["powered"], false);
        assert!(
            v["observed"]
                .as_str()
                .unwrap()
                .contains("left the USB tree"),
            "{v}"
        );
        assert!(probe(&sys, "3-2.2").is_none());
        let v = set(&hub, &sys, &port(), true, DEFAULT_BACK_TIMEOUT).unwrap();
        assert!(
            v["observed"].as_str().unwrap().contains("enumerated"),
            "{v}"
        );
    }

    #[test]
    fn the_exact_flag_reaches_the_uhubctl_command() {
        let (_d, sys) = writable_sys();
        let hub = FakeHub::new(&sys, Behaviour::Ignores);
        let p = PortRef {
            hub: "2-3".into(),
            port: 4,
            exact: true,
        };
        set(&hub, &sys, &p, false, DEFAULT_BACK_TIMEOUT).unwrap();
        assert_eq!(
            *hub.calls.borrow(),
            ["sudo -n uhubctl -N -l 2-3 -p 4 -a off -e"]
        );
    }

    #[test]
    fn root_hub_ports_are_named_by_bus_number_for_uhubctl() {
        let p = PortRef {
            hub: "usb3".into(),
            port: 4,
            exact: false,
        };
        assert_eq!(
            (p.location().as_str(), p.child().as_str(), p.key().as_str()),
            ("3", "3-4", "usb3/4")
        );
    }
}
