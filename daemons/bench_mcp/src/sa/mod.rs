// SPDX-License-Identifier: AGPL-3.0-or-later
//! Siglent SSA3000X-series spectrum analyzer with the tracking generator (written against an SSA3021X, firmware 1.3.9.10),
//! SCPI over the LAN raw socket (port 5025). Only the base analyzer and the TG option are used: nothing here depends on the
//! trial options (Meas, EMI, CAT), which lapse. The tracking generator is treated like any other setting (`read_config`, no
//! confirm flags; its output is low, 0 dBm at most); it is off unless a tool turns it on, and the S21 tools switch it off again by default.
//!
//! The entry in the bench config (`BENCH_MCP_CONFIG`):
//! `{"driver": "siglent_ssa", "tier": "read_config", "host": "192.168.10.42", "port": 5025}` (`max_tg_dbm` is an optional extra cap)..
//! Files: `mod.rs` (config, tool list, shared helpers, status, dispatch), `sweep.rs` (sweep, trace, peaks, CSV, marker,
//! screenshot), `tg.rs` (tracking generator, through reference, S21), `testkit.rs` (the fake instrument for the tests).

#[cfg(test)]
pub mod testkit;

mod sweep;
mod tg;

use crate::config::{Instrument, Kind, Tier};
use crate::link::Link;
use rmcp::model::Tool;
use serde_json::{json, Value};
use std::sync::Arc;

// [@ANCHOR: bench_mcp_sa_limits]
/// The model prefix the instrument must report in `*IDN?` (the SSA3000X family). Anything else is another device at that
/// address: stop before sending it any other command.
const MODEL_PREFIX: &str = "SSA3";
/// Read timeout of an ordinary exchange, seconds.
const REPLY_TIMEOUT_S: u64 = 5;
/// Queries are tried this many times in all; commands that change the instrument are never repeated blind.
const READ_TRIES: u32 = 3;
/// Attempts to switch the tracking generator off and see it off.
const OFF_TRIES: u32 = 3;
/// The tracking generator's rated output range, dBm (guide section 3.9.2).
pub(crate) const TG_RATED_MIN_DBM: f64 = -20.0;
pub(crate) const TG_RATED_MAX_DBM: f64 = 0.0;

// [@ANCHOR: bench_mcp_sa_config]
/// The `siglent_ssa` settings of an instrument entry.
#[derive(Clone, Debug, PartialEq)]
pub struct Config {
    /// An optional owner cap on the tracking-generator level, dBm (the unit's own range is -20 to 0 dBm, which is the default: no extra cap).
    pub max_tg_dbm: f64,
}

impl Default for Config {
    fn default() -> Config {
        Config {
            max_tg_dbm: TG_RATED_MAX_DBM,
        }
    }
}

/// Reads `max_tg_dbm`. A value that is not a finite number inside the generator's rating is refused when the config file is read.
pub fn parse_config(c: &Value) -> Result<Config, String> {
    // [@ANCHOR: bench_mcp_sa_parse_config]
    let mut cfg = Config::default();
    if let Some(v) = c.get("max_tg_dbm") {
        let x = v.as_f64().ok_or("max_tg_dbm must be a number")?;
        if !(x.is_finite() && (TG_RATED_MIN_DBM..=TG_RATED_MAX_DBM).contains(&x)) {
            return Err(format!(
                "max_tg_dbm {x} must be {TG_RATED_MIN_DBM} to {TG_RATED_MAX_DBM} dBm (the tracking generator's rating)"
            ));
        }
        cfg.max_tg_dbm = x;
    }
    Ok(cfg)
}

// [@ANCHOR: bench_mcp_sa_spec]
/// One tool: name, tier, description for the model, JSON-schema properties and required keys.
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

