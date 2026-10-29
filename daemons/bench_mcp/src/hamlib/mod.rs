// SPDX-License-Identifier: AGPL-3.0-or-later
//! Hamlib rigs for bench_mcp: receivers and (for rigs the owner has attested into a dummy load) transmitters, driven through the
//! `rigctld` daemon's documented TCP protocol. Hamlib is LGPL and is not linked or copied here: the daemon is a separate program,
//! started by this module (bound to 127.0.0.1) or already running, and the models list comes from running `rigctl`.
//!
//! One configured instrument is one rig. The entry in the bench config (`BENCH_MCP_CONFIG`) is either
//! `{"driver": "hamlib", "tier": "read_config", "model": 1}` (the Hamlib dummy rig; this module starts and stops its rigctld) or
//! `{"driver": "hamlib", "tier": "read_only", "rigctld_port": 4532}` (an rigctld that is already running on this machine).
//! The keys are listed at `parse_config`; the safety model is in the manual page `instruments/hamlib/limits`.

#[cfg(test)]
pub mod testkit;

pub mod control;
pub mod diag;
pub mod models;
pub mod proto;
pub mod rigd;
pub mod status;
pub mod tx;

use crate::audio::Process;
use crate::config::{Instrument, Kind, Tier};
use proto::Client;
use rmcp::model::Tool;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

