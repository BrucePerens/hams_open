// SPDX-License-Identifier: AGPL-3.0-or-later
//! The kernel's USB event history (`journalctl -k -o short-unix`): the connect, disconnect and enumeration timeline, USB error
//! counters, reset counts and flapping detection per port; and a live watch that polls the sysfs tree.
//!
//! The kernel journal is readable only by root and the groups `adm` and `systemd-journal`. Without that the tool fails with a message
//! naming the fix; `use_sudo: true` opts in to `sudo -n journalctl` for one call.

use super::sysfs::{child_path, Tree};
use super::uhubctl::{sudo, Runner};
use super::Config;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::Path;
use std::time::{Duration, Instant};

#[derive(Clone, Debug, PartialEq)]
pub struct Event {
    pub ts: f64,
    pub kind: &'static str,
    /// The device path the line is about (`3-2.2.1`); for a port message the device that would be on the port.
    pub path: Option<String>,
    pub errno: Option<i32>,
    pub devnum: Option<u32>,
    pub text: String,
}

/// `3-2.2-port1` is port 1 of hub `3-2.2`, so the device `3-2.2.1`; `usb3-port2` is root-hub port 2, device `3-2`.
fn port_target(tok: &str) -> Option<String> {
    // [@ANCHOR: bench_mcp_usb_hub_history_port_target]
    let (hub, port) = tok.rsplit_once("-port")?;
    let port: u32 = port.trim_end_matches(':').parse().ok()?;
    Some(child_path(hub, port))
}

fn errno_of(msg: &str) -> Option<i32> {
    // [@ANCHOR: bench_mcp_usb_hub_history_errno_of]
    for key in ["error ", "err = ", "err ", "error=", "status "] {
        let mut from = 0;
        while let Some(i) = msg[from..].find(key) {
            let rest = &msg[from + i + key.len()..];
            if let Some(num) = rest.strip_prefix('-') {
                let digits: String = num.chars().take_while(|c| c.is_ascii_digit()).collect();
                if let Ok(n) = digits.parse::<i32>() {
                    return Some(-n);
                }
            }
            from += i + key.len();
        }
    }
    None
}

fn devnum_of(msg: &str) -> Option<u32> {
    let i = msg.find("number ")?;
    msg[i + 7..]
        .split(|c: char| !c.is_ascii_digit())
        .next()?
        .parse()
        .ok()
}

/// Parses one journal line into an event, when it is about USB.
pub fn parse_line(line: &str) -> Option<Event> {
    // [@ANCHOR: usb_hub_history_parse]
    let (head, msg) = line.split_once(" kernel: ")?;
    let ts: f64 = head.split_whitespace().next()?.parse().ok()?;
    let mut words = msg.split_whitespace();
    let first = words.next()?;
    let (path, rest) = match first {
        "usb" | "hub" => {
            let tok = words.next()?;
            let rest = msg.splitn(3, ' ').nth(2).unwrap_or("");
            if let Some(p) = port_target(tok) {
                (Some(p), rest)
            } else {
                (
                    Some(tok.trim_end_matches(':').split(':').next()?.to_string()),
                    rest,
                )
            }
        }
        s if s.starts_with("xhci_hcd")
            || s.starts_with("ehci")
            || s.starts_with("ohci")
            || s.starts_with("uhci") =>
        {
            (None, msg)
        }
        _ => return None,
    };
    let path = path.filter(|p| {
        p.chars()
            .next()
            .is_some_and(|c| c.is_ascii_digit() || p.starts_with("usb"))
    });
    let errno = errno_of(rest);
    let low = rest.to_ascii_lowercase();
    let kind = if low.contains("new ") && low.contains("usb device number") {
        "connect"
    } else if low.starts_with("new usb device found") {
        "found"
    } else if low.starts_with("usb disconnect") {
        "disconnect"
    } else if low.starts_with("reset ") && low.contains("usb device number") {
        "reset"
    } else if low.contains("unable to enumerate") {
        "enumeration_failed"
    } else if low.contains("device descriptor read") {
        "descriptor_read_error"
    } else if low.contains("not accepting address")
        || low.contains("not responding to setup address")
    {
        "address_error"
    } else if low.contains("attempt power cycle") {
        "power_cycle_attempt"
    } else if low.contains("over-current") || low.contains("overcurrent") {
        "over_current"
    } else if low.contains("cannot submit urb") && errno == Some(-19) {
        "urb_no_device"
    } else if low.contains("disabled by hub") || low.contains("babble") {
        "hub_port_error"
    } else if first.starts_with("xhci")
        && (low.contains("error")
            || low.contains("died")
            || low.contains("warn")
            || low.contains("not responding"))
    {
        "xhci_error"
    } else if errno.is_some() {
        "error"
    } else if low.contains("usb hub found")
        || low.contains("ports detected")
        || low.contains("manufacturer:")
        || low.contains("product:")
        || low.contains("serialnumber")
        || low.contains("new usb device strings")
    {
        "info"
    } else {
        return None;
    };
    Some(Event {
        ts,
        kind,
        path,
        errno,
        devnum: devnum_of(rest),
        text: rest.trim().to_string(),
    })
}

