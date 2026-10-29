// SPDX-License-Identifier: AGPL-3.0-or-later
//! THE SKELETON FOR A NEW INSTRUMENT. Copy this directory (`tools/new_instrument.py` does it), then replace the
//! example dialect with the instrument's. The guide is the manual page `concepts/extending` (also
//! `docs/EXTENDING.md`); this file is the code it points at.
//!
//! The example instrument is an imaginary SCPI-over-TCP signal source, "ACME EXAMPLE-1000": it answers `*IDN?`, has an
//! output that is on or off, a level in volts, and a voltage meter. It is driven through `crate::link::Link`, the same
//! line-based link the scope uses, and its tests run against a fake device on a local socket (`testkit.rs`), so
//! nothing here needs hardware. Only a bench whose config has `"driver": "example"` ever lists these tools.
//!
//! What the four tools show, one rule each:
//!
//! * `bench_example_status`, `bench_example_measure` (read_only): read, check every reply, never return a guess.
//! * `bench_example_pulse` (full): an actuating tool: every limit checked before the first byte is sent, `dry_run`,
//!   an explicit `confirm`, a hard time limit held by the instrument itself, and the output switched off again on every path.
//! * `bench_example_safe_state` (read_config): puts the instrument in its safe state and says whether it verified that.
//!
//! The entry in the bench config (`BENCH_MCP_CONFIG`):
//! `{"driver": "example", "tier": "read_config", "host": "192.0.2.10", "port": 5025, "max_level_v": 1.0, "max_pulse_s": 2.0}`.

#[cfg(test)]
pub mod testkit;

use crate::config::{Instrument, Kind, Tier};
use crate::link::Link;
use rmcp::model::Tool;
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::Duration;

/// The model string the instrument must report. A different reply means the address is something else on the network:
/// stop before sending it any command.
const MODEL: &str = "EXAMPLE-1000";
/// Read timeout of every exchange, in seconds. No call waits longer than this per reply (`Link` is never left at its default).
const REPLY_TIMEOUT_S: u64 = 2;
/// Reads (queries) are repeated this many times in all; commands that change the instrument are never repeated blind.
const READ_TRIES: u32 = 3;
/// Attempts to switch the output off and see it off.
const OFF_TRIES: u32 = 3;
/// The lowest and highest settable output level, the instrument's own rating.
const RATED_LEVEL_V: f64 = 5.0;
const MIN_PULSE_S: f64 = 0.1;

/// The `example` settings of an instrument entry. The limits are the owner's; the tools can only lower them per call.
#[derive(Clone, Debug, PartialEq)]
pub struct Config {
    /// The highest output level any tool may set, volts (0 to the rating).
    pub max_level_v: f64,
    /// The longest time the output may be on in one call, seconds.
    pub max_pulse_s: f64,
}

impl Default for Config {
    fn default() -> Config {
        Config {
            max_level_v: 1.0,
            max_pulse_s: 2.0,
        }
    }
}