/// A managed rigctld: this server starts it for the rig.
#[derive(Clone, Debug, PartialEq)]
pub struct Managed {
    pub model: u32,
    pub device: Option<String>,
    pub speed: Option<u32>,
    pub conf: Vec<(String, String)>,
    pub port: Option<u16>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Source {
    Managed(Managed),
    Existing { host: String, port: u16 },
}

/// The `hamlib` settings of an instrument entry.
#[derive(Clone, Debug, PartialEq)]
pub struct Config {
    pub source: Source,
    /// How long one rigctld command may take before the call fails, seconds.
    pub reply_timeout_s: u64,
    /// The owner's receive range limits in Hz, applied in addition to the ranges the rig reports. Empty: the rig's own.
    pub rx_ranges_hz: Vec<(f64, f64)>,
    pub managed_dir: Option<PathBuf>,
}

impl Default for Config {
    fn default() -> Config {
        Config {
            source: Source::Existing {
                host: "127.0.0.1".into(),
                port: 4532,
            },
            reply_timeout_s: 3,
            rx_ranges_hz: vec![],
            managed_dir: None,
        }
    }
}

const BAUDS: &[u32] = &[
    1200, 2400, 4800, 9600, 19200, 38400, 57600, 115200, 230400, 460800, 921600,
];

/// A string safe to put in a file name.
pub fn safe_name(s: &str) -> String {
    // [@ANCHOR: hamlib_safe_name]
    s.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

fn is_loopback(host: &str) -> bool {
    matches!(host, "127.0.0.1" | "localhost" | "::1")
}

fn token_ok(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 120
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_./:-".contains(&b))
}

/// Reads the hamlib settings of an instrument entry: `model` (Hamlib model number; the module starts a rigctld for it) with `device`
/// (a /dev path, or host:port for model 2), `speed` (serial baud rate), `set_conf` (an object of Hamlib config keys and values) and
/// `listen_port`; OR `rigctld_port` (with `rigctld_host`, loopback unless `allow_remote_rigctld` is true) for a rigctld that already
/// runs. Also `reply_timeout_s` (1 to 30, default 3), `rx_ranges_hz` (a list of [low, high]) and `managed_dir`.
pub fn parse_config(c: &Value) -> Result<Config, String> {
    // [@ANCHOR: hamlib_config]
    let mut cfg = Config::default();
    let model = c.get("model");
    let port = c.get("rigctld_port");
    cfg.source = match (model, port) {
        (Some(_), Some(_)) => return Err("give either model (this server starts rigctld) or rigctld_port (one already runs), not both".into()),
        (None, None) => return Err("a hamlib rig needs model (a Hamlib model number) or rigctld_port".into()),
        (Some(m), None) => {
            let model = m.as_u64().filter(|m| (1..=1_000_000).contains(m)).ok_or("model must be a whole number from 1 to 1000000 (bench_hamlib_models lists them)")? as u32;
            let device = match c.get("device") {
                None | Some(Value::Null) => None,
                Some(d) => {
                    let d = d.as_str().filter(|d| token_ok(d)).ok_or("device must be a path such as /dev/serial/by-id/usb-... (letters, digits and _ . / : -)")?;
                    if model == 2 {
                        let (h, p) = d.rsplit_once(':').ok_or("model 2 (NET rigctl) needs device as host:port")?;
                        if !is_loopback(h) || p.parse::<u16>().is_err() {
                            return Err("model 2 device must be 127.0.0.1:<port> (rigctld is never shared on a network interface)".into());
                        }
                    } else if !d.starts_with("/dev/") || d.contains("..") {
                        return Err(format!("device {d:?} must be a path under /dev without .."));
                    }
                    Some(d.to_string())
                }
            };
            let speed = match c.get("speed") {
                None | Some(Value::Null) => None,
                Some(s) => Some(s.as_u64().and_then(|s| u32::try_from(s).ok()).filter(|s| BAUDS.contains(s)).ok_or_else(|| format!("speed must be one of {BAUDS:?}"))?),
            };
            let mut conf = Vec::new();
            if let Some(o) = c.get("set_conf") {
                for (k, v) in o.as_object().ok_or("set_conf must be an object of key: value")? {
                    let v = match v {
                        Value::String(s) => s.clone(),
                        Value::Number(n) => n.to_string(),
                        Value::Bool(b) => (if *b { "1" } else { "0" }).to_string(),
                        _ => return Err(format!("set_conf {k}: the value must be a string, number or boolean")),
                    };
                    if !token_ok(k) || !token_ok(&v) {
                        return Err(format!("set_conf {k}={v}: keys and values use letters, digits and _ . / : - only"));
                    }
                    conf.push((k.clone(), v));
                }
            }
            let listen = match c.get("listen_port") {
                None | Some(Value::Null) => None,
                Some(p) => Some(p.as_u64().and_then(|p| u16::try_from(p).ok()).filter(|p| *p >= 1024).ok_or("listen_port must be 1024 to 65535")?),
            };
            Source::Managed(Managed { model, device, speed, conf, port: listen })
        }
        (None, Some(p)) => {
            let port = p.as_u64().and_then(|p| u16::try_from(p).ok()).filter(|p| *p > 0).ok_or("rigctld_port must be 1 to 65535")?;
            let host = c.get("rigctld_host").map(|h| h.as_str().ok_or("rigctld_host must be a string")).transpose()?.unwrap_or("127.0.0.1");
            let remote_ok = c.get("allow_remote_rigctld").and_then(|v| v.as_bool()).unwrap_or(false);
            if !is_loopback(host) && !remote_ok {
                return Err(format!("rigctld_host {host:?} is not this machine. rigctld has no authentication and must not be shared on a network; set allow_remote_rigctld true only if the owner accepts that"));
            }
            if !token_ok(host) {
                return Err("rigctld_host has characters that are not allowed".into());
            }
            Source::Existing { host: host.to_string(), port }
        }
    };
    if let Some(t) = c.get("reply_timeout_s") {
        cfg.reply_timeout_s = t
            .as_u64()
            .filter(|t| (1..=30).contains(t))
            .ok_or("reply_timeout_s must be a whole number from 1 to 30")?;
    }
    if let Some(r) = c.get("rx_ranges_hz") {
        for pair in r
            .as_array()
            .ok_or("rx_ranges_hz must be a list of [low, high] pairs in Hz")?
        {
            let p = pair
                .as_array()
                .filter(|p| p.len() == 2)
                .ok_or("each rx_ranges_hz entry is [low, high]")?;
            let (lo, hi) = (
                p[0].as_f64().unwrap_or(f64::NAN),
                p[1].as_f64().unwrap_or(f64::NAN),
            );
            if !(lo.is_finite() && hi.is_finite() && lo >= 0.0 && hi > lo) {
                return Err("rx_ranges_hz entries need finite 0 <= low < high".into());
            }
            cfg.rx_ranges_hz.push((lo, hi));
        }
    }
    if let Some(d) = c.get("managed_dir") {
        cfg.managed_dir = Some(PathBuf::from(
            d.as_str()
                .filter(|d| d.starts_with('/') && !d.contains(".."))
                .ok_or("managed_dir must be an absolute path without ..")?,
        ));
    }
    Ok(cfg)
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

/// Every tool of this module. A new tool is an entry here, an arm in `call`, a page under `manual/tools/` and a test.
pub const SPECS: &[Spec] = &[
    Spec {
        name: "bench_hamlib_models",
        tier: Tier::ReadOnly,
        desc: "HAMLIB: the radio models Hamlib supports (from `rigctl -l`), filtered by `search` (words of the manufacturer or model name), or with `model` (a Hamlib model number) what that model can do: readable and settable levels and functions, modes, VFOs, receive and transmit ranges with power, PTT type. This is what the backend declares, not what one unit does. Needs no rig.",
        props: props!(r#""instrument":{"type":"string"},"search":{"type":"string","description":"words to find in the manufacturer or model name"},"model":{"type":"integer","description":"a Hamlib model number: describe it instead of listing"},"limit":{"type":"integer","description":"most models to list, 1 to 500, default 50"}"#),
        required: &[],
    },
    Spec {
        name: "bench_hamlib_rigs",
        tier: Tier::ReadOnly,
        desc: "HAMLIB: the configured rigs and their state: tier, whether rigctld is managed by this server and running, whether the rig answers now, whether transmitting is configured. Starts nothing. With `instrument`, one rig.",
        props: props!(r#""instrument":{"type":"string"}"#),
        required: &[],
    },
    Spec {
        name: "bench_hamlib_status",
        tier: Tier::ReadOnly,
        desc: "HAMLIB receiver status in one call: frequency, mode and passband, VFO, split, RIT/XIT, PTT state, carrier detect, power state, rig info, signal strength (dB relative to S9 and S-units), AF/RF gain, squelch, AGC, attenuator, preamp, noise blanker/reduction and the other levels and functions the rig supports. A value the rig cannot give is listed under `unavailable` with the reason. Changes nothing.",
        props: props!(r#""instrument":{"type":"string"}"#),
        required: &[],
    },
    Spec {
        name: "bench_hamlib_get",
        tier: Tier::ReadOnly,
        desc: "HAMLIB: one value, read now. `item` is frequency, mode, vfo, split, rit, xit, ptt, squelch, power, info, memory, channel (with `channel`), level or function (with `name`, such as AF, ATT, NB), or signal (the signal strength; `count` readings `interval_ms` apart give the mean and spread). Changes nothing.",
        props: props!(r#""instrument":{"type":"string"},"item":{"type":"string","description":"frequency, mode, vfo, split, rit, xit, ptt, squelch, power, info, memory, channel, level, function or signal"},"name":{"type":"string","description":"level or function name for item level or function, such as STRENGTH, AF, ATT, NB"},"channel":{"type":"integer","description":"memory channel number for item channel"},"count":{"type":"integer","description":"readings for item signal, 1 to 50, default 1"},"interval_ms":{"type":"integer","description":"milliseconds between readings, 0 to 2000, default 100"}"#),
        required: &["item"],
    },
    Spec {
        name: "bench_hamlib_rigctld",
        tier: Tier::ReadConfig,
        desc: "HAMLIB: the rigctld daemon of a rig this server manages: `status` (running, port, restarts, last exit), `start`, `stop` or `restart`. It listens on 127.0.0.1 only. A rig configured with an existing rigctld is not started or stopped by this server. Other tools start the daemon by themselves when needed.",
        props: props!(r#""instrument":{"type":"string"},"action":{"type":"string","description":"status, start, stop or restart"}"#),
        required: &["action"],
    },
    Spec {
        name: "bench_hamlib_set",
        tier: Tier::ReadConfig,
        desc: "HAMLIB receiver control, each change read back: `frequency_hz` (inside the rig's receive range and the config's rx_ranges_hz), `mode` with `passband_hz`, `vfo`, `split` ({enabled, tx_vfo}), `rit_hz`, `xit_hz`, `levels` (AF, RF, SQL, IF, APF, NR, NB, PBT_IN, PBT_OUT, CWPITCH, NOTCHF, BAL, AGC, ATT, PREAMP) and `functions` (NB, NR, ANF, APF, AIP, MN, LOCK, MUTE, SQL...). Every argument is checked before anything is sent. Transmit-side settings (RF power, microphone gain, VOX, the tuner) are refused. Refuses while the rig reports PTT on. Never keys the transmitter.",
        props: props!(r#""instrument":{"type":"string"},"frequency_hz":{"type":"number","description":"receive frequency in Hz"},"mode":{"type":"string","description":"Hamlib mode name such as USB, LSB, CW, AM, FM"},"passband_hz":{"type":"number","description":"passband width in Hz, 0 for the rig's default; needs mode"},"vfo":{"type":"string","description":"VFOA, VFOB, MEM, Main, Sub..."},"split":{"type":"object","description":"{enabled: bool, tx_vfo: VFOB}"},"rit_hz":{"type":"number","description":"-9999 to 9999"},"xit_hz":{"type":"number","description":"-9999 to 9999"},"levels":{"type":"object","description":"level name to value, for example {\"AF\":0.5,\"ATT\":20}; fractions are 0 to 1, ATT and PREAMP in dB"},"functions":{"type":"object","description":"function name to true or false, for example {\"NB\":true}"}"#),
        required: &[],
    },
    Spec {
        name: "bench_hamlib_memory",
        tier: Tier::ReadConfig,
        desc: "HAMLIB: recall memory channel `channel` (selects the memory VFO and the channel) and return the frequency and mode the rig then shows. Warns when the channel holds a frequency outside the receive limits. Refuses while the rig reports PTT on.",
        props: props!(r#""instrument":{"type":"string"},"channel":{"type":"integer","description":"memory channel number, 0 to 99999"}"#),
        required: &["channel"],
    },
    Spec {
        name: "bench_hamlib_scan",
        tier: Tier::ReadConfig,
        desc: "HAMLIB: step the receiver from `start_hz` to `stop_hz` in `step_hz` steps, dwelling `dwell_ms` (20 to 5000, default 200) at each, and list signal strength and squelch state per frequency with the strongest `top` ranked. At most 400 steps and 120 s in all; the range must be inside the receive limits. `stop_when_above_db` ends the scan at the first frequency stronger than that many dB relative to S9. The rig's frequency and mode are restored afterwards.",
        props: props!(r#""instrument":{"type":"string"},"start_hz":{"type":"number","description":"first frequency, Hz"},"stop_hz":{"type":"number","description":"last frequency, Hz"},"step_hz":{"type":"number","description":"step, Hz"},"dwell_ms":{"type":"number","description":"milliseconds at each frequency, 20 to 5000, default 200"},"mode":{"type":"string","description":"mode to scan in; default: the rig's current mode"},"top":{"type":"integer","description":"how many of the strongest to rank, 1 to 100, default 10"},"stop_when_above_db":{"type":"number","description":"stop at the first reading above this many dB relative to S9"}"#),
        required: &["start_hz", "stop_hz", "step_hz"],
    },
    Spec {
        name: "bench_hamlib_diagnose",
        tier: Tier::ReadOnly,
        desc: "HAMLIB connection test for a rig that does not answer: the rigctld program, the model, the serial speed against what the backend allows, the device (exists, permissions, other processes holding it open), the DTR/RTS settings, whether the daemon runs and the rig answers, and the daemon's log. Never opens the serial port and never starts the daemon.",
        props: props!(r#""instrument":{"type":"string"}"#),
        required: &[],
    },
];

fn instrument<'a>(insts: &'a [Instrument], args: &Value) -> Result<&'a Instrument, String> {
    // [@ANCHOR: hamlib_instrument]
    let ours: Vec<&Instrument> = insts.iter().filter(|i| i.kind == Kind::Hamlib).collect();
    if let Some(n) = args.get("instrument").and_then(|n| n.as_str()) {
        return ours
            .iter()
            .copied()
            .find(|i| i.name == n)
            .ok_or_else(|| format!("no hamlib instrument named {n:?}"));
    }
    match ours.as_slice() {
        [one] => Ok(one),
        [] => Err("no hamlib instrument is configured".into()),
        many => Err(format!(
            "name the rig: {}",
            many.iter()
                .map(|i| i.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )),
    }
}

/// The tools to list: an entry of `SPECS` appears only when a hamlib rig is configured at or above its tier.
pub fn tool_defs(insts: &[Instrument]) -> Vec<Tool> {
    SPECS
        .iter()
        .filter(|s| insts.iter().any(|i| i.kind == Kind::Hamlib && i.tier >= s.tier))
        .map(|s| {
            let schema = json!({"type": "object", "properties": serde_json::from_str::<Value>(s.props).unwrap(), "required": s.required});
            let Value::Object(schema) = schema else { unreachable!() };
            Tool::new(s.name, format!("[{:?}] {}", s.tier, s.desc), Arc::new(schema))
        })
        .collect()
}

/// A connection to the rig's rigctld, starting the managed daemon if it is not running.
pub fn open(inst: &Instrument) -> Result<Client, String> {
    // [@ANCHOR: hamlib_open]
    let (host, port) = rigd::endpoint(&inst.name, &inst.hamlib)?;
    Client::connect(
        &host,
        port,
        Duration::from_secs(inst.hamlib.reply_timeout_s),
    )
    .map_err(|e| e.to_string())
}

pub fn call(insts: &[Instrument], name: &str, args: &Value) -> Result<Value, String> {
    // [@ANCHOR: hamlib_call]
    let spec = SPECS
        .iter()
        .find(|s| s.name == name)
        .ok_or_else(|| format!("unknown tool {name}"))?;
    if name == "bench_hamlib_models" {
        let tier_ok = insts
            .iter()
            .any(|i| i.kind == Kind::Hamlib && i.tier >= spec.tier);
        return if tier_ok {
            models::tool(&Process, args)
        } else {
            Err(format!(
                "{name} needs a hamlib instrument at tier {:?}",
                spec.tier
            ))
        };
    }
    if name == "bench_hamlib_rigs" {
        return rigs(insts, args);
    }
    let inst = instrument(insts, args)?;
    if inst.tier < spec.tier {
        return Err(format!(
            "{name} needs tier {:?}; {:?} is configured at {:?}",
            spec.tier, inst.name, inst.tier
        ));
    }
    let lock = crate::instrument_lock(&inst.name);
    let _held = lock.lock().unwrap_or_else(|e| e.into_inner());
    match name {
        "bench_hamlib_status" => status::status(&mut open(inst)?, &inst.name),
        "bench_hamlib_get" => status::get(&mut open(inst)?, args),
        "bench_hamlib_set" => control::set(&mut open(inst)?, &inst.hamlib, args),
        "bench_hamlib_memory" => control::memory(&mut open(inst)?, &inst.hamlib, args),
        "bench_hamlib_scan" => control::scan(&mut open(inst)?, &inst.hamlib, args),
        "bench_hamlib_diagnose" => diag::diagnose(&Process, &inst.name, &inst.hamlib),
        "bench_hamlib_rigctld" => rigctld_action(inst, args),
        _ => Err(format!("{name} does not apply to {:?}", inst.name)),
    }
}

/// `bench_hamlib_rigctld`: status, start, stop, restart.
fn rigctld_action(inst: &Instrument, args: &Value) -> Result<Value, String> {
    // [@ANCHOR: hamlib_rigctld_tool]
    let action = args
        .get("action")
        .and_then(|a| a.as_str())
        .ok_or("action is required: status, start, stop or restart")?;
    let (name, cfg) = (&inst.name, &inst.hamlib);
    match action {
        "status" => Ok(rigd::status(name, cfg)),
        "start" => rigd::start(name, cfg),
        "stop" => rigd::stop(name, cfg),
        "restart" => {
            rigd::stop(name, cfg)?;
            rigd::start(name, cfg)
        }
        other => Err(format!(
            "action {other:?} is not status, start, stop or restart"
        )),
    }
}

/// `bench_hamlib_rigs`: every configured rig, whether its daemon runs and whether the rig answers. Starts nothing.
fn rigs(insts: &[Instrument], args: &Value) -> Result<Value, String> {
    // [@ANCHOR: hamlib_rigs]
    let only = args.get("instrument").and_then(|n| n.as_str());
    let mut out = Vec::new();
    for i in insts
        .iter()
        .filter(|i| i.kind == Kind::Hamlib && only.is_none_or(|n| n == i.name))
    {
        let daemon = rigd::status(&i.name, &i.hamlib);
        let endpoint = match (
            &i.hamlib.source,
            daemon["running"].as_bool(),
            daemon["port"].as_u64(),
        ) {
            (Source::Existing { host, port }, _, _) => Some((host.clone(), *port)),
            (_, Some(true), Some(p)) => Some(("127.0.0.1".to_string(), p as u16)),
            _ => None,
        };
        let answers = match endpoint {
            None => json!("not started: the daemon starts at the first call that needs the rig"),
            Some((h, p)) => {
                match Client::connect(&h, p, Duration::from_secs(i.hamlib.reply_timeout_s))
                    .and_then(|mut c| c.read_number("get_freq", Some("Frequency")))
                {
                    Ok(f) => json!({"answers": true, "frequency_hz": f}),
                    Err(e) => json!({"answers": false, "error": e.to_string()}),
                }
            }
        };
        out.push(json!({"name": i.name, "tier": format!("{:?}", i.tier), "rigctld": daemon, "rig": answers}));
    }
    if out.is_empty() {
        return Err(match only {
            Some(n) => format!("no hamlib instrument named {n:?}"),
            None => "no hamlib instrument is configured".into(),
        });
    }
    Ok(json!({"rigs": out}))
}

/// Stops what this module started: the managed daemons. Called when the server stops.
pub fn shutdown() {
    rigd::shutdown();
}

#[cfg(test)]
mod tests;