pub fn parse(text: &str) -> Vec<Event> {
    // [@ANCHOR: bench_mcp_usb_hub_history_parse]
    let mut v: Vec<Event> = text.lines().filter_map(parse_line).collect();
    v.sort_by(|a, b| a.ts.total_cmp(&b.ts));
    v
}

/// Options for `analyze`.
pub struct Query {
    pub device: Option<String>,
    pub since: Option<f64>,
    pub limit: usize,
    pub flap_window_s: f64,
    pub flap_connects: usize,
}

/// A device that connected `connects` times within `window_s`, starting at `from`.
fn flaps(times: &[f64], window_s: f64, min: usize) -> Option<(usize, f64)> {
    // [@ANCHOR: bench_mcp_usb_hub_history_flaps]
    let mut best: Option<(usize, f64)> = None;
    let mut lo = 0;
    for hi in 0..times.len() {
        while times[hi] - times[lo] > window_s {
            lo += 1;
        }
        let n = hi - lo + 1;
        if n >= min && best.is_none_or(|b| n > b.0) {
            best = Some((n, times[lo]));
        }
    }
    best
}

fn under(path: &str, dev: &str) -> bool {
    path == dev || path.starts_with(&format!("{dev}.")) || path.starts_with(&format!("{dev}:"))
}

/// The summary of the events selected by the query.
pub fn analyze(events: &[Event], q: &Query) -> Value {
    // [@ANCHOR: bench_mcp_usb_hub_history_analyze]
    let sel: Vec<&Event> = events
        .iter()
        .filter(|e| e.kind != "info" || q.device.is_some())
        .filter(|e| q.since.is_none_or(|s| e.ts >= s))
        .filter(|e| {
            q.device
                .as_ref()
                .is_none_or(|d| e.path.as_ref().is_some_and(|p| under(p, d)))
        })
        .collect();
    let mut per: BTreeMap<String, BTreeMap<&str, u64>> = BTreeMap::new();
    let mut errnos: BTreeMap<String, BTreeMap<i32, u64>> = BTreeMap::new();
    let mut connects: BTreeMap<String, Vec<f64>> = BTreeMap::new();
    let mut global: BTreeMap<&str, u64> = BTreeMap::new();
    for e in &sel {
        *global.entry(e.kind).or_default() += 1;
        let key = e.path.clone().unwrap_or_else(|| "(controller)".into());
        *per.entry(key.clone())
            .or_default()
            .entry(e.kind)
            .or_default() += 1;
        if let (Some(n), true) = (e.errno, e.kind != "info") {
            *errnos.entry(key.clone()).or_default().entry(n).or_default() += 1;
        }
        if e.kind == "connect" {
            connects.entry(key).or_default().push(e.ts);
        }
    }
    let flapping: Vec<Value> = connects
        .iter()
        .filter_map(|(p, t)| flaps(t, q.flap_window_s, q.flap_connects).map(|(n, from)| json!({"device": p, "connects_in_window": n, "window_s": q.flap_window_s, "first_at": from, "total_connects": t.len()})))
        .collect();
    let counters: Vec<Value> = per
        .iter()
        .map(|(p, k)| {
            let n = |kind: &str| k.get(kind).copied().unwrap_or(0);
            json!({
                "device": p, "connects": n("connect"), "disconnects": n("disconnect"), "resets": n("reset"),
                "enumeration_failures": n("enumeration_failed"), "descriptor_read_errors": n("descriptor_read_error"), "address_errors": n("address_error"),
                "power_cycle_attempts_by_the_kernel": n("power_cycle_attempt"), "over_current": n("over_current"),
                "urb_no_device_minus_19": n("urb_no_device"), "xhci_errors": n("xhci_error"), "other_errors": n("error"), "hub_port_errors": n("hub_port_error"),
                "errno_counts": errnos.get(p).map(|m| m.iter().map(|(k, v)| (k.to_string(), json!(v))).collect::<serde_json::Map<_, _>>()),
            })
        })
        .collect();
    let skip = sel.len().saturating_sub(q.limit);
    let timeline: Vec<Value> = sel.iter().skip(skip).map(|e| json!({"t": e.ts, "kind": e.kind, "device": e.path, "errno": e.errno, "devnum": e.devnum, "text": e.text})).collect();
    json!({
        "events_considered": sel.len(), "first_t": sel.first().map(|e| e.ts), "last_t": sel.last().map(|e| e.ts),
        "totals": global, "per_device": counters, "flapping": flapping,
        "flapping_rule": format!("{} or more connects of one device within {} s", q.flap_connects, q.flap_window_s),
        "timeline": timeline, "timeline_truncated_to_last": if skip > 0 { Some(q.limit) } else { None },
    })
}

