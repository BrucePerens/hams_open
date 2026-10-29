// SPDX-License-Identifier: AGPL-3.0-or-later
//! Antenna rotators through Hamlib's `rotctld` daemon, over its documented TCP protocol (the same text protocol as `rigctld`, so the
//! client of the `hamlib` module is used). Hamlib is LGPL and is not linked. A rotator is a motor: the limits in the owner's config
//! narrow what the rotator itself reports, every move is checked before it is sent, and the safe-state pair `bench_rotator_stop`
//! and `bench_rotator_park` is always available.
//!
//! The entry in the bench config: `{"driver": "rotator", "tier": "read_config", "rotctld_port": 4533, "min_az": 0, "max_az": 360,
//! "min_el": 0, "max_el": 90}`. This step talks to a `rotctld` that already runs (the owner starts it for the rotator, for example
//! `rotctld -m 202 -r /dev/serial/by-id/... -s 9600 -t 4533 -T 127.0.0.1`); a managed rotctld like the rig tools have is not done.

use crate::config::{Instrument, Kind, Tier};
use crate::hamlib::proto::{code_name, Client, RigError};
use rmcp::model::Tool;
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// The most a call may wait for a move to finish.
const HARD_MAX_WAIT_S: f64 = 600.0;

/// The `rotator` settings of an instrument entry.
#[derive(Clone, Debug, PartialEq)]
pub struct Config {
    pub host: String,
    pub port: u16,
    pub reply_timeout_s: u64,
    /// The owner's limits in degrees; each narrows what the rotator reports. `None`: the rotator's own.
    pub min_az: Option<f64>,
    pub max_az: Option<f64>,
    pub min_el: Option<f64>,
    pub max_el: Option<f64>,
    /// How close (degrees) the reported position must be to the target for a move to count as arrived.
    pub tolerance_deg: f64,
    /// The longest a call waits for a move, seconds.
    pub max_wait_s: f64,
}

impl Default for Config {
    fn default() -> Config {
        Config {
            host: "127.0.0.1".into(),
            port: 4533,
            reply_timeout_s: 3,
            min_az: None,
            max_az: None,
            min_el: None,
            max_el: None,
            tolerance_deg: 2.0,
            max_wait_s: 120.0,
        }
    }
}

fn degrees(c: &Value, key: &str, lo: f64, hi: f64) -> Result<Option<f64>, String> {
    // [@ANCHOR: rotator_degrees]
    match c.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v
            .as_f64()
            .filter(|x| x.is_finite() && (lo..=hi).contains(x))
            .map(Some)
            .ok_or_else(|| format!("{key} must be a number from {lo} to {hi} degrees")),
    }
}

/// Reads `rotctld_port` (required), `rotctld_host` (this machine unless `allow_remote_rotctld`), `reply_timeout_s`, the limits
/// `min_az`, `max_az`, `min_el`, `max_el` (degrees), `tolerance_deg` and `max_wait_s` (up to 600).
pub fn parse_config(c: &Value) -> Result<Config, String> {
    // [@ANCHOR: rotator_config]
    let port = c
        .get("rotctld_port")
        .and_then(|p| p.as_u64())
        .and_then(|p| u16::try_from(p).ok())
        .filter(|p| *p > 0)
        .ok_or(
            "a rotator needs rotctld_port (1 to 65535): the port of a rotctld that already runs",
        )?;
    let mut cfg = Config {
        port,
        ..Config::default()
    };
    if let Some(h) = c.get("rotctld_host") {
        let h = h
            .as_str()
            .filter(|h| {
                !h.is_empty()
                    && h.bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b".-:".contains(&b))
            })
            .ok_or("rotctld_host must be a host name or address")?;
        let remote_ok = c
            .get("allow_remote_rotctld")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        if !matches!(h, "127.0.0.1" | "localhost" | "::1") && !remote_ok {
            return Err(format!("rotctld_host {h:?} is not this machine. rotctld has no authentication and must not be shared on a network; set allow_remote_rotctld true only if the owner accepts that"));
        }
        cfg.host = h.to_string();
    }
    if let Some(t) = c.get("reply_timeout_s") {
        cfg.reply_timeout_s = t
            .as_u64()
            .filter(|t| (1..=30).contains(t))
            .ok_or("reply_timeout_s must be a whole number from 1 to 30")?;
    }
    cfg.min_az = degrees(c, "min_az", -360.0, 720.0)?;
    cfg.max_az = degrees(c, "max_az", -360.0, 720.0)?;
    cfg.min_el = degrees(c, "min_el", -90.0, 180.0)?;
    cfg.max_el = degrees(c, "max_el", -90.0, 180.0)?;
    if cfg.min_az.zip(cfg.max_az).is_some_and(|(a, b)| a >= b)
        || cfg.min_el.zip(cfg.max_el).is_some_and(|(a, b)| a >= b)
    {
        return Err("min_az must be below max_az and min_el below max_el".into());
    }
    cfg.tolerance_deg = degrees(c, "tolerance_deg", 0.1, 20.0)?.unwrap_or(cfg.tolerance_deg);
    cfg.max_wait_s = degrees(c, "max_wait_s", 1.0, HARD_MAX_WAIT_S)?.unwrap_or(cfg.max_wait_s);
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

