// SPDX-License-Identifier: AGPL-3.0-or-later
//! Receiver control (tier `read_config`): frequency, mode, VFO, split, RIT/XIT, levels, functions, memory recall and a bounded scan.
//! Nothing here keys the transmitter. Transmit-side levels (RF power, microphone gain) and functions that can radiate (the antenna
//! tuner, VOX) are refused here; only the transmit tools, with the owner's attestation, touch them.
//!
//! Every argument is checked before the first byte is sent. Each change is read back and the read-back is returned.

use super::proto::{check_arg, Client, RigError};
use super::status::{bounded, tolerate};
use super::Config;
use serde_json::{json, Value};
use std::time::{Duration, Instant};

/// Hamlib's mode names (the `RIG_MODE_*` set, without the prefix). A name the rig lacks is refused by the rig and read back.
pub const MODES: &[&str] = &[
    "AM", "CW", "USB", "LSB", "RTTY", "FM", "WFM", "CWR", "RTTYR", "AMS", "PKTLSB", "PKTUSB",
    "PKTFM", "ECSSUSB", "ECSSLSB", "FAX", "SAM", "SAL", "SAH", "DSB", "FMN", "PKTAM", "P25",
    "DSTAR", "DPMR", "NXDNVN", "NXDN_N", "DCR", "AMN", "PSK", "PSKR", "DD", "C4FM", "PKTFMN",
    "SPEC", "CWN", "IQ",
];
const VFOS: &[&str] = &[
    "VFOA", "VFOB", "VFOC", "currVFO", "MEM", "Main", "Sub", "MainA", "MainB", "SubA", "SubB",
];
/// Levels the control tool may set, with their allowed range (low, high). Fractions are 0 to 1 as Hamlib uses them.
const SETTABLE_LEVELS: &[(&str, f64, f64)] = &[
    ("AF", 0.0, 1.0),
    ("RF", 0.0, 1.0),
    ("SQL", 0.0, 1.0),
    ("IF", -10000.0, 10000.0),
    ("APF", 0.0, 1.0),
    ("NR", 0.0, 1.0),
    ("NB", 0.0, 1.0),
    ("PBT_IN", -10000.0, 10000.0),
    ("PBT_OUT", -10000.0, 10000.0),
    ("CWPITCH", 300.0, 1000.0),
    ("NOTCHF", 0.0, 10000.0),
    ("BAL", 0.0, 1.0),
    ("AGC", 0.0, 6.0),
    ("ATT", 0.0, 60.0),
    ("PREAMP", 0.0, 30.0),
];
const SETTABLE_FUNCS: &[&str] = &[
    "NB", "NB2", "NR", "ANF", "APF", "AIP", "MN", "LOCK", "MUTE", "SQL", "AFLT", "ANL", "FAGC",
];
/// Refused here whatever the rig offers: they set what the transmitter does.
const TRANSMIT_SIDE: &[&str] = &[
    "RFPOWER",
    "MICGAIN",
    "KEYSPD",
    "COMP",
    "VOXGAIN",
    "VOXDELAY",
    "ANTIVOX",
    "TUNER",
    "VOX",
    "TONE",
    "TSQL",
    "MON",
    "SBKIN",
    "FBKIN",
    "BKIN_DLYMS",
    "SEND_MORSE",
    "SEND_VOICE_MEM",
];

/// The receive ranges in force: the rig's own (from `dump_state`) narrowed by the owner's `rx_ranges_hz`. The text says where the limit came from.
pub fn rx_ranges(c: &mut Client, cfg: &Config) -> Result<(Vec<(f64, f64)>, String), String> {
    // [@ANCHOR: hamlib_rx_ranges]
    let rig = match tolerate(c.read("dump_state"))? {
        Ok(r) => parse_dump_ranges(&r.lines).0,
        Err(_) => vec![],
    };
    Ok(combine_ranges(&rig, &cfg.rx_ranges_hz))
}

/// A transmit range as `dump_state` gives it: (low Hz, high Hz, lowest power mW, highest power mW).
pub type TxRange = (f64, f64, f64, f64);

