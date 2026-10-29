// SPDX-License-Identifier: AGPL-3.0-or-later
//! The read-only tools: everything a receiver reports, in one call (`bench_hamlib_status`) or one value at a time (`bench_hamlib_get`).
//! A function the rig lacks is reported as unavailable with the reason; a link failure ends the call with an error, so a value that
//! was not just read is never returned.

use super::proto::{check_arg, Client, RigError};
use serde_json::{json, Map, Value};

/// The levels a receiver status shows (the RX side first, then the transmit-side meters, which read zero when not keyed).
pub const STATUS_LEVELS: &[&str] = &[
    "STRENGTH",
    "RAWSTR",
    "AF",
    "RF",
    "SQL",
    "IF",
    "APF",
    "NR",
    "NB",
    "PBT_IN",
    "PBT_OUT",
    "CWPITCH",
    "NOTCHF",
    "BAL",
    "AGC",
    "AGC_TIME",
    "ATT",
    "PREAMP",
    "RFPOWER",
    "SWR",
    "ALC",
    "TEMP_METER",
    "RFPOWER_METER_WATTS",
];
pub const STATUS_FUNCS: &[&str] = &[
    "NB", "NR", "ANF", "APF", "AIP", "MN", "SQL", "LOCK", "MUTE", "TUNER", "COMP", "VOX", "MON",
    "RIT", "XIT",
];
const AGC_NAMES: &[(i64, &str)] = &[
    (0, "off"),
    (1, "superfast"),
    (2, "fast"),
    (3, "slow"),
    (4, "user"),
    (5, "medium"),
    (6, "auto"),
];

/// The S-meter reading in S-units from Hamlib's STRENGTH (dB relative to S9): S9 at 0 dB, 6 dB per S-unit below it.
pub fn s_units(db_rel_s9: f64) -> f64 {
    // [@ANCHOR: hamlib_s_units]
    let s = if db_rel_s9 >= 0.0 {
        9.0
    } else {
        (9.0 + db_rel_s9 / 6.0).max(0.0)
    };
    (s * 10.0).round() / 10.0
}

/// "S7", or "S9+20 dB" above S9.
pub fn s_text(db_rel_s9: f64) -> String {
    if db_rel_s9 >= 0.5 {
        format!("S9+{:.0} dB", db_rel_s9)
    } else {
        format!("S{:.0}", s_units(db_rel_s9))
    }
}

/// A read whose rig-side refusal is data (`Ok(Err)`) and whose link failure is an error.
pub type Read<T> = Result<Result<T, RigError>, String>;

pub fn tolerate<T>(r: Result<T, RigError>) -> Read<T> {
    match r {
        Err(RigError::Link(m)) => Err(m),
        other => Ok(other),
    }
}

/// The names a `get_level ?` or `get_func ?` style query lists. `None` when the rig will not say.
pub fn listed(c: &mut Client, cmd: &str) -> Result<Option<Vec<String>>, String> {
    // [@ANCHOR: hamlib_capability_query]
    match tolerate(c.read(cmd))? {
        Ok(r) => Ok(Some(
            r.lines
                .iter()
                .flat_map(|l| l.split_whitespace().map(String::from))
                .collect(),
        )),
        Err(_) => Ok(None),
    }
}

fn mode_of(c: &mut Client) -> Read<(String, f64)> {
    tolerate(c.read("get_mode").and_then(|r| {
        let mode = r
            .kv("Mode")
            .ok_or_else(|| RigError::Link(format!("get_mode reply has no Mode: {:?}", r.lines)))?
            .to_string();
        Ok((mode, r.number(Some("Passband"))?))
    }))
}

fn level_json(name: &str, v: f64) -> Value {
    match name {
        "STRENGTH" => json!({"db_relative_to_s9": v, "s_units": s_units(v), "text": s_text(v)}),
        "AGC" => {
            let code = v.round() as i64;
            json!({"code": code, "name": AGC_NAMES.iter().find(|(c, _)| *c == code).map(|(_, n)| *n)})
        }
        _ => json!(v),
    }
}

