// SPDX-License-Identifier: AGPL-3.0-or-later
//! Is a VNWA codec visible to the desktop sound server? A sound server that manages the codecs can make one the default output (system sounds would
//! play into the analyzer) and can hold it open during a sweep. The check reads the sound server's own device list (`pw-dump` for PipeWire, `pactl list
//! cards` for PulseAudio) and reports a card as exposed only if it is a VNWA codec by topology (`reserved::is_vnwa_codec`), not merely a 08bb:29c0.
//! Nothing here changes the sound server.

use std::path::Path;

use crate::reserved::{vnwa_codec_paths, CODEC_PID, CODEC_VID};

/// One card the sound server lists with the codec chip's USB id.
#[derive(Clone, Debug, PartialEq)]
pub struct Card {
    pub description: String,
    /// The USB port path from the card's bus path, for example `2.2.2` from `pci-0000:00:14.0-usb-0:2.2.2:1.0`.
    pub port: Option<String>,
    /// The sound server's id of the card (the PipeWire object id), for `wpctl set-profile`.
    pub id: Option<u64>,
}

/// What the check found.
#[derive(Clone, Debug, PartialEq)]
pub enum Exposure {
    /// The sound server runs and lists none of the VNWA's codecs.
    Protected,
    /// The sound server lists these VNWA codecs.
    Exposed(Vec<Card>),
    /// Could not tell, and why (no sound server, no VNWA plugged in, tool missing).
    Unknown(String),
}

fn port_of(bus_path: &str) -> Option<String> {
    let rest = bus_path.split("-usb-").nth(1)?;
    let after_bus = rest.split_once(':')?.1;
    Some(after_bus.split(':').next()?.to_string())
}

/// The codec-chip cards in a `pw-dump` document.
pub fn parse_pw_dump(json: &str) -> Result<Vec<Card>, String> {
    // [@ANCHOR: vnwa_open_sound_server_pipewire]
    let v: serde_json::Value =
        serde_json::from_str(json).map_err(|e| format!("pw-dump output is not JSON: {e}"))?;
    let objects = v.as_array().ok_or("pw-dump output is not a list")?;
    let mut cards = Vec::new();
    for o in objects {
        let Some(props) = o.pointer("/info/props") else {
            continue;
        };
        let get = |k: &str| props.get(k).and_then(|x| x.as_str()).map(str::to_lowercase);
        if get("device.vendor.id").as_deref() == Some(&format!("0x{CODEC_VID}"))
            && get("device.product.id").as_deref() == Some(&format!("0x{CODEC_PID}"))
        {
            cards.push(Card {
                description: props
                    .get("device.description")
                    .and_then(|x| x.as_str())
                    .unwrap_or("USB audio codec")
                    .to_string(),
                port: props
                    .get("device.bus-path")
                    .and_then(|x| x.as_str())
                    .and_then(port_of),
                id: o.get("id").and_then(|x| x.as_u64()),
            });
        }
    }
    Ok(cards)
}

/// The codec-chip cards in the text of `pactl list cards`.
pub fn parse_pactl_cards(text: &str) -> Vec<Card> {
    // [@ANCHOR: vnwa_open_sound_server_pulse]
    let mut cards = Vec::new();
    for block in text.split("Card #").skip(1) {
        let val = |key: &str| {
            block.lines().find_map(|l| {
                let l = l.trim();
                l.strip_prefix(key)?
                    .trim_start()
                    .strip_prefix('=')
                    .map(|v| v.trim().trim_matches('"').to_string())
            })
        };
        if val("device.vendor.id").map(|v| v.to_lowercase()) == Some(format!("0x{CODEC_VID}"))
            && val("device.product.id").map(|v| v.to_lowercase()) == Some(format!("0x{CODEC_PID}"))
        {
            cards.push(Card {
                description: val("device.description").unwrap_or_else(|| "USB audio codec".into()),
                port: val("device.bus_path").as_deref().and_then(port_of),
                id: block.split_whitespace().next().and_then(|n| n.parse().ok()),
            });
        }
    }
    cards
}

