// SPDX-License-Identifier: AGPL-3.0-or-later
//! The soak harness's USB topology comparison: only the unit's own part of `lsusb -t` counts (to-do e6af16bc). Documented in `README.md`.

use crate::power::HubPort;

/// What a later `lsusb -t` shows compared with the first one.
#[derive(Clone, Debug, PartialEq)]
pub enum Change {
    // [@ANCHOR: vnwa_open_soak_topology_change]
    /// Nothing differs anywhere.
    None,
    /// Something differs outside the unit's part of the tree only (another device plugged in or removed): information, not a failure.
    OutsideOnly,
    /// The unit's part differs (a codec missing, a different port, a changed speed or driver): an event. Both texts are the normalised unit subtrees.
    Unit { first: String, now: String },
}

/// `lsusb -t` with the device numbers (`Dev 17, `) removed, because they change at every enumeration and the tree must not.
pub fn normalise(lsusb_t: &str) -> String {
    // [@ANCHOR: vnwa_open_soak_topology_normalise]
    lsusb_t
        .lines()
        .map(|l| match l.find("Dev ") {
            Some(i) => match l[i..].find(',') {
                Some(j) => format!("{}{}", &l[..i], &l[i + j + 1..]),
                None => l.to_string(),
            },
            None => l.to_string(),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The part of `lsusb -t` that belongs to the unit behind `hub`: the root hub line of its bus (the controller), the line or lines of every hub on the way down to the
/// switched port, and the device at that port with everything below it, all normalised. `None` if the bus (or the hub name) is not in the text at all.
pub fn unit_subtree(lsusb_t: &str, hub: &HubPort) -> Option<String> {
    // [@ANCHOR: vnwa_open_soak_topology_unit_subtree]
    // The hub is named like uhubctl does: `<bus>` then `-<port>` and `.<port>` for each level, for example `3-2` or `3-2.1`.
    let (bus, ports) = hub.hub.split_once('-')?;
    let bus: u32 = bus.parse().ok()?;
    let mut target: Vec<u32> = ports
        .split('.')
        .map(|p| p.parse().ok())
        .collect::<Option<_>>()?;
    target.push(hub.port as u32);
    let mut in_bus = false;
    let mut found = false;
    let mut chain: Vec<u32> = Vec::new();
    let mut keep = Vec::new();
    for line in normalise(lsusb_t).lines() {
        if let Some(rest) = line.strip_prefix("/:") {
            let n = rest.trim().strip_prefix("Bus ").and_then(|b| b.get(..3));
            in_bus = n.and_then(|n| n.parse::<u32>().ok()) == Some(bus);
            chain.clear();
            if in_bus {
                found = true;
                keep.push(line.to_string());
            }
            continue;
        }
        if !in_bus {
            continue;
        }
        // Four columns per level: `    |__ Port 002: ...` is level 1.
        let Some(at) = line.find("|__ Port ") else {
            continue;
        };
        let port: Option<u32> = line[at + 9..]
            .split(':')
            .next()
            .and_then(|p| p.trim().parse().ok());
        let Some(port) = port else { continue };
        chain.truncate((at / 4).saturating_sub(1));
        chain.push(port);
        let common = chain.len().min(target.len());
        if chain[..common] == target[..common] {
            keep.push(line.to_string());
        }
    }
    found.then(|| keep.join("\n"))
}

/// Compares two `lsusb -t` texts for the unit behind `hub`: `Unit` if its part differs, `OutsideOnly` if only the rest of the tree differs, `None` if the whole tree is the same.
/// A tree that does not contain the unit's bus at all counts as a unit change when the other one does contain it.
pub fn compare(first: &str, now: &str, hub: &HubPort) -> Change {
    // [@ANCHOR: vnwa_open_soak_topology_compare]
    let (f, n) = (unit_subtree(first, hub), unit_subtree(now, hub));
    if f != n {
        return Change::Unit {
            first: f.unwrap_or_else(|| "(bus not present)".into()),
            now: n.unwrap_or_else(|| "(bus not present)".into()),
        };
    }
    if normalise(first) != normalise(now) {
        Change::OutsideOnly
    } else {
        Change::None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const START: &str = include_str!("../tests/soak_fixtures/lsusb_start.txt");
    const CYCLE_2: &str = include_str!("../tests/soak_fixtures/lsusb_cycle_2.txt");

    fn hub() -> HubPort {
        HubPort {
            hub: "3-2".into(),
            port: 2,
        }
    }

    // Tests [@ANCHOR: vnwa_open_soak_topology_normalise]
    #[test]
    fn device_numbers_are_removed_and_the_rest_of_the_line_is_kept() {
        let n = normalise("    |__ Port 002: Dev 017, If 0, Class=Hub, Driver=hub/5p, 480M\n/:  Bus 003.Port 001: Dev 001, Class=root_hub, Driver=xhci_hcd/12p, 480M");
        assert_eq!(
            n,
            "    |__ Port 002:  If 0, Class=Hub, Driver=hub/5p, 480M\n/:  Bus 003.Port 001:  Class=root_hub, Driver=xhci_hcd/12p, 480M"
        );
        assert_eq!(normalise("no device number here"), "no device number here");
    }

    // Tests [@ANCHOR: vnwa_open_soak_topology_unit_subtree]
    #[test]
    fn the_unit_subtree_is_its_controller_its_hubs_and_the_branch_below_the_port() {
        let s = unit_subtree(START, &hub()).unwrap();
        let lines: Vec<&str> = s.lines().collect();
        assert!(lines[0].starts_with("/:  Bus 003.Port 001:"), "{s}");
        assert!(
            lines[1].contains("Port 002:  If 0, Class=Hub, Driver=hub/5p"),
            "{s}"
        );
        assert!(
            lines[2].contains("Port 002:  If 0, Class=Hub, Driver=hub/3p"),
            "{s}"
        );
        assert_eq!(
            lines.iter().filter(|l| l.contains("snd-usb-audio")).count(),
            6,
            "the two codecs' audio interfaces: {s}"
        );
        assert!(
            s.contains("Port 001:  If 0, Class=[unknown], Driver=[none], 1.5M"),
            "{s}"
        );
        // Nothing of the dock, the other hub ports, other buses or the Bluetooth radio.
        for gone in [
            "r8152",
            "uvcvideo",
            "btusb",
            "Bus 001",
            "Bus 002",
            "Bus 004",
            "Port 003:  If 0, Class=Hub",
        ] {
            assert!(!s.contains(gone), "{gone} in {s}");
        }
        // A hub or bus that is not there.
        assert_eq!(
            unit_subtree(
                START,
                &HubPort {
                    hub: "9-2".into(),
                    port: 2
                }
            ),
            None
        );
        assert_eq!(
            unit_subtree(
                START,
                &HubPort {
                    hub: "x".into(),
                    port: 2
                }
            ),
            None
        );
    }

    // Tests [@ANCHOR: vnwa_open_soak_topology_compare]
    // Tests [@ANCHOR: vnwa_open_soak_topology_change]
    #[test]
    fn removing_the_dock_is_not_an_event_but_a_missing_codec_or_another_port_is() {
        // The recorded pair of the second soak: the dock (r8152 Ethernet, webcam, keyboard and mouse hubs) was unplugged between the start and cycle 2.
        assert_ne!(
            normalise(START),
            normalise(CYCLE_2),
            "the whole trees do differ"
        );
        assert_eq!(compare(START, CYCLE_2, &hub()), Change::OutsideOnly);
        assert_eq!(compare(START, START, &hub()), Change::None);
        // Device numbers alone change nothing.
        let renumbered = START.replace("Dev ", "Dev 9");
        assert_eq!(compare(START, &renumbered, &hub()), Change::None);
        // A codec missing (the 08bb:29c0 at port 3 of the unit's hub).
        let no_codec: String = CYCLE_2
            .lines()
            .filter(|l| !l.contains("Port 003: Dev") || !l.contains("snd-usb-audio"))
            .collect::<Vec<_>>()
            .join("\n");
        match compare(START, &no_codec, &hub()) {
            Change::Unit { first, now } => assert!(first.len() > now.len()),
            c => panic!("{c:?}"),
        }
        // The same devices on a different port of the hub.
        let moved = CYCLE_2.replace("        |__ Port 002: Dev", "        |__ Port 003: Dev");
        assert!(matches!(
            compare(START, &moved, &hub()),
            Change::Unit { .. }
        ));
        // A changed negotiated speed on a codec.
        let slow = START.replacen(
            "Class=Audio, Driver=snd-usb-audio, 12M",
            "Class=Audio, Driver=snd-usb-audio, 1.5M",
            1,
        );
        assert!(matches!(compare(START, &slow, &hub()), Change::Unit { .. }));
        // The unit's bus vanishing is a unit change.
        let no_bus: String = START
            .lines()
            .filter(|l| !l.contains("Bus 003") && !l.starts_with("    "))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(matches!(
            compare(START, &no_bus, &hub()),
            Change::Unit { .. }
        ));
    }
}