pub fn status(c: &mut Client, rig: &str) -> Result<Value, String> {
    // [@ANCHOR: hamlib_status]
    let mut out = Map::new();
    let mut gaps = Map::new();
    out.insert("rig".into(), json!(rig));
    let note = |gaps: &mut Map<String, Value>, what: &str, e: &RigError| {
        gaps.insert(
            what.into(),
            json!(if e.unsupported() {
                "not available on this rig".to_string()
            } else {
                e.to_string()
            }),
        );
    };
    match tolerate(c.read_number("get_freq", Some("Frequency")))? {
        Ok(f) => out.insert("frequency_hz".into(), json!(f)),
        Err(e) => {
            note(&mut gaps, "frequency", &e);
            None
        }
    };
    match mode_of(c)? {
        Ok((m, w)) => {
            out.insert("mode".into(), json!(m));
            out.insert("passband_hz".into(), json!(w));
        }
        Err(e) => note(&mut gaps, "mode", &e),
    }
    match tolerate(c.read("get_vfo"))? {
        Ok(r) => {
            out.insert("vfo".into(), json!(r.kv("VFO")));
        }
        Err(e) => note(&mut gaps, "vfo", &e),
    }
    match tolerate(c.read("get_split_vfo"))? {
        Ok(r) => {
            out.insert(
                "split".into(),
                json!({"enabled": r.kv("Split").map(|s| s != "0"), "tx_vfo": r.kv("TX VFO")}),
            );
        }
        Err(e) => note(&mut gaps, "split", &e),
    }
    for (key, cmd, k) in [("rit_hz", "get_rit", "RIT"), ("xit_hz", "get_xit", "XIT")] {
        match tolerate(c.read_number(cmd, Some(k)))? {
            Ok(v) => {
                out.insert(key.into(), json!(v));
            }
            Err(e) => note(&mut gaps, key, &e),
        }
    }
    match tolerate(c.read("get_ptt"))? {
        Ok(r) => {
            out.insert(
                "ptt".into(),
                json!(match r.kv("PTT") {
                    Some("0") => "receive",
                    Some(_) => "TRANSMIT",
                    None => "unknown",
                }),
            );
        }
        Err(e) => {
            out.insert(
                "ptt".into(),
                json!("unknown (this rig does not report PTT)"),
            );
            note(&mut gaps, "ptt", &e);
        }
    }
    if let Ok(r) = tolerate(c.read("get_dcd"))? {
        out.insert(
            "carrier_detect_or_squelch_open".into(),
            json!(r.kv("DCD").map(|s| s != "0")),
        );
    }
    if let Ok(r) = tolerate(c.read("get_powerstat"))? {
        out.insert(
            "power_on".into(),
            json!(r.kv("Power Status").map(|s| s == "1")),
        );
    }
    if let Ok(r) = tolerate(c.read("get_info"))? {
        out.insert(
            "info".into(),
            json!(r.kv("Info").unwrap_or_else(|| r.bare().unwrap_or(""))),
        );
    }
    let supported = listed(c, "get_level ?")?;
    let mut levels = Map::new();
    for name in STATUS_LEVELS {
        if supported
            .as_ref()
            .is_some_and(|s| !s.iter().any(|x| x == name))
        {
            continue;
        }
        match tolerate(c.read_number(&format!("get_level {name}"), None))? {
            Ok(v) => {
                levels.insert((*name).into(), level_json(name, v));
            }
            Err(e) if supported.is_none() && e.unsupported() => {}
            Err(e) => note(&mut gaps, &format!("level {name}"), &e),
        }
    }
    out.insert("levels".into(), Value::Object(levels));
    let supported = listed(c, "get_func ?")?;
    let mut funcs = Map::new();
    for name in STATUS_FUNCS {
        if supported
            .as_ref()
            .is_some_and(|s| !s.iter().any(|x| x == name))
        {
            continue;
        }
        match tolerate(c.read_number(&format!("get_func {name}"), None))? {
            Ok(v) => {
                funcs.insert((*name).into(), json!(v != 0.0));
            }
            Err(e) if supported.is_none() && e.unsupported() => {}
            Err(e) => note(&mut gaps, &format!("function {name}"), &e),
        }
    }
    out.insert("functions".into(), Value::Object(funcs));
    if let Ok(r) = tolerate(c.read("get_mem"))? {
        out.insert(
            "memory_channel".into(),
            json!(r.kv("Memory#").and_then(|s| s.parse::<i64>().ok())),
        );
    }
    out.insert("unavailable".into(), Value::Object(gaps));
    Ok(Value::Object(out))
}