const NO_ACCESS: &str = "the kernel journal is not readable by this user: add the user to the group systemd-journal (or adm), or pass use_sudo: true to use `sudo -n journalctl -k` for this call";

/// Reads the kernel journal text.
pub fn journal(
    run: &dyn Runner,
    since_unix: Option<f64>,
    all_boots: bool,
    use_sudo: bool,
) -> Result<String, String> {
    // [@ANCHOR: bench_mcp_usb_hub_history_journal]
    let mut args: Vec<String> = ["-k", "-o", "short-unix", "--no-pager", "-q"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    if !all_boots {
        args.push("-b".into());
    }
    if let Some(s) = since_unix {
        args.push(format!("--since=@{}", s.max(0.0) as u64));
    }
    if use_sudo {
        let a: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
        return sudo(run, "journalctl", &a);
    }
    let (ok, text) = run.run("journalctl", &args)?;
    if !ok
        || text.contains("No journal files were opened due to insufficient permissions")
        || text.contains("Users in groups 'adm', 'systemd-journal'") && text.lines().count() < 6
    {
        return Err(NO_ACCESS.to_string());
    }
    Ok(text)
}

fn num(args: &Value, k: &str) -> Result<Option<f64>, String> {
    // [@ANCHOR: bench_mcp_usb_hub_history_num]
    match args.get(k) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v
            .as_f64()
            .filter(|n| n.is_finite() && *n >= 0.0)
            .map(Some)
            .ok_or_else(|| format!("{k} must be a non-negative number")),
    }
}

/// `bench_usb_history`.
pub fn history(
    run: &dyn Runner,
    sysfs: &Path,
    cfg: &Config,
    now: f64,
    args: &Value,
) -> Result<Value, String> {
    // [@ANCHOR: bench_mcp_usb_hub_history_history]
    let device = match args.get("device").and_then(|v| v.as_str()) {
        Some(d) => {
            let t = Tree::read(sysfs)?;
            Some(super::resolve_device(&t, cfg, d).or_else(|e| {
                if d.chars().next().is_some_and(|c| c.is_ascii_digit()) {
                    Ok(d.to_string())
                } else {
                    Err(e)
                }
            })?)
        }
        None => None,
    };
    let since = num(args, "since_s")?.map(|s| now - s);
    let limit = num(args, "max_events")?
        .map(|n| n as usize)
        .unwrap_or(100)
        .clamp(1, 1000);
    let q = Query {
        device,
        since,
        limit,
        flap_window_s: num(args, "flap_window_s")?.unwrap_or(120.0).max(1.0),
        flap_connects: num(args, "flap_connects")?
            .map(|n| n as usize)
            .unwrap_or(3)
            .max(2),
    };
    let all = args
        .get("all_boots")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let sudo_ok = args
        .get("use_sudo")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let text = journal(run, since, all, sudo_ok)?;
    let events = parse(&text);
    let mut v = analyze(&events, &q);
    v["source"] = json!(if sudo_ok {
        "sudo -n journalctl -k"
    } else {
        "journalctl -k"
    });
    v["note"] = json!("-19 urb lines mean something kept submitting to a device that was already gone; -71 is a protocol error on the link (cable, hub, power or a failing device)");
    Ok(v)
}