/// (receive ranges, transmit ranges) from the lines of `dump_state`.
pub fn parse_dump_ranges(lines: &[String]) -> (Vec<(f64, f64)>, Vec<TxRange>) {
    // [@ANCHOR: hamlib_dump_ranges]
    let (mut rx, mut tx) = (vec![], vec![]);
    let mut list = 0;
    for l in lines.iter().skip(3) {
        let f: Vec<&str> = l.split_whitespace().collect();
        if f.len() != 7 {
            break;
        }
        if f.iter().all(|t| *t == "0") {
            list += 1;
            if list == 2 {
                break;
            }
            continue;
        }
        let num = |i: usize| f[i].parse::<f64>().ok();
        let (Some(lo), Some(hi)) = (num(0), num(1)) else {
            break;
        };
        if list == 0 {
            rx.push((lo, hi));
        } else {
            tx.push((lo, hi, num(3).unwrap_or(-1.0), num(4).unwrap_or(-1.0)));
        }
    }
    (rx, tx)
}

/// The owner's limits narrow the rig's; with neither there is no range check, and the text says so.
pub fn combine_ranges(rig: &[(f64, f64)], owner: &[(f64, f64)]) -> (Vec<(f64, f64)>, String) {
    // [@ANCHOR: hamlib_combine_ranges]
    match (rig.is_empty(), owner.is_empty()) {
        (true, true) => (vec![], "the rig reports no receive range and the config sets none: frequencies are not range-checked".into()),
        (false, true) => (rig.to_vec(), "the rig's reported receive range".into()),
        (true, false) => (owner.to_vec(), "rx_ranges_hz in the bench config".into()),
        (false, false) => {
            let mut out = vec![];
            for r in rig {
                for o in owner {
                    let (lo, hi) = (r.0.max(o.0), r.1.min(o.1));
                    if lo < hi {
                        out.push((lo, hi));
                    }
                }
            }
            (out, "the rig's reported receive range narrowed by rx_ranges_hz in the bench config".into())
        }
    }
}

/// Refuses a frequency outside the ranges in force, naming the limit and its source. No ranges: accepted.
pub fn check_freq(f: f64, ranges: &[(f64, f64)], source: &str) -> Result<(), String> {
    // [@ANCHOR: hamlib_freq_guard]
    if !f.is_finite() || f <= 0.0 {
        return Err(format!("frequency {f} Hz is not a positive finite number"));
    }
    if ranges.is_empty() || ranges.iter().any(|(lo, hi)| f >= *lo && f <= *hi) {
        return Ok(());
    }
    let list: Vec<String> = ranges
        .iter()
        .map(|(lo, hi)| format!("{lo:.0} to {hi:.0} Hz"))
        .collect();
    Err(format!(
        "frequency {f:.0} Hz is outside the receive range ({}); source: {source}",
        list.join(", ")
    ))
}

fn ptt_is_on(c: &mut Client) -> Result<bool, String> {
    // [@ANCHOR: hamlib_ptt_is_on]
    match tolerate(c.read("get_ptt"))? {
        Ok(r) => Ok(r.kv("PTT").is_some_and(|v| v != "0")),
        Err(_) => Ok(false),
    }
}

/// Everything `bench_hamlib_set` will do, checked.
struct Plan {
    freq: Option<f64>,
    mode: Option<(String, f64)>,
    vfo: Option<String>,
    split: Option<(bool, Option<String>)>,
    rit: Option<f64>,
    xit: Option<f64>,
    levels: Vec<(String, f64)>,
    funcs: Vec<(String, bool)>,
}

