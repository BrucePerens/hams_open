// SPDX-License-Identifier: AGPL-3.0-or-later
//! Finding the radios on this computer for `radioctl-selftest`, without guessing and without
//! writing to anything that is not a radio.
//!
//! A serial port is looked at in three steps. (1) A port whose own text says it is not a radio
//! (a GPS receiver, a programmer, an Arduino) is skipped untouched. (2) The product string is
//! matched against the descriptions' model names; a port with no evidence is `unidentified` and
//! is not opened, because a generic USB-serial chip says nothing about what is behind it (name it
//! with `--port` and `--description`). (3) A port that fits several descriptions is asked by
//! their own read-only identify commands, at the rates the baud probe tries; if the answers do
//! not leave exactly one, it is `ambiguous` and skipped. Network radios that announce themselves
//! (FlexRadio's discovery broadcast, which is only listened to) are listed.
//! Nothing here sends a setting, a key or a transmit command.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use radioctl_core::baud_probe::{probe, SerialProbePort, DEFAULT_RATES};
use radioctl_core::capability::Capability;
use radioctl_core::description::RigDescription;
use radioctl_core::op::{Observation, Op};
use radioctl_core::serial::{LineControl, PortTarget, SerialConfig};
use radioctl_core::usb::PortInfo;
use radioctl_protocols::conformance::engine_for;
use radioctl_protocols::flex::{parse_discovery, Discovery, DISCOVERY_PORT};
use tokio::runtime::Handle;

use crate::candidates::{candidates_for_product, squash, RigSummary};
use crate::selftest::open;

/// USB vendor ids that are never a radio's CAT port: Arduino, u-blox, Prolific-less GPS, SEGGER,
/// STMicro bootloaders, Microchip programmers.
const NOT_RADIO_VIDS: [u16; 5] = [0x2341, 0x1546, 0x1366, 0x0483, 0x03eb];
/// Words in a product string that say the device is not a radio.
const NOT_RADIO_WORDS: [&str; 9] = [
    "gps",
    "gnss",
    "arduino",
    "programmer",
    "jtag",
    "bootloader",
    "ublox",
    "pickit",
    "stlink",
];

/// What discovery concluded about one port.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Clearly not a radio; never opened. Holds the reason.
    NotRadio(String),
    /// No description is named by the device; never opened.
    Unidentified,
    /// One description, and how it was decided.
    Identified {
        /// The description id.
        id: String,
        /// `product string` or `answered ID n`.
        how: String,
    },
    /// Several descriptions fit and the radio's answers did not settle it; skipped.
    Ambiguous(Vec<String>),
}

/// One serial port and what was concluded.
#[derive(Debug, Clone)]
pub struct Finding {
    /// The port.
    pub port: PortInfo,
    /// The conclusion.
    pub outcome: Outcome,
}

/// A radio found on the network.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetFinding {
    /// Address and TCP port.
    pub address: String,
    /// The model the radio announced.
    pub model: String,
    /// The description id it matches, if exactly one.
    pub description: Option<String>,
}

// [@ANCHOR: radioctld_discovery_not_radio_reason]
/// Why this port is clearly not a radio, or `None`. Only the device's own ids and text are used.
pub fn not_radio_reason(port: &PortInfo) -> Option<String> {
    if let Some(vid) = port.vid.filter(|v| NOT_RADIO_VIDS.contains(v)) {
        return Some(format!("USB vendor {vid:04x} makes boards, not radios"));
    }
    let product = squash(port.product.as_deref()?);
    NOT_RADIO_WORDS
        .iter()
        .find(|w| product.contains(**w))
        .map(|w| format!("the product string says {w}"))
}

// [@ANCHOR: radioctld_discovery_expected_id]
/// The model id the description says the radio answers to an identify read, if it states one.
pub fn expected_id(description: &RigDescription) -> Option<u16> {
    description
        .civ
        .as_ref()
        .map(|c| u16::from(c.model_id))
        .or(description.kenwood.as_ref().map(|k| k.model_id))
        .or(description.elecraft.as_ref().map(|e| e.model_id))
        .or(description.yaesu.as_ref().map(|y| y.model_id))
}

// [@ANCHOR: radioctld_discovery_ask_radio]
/// Asks the radio at `path` whether it is `description`: the baud probe's frequency read at each
/// usual rate, then the identify read, both read-only. True only if the answered id equals the
/// description's own. A port that does not answer, or answers otherwise, is false.
pub async fn ask_radio(description: &RigDescription, path: &Path, unmanaged: bool) -> bool {
    let Some(expected) = expected_id(description) else {
        return false;
    };
    if !description.capabilities.has(Capability::Identify) {
        return false;
    }
    let Ok(mut engine) = engine_for(description) else {
        return false;
    };
    let lines = if unmanaged {
        LineControl::Unmanaged
    } else {
        LineControl::Managed
    };
    let mut template = SerialConfig::new(PortTarget::Path(path.to_path_buf()), DEFAULT_RATES[1]);
    template.lines = lines;
    let mut port = SerialProbePort::new(Handle::current(), template);
    let Ok(found) = probe(&mut port, &mut engine, &DEFAULT_RATES).await else {
        return false;
    };
    let wire_path = std::env::temp_dir().join(format!(
        "radioctl-discovery-{}-{}.wire",
        std::process::id(),
        description.id
    ));
    let Ok(file) = std::fs::File::create(&wire_path) else {
        return false;
    };
    let target = path.display().to_string();
    let answered = match open(
        description,
        &target,
        found.baud,
        unmanaged,
        Arc::new(Mutex::new(file)),
    )
    .await
    {
        Ok(rig) => matches!(
            rig.execute(Op::GetModelId).await,
            Ok(Some(Observation::ModelId(id))) if id == expected
        ),
        Err(_) => false,
    };
    let _ = std::fs::remove_file(&wire_path);
    answered
}

