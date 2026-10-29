// SPDX-License-Identifier: AGPL-3.0-or-later
//! Sound-device tools for bench_mcp: list the sound cards, record and play, verify that a device under test made a sound,
//! detect sound events, speak, alert the operator, set levels and discover which outputs and inputs really work by listening.
//!
//! The tools belong to one instrument entry with `"driver": "audio"` in the bench config. Its tier decides what is listed and
//! allowed: `read_only` (devices, status, levels), `read_config` (record, verify, detect: they hear the room), `full`
//! (play, speak, alert, set volume, loopback, discovery: they make noise in the room, inside the volume cap and quiet hours).
//!
//! The sound cards inside the VNWA are reserved (`08bb:29c0` by default): the VNA driver opens them exclusively, so these
//! tools list them, mark them reserved and never open them.

pub mod devices;
pub mod levels;
#[cfg(test)]
pub mod testkit;

use crate::config::{Instrument, Kind, Tier};
use rmcp::model::Tool;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

/// Runs a program and gives back its success flag and its output; production is a process, tests script it.
pub trait Run {
    fn run(&self, program: &str, args: &[String]) -> Result<(bool, String), String>;
}

pub struct Process;

impl Run for Process {
    fn run(&self, program: &str, args: &[String]) -> Result<(bool, String), String> {
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

/// An alias for a device: what it matches (a node name, or a fragment of the node name or description) and an optional
/// volume cap of its own.
#[derive(Clone, Debug, PartialEq)]
pub struct Port {
    pub matcher: String,
    pub volume_cap: Option<f64>,
}

/// Local quiet hours as minutes of the day; `start > end` means the span crosses midnight.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct QuietHours {
    pub start: u32,
    pub end: u32,
}

impl QuietHours {
    pub fn contains(&self, minute: u32) -> bool {
        if self.start <= self.end {
            (self.start..self.end).contains(&minute)
        } else {
            minute >= self.start || minute < self.end
        }
    }
}

/// The `audio` entry of the bench config.
#[derive(Clone, Debug, PartialEq)]
pub struct Config {
    pub outputs: BTreeMap<String, Port>,
    pub inputs: BTreeMap<String, Port>,
    /// `auto` or an alias of `outputs`.
    pub alert_output: String,
    /// `auto` or an alias of `inputs`.
    pub verify_input: String,
    /// The highest wpctl volume any tool sets or plays at.
    pub volume_cap: f64,
    /// The volume used for audible output in quiet hours.
    pub quiet_volume: f64,
    pub quiet_hours: Option<QuietHours>,
    pub capture_dir: Option<PathBuf>,
    pub retention_files: usize,
    pub retention_days: u64,
    pub max_record_s: f64,
    /// `vid:pid` values and lower-case node-name fragments that are never opened. Always includes `08bb:29c0`.
    pub reserved: Vec<String>,
}

pub const VNWA_CODEC: &str = "08bb:29c0";

impl Default for Config {
    fn default() -> Config {
        Config {
            outputs: BTreeMap::new(),
            inputs: BTreeMap::new(),
            alert_output: "auto".into(),
            verify_input: "auto".into(),
            volume_cap: 0.5,
            quiet_volume: 0.15,
            quiet_hours: Some(QuietHours {
                start: 22 * 60,
                end: 7 * 60,
            }),
            capture_dir: None,
            retention_files: 50,
            retention_days: 14,
            max_record_s: 30.0,
            reserved: vec![VNWA_CODEC.into()],
        }
    }
}

impl Config {
    /// The alias, if any, that names this device.
    pub fn alias_of(&self, d: &devices::Device) -> Option<String> {
        let map = match d.dir {
            devices::Dir::Output => &self.outputs,
            devices::Dir::Input => &self.inputs,
        };
        map.iter()
            .find(|(_, p)| {
                let m = p.matcher.to_ascii_lowercase();
                d.node_name.eq_ignore_ascii_case(&p.matcher)
                    || d.node_name.to_ascii_lowercase().contains(&m)
                    || d.description.to_ascii_lowercase().contains(&m)
            })
            .map(|(a, _)| a.clone())
    }