/// What the tree shows for one device, for the watch diff.
#[derive(Clone, Debug, PartialEq)]
struct Snap {
    devnum: u32,
    vidpid: String,
    speed: String,
    authorized: bool,
}

type Snaps = (BTreeMap<String, Snap>, BTreeMap<String, (String, u64)>);

fn snapshot(t: &Tree) -> Snaps {
    // [@ANCHOR: bench_mcp_usb_hub_history_snapshot]
    let devs = t
        .devices
        .values()
        .map(|d| {
            (
                d.path.clone(),
                Snap {
                    devnum: d.devnum,
                    vidpid: d.vidpid(),
                    speed: d.speed.clone(),
                    authorized: d.authorized,
                },
            )
        })
        .collect();
    let ports = t
        .devices
        .values()
        .flat_map(|d| {
            d.ports.iter().map(|p| {
                (
                    format!("{}/{}", d.path, p.number),
                    (p.state.clone(), p.over_current_count),
                )
            })
        })
        .collect();
    (devs, ports)
}

/// The differences between two snapshots as events.
fn diff(at: f64, a: &Snaps, b: &Snaps) -> Vec<Value> {
    // [@ANCHOR: bench_mcp_usb_hub_history_diff]
    let mut out = Vec::new();
    for (p, s) in &b.0 {
        match a.0.get(p) {
            None => out.push(json!({"t": at, "event": "appeared", "device": p, "vidpid": s.vidpid, "devnum": s.devnum, "speed_mbps": s.speed})),
            Some(o) if o.devnum != s.devnum => out.push(json!({"t": at, "event": "re-enumerated", "device": p, "devnum_before": o.devnum, "devnum_after": s.devnum, "vidpid": s.vidpid})),
            Some(o) if o.speed != s.speed => out.push(json!({"t": at, "event": "speed changed", "device": p, "from": o.speed, "to": s.speed})),
            Some(o) if o.authorized != s.authorized => out.push(json!({"t": at, "event": "authorized changed", "device": p, "to": s.authorized})),
            _ => {}
        }
    }
    for (p, s) in &a.0 {
        if !b.0.contains_key(p) {
            out.push(json!({"t": at, "event": "disappeared", "device": p, "vidpid": s.vidpid, "devnum": s.devnum}));
        }
    }
    for (p, (st, oc)) in &b.1 {
        if let Some((ost, ooc)) = a.1.get(p) {
            if ost != st {
                out.push(json!({"t": at, "event": "port state changed", "port": p, "from": ost, "to": st}));
            }
            if ooc != oc {
                out.push(json!({"t": at, "event": "over-current count rose", "port": p, "from": ooc, "to": oc}));
            }
        }
    }
    out
}