// [@ANCHOR: radioctld_discovery_discover_ports]
/// Classifies every port. `descriptions` are the loaded descriptions and `rigs` their summaries.
pub async fn discover_ports(
    ports: &[PortInfo],
    descriptions: &[RigDescription],
    rigs: &[RigSummary],
    unmanaged: bool,
) -> Vec<Finding> {
    let mut findings = Vec::new();
    for port in ports {
        let outcome = if let Some(reason) = not_radio_reason(port) {
            Outcome::NotRadio(reason)
        } else {
            let named: Vec<&RigSummary> = port
                .product
                .as_deref()
                .map(|p| candidates_for_product(p, rigs))
                .unwrap_or_default();
            match named.as_slice() {
                [] => Outcome::Unidentified,
                [one] => Outcome::Identified {
                    id: one.id.clone(),
                    how: "product string".into(),
                },
                many => {
                    let mut answered = Vec::new();
                    for rig in many {
                        let found = descriptions.iter().find(|d| d.id == rig.id);
                        if let Some(description) = found {
                            if ask_radio(description, &port.path, unmanaged).await {
                                answered.push(rig.id.clone());
                            }
                        }
                    }
                    match answered.as_slice() {
                        [one] => Outcome::Identified {
                            id: one.clone(),
                            how: "answered its identify read".into(),
                        },
                        [] => Outcome::Ambiguous(many.iter().map(|r| r.id.clone()).collect()),
                        _ => Outcome::Ambiguous(answered),
                    }
                }
            }
        };
        findings.push(Finding {
            port: port.clone(),
            outcome,
        });
    }
    findings
}

// [@ANCHOR: radioctld_discovery_net_from_packet]
/// A network finding from one discovery datagram, matched to a description by model name.
pub fn net_from_packet(packet: &[u8], rigs: &[RigSummary]) -> Option<NetFinding> {
    let Discovery {
        model, ip, port, ..
    } = parse_discovery(packet)?;
    let named = candidates_for_product(&model, rigs);
    Some(NetFinding {
        address: format!("{ip}:{port}"),
        description: match named.as_slice() {
            [one] => Some(one.id.clone()),
            _ => None,
        },
        model,
    })
}

// [@ANCHOR: radioctld_discovery_listen_flex]
/// Listens (receive only, nothing is sent) for FlexRadio discovery broadcasts for `window`.
/// A port already in use, or no network, gives an empty list.
pub async fn listen_flex(window: Duration, rigs: &[RigSummary]) -> Vec<NetFinding> {
    let mut found: Vec<NetFinding> = Vec::new();
    let Ok(socket) = tokio::net::UdpSocket::bind(("0.0.0.0", DISCOVERY_PORT)).await else {
        return found;
    };
    let end = tokio::time::Instant::now() + window;
    let mut buffer = [0u8; 2048];
    while let Ok(Ok(n)) = tokio::time::timeout_at(end, socket.recv(&mut buffer)).await {
        if let Some(radio) = buffer
            .get(..n)
            .and_then(|packet| net_from_packet(packet, rigs))
        {
            if !found.iter().any(|f| f.address == radio.address) {
                found.push(radio);
            }
        }
    }
    found
}

// [@ANCHOR: radioctld_discovery_render_table]
/// The discovery table: where, what the device says it is, and what was chosen.
pub fn render_table(findings: &[Finding], net: &[NetFinding]) -> String {
    let mut text = String::from("Discovery:\n");
    for f in findings {
        let ids = match (f.port.vid, f.port.pid) {
            (Some(v), Some(p)) => format!("usb {v:04x}:{p:04x}"),
            _ => "no usb ids".into(),
        };
        let product = f.port.product.as_deref().unwrap_or("(no product string)");
        let verdict = match &f.outcome {
            Outcome::NotRadio(why) => format!("skipped, not a radio ({why})"),
            Outcome::Unidentified => {
                "unidentified (use --port and --description to test it)".into()
            }
            Outcome::Identified { id, how } => format!("{id} ({how})"),
            Outcome::Ambiguous(ids) => format!("ambiguous: candidates {}", ids.join(", ")),
        };
        text.push_str(&format!(
            "  {:<16} {ids}, {product}: {verdict}\n",
            f.port.path.display()
        ));
    }
    for n in net {
        let verdict = match &n.description {
            Some(id) => format!("{id} (listed only: network rigs are not self-tested yet)"),
            None => "unidentified".into(),
        };
        text.push_str(&format!("  {:<16} {}: {verdict}\n", n.address, n.model));
    }
    if findings.is_empty() && net.is_empty() {
        text.push_str("  nothing found\n");
    }
    text
}

