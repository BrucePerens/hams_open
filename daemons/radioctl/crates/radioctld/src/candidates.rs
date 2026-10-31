// SPDX-License-Identifier: AGPL-3.0-or-later
//! Listing candidate rig descriptions for a detected serial device, without guessing.
//!
//! USB ids name the bridge chip (Silicon Labs CP210x, FTDI, WCH), not the radio, so they cannot
//! choose a description. The only evidence used here is the device's own product string, and only
//! when it contains a description's model name. A device with no such evidence gets no
//! candidate, only the full list; a device with several gets all of them, flagged. radioctl picks
//! one only when the device's own text names exactly one description and it is the only port
//! ([`auto_pick`]); otherwise the user chooses.

use std::path::Path;

use radioctl_core::rigfile::{self, LoadedDescription, Verified};
use radioctl_core::usb::{classify, PortInfo};

/// What a listing needs to know about one rig description file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RigSummary {
    /// The file stem and description id, such as `icom-ic-7300`.
    pub id: String,
    /// The display name, such as `Icom IC-7300`.
    pub name: String,
    /// The protocol family name.
    pub family: String,
    /// The highest verified state among the facts.
    pub highest: &'static str,
}

// [@ANCHOR: radioctld_candidates_highest_state]
/// The highest verified state among a description's facts: `unverified` when there are none.
pub fn highest_state(loaded: &LoadedDescription) -> &'static str {
    let rank = |v: &Verified| match v {
        Verified::Unverified => 0,
        Verified::ManualChecked { .. } => 1,
        Verified::TranscriptChecked { .. } => 2,
        Verified::HardwareVerified { .. } => 3,
    };
    let names = [
        "unverified",
        "manual-checked",
        "transcript-checked",
        "hardware-verified",
    ];
    let top = loaded
        .facts
        .iter()
        .map(|f| rank(&f.verified))
        .max()
        .unwrap_or(0);
    names.get(top).copied().unwrap_or("unverified")
}

// [@ANCHOR: radioctld_candidates_load_summaries]
/// Reads every `*.toml` in `dir` as a rig description, sorted by id.
pub fn load_summaries(dir: &Path) -> Result<Vec<RigSummary>, String> {
    let mut texts = Vec::new();
    let entries =
        std::fs::read_dir(dir).map_err(|e| format!("cannot read {}: {e}", dir.display()))?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("toml") {
            continue;
        }
        let text = std::fs::read_to_string(&path)
            .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        texts.push((path.display().to_string(), text));
    }
    summaries_from(&texts)
}

// [@ANCHOR: radioctld_candidates_summaries_from]
/// Parses `(label, description text)` pairs into summaries sorted by id; the label names the
/// source in an error. This is what the embedded descriptions and a directory both go through.
pub fn summaries_from(texts: &[(String, String)]) -> Result<Vec<RigSummary>, String> {
    let mut out = Vec::new();
    for (label, text) in texts {
        let loaded = rigfile::parse(text).map_err(|e| format!("{label}: {e}"))?;
        out.push(RigSummary {
            id: loaded.description.id.clone(),
            name: loaded.description.name.clone(),
            family: format!("{:?}", loaded.description.family),
            highest: highest_state(&loaded),
        });
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(out)
}

// [@ANCHOR: radioctld_candidates_auto_pick]
/// The one port and description the devices themselves name: exactly one USB port whose product
/// string fits exactly one description. Anything else (no port, several ports, no evidence, two
/// models) is an `Err` that says why, and the caller prints the listing instead of choosing.
pub fn auto_pick<'a>(
    ports: &'a [PortInfo],
    rigs: &'a [RigSummary],
) -> Result<(&'a PortInfo, &'a RigSummary), String> {
    let named: Vec<(&PortInfo, &RigSummary)> = ports
        .iter()
        .filter_map(|port| {
            let found = candidates_for_product(port.product.as_deref()?, rigs);
            match found.as_slice() {
                [one] => Some((port, *one)),
                _ => None,
            }
        })
        .collect();
    match (named.as_slice(), ports.len()) {
        ([one], 1) => Ok(*one),
        (_, 0) => Err("no USB serial port found".into()),
        ([], _) => Err("no port's own product string names exactly one rig description".into()),
        _ => Err("more than one serial port, or a port that fits more than one description".into()),
    }
}

// [@ANCHOR: radioctld_candidates_squash]
/// Lower-case letters and digits only, so `IC-7300` and `ic 7300` compare equal.
pub fn squash(text: &str) -> String {
    text.chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

// [@ANCHOR: radioctld_candidates_for_product]
/// The descriptions whose model name (the display name without its first word, the maker) is
/// contained in the device's product string. Names shorter than four characters never match.
pub fn candidates_for_product<'a>(product: &str, rigs: &'a [RigSummary]) -> Vec<&'a RigSummary> {
    let product = squash(product);
    rigs.iter()
        .filter(|rig| {
            let model = squash(
                rig.name
                    .split_once(' ')
                    .map_or(rig.name.as_str(), |(_, m)| m),
            );
            model.len() >= 4 && product.contains(&model)
        })
        .collect()
}

