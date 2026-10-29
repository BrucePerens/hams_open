// SPDX-License-Identifier: AGPL-3.0-or-later
//! `sudo -n uhubctl` and `sudo -n lsusb -v`: what only root can read (the hub class descriptor and the raw port status words)
//! and the one program that switches port power. Every call uses `sudo -n`, so a missing sudoers rule is an error and never
//! a password prompt.

use serde_json::{json, Value};
use std::process::Command;

/// Runs a program and gives back its success flag and its output; production is a process, tests script it.
pub trait Runner {
    fn run(&self, program: &str, args: &[String]) -> Result<(bool, String), String>;
}

pub struct ProcessRunner;

impl Runner for ProcessRunner {
    fn run(&self, program: &str, args: &[String]) -> Result<(bool, String), String> {
        // [@ANCHOR: bench_mcp_usb_hub_uhubctl_processrunner_run]
        let out = Command::new(program).args(args).output().map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                format!("{program} is not installed (not found on PATH)")
            } else {
                format!("{program}: {e}")
            }
        })?;
        let mut text = String::from_utf8_lossy(&out.stdout).to_string();
        text.push_str(&String::from_utf8_lossy(&out.stderr));
        Ok((out.status.success(), text))
    }
}

/// Turns a failed `sudo -n <tool>` into a message that says what to fix.
pub fn explain_failure(tool: &str, text: &str) -> String {
    // [@ANCHOR: bench_mcp_usb_hub_uhubctl_explain_failure]
    let t = text.trim();
    if t.contains("a password is required") || t.contains("a terminal is required") {
        return format!("sudo -n {tool} needs a password: add a NOPASSWD sudoers rule for {tool} for this user ({t})");
    }
    if t.contains("command not found") || t.contains("not found") && t.contains(tool) {
        return format!("{tool} is not installed (sudo could not find it): {t}");
    }
    if t.contains("not in the sudoers") || t.contains("is not allowed to execute") {
        return format!("sudo does not allow this user to run {tool}: {t}");
    }
    format!("sudo -n {tool} failed: {t}")
}

pub fn sudo(run: &dyn Runner, tool: &str, args: &[&str]) -> Result<String, String> {
    // [@ANCHOR: bench_mcp_usb_hub_uhubctl_sudo]
    let mut a: Vec<String> = vec!["-n".into(), tool.into()];
    a.extend(args.iter().map(|s| s.to_string()));
    let (ok, text) = run.run("sudo", &a).map_err(|e| {
        if e.starts_with("sudo") {
            format!("sudo is not installed: {e}")
        } else {
            e
        }
    })?;
    if ok {
        Ok(text)
    } else {
        Err(explain_failure(tool, &text))
    }
}

/// How uhubctl describes a hub's power switching (its `lpsm` field).
#[derive(Clone, Debug, PartialEq)]
pub enum Switching {
    PerPort,
    Ganged,
    None,
    Unknown(String),
}

impl Switching {
    pub fn name(&self) -> String {
        match self {
            Switching::PerPort => "per-port".into(),
            Switching::Ganged => "ganged".into(),
            Switching::None => "none".into(),
            Switching::Unknown(s) => format!("unknown ({s})"),
        }
    }
}

/// The link state of a SuperSpeed port (bits 5 to 8 of its status word).
pub fn link_state(n: u16) -> &'static str {
    // [@ANCHOR: bench_mcp_usb_hub_uhubctl_link_state]
    match n {
        0 => "U0",
        1 => "U1",
        2 => "U2",
        3 => "U3 (suspended)",
        4 => "SS.Disabled",
        5 => "Rx.Detect",
        6 => "SS.Inactive",
        7 => "Polling",
        8 => "Recovery",
        9 => "Hot Reset",
        10 => "Compliance",
        11 => "Loopback",
        _ => "reserved",
    }
}