/// The sweep arguments shared by the sweep, normalize and S21 tools, as JSON-schema properties.
macro_rules! sweep_props {
    () => {
        concat!(
            r#""instrument":{"type":"string"},"#,
            r#""start_hz":{"type":"number","description":"first frequency, Hz (0 to the model's top, 2.1 GHz on the SSA3021X); give start_hz with stop_hz, or center_hz with span_hz"},"#,
            r#""stop_hz":{"type":"number","description":"last frequency, Hz; at least 100 Hz above start_hz"},"#,
            r#""center_hz":{"type":"number","description":"centre frequency, Hz (with span_hz)"},"#,
            r#""span_hz":{"type":"number","description":"span, Hz, at least 100"},"#,
            r#""rbw_hz":{"type":"number","description":"resolution bandwidth, Hz, 10 to 1000000; the unit rounds up to 10, 30, 100, 300 ... 1 MHz. Omitted: left as it is"},"#,
            r#""vbw_hz":{"type":"number","description":"video bandwidth, Hz, 1 to 1000000. Omitted: left as it is"},"#,
            r#""ref_level_dbm":{"type":"number","description":"reference level, dBm, -100 to 30. Omitted: left as it is"},"#,
            r#""attenuation_db":{"type":"integer","description":"input attenuator, dB, 0 to 50 (switches auto off). Omitted: left as it is"},"#,
            r#""detector":{"type":"string","enum":["positive","negative","sample","average","normal"],"description":"trace detector. Omitted: left as it is"},"#,
            r#""average_count":{"type":"integer","description":"sweeps averaged, 1 to 999; 1 or omitted is a single sweep"},"#,
            r#""continuous_after":{"type":"boolean","description":"true (default): put the unit back in continuous sweep when done"}"#
        )
    };
}