/// `bench_hamlib_get`: one value, or several readings of the signal strength with their spread.
pub fn get(c: &mut Client, args: &Value) -> Result<Value, String> {
    // [@ANCHOR: hamlib_get]
    let item = args.get("item").and_then(|v| v.as_str()).ok_or("item is required (frequency, mode, vfo, split, rit, xit, ptt, squelch, power, info, memory, channel, level, function, signal)")?;
    let name = args.get("name").and_then(|v| v.as_str());
    let need_name = || {
        name.ok_or_else(|| {
            format!("item {item:?} needs name (a level or function such as AF, ATT, NB)")
        })
        .and_then(|n| check_arg(n).map(|n| n.to_ascii_uppercase()))
    };
    let one = |c: &mut Client, cmd: &str, key: Option<&str>| -> Result<f64, String> {
        c.read_number(cmd, key).map_err(|e| e.to_string())
    };
    match item {
        "frequency" => Ok(json!({"frequency_hz": one(c, "get_freq", Some("Frequency"))?})),
        "mode" => match mode_of(c)? {
            Ok((m, w)) => Ok(json!({"mode": m, "passband_hz": w})),
            Err(e) => Err(e.to_string()),
        },
        "vfo" => Ok(json!({"vfo": c.read("get_vfo").map_err(|e| e.to_string())?.kv("VFO")})),
        "split" => {
            let r = c.read("get_split_vfo").map_err(|e| e.to_string())?;
            Ok(json!({"enabled": r.kv("Split").map(|s| s != "0"), "tx_vfo": r.kv("TX VFO")}))
        }
        "rit" => Ok(json!({"rit_hz": one(c, "get_rit", Some("RIT"))?})),
        "xit" => Ok(json!({"xit_hz": one(c, "get_xit", Some("XIT"))?})),
        "ptt" => {
            let r = c.read("get_ptt").map_err(|e| e.to_string())?;
            Ok(
                json!({"ptt": match r.kv("PTT") { Some("0") => "receive", Some(_) => "TRANSMIT", None => "unknown" }}),
            )
        }
        "squelch" => {
            Ok(json!({"carrier_detect_or_squelch_open": one(c, "get_dcd", Some("DCD"))? != 0.0}))
        }
        "power" => Ok(json!({"power_on": one(c, "get_powerstat", Some("Power Status"))? == 1.0})),
        "info" => {
            let r = c.read("get_info").map_err(|e| e.to_string())?;
            Ok(json!({"info": r.kv("Info").unwrap_or_else(|| r.bare().unwrap_or(""))}))
        }
        "memory" => Ok(json!({"memory_channel": one(c, "get_mem", Some("Memory#"))?})),
        "channel" => {
            let n = args
                .get("channel")
                .and_then(|v| v.as_u64())
                .filter(|n| *n < 100_000)
                .ok_or("item channel needs channel (a whole number)")?;
            let r = c
                .read(&format!("get_channel {n} 0"))
                .map_err(|e| e.to_string())?;
            Ok(json!({"channel": n, "as_reported": r.lines}))
        }
        "level" => {
            let n = need_name()?;
            let v = one(c, &format!("get_level {n}"), None)?;
            Ok(json!({"level": n, "value": level_json(&n, v)}))
        }
        "function" => {
            let n = need_name()?;
            Ok(json!({"function": n, "on": one(c, &format!("get_func {n}"), None)? != 0.0}))
        }
        "signal" => signal(c, args),
        other => Err(format!("unknown item {other:?}")),
    }
}

/// Several readings of the signal strength, spaced by `interval_ms`: mean, spread, and the S-unit text of the mean.
fn signal(c: &mut Client, args: &Value) -> Result<Value, String> {
    // [@ANCHOR: hamlib_signal_readings]
    let count = bounded(args, "count", 1.0, 1.0, 50.0)? as usize;
    let gap = bounded(args, "interval_ms", 100.0, 0.0, 2000.0)? as u64;
    let mut v = Vec::new();
    for i in 0..count {
        if i > 0 {
            std::thread::sleep(std::time::Duration::from_millis(gap));
        }
        v.push(
            c.read_number("get_level STRENGTH", None)
                .map_err(|e| e.to_string())?,
        );
    }
    let mean = v.iter().sum::<f64>() / v.len() as f64;
    let (lo, hi) = (
        v.iter().cloned().fold(f64::INFINITY, f64::min),
        v.iter().cloned().fold(f64::NEG_INFINITY, f64::max),
    );
    Ok(
        json!({"readings": v, "mean_db_relative_to_s9": mean, "spread_db": hi - lo, "s_units": s_units(mean), "text": s_text(mean)}),
    )
}

pub fn bounded(args: &Value, key: &str, default: f64, lo: f64, hi: f64) -> Result<f64, String> {
    let v = match args.get(key) {
        None | Some(Value::Null) => default,
        Some(x) => x
            .as_f64()
            .ok_or_else(|| format!("{key} must be a number"))?,
    };
    if v.is_finite() && (lo..=hi).contains(&v) {
        Ok(v)
    } else {
        Err(format!("{key} {v} is outside {lo} to {hi}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Tests [@ANCHOR: hamlib_s_units]
    #[test]
    fn s_units_follow_six_db_per_unit_below_s9() {
        assert_eq!(s_units(0.0), 9.0);
        assert_eq!(s_units(-6.0), 8.0);
        assert_eq!(s_units(-54.0), 0.0);
        assert_eq!(s_units(-100.0), 0.0);
        assert_eq!(s_units(30.0), 9.0);
        assert_eq!(s_text(-12.0), "S7");
        assert_eq!(s_text(20.0), "S9+20 dB");
    }

    #[test]
    fn bounded_refuses_what_is_out_of_range_or_not_a_number() {
        assert_eq!(bounded(&json!({}), "count", 3.0, 1.0, 5.0).unwrap(), 3.0);
        assert!(bounded(&json!({"count": 9}), "count", 3.0, 1.0, 5.0).is_err());
        assert!(bounded(&json!({"count": "x"}), "count", 3.0, 1.0, 5.0).is_err());
    }
}
