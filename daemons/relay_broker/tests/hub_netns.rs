// SPDX-License-Identifier: AGPL-3.0-or-later
//! The emergency hub's configuration run against the real `dnsmasq` and `hostapd` (RELAY_STANDALONE_EMERGENCY_HUB.md, phase 1). No Wi-Fi: a veth pair
//! between two network namespaces stands in for the radio link. `dnsmasq` runs in the hub namespace from the generated `dnsmasq.conf`; a client
//! namespace asks for a DHCP lease and then resolves names against the hub. `hostapd` cannot start without a radio, so its generated file is only parsed
//! (it fails on the missing interface, never on a configuration line). `sudo` is used for `ip netns` only.
//!
//! Skipped unless `HAMS_HUB_NETNS=1` (it needs passwordless `sudo ip`, `/usr/sbin/dnsmasq` and `/usr/sbin/hostapd`). Run it as a bounded job:
//! `agent_procs.py run --tag NAME hubns -- timeout 120 env HAMS_HUB_NETNS=1 cargo test -p relay_broker --test hub_netns`.
#![allow(clippy::expect_used, clippy::unwrap_used)]

mod common;

use std::net::Ipv4Addr;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use common::temp_dir;
use relay_broker::hub::{dnsmasq_conf, hostapd_conf, HubSettings};

const HUB_NS: &str = "hamshub_ap";
const CLIENT_NS: &str = "hamshub_cl";

/// Runs `sudo -n ip <args>` and returns its stdout and stderr.
fn ip(args: &[&str]) -> (bool, String) {
    let out = Command::new("sudo")
        .arg("-n")
        .arg("ip")
        .args(args)
        .output()
        .expect("run sudo ip");
    let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&out.stderr));
    (out.status.success(), text)
}

/// Runs a command inside a namespace and returns its success and output.
fn in_ns(ns: &str, cmd: &[&str]) -> (bool, String) {
    let mut args = vec!["netns", "exec", ns];
    args.extend_from_slice(cmd);
    ip(&args)
}

/// Both namespaces (and with them the veth pair) and the `dnsmasq` child, removed when the test ends or fails.
struct Lab {
    dnsmasq: Option<Child>,
}

impl Lab {
    fn teardown(&mut self) {
        if let Some(mut child) = self.dnsmasq.take() {
            // `sudo` relays SIGTERM to `dnsmasq`; SIGKILL would orphan it.
            let _ = Command::new("kill").arg(child.id().to_string()).status();
            let _ = child.wait();
        }
        for ns in [HUB_NS, CLIENT_NS] {
            let _ = ip(&["netns", "del", ns]);
        }
    }
}

impl Drop for Lab {
    fn drop(&mut self) {
        self.teardown();
    }
}

/// A raw DHCP DISCOVER from the client namespace; prints `yiaddr`, option 3, 6, 42 and the lease time one per line.
const DHCP_PROBE: &str = r#"
import socket, struct, sys
s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
s.setsockopt(socket.SOL_SOCKET, socket.SO_BROADCAST, 1)
s.setsockopt(socket.SOL_SOCKET, socket.SO_BINDTODEVICE, b"hubcl0")
s.bind(("0.0.0.0", 68))
# A client with no address cannot receive the offer on a UDP socket; read the frames the way a DHCP client does.
raw = socket.socket(socket.AF_PACKET, socket.SOCK_RAW, socket.htons(0x0800))
raw.bind(("hubcl0", 0))
raw.settimeout(3)
mac = bytes.fromhex("020000aabb01")
pkt = struct.pack("!BBBBIHH4s4s4s4s16s64s128s", 1, 1, 6, 0, 0x1234, 0, 0x8000,
                  b"\0" * 4, b"\0" * 4, b"\0" * 4, b"\0" * 4, mac + b"\0" * 10, b"", b"")
pkt += bytes([99, 130, 83, 99, 53, 1, 1, 55, 4, 3, 6, 42, 51, 255])
s.sendto(pkt, ("255.255.255.255", 67))
while True:
    frame = raw.recv(2000)
    ihl = (frame[14] & 15) * 4
    if frame[23] == 17 and struct.unpack("!H", frame[14 + ihl + 2:14 + ihl + 4])[0] == 68:
        data = frame[14 + ihl + 8:]
        break
print("yiaddr=" + socket.inet_ntoa(data[16:20]))
i = 240
while i < len(data) and data[i] != 255:
    code, n = data[i], data[i + 1]
    body = data[i + 2:i + 2 + n]
    if code in (3, 6, 42):
        print("opt%d=%s" % (code, socket.inet_ntoa(body[:4])))
    if code == 51:
        print("lease=%d" % struct.unpack("!I", body)[0])
    i += 2 + n
"#;

fn settings() -> HubSettings {
    HubSettings {
        interface: "hubap0".into(),
        ssid: "HAMS-EMERGENCY-TEST".into(),
        passphrase: "correct horse battery".into(),
        channel: 6,
        five_ghz: false,
        hub_address: Ipv4Addr::new(192, 168, 50, 1),
        relay_name: "relay".into(),
        uplink_dns: None,
        lease_minutes: 60,
    }
}

/// `dig +short` of `name` against the hub from the client namespace, one answer per line.
fn dig(name: &str) -> String {
    let (_, out) = in_ns(
        CLIENT_NS,
        &[
            "dig",
            "+short",
            "+time=2",
            "+tries=1",
            "@192.168.50.1",
            name,
            "A",
        ],
    );
    out.trim().to_string()
}