fn plan(args: &Value) -> Result<Plan, String> {
    // [@ANCHOR: hamlib_set_plan]
    let num = |k: &str, lo: f64, hi: f64| -> Result<Option<f64>, String> {
        match args.get(k) {
            None | Some(Value::Null) => Ok(None),
            Some(_) => bounded(args, k, 0.0, lo, hi).map(Some),
        }
    };
    let freq = num("frequency_hz", 1.0, 1.0e12)?;
    let width = num("passband_hz", 0.0, 250_000.0)?;
    let mode = match args.get("mode").and_then(|m| m.as_str()) {
        None => {
            if width.is_some() {
                return Err(
                    "passband_hz needs mode in the same call (rigctld sets them together)".into(),
                );
            }
            None
        }
        Some(m) => {
            let m = m.to_ascii_uppercase();
            if !MODES.contains(&m.as_str()) {
                return Err(format!(
                    "mode {m:?} is not a Hamlib mode name ({})",
                    MODES.join(" ")
                ));
            }
            Some((m, width.unwrap_or(0.0)))
        }
    };
    let vfo = match args.get("vfo") {
        None | Some(Value::Null) => None,
        Some(v) => {
            let v = v.as_str().ok_or("vfo must be text")?;
            Some(
                VFOS.iter()
                    .find(|x| x.eq_ignore_ascii_case(v))
                    .ok_or_else(|| format!("vfo {v:?} is not one of {}", VFOS.join(" ")))?
                    .to_string(),
            )
        }
    };
    let split = match args.get("split") {
        None | Some(Value::Null) => None,
        Some(s) => {
            let on = s
                .get("enabled")
                .and_then(|e| e.as_bool())
                .ok_or("split needs enabled true or false")?;
            let tx = s
                .get("tx_vfo")
                .and_then(|v| v.as_str())
                .map(|v| {
                    VFOS.iter()
                        .find(|x| x.eq_ignore_ascii_case(v))
                        .map(|x| x.to_string())
                        .ok_or_else(|| format!("tx_vfo {v:?} is not one of {}", VFOS.join(" ")))
                })
                .transpose()?;
            Some((on, tx))
        }
    };
    let mut levels = vec![];
    if let Some(o) = args.get("levels") {
        for (k, v) in o
            .as_object()
            .ok_or("levels must be an object of NAME: number")?
        {
            let k = k.to_ascii_uppercase();
            if TRANSMIT_SIDE.contains(&k.as_str()) {
                return Err(format!("{k} sets what the transmitter does; the receiver-control tool refuses it (only the transmit tools, for an attested rig, change transmit settings)"));
            }
            let &(_, lo, hi) = SETTABLE_LEVELS
                .iter()
                .find(|(n, _, _)| *n == k)
                .ok_or_else(|| {
                    format!(
                        "level {k} is not settable here ({})",
                        SETTABLE_LEVELS
                            .iter()
                            .map(|l| l.0)
                            .collect::<Vec<_>>()
                            .join(" ")
                    )
                })?;
            let x = v
                .as_f64()
                .filter(|x| x.is_finite() && (lo..=hi).contains(x))
                .ok_or_else(|| format!("level {k} must be a number from {lo} to {hi}"))?;
            levels.push((k, x));
        }
    }
    let mut funcs = vec![];
    if let Some(o) = args.get("functions") {
        for (k, v) in o
            .as_object()
            .ok_or("functions must be an object of NAME: true or false")?
        {
            let k = k.to_ascii_uppercase();
            if TRANSMIT_SIDE.contains(&k.as_str()) {
                return Err(format!(
                    "function {k} can make the rig transmit; the receiver-control tool refuses it"
                ));
            }
            if !SETTABLE_FUNCS.contains(&k.as_str()) {
                return Err(format!(
                    "function {k} is not settable here ({})",
                    SETTABLE_FUNCS.join(" ")
                ));
            }
            let on = v
                .as_bool()
                .ok_or_else(|| format!("function {k} must be true or false"))?;
            funcs.push((k, on));
        }
    }
    let rit = num("rit_hz", -9999.0, 9999.0)?;
    let xit = num("xit_hz", -9999.0, 9999.0)?;
    let p = Plan {
        freq,
        mode,
        vfo,
        split,
        rit,
        xit,
        levels,
        funcs,
    };
    if p.freq.is_none()
        && p.mode.is_none()
        && p.vfo.is_none()
        && p.split.is_none()
        && p.rit.is_none()
        && p.xit.is_none()
        && p.levels.is_empty()
        && p.funcs.is_empty()
    {
        return Err("nothing to set: give frequency_hz, mode, vfo, split, rit_hz, xit_hz, levels or functions".into());
    }
    Ok(p)
}