/// A port status word decoded. USB 2 and USB 3 hubs lay the word out differently.
#[derive(Clone, Debug, PartialEq)]
pub struct PortBits {
    pub word: u16,
    pub powered: bool,
    pub connected: bool,
    pub enabled: bool,
    pub suspended: Option<bool>,
    pub over_current: bool,
    pub reset: bool,
    pub speed: &'static str,
    pub link_state: Option<&'static str>,
}

pub fn decode_port(word: u16, usb3: bool) -> PortBits {
    // [@ANCHOR: bench_mcp_usb_hub_uhubctl_decode_port]
    let bit = |n: u16| word & (1 << n) != 0;
    if usb3 {
        let speed = match (word >> 10) & 7 {
            0 => "SuperSpeed 5 Gb/s",
            1 => "SuperSpeed+ 10 Gb/s",
            _ => "unknown",
        };
        PortBits {
            word,
            powered: bit(9),
            connected: bit(0),
            enabled: bit(1),
            suspended: None,
            over_current: bit(3),
            reset: bit(4),
            speed,
            link_state: Some(link_state((word >> 5) & 0xf)),
        }
    } else {
        let speed = if bit(9) {
            "low-speed"
        } else if bit(10) {
            "high-speed"
        } else if bit(0) {
            "full-speed"
        } else {
            "none"
        };
        PortBits {
            word,
            powered: bit(8),
            connected: bit(0),
            enabled: bit(1),
            suspended: Some(bit(2)),
            over_current: bit(3),
            reset: bit(4),
            speed,
            link_state: None,
        }
    }
}