/// Which of the listed cards are VNWA codecs: those on the USB port of a codec that has the control chip beside it.
pub fn judge(cards: Vec<Card>, vnwa_usb_paths: &[String]) -> Exposure {
    let ports: Vec<&str> = vnwa_usb_paths
        .iter()
        .filter_map(|p| p.split_once('-').map(|x| x.1))
        .collect();
    if ports.is_empty() {
        return Exposure::Unknown("no VNWA codecs are plugged in".into());
    }
    let hit: Vec<Card> = cards
        .into_iter()
        .filter(|c| c.port.as_deref().is_some_and(|p| ports.contains(&p)))
        .collect();
    if hit.is_empty() {
        Exposure::Protected
    } else {
        Exposure::Exposed(hit)
    }
}

/// Runs the sound server's listing and judges it. `run` executes a program and returns its output, or `None` if it is not installed or failed.
pub fn check(sys: &Path, run: &dyn Fn(&str, &[&str]) -> Option<String>) -> Exposure {
    let paths = vnwa_codec_paths(sys);
    if let Some(out) = run("pw-dump", &[]) {
        return match parse_pw_dump(&out) {
            Ok(cards) => judge(cards, &paths),
            Err(e) => Exposure::Unknown(e),
        };
    }
    if let Some(out) = run("pactl", &["list", "cards"]) {
        return judge(parse_pactl_cards(&out), &paths);
    }
    Exposure::Unknown(
        "neither pw-dump (PipeWire) nor pactl (PulseAudio) answered: no sound server to inspect"
            .into(),
    )
}

/// The program runner used in production.
pub fn run_program(program: &str, args: &[&str]) -> Option<String> {
    let out = std::process::Command::new(program)
        .args(args)
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).to_string())
}

/// The plain statement for a report: `Some(true)` protected, `Some(false)` exposed, `None` unknown.
pub fn protected(e: &Exposure) -> Option<bool> {
    match e {
        Exposure::Protected => Some(true),
        Exposure::Exposed(_) => Some(false),
        Exposure::Unknown(_) => None,
    }
}