#[test]
fn the_generated_hub_config_runs_in_real_dnsmasq_and_parses_in_real_hostapd() {
    if std::env::var("HAMS_HUB_NETNS").as_deref() != Ok("1") {
        eprintln!("skipped: set HAMS_HUB_NETNS=1 (needs sudo ip, dnsmasq and hostapd)");
        return;
    }
    let dir = temp_dir("hub_netns");
    let s = settings();

    // hostapd: the generated file is accepted; a deliberately broken one is not (so the check can fail).
    let good = dir.join("hostapd.conf");
    std::fs::write(&good, hostapd_conf(&s).unwrap()).unwrap();
    let run_hostapd = |path: &std::path::Path| {
        let out = Command::new("timeout")
            .args(["10", "/usr/sbin/hostapd"])
            .arg(path)
            .output()
            .expect("run hostapd");
        String::from_utf8_lossy(&out.stdout).into_owned() + &String::from_utf8_lossy(&out.stderr)
    };
    let text = run_hostapd(&good);
    assert!(
        !text.contains("unknown configuration item") && !text.contains("errors found"),
        "hostapd rejected the generated file:\n{text}"
    );
    assert!(
        text.contains("Could not read interface hubap0") || text.contains("nl80211"),
        "hostapd did not get as far as the radio:\n{text}"
    );
    let bad = dir.join("bad.conf");
    std::fs::write(&bad, format!("{}bogus_item=1\n", hostapd_conf(&s).unwrap())).unwrap();
    assert!(run_hostapd(&bad).contains("unknown configuration item"));

    // The namespaces and the link between them.
    let mut lab = Lab { dnsmasq: None };
    lab.teardown();
    for step in [
        vec!["netns", "add", HUB_NS],
        vec!["netns", "add", CLIENT_NS],
        vec![
            "link", "add", "hubap0", "netns", HUB_NS, "type", "veth", "peer", "name", "hubcl0",
            "netns", CLIENT_NS,
        ],
    ] {
        let (ok, text) = ip(&step);
        assert!(ok, "ip {step:?} failed: {text}");
    }
    for cmd in [
        &["ip", "addr", "add", "192.168.50.1/24", "dev", "hubap0"][..],
        &["ip", "link", "set", "hubap0", "up"],
        &["ip", "link", "set", "lo", "up"],
    ] {
        let (ok, text) = in_ns(HUB_NS, cmd);
        assert!(ok, "{cmd:?} in the hub namespace failed: {text}");
    }
    for cmd in [
        &["ip", "link", "set", "hubcl0", "up"][..],
        &["ip", "link", "set", "lo", "up"],
    ] {
        let (ok, text) = in_ns(CLIENT_NS, cmd);
        assert!(ok, "{cmd:?} in the client namespace failed: {text}");
    }

    // dnsmasq with the generated file, in the foreground so the test owns the process.
    let conf = dir.join("dnsmasq.conf");
    std::fs::write(&conf, dnsmasq_conf(&s).unwrap()).unwrap();
    let log = dir.join("dnsmasq.log");
    lab.dnsmasq = Some(
        Command::new("sudo")
            .args([
                "-n",
                "ip",
                "netns",
                "exec",
                HUB_NS,
                "/usr/sbin/dnsmasq",
                "-k",
            ])
            .arg(format!("--conf-file={}", conf.display()))
            .arg("--pid-file=")
            .arg("--dhcp-leasefile=/dev/null")
            .arg(format!("--log-facility={}", log.display()))
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("start dnsmasq"),
    );

    // DHCP: poll until dnsmasq answers.
    let started = Instant::now();
    let lease = loop {
        let (ok, text) = in_ns(CLIENT_NS, &["python3", "-I", "-c", DHCP_PROBE]);
        if ok {
            break text;
        }
        assert!(
            started.elapsed() < Duration::from_secs(15),
            "no DHCP offer from dnsmasq: {text}"
        );
        std::thread::sleep(Duration::from_millis(500));
    };
    let get = |key: &str| {
        lease
            .lines()
            .find_map(|l| l.strip_prefix(&format!("{key}=")))
            .unwrap_or_else(|| panic!("{key} missing from the offer:\n{lease}"))
            .to_string()
    };
    let offered: Ipv4Addr = get("yiaddr").parse().unwrap();
    assert_eq!(offered.octets()[..3], [192, 168, 50], "{lease}");
    assert!((20..=250).contains(&offered.octets()[3]), "{lease}");
    assert_eq!(get("opt3"), "192.168.50.1", "router");
    assert_eq!(get("opt6"), "192.168.50.1", "DNS server");
    assert_eq!(get("opt42"), "192.168.50.1", "NTP server");
    assert_eq!(get("lease"), "3600", "lease seconds");

    // DNS from a client that took the lease.
    let (ok, text) = in_ns(
        CLIENT_NS,
        &[
            "ip",
            "addr",
            "add",
            &format!("{offered}/24"),
            "dev",
            "hubcl0",
        ],
    );
    assert!(ok, "client address: {text}");
    assert_eq!(dig("relay"), "192.168.50.1");
    assert_eq!(dig("relay.local"), "192.168.50.1");
    for probe in [
        "connectivitycheck.gstatic.com",
        "clients3.google.com",
        "captive.apple.com",
        "www.msftconnecttest.com",
        "detectportal.firefox.com",
    ] {
        assert_eq!(dig(probe), "192.168.50.1", "{probe}");
    }
    assert_eq!(
        dig("example.invalid"),
        "",
        "other names are refused with no uplink"
    );
}