// [@ANCHOR: bench_mcp_sa_specs]
/// Every tool of this instrument. A new tool is a new entry here, a match arm in `call_with`, a page under
/// `manual/tools/<name>.md` and a test.
pub const SPECS: &[Spec] = &[
    Spec {
        name: "bench_sa_status",
        tier: Tier::ReadOnly,
        desc: "Siglent SSA3000X spectrum analyzer: identity (model, serial, firmware), installed options, the tracking generator (on or off, level, the config's cap) and the sweep settings (start, stop, RBW, VBW, reference level, attenuation, detector, sweep time, continuous or single). Changes nothing. Refuses an address that does not answer as an SSA3000X.",
        props: props!(r#""instrument":{"type":"string"}"#),
        required: &[],
    },
    Spec {
        name: "bench_sa_sweep",
        tier: Tier::ReadConfig,
        desc: "Siglent SSA3000X: set the frequency range (start_hz and stop_hz, or center_hz and span_hz) and optionally RBW, VBW, reference level, attenuation, detector and averaging, run ONE sweep, read the 751-point trace in dBm and save it as a CSV (freq_hz, level_dbm) in BENCH_MCP_OUT. Returns the settings the unit reports, the trace summary and the strongest peaks. Does not touch the tracking generator unless tg_off_after is true (then it is switched off afterwards and verified). The number of points is fixed by the unit.",
        props: props!(
            sweep_props!(),
            r#","peaks":{"type":"integer","description":"peaks to list, 0 to 50, default 5"},"#,
            r#""peak_threshold_db":{"type":"number","description":"a peak must be this far above the trace median, dB, default 10"},"#,
            r#""tg_off_after":{"type":"boolean","description":"true: switch the tracking generator off after the sweep (also when it fails) and verify it"}"#
        ),
        required: &[],
    },
    Spec {
        name: "bench_sa_marker",
        tier: Tier::ReadConfig,
        desc: "Siglent SSA3000X: marker 1 to 4 on trace 1: switch it on, optionally search (max = highest peak, next = next lower peak, left or right = next peak on that side) or place it at freq_hz, and read its frequency and level. Works on the trace the unit currently holds (run bench_sa_sweep first for a fresh one).",
        props: props!(
            r#""instrument":{"type":"string"},"#,
            r#""marker":{"type":"integer","description":"marker number 1 to 4, default 1"},"#,
            r#""search":{"type":"string","enum":["max","next","left","right"],"description":"peak search to run first"},"#,
            r#""freq_hz":{"type":"number","description":"place the marker here, Hz (not with search)"}"#
        ),
        required: &[],
    },
    Spec {
        name: "bench_sa_screenshot",
        tier: Tier::ReadOnly,
        desc: "Siglent SSA3000X: the screen as a 24-bit BMP (about 1.8 MB, 1024 x 600) saved in BENCH_MCP_OUT, read over the LAN with :HCOPy:SDUMp:DATA?. Returns the file, size and dimensions. Changes nothing.",
        props: props!(r#""instrument":{"type":"string"}"#),
        required: &[],
    },
    Spec {
        name: "bench_sa_tg_on",
        tier: Tier::ReadConfig,
        desc: "Siglent SSA3000X tracking generator: outputs RF on the TG OUTPUT connector at tg_level_dbm (-20 to 0 dBm; omitted: the level it is already set to). The level is set and read back before the output is switched on. It stays on until bench_sa_tg_off (or a tool with tg_off_after).",
        props: props!(
            r#""instrument":{"type":"string"},"#,
            r#""tg_level_dbm":{"type":"number","description":"output level, dBm, -20 to 0"}"#
        ),
        required: &[],
    },
    Spec {
        name: "bench_sa_tg_off",
        tier: Tier::ReadConfig,
        desc: "Siglent SSA3000X tracking generator: switch the TG output off and read it back to prove it (tried up to 3 times). Safe at any time; use it first when anything went wrong. If the unit will not confirm off, the answer says SAFE STATE NOT CONFIRMED and what to do by hand.",
        props: props!(r#""instrument":{"type":"string"}"#),
        required: &[],
    },
    Spec {
        name: "bench_sa_tg_level",
        tier: Tier::ReadConfig,
        desc: "Siglent SSA3000X tracking generator: set the output level, dBm (-20 to 0) WITHOUT changing whether the output is on, and read it back. Returns the level and the on or off state.",
        props: props!(
            r#""instrument":{"type":"string"},"#,
            r#""tg_level_dbm":{"type":"number","description":"output level, dBm, -20 to 0"}"#
        ),
        required: &["tg_level_dbm"],
    },
    Spec {
        name: "bench_sa_tg_normalize",
        tier: Tier::ReadConfig,
        desc: "Siglent SSA3000X tracking generator, step 1 of a through-normalised S21: with the TG output cabled straight to the RF input (the 'through'), switch the TG on at tg_level_dbm, sweep, and store the trace under `name` in this server (and as a CSV with its settings in BENCH_MCP_OUT) as the 0 dB reference. The TG is switched off afterwards unless leave_tg_on is true.",
        props: props!(
            sweep_props!(),
            r#","name":{"type":"string","description":"name of the reference, default \"default\" (letters, digits, - and _)"},"#,
            r#""tg_level_dbm":{"type":"number","description":"TG level, dBm, -20 to 0, default -20"},"#,
            r#""leave_tg_on":{"type":"boolean","description":"true: leave the TG output on afterwards; default false"}"#
        ),
        required: &[],
    },
    Spec {
        name: "bench_sa_tg_s21",
        tier: Tier::ReadConfig,
        desc: "Siglent SSA3000X tracking generator: a scalar S21 (transmission) sweep of the device between the TG output and the RF input. Switches the TG on at tg_level_dbm, sweeps, and saves a CSV (freq_hz, s21_db, measured_dbm). With `reference` (a bench_sa_tg_normalize name) or `reference_file` the values are dB relative to the through, and unset sweep arguments are taken from the reference; without one they are the measured level minus the TG level (cable and TG flatness included). The TG is switched off afterwards unless leave_tg_on is true.",
        props: props!(
            sweep_props!(),
            r#","tg_level_dbm":{"type":"number","description":"TG level, dBm, -20 to 0, default the reference's, else -20"},"#,
            r#""reference":{"type":"string","description":"name stored by bench_sa_tg_normalize"},"#,
            r#""reference_file":{"type":"string","description":"CSV written by bench_sa_tg_normalize (an earlier session)"},"#,
            r#""leave_tg_on":{"type":"boolean","description":"true: leave the TG output on afterwards; default false"}"#
        ),
        required: &[],
    },
];

fn instrument<'a>(insts: &'a [Instrument], args: &Value) -> Result<&'a Instrument, String> {
    // [@ANCHOR: bench_mcp_sa_instrument]
    let ours: Vec<&Instrument> = insts.iter().filter(|i| i.kind == Kind::Sa).collect();
    if let Some(n) = args.get("instrument").and_then(|n| n.as_str()) {
        return ours
            .iter()
            .copied()
            .find(|i| i.name == n)
            .ok_or_else(|| format!("no spectrum analyzer named {n:?}"));
    }
    match ours.as_slice() {
        [one] => Ok(one),
        [] => Err("no spectrum analyzer is configured".into()),
        many => Err(format!(
            "name the instrument: {}",
            many.iter()
                .map(|i| i.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )),
    }
}

/// The tools to list: an entry of `SPECS` appears only when a spectrum analyzer is configured at or above its tier.
pub fn tool_defs(insts: &[Instrument]) -> Vec<Tool> {
    // [@ANCHOR: bench_mcp_sa_tool_defs]
    SPECS
        .iter()
        .filter(|s| insts.iter().any(|i| i.kind == Kind::Sa && i.tier >= s.tier))
        .map(|s| {
            let schema = json!({"type": "object", "properties": serde_json::from_str::<Value>(s.props).unwrap(), "required": s.required});
            let Value::Object(schema) = schema else { unreachable!() };
            Tool::new(s.name, format!("[{:?}] {}", s.tier, s.desc), Arc::new(schema))
        })
        .collect()
}

/// A boolean argument that may be absent (false) but must not be anything else.
pub(crate) fn flag(args: &Value, key: &str, default: bool) -> Result<bool, String> {
    // [@ANCHOR: bench_mcp_sa_flag]
    match args.get(key) {
        None | Some(Value::Null) => Ok(default),
        Some(Value::Bool(b)) => Ok(*b),
        Some(_) => Err(format!("{key} must be true or false")),
    }
}

/// A number argument that may be absent (`None`) but, when given, must be finite and inside `lo..=hi`; the error names the
/// range and where the limit comes from.
pub(crate) fn number(
    args: &Value,
    key: &str,
    lo: f64,
    hi: f64,
    why: &str,
) -> Result<Option<f64>, String> {
    // [@ANCHOR: bench_mcp_sa_number]
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => {
            let x = v
                .as_f64()
                .ok_or_else(|| format!("{key} must be a number"))?;
            if x.is_finite() && (lo..=hi).contains(&x) {
                Ok(Some(x))
            } else {
                Err(format!("{key} {x} is outside {lo}..={hi} ({why})"))
            }
        }
    }
}

/// The text to end an I/O error with: the next step, in order.
fn next_step(what: &str) -> String {
    // [@ANCHOR: bench_mcp_sa_next_step]
    format!(
        "{what}. Next: check the LAN cable and that the analyzer is on and at the configured host and port (the raw SCPI socket is port 5025), then call bench_sa_status. If it stays silent, power-cycle the analyzer. Do not use the USB connection: its replies are cut short."
    )
}

/// A query repeated up to `tries` times in all, each with a `secs` reply timeout. Only for reads: repeating a read changes nothing.
pub(crate) fn ask(l: &mut Link, cmd: &str, tries: u32, secs: u64) -> Result<String, String> {
    // [@ANCHOR: bench_mcp_sa_ask]
    let mut last = String::new();
    for _ in 0..tries {
        l.set_timeout_secs(secs);
        match l.query(cmd) {
            Ok(r) if !r.is_empty() => return Ok(r),
            Ok(_) => last = "empty reply".into(),
            Err(e) => last = e,
        }
    }
    Err(next_step(&format!(
        "{cmd}: no usable reply after {tries} tries of {secs} s ({last})"
    )))
}

/// Query `cmd` (ordinary timeout and tries) and require a finite number in the reply.
pub(crate) fn ask_f64(l: &mut Link, cmd: &str) -> Result<f64, String> {
    // [@ANCHOR: bench_mcp_sa_ask_f64]
    let r = ask(l, cmd, READ_TRIES, REPLY_TIMEOUT_S)?;
    match r.trim().parse::<f64>() {
        Ok(v) if v.is_finite() => Ok(v),
        _ => Err(next_step(&format!(
            "{cmd}: the reply {r:?} is not a finite number"
        ))),
    }
}

/// `1`/`ON` or `0`/`OFF`, exactly. Any other word is a fault, not a guess.
pub(crate) fn on_off(cmd: &str, reply: &str) -> Result<bool, String> {
    // [@ANCHOR: bench_mcp_sa_on_off]
    match reply.trim().to_ascii_uppercase().as_str() {
        "ON" | "1" | "1.000000" => Ok(true),
        "OFF" | "0" | "0.000000" => Ok(false),
        _ => Err(next_step(&format!(
            "{cmd}: the reply {reply:?} is neither on nor off"
        ))),
    }
}

/// The highest frequency of an SSA3000X from its model name: `SSA3021X` is the 2.1 GHz model, `SSA3032X` 3.2 GHz,
/// `SSA3015X` 1.5 GHz, in Hz. `None` for a name that does not carry two digits after `SSA30`.
pub(crate) fn model_max_hz(model: &str) -> Option<f64> {
    // [@ANCHOR: bench_mcp_sa_model_max_hz]
    let d = model.strip_prefix("SSA30")?;
    let n: f64 = d.get(..2)?.parse().ok()?;
    (n > 0.0).then_some(n * 1e8)
}

// [@ANCHOR: bench_mcp_sa_ident]
/// What `open` learned from `*IDN?`.
pub struct Ident {
    pub make: String,
    pub model: String,
    pub serial: String,
    pub firmware: String,
    /// The model's top frequency, Hz.
    pub max_hz: f64,
}

/// Opens the link and refuses an address that does not answer as an SSA3000X, before any other command.
fn open(connect: &Connector, inst: &Instrument) -> Result<(Link, Ident), String> {
    // [@ANCHOR: bench_mcp_sa_open]
    let mut l = connect(inst)?;
    let idn = ask(&mut l, "*IDN?", READ_TRIES, REPLY_TIMEOUT_S)?;
    let p: Vec<String> = idn.split(',').map(|s| s.trim().to_string()).collect();
    let max_hz = p.get(1).and_then(|m| model_max_hz(m));
    if p.len() < 4 || !p[1].starts_with(MODEL_PREFIX) || max_hz.is_none() {
        return Err(format!(
            "{}:{} answered *IDN? with {idn:?}, not an SSA3000X spectrum analyzer. Nothing else was sent to it. Next: check the host and port in the bench config.",
            inst.host.as_deref().unwrap_or("?"),
            inst.port
        ));
    }
    Ok((
        l,
        Ident {
            make: p[0].clone(),
            model: p[1].clone(),
            serial: p[2].clone(),
            firmware: p[3].clone(),
            max_hz: max_hz.unwrap_or(0.0),
        },
    ))
}

/// The installed options as the unit lists them (`:SYSTem:OPTions?`, space separated, for example `Meas EMI TG CAT`).
pub(crate) fn options(l: &mut Link) -> Result<Vec<String>, String> {
    // [@ANCHOR: bench_mcp_sa_options]
    let r = ask(l, ":SYSTem:OPTions?", READ_TRIES, REPLY_TIMEOUT_S)?;
    Ok(r.split([' ', ','])
        .filter(|s| !s.is_empty())
        .map(String::from)
        .collect())
}

/// Identity, options, the tracking generator and the sweep settings, read back from the unit. Changes nothing.
pub fn status(connect: &Connector, inst: &Instrument) -> Result<Value, String> {
    // [@ANCHOR: bench_mcp_sa_status]
    let (mut l, id) = open(connect, inst)?;
    let opts = options(&mut l)?;
    let has_tg = opts.iter().any(|o| o.eq_ignore_ascii_case("TG"));
    let tg = if has_tg {
        let (on, level) = tg::tg_state(&mut l)?;
        json!({"state": if on { "on" } else { "off" }, "level_dbm": level, "max_tg_dbm": inst.sa.max_tg_dbm,
               "level_offset_db": ask_f64(&mut l, ":SOURce:CORRection:OFFSet?")?})
    } else {
        json!({"state": "not installed", "max_tg_dbm": inst.sa.max_tg_dbm})
    };
    let cont = on_off(
        ":INITiate:CONTinuous?",
        &ask(&mut l, ":INITiate:CONTinuous?", READ_TRIES, REPLY_TIMEOUT_S)?,
    )?;
    Ok(json!({
        "make": id.make, "model": id.model, "serial": id.serial, "firmware": id.firmware,
        "max_frequency_hz": id.max_hz, "options": opts, "tracking_generator": tg,
        "sweep": {
            "start_hz": ask_f64(&mut l, ":SENSe:FREQuency:STARt?")?,
            "stop_hz": ask_f64(&mut l, ":SENSe:FREQuency:STOP?")?,
            "rbw_hz": ask_f64(&mut l, ":SENSe:BWIDth?")?,
            "vbw_hz": ask_f64(&mut l, ":SENSe:BWIDth:VIDeo?")?,
            "ref_level_dbm": ask_f64(&mut l, ":DISPlay:WINDow:TRACe:Y:RLEVel?")?,
            "attenuation_db": ask_f64(&mut l, ":SENSe:POWer:RF:ATTenuation?")?,
            "detector": ask(&mut l, ":SENSe:DETector:TRACe1:FUNCtion?", READ_TRIES, REPLY_TIMEOUT_S)?,
            "sweep_time_s": ask_f64(&mut l, ":SENSe:SWEep:TIME?")?,
            "continuous": cont,
            "points": sweep::POINTS,
        },
    }))
}

// [@ANCHOR: bench_mcp_sa_connector]
/// How a call reaches the instrument. Production opens the TCP socket; tests pass a connector to a fake instrument.
pub type Connector = dyn Fn(&Instrument) -> Result<Link, String>;

pub fn tcp(inst: &Instrument) -> Result<Link, String> {
    // [@ANCHOR: bench_mcp_sa_tcp]
    Link::tcp(inst.host.as_deref().unwrap_or_default(), inst.port)
}

pub fn call(insts: &[Instrument], name: &str, args: &Value) -> Result<Value, String> {
    // [@ANCHOR: bench_mcp_sa_call]
    call_with(&tcp, insts, name, args)
}

/// `call` with the way to reach the instrument given, so the tests run the whole path (tier check, argument checks, the
/// dialect) against a fake instrument.
pub fn call_with(
    connect: &Connector,
    insts: &[Instrument],
    name: &str,
    args: &Value,
) -> Result<Value, String> {
    // [@ANCHOR: bench_mcp_sa_call_with]
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
    match name {
        "bench_sa_status" => status(connect, inst),
        "bench_sa_sweep" => sweep::sweep(connect, inst, args),
        "bench_sa_marker" => sweep::marker(connect, inst, args),
        "bench_sa_screenshot" => sweep::screenshot(connect, inst),
        "bench_sa_tg_on" => tg::tg_on(connect, inst, args),
        "bench_sa_tg_off" => tg::tg_off(connect, inst),
        "bench_sa_tg_level" => tg::tg_level(connect, inst, args),
        "bench_sa_tg_normalize" => tg::normalize(connect, inst, args),
        "bench_sa_tg_s21" => tg::s21(connect, inst, args),
        _ => Err(format!("{name} does not apply to {:?}", inst.name)),
    }
}

#[cfg(test)]
mod tests {
    use super::testkit::*;
    use super::*;

    pub(crate) fn bench(extra: &str) -> Vec<Instrument> {
        crate::config::parse(&format!(
            r#"{{"instruments":{{"sa":{{"driver":"siglent_ssa","tier":"read_config","host":"127.0.0.1"{extra}}}}}}}"#
        ))
        .unwrap()
    }

    // Tests [@ANCHOR: bench_mcp_sa_parse_config]
    // Tests [@ANCHOR: bench_mcp_sa_config]
    // Tests [@ANCHOR: bench_mcp_sa_limits]
    #[test]
    fn config_takes_the_tg_cap_with_a_default_and_refuses_bad_ones() {
        assert_eq!(bench("")[0].sa, Config::default());
        assert_eq!(bench(r#","max_tg_dbm":-10"#)[0].sa.max_tg_dbm, -10.0);
        for bad in [
            r#","max_tg_dbm":3"#,
            r#","max_tg_dbm":-30"#,
            r#","max_tg_dbm":"low""#,
        ] {
            let text = format!(
                r#"{{"instruments":{{"sa":{{"driver":"siglent_ssa","host":"127.0.0.1"{bad}}}}}}}"#
            );
            assert!(crate::config::parse(&text).is_err(), "{bad}");
        }
        let no_host = r#"{"instruments":{"sa":{"driver":"siglent_ssa"}}}"#;
        assert!(crate::config::parse(no_host).unwrap_err().contains("host"));
    }

    // Tests [@ANCHOR: bench_mcp_sa_flag]
    // Tests [@ANCHOR: bench_mcp_sa_number]
    // Tests [@ANCHOR: bench_mcp_sa_on_off]
    // Tests [@ANCHOR: bench_mcp_sa_model_max_hz]
    #[test]
    fn argument_helpers_refuse_wrong_types_and_ranges_and_models_give_their_top_frequency() {
        let a = json!({"x": 1.5, "n": null, "s": "1", "b": true});
        assert_eq!(number(&a, "x", 0.0, 2.0, "w"), Ok(Some(1.5)));
        assert_eq!(number(&a, "n", 0.0, 2.0, "w"), Ok(None));
        assert_eq!(number(&a, "absent", 0.0, 2.0, "w"), Ok(None));
        assert!(number(&a, "x", 0.0, 1.0, "the cap")
            .unwrap_err()
            .contains("the cap"));
        assert!(number(&a, "s", 0.0, 1.0, "w").is_err());
        assert_eq!(flag(&a, "b", false), Ok(true));
        assert_eq!(flag(&a, "absent", true), Ok(true));
        assert!(flag(&a, "s", false).is_err());
        assert_eq!(on_off("c", "1"), Ok(true));
        assert_eq!(on_off("c", "0.000000"), Ok(false));
        assert!(on_off("c", "MAYBE").unwrap_err().contains("Next:"));
        assert_eq!(model_max_hz("SSA3021X"), Some(2.1e9));
        assert_eq!(model_max_hz("SSA3032X-R"), Some(3.2e9));
        assert_eq!(model_max_hz("SDS1202X-E"), None);
        assert_eq!(model_max_hz("SSA30"), None);
    }

    // Tests [@ANCHOR: bench_mcp_sa_instrument]
    // Tests [@ANCHOR: bench_mcp_sa_tool_defs]
    // Tests [@ANCHOR: bench_mcp_sa_spec]
    // Tests [@ANCHOR: bench_mcp_sa_specs]
    // Tests [@ANCHOR: bench_mcp_sa_connector]
    // Tests [@ANCHOR: bench_mcp_sa_call]
    // Tests [@ANCHOR: bench_mcp_sa_call_with]
    #[test]
    fn tools_are_listed_by_tier_and_refused_below_it_before_any_connection() {
        let ro = crate::config::parse(
            r#"{"instruments":{"sa":{"driver":"siglent_ssa","tier":"read_only","host":"127.0.0.1"}}}"#,
        )
        .unwrap();
        let names = |v: &[Instrument]| -> Vec<String> {
            tool_defs(v).iter().map(|t| t.name.to_string()).collect()
        };
        assert_eq!(names(&ro), ["bench_sa_status", "bench_sa_screenshot"]);
        assert_eq!(names(&bench("")).len(), SPECS.len());
        let dev = FakeDevice::sim(Sim::default());
        let e = call_with(&dev.connector(), &ro, "bench_sa_tg_on", &json!({})).unwrap_err();
        assert!(e.contains("needs tier"), "{e}");
        assert!(dev.received().is_empty());
        let e = call_with(&dev.connector(), &bench(""), "bench_sa_nope", &json!({})).unwrap_err();
        assert!(e.contains("unknown tool"));
        let e = call_with(&dev.connector(), &[], "bench_sa_status", &json!({})).unwrap_err();
        assert!(e.contains("no spectrum analyzer"), "{e}");
        let e = call_with(
            &dev.connector(),
            &bench(""),
            "bench_sa_status",
            &json!({"instrument": "other"}),
        )
        .unwrap_err();
        assert!(e.contains("no spectrum analyzer named"), "{e}");
        let e = call(&[], "bench_sa_status", &json!({})).unwrap_err();
        assert!(e.contains("no spectrum analyzer"), "{e}");
    }

    // Tests [@ANCHOR: bench_mcp_sa_status]
    // Tests [@ANCHOR: bench_mcp_sa_open]
    // Tests [@ANCHOR: bench_mcp_sa_options]
    // Tests [@ANCHOR: bench_mcp_sa_ident]
    #[test]
    fn status_reads_the_recorded_session_and_a_stranger_is_sent_nothing() {
        let dev = FakeDevice::from_transcript("tests/sa_fixtures/status.txt");
        let v = call_with(&dev.connector(), &bench(""), "bench_sa_status", &json!({})).unwrap();
        assert_eq!(v["model"], "SSA3021X");
        assert_eq!(v["firmware"], "1.3.9.10");
        assert_eq!(v["max_frequency_hz"].as_f64(), Some(2.1e9));
        assert_eq!(v["options"], json!(["Meas", "EMI", "TG", "CAT"]));
        assert_eq!(v["tracking_generator"]["state"], "off");
        assert_eq!(v["tracking_generator"]["level_dbm"].as_f64(), Some(-20.0));
        assert_eq!(v["sweep"]["stop_hz"].as_f64(), Some(2.1e9));
        assert_eq!(v["sweep"]["continuous"], true);
        assert_eq!(v["sweep"]["points"], 751);
        let sent = dev.received();
        assert_eq!(sent[0], "*IDN?");
        assert!(
            sent.iter().all(|c| c.ends_with('?')),
            "status only queries: {sent:?}"
        );
        let other = FakeDevice::start(|c| (c == "*IDN?").then(|| "OTHER,THING-1,0,1".to_string()));
        let e = call_with(
            &other.connector(),
            &bench(""),
            "bench_sa_status",
            &json!({}),
        )
        .unwrap_err();
        assert!(e.contains("not an SSA3000X"), "{e}");
        assert_eq!(other.received(), ["*IDN?"]);
    }

    // Tests [@ANCHOR: bench_mcp_sa_ask]
    // Tests [@ANCHOR: bench_mcp_sa_ask_f64]
    // Tests [@ANCHOR: bench_mcp_sa_tcp]
    // Tests [@ANCHOR: bench_mcp_sa_next_step]
    #[test]
    fn a_silent_analyzer_is_retried_a_bounded_number_of_times_and_a_garbage_number_is_an_error() {
        let quiet = FakeDevice::start(|_| None);
        let t = std::time::Instant::now();
        let e = call_with(
            &quiet.connector(),
            &bench(""),
            "bench_sa_status",
            &json!({}),
        )
        .unwrap_err();
        assert!(e.contains("after 3 tries") && e.contains("Next:"), "{e}");
        assert_eq!(quiet.received().len(), READ_TRIES as usize);
        assert!(
            t.elapsed().as_secs() <= READ_TRIES as u64 * REPLY_TIMEOUT_S + 3,
            "bounded wait"
        );
        let flaky = FakeDevice::sim(Sim {
            silent_queries: 1,
            ..Sim::default()
        });
        let v = call_with(
            &flaky.connector(),
            &bench(""),
            "bench_sa_status",
            &json!({}),
        )
        .unwrap();
        assert_eq!(v["model"], "SSA3021X");
        let bad = FakeDevice::sim(Sim {
            garbage: Some((":SENSe:BWIDth?".into(), "----".into())),
            ..Sim::default()
        });
        let e = call_with(&bad.connector(), &bench(""), "bench_sa_status", &json!({})).unwrap_err();
        assert!(e.contains("not a finite number"), "{e}");
        // The production connector cannot reach a port nothing listens on.
        let port = {
            let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            l.local_addr().unwrap().port()
        };
        let dead = bench(&format!(r#","port":{port}"#));
        assert!(tcp(&dead[0]).err().unwrap().contains("connect"));
        let e = call(&dead, "bench_sa_status", &json!({})).unwrap_err();
        assert!(e.contains("connect"), "{e}");
    }
}