    /// The volume cap that applies to a device: its own, never above the global cap.
    pub fn cap_for(&self, d: &devices::Device) -> f64 {
        let own = d
            .alias
            .as_ref()
            .and_then(|a| match d.dir {
                devices::Dir::Output => self.outputs.get(a),
                devices::Dir::Input => self.inputs.get(a),
            })
            .and_then(|p| p.volume_cap);
        own.map_or(self.volume_cap, |c| c.min(self.volume_cap))
    }

    pub fn capture_dir(&self) -> PathBuf {
        self.capture_dir
            .clone()
            .unwrap_or_else(|| crate::out_dir().join("audio"))
    }
}

fn hhmm(s: &str) -> Result<u32, String> {
    let (h, m) = s.split_once(':').ok_or("time must look like \"22:00\"")?;
    let (h, m): (u32, u32) = (
        h.parse().map_err(|_| "bad hour")?,
        m.parse().map_err(|_| "bad minute")?,
    );
    if h > 23 || m > 59 {
        return Err(format!("{s:?} is not a time of day"));
    }
    Ok(h * 60 + m)
}

fn ports(c: &Value, key: &str) -> Result<BTreeMap<String, Port>, String> {
    let mut out = BTreeMap::new();
    let Some(v) = c.get(key) else { return Ok(out) };
    for (alias, p) in v.as_object().ok_or(format!("{key} must be an object"))? {
        if alias.trim().is_empty() || alias == "auto" {
            return Err(format!("{key}: {alias:?} is not a usable alias"));
        }
        let (matcher, volume_cap) = match p {
            Value::String(s) => (s.clone(), None),
            Value::Object(_) => (
                p.get("match")
                    .and_then(|m| m.as_str())
                    .ok_or(format!("{key}.{alias}: no \"match\""))?
                    .to_string(),
                p.get("volume_cap").and_then(|x| x.as_f64()),
            ),
            _ => return Err(format!("{key}.{alias} must be a string or an object")),
        };
        if matcher.trim().is_empty() {
            return Err(format!("{key}.{alias}: match is empty"));
        }
        if let Some(cap) = volume_cap {
            if !(cap > 0.0 && cap <= 1.0) {
                return Err(format!(
                    "{key}.{alias}: volume_cap must be above 0 and at most 1"
                ));
            }
        }
        out.insert(
            alias.clone(),
            Port {
                matcher,
                volume_cap,
            },
        );
    }
    Ok(out)
}

/// Reads the `audio` settings of an instrument entry: `outputs` and `inputs` (alias to a name fragment, or
/// `{"match": ..., "volume_cap": ...}`), `alert_output` and `verify_input` (`auto` or an alias), `volume_cap` and
/// `quiet_volume` (wpctl scale, 0 to 1), `quiet_hours` (`{"start":"22:00","end":"07:00"}` or `false`), `capture_dir`,
/// `retention_files`, `retention_days`, `max_record_s`, and `reserved` (more `vid:pid` values or node-name fragments;
/// `08bb:29c0` is always reserved).
pub fn parse_config(c: &Value) -> Result<Config, String> {
    // [@ANCHOR: audio_config]
    let mut cfg = Config {
        outputs: ports(c, "outputs")?,
        inputs: ports(c, "inputs")?,
        ..Config::default()
    };
    let text = |k: &str| c.get(k).and_then(|x| x.as_str()).map(String::from);
    let num = |k: &str| c.get(k).and_then(|x| x.as_f64());
    if let Some(a) = text("alert_output") {
        if a != "auto" && !cfg.outputs.contains_key(&a) {
            return Err(format!(
                "alert_output {a:?} is neither \"auto\" nor an alias in outputs"
            ));
        }
        cfg.alert_output = a;
    }
    if let Some(a) = text("verify_input") {
        if a != "auto" && !cfg.inputs.contains_key(&a) {
            return Err(format!(
                "verify_input {a:?} is neither \"auto\" nor an alias in inputs"
            ));
        }
        cfg.verify_input = a;
    }
    if let Some(v) = num("volume_cap") {
        if !(v > 0.0 && v <= 1.0) {
            return Err("volume_cap must be above 0 and at most 1".into());
        }
        cfg.volume_cap = v;
    }
    if let Some(v) = num("quiet_volume") {
        if !(v > 0.0 && v <= 1.0) {
            return Err("quiet_volume must be above 0 and at most 1".into());
        }
        cfg.quiet_volume = v;
    }
    if cfg.quiet_volume > cfg.volume_cap {
        cfg.quiet_volume = cfg.volume_cap;
    }
    match c.get("quiet_hours") {
        None => {}
        Some(Value::Bool(false)) => cfg.quiet_hours = None,
        Some(q) => {
            let get = |k: &str| {
                q.get(k)
                    .and_then(|x| x.as_str())
                    .ok_or(format!("quiet_hours needs \"{k}\" like \"22:00\""))
                    .and_then(hhmm)
            };
            let (start, end) = (get("start")?, get("end")?);
            if start == end {
                return Err("quiet_hours start and end are the same time".into());
            }
            cfg.quiet_hours = Some(QuietHours { start, end });
        }
    }
    cfg.capture_dir = text("capture_dir").map(PathBuf::from);
    if let Some(n) = c.get("retention_files").and_then(|x| x.as_u64()) {
        cfg.retention_files = n as usize;
    }
    if let Some(n) = c.get("retention_days").and_then(|x| x.as_u64()) {
        cfg.retention_days = n;
    }
    if let Some(m) = num("max_record_s") {
        if !(1.0..=600.0).contains(&m) {
            return Err("max_record_s must be 1 to 600".into());
        }
        cfg.max_record_s = m;
    }
    if let Some(list) = c.get("reserved") {
        for x in list
            .as_array()
            .ok_or("reserved must be a list of strings")?
        {
            let s = x
                .as_str()
                .filter(|s| !s.trim().is_empty())
                .ok_or("reserved entries must be non-empty strings")?
                .to_ascii_lowercase();
            if !cfg.reserved.contains(&s) {
                cfg.reserved.push(s);
            }
        }
    }
    Ok(cfg)
}

/// Minutes since local midnight now.
pub fn local_minute_of_day() -> u32 {
    let mut t: libc::time_t = 0;
    // SAFETY: time and localtime_r write only to the locals given; tm is fully initialised by localtime_r on success.
    let tm = unsafe {
        libc::time(&mut t);
        let mut tm: libc::tm = std::mem::zeroed();
        libc::localtime_r(&t, &mut tm);
        tm
    };
    (tm.tm_hour as u32) * 60 + tm.tm_min as u32
}

pub struct Spec {
    pub name: &'static str,
    pub tier: Tier,
    pub desc: &'static str,
    pub props: &'static str,
    pub required: &'static [&'static str],
}