pub const SPECS: &[Spec] = &[
    Spec {
        name: "bench_rotator_status",
        tier: Tier::ReadOnly,
        desc: "ROTATOR: the position (azimuth and elevation in degrees), the limits (the rotator's own narrowed by the owner's config), the rotator's info text and whether it is moving (two position reads apart). Changes nothing.",
        props: props!(r#""instrument":{"type":"string"}"#),
        required: &[],
    },
    Spec {
        name: "bench_rotator_point",
        tier: Tier::ReadConfig,
        desc: "ROTATOR MOVES A MOTOR: points the antenna at `azimuth_deg` and `elevation_deg` (elevation omitted: the current one), inside the limits (the rotator's own narrowed by the owner's config; checked before anything is sent). With `wait_s` it waits up to that long (at most the config's max_wait_s) for the position to arrive within the tolerance and says whether it did; the position is always read back. `dry_run` returns the plan and sends nothing. The safe-state companions are bench_rotator_stop and bench_rotator_park.",
        props: props!(r#""instrument":{"type":"string"},"azimuth_deg":{"type":"number","description":"azimuth in degrees, inside the limits"},"elevation_deg":{"type":"number","description":"elevation in degrees, inside the limits; default the current elevation"},"wait_s":{"type":"number","description":"seconds to wait for arrival, 0 to max_wait_s, default 0"},"dry_run":{"type":"boolean","description":"return the plan and send nothing"}"#),
        required: &["azimuth_deg"],
    },
    Spec {
        name: "bench_rotator_stop",
        tier: Tier::ReadConfig,
        desc: "ROTATOR safe state: stops the motion at once and checks, with two position reads apart, that the rotator no longer moves. Safe at any time; use it first when anything goes wrong. If the rotator still moves, the answer says what to do by hand.",
        props: props!(r#""instrument":{"type":"string"}"#),
        required: &[],
    },
    Spec {
        name: "bench_rotator_park",
        tier: Tier::ReadConfig,
        desc: "ROTATOR: sends the rotator to its park position. With `wait_s` it waits up to that long (at most the config's max_wait_s) for the motion to end. Use bench_rotator_stop to stop it.",
        props: props!(r#""instrument":{"type":"string"},"wait_s":{"type":"number","description":"seconds to wait for the motion to end, 0 to max_wait_s, default 0"}"#),
        required: &[],
    },
];

fn instrument<'a>(insts: &'a [Instrument], args: &Value) -> Result<&'a Instrument, String> {
    // [@ANCHOR: rotator_instrument]
    let ours: Vec<&Instrument> = insts.iter().filter(|i| i.kind == Kind::Rotator).collect();
    if let Some(n) = args.get("instrument").and_then(|n| n.as_str()) {
        return ours
            .iter()
            .copied()
            .find(|i| i.name == n)
            .ok_or_else(|| format!("no rotator named {n:?}"));
    }
    match ours.as_slice() {
        [one] => Ok(one),
        [] => Err("no rotator is configured".into()),
        many => Err(format!(
            "name the rotator: {}",
            many.iter()
                .map(|i| i.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )),
    }
}

/// The tools to list: those at or below the tier of a configured rotator.
pub fn tool_defs(insts: &[Instrument]) -> Vec<Tool> {
    SPECS
        .iter()
        .filter(|s| insts.iter().any(|i| i.kind == Kind::Rotator && i.tier >= s.tier))
        .map(|s| {
            let schema = json!({"type": "object", "properties": serde_json::from_str::<Value>(s.props).unwrap(), "required": s.required});
            let Value::Object(schema) = schema else { unreachable!() };
            Tool::new(s.name, format!("[{:?}] {}", s.tier, s.desc), Arc::new(schema))
        })
        .collect()
}

/// An error of the rotator link as text, ending with the next action for a rotator (the shared client's own advice is about rigs).
fn text(e: RigError) -> String {
    // [@ANCHOR: rotator_error_text]
    match e {
        RigError::Link(m) => {
            let head = m
                .split(" Next:")
                .next()
                .unwrap_or(&m)
                .trim_end_matches('.')
                .to_string();
            format!("{head}. Next: check that rotctld runs on the host and port in the config, the rotator's cable and power, and restart rotctld; then bench_rotator_stop if it may be moving.")
        }
        RigError::Rig { code, cmd } => format!(
            "rotctld refused {cmd}: RPRT {code} ({}). Next: {}",
            code_name(code),
            if code == -21 || code == -1 || code == -17 {
                "the position is outside what the rotator accepts; bench_rotator_status shows its limits"
            } else {
                "check the rotator, its cable and power; then bench_rotator_stop if it may be moving"
            }
        ),
    }
}

fn open(inst: &Instrument) -> Result<Client, String> {
    let c = &inst.rotator;
    Client::connect(&c.host, c.port, Duration::from_secs(c.reply_timeout_s)).map_err(text)
}

/// Azimuth and elevation, read now.
fn position(c: &mut Client) -> Result<(f64, f64), String> {
    // [@ANCHOR: rotator_position]
    let r = c.read("get_pos").map_err(text)?;
    Ok((
        r.number(Some("Azimuth")).map_err(text)?,
        r.number(Some("Elevation")).map_err(text)?,
    ))
}

/// The rotator's limits (azimuth low and high, elevation low and high) from `dump_state`, narrowed by the owner's.
pub fn limits(c: &mut Client, cfg: &Config) -> Result<([f64; 4], String), String> {
    // [@ANCHOR: rotator_limits]
    let r = c.read("dump_state").map_err(text)?;
    let get = |k: &str| r.kv(k).and_then(|v| v.parse::<f64>().ok());
    let own = [
        get("Minimum Azimuth"),
        get("Maximum Azimuth"),
        get("Minimum Elevation"),
        get("Maximum Elevation"),
    ];
    let owner = [cfg.min_az, cfg.max_az, cfg.min_el, cfg.max_el];
    let mut out = [0.0; 4];
    for i in 0..4 {
        let (a, b) = (own[i], owner[i]);
        out[i] = match (a, b) {
            (Some(a), Some(b)) => if i % 2 == 0 { a.max(b) } else { a.min(b) },
            (Some(a), None) => a,
            (None, Some(b)) => b,
            (None, None) => return Err("the rotator reports no limits and the config sets none for it; set min_az, max_az, min_el and max_el in the config: a rotator is not moved without limits".into()),
        };
    }
    let source = if owner.iter().all(|o| o.is_none()) {
        "the rotator's reported limits"
    } else {
        "the rotator's reported limits narrowed by min_az, max_az, min_el and max_el in the bench config"
    };
    Ok((out, source.into()))
}

/// Refuses a position outside the limits, naming them and where they come from.
pub fn check_target(az: f64, el: f64, lim: &[f64; 4], source: &str) -> Result<(), String> {
    // [@ANCHOR: rotator_target_check]
    if !(az.is_finite() && el.is_finite()) {
        return Err("azimuth_deg and elevation_deg must be finite numbers".into());
    }
    if az < lim[0] || az > lim[1] || el < lim[2] || el > lim[3] {
        return Err(format!("azimuth {az} / elevation {el} is outside the limits (azimuth {} to {}, elevation {} to {} degrees); source: {source}. Nothing was sent.", lim[0], lim[1], lim[2], lim[3]));
    }
    Ok(())
}

fn moving(c: &mut Client) -> Result<(bool, (f64, f64)), String> {
    // [@ANCHOR: rotator_moving]
    let a = position(c)?;
    std::thread::sleep(Duration::from_millis(500));
    let b = position(c)?;
    Ok((((a.0 - b.0).abs() > 0.05) || ((a.1 - b.1).abs() > 0.05), b))
}

fn status(c: &mut Client, cfg: &Config) -> Result<Value, String> {
    // [@ANCHOR: rotator_status]
    let (lim, source) = limits(c, cfg).unwrap_or(([f64::NAN; 4], "no limits known".into()));
    let (is_moving, pos) = moving(c)?;
    let info = c
        .read("get_info")
        .ok()
        .and_then(|r| r.kv("Info").map(String::from));
    Ok(json!({
        "azimuth_deg": pos.0, "elevation_deg": pos.1, "moving": is_moving,
        "limits": if lim[0].is_nan() { Value::Null } else { json!({"min_az": lim[0], "max_az": lim[1], "min_el": lim[2], "max_el": lim[3], "source": source}) },
        "info": info,
    }))
}

/// Waits until the position is within the tolerance of the target (or `done` says the motion ended), at most `wait_s`.
fn wait_for(
    c: &mut Client,
    target: Option<(f64, f64)>,
    tol: f64,
    wait_s: f64,
) -> Result<(bool, (f64, f64), f64), String> {
    // [@ANCHOR: rotator_wait]
    let began = Instant::now();
    let mut last = position(c)?;
    loop {
        let arrived = match target {
            Some((az, el)) => (last.0 - az).abs() <= tol && (last.1 - el).abs() <= tol,
            None => false,
        };
        if arrived {
            return Ok((true, last, began.elapsed().as_secs_f64()));
        }
        if began.elapsed().as_secs_f64() >= wait_s {
            return Ok((false, last, began.elapsed().as_secs_f64()));
        }
        std::thread::sleep(Duration::from_millis(500));
        let now = position(c)?;
        // Without a target (park), a position that stops changing is the end of the motion.
        if target.is_none() && (now.0 - last.0).abs() < 0.05 && (now.1 - last.1).abs() < 0.05 {
            return Ok((true, now, began.elapsed().as_secs_f64()));
        }
        last = now;
    }
}

fn wait_arg(args: &Value, cfg: &Config) -> Result<f64, String> {
    // [@ANCHOR: rotator_wait_arg]
    match args.get("wait_s") {
        None | Some(Value::Null) => Ok(0.0),
        Some(v) => v
            .as_f64()
            .filter(|x| x.is_finite() && (0.0..=cfg.max_wait_s).contains(x))
            .ok_or_else(|| {
                format!(
                    "wait_s must be a number from 0 to {} (max_wait_s in the config)",
                    cfg.max_wait_s
                )
            }),
    }
}

fn point(c: &mut Client, cfg: &Config, args: &Value) -> Result<Value, String> {
    // [@ANCHOR: rotator_point]
    let az = args
        .get("azimuth_deg")
        .and_then(|v| v.as_f64())
        .ok_or("azimuth_deg is required (degrees)")?;
    let dry = match args.get("dry_run") {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(_) => return Err("dry_run must be true or false".into()),
    };
    let wait = wait_arg(args, cfg)?;
    let (lim, source) = limits(c, cfg)?;
    let now = position(c)?;
    let el = match args.get("elevation_deg") {
        None | Some(Value::Null) => now.1,
        Some(v) => v.as_f64().ok_or("elevation_deg must be a number")?,
    };
    check_target(az, el, &lim, &source)?;
    if dry {
        return Ok(
            json!({"dry_run": true, "sent": [], "would": [format!("set_pos {az} {el}"), format!("read the position back; wait up to {wait} s for arrival within {} degrees", cfg.tolerance_deg)], "position_now": {"azimuth_deg": now.0, "elevation_deg": now.1}, "limits_source": source}),
        );
    }
    c.command(&format!("set_pos {az} {el}")).map_err(text)?;
    let (arrived, pos, took) = if wait > 0.0 {
        wait_for(c, Some((az, el)), cfg.tolerance_deg, wait)?
    } else {
        (false, position(c)?, 0.0)
    };
    Ok(json!({
        "target": {"azimuth_deg": az, "elevation_deg": el}, "position": {"azimuth_deg": pos.0, "elevation_deg": pos.1},
        "waited": wait > 0.0, "arrived": arrived, "waited_s": (took * 10.0).round() / 10.0, "tolerance_deg": cfg.tolerance_deg,
        "note": if wait > 0.0 && !arrived { "it had not arrived when the wait ended; it may still be moving: bench_rotator_status, bench_rotator_stop" } else { "the position is the read-back at the end of the call" },
    }))
}

fn stop(c: &mut Client) -> Result<Value, String> {
    // [@ANCHOR: rotator_stop]
    let mut last = String::new();
    for _ in 0..3 {
        match c.command("stop") {
            Ok(_) => {
                let (is_moving, pos) = moving(c)?;
                if !is_moving {
                    return Ok(
                        json!({"stopped": true, "moving": false, "azimuth_deg": pos.0, "elevation_deg": pos.1}),
                    );
                }
                last = "the position still changes".into();
            }
            Err(e) => last = text(e),
        }
    }
    Err(format!("STOP NOT CONFIRMED: {last}. The rotator may still be moving. Do this now: remove power from the rotator's controller, or use its own stop control. Do not use it again until the owner has looked at it."))
}

fn park(c: &mut Client, cfg: &Config, args: &Value) -> Result<Value, String> {
    // [@ANCHOR: rotator_park]
    let wait = wait_arg(args, cfg)?;
    c.command("park").map_err(text)?;
    let (done, pos, took) = if wait > 0.0 {
        wait_for(c, None, cfg.tolerance_deg, wait)?
    } else {
        (false, position(c)?, 0.0)
    };
    Ok(
        json!({"parked_command_sent": true, "motion_ended": wait > 0.0 && done, "position": {"azimuth_deg": pos.0, "elevation_deg": pos.1}, "waited_s": (took * 10.0).round() / 10.0}),
    )
}

pub fn call(insts: &[Instrument], name: &str, args: &Value) -> Result<Value, String> {
    // [@ANCHOR: rotator_call]
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
    // The stop does not wait for a move that is running: it is the way to end one.
    let lock = crate::instrument_lock(&inst.name);
    let _held = if name == "bench_rotator_stop" {
        None
    } else {
        Some(lock.lock().unwrap_or_else(|e| e.into_inner()))
    };
    let mut c = open(inst)?;
    match name {
        "bench_rotator_status" => status(&mut c, &inst.rotator),
        "bench_rotator_point" => point(&mut c, &inst.rotator, args),
        "bench_rotator_stop" => stop(&mut c),
        "bench_rotator_park" => park(&mut c, &inst.rotator, args),
        _ => Err(format!("{name} does not apply to {:?}", inst.name)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hamlib::testkit::{err, ok, FakeRigctld, Reply};

    /// Hamlib's dummy rotator, in its own rotctld on a free localhost port: the owner's "rotctld that already runs".
    struct Rotctld {
        child: std::process::Child,
        port: u16,
    }

    impl Rotctld {
        fn start() -> Rotctld {
            let port = std::net::TcpListener::bind("127.0.0.1:0")
                .unwrap()
                .local_addr()
                .unwrap()
                .port();
            let child = std::process::Command::new("rotctld")
                .args(["-m", "1", "-t", &port.to_string(), "-T", "127.0.0.1"])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
                .expect("rotctld (libhamlib-utils) is needed for this test");
            let until = Instant::now() + Duration::from_secs(10);
            while std::net::TcpStream::connect(("127.0.0.1", port)).is_err() {
                assert!(Instant::now() < until, "rotctld did not start");
                std::thread::sleep(Duration::from_millis(50));
            }
            Rotctld { child, port }
        }
    }

    impl Drop for Rotctld {
        fn drop(&mut self) {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }

    fn bench(name: &str, port: u16, tier: &str, extra: &str) -> Vec<Instrument> {
        crate::config::parse(&format!(r#"{{"instruments":{{"{name}":{{"driver":"rotator","tier":"{tier}","rotctld_port":{port}{extra}}}}}}}"#)).unwrap()
    }

    fn parse(extra: &str) -> Result<Config, String> {
        parse_config(&serde_json::from_str::<Value>(&format!("{{{extra}}}")).unwrap())
    }

    #[test]
    fn every_tool_schema_is_valid_json() {
        for spec in SPECS {
            serde_json::from_str::<Value>(spec.props)
                .unwrap_or_else(|e| panic!("{}: {e}", spec.name));
        }
    }

    // Tests [@ANCHOR: rotator_config]
    // Tests [@ANCHOR: rotator_degrees]
    #[test]
    fn config_needs_a_port_keeps_rotctld_on_this_machine_and_refuses_bad_limits() {
        let c = parse(r#""rotctld_port":4533,"min_az":0,"max_az":360,"min_el":0,"max_el":90,"tolerance_deg":1,"max_wait_s":300"#).unwrap();
        assert_eq!(
            (c.port, c.min_az, c.max_az, c.tolerance_deg, c.max_wait_s),
            (4533, Some(0.0), Some(360.0), 1.0, 300.0)
        );
        for bad in [
            r#""#,
            r#""rotctld_port":0"#,
            r#""rotctld_port":4533,"rotctld_host":"192.0.2.7""#,
            r#""rotctld_port":4533,"min_az":400,"max_az":300"#,
            r#""rotctld_port":4533,"min_el":"x""#,
            r#""rotctld_port":4533,"max_wait_s":9000"#,
            r#""rotctld_port":4533,"reply_timeout_s":0"#,
            r#""rotctld_port":4533,"min_az":99999"#,
        ] {
            assert!(parse(bad).is_err(), "{bad}");
        }
        assert!(parse(
            r#""rotctld_port":4533,"rotctld_host":"192.0.2.7","allow_remote_rotctld":true"#
        )
        .is_ok());
    }

    // Tests [@ANCHOR: rotator_call]
    // Tests [@ANCHOR: rotator_instrument]
    // Tests [@ANCHOR: rotator_status]
    // Tests [@ANCHOR: rotator_position]
    // Tests [@ANCHOR: rotator_limits]
    // Tests [@ANCHOR: rotator_moving]
    #[test]
    fn the_status_of_the_dummy_rotator_has_its_position_limits_and_motion() {
        let d = Rotctld::start();
        let b = bench(
            "rot_status",
            d.port,
            "read_only",
            r#","max_az":360,"min_az":0"#,
        );
        let v = call(&b, "bench_rotator_status", &json!({})).unwrap();
        assert_eq!(
            (v["azimuth_deg"].clone(), v["moving"].clone()),
            (json!(0.0), json!(false)),
            "{v}"
        );
        // The rotator allows -180 to 450; the owner narrows azimuth to 0 to 360; elevation stays the rotator's.
        assert_eq!(
            (
                v["limits"]["min_az"].clone(),
                v["limits"]["max_az"].clone(),
                v["limits"]["max_el"].clone()
            ),
            (json!(0.0), json!(360.0), json!(90.0))
        );
        assert!(v["limits"]["source"].as_str().unwrap().contains("narrowed"));
        assert!(v["info"].as_str().unwrap().contains("Dummy"));
        // Tiers, names and unknown tools.
        assert!(call(&b, "bench_rotator_point", &json!({"azimuth_deg": 10}))
            .unwrap_err()
            .contains("needs tier"));
        assert!(
            call(&b, "bench_rotator_status", &json!({"instrument": "x"}))
                .unwrap_err()
                .contains("no rotator named")
        );
        assert!(call(&b, "bench_rotator_nonsense", &json!({})).is_err());
        let two = crate::config::parse(&format!(r#"{{"instruments":{{"a":{{"driver":"rotator","rotctld_port":{0}}},"b":{{"driver":"rotator","rotctld_port":{0}}}}}}}"#, d.port)).unwrap();
        assert!(call(&two, "bench_rotator_status", &json!({}))
            .unwrap_err()
            .contains("name the rotator: a, b"));
        // The tools listed follow the tier.
        let names = |t: &str| {
            tool_defs(&bench("r", d.port, t, ""))
                .iter()
                .map(|x| x.name.to_string())
                .collect::<Vec<_>>()
        };
        assert_eq!(names("read_only"), ["bench_rotator_status"]);
        assert_eq!(names("read_config").len(), 4);
    }

    // Tests [@ANCHOR: rotator_target_check]
    // Tests [@ANCHOR: rotator_point]
    #[test]
    fn a_target_outside_the_limits_or_a_dry_run_sends_nothing_and_a_move_is_read_back() {
        let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
        let s = seen.clone();
        let fake = FakeRigctld::serve(move |c| {
            s.lock().unwrap().push(c.to_string());
            match c {
                "get_pos" => ok(&["Azimuth: 10.0", "Elevation: 5.0"]),
                "dump_state" => ok(&[
                    "rotctld Protocol Ver: 1",
                    "Minimum Azimuth: -180.000000",
                    "Maximum Azimuth: 450.000000",
                    "Minimum Elevation: 0.000000",
                    "Maximum Elevation: 90.000000",
                    "done",
                ]),
                _ => ok(&[]),
            }
        });
        let b = bench(
            "rot_target",
            fake.port,
            "read_config",
            r#","min_az":0,"max_az":360"#,
        );
        for (az, el) in [(400.0, 10.0), (-5.0, 10.0), (90.0, 120.0), (90.0, -1.0)] {
            let e = call(
                &b,
                "bench_rotator_point",
                &json!({"azimuth_deg": az, "elevation_deg": el}),
            )
            .unwrap_err();
            assert!(
                e.contains("outside the limits")
                    && e.contains("Nothing was sent")
                    && e.contains("source"),
                "{e}"
            );
        }
        assert!(call(&b, "bench_rotator_point", &json!({})).is_err());
        assert!(call(
            &b,
            "bench_rotator_point",
            &json!({"azimuth_deg": 10, "wait_s": 9999})
        )
        .is_err());
        assert!(call(
            &b,
            "bench_rotator_point",
            &json!({"azimuth_deg": 10, "dry_run": "yes"})
        )
        .is_err());
        let dry = call(
            &b,
            "bench_rotator_point",
            &json!({"azimuth_deg": 90, "dry_run": true}),
        )
        .unwrap();
        assert_eq!(dry["dry_run"], true);
        assert_eq!(
            dry["would"][0], "set_pos 90 5",
            "elevation omitted: the current one"
        );
        assert!(
            seen.lock()
                .unwrap()
                .iter()
                .all(|c| !c.starts_with("set_pos")),
            "nothing moved: {:?}",
            seen.lock().unwrap()
        );
        // A real move is sent as checked and the position is read back.
        let v = call(
            &b,
            "bench_rotator_point",
            &json!({"azimuth_deg": 90, "elevation_deg": 20}),
        )
        .unwrap();
        assert!(seen.lock().unwrap().contains(&"set_pos 90 20".to_string()));
        assert_eq!(v["position"]["azimuth_deg"], 10.0);
        assert_eq!(v["waited"], false);
    }

    // Tests [@ANCHOR: rotator_wait]
    // Tests [@ANCHOR: rotator_wait_arg]
    #[test]
    fn a_move_waits_for_arrival_and_a_wait_that_ends_first_is_reported_not_hidden() {
        let d = Rotctld::start();
        let b = bench(
            "rot_wait",
            d.port,
            "read_config",
            r#","tolerance_deg":2,"max_wait_s":30"#,
        );
        let v = call(
            &b,
            "bench_rotator_point",
            &json!({"azimuth_deg": 10, "elevation_deg": 5, "wait_s": 10}),
        )
        .unwrap();
        assert_eq!(v["arrived"], true, "{v}");
        let pos = v["position"]["azimuth_deg"].as_f64().unwrap();
        assert!((pos - 10.0).abs() <= 2.0, "{v}");
        // A far target and a short wait: not arrived, said plainly; then it is stopped.
        let v = call(
            &b,
            "bench_rotator_point",
            &json!({"azimuth_deg": 300, "elevation_deg": 5, "wait_s": 1}),
        )
        .unwrap();
        assert_eq!(v["arrived"], false);
        assert!(v["note"].as_str().unwrap().contains("may still be moving"));
        assert!(call(&b, "bench_rotator_stop", &json!({})).unwrap()["stopped"] == true);
    }

    // Tests [@ANCHOR: rotator_stop]
    #[test]
    fn the_stop_ends_a_move_and_works_while_another_call_holds_the_lock() {
        let d = Rotctld::start();
        let b = bench("rot_stop", d.port, "read_config", "");
        call(
            &b,
            "bench_rotator_point",
            &json!({"azimuth_deg": 200, "elevation_deg": 10}),
        )
        .unwrap();
        std::thread::sleep(Duration::from_millis(600));
        let moving = call(&b, "bench_rotator_status", &json!({})).unwrap();
        assert_eq!(moving["moving"], true, "{moving}");
        // A long call holds the instrument lock; the stop does not wait for it.
        let held = crate::instrument_lock("rot_stop");
        let _g = held.lock().unwrap();
        let v = call(&b, "bench_rotator_stop", &json!({})).unwrap();
        assert_eq!(
            (v["stopped"].clone(), v["moving"].clone()),
            (json!(true), json!(false)),
            "{v}"
        );
        drop(_g);
        let after = call(&b, "bench_rotator_status", &json!({})).unwrap();
        assert_eq!(after["moving"], false);
        // A rotator that keeps moving after the stop is reported loudly, with what to do by hand.
        let mut az = 0.0;
        let fake = FakeRigctld::serve(move |c| {
            if c == "get_pos" {
                az += 1.0;
                return ok(&[&format!("Azimuth: {az}"), "Elevation: 0"]);
            }
            ok(&[])
        });
        let e = call(
            &bench("rot_stuck", fake.port, "read_config", ""),
            "bench_rotator_stop",
            &json!({}),
        )
        .unwrap_err();
        assert!(
            e.contains("STOP NOT CONFIRMED") && e.contains("remove power"),
            "{e}"
        );
    }

    // Tests [@ANCHOR: rotator_park]
    #[test]
    fn the_park_sends_the_rotator_home_and_can_wait_for_the_end_of_the_motion() {
        let d = Rotctld::start();
        let b = bench("rot_park", d.port, "read_config", "");
        call(
            &b,
            "bench_rotator_point",
            &json!({"azimuth_deg": 12, "elevation_deg": 6, "wait_s": 10}),
        )
        .unwrap();
        let v = call(&b, "bench_rotator_park", &json!({"wait_s": 20})).unwrap();
        assert_eq!(v["motion_ended"], true, "{v}");
        assert!(v["position"]["azimuth_deg"].as_f64().unwrap() < 1.0, "{v}");
        assert_eq!(
            call(&b, "bench_rotator_park", &json!({})).unwrap()["motion_ended"],
            false,
            "without wait it does not claim the end"
        );
    }

    // Tests [@ANCHOR: rotator_error_text]
    #[test]
    fn a_rotator_that_does_not_answer_or_refuses_is_an_error_with_a_next_action_for_a_rotator() {
        let silent = FakeRigctld::serve(|_| Reply::Silent);
        let e = call(
            &bench(
                "rot_silent",
                silent.port,
                "read_only",
                r#","reply_timeout_s":1"#,
            ),
            "bench_rotator_status",
            &json!({}),
        )
        .unwrap_err();
        assert!(
            e.contains("Next:") && e.contains("rotctld") && !e.contains("bench_hamlib"),
            "{e}"
        );
        let refusing = FakeRigctld::serve(|c| if c == "get_pos" { err(-21) } else { ok(&[]) });
        let e = call(
            &bench("rot_refuse", refusing.port, "read_only", ""),
            "bench_rotator_status",
            &json!({}),
        )
        .unwrap_err();
        assert!(
            e.contains("RPRT -21") && e.contains("Next:") && !e.contains("bench_hamlib"),
            "{e}"
        );
        let refused_port = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let e = call(
            &bench("rot_down", refused_port, "read_only", ""),
            "bench_rotator_status",
            &json!({}),
        )
        .unwrap_err();
        assert!(
            e.contains("cannot connect") && !e.contains("bench_hamlib"),
            "{e}"
        );
    }
}