fn apply<T>(done: &[Value], what: &str, r: Result<T, RigError>) -> Result<(), String> {
    // [@ANCHOR: hamlib_apply]
    match r {
        Ok(_) => Ok(()),
        Err(e) => {
            let applied: Vec<String> = done
                .iter()
                .map(|d| d["set"].as_str().unwrap_or("").to_string())
                .collect();
            Err(format!(
                "{what} failed: {e} (already applied before it: {})",
                if applied.is_empty() {
                    "nothing".into()
                } else {
                    applied.join(", ")
                }
            ))
        }
    }
}

/// `bench_hamlib_set`.
pub fn set(c: &mut Client, cfg: &Config, args: &Value) -> Result<Value, String> {
    // [@ANCHOR: hamlib_set]
    let p = plan(args)?;
    if let Some(f) = p.freq {
        let (ranges, source) = rx_ranges(c, cfg)?;
        check_freq(f, &ranges, &source)?;
    }
    if ptt_is_on(c)? {
        return Err("the rig reports PTT on (transmitting). Nothing was changed. Next: find out who keyed it; bench_hamlib_safe_state releases it where the transmit tools exist, otherwise switch it off at the rig.".into());
    }
    let mut done: Vec<Value> = vec![];
    if let Some(v) = &p.vfo {
        apply(
            &done,
            "set vfo",
            c.command(&format!("set_vfo {}", check_arg(v)?)),
        )?;
        done.push(json!({"set": "vfo", "value": v}));
    }
    if let Some(f) = p.freq {
        apply(
            &done,
            "set frequency",
            c.command(&format!("set_freq {f:.0}")),
        )?;
        let back = c
            .read_number("get_freq", Some("Frequency"))
            .map_err(|e| e.to_string())?;
        done.push(json!({"set": "frequency_hz", "requested": f, "read_back": back}));
    }
    if let Some((m, w)) = &p.mode {
        apply(
            &done,
            "set mode",
            c.command(&format!("set_mode {m} {w:.0}")),
        )?;
        let back = c.read("get_mode").map_err(|e| e.to_string())?;
        let mode_back = back.kv("Mode").unwrap_or("").to_string();
        let mut d = json!({"set": "mode", "requested": m, "read_back": mode_back, "passband_hz": back.kv("Passband")});
        if !mode_back.eq_ignore_ascii_case(m) {
            d["warning"] = json!(
                "the rig reports a different mode than requested: it may not offer that mode"
            );
        }
        done.push(d);
    }
    if let Some((on, tx)) = &p.split {
        let tx_vfo = tx.clone().unwrap_or_else(|| "VFOB".into());
        apply(
            &done,
            "set split",
            c.command(&format!(
                "set_split_vfo {} {}",
                u8::from(*on),
                check_arg(&tx_vfo)?
            )),
        )?;
        let back = c.read("get_split_vfo").map_err(|e| e.to_string())?;
        done.push(json!({"set": "split", "requested": on, "read_back": back.kv("Split").map(|s| s != "0"), "tx_vfo": back.kv("TX VFO")}));
    }
    for (name, cmd, get, key, v) in [
        ("rit_hz", "set_rit", "get_rit", "RIT", p.rit),
        ("xit_hz", "set_xit", "get_xit", "XIT", p.xit),
    ] {
        if let Some(v) = v {
            apply(&done, name, c.command(&format!("{cmd} {v:.0}")))?;
            let back = c.read_number(get, Some(key)).map_err(|e| e.to_string())?;
            done.push(json!({"set": name, "requested": v, "read_back": back}));
        }
    }
    for (name, v) in &p.levels {
        apply(
            &done,
            &format!("set level {name}"),
            c.command(&format!("set_level {} {v}", check_arg(name)?)),
        )?;
        let back = c
            .read_number(&format!("get_level {name}"), None)
            .map_err(|e| e.to_string())?;
        done.push(json!({"set": format!("level {name}"), "requested": v, "read_back": back}));
    }
    for (name, on) in &p.funcs {
        apply(
            &done,
            &format!("set function {name}"),
            c.command(&format!("set_func {} {}", check_arg(name)?, u8::from(*on))),
        )?;
        let back = c
            .read_number(&format!("get_func {name}"), None)
            .map_err(|e| e.to_string())?
            != 0.0;
        done.push(json!({"set": format!("function {name}"), "requested": on, "read_back": back}));
    }
    Ok(json!({"changes": done}))
}