/// `bench_usb_watch`: polls the tree for `seconds` and reports what changed, with the time of each change and the number of
/// connects per device (a device that connects again and again is flapping).
pub fn watch(sysfs: &Path, args: &Value) -> Result<Value, String> {
    // [@ANCHOR: usb_hub_watch]
    let secs = num(args, "seconds")?.unwrap_or(10.0);
    if !(1.0..=120.0).contains(&secs) {
        return Err("seconds must be 1 to 120".into());
    }
    let every = Duration::from_millis(
        num(args, "interval_ms")?
            .unwrap_or(200.0)
            .clamp(50.0, 5000.0) as u64,
    );
    let start = Instant::now();
    let unix0 = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0);
    let mut prev = snapshot(&Tree::read(sysfs)?);
    let mut events = Vec::new();
    while start.elapsed().as_secs_f64() < secs {
        std::thread::sleep(every);
        let cur = snapshot(&Tree::read(sysfs)?);
        events.extend(diff(start.elapsed().as_secs_f64(), &prev, &cur));
        prev = cur;
    }
    let mut appeared: BTreeMap<String, u64> = BTreeMap::new();
    for e in &events {
        if e["event"] == "appeared" || e["event"] == "re-enumerated" {
            *appeared
                .entry(e["device"].as_str().unwrap_or_default().to_string())
                .or_default() += 1;
        }
    }
    let flapping: Vec<&String> = appeared
        .iter()
        .filter(|(_, n)| **n >= 3)
        .map(|(p, _)| p)
        .collect();
    Ok(json!({
        "watched_s": start.elapsed().as_secs_f64(), "started_unix": unix0, "poll_ms": every.as_millis() as u64,
        "events": events, "enumerations_per_device": appeared, "flapping": flapping,
        "method": "polling /sys/bus/usb/devices; a change shorter than the poll interval can be missed, the kernel journal (bench_usb_history) is exact",
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::usb_hub::testkit::{recorded, writable_sys, Behaviour, FakeHub};

    fn fx() -> String {
        std::fs::read_to_string(recorded().join("kernel_journal.txt")).unwrap()
    }

    fn q() -> Query {
        Query {
            device: None,
            since: None,
            limit: 50,
            flap_window_s: 120.0,
            flap_connects: 3,
        }
    }

    // Tests [@ANCHOR: usb_hub_history_parse]
    // Tests [@ANCHOR: bench_mcp_usb_hub_history_errno_of]
    // Tests [@ANCHOR: bench_mcp_usb_hub_history_port_target]
    #[test]
    fn lines_are_classified_with_device_and_error_numbers() {
        let l = |m: &str| parse_line(&format!("1793289140.5 devbox kernel: {m}")).unwrap();
        let e = l("usb 3-2.2.2: new full-speed USB device number 38 using xhci_hcd");
        assert_eq!(
            (e.kind, e.path.as_deref(), e.devnum),
            ("connect", Some("3-2.2.2"), Some(38))
        );
        assert_eq!(
            l("usb 3-2.2.2: USB disconnect, device number 38").kind,
            "disconnect"
        );
        let e = l("usb 3-2.2-port1: unable to enumerate USB device");
        assert_eq!(
            (e.kind, e.path.as_deref()),
            ("enumeration_failed", Some("3-2.2.1"))
        );
        let e = l("usb 3-2.2.1: device descriptor read/64, error -71");
        assert_eq!((e.kind, e.errno), ("descriptor_read_error", Some(-71)));
        let e = l("usb 3-2.2.3: cannot submit urb 0, error -19: no device");
        assert_eq!((e.kind, e.errno), ("urb_no_device", Some(-19)));
        assert_eq!(
            l("usb usb3-port2: over-current condition").path.as_deref(),
            Some("3-2")
        );
        assert_eq!(
            l("usb 3-2.2-port1: attempt power cycle").kind,
            "power_cycle_attempt"
        );
        assert_eq!(
            l("usb 3-2.2.1: reset full-speed USB device number 37 using xhci_hcd").kind,
            "reset"
        );
        assert_eq!(
            l("hub 3-2:1.0: hub_ext_port_status failed (err = -71)").errno,
            Some(-71)
        );
        assert!(parse_line("1793289140.5 devbox kernel: audit: type=1400").is_none());
        assert!(parse_line("garbage").is_none());
    }

    // Tests [@ANCHOR: bench_mcp_usb_hub_history_analyze]
    // Tests [@ANCHOR: bench_mcp_usb_hub_history_parse]
    #[test]
    fn the_recorded_journal_shows_the_vnwa_chain_flapping_and_its_errors() {
        let ev = parse(&fx());
        assert!(ev.len() > 100, "{}", ev.len());
        let calm = analyze(&ev, &q());
        assert!(
            calm["flapping"].as_array().unwrap().is_empty(),
            "power cycles minutes apart are not flapping"
        );
        let mut wide = q();
        wide.flap_window_s = 300.0;
        let v = analyze(&ev, &wide);
        assert!(
            v["totals"]["connect"].as_u64().unwrap() > 5,
            "{}",
            v["totals"]
        );
        assert!(
            v["totals"]["enumeration_failed"].as_u64().unwrap() >= 4,
            "{}",
            v["totals"]
        );
        let flap: Vec<_> = v["flapping"]
            .as_array()
            .unwrap()
            .iter()
            .map(|f| f["device"].as_str().unwrap().to_string())
            .collect();
        assert!(flap.iter().any(|d| d.starts_with("3-2.2")), "{flap:?}");
        let p1 = v["per_device"]
            .as_array()
            .unwrap()
            .iter()
            .find(|d| d["device"] == "3-2.2.1")
            .unwrap();
        assert!(
            p1["enumeration_failures"].as_u64().unwrap() >= 1
                && p1["power_cycle_attempts_by_the_kernel"].as_u64().unwrap() >= 1,
            "{p1}"
        );
        let e71: u64 = v["per_device"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|d| d["errno_counts"]["-71"].as_u64())
            .sum();
        assert!(e71 > 0, "-71 errors counted");
        assert!(v["timeline"].as_array().unwrap().len() <= 50);
    }

    #[test]
    fn a_device_filter_includes_its_subtree_and_a_window_limits_the_events() {
        let ev = parse(&fx());
        let mut qq = q();
        qq.device = Some("3-2.2.2".into());
        let v = analyze(&ev, &qq);
        assert!(v["per_device"]
            .as_array()
            .unwrap()
            .iter()
            .all(|d| d["device"] == "3-2.2.2"));
        qq.device = Some("3-2".into());
        let all = analyze(&ev, &qq);
        assert!(
            all["per_device"].as_array().unwrap().len() > 1,
            "a hub's filter includes the devices below it"
        );
        qq.since = Some(ev.last().unwrap().ts + 1.0);
        assert_eq!(analyze(&ev, &qq)["events_considered"], 0);
        assert!(!under("3-22", "3-2") && under("3-2.2", "3-2"));
    }

    // Tests [@ANCHOR: bench_mcp_usb_hub_history_flaps]
    #[test]
    fn flapping_needs_enough_connects_inside_the_window() {
        assert_eq!(flaps(&[0.0, 10.0, 20.0], 30.0, 3), Some((3, 0.0)));
        assert_eq!(flaps(&[0.0, 100.0, 200.0], 30.0, 3), None);
        assert_eq!(flaps(&[], 30.0, 3), None);
    }

    struct Denied;
    impl Runner for Denied {
        fn run(&self, p: &str, a: &[String]) -> Result<(bool, String), String> {
            if p == "sudo" {
                assert!(a.contains(&"journalctl".to_string()));
                return Ok((
                    true,
                    std::fs::read_to_string(recorded().join("kernel_journal.txt")).unwrap(),
                ));
            }
            Ok((
                true,
                "No journal files were opened due to insufficient permissions.\n".into(),
            ))
        }
    }

    // Tests [@ANCHOR: bench_mcp_usb_hub_history_history]
    // Tests [@ANCHOR: bench_mcp_usb_hub_history_journal]
    // Tests [@ANCHOR: bench_mcp_usb_hub_history_num]
    #[test]
    fn an_unreadable_journal_fails_with_the_fix_and_sudo_is_opt_in() {
        let (_d, sys) = writable_sys();
        let c = Config::default();
        let e = history(&Denied, &sys, &c, 0.0, &json!({})).unwrap_err();
        assert!(
            e.contains("systemd-journal") && e.contains("use_sudo"),
            "{e}"
        );
        let v = history(
            &Denied,
            &sys,
            &c,
            0.0,
            &json!({"use_sudo": true, "max_events": 5}),
        )
        .unwrap();
        assert_eq!(v["source"], "sudo -n journalctl -k");
        assert_eq!(v["timeline"].as_array().unwrap().len(), 5);
        assert!(history(
            &Denied,
            &sys,
            &c,
            0.0,
            &json!({"since_s": -1, "use_sudo": true})
        )
        .is_err());
    }

    // Tests [@ANCHOR: usb_hub_watch]
    // Tests [@ANCHOR: bench_mcp_usb_hub_history_diff]
    // Tests [@ANCHOR: bench_mcp_usb_hub_history_snapshot]
    #[test]
    fn a_watch_reports_a_device_leaving_and_returning_with_a_new_number() {
        let (_d, sys) = writable_sys();
        let hub = FakeHub::new(&sys, Behaviour::Switches);
        let t = std::thread::spawn({
            let sys = sys.clone();
            move || watch(&sys, &json!({"seconds": 2, "interval_ms": 50})).unwrap()
        });
        std::thread::sleep(Duration::from_millis(400));
        let off: Vec<String> = ["-N", "-l", "3-2", "-p", "2", "-a", "off"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        hub.run("sudo", &off).unwrap();
        std::thread::sleep(Duration::from_millis(400));
        let on: Vec<String> = ["-N", "-l", "3-2", "-p", "2", "-a", "on"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        hub.run("sudo", &on).unwrap();
        let v = t.join().unwrap();
        let kinds: Vec<_> = v["events"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|e| e["device"] == "3-2.2")
            .map(|e| e["event"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(kinds, ["disappeared", "appeared"], "{v}");
        assert_eq!(v["enumerations_per_device"]["3-2.2"], 1);
        assert!(watch(&sys, &json!({"seconds": 500})).is_err());
    }
}