/// What to tell the user when the codecs are exposed.
pub fn warning(e: &Exposure) -> Option<String> {
    match e {
        Exposure::Exposed(cards) => Some(format!(
            "the sound server lists the VNWA's codec(s) ({}): system sounds can be played into the analyzer and a sweep can find the card busy. Fix: run `vnwa_open install-system-config --apply` (a rule that takes effect at the next login or after replugging the unit), or now, without touching other clients: `wpctl set-profile <id> 0` for each card id ({}) to switch the card off in PipeWire.",
            cards.iter().map(|c| c.description.clone()).collect::<Vec<_>>().join(", "),
            cards.iter().filter_map(|c| c.id).map(|i| i.to_string()).collect::<Vec<_>>().join(", ")
        )),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reserved::fixtures::{tmp, tree};

    // A device object as pw-dump prints them (modelled on the real object of a USB camera on this machine, with the codec's ids and a bus path
    // of the unit under test); the other object is an unrelated PCI sound card.
    const PW_DUMP: &str = r#"[
      {"id":63,"type":"PipeWire:Interface:Device","info":{"props":{"device.vendor.id":"0x046d","device.product.id":"0x082d","device.bus-path":"pci-0000:00:14.0-usb-0:1:1.2","device.description":"HD Pro Webcam C920"}}},
      {"id":77,"type":"PipeWire:Interface:Device","info":{"props":{"device.vendor.id":"0x08bb","device.product.id":"0x29c0","device.bus-path":"pci-0000:00:14.0-usb-0:2.2.2:1.0","device.description":"USB AUDIO  CODEC"}}},
      {"id":78,"type":"PipeWire:Interface:Device","info":{"props":{"device.vendor.id":"0x08BB","device.product.id":"0x29C0","device.bus-path":"pci-0000:00:14.0-usb-0:5:1.0","device.description":"A dongle elsewhere"}}},
      {"id":65,"type":"PipeWire:Interface:Core"}
    ]"#;

    const PACTL: &str = "Card #3\n\tName: alsa_card.usb-BurrBrown_USB_AUDIO_CODEC-00\n\tProperties:\n\t\tdevice.bus_path = \"pci-0000:00:14.0-usb-0:2.2.3:1.0\"\n\t\tdevice.vendor.id = \"08bb\"\nCard #4\n\tProperties:\n\t\tdevice.vendor.id = \"0x08bb\"\n\t\tdevice.product.id = \"0x29c0\"\n\t\tdevice.bus_path = \"pci-0000:00:14.0-usb-0:2.2.3:1.0\"\n\t\tdevice.description = \"USB AUDIO  CODEC\"\n";

    fn unit_tree(tag: &str) -> std::path::PathBuf {
        let root = tmp(tag);
        tree(
            &root,
            &[
                ("3-2.2.1", "20a0", "4118"),
                ("3-2.2.2", "08bb", "29c0"),
                ("3-2.2.3", "08bb", "29c0"),
                ("3-5", "08bb", "29c0"),
            ],
        );
        // find_cards needs class/sound/cardN/{id,device -> .../<usb>/<usb>:1.0}
        for (n, usb) in [(1, "3-2.2.2"), (2, "3-2.2.3")] {
            let card = root.join(format!("class/sound/card{n}"));
            std::fs::create_dir_all(&card).unwrap();
            std::fs::write(card.join("id"), format!("CODEC{n}\n")).unwrap();
            let iface = root
                .join("bus/usb/devices")
                .join(usb)
                .join(format!("{usb}:1.0"));
            std::fs::create_dir_all(&iface).unwrap();
            std::os::unix::fs::symlink(&iface, card.join("device")).unwrap();
        }
        root
    }

    // Tests [@ANCHOR: vnwa_open_sound_server_pipewire]
    #[test]
    fn a_pw_dump_with_the_unit_codec_is_exposed_but_a_dongle_elsewhere_is_not_the_units() {
        let cards = parse_pw_dump(PW_DUMP).unwrap();
        assert_eq!(cards.len(), 2);
        assert_eq!(cards[0].port.as_deref(), Some("2.2.2"));
        assert_eq!(cards[0].id, Some(77));
        let root = unit_tree("sound_pw");
        let paths = vnwa_codec_paths(&root);
        assert_eq!(paths.len(), 2, "{paths:?}");
        match judge(cards, &paths) {
            Exposure::Exposed(hit) => {
                assert_eq!(
                    hit.len(),
                    1,
                    "only the codec on the unit's port, not the dongle on port 5"
                );
                assert_eq!(hit[0].id, Some(77));
            }
            other => panic!("{other:?}"),
        }
        // Without the codec in the list: protected.
        let none = parse_pw_dump(r#"[{"id":1,"type":"PipeWire:Interface:Core"}]"#).unwrap();
        assert_eq!(judge(none, &paths), Exposure::Protected);
        assert!(parse_pw_dump("not json").is_err());
        let _ = std::fs::remove_dir_all(&root);
    }

    // Tests [@ANCHOR: vnwa_open_sound_server_pulse]
    #[test]
    fn pactl_cards_are_parsed_per_block_and_the_check_reports_what_it_could_not_inspect() {
        let cards = parse_pactl_cards(PACTL);
        assert_eq!(cards.len(), 1, "the first block lacks the product id");
        assert_eq!(cards[0].port.as_deref(), Some("2.2.3"));
        assert_eq!(cards[0].id, Some(4));
        let root = unit_tree("sound_pa");
        let exposed = check(&root, &|p, _| {
            if p == "pactl" {
                Some(PACTL.to_string())
            } else {
                None
            }
        });
        assert_eq!(protected(&exposed), Some(false));
        let w = warning(&exposed).unwrap();
        assert!(
            w.contains("install-system-config") && w.contains("wpctl set-profile"),
            "{w}"
        );
        let neither = check(&root, &|_, _| None);
        assert_eq!(protected(&neither), None);
        // No VNWA plugged in: unknown, not "protected".
        let empty = tmp("sound_empty");
        assert_eq!(protected(&check(&empty, &|_, _| Some("[]".into()))), None);
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&empty);
    }
}