/// `bench_hamlib_memory`: selects a memory channel and reports what the rig then shows.
pub fn memory(c: &mut Client, cfg: &Config, args: &Value) -> Result<Value, String> {
    // [@ANCHOR: hamlib_memory]
    let n = bounded(args, "channel", -1.0, 0.0, 99_999.0)
        .map_err(|_| "channel is required: a whole number from 0 to 99999".to_string())?;
    if n.fract() != 0.0 {
        return Err("channel must be a whole number".into());
    }
    if ptt_is_on(c)? {
        return Err("the rig reports PTT on (transmitting); no channel was selected".into());
    }
    // Some backends do not accept a switch to the memory VFO and recall by channel number alone; that is noted, not fatal.
    let vfo_note = match tolerate(c.command("set_vfo MEM"))? {
        Ok(_) => None,
        Err(e) => Some(format!(
            "the rig did not switch to the memory VFO ({e}); the channel was selected by number"
        )),
    };
    c.command(&format!("set_mem {n:.0}"))
        .map_err(|e| format!("select channel {n:.0}: {e}"))?;
    let freq = c
        .read_number("get_freq", Some("Frequency"))
        .map_err(|e| e.to_string())?;
    let mode = tolerate(c.read("get_mode"))?
        .ok()
        .map(|r| json!({"mode": r.kv("Mode"), "passband_hz": r.kv("Passband")}));
    let (ranges, source) = rx_ranges(c, cfg)?;
    let mut out = json!({"channel": n, "frequency_hz": freq, "mode": mode, "note": vfo_note});
    if let Err(w) = check_freq(freq, &ranges, &source) {
        out["warning"] = json!(format!(
            "the channel holds a frequency outside the receive limits: {w}"
        ));
    }
    Ok(out)
}

const SCAN_MAX_STEPS: f64 = 400.0;
const SCAN_MAX_S: f64 = 120.0;