/// Reads `max_level_v` and `max_pulse_s`. A bad value is refused when the config file is read, so the server never starts with
/// a limit it cannot honour.
pub fn parse_config(c: &Value) -> Result<Config, String> {
    // [@ANCHOR: example_config]
    let mut cfg = Config::default();
    if let Some(v) = c.get("max_level_v") {
        let x = v.as_f64().ok_or("max_level_v must be a number")?;
        if !(x.is_finite() && x > 0.0 && x <= RATED_LEVEL_V) {
            return Err(format!(
                "max_level_v {x} must be above 0 and at most the rating {RATED_LEVEL_V} V"
            ));
        }
        cfg.max_level_v = x;
    }
    if let Some(v) = c.get("max_pulse_s") {
        let x = v.as_f64().ok_or("max_pulse_s must be a number")?;
        if !(x.is_finite() && (MIN_PULSE_S..=60.0).contains(&x)) {
            return Err(format!(
                "max_pulse_s {x} must be {MIN_PULSE_S} to 60 seconds"
            ));
        }
        cfg.max_pulse_s = x;
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

/// Every tool of this instrument. A new tool is a new entry here, a match arm in `call_with`, a page under
/// `manual/tools/<name>.md` and a test; the tests of `src/manual/tests.rs` fail until the page exists.
pub const SPECS: &[Spec] = &[
    Spec {
        name: "bench_example_status",
        tier: Tier::ReadOnly,
        desc: "EXAMPLE instrument: identity (model, serial, firmware), whether the output is on or off and the level it is set to. Changes nothing. Refuses an address that does not answer as an EXAMPLE-1000.",
        props: props!(r#""instrument":{"type":"string"}"#),
        required: &[],
    },
    Spec {
        name: "bench_example_measure",
        tier: Tier::ReadOnly,
        desc: "EXAMPLE instrument: the voltage at the meter input in volts, the mean of `count` readings (1 to 10, default 1) with their spread. A reading the instrument does not give as a finite number is an error, never a zero. Changes nothing.",
        props: props!(r#""instrument":{"type":"string"},"count":{"type":"integer","description":"readings to average, 1 to 10, default 1"}"#),
        required: &[],
    },
    Spec {
        name: "bench_example_pulse",
        tier: Tier::Full,
        desc: "EXAMPLE instrument, DRIVES ITS OUTPUT: sets `level_v` (volts, at most the config's max_level_v and the rating), switches the output on for `pulse_s` seconds (at most the config's max_pulse_s), then switches it off and checks that it is off. The instrument also holds its own time limit, so it switches off even if the server dies. `confirm` must be true, and must come from the person at the bench. `dry_run` returns the plan and sends nothing. If it cannot confirm the output is off it says so loudly and names what to do by hand.",
        props: props!(r#""instrument":{"type":"string"},"level_v":{"type":"number","description":"output level in volts"},"pulse_s":{"type":"number","description":"seconds the output stays on"},"confirm":{"type":"boolean","description":"true: the person at the bench agrees this output may be driven"},"dry_run":{"type":"boolean","description":"return the plan and send nothing"}"#),
        required: &["level_v", "pulse_s"],
    },
    Spec {
        name: "bench_example_safe_state",
        tier: Tier::ReadConfig,
        desc: "EXAMPLE instrument: switches the output off and reads it back to prove it. Safe at any time; use it first when anything went wrong. If the instrument will not confirm off, the answer says what to do by hand.",
        props: props!(r#""instrument":{"type":"string"}"#),
        required: &[],
    },
];

fn instrument<'a>(insts: &'a [Instrument], args: &Value) -> Result<&'a Instrument, String> {
    // [@ANCHOR: bench_mcp_example_instrument]
    let ours: Vec<&Instrument> = insts.iter().filter(|i| i.kind == Kind::Example).collect();
    if let Some(n) = args.get("instrument").and_then(|n| n.as_str()) {
        return ours
            .iter()
            .copied()
            .find(|i| i.name == n)
            .ok_or_else(|| format!("no example instrument named {n:?}"));
    }
    match ours.as_slice() {
        [one] => Ok(one),
        [] => Err("no example instrument is configured".into()),
        many => Err(format!(
            "name the instrument: {}",
            many.iter()
                .map(|i| i.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )),
    }
}

/// The tools to list: an entry of `SPECS` appears only when an example instrument is configured at or above its tier.
pub fn tool_defs(insts: &[Instrument]) -> Vec<Tool> {
    SPECS
        .iter()
        .filter(|s| insts.iter().any(|i| i.kind == Kind::Example && i.tier >= s.tier))
        .map(|s| {
            let schema = json!({"type": "object", "properties": serde_json::from_str::<Value>(s.props).unwrap(), "required": s.required});
            let Value::Object(schema) = schema else { unreachable!() };
            Tool::new(s.name, format!("[{:?}] {}", s.tier, s.desc), Arc::new(schema))
        })
        .collect()
}

/// A boolean argument that may be absent (false) but must not be anything else.
fn flag(args: &Value, key: &str) -> Result<bool, String> {
    // [@ANCHOR: bench_mcp_example_flag]
    match args.get(key) {
        None | Some(Value::Null) => Ok(false),
        Some(Value::Bool(b)) => Ok(*b),
        Some(_) => Err(format!("{key} must be true or false")),
    }
}

/// A number argument that must be present, finite and in `lo..=hi`; the error names the range and the cause of the limit.
fn number(args: &Value, key: &str, lo: f64, hi: f64, why: &str) -> Result<f64, String> {
    // [@ANCHOR: example_args]
    let v = args
        .get(key)
        .and_then(|v| v.as_f64())
        .ok_or_else(|| format!("{key} is required and must be a number"))?;
    if v.is_finite() && (lo..=hi).contains(&v) {
        Ok(v)
    } else {
        Err(format!("{key} {v} is outside {lo}..={hi} ({why})"))
    }
}

/// The text to end an I/O error with: the next step, in order. Every failure of a link ends with one of these.
fn next_step(what: &str) -> String {
    format!(
        "{what}. Next: check the cable and that the instrument is on, then call bench_example_status. If it stays silent, power-cycle it (bench_usb_port_power when it hangs off a switchable hub port, else by hand). No value from before this failure has been used."
    )
}

/// A query repeated up to `tries` times in all, each with the reply timeout. Only for reads: repeating a read changes nothing.
/// The error names the command and the number of tries.
fn ask(l: &mut Link, cmd: &str, tries: u32) -> Result<String, String> {
    // [@ANCHOR: example_retry]
    let mut last = String::new();
    for _ in 0..tries {
        l.set_timeout_secs(REPLY_TIMEOUT_S);
        match l.query(cmd) {
            Ok(r) if !r.is_empty() => return Ok(r),
            Ok(_) => last = "empty reply".into(),
            Err(e) => last = e,
        }
    }
    Err(next_step(&format!(
        "{cmd}: no usable reply after {tries} tries of {REPLY_TIMEOUT_S} s ({last})"
    )))
}

/// A reply that must be a finite number (a unit suffix is allowed). Anything else is an error: a dash, `NaN` or a half
/// line is an instrument or link fault, and a measurement tool must not turn it into a number.
fn finite(cmd: &str, reply: &str) -> Result<f64, String> {
    // [@ANCHOR: example_readings]
    let text = reply
        .trim()
        .trim_end_matches(|c: char| c.is_ascii_alphabetic());
    match text.parse::<f64>() {
        Ok(v) if v.is_finite() => Ok(v),
        _ => Err(next_step(&format!(
            "{cmd}: the reply {reply:?} is not a finite number"
        ))),
    }
}

/// `ON` or `OFF`, exactly. Any other word is a fault, not a guess.
fn on_off(reply: &str) -> Result<bool, String> {
    // [@ANCHOR: bench_mcp_example_on_off]
    match reply.trim().to_ascii_uppercase().as_str() {
        "ON" | "1" => Ok(true),
        "OFF" | "0" => Ok(false),
        _ => Err(next_step(&format!(
            "OUTP?: the reply {reply:?} is neither ON nor OFF"
        ))),
    }
}

/// Opens the link and refuses an address that does not answer as the expected model, before any other command.
fn open(connect: &Connector, inst: &Instrument) -> Result<(Link, Vec<String>), String> {
    // [@ANCHOR: example_identity]
    let mut l = connect(inst)?;
    let idn = ask(&mut l, "*IDN?", READ_TRIES)?;
    let parts: Vec<String> = idn.split(',').map(|s| s.trim().to_string()).collect();
    if parts.len() < 4 || parts[1] != MODEL {
        return Err(format!(
            "{}:{} answered *IDN? with {idn:?}, not an {MODEL}. Nothing was sent to it. Next: check the host and port in the bench config.",
            inst.host.as_deref().unwrap_or("?"),
            inst.port
        ));
    }
    Ok((l, parts))
}

pub fn status(connect: &Connector, inst: &Instrument) -> Result<Value, String> {
    // [@ANCHOR: bench_mcp_example_status]
    let (mut l, idn) = open(connect, inst)?;
    let out = on_off(&ask(&mut l, "OUTP?", READ_TRIES)?)?;
    let level = finite("LEV?", &ask(&mut l, "LEV?", READ_TRIES)?)?;
    Ok(json!({
        "make": idn[0], "model": idn[1], "serial": idn[2], "firmware": idn[3],
        "output": if out { "on" } else { "off" },
        "level_v": level,
        "limits": {"max_level_v": inst.example.max_level_v, "max_pulse_s": inst.example.max_pulse_s},
    }))
}

pub fn measure(connect: &Connector, inst: &Instrument, args: &Value) -> Result<Value, String> {
    // [@ANCHOR: bench_mcp_example_measure]
    let count = match args.get("count") {
        None | Some(Value::Null) => 1,
        Some(_) => number(args, "count", 1.0, 10.0, "readings per call")? as usize,
    };
    let (mut l, _) = open(connect, inst)?;
    let mut v = Vec::new();
    for _ in 0..count {
        v.push(finite(
            "MEAS:VOLT?",
            &ask(&mut l, "MEAS:VOLT?", READ_TRIES)?,
        )?);
    }
    let mean = v.iter().sum::<f64>() / v.len() as f64;
    let lo = v.iter().cloned().fold(f64::INFINITY, f64::min);
    let hi = v.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    Ok(json!({"voltage_v": mean, "readings": v.len(), "spread_v": hi - lo}))
}

/// Switches the output off and reads it back, up to `OFF_TRIES` times. Ok only when the instrument itself says OFF.
fn ensure_off(l: &mut Link) -> Result<(), String> {
    // [@ANCHOR: example_safe_state]
    let mut last = String::new();
    for _ in 0..OFF_TRIES {
        l.set_timeout_secs(REPLY_TIMEOUT_S);
        if let Err(e) = l.send("OUTP OFF") {
            last = e;
            continue;
        }
        match l.query("OUTP?").and_then(|r| on_off(&r)) {
            Ok(false) => return Ok(()),
            Ok(true) => last = "it still reports ON".into(),
            Err(e) => last = e,
        }
    }
    Err(format!(
        "SAFE STATE NOT CONFIRMED: the output may still be ON ({last}). Do this now: switch the instrument's output off at its front panel or pull its power. Do not use the instrument again until the owner has looked at it."
    ))
}

pub fn safe_state(connect: &Connector, inst: &Instrument) -> Result<Value, String> {
    // [@ANCHOR: bench_mcp_example_safe_state]
    let (mut l, _) = open(connect, inst)?;
    ensure_off(&mut l)?;
    Ok(json!({"output": "off", "verified": true}))
}

/// The arguments of a pulse, checked against the rating and the owner's limits before anything is sent.
pub struct Pulse {
    pub level_v: f64,
    pub pulse_s: f64,
}

pub fn pulse_args(cfg: &Config, args: &Value) -> Result<Pulse, String> {
    // [@ANCHOR: example_pulse_args]
    let level_v = number(
        args,
        "level_v",
        0.0,
        cfg.max_level_v,
        "the cap in the server's config",
    )?;
    let pulse_s = number(
        args,
        "pulse_s",
        MIN_PULSE_S,
        cfg.max_pulse_s,
        "the longest pulse in the server's config",
    )?;
    Ok(Pulse { level_v, pulse_s })
}

pub fn pulse(connect: &Connector, inst: &Instrument, args: &Value) -> Result<Value, String> {
    // [@ANCHOR: example_pulse]
    let p = pulse_args(&inst.example, args)?;
    let dry = flag(args, "dry_run")?;
    let confirm = flag(args, "confirm")?;
    // The device holds the limit as well as this process: if the server dies mid-pulse the output still goes off.
    let plan = [
        format!("OUTP:TMO {:.1}", p.pulse_s + 1.0),
        format!("LEV {:.3}", p.level_v),
        "OUTP ON".to_string(),
        format!("wait {:.1} s", p.pulse_s),
        "OUTP OFF".to_string(),
        "OUTP? (must read OFF)".to_string(),
    ];
    if dry {
        return Ok(json!({"dry_run": true, "sent": [], "would": plan,
            "note": "nothing was sent; the output stays as it is"}));
    }
    if !confirm {
        return Err(format!(
            "bench_example_pulse drives the output at {:.3} V for {:.1} s; call it with confirm: true only when the person at the bench has said this output may be driven (dry_run: true shows the plan)",
            p.level_v, p.pulse_s
        ));
    }
    let (mut l, _) = open(connect, inst)?;
    if on_off(&ask(&mut l, "OUTP?", READ_TRIES)?)? {
        return Err("the output is already ON; refusing to start a pulse. Call bench_example_safe_state first.".into());
    }
    let ran = run_pulse(&mut l, &p);
    // Every path after the first command ends here, success or not.
    let off = ensure_off(&mut l);
    match (ran, off) {
        (Ok(()), Ok(())) => Ok(
            json!({"level_v": p.level_v, "pulse_s": p.pulse_s, "output": "off", "verified_off": true}),
        ),
        (Err(e), Ok(())) => Err(format!(
            "the pulse failed ({e}); the output was switched off and verified off"
        )),
        (_, Err(e)) => Err(e),
    }
}

fn run_pulse(l: &mut Link, p: &Pulse) -> Result<(), String> {
    // [@ANCHOR: bench_mcp_example_run_pulse]
    l.send(&format!("OUTP:TMO {:.1}", p.pulse_s + 1.0))?;
    l.send(&format!("LEV {:.3}", p.level_v))?;
    l.send("OUTP ON")?;
    std::thread::sleep(Duration::from_secs_f64(p.pulse_s));
    Ok(())
}

/// How a call reaches the instrument. Production opens the TCP socket; tests pass a connector to a fake device.
pub type Connector = dyn Fn(&Instrument) -> Result<Link, String>;

pub fn tcp(inst: &Instrument) -> Result<Link, String> {
    Link::tcp(inst.host.as_deref().unwrap_or_default(), inst.port)
}

pub fn call(insts: &[Instrument], name: &str, args: &Value) -> Result<Value, String> {
    call_with(&tcp, insts, name, args)
}

/// `call` with the way to reach the instrument given, so the tests run the whole path (tier check, argument checks, the
/// dialect) against a fake device.
pub fn call_with(
    connect: &Connector,
    insts: &[Instrument],
    name: &str,
    args: &Value,
) -> Result<Value, String> {
    // [@ANCHOR: bench_mcp_example_call_with]
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
        "bench_example_status" => status(connect, inst),
        "bench_example_measure" => measure(connect, inst, args),
        "bench_example_pulse" => pulse(connect, inst, args),
        "bench_example_safe_state" => safe_state(connect, inst),
        _ => Err(format!("{name} does not apply to {:?}", inst.name)),
    }
}

#[cfg(test)]
mod tests {
    use super::testkit::*;
    use super::*;

    fn bench(extra: &str) -> Vec<Instrument> {
        crate::config::parse(&format!(
            r#"{{"instruments":{{"ex":{{"driver":"example","tier":"full","host":"127.0.0.1"{extra}}}}}}}"#
        ))
        .unwrap()
    }

    // Tests [@ANCHOR: example_config]
    #[test]
    fn config_takes_limits_with_defaults_and_refuses_bad_ones() {
        let v = bench("");
        assert_eq!(v[0].example, Config::default());
        let v = bench(r#","max_level_v":2.5,"max_pulse_s":5"#);
        assert_eq!(
            (v[0].example.max_level_v, v[0].example.max_pulse_s),
            (2.5, 5.0)
        );
        for bad in [
            r#","max_level_v":0"#,
            r#","max_level_v":9"#,
            r#","max_level_v":"high""#,
            r#","max_pulse_s":0.01"#,
            r#","max_pulse_s":600"#,
        ] {
            let text = format!(
                r#"{{"instruments":{{"ex":{{"driver":"example","host":"127.0.0.1"{bad}}}}}}}"#
            );
            assert!(crate::config::parse(&text).is_err(), "{bad}");
        }
        let no_host = r#"{"instruments":{"ex":{"driver":"example"}}}"#;
        assert!(crate::config::parse(no_host).unwrap_err().contains("host"));
    }

    // Tests [@ANCHOR: example_args]
    #[test]
    fn numbers_must_be_present_finite_and_in_range() {
        let a = json!({"x": 1.5, "nan": null, "s": "1"});
        assert_eq!(number(&a, "x", 0.0, 2.0, "why"), Ok(1.5));
        assert!(number(&a, "x", 0.0, 1.0, "the cap")
            .unwrap_err()
            .contains("the cap"));
        assert!(number(&a, "nan", 0.0, 1.0, "w").is_err());
        assert!(number(&a, "s", 0.0, 1.0, "w").is_err());
        assert!(number(&a, "absent", 0.0, 1.0, "w").is_err());
        assert!(number(&json!({"x": f64::INFINITY}), "x", 0.0, 1.0, "w").is_err());
    }

    // Tests [@ANCHOR: example_readings]
    // Tests [@ANCHOR: bench_mcp_example_on_off]
    #[test]
    fn a_reply_that_is_not_a_finite_number_is_an_error_not_a_zero() {
        assert_eq!(finite("c", "1.250V"), Ok(1.25));
        assert_eq!(finite("c", " 0.5 "), Ok(0.5));
        for bad in ["", "---", "NaN", "inf", "1.2.3", "ERR"] {
            let e = finite("MEAS:VOLT?", bad).unwrap_err();
            assert!(e.contains("Next:"), "{bad}: {e}");
        }
        assert_eq!(on_off("on"), Ok(true));
        assert_eq!(on_off("0"), Ok(false));
        assert!(on_off("MAYBE").is_err());
    }

    // Tests [@ANCHOR: example_identity]
    // Tests [@ANCHOR: bench_mcp_example_call_with]
    // Tests [@ANCHOR: bench_mcp_example_flag]
    // Tests [@ANCHOR: bench_mcp_example_instrument]
    // Tests [@ANCHOR: bench_mcp_example_measure]
    // Tests [@ANCHOR: bench_mcp_example_run_pulse]
    // Tests [@ANCHOR: bench_mcp_example_safe_state]
    // Tests [@ANCHOR: bench_mcp_example_status]
    #[test]
    fn status_reads_the_recorded_session_and_a_stranger_is_sent_nothing() {
        let dev = FakeDevice::from_transcript("tests/example_fixtures/status.txt");
        let v = call_with(
            &dev.connector(),
            &bench(""),
            "bench_example_status",
            &json!({}),
        )
        .unwrap();
        assert_eq!(v["model"], "EXAMPLE-1000");
        assert_eq!(
            (v["output"].as_str(), v["level_v"].as_f64()),
            (Some("off"), Some(0.25))
        );
        assert_eq!(dev.received(), ["*IDN?", "OUTP?", "LEV?"]);
        let other = FakeDevice::start(|c| (c == "*IDN?").then(|| "OTHER,THING-1,0,1".to_string()));
        let e = call_with(
            &other.connector(),
            &bench(""),
            "bench_example_status",
            &json!({}),
        )
        .unwrap_err();
        assert!(e.contains("not an EXAMPLE-1000"), "{e}");
        assert_eq!(
            other.received(),
            ["*IDN?"],
            "nothing else is sent to a stranger"
        );
    }

    // Tests [@ANCHOR: example_retry]
    #[test]
    fn a_silent_instrument_is_retried_a_bounded_number_of_times_then_reported_with_the_next_step() {
        let quiet = FakeDevice::start(|_| None);
        let t = std::time::Instant::now();
        let e = call_with(
            &quiet.connector(),
            &bench(""),
            "bench_example_status",
            &json!({}),
        )
        .unwrap_err();
        assert!(e.contains("after 3 tries") && e.contains("Next:"), "{e}");
        assert_eq!(quiet.received().len(), READ_TRIES as usize);
        assert!(
            t.elapsed().as_secs() <= READ_TRIES as u64 * REPLY_TIMEOUT_S + 2,
            "bounded wait"
        );
        // One lost reply is retried and the answer is still right.
        let flaky = FakeDevice::sim(Sim::dropping_first_replies(1));
        let v = call_with(
            &flaky.connector(),
            &bench(""),
            "bench_example_measure",
            &json!({"count": 2}),
        )
        .unwrap();
        assert_eq!(v["readings"], 2);
    }

    #[test]
    fn measure_averages_and_refuses_a_garbage_reading() {
        let dev = FakeDevice::sim(Sim::default());
        let v = call_with(
            &dev.connector(),
            &bench(""),
            "bench_example_measure",
            &json!({"count": 3}),
        )
        .unwrap();
        assert_eq!(v["voltage_v"].as_f64(), Some(0.0));
        let bad = FakeDevice::sim(Sim {
            meter_reply: Some("---".into()),
            ..Sim::default()
        });
        let e = call_with(
            &bad.connector(),
            &bench(""),
            "bench_example_measure",
            &json!({}),
        )
        .unwrap_err();
        assert!(e.contains("not a finite number"), "{e}");
        assert!(call_with(
            &dev.connector(),
            &bench(""),
            "bench_example_measure",
            &json!({"count": 11})
        )
        .is_err());
    }

    // Tests [@ANCHOR: example_pulse_args]
    #[test]
    fn pulse_arguments_are_checked_against_the_rating_and_the_owners_limits() {
        let cfg = Config {
            max_level_v: 1.0,
            max_pulse_s: 2.0,
        };
        assert!(pulse_args(&cfg, &json!({"level_v": 0.5, "pulse_s": 1.0})).is_ok());
        for a in [
            json!({"level_v": 1.5, "pulse_s": 1.0}),
            json!({"level_v": -0.1, "pulse_s": 1.0}),
            json!({"level_v": 0.5, "pulse_s": 9.0}),
            json!({"level_v": 0.5, "pulse_s": 0.0}),
            json!({"level_v": 0.5}),
            json!({"level_v": f64::INFINITY, "pulse_s": 1.0}),
        ] {
            assert!(pulse_args(&cfg, &a).is_err(), "{a}");
        }
    }

    // Tests [@ANCHOR: example_pulse]
    #[test]
    fn dry_run_and_a_missing_confirm_send_nothing_and_a_confirmed_pulse_ends_off() {
        let dev = FakeDevice::sim(Sim::default());
        let args = json!({"level_v": 0.5, "pulse_s": 0.1});
        let mut dry = args.clone();
        dry["dry_run"] = json!(true);
        let v = call_with(&dev.connector(), &bench(""), "bench_example_pulse", &dry).unwrap();
        assert_eq!(v["dry_run"], true);
        assert!(v["would"].as_array().unwrap().len() >= 5);
        let e = call_with(&dev.connector(), &bench(""), "bench_example_pulse", &args).unwrap_err();
        assert!(e.contains("confirm: true"), "{e}");
        assert!(
            dev.received().is_empty(),
            "no connection was even needed: {:?}",
            dev.received()
        );
        let mut ok = args.clone();
        ok["confirm"] = json!(true);
        let v = call_with(&dev.connector(), &bench(""), "bench_example_pulse", &ok).unwrap();
        assert_eq!(v["verified_off"], true);
        let sent = dev.received();
        let on = sent.iter().position(|c| c == "OUTP ON").unwrap();
        assert!(
            sent[..on].contains(&"OUTP:TMO 1.1".to_string()),
            "the device's own limit is set before ON: {sent:?}"
        );
        assert_eq!(sent.last().map(String::as_str), Some("OUTP?"));
        assert!(sent.contains(&"OUTP OFF".to_string()));
        // Below the tier the tool is refused before any connection.
        let ro = crate::config::parse(
            r#"{"instruments":{"ex":{"driver":"example","tier":"read_config","host":"127.0.0.1"}}}"#,
        )
        .unwrap();
        let e = call_with(&dev.connector(), &ro, "bench_example_pulse", &ok).unwrap_err();
        assert!(e.contains("needs tier"), "{e}");
    }

    // Tests [@ANCHOR: example_safe_state]
    #[test]
    fn the_output_is_switched_off_when_the_pulse_fails_and_a_stuck_output_is_reported_loudly() {
        let mut ok = json!({"level_v": 0.5, "pulse_s": 0.1, "confirm": true});
        // The connection drops while the output is on: the host cannot confirm off, says so, and the device's own
        // time limit (OUTP:TMO, sent before ON) is what switches it off.
        let hangup = FakeDevice::sim(Sim {
            hangup_on: Some("OUTP ON".into()),
            ..Sim::default()
        });
        let e = call_with(&hangup.connector(), &bench(""), "bench_example_pulse", &ok).unwrap_err();
        assert!(e.contains("SAFE STATE NOT CONFIRMED"), "{e}");
        assert!(hangup.received().contains(&"OUTP:TMO 1.1".to_string()));
        // An instrument that will not go off: the answer says SAFE STATE NOT CONFIRMED and what to do by hand.
        let stuck = FakeDevice::sim(Sim {
            stuck_on: true,
            ..Sim::default()
        });
        let e = call_with(&stuck.connector(), &bench(""), "bench_example_pulse", &ok).unwrap_err();
        assert!(
            e.contains("SAFE STATE NOT CONFIRMED") && e.contains("front panel"),
            "{e}"
        );
        let e = call_with(
            &stuck.connector(),
            &bench(""),
            "bench_example_safe_state",
            &json!({}),
        )
        .unwrap_err();
        assert!(e.contains("SAFE STATE NOT CONFIRMED"), "{e}");
        let fine = FakeDevice::sim(Sim::default());
        let v = call_with(
            &fine.connector(),
            &bench(""),
            "bench_example_safe_state",
            &json!({}),
        )
        .unwrap();
        assert_eq!(v["verified"], true);
        ok["pulse_s"] = json!(9.0);
        assert!(call_with(&fine.connector(), &bench(""), "bench_example_pulse", &ok).is_err());
    }
}