impl PortBits {
    pub fn json(&self) -> Value {
        json!({
            "status_word": format!("{:04x}", self.word), "powered": self.powered, "connected": self.connected, "enabled": self.enabled,
            "suspended": self.suspended, "over_current": self.over_current, "reset": self.reset, "speed": self.speed, "link_state": self.link_state,
        })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct UPort {
    pub number: u32,
    pub bits: PortBits,
    pub flags: Vec<String>,
    pub device_vidpid: Option<String>,
    pub device_text: Option<String>,
}

/// One hub block of `uhubctl` output.
#[derive(Clone, Debug, PartialEq)]
pub struct UHub {
    pub location: String,
    pub vidpid: String,
    pub description: Option<String>,
    pub usb_version: String,
    pub ports: u32,
    pub switching: Switching,
    pub port_list: Vec<UPort>,
}

fn switching_of(s: &str) -> Switching {
    // [@ANCHOR: bench_mcp_usb_hub_uhubctl_switching_of]
    match s {
        "ppps" => Switching::PerPort,
        "ganged" => Switching::Ganged,
        "nops" => Switching::None,
        o => Switching::Unknown(o.to_string()),
    }
}

/// The bracketed part of a hub line: `14b0:013a Generic 4-Port USB 2.0 Hub, USB 2.10, 5 ports, ppps`.
fn bracket(s: &str) -> Option<(String, Option<String>, String, u32, Switching)> {
    // [@ANCHOR: bench_mcp_usb_hub_uhubctl_bracket]
    let parts: Vec<&str> = s.split(", ").collect();
    if parts.len() < 4 {
        return None;
    }
    let n = parts.len();
    let (first, ver, ports, mode) = (parts[0], parts[n - 3], parts[n - 2], parts[n - 1]);
    let ver = ver.strip_prefix("USB ")?;
    let ports: u32 = ports.strip_suffix(" ports")?.parse().ok()?;
    let (vidpid, desc) = match first.split_once(' ') {
        Some((a, b)) => (a, Some(b.to_string())),
        None => (first, None),
    };
    if vidpid.len() != 9 || vidpid.as_bytes()[4] != b':' {
        return None;
    }
    Some((
        vidpid.to_string(),
        desc,
        ver.to_string(),
        ports,
        switching_of(mode),
    ))
}

/// Parses the output of `uhubctl` without options (or with `-N`).
pub fn parse_status(text: &str) -> Result<Vec<UHub>, String> {
    // [@ANCHOR: usb_hub_uhubctl_parse]
    let mut hubs: Vec<UHub> = Vec::new();
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("Current status for hub ") {
            let (loc, br) = rest
                .split_once(" [")
                .ok_or_else(|| format!("unreadable hub line: {line}"))?;
            let br = br
                .strip_suffix(']')
                .ok_or_else(|| format!("unreadable hub line: {line}"))?;
            let (vidpid, description, usb_version, ports, switching) =
                bracket(br).ok_or_else(|| format!("unreadable hub line: {line}"))?;
            hubs.push(UHub {
                location: loc.to_string(),
                vidpid,
                description,
                usb_version,
                ports,
                switching,
                port_list: vec![],
            });
            continue;
        }
        let t = line.trim();
        let Some(rest) = t.strip_prefix("Port ") else {
            continue;
        };
        let hub = hubs
            .last_mut()
            .ok_or_else(|| format!("port line before any hub: {line}"))?;
        let (num, rest) = rest
            .split_once(": ")
            .ok_or_else(|| format!("unreadable port line: {line}"))?;
        let (word, rest) = rest.split_once(' ').unwrap_or((rest, ""));
        let word = u16::from_str_radix(word, 16)
            .map_err(|_| format!("unreadable port status in: {line}"))?;
        let (flags, dev) = match rest.split_once(" [") {
            Some((f, d)) => (f, Some(d.trim_end_matches(']').to_string())),
            None => (rest, None),
        };
        let usb3 = hub.usb_version.starts_with('3');
        hub.port_list.push(UPort {
            number: num
                .parse()
                .map_err(|_| format!("unreadable port number in: {line}"))?,
            bits: decode_port(word, usb3),
            flags: flags.split_whitespace().map(String::from).collect(),
            device_vidpid: dev.as_ref().map(|d| d.chars().take(9).collect()),
            device_text: dev,
        });
    }
    if hubs.is_empty() {
        return Err(format!("no hubs in uhubctl output: {}", text.trim()));
    }
    Ok(hubs)
}

/// Runs `sudo -n uhubctl -N` (no device descriptions, so an unresponsive device cannot stall it) and parses it.
pub fn status(run: &dyn Runner) -> Result<Vec<UHub>, String> {
    let text = sudo(run, "uhubctl", &["-N"])?;
    parse_status(&text)
}

/// The hub class descriptor fields only `lsusb -v` as root shows.
#[derive(Clone, Debug, PartialEq)]
pub struct HubDescriptor {
    pub characteristics: u16,
    pub ports: u32,
    pub power_on_to_good_ms: u32,
    pub controller_current_ma: u32,
}

impl HubDescriptor {
    /// wHubCharacteristics bits 0 and 1.
    pub fn switching(&self) -> &'static str {
        // [@ANCHOR: bench_mcp_usb_hub_uhubctl_hubdescriptor_switching]
        match self.characteristics & 3 {
            0 => "ganged",
            1 => "per-port",
            _ => "none",
        }
    }
    pub fn compound(&self) -> bool {
        self.characteristics & 4 != 0
    }
    /// Bits 3 and 4.
    pub fn over_current(&self) -> &'static str {
        // [@ANCHOR: bench_mcp_usb_hub_uhubctl_hubdescriptor_over_current]
        match (self.characteristics >> 3) & 3 {
            0 => "global",
            1 => "per-port",
            _ => "none",
        }
    }
    /// Bits 5 and 6, in full-speed bit times (USB 2 hubs).
    pub fn think_time_fs_bits(&self) -> u32 {
        8 * (1 + ((self.characteristics >> 5) & 3) as u32)
    }
    pub fn port_indicators(&self) -> bool {
        self.characteristics & 0x80 != 0
    }
}