// [@ANCHOR: radioctld_candidates_render]
/// The text `radioctl-selftest --list` prints: each port, what the ids and product string say, and the
/// descriptions that exist. Never names a single choice unless the device's own text does.
pub fn render_listing(ports: &[PortInfo], rigs: &[RigSummary]) -> String {
    let mut text = String::new();
    if ports.is_empty() {
        text.push_str("No USB serial ports found.\n");
    }
    for port in ports {
        text.push_str(&format!("{}\n", port.path.display()));
        if let (Some(vid), Some(pid)) = (port.vid, port.pid) {
            text.push_str(&format!(
                "  usb {vid:04x}:{pid:04x}, chip {:?}, product {:?}\n",
                classify(vid, pid, port.cdc_acm),
                port.product.as_deref().unwrap_or("(none)")
            ));
            text.push_str(&format!(
                "  target for radioctl-selftest: usb:{vid:04x}:{pid:04x}\n"
            ));
        }
        let found = port
            .product
            .as_deref()
            .map(|p| candidates_for_product(p, rigs))
            .unwrap_or_default();
        match found.as_slice() {
            [] => text.push_str(
                "  no description is named by this device; the USB ids identify the bridge chip,\n  not the radio. Choose from the list below.\n",
            ),
            [one] => text.push_str(&format!(
                "  the product string names {} ({}); check it is your radio\n",
                one.id, one.name
            )),
            many => {
                let ids: Vec<&str> = many.iter().map(|r| r.id.as_str()).collect();
                text.push_str(&format!(
                    "  AMBIGUOUS: the product string fits {}; choose by the exact model on the radio\n",
                    ids.join(", ")
                ));
            }
        }
    }
    text.push_str("\nDescriptions (id, name, family, highest verified state):\n");
    for rig in rigs {
        text.push_str(&format!(
            "  {:<22} {:<26} {:<12} {}\n",
            rig.id, rig.name, rig.family, rig.highest
        ));
    }
    text.push_str(
        "\nA model with no description is in docs/RIG_SUPPORT.md. Every state other than\nhardware-verified means no volunteer has confirmed it on a radio yet.\n",
    );
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn rigs() -> Vec<RigSummary> {
        load_summaries(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../rigs"))
            .expect("shipped descriptions load")
    }

    fn port(product: Option<&str>) -> PortInfo {
        PortInfo {
            path: PathBuf::from("/dev/ttyUSB0"),
            vid: Some(0x10c4),
            pid: Some(0xea60),
            serial: Some("S".into()),
            product: product.map(str::to_string),
            cdc_acm: false,
        }
    }

    // Tests [@ANCHOR: radioctld_candidates_load_summaries]
    // Tests [@ANCHOR: radioctld_candidates_highest_state]
    #[test]
    fn shipped_descriptions_load_and_none_is_hardware_verified_yet() {
        let rigs = rigs();
        assert!(rigs.len() >= 27, "{}", rigs.len());
        assert!(rigs.iter().any(|r| r.id == "icom-ic-7300"));
        assert!(rigs.iter().all(|r| r.highest != "hardware-verified"));
    }

    // Tests [@ANCHOR: radioctld_candidates_summaries_from]
    #[test]
    fn summaries_come_from_text_and_a_bad_one_names_its_label() {
        let embedded = crate::builtin::embedded_texts();
        assert_eq!(
            summaries_from(&embedded).expect("embedded").len(),
            rigs().len()
        );
        let bad = [("mine.toml".to_string(), "not a description".to_string())];
        assert!(summaries_from(&bad)
            .expect_err("bad")
            .starts_with("mine.toml:"));
    }

    // Tests [@ANCHOR: radioctld_candidates_auto_pick]
    #[test]
    fn auto_pick_needs_one_port_naming_one_description() {
        let rigs = rigs();
        let one = [port(Some("IC-7300"))];
        let picked = auto_pick(&one, &rigs).expect("one");
        assert_eq!(picked.1.id, "icom-ic-7300");
        for ports in [
            vec![],
            vec![port(Some("CP2102 USB to UART Bridge Controller"))],
            vec![port(Some("TS-590SG"))],
            vec![port(Some("IC-7300")), port(Some("IC-7300"))],
            vec![port(Some("IC-7300")), port(None)],
        ] {
            assert!(auto_pick(&ports, &rigs).is_err(), "{ports:?}");
        }
    }

    // Tests [@ANCHOR: radioctld_candidates_squash]
    #[test]
    fn squash_ignores_case_and_punctuation() {
        assert_eq!(squash("IC-7300 USB"), "ic7300usb");
    }

    // Tests [@ANCHOR: radioctld_candidates_for_product]
    #[test]
    fn a_generic_bridge_string_names_nothing() {
        let rigs = rigs();
        assert!(candidates_for_product("CP2102 USB to UART Bridge Controller", &rigs).is_empty());
    }

    // Tests [@ANCHOR: radioctld_candidates_for_product]
    #[test]
    fn a_product_string_that_names_two_models_returns_both() {
        let rigs = rigs();
        let found = candidates_for_product("Kenwood TS-590SG CAT", &rigs);
        let ids: Vec<&str> = found.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(ids, ["kenwood-ts-590s", "kenwood-ts-590sg"]);
    }

    // Tests [@ANCHOR: radioctld_candidates_render]
    #[test]
    fn listing_does_not_pick_for_a_generic_chip_and_flags_ambiguity() {
        let rigs = rigs();
        let generic = render_listing(&[port(Some("CP2102 USB to UART Bridge Controller"))], &rigs);
        assert!(generic.contains("no description is named by this device"));
        assert!(generic.contains("usb:10c4:ea60"));
        assert!(generic.contains("icom-ic-7300"));
        let named = render_listing(&[port(Some("IC-7300"))], &rigs);
        assert!(named.contains("the product string names icom-ic-7300"));
        let ambiguous = render_listing(&[port(Some("TS-590SG"))], &rigs);
        assert!(ambiguous.contains("AMBIGUOUS"));
        assert!(render_listing(&[], &rigs).contains("No USB serial ports found."));
    }
}