// [@ANCHOR: radioctld_discovery_identified]
/// The serial path of a finding that was identified, with its description id.
pub fn identified(findings: &[Finding]) -> Vec<(String, PathBuf, PortInfo)> {
    findings
        .iter()
        .filter_map(|f| match &f.outcome {
            Outcome::Identified { id, .. } => {
                Some((id.clone(), f.port.path.clone(), f.port.clone()))
            }
            _ => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builtin::embedded_texts;
    use crate::candidates::summaries_from;

    fn port(path: &str, vid: u16, product: Option<&str>) -> PortInfo {
        PortInfo {
            path: PathBuf::from(path),
            vid: Some(vid),
            pid: Some(1),
            serial: Some("SECRET".into()),
            product: product.map(str::to_string),
            cdc_acm: false,
        }
    }

    fn rigs() -> Vec<RigSummary> {
        summaries_from(&embedded_texts()).expect("embedded")
    }

    // Tests [@ANCHOR: radioctld_discovery_not_radio_reason]
    #[test]
    fn gps_programmers_and_boards_are_not_radios() {
        assert!(not_radio_reason(&port("/a", 0x1546, None)).is_some());
        assert!(not_radio_reason(&port("/a", 0x10c4, Some("u-blox GNSS receiver"))).is_some());
        assert!(not_radio_reason(&port("/a", 0x10c4, Some("Arduino Uno"))).is_some());
        assert!(not_radio_reason(&port("/a", 0x10c4, Some("CP2102 USB to UART Bridge"))).is_none());
        assert!(not_radio_reason(&port("/a", 0x10c4, None)).is_none());
    }

    // Tests [@ANCHOR: radioctld_discovery_expected_id]
    #[test]
    fn the_expected_identify_answer_comes_from_the_description() {
        assert_eq!(expected_id(&RigDescription::ic7300()), Some(0x94));
        assert!(expected_id(&RigDescription::ts2000()).is_some());
    }

    // Tests [@ANCHOR: radioctld_discovery_discover_ports]
    // Tests [@ANCHOR: radioctld_discovery_ask_radio]
    // Tests [@ANCHOR: radioctld_discovery_identified]
    #[tokio::test]
    async fn ports_are_skipped_named_or_left_ambiguous_without_writing_to_non_radios() {
        let texts = embedded_texts();
        let descriptions: Vec<RigDescription> = texts
            .iter()
            .map(|(_, t)| radioctl_core::rigfile::parse(t).expect("parse").description)
            .collect();
        let ports = [
            port("/dev/gps-never-opened", 0x1546, Some("GPS")),
            port(
                "/dev/generic-never-opened",
                0x10c4,
                Some("CP2102 USB to UART Bridge Controller"),
            ),
            port("/dev/ic7300-never-opened", 0x10c4, Some("IC-7300")),
            port("/dev/no-such-radio-selftest", 0x10c4, Some("TS-590SG")),
        ];
        let found = discover_ports(&ports, &descriptions, &rigs(), true).await;
        assert!(matches!(found[0].outcome, Outcome::NotRadio(_)));
        assert_eq!(found[1].outcome, Outcome::Unidentified);
        assert!(
            matches!(&found[2].outcome, Outcome::Identified { id, .. } if id == "icom-ic-7300")
        );
        assert_eq!(
            found[3].outcome,
            Outcome::Ambiguous(vec!["kenwood-ts-590s".into(), "kenwood-ts-590sg".into()])
        );
        let ids = identified(&found);
        assert_eq!(ids.len(), 1);
        assert!(
            !ask_radio(
                &descriptions[0],
                Path::new("/dev/no-such-radio-selftest"),
                true
            )
            .await
        );
        let table = render_table(&found, &[]);
        assert!(table.contains("ambiguous: candidates kenwood-ts-590s, kenwood-ts-590sg"));
        assert!(table.contains("skipped, not a radio") && table.contains("unidentified"));
        assert!(!table.contains("SECRET"));
    }

    // Tests [@ANCHOR: radioctld_discovery_net_from_packet]
    // Tests [@ANCHOR: radioctld_discovery_listen_flex]
    #[tokio::test]
    async fn garbage_is_not_a_discovery_packet_and_listening_is_receive_only() {
        assert_eq!(net_from_packet(b"not vita", &rigs()), None);
        // A zero window returns at once whether or not the port could be bound.
        assert!(listen_flex(Duration::ZERO, &rigs()).await.is_empty());
    }

    // Tests [@ANCHOR: radioctld_discovery_render_table]
    #[test]
    fn the_table_lists_network_radios_and_says_when_nothing_was_found() {
        assert!(render_table(&[], &[]).contains("nothing found"));
        let net = [NetFinding {
            address: "192.0.2.5:4992".into(),
            model: "FLEX-6300".into(),
            description: Some("flex-6300".into()),
        }];
        assert!(render_table(&[], &net).contains("flex-6300"));
    }
}