pub fn parse_hub_descriptor(text: &str) -> Option<HubDescriptor> {
    // [@ANCHOR: bench_mcp_usb_hub_uhubctl_parse_hub_descriptor]
    let mut at = text
        .lines()
        .skip_while(|l| !l.starts_with("Hub Descriptor:"));
    at.next()?;
    let mut d = HubDescriptor {
        characteristics: 0,
        ports: 0,
        power_on_to_good_ms: 0,
        controller_current_ma: 0,
    };
    let mut seen = false;
    for l in at {
        if !l.starts_with(' ') {
            break;
        }
        let t = l.trim();
        let num = |rest: &str| {
            rest.split_whitespace()
                .next()
                .and_then(|n| n.parse::<u32>().ok())
                .unwrap_or(0)
        };
        if let Some(r) = t.strip_prefix("wHubCharacteristic") {
            d.characteristics =
                u16::from_str_radix(r.split_whitespace().next()?.trim_start_matches("0x"), 16)
                    .ok()?;
            seen = true;
        } else if let Some(r) = t.strip_prefix("nNbrPorts") {
            d.ports = num(r);
        } else if let Some(r) = t.strip_prefix("bPwrOn2PwrGood") {
            d.power_on_to_good_ms = 2 * num(r);
        } else if let Some(r) = t.strip_prefix("bHubContrCurrent") {
            d.controller_current_ma = num(r);
        }
    }
    seen.then_some(d)
}