/// `bench_hamlib_scan`: steps the receiver across a range, dwelling at each frequency, and lists signal strength and squelch state.
/// The frequency and mode the rig had are restored on every path.
pub fn scan(c: &mut Client, cfg: &Config, args: &Value) -> Result<Value, String> {
    // [@ANCHOR: hamlib_scan]
    let start = bounded(args, "start_hz", f64::NAN, 1.0, 1.0e12)
        .map_err(|_| "start_hz is required (Hz)".to_string())?;
    let stop = bounded(args, "stop_hz", f64::NAN, 1.0, 1.0e12)
        .map_err(|_| "stop_hz is required (Hz)".to_string())?;
    let step = bounded(args, "step_hz", f64::NAN, 1.0, 1.0e9)
        .map_err(|_| "step_hz is required (Hz)".to_string())?;
    let dwell = bounded(args, "dwell_ms", 200.0, 20.0, 5000.0)?;
    let top = bounded(args, "top", 10.0, 1.0, 100.0)? as usize;
    let stop_above = match args.get("stop_when_above_db") {
        None | Some(Value::Null) => None,
        Some(_) => Some(bounded(args, "stop_when_above_db", 0.0, -120.0, 60.0)?),
    };
    if stop < start {
        return Err("stop_hz is below start_hz".into());
    }
    let steps = ((stop - start) / step).floor() + 1.0;
    if steps > SCAN_MAX_STEPS {
        return Err(format!("{steps:.0} steps is more than the {SCAN_MAX_STEPS:.0} allowed in one scan; raise step_hz or split the range"));
    }
    let planned_s = steps * (dwell + 60.0) / 1000.0;
    if planned_s > SCAN_MAX_S {
        return Err(format!("{steps:.0} steps of {dwell:.0} ms would take about {planned_s:.0} s; a scan is limited to {SCAN_MAX_S:.0} s. Lower dwell_ms or the number of steps."));
    }
    let mode = match args.get("mode").and_then(|m| m.as_str()) {
        None => None,
        Some(m) => {
            let m = m.to_ascii_uppercase();
            MODES
                .contains(&m.as_str())
                .then_some(m.clone())
                .ok_or_else(|| format!("mode {m:?} is not a Hamlib mode name"))?
                .into()
        }
    };
    let (ranges, source) = rx_ranges(c, cfg)?;
    check_freq(start, &ranges, &source)?;
    check_freq(stop, &ranges, &source)?;
    if ptt_is_on(c)? {
        return Err("the rig reports PTT on (transmitting); not scanning".into());
    }
    let had_freq = c
        .read_number("get_freq", Some("Frequency"))
        .map_err(|e| e.to_string())?;
    let had_mode = tolerate(c.read("get_mode"))?
        .ok()
        .and_then(|r| Some((r.kv("Mode")?.to_string(), r.number(Some("Passband")).ok()?)));
    let has_dcd = tolerate(c.read("get_dcd"))?.is_ok();
    let result = scan_steps(
        c,
        start,
        step,
        steps as usize,
        dwell,
        mode.as_deref(),
        has_dcd,
        stop_above,
    );
    // Back to where the rig was, whatever happened.
    let restored_freq = c.command(&format!("set_freq {had_freq:.0}")).is_ok();
    let restored_mode = match &had_mode {
        Some((m, w)) => c.command(&format!("set_mode {m} {w:.0}")).is_ok(),
        None => true,
    };
    let (points, stopped_early) = result?;
    let mut ranked: Vec<&Value> = points.iter().collect();
    ranked.sort_by(|a, b| {
        b["strength_db_rel_s9"]
            .as_f64()
            .partial_cmp(&a["strength_db_rel_s9"].as_f64())
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    Ok(json!({
        "steps": points.len(), "stopped_early": stopped_early, "points": points,
        "strongest": ranked.into_iter().take(top).collect::<Vec<_>>(),
        "restored": {"frequency": restored_freq, "mode": restored_mode, "frequency_hz": had_freq},
    }))
}

#[allow(clippy::too_many_arguments)]
fn scan_steps(
    c: &mut Client,
    start: f64,
    step: f64,
    steps: usize,
    dwell_ms: f64,
    mode: Option<&str>,
    has_dcd: bool,
    stop_above: Option<f64>,
) -> Result<(Vec<Value>, bool), String> {
    // [@ANCHOR: hamlib_scan_steps]
    let began = Instant::now();
    if let Some(m) = mode {
        c.command(&format!("set_mode {m} 0"))
            .map_err(|e| format!("set mode {m}: {e}"))?;
    }
    let mut points = Vec::new();
    for i in 0..steps {
        if began.elapsed() > Duration::from_secs_f64(SCAN_MAX_S) {
            return Ok((points, true));
        }
        let f = start + step * i as f64;
        c.command(&format!("set_freq {f:.0}"))
            .map_err(|e| format!("set {f:.0} Hz: {e}"))?;
        std::thread::sleep(Duration::from_secs_f64(dwell_ms / 1000.0));
        let s = c
            .read_number("get_level STRENGTH", None)
            .map_err(|e| format!("signal strength at {f:.0} Hz: {e}"))?;
        let mut p = json!({"frequency_hz": f, "strength_db_rel_s9": s, "s_units": super::status::s_units(s)});
        if has_dcd {
            p["squelch_open"] = json!(
                c.read_number("get_dcd", Some("DCD"))
                    .map_err(|e| e.to_string())?
                    != 0.0
            );
        }
        points.push(p);
        if stop_above.is_some_and(|t| s > t) {
            return Ok((points, true));
        }
    }
    Ok((points, false))
}

#[cfg(test)]
mod tests {
    use super::*;

    // Tests [@ANCHOR: hamlib_dump_ranges]
    #[test]
    fn dump_state_ranges_are_read_until_the_zero_lines() {
        // The first three lines of the real dump are single numbers (protocol, model, region), then the receive list, then the transmit list.
        let lines: Vec<String> = [
            "1",
            "1",
            "0",
            "150000.000000 1500000000.000000 0x1ff -1 -1 0x17e00007 0xf",
            "0 0 0 0 0 0 0",
            "150000.000000 1500000000.000000 0x1ff 5000 100000 0x17e00007 0xf",
            "0 0 0 0 0 0 0",
            "0x1ff 1",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let (rx, tx) = parse_dump_ranges(&lines);
        assert_eq!(rx, [(150_000.0, 1_500_000_000.0)]);
        assert_eq!(tx, [(150_000.0, 1_500_000_000.0, 5000.0, 100_000.0)]);
    }
}
