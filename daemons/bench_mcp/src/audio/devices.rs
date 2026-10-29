// SPDX-License-Identifier: AGPL-3.0-or-later
//! The sound devices as PipeWire reports them (`pw-dump`): every audio sink and source node with its ALSA card, its USB
//! path (to link to the `bench_usb_*` tools), its formats, the port availability the driver reports (jack detection), whether
//! it is the default, and whether it is reserved for another driver (the VNWA codecs) and so never opened by these tools.

use super::{Config, Run};
use serde_json::{json, Value};
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Dir {
    Output,
    Input,
}

impl Dir {
    pub fn word(self) -> &'static str {
        match self {
            Dir::Output => "output",
            Dir::Input => "input",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Device {
    /// The PipeWire object id; it changes when the device is re-plugged, so persistent names use `node_name`.
    pub id: u32,
    pub node_name: String,
    pub description: String,
    pub dir: Dir,
    pub card: Option<u32>,
    pub alsa_device: Option<u32>,
    pub alsa_path: Option<String>,
    /// `usb`, `pci`, `bluetooth`, `network` or whatever PipeWire says.
    pub bus: String,
    /// The USB device path as the hub tools write it (`3-2.2.2`), for USB sound cards.
    pub usb_path: Option<String>,
    /// `vid:pid` of the USB device (from PipeWire's device properties, and from `/proc/asound/cardN/usbid`).
    pub usb_id: Option<String>,
    pub port_type: Option<String>,
    /// The route availability the driver reports for the port: `yes`, `no` (an unplugged jack, an HDMI port without a
    /// monitor) or `unknown`. Supporting evidence only.
    pub availability: Option<String>,
    pub is_default: bool,
    pub state: String,
    pub channels: Option<u64>,
    pub rates: Vec<u64>,
    pub formats: Vec<String>,
    pub reserved: Option<String>,
    pub alias: Option<String>,
}

impl Device {
    pub fn transport(&self) -> &'static str {
        if self.node_name.starts_with("bluez_") || self.bus == "bluetooth" {
            "bluetooth"
        } else if self.node_name.contains("raop") || self.node_name.starts_with("tunnel.") {
            "network"
        } else {
            "alsa"
        }
    }

    pub fn json(&self) -> Value {
        json!({
            "name": self.node_name, "alias": self.alias, "description": self.description, "direction": self.dir.word(),
            "pipewire_id": self.id, "alsa_card": self.card, "alsa_device": self.alsa_device, "alsa_path": self.alsa_path,
            "bus": self.bus, "usb_path": self.usb_path, "usb_id": self.usb_id, "transport": self.transport(),
            "port_type": self.port_type, "jack_or_port_available": self.availability, "default": self.is_default,
            "state": self.state, "channels": self.channels, "sample_rates_hz": self.rates, "formats": self.formats,
            "reserved": self.reserved.is_some(), "reserved_reason": self.reserved,
        })
    }
}

fn text(v: &Value, k: &str) -> Option<String> {
    v.get(k).and_then(|x| x.as_str()).map(String::from)
}

fn num(v: &Value, k: &str) -> Option<u64> {
    v.get(k).and_then(|x| {
        x.as_u64()
            .or_else(|| x.as_str().and_then(|s| s.parse().ok()))
    })
}

/// The USB device path in a sysfs path such as `/devices/pci0000:00/0000:00:14.0/usb3/3-2/3-2.2/3-2.2.2/3-2.2.2:1.0/sound/card1`.
fn usb_path_of(sysfs: &str) -> Option<String> {
    sysfs
        .split('/')
        .rfind(|c| {
            c.contains('-')
                && !c.contains(':')
                && c.bytes()
                    .all(|b| b.is_ascii_digit() || b == b'-' || b == b'.')
        })
        .map(String::from)
}

fn rates_of(v: &Value, out: &mut Vec<u64>) {
    match v {
        Value::Number(n) => out.extend(n.as_u64()),
        Value::Object(o) => {
            for k in ["default", "min", "max"] {
                if let Some(n) = o.get(k).and_then(|x| x.as_u64()) {
                    out.push(n);
                }
            }
        }
        Value::Array(a) => a.iter().for_each(|x| rates_of(x, out)),
        _ => {}
    }
}

fn formats_of(v: &Value, out: &mut Vec<String>) {
    match v {
        Value::String(s) => out.push(s.clone()),
        Value::Object(o) => o.values().for_each(|x| formats_of(x, out)),
        Value::Array(a) => a.iter().for_each(|x| formats_of(x, out)),
        _ => {}
    }
}

/// The reserved reason for a device: a `vid:pid` or a node-name fragment of the config's reserved list.
fn reserved_reason(cfg: &Config, d: &Device) -> Option<String> {
    for r in &cfg.reserved {
        let hit = if r.contains(':') && r.len() == 9 {
            d.usb_id.as_deref() == Some(r.as_str())
        } else {
            d.node_name.to_ascii_lowercase().contains(r)
        };
        if hit {
            return Some(format!(
                "matches the reserved entry {r:?}: another driver (the VNWA) owns this sound card and a second opener breaks its sweeps"
            ));
        }
    }
    None
}

/// Parses `pw-dump` JSON into the audio devices. `asound` is the `/proc/asound` directory (the `usbid` file of each card, which is
/// read instead of trusting card numbers, because a power cycle of the VNA renumbers the cards).
pub fn parse(dump: &str, asound: &Path, cfg: &Config) -> Result<Vec<Device>, String> {
    // [@ANCHOR: audio_devices_parse]
    let objs: Vec<Value> =
        serde_json::from_str(dump).map_err(|e| format!("pw-dump output: {e}"))?;
    let kind = |o: &Value| {
        o.get("type")
            .and_then(|t| t.as_str())
            .and_then(|t| t.rsplit(':').next())
            .unwrap_or_default()
            .to_string()
    };
    let mut default_sink = None;
    let mut default_source = None;
    for o in objs.iter().filter(|o| kind(o) == "Metadata") {
        for m in o
            .get("metadata")
            .and_then(|m| m.as_array())
            .into_iter()
            .flatten()
        {
            let name = m.pointer("/value/name").and_then(|n| n.as_str());
            match m.get("key").and_then(|k| k.as_str()) {
                Some("default.audio.sink") => default_sink = name.map(String::from),
                Some("default.audio.source") => default_source = name.map(String::from),
                _ => {}
            }
        }
    }
    let device_obj = |id: u64| {
        objs.iter()
            .find(|o| kind(o) == "Device" && o.get("id").and_then(|i| i.as_u64()) == Some(id))
    };
    let mut out = Vec::new();
    for o in objs.iter().filter(|o| kind(o) == "Node") {
        let props = o.pointer("/info/props").unwrap_or(&Value::Null);
        let dir = match text(props, "media.class").as_deref() {
            Some("Audio/Sink") => Dir::Output,
            Some("Audio/Source") => Dir::Input,
            _ => continue,
        };
        let name = text(props, "node.name").ok_or("a node has no node.name")?;
        let card = num(props, "alsa.card").or_else(|| num(props, "api.alsa.pcm.card"));
        let parent = num(props, "device.id").and_then(device_obj);
        let dprops = parent
            .and_then(|p| p.pointer("/info/props"))
            .unwrap_or(&Value::Null);
        let mut usb_id = match (
            text(dprops, "device.vendor.id"),
            text(dprops, "device.product.id"),
        ) {
            (Some(v), Some(p)) if text(dprops, "device.bus").as_deref() == Some("usb") => {
                Some(format!(
                    "{}:{}",
                    v.trim_start_matches("0x").to_ascii_lowercase(),
                    p.trim_start_matches("0x").to_ascii_lowercase()
                ))
            }
            _ => None,
        };
        // The kernel's own answer wins over PipeWire's properties.
        if let Some(c) = card {
            if let Ok(t) = std::fs::read_to_string(asound.join(format!("card{c}/usbid"))) {
                usb_id = Some(t.trim().to_ascii_lowercase());
            }
        }
        let mut availability = None;
        let mut port_type = None;
        if let Some(profile_dev) = num(props, "card.profile.device") {
            let routes = parent
                .and_then(|p| p.pointer("/info/params/Route"))
                .and_then(|r| r.as_array());
            let want = match dir {
                Dir::Output => "Output",
                Dir::Input => "Input",
            };
            for r in routes.into_iter().flatten() {
                let same = r.get("device").and_then(|d| d.as_u64()) == Some(profile_dev)
                    && r.get("direction").and_then(|d| d.as_str()) == Some(want);
                if same {
                    availability = text(r, "available");
                    let info = r.get("info").and_then(|i| i.as_array());
                    port_type = info.and_then(|i| {
                        i.windows(2)
                            .find(|w| w[0].as_str() == Some("port.type"))
                            .and_then(|w| w[1].as_str().map(String::from))
                    });
                }
            }
        }
        let mut rates = Vec::new();
        let mut formats = Vec::new();
        let mut channels = num(props, "audio.channels");
        for f in o
            .pointer("/info/params/EnumFormat")
            .and_then(|f| f.as_array())
            .into_iter()
            .flatten()
        {
            if let Some(r) = f.get("rate") {
                rates_of(r, &mut rates);
            }
            if let Some(r) = f.get("format") {
                formats_of(r, &mut formats);
            }
            if let Some(c) = f.get("channels").and_then(|c| c.as_u64()) {
                channels = Some(c);
            }
        }
        rates.sort_unstable();
        rates.dedup();
        formats.dedup();
        let is_default = match dir {
            Dir::Output => default_sink.as_deref() == Some(name.as_str()),
            Dir::Input => default_source.as_deref() == Some(name.as_str()),
        };
        let sysfs = text(dprops, "device.sysfs.path").unwrap_or_default();
        let mut d = Device {
            id: o.get("id").and_then(|i| i.as_u64()).unwrap_or(0) as u32,
            description: text(props, "node.description").unwrap_or_else(|| name.clone()),
            node_name: name,
            dir,
            card: card.map(|c| c as u32),
            alsa_device: num(props, "alsa.device").map(|c| c as u32),
            alsa_path: text(props, "api.alsa.path"),
            bus: text(dprops, "device.bus").unwrap_or_else(|| "unknown".into()),
            usb_path: usb_path_of(&sysfs),
            usb_id,
            port_type,
            availability,
            is_default,
            state: o
                .pointer("/info/state")
                .and_then(|s| s.as_str())
                .unwrap_or("unknown")
                .to_string(),
            channels,
            rates,
            formats,
            reserved: None,
            alias: None,
        };
        d.reserved = reserved_reason(cfg, &d);
        d.alias = cfg.alias_of(&d);
        out.push(d);
    }
    out.sort_by(|a, b| (a.dir as u8, &a.node_name).cmp(&(b.dir as u8, &b.node_name)));
    Ok(out)
}

/// Reads the live device list: `pw-dump` and `/proc/asound`.
pub fn list(run: &dyn Run, asound: &Path, cfg: &Config) -> Result<Vec<Device>, String> {
    let (ok, text) = run.run("pw-dump", &[])?;
    if !ok {
        return Err(format!(
            "pw-dump failed (is PipeWire running for this user?): {}",
            text.trim()
        ));
    }
    parse(&text, asound, cfg)
}

/// Finds a device by alias, exact node name, or a unique fragment of the node name or description, in the given direction.
/// A reserved device is never returned: the answer is an error that says so.
pub fn resolve<'a>(devs: &'a [Device], want: &str, dir: Dir) -> Result<&'a Device, String> {
    let w = want.to_ascii_lowercase();
    let side: Vec<&Device> = devs.iter().filter(|d| d.dir == dir).collect();
    let exact: Vec<&&Device> = side
        .iter()
        .filter(|d| {
            d.node_name.eq_ignore_ascii_case(want)
                || d.alias
                    .as_deref()
                    .is_some_and(|a| a.eq_ignore_ascii_case(want))
        })
        .collect();
    let hits: Vec<&Device> = if exact.len() == 1 {
        vec![*exact[0]]
    } else {
        side.iter()
            .copied()
            .filter(|d| {
                d.node_name.to_ascii_lowercase().contains(&w)
                    || d.description.to_ascii_lowercase().contains(&w)
            })
            .collect()
    };
    // Reserved devices never count as candidates; when only reserved ones match, say so.
    if let Some(r) = hits.iter().find(|d| d.reserved.is_some()) {
        if hits.iter().all(|d| d.reserved.is_some()) {
            return Err(format!(
                "{want:?} names only reserved devices ({}) and the audio tools never open them: {}",
                hits.iter()
                    .map(|d| d.node_name.as_str())
                    .collect::<Vec<_>>()
                    .join(", "),
                r.reserved.as_deref().unwrap_or_default()
            ));
        }
    }
    let free: Vec<&Device> = hits.into_iter().filter(|d| d.reserved.is_none()).collect();
    match free.as_slice() {
        [one] => Ok(one),
        [] => Err(format!(
            "no {} device matches {want:?}; bench_audio_devices lists them",
            dir.word()
        )),
        many => Err(format!(
            "{want:?} matches {} {} devices ({}); use a more specific name or an alias",
            many.len(),
            dir.word(),
            many.iter()
                .map(|d| d.node_name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::testkit::{devbox_devices, recorded};

    // Tests [@ANCHOR: audio_devices_parse]
    #[test]
    fn the_recorded_devbox_lists_its_sinks_and_sources_with_defaults() {
        let devs = devbox_devices();
        let names = |d: Dir| -> Vec<&str> {
            devs.iter()
                .filter(|x| x.dir == d)
                .map(|x| x.description.as_str())
                .collect()
        };
        assert!(names(Dir::Output).contains(&"Raptor Lake-P/U/H cAVS Speaker"));
        assert!(names(Dir::Input).contains(&"Raptor Lake-P/U/H cAVS Digital Microphone"));
        assert_eq!(
            devs.iter()
                .filter(|d| d.dir == Dir::Output && d.description.contains("HDMI"))
                .count(),
            3
        );
        let speaker = devs
            .iter()
            .find(|d| d.description.ends_with("Speaker"))
            .unwrap();
        assert!(
            speaker.is_default,
            "the active default sink, not the configured one that is absent"
        );
        assert_eq!((speaker.card, speaker.alsa_device), (Some(0), Some(0)));
        assert_eq!(speaker.port_type.as_deref(), Some("speaker"));
        assert_eq!(speaker.bus, "pci");
        assert!(speaker.usb_path.is_none() && speaker.reserved.is_none());
        assert!(speaker.rates.contains(&48000) && speaker.formats.iter().any(|f| f == "S16LE"));
        let dmic = devs
            .iter()
            .find(|d| d.description.ends_with("Digital Microphone"))
            .unwrap();
        assert!(dmic.is_default);
        let mic2 = devs
            .iter()
            .find(|d| d.description.ends_with("Stereo Microphone"))
            .unwrap();
        assert!(!mic2.is_default);
    }

    #[test]
    fn hdmi_ports_without_a_monitor_report_unavailable_as_evidence() {
        let devs = devbox_devices();
        let hdmi: Vec<&Device> = devs
            .iter()
            .filter(|d| d.port_type.as_deref() == Some("hdmi"))
            .collect();
        assert!(!hdmi.is_empty());
        assert!(hdmi.iter().any(|d| d.availability.as_deref() == Some("no")));
    }

    #[test]
    fn the_two_vnwa_codecs_are_reserved_by_usb_id_and_never_resolve() {
        let devs = devbox_devices();
        let usb: Vec<&Device> = devs.iter().filter(|d| d.bus == "usb").collect();
        assert!(usb.len() >= 2, "{usb:?}");
        for d in &usb {
            assert_eq!(d.usb_id.as_deref(), Some("08bb:29c0"));
            assert!(d.reserved.is_some(), "{}", d.node_name);
            assert!(
                d.usb_path
                    .as_deref()
                    .is_some_and(|p| p.starts_with("3-2.2.")),
                "{:?}",
                d.usb_path
            );
        }
        let e = resolve(&devs, "USB_AUDIO_CODEC-00", Dir::Input);
        let e = e.unwrap_err();
        assert!(e.contains("only reserved devices"), "{e}");
        let by_desc = resolve(&devs, "PCM2900C Audio CODEC", Dir::Output).unwrap_err();
        assert!(
            by_desc.contains("reserved") || by_desc.contains("matches"),
            "{by_desc}"
        );
    }

    #[test]
    fn the_kernel_usbid_file_overrides_what_pipewire_claims() {
        // Card numbers are looked up through /proc/asound/cardN/usbid, so a node whose PipeWire properties lack the ids is
        // still reserved when the kernel says the card is the VNWA codec.
        let dump = std::fs::read_to_string(recorded().join("pw-dump.json")).unwrap();
        let stripped = dump
            .replace("\"device.vendor.id\"", "\"x.vendor\"")
            .replace("\"device.product.id\"", "\"x.product\"");
        let devs = parse(&stripped, &recorded(), &Config::default()).unwrap();
        let usb: Vec<&Device> = devs
            .iter()
            .filter(|d| d.card.is_some_and(|c| c > 0))
            .collect();
        assert!(!usb.is_empty());
        assert!(usb.iter().all(|d| d.reserved.is_some()), "{usb:?}");
    }

    #[test]
    fn resolve_takes_names_aliases_and_fragments_and_refuses_ambiguity() {
        let mut cfg = Config::default();
        cfg.outputs.insert(
            "desk".into(),
            super::super::Port {
                matcher: "Speaker".into(),
                volume_cap: None,
            },
        );
        let dump = std::fs::read_to_string(recorded().join("pw-dump.json")).unwrap();
        let devs = parse(&dump, &recorded(), &cfg).unwrap();
        assert!(resolve(&devs, "desk", Dir::Output)
            .unwrap()
            .description
            .ends_with("Speaker"));
        assert!(resolve(&devs, "Speaker", Dir::Output).is_ok());
        assert!(resolve(&devs, "HDMI", Dir::Output)
            .unwrap_err()
            .contains("matches 3"));
        assert!(resolve(&devs, "Speaker", Dir::Input)
            .unwrap_err()
            .contains("no input"));
    }

    #[test]
    fn a_bluetooth_node_is_recognised_by_its_name() {
        let dump = r#"[{"id":7,"type":"PipeWire:Interface:Node","info":{"state":"idle","props":{"node.name":"bluez_output.AA_BB.1","media.class":"Audio/Sink","node.description":"Headset"},"params":{}}}]"#;
        let devs = parse(dump, Path::new("/nonexistent"), &Config::default()).unwrap();
        assert_eq!(devs[0].transport(), "bluetooth");
    }

    #[test]
    fn broken_json_is_an_error_and_not_an_empty_list() {
        assert!(parse("not json", &recorded(), &Config::default()).is_err());
    }
}