/// `sudo -n lsusb -v -s <bus>:<devnum>` and its hub descriptor.
pub fn hub_descriptor(run: &dyn Runner, bus: u32, devnum: u32) -> Result<HubDescriptor, String> {
    let sel = format!("{bus}:{devnum}");
    let text = sudo(run, "lsusb", &["-v", "-s", &sel])?;
    parse_hub_descriptor(&text).ok_or_else(|| format!("lsusb printed no hub descriptor for {sel}"))
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use std::path::Path;

    pub fn fx(name: &str) -> String {
        std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/usb_fixtures/devbox")
                .join(name),
        )
        .unwrap()
    }

    // Tests [@ANCHOR: usb_hub_uhubctl_parse]
    // Tests [@ANCHOR: bench_mcp_usb_hub_uhubctl_bracket]
    // Tests [@ANCHOR: bench_mcp_usb_hub_uhubctl_switching_of]
    #[test]
    fn the_recorded_uhubctl_output_parses_with_and_without_descriptions() {
        for name in ["uhubctl.txt", "uhubctl_nodesc.txt"] {
            let h = parse_status(&fx(name)).unwrap();
            assert_eq!(h.len(), 2, "{name}");
            assert_eq!(
                (
                    h[0].location.as_str(),
                    h[0].vidpid.as_str(),
                    h[0].ports,
                    h[0].switching.clone()
                ),
                ("3-2", "14b0:013a", 5, Switching::PerPort)
            );
            assert_eq!(h[0].usb_version, "2.10");
            let p2 = &h[0].port_list[1];
            assert!(
                p2.bits.powered && p2.bits.connected && p2.bits.enabled && !p2.bits.over_current,
                "{p2:?}"
            );
            assert_eq!(p2.device_vidpid.as_deref(), Some("0451:2036"));
            assert!(p2.device_text.as_deref().unwrap().ends_with("ganged"));
            assert_eq!(p2.bits.speed, "full-speed");
            assert!(!h[0].port_list[0].bits.connected && h[0].port_list[0].bits.powered);
            let ss = &h[1].port_list[0];
            assert_eq!(
                (ss.bits.link_state, ss.bits.speed, ss.bits.powered),
                (Some("Rx.Detect"), "SuperSpeed 5 Gb/s", true)
            );
        }
        assert_eq!(
            parse_status(&fx("uhubctl.txt")).unwrap()[0]
                .description
                .as_deref(),
            Some("Generic 4-Port USB 2.0 Hub")
        );
    }

    // Tests [@ANCHOR: bench_mcp_usb_hub_uhubctl_decode_port]
    // Tests [@ANCHOR: bench_mcp_usb_hub_uhubctl_link_state]
    #[test]
    fn port_words_decode_for_usb2_and_usb3() {
        let b = decode_port(0x0503, false);
        assert!(
            b.powered
                && b.connected
                && b.enabled
                && b.speed == "high-speed"
                && b.suspended == Some(false)
        );
        let oc = decode_port(0x010b, false);
        assert!(oc.over_current && oc.powered);
        assert_eq!(decode_port(0x0107, false).suspended, Some(true));
        let u3 = decode_port(0x0203, true);
        assert!(u3.powered && u3.connected && u3.link_state == Some("U0"));
        assert_eq!(decode_port(0x02e0, true).link_state, Some("Polling"));
    }

    #[test]
    fn bad_output_is_an_error_not_an_empty_list() {
        assert!(parse_status(
            "There were permission problems while accessing USB.\nNo compatible devices detected!"
        )
        .is_err());
        assert!(parse_status("Current status for hub 3-2 [garbage]\n").is_err());
        assert!(parse_status("  Port 1: 0100 power\n").is_err());
    }

    // Tests [@ANCHOR: bench_mcp_usb_hub_uhubctl_hubdescriptor_over_current]
    // Tests [@ANCHOR: bench_mcp_usb_hub_uhubctl_hubdescriptor_switching]
    // Tests [@ANCHOR: bench_mcp_usb_hub_uhubctl_parse_hub_descriptor]
    #[test]
    fn the_hub_descriptor_from_lsusb_gives_switching_overcurrent_and_think_time() {
        let d = parse_hub_descriptor(&fx("lsusb_v_hub_3-2.txt")).unwrap();
        assert_eq!((d.characteristics, d.ports), (0x00a9, 5));
        assert_eq!(
            (d.switching(), d.over_current(), d.think_time_fs_bits()),
            ("per-port", "per-port", 16)
        );
        assert!(d.port_indicators() && !d.compound());
        let s = parse_hub_descriptor(&fx("lsusb_v_hub_2-3.txt")).unwrap();
        assert_eq!(
            (s.switching(), s.over_current(), s.ports),
            ("per-port", "per-port", 4)
        );
        assert!(
            parse_hub_descriptor("Couldn't open device, some information will be missing\n")
                .is_none()
        );
    }

    struct Scripted(bool, &'static str);
    impl Runner for Scripted {
        fn run(&self, _: &str, _: &[String]) -> Result<(bool, String), String> {
            Ok((self.0, self.1.to_string()))
        }
    }

    // Tests [@ANCHOR: bench_mcp_usb_hub_uhubctl_processrunner_run]
    // Tests [@ANCHOR: bench_mcp_usb_hub_uhubctl_explain_failure]
    // Tests [@ANCHOR: bench_mcp_usb_hub_uhubctl_sudo]
    #[test]
    fn sudo_failures_say_what_to_fix() {
        let e = sudo(
            &Scripted(false, "sudo: a password is required"),
            "uhubctl",
            &[],
        )
        .unwrap_err();
        assert!(e.contains("NOPASSWD") && e.contains("uhubctl"), "{e}");
        let e = sudo(
            &Scripted(false, "sudo: uhubctl: command not found"),
            "uhubctl",
            &[],
        )
        .unwrap_err();
        assert!(e.contains("not installed"), "{e}");
        let e = sudo(
            &Scripted(
                false,
                "Sorry, user x is not allowed to execute '/sbin/uhubctl'",
            ),
            "uhubctl",
            &[],
        )
        .unwrap_err();
        assert!(e.contains("does not allow"), "{e}");
        assert!(sudo(&Scripted(true, "ok"), "uhubctl", &[]).is_ok());
        let missing = ProcessRunner
            .run("definitely-not-a-program-xyz", &[])
            .unwrap_err();
        assert!(missing.contains("not installed"), "{missing}");
    }
}