macro_rules! props {
    ($($rest:tt)*) => { concat!("{", $($rest)*, "}") };
}

pub const SPECS: &[Spec] = &[
    Spec {
        name: "bench_audio_devices",
        tier: Tier::ReadOnly,
        desc: "List the sound outputs and inputs PipeWire knows: name (the persistent node name), config alias, description, direction, ALSA card and device, bus, USB path (to use with the bench_usb_* tools) and vid:pid for USB cards, port type, the driver's jack or port availability (an unplugged jack or an HDMI port with no monitor reports 'no': supporting evidence only, never proof), default, sample rates, formats, channels, volume and mute, and the reserved flag. The two USB AUDIO CODEC cards (08bb:29c0) inside the VNWA are listed as reserved and are never opened by any audio tool. Opens no audio stream.",
        props: props!(r#""instrument":{"type":"string"},"device":{"type":"string","description":"one device: alias, node name or a fragment of its name (reserved devices are refused here; they only appear in the plain list)"},"direction":{"type":"string","enum":["output","input"],"description":"filters the list; with device, says which side (default output)"},"include_reserved":{"type":"boolean","description":"default true; false hides the reserved devices"},"levels":{"type":"boolean","description":"default true; read volume and mute with wpctl"}"#),
        required: &[],
    },
    Spec {
        name: "bench_audio_status",
        tier: Tier::ReadOnly,
        desc: "The audio setup: which helper programs are installed (pw-dump, pw-play, pw-record, wpctl, espeak-ng, spd-say, piper, ...), the effective safety limits (volume cap, quiet volume, quiet hours and whether it is quiet now), the capture directory with its retention, the reserved list, and the aliases from the config. Opens no audio stream.",
        props: props!(r#""instrument":{"type":"string"}"#),
        required: &[],
    },
];

fn instrument<'a>(insts: &'a [Instrument], args: &Value) -> Result<&'a Instrument, String> {
    let audio: Vec<&Instrument> = insts.iter().filter(|i| i.kind == Kind::Audio).collect();
    if let Some(n) = args.get("instrument").and_then(|n| n.as_str()) {
        return audio
            .iter()
            .copied()
            .find(|i| i.name == n)
            .ok_or_else(|| format!("no audio instrument named {n:?}"));
    }
    match audio.as_slice() {
        [one] => Ok(one),
        [] => Err("no audio instrument is configured".into()),
        many => Err(format!(
            "name the instrument: {}",
            many.iter()
                .map(|i| i.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )),
    }
}

pub fn tool_defs(insts: &[Instrument]) -> Vec<Tool> {
    SPECS
        .iter()
        .filter(|s| insts.iter().any(|i| i.kind == Kind::Audio && i.tier >= s.tier))
        .map(|s| {
            let schema = json!({"type": "object", "properties": serde_json::from_str::<Value>(s.props).unwrap(), "required": s.required});
            let Value::Object(schema) = schema else { unreachable!() };
            Tool::new(s.name, format!("[{:?}] {}", s.tier, s.desc), Arc::new(schema))
        })
        .collect()
}

/// Where to find a program on `PATH`.
fn on_path(program: &str) -> Option<PathBuf> {
    std::env::var_os("PATH").and_then(|p| {
        std::env::split_paths(&p)
            .map(|d| d.join(program))
            .find(|f| f.is_file())
    })
}

pub fn status(cfg: &Config, now_minute: u32) -> Value {
    let programs: Vec<Value> = [
        "pw-dump",
        "pw-play",
        "pw-record",
        "wpctl",
        "espeak-ng",
        "spd-say",
        "piper",
        "sox",
        "ffmpeg",
    ]
    .iter()
    .map(|p| json!({"program": p, "path": on_path(p)}))
    .collect();
    json!({
        "programs": programs,
        "volume_cap": cfg.volume_cap, "quiet_volume": cfg.quiet_volume,
        "volume_scale": "wpctl volume (0 to 1; the signal amplitude factor is the cube: 0.5 is about -18 dB)",
        "quiet_hours": cfg.quiet_hours.map(|q| json!({"start_minute": q.start, "end_minute": q.end})),
        "quiet_now": cfg.quiet_hours.is_some_and(|q| q.contains(now_minute)),
        "local_minute_of_day": now_minute,
        "capture_dir": cfg.capture_dir(), "retention_files": cfg.retention_files, "retention_days": cfg.retention_days,
        "max_record_s": cfg.max_record_s, "reserved": cfg.reserved,
        "alert_output": cfg.alert_output, "verify_input": cfg.verify_input,
        "outputs": cfg.outputs.iter().map(|(a, p)| json!({"alias": a, "match": p.matcher, "volume_cap": p.volume_cap})).collect::<Vec<_>>(),
        "inputs": cfg.inputs.iter().map(|(a, p)| json!({"alias": a, "match": p.matcher, "volume_cap": p.volume_cap})).collect::<Vec<_>>(),
    })
}

fn list_devices(run: &dyn Run, asound: &Path, cfg: &Config, args: &Value) -> Result<Value, String> {
    let mut devs = devices::list(run, asound, cfg)?;
    if let Some(name) = args.get("device").and_then(|d| d.as_str()) {
        let dir = match args.get("direction").and_then(|d| d.as_str()) {
            Some("input") => devices::Dir::Input,
            _ => devices::Dir::Output,
        };
        // resolve refuses reserved devices; to see a reserved card use the plain list.
        let found = devices::resolve(&devs, name, dir)?.node_name.clone();
        devs.retain(|d| d.node_name == found);
    }
    match args.get("direction").and_then(|d| d.as_str()) {
        None => {}
        Some("output") => devs.retain(|d| d.dir == devices::Dir::Output),
        Some("input") => devs.retain(|d| d.dir == devices::Dir::Input),
        Some(o) => return Err(format!("direction {o:?} is not output or input")),
    }
    if args.get("include_reserved").and_then(|v| v.as_bool()) == Some(false) {
        devs.retain(|d| d.reserved.is_none());
    }
    let want_levels = args.get("levels").and_then(|v| v.as_bool()) != Some(false);
    let mut out = Vec::new();
    for d in &devs {
        let mut j = d.json();
        j["volume_cap"] = json!(cfg.cap_for(d));
        // The volume of a reserved device is not read either: it is not touched in any way.
        if want_levels && d.reserved.is_none() {
            match levels::get(run, d.id) {
                Ok(l) => {
                    j["volume"] = json!(l.volume);
                    j["muted"] = json!(l.muted);
                    j["gain_db"] = json!(levels::gain_db(l.volume));
                }
                Err(e) => j["levels_error"] = json!(e),
            }
        }
        out.push(j);
    }
    Ok(
        json!({"devices": out, "volume_scale": "wpctl volume; the signal amplitude factor is the cube"}),
    )
}

pub fn call(insts: &[Instrument], name: &str, args: &Value) -> Result<Value, String> {
    call_with(&Process, Path::new("/proc/asound"), insts, name, args)
}

/// `call` with the program runner and the `/proc/asound` directory given: production passes the process runner and the real directory.
pub fn call_with(
    run: &dyn Run,
    asound: &Path,
    insts: &[Instrument],
    name: &str,
    args: &Value,
) -> Result<Value, String> {
    let spec = SPECS
        .iter()
        .find(|s| s.name == name)
        .ok_or_else(|| format!("unknown tool {name}"))?;
    let inst = instrument(insts, args)?;
    if inst.tier < spec.tier {
        return Err(format!(
            "{name} needs tier {:?}; {:?} is configured at {:?}",
            spec.tier, inst.name, inst.tier
        ));
    }
    let cfg = &inst.audio;
    match name {
        "bench_audio_devices" => list_devices(run, asound, cfg, args),
        "bench_audio_status" => Ok(status(cfg, local_minute_of_day())),
        _ => Err(format!("{name} is not implemented")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use testkit::{recorded, Scripted};

    fn insts(cfg: &str) -> Vec<Instrument> {
        crate::config::parse(&format!(
            r#"{{"instruments":{{"snd":{{"driver":"audio"{cfg}}}}}}}"#
        ))
        .unwrap()
    }

    // Tests [@ANCHOR: audio_config]
    #[test]
    fn the_config_defaults_and_the_vnwa_codec_is_always_reserved() {
        let i = insts("");
        let c = &i[0].audio;
        assert_eq!(c.reserved, vec![VNWA_CODEC.to_string()]);
        assert_eq!((c.volume_cap, c.alert_output.as_str()), (0.5, "auto"));
        assert_eq!(
            c.quiet_hours,
            Some(QuietHours {
                start: 1320,
                end: 420
            })
        );
        let i = insts(r#","reserved":["1234:abcd","Radio"]"#);
        assert_eq!(i[0].audio.reserved, vec![VNWA_CODEC, "1234:abcd", "radio"]);
    }

    #[test]
    fn aliases_caps_and_quiet_hours_parse_and_bad_ones_are_refused() {
        let i = insts(
            r#","outputs":{"desk":"Speaker","tv":{"match":"HDMI 1","volume_cap":0.2}},"inputs":{"mic":"Digital Microphone"},
            "alert_output":"desk","verify_input":"mic","volume_cap":0.4,"quiet_volume":0.1,
            "quiet_hours":{"start":"23:30","end":"06:15"},"max_record_s":20"#,
        );
        let c = &i[0].audio;
        assert_eq!(c.outputs["tv"].volume_cap, Some(0.2));
        assert_eq!(
            c.quiet_hours,
            Some(QuietHours {
                start: 23 * 60 + 30,
                end: 6 * 60 + 15
            })
        );
        let bad = |extra: &str| {
            crate::config::parse(&format!(
                r#"{{"instruments":{{"s":{{"driver":"audio"{extra}}}}}}}"#
            ))
            .is_err()
        };
        assert!(bad(r#","alert_output":"nope""#));
        assert!(bad(r#","verify_input":"nope""#));
        assert!(bad(r#","volume_cap":1.5"#));
        assert!(bad(r#","volume_cap":0"#));
        assert!(bad(r#","quiet_hours":{"start":"25:00","end":"07:00"}"#));
        assert!(bad(r#","quiet_hours":{"start":"07:00","end":"07:00"}"#));
        assert!(bad(r#","outputs":{"auto":"x"}"#));
        assert!(bad(r#","outputs":{"a":{"volume_cap":0.2}}"#));
        assert!(bad(r#","max_record_s":0"#));
        assert!(insts(r#","quiet_hours":false"#)[0]
            .audio
            .quiet_hours
            .is_none());
    }

    #[test]
    fn quiet_hours_cross_midnight() {
        let q = QuietHours {
            start: 22 * 60,
            end: 7 * 60,
        };
        assert!(q.contains(23 * 60) && q.contains(0) && q.contains(6 * 60 + 59));
        assert!(!q.contains(7 * 60) && !q.contains(12 * 60) && !q.contains(22 * 60 - 1));
        let day = QuietHours {
            start: 13 * 60,
            end: 14 * 60,
        };
        assert!(day.contains(13 * 60 + 30) && !day.contains(14 * 60));
    }

    #[test]
    fn the_quiet_volume_never_exceeds_the_cap() {
        assert_eq!(
            insts(r#","volume_cap":0.1,"quiet_volume":0.3"#)[0]
                .audio
                .quiet_volume,
            0.1
        );
    }

    #[test]
    fn the_device_list_reads_levels_for_free_devices_and_not_for_reserved_ones() {
        let i = insts(r#","outputs":{"desk":"Speaker"}"#);
        let all = devices::list(&Scripted::new(&[]), &recorded(), &i[0].audio).unwrap();
        let speaker = all
            .iter()
            .find(|d| d.alias.as_deref() == Some("desk"))
            .unwrap();
        let run = Scripted::new(&[(speaker.id, 0.52, false)]);
        let v = call_with(&run, &recorded(), &i, "bench_audio_devices", &json!({})).unwrap();
        let list = v["devices"].as_array().unwrap();
        let sp = list.iter().find(|d| d["alias"] == "desk").unwrap();
        assert_eq!(
            (sp["volume"].as_f64(), sp["muted"].as_bool()),
            (Some(0.52), Some(false))
        );
        let reserved: Vec<&Value> = list.iter().filter(|d| d["reserved"] == true).collect();
        assert!(reserved.len() >= 2);
        assert!(reserved
            .iter()
            .all(|d| d.get("volume").is_none() && d["usb_id"] == "08bb:29c0"));
        let asked: Vec<String> = run.calls.borrow().clone();
        for r in &reserved {
            let id = r["pipewire_id"].to_string();
            assert!(
                !asked.iter().any(|c| c.ends_with(&format!(" {id}"))),
                "a reserved device was queried: {id}"
            );
        }
        let no_res = call_with(
            &run,
            &recorded(),
            &i,
            "bench_audio_devices",
            &json!({"include_reserved": false, "direction": "output", "levels": false}),
        )
        .unwrap();
        assert!(no_res["devices"]
            .as_array()
            .unwrap()
            .iter()
            .all(|d| d["reserved"] == false && d["direction"] == "output"));
    }

    #[test]
    fn one_device_by_alias_and_a_reserved_one_is_refused() {
        let i = insts(r#","outputs":{"desk":"Speaker"}"#);
        let run = Scripted::new(&[]);
        let one = call_with(
            &run,
            &recorded(),
            &i,
            "bench_audio_devices",
            &json!({"device": "desk", "levels": false}),
        )
        .unwrap();
        assert_eq!(one["devices"].as_array().unwrap().len(), 1);
        let e = call_with(
            &run,
            &recorded(),
            &i,
            "bench_audio_devices",
            &json!({"device": "USB_AUDIO_CODEC", "direction": "input"}),
        )
        .unwrap_err();
        assert!(e.contains("reserved"), "{e}");
    }

    #[test]
    fn status_reports_limits_and_the_tier_gate_works() {
        let i = insts(r#","tier":"read_only""#);
        let v = call_with(
            &Scripted::new(&[]),
            &recorded(),
            &i,
            "bench_audio_status",
            &json!({}),
        )
        .unwrap();
        assert_eq!(v["volume_cap"], 0.5);
        assert!(v["programs"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["program"] == "wpctl"));
        let defs = tool_defs(&i);
        assert!(defs.iter().all(|t| t.name.starts_with("bench_audio_")));
        assert!(call_with(
            &Scripted::new(&[]),
            &recorded(),
            &i,
            "bench_audio_nope",
            &json!({})
        )
        .is_err());
    }
}
