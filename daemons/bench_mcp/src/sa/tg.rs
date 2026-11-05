// SPDX-License-Identifier: AGPL-3.0-or-later
//! The tracking generator of the Siglent SSA3000X driver: on, off, level, the stored through reference and the scalar S21 sweep.

use super::sweep::{
    bin_hz, find_peaks, run_sweep, sweep_args, trace_summary, write_csv, SweepReq, Trace,
};
use super::{
    ask, ask_f64, flag, number, on_off, open, options, Connector, OFF_TRIES, READ_TRIES,
    REPLY_TIMEOUT_S, TG_RATED_MIN_DBM,
};
use crate::config::Instrument;
use crate::link::Link;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

/// Tolerance when a level read back is compared with the one set, dB.
const LEVEL_TOLERANCE_DB: f64 = 0.05;

/// The generator's state: (output on, level in dBm), as the unit reports it.
pub fn tg_state(l: &mut Link) -> Result<(bool, f64), String> {
    // [@ANCHOR: bench_mcp_sa_tg_state]
    let on = on_off(
        ":OUTPut:STATe?",
        &ask(l, ":OUTPut:STATe?", READ_TRIES, REPLY_TIMEOUT_S)?,
    )?;
    Ok((on, ask_f64(l, ":SOURce:POWer?")?))
}

/// Switches the generator off and reads it back, up to `OFF_TRIES` times. Ok only when the unit itself says off.
pub fn ensure_tg_off(l: &mut Link) -> Result<(), String> {
    // [@ANCHOR: bench_mcp_sa_ensure_tg_off]
    let mut last = String::new();
    for _ in 0..OFF_TRIES {
        l.set_timeout_secs(REPLY_TIMEOUT_S);
        if let Err(e) = l.send(":OUTPut:STATe OFF") {
            last = e;
            continue;
        }
        match l
            .query(":OUTPut:STATe?")
            .and_then(|r| on_off(":OUTPut:STATe?", &r))
        {
            Ok(false) => return Ok(()),
            Ok(true) => last = "it still reports on".into(),
            Err(e) => last = e,
        }
    }
    Err(format!(
        "SAFE STATE NOT CONFIRMED: the tracking generator may still be ON ({last}). Do this now: press TG then TG off on the analyzer's front panel, or disconnect what is on the TG OUTPUT. Do not rely on the analyzer until it reports off."
    ))
}

/// Combines the result of the work with the result of the TG switch-off that followed it (`None`: no switch-off was asked
/// for). A failed switch-off is always the error returned (with the work's own error if both failed).
pub fn finish<T>(work: Result<T, String>, off: Option<Result<(), String>>) -> Result<T, String> {
    // [@ANCHOR: bench_mcp_sa_finish]
    match (work, off) {
        (w, None) | (w, Some(Ok(()))) => w,
        (Ok(_), Some(Err(e))) => Err(e),
        (Err(w), Some(Err(e))) => Err(format!("{w}; {e}")),
    }
}

/// The level argument `tg_level_dbm` checked against the unit's rating and the config's cap; `None` when absent.
fn level_arg(inst: &Instrument, args: &Value) -> Result<Option<f64>, String> {
    // [@ANCHOR: bench_mcp_sa_level_arg]
    let hi = inst.sa.max_tg_dbm;
    number(
        args,
        "tg_level_dbm",
        TG_RATED_MIN_DBM,
        hi,
        "the generator's range -20 to 0 dBm, and the config's max_tg_dbm",
    )
}

fn require_tg(l: &mut Link) -> Result<(), String> {
    // [@ANCHOR: bench_mcp_sa_require_tg]
    if options(l)?.iter().any(|o| o.eq_ignore_ascii_case("TG")) {
        Ok(())
    } else {
        Err(
            "this analyzer does not list the TG (tracking generator) option in :SYSTem:OPTions?"
                .into(),
        )
    }
}

/// Sets the level, reads it back, and fails if it differs; the output state is not touched.
fn set_level(l: &mut Link, dbm: f64) -> Result<(), String> {
    // [@ANCHOR: bench_mcp_sa_set_level]
    l.send(&format!(":SOURce:POWer {dbm}"))?;
    let got = ask_f64(l, ":SOURce:POWer?")?;
    if (got - dbm).abs() > LEVEL_TOLERANCE_DB {
        return Err(format!(
            "the generator level reads back {got} dBm, not the {dbm} dBm set"
        ));
    }
    Ok(())
}

/// `bench_sa_tg_on`: level (if given) set and verified, then the output switched on and read back.
pub fn tg_on(connect: &Connector, inst: &Instrument, args: &Value) -> Result<Value, String> {
    // [@ANCHOR: bench_mcp_sa_tg_on]
    let level = level_arg(inst, args)?;
    let (mut l, _) = open(connect, inst)?;
    require_tg(&mut l)?;
    switch_on(&mut l, inst, level)
}

/// Sets the level (or checks the existing one against the cap), switches the output on and verifies it; on any failure the
/// output is switched off again.
fn switch_on(l: &mut Link, inst: &Instrument, level: Option<f64>) -> Result<Value, String> {
    // [@ANCHOR: bench_mcp_sa_switch_on]
    let r = (|| {
        match level {
            Some(x) => set_level(l, x)?,
            None => {
                let (_, cur) = tg_state(l)?;
                if cur > inst.sa.max_tg_dbm {
                    return Err(format!(
                        "the generator is set to {cur} dBm, above max_tg_dbm {}; give tg_level_dbm",
                        inst.sa.max_tg_dbm
                    ));
                }
            }
        }
        l.send(":OUTPut:STATe ON")?;
        let (on, lvl) = tg_state(l)?;
        if on {
            Ok(lvl)
        } else {
            Err("the generator did not report on after :OUTPut:STATe ON".to_string())
        }
    })();
    match r {
        Ok(lvl) => Ok(json!({"tracking_generator": "on", "level_dbm": lvl})),
        Err(e) => finish(Err(e), Some(ensure_tg_off(l))),
    }
}

/// `bench_sa_tg_off`: switches the output off and proves it.
pub fn tg_off(connect: &Connector, inst: &Instrument) -> Result<Value, String> {
    // [@ANCHOR: bench_mcp_sa_tg_off]
    let (mut l, _) = open(connect, inst)?;
    ensure_tg_off(&mut l)?;
    let (_, lvl) = tg_state(&mut l)?;
    Ok(json!({"tracking_generator": "off", "verified": true, "level_dbm": lvl}))
}

/// `bench_sa_tg_level`: sets the level without changing the on/off state and returns both as read back.
pub fn tg_level(connect: &Connector, inst: &Instrument, args: &Value) -> Result<Value, String> {
    // [@ANCHOR: bench_mcp_sa_tg_level]
    let level = level_arg(inst, args)?.ok_or("tg_level_dbm is required")?;
    let (mut l, _) = open(connect, inst)?;
    require_tg(&mut l)?;
    set_level(&mut l, level)?;
    let (on, lvl) = tg_state(&mut l)?;
    Ok(json!({"tracking_generator": if on {"on"} else {"off"}, "level_dbm": lvl}))
}

// [@ANCHOR: bench_mcp_sa_reference]
/// A through reference: the trace taken with the TG cabled to the input, and the settings it was taken with.
#[derive(Clone, Debug, PartialEq)]
pub struct Reference {
    pub tg_level_dbm: f64,
    pub trace: Trace,
}

static REFS: OnceLock<Mutex<HashMap<String, Reference>>> = OnceLock::new();

fn refs() -> &'static Mutex<HashMap<String, Reference>> {
    // [@ANCHOR: bench_mcp_sa_refs]
    REFS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// A reference name: 1 to 40 of letters, digits, `-` and `_`.
fn ref_name(v: Option<&str>) -> Result<String, String> {
    // [@ANCHOR: bench_mcp_sa_ref_name]
    let n = v.unwrap_or("default");
    if n.is_empty()
        || n.len() > 40
        || !n
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err("name must be 1 to 40 letters, digits, - or _".into());
    }
    Ok(n.to_string())
}

/// The `# key=value` header lines of a reference CSV: the tool, the TG level and every setting `check_same` compares.
fn comments(r: &Reference) -> Vec<(String, String)> {
    // [@ANCHOR: bench_mcp_sa_reference_comments]
    let t = &r.trace;
    [
        ("tool", "bench_sa_tg_normalize".to_string()),
        ("tg_level_dbm", r.tg_level_dbm.to_string()),
        ("start_hz", t.start_hz.to_string()),
        ("stop_hz", t.stop_hz.to_string()),
        ("rbw_hz", t.rbw_hz.to_string()),
        ("vbw_hz", t.vbw_hz.to_string()),
        ("attenuation_db", t.attenuation_db.to_string()),
        ("detector", t.detector.clone()),
        ("average_count", t.average.to_string()),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v))
    .collect()
}

/// Reads a reference CSV written by `bench_sa_tg_normalize`: the `# key=value` lines carry the settings, the data lines
/// `freq_hz,level_dbm`. Anything missing or not a finite number is an error.
pub fn parse_reference(text: &str) -> Result<Reference, String> {
    // [@ANCHOR: bench_mcp_sa_parse_reference]
    let mut kv: HashMap<&str, &str> = HashMap::new();
    let mut dbm = Vec::new();
    for line in text.lines() {
        if let Some(c) = line.strip_prefix("# ") {
            if let Some((k, v)) = c.split_once('=') {
                kv.insert(k, v);
            }
        } else if !line.starts_with("freq_hz") && !line.trim().is_empty() {
            let f = line
                .split(',')
                .nth(1)
                .and_then(|x| x.trim().parse::<f64>().ok());
            dbm.push(
                f.filter(|x| x.is_finite())
                    .ok_or_else(|| format!("bad reference row {line:?}"))?,
            );
        }
    }
    let num = |k: &str| -> Result<f64, String> {
        kv.get(k)
            .and_then(|v| v.parse::<f64>().ok())
            .filter(|x| x.is_finite())
            .ok_or_else(|| format!("the reference file has no usable `# {k}=` line"))
    };
    if dbm.len() != super::sweep::POINTS {
        return Err(format!(
            "the reference file has {} points, not {}",
            dbm.len(),
            super::sweep::POINTS
        ));
    }
    Ok(Reference {
        tg_level_dbm: num("tg_level_dbm")?,
        trace: Trace {
            start_hz: num("start_hz")?,
            stop_hz: num("stop_hz")?,
            dbm,
            rbw_hz: num("rbw_hz")?,
            vbw_hz: num("vbw_hz")?,
            ref_level_dbm: 0.0,
            attenuation_db: num("attenuation_db")?,
            detector: kv
                .get("detector")
                .ok_or("the reference file has no `# detector=` line")?
                .to_string(),
            sweep_time_s: 0.0,
            average: num("average_count")? as u32,
        },
    })
}

/// Refuses a measurement whose settings differ from the reference's, naming the first field that differs.
pub fn check_same(r: &Reference, t: &Trace, tg_level_dbm: f64) -> Result<(), String> {
    // [@ANCHOR: bench_mcp_sa_check_same]
    let rt = &r.trace;
    let close = |a: f64, b: f64| (a - b).abs() <= 1e-6 * a.abs().max(b.abs()).max(1.0);
    let checks = [
        ("start_hz", rt.start_hz, t.start_hz),
        ("stop_hz", rt.stop_hz, t.stop_hz),
        ("rbw_hz", rt.rbw_hz, t.rbw_hz),
        ("vbw_hz", rt.vbw_hz, t.vbw_hz),
        ("attenuation_db", rt.attenuation_db, t.attenuation_db),
        ("average_count", rt.average as f64, t.average as f64),
        ("tg_level_dbm", r.tg_level_dbm, tg_level_dbm),
    ];
    for (name, a, b) in checks {
        if !close(a, b) {
            return Err(format!(
                "the reference was taken with {name} {a} and this sweep has {b}. Next: repeat bench_sa_tg_normalize with the settings you want, or leave those arguments out to use the reference's."
            ));
        }
    }
    if !rt.detector.eq_ignore_ascii_case(&t.detector) {
        return Err(format!(
            "the reference was taken with detector {} and this sweep has {}. Next: repeat bench_sa_tg_normalize or leave detector out.",
            rt.detector, t.detector
        ));
    }
    Ok(())
}

/// Fills the sweep arguments the caller left out from the reference's settings.
fn with_defaults(args: &Value, r: &Reference) -> Value {
    // [@ANCHOR: bench_mcp_sa_with_defaults]
    let mut a = args.clone();
    let t = &r.trace;
    let has_span = ["start_hz", "stop_hz", "center_hz", "span_hz"]
        .iter()
        .any(|k| a.get(*k).is_some_and(|v| !v.is_null()));
    let det = match t.detector.to_ascii_lowercase().as_str() {
        d if d.starts_with("pos") => "positive",
        d if d.starts_with("neg") => "negative",
        d if d.starts_with("sam") => "sample",
        d if d.starts_with("ave") => "average",
        _ => "normal",
    };
    let mut put = |k: &str, v: Value| {
        if a.get(k).is_none_or(|x| x.is_null()) {
            a[k] = v;
        }
    };
    if !has_span {
        put("start_hz", json!(t.start_hz));
        put("stop_hz", json!(t.stop_hz));
    }
    put("rbw_hz", json!(t.rbw_hz));
    put("vbw_hz", json!(t.vbw_hz));
    put("attenuation_db", json!(t.attenuation_db));
    put("detector", json!(det));
    put("average_count", json!(t.average));
    put("tg_level_dbm", json!(r.tg_level_dbm));
    a
}

/// TG on at `level`, one sweep, TG off again unless `leave_on`. The off is attempted on every path.
fn tg_sweep(
    l: &mut Link,
    inst: &Instrument,
    req: &SweepReq,
    level: f64,
    leave_on: bool,
) -> Result<Trace, String> {
    // [@ANCHOR: bench_mcp_sa_tg_sweep]
    let on = switch_on(l, inst, Some(level)).map(|_| ());
    let t = match on {
        Ok(()) => run_sweep(l, req),
        Err(e) => Err(e),
    };
    let off = (!leave_on).then(|| ensure_tg_off(l));
    finish(t, off)
}

/// `bench_sa_tg_normalize`: sweeps the through with the TG on and stores it under `name` (memory and a CSV with the settings).
pub fn normalize(connect: &Connector, inst: &Instrument, args: &Value) -> Result<Value, String> {
    // [@ANCHOR: bench_mcp_sa_normalize]
    let name = ref_name(args.get("name").and_then(|v| v.as_str()))?;
    let level = level_arg(inst, args)?.unwrap_or(TG_RATED_MIN_DBM);
    let leave_on = flag(args, "leave_tg_on", false)?;
    let (mut l, id) = open(connect, inst)?;
    let req = sweep_args(args, id.max_hz)?;
    require_tg(&mut l)?;
    let t = tg_sweep(&mut l, inst, &req, level, leave_on)?;
    let r = Reference {
        tg_level_dbm: level,
        trace: t,
    };
    let file = write_csv(
        &crate::out_dir(),
        &format!("sa_tg_reference_{name}"),
        &comments(&r),
        "freq_hz,level_dbm",
        r.trace.start_hz,
        r.trace.stop_hz,
        &[&r.trace.dbm],
    )?;
    let mut v = trace_summary(&r.trace, &find_peaks(&r.trace, 0, 10.0));
    refs()
        .lock()
        .unwrap()
        .insert(format!("{}:{name}", inst.name), r.clone());
    v["name"] = json!(name);
    v["tg_level_dbm"] = json!(level);
    v["file"] = json!(file);
    v["tracking_generator"] = json!(if leave_on { "on" } else { "off (verified)" });
    Ok(v)
}

/// `bench_sa_tg_s21`: TG on, sweep, TG off unless asked otherwise; values relative to the through reference or to the TG level.
pub fn s21(connect: &Connector, inst: &Instrument, args: &Value) -> Result<Value, String> {
    // [@ANCHOR: bench_mcp_sa_s21]
    let leave_on = flag(args, "leave_tg_on", false)?;
    let reference = match (
        args.get("reference").and_then(|v| v.as_str()),
        args.get("reference_file").and_then(|v| v.as_str()),
    ) {
        (Some(_), Some(_)) => return Err("give reference or reference_file, not both".into()),
        (Some(n), None) => {
            let key = format!("{}:{}", inst.name, ref_name(Some(n))?);
            Some(refs().lock().unwrap().get(&key).cloned().ok_or_else(|| {
                format!("no reference named {n:?} in this server. Next: run bench_sa_tg_normalize with name {n:?} first, or pass reference_file.")
            })?)
        }
        (None, Some(f)) => {
            let text = std::fs::read_to_string(f).map_err(|e| format!("{f}: {e}"))?;
            Some(parse_reference(&text)?)
        }
        (None, None) => None,
    };
    let args = match &reference {
        Some(r) => with_defaults(args, r),
        None => args.clone(),
    };
    let level = level_arg(inst, &args)?.unwrap_or(TG_RATED_MIN_DBM);
    let (mut l, id) = open(connect, inst)?;
    let req = sweep_args(&args, id.max_hz)?;
    require_tg(&mut l)?;
    let t = tg_sweep(&mut l, inst, &req, level, leave_on)?;
    let base: Vec<f64> = match &reference {
        Some(r) => {
            check_same(r, &t, level)?;
            r.trace.dbm.clone()
        }
        None => vec![level; t.dbm.len()],
    };
    let s21_db: Vec<f64> = t.dbm.iter().zip(&base).map(|(m, b)| m - b).collect();
    let file = write_csv(
        &crate::out_dir(),
        "sa_tg_s21",
        &[],
        "freq_hz,s21_db,measured_dbm",
        t.start_hz,
        t.stop_hz,
        &[&s21_db, &t.dbm],
    )?;
    let (imax, imin) = s21_db.iter().enumerate().fold((0, 0), |(a, b), (i, &v)| {
        (
            if v > s21_db[a] { i } else { a },
            if v < s21_db[b] { i } else { b },
        )
    });
    let at = |i: usize| bin_hz(t.start_hz, t.stop_hz, s21_db.len(), i);
    let mut v = trace_summary(&t, &[]);
    v["s21"] = json!({
        "relative_to": if reference.is_some() { "through reference" } else { "tg level (uncalibrated: cable and generator flatness included)" },
        "max_db": s21_db[imax], "max_at_hz": at(imax), "min_db": s21_db[imin], "min_at_hz": at(imin),
        "mean_db": s21_db.iter().sum::<f64>() / s21_db.len() as f64,
    });
    v["tg_level_dbm"] = json!(level);
    v["file"] = json!(file);
    v["tracking_generator"] = json!(if leave_on { "on" } else { "off (verified)" });
    Ok(v)
}

#[cfg(test)]
mod tests {
    use super::super::testkit::*;
    use super::super::tests::bench;
    use super::super::*;
    use super::*;

    fn sweep_args_json(extra: Value) -> Value {
        let mut a = json!({"start_hz": 1e6, "stop_hz": 751e6, "rbw_hz": 100000});
        for (k, v) in extra.as_object().unwrap() {
            a[k] = v.clone();
        }
        a
    }

    fn states(dev: &FakeDevice) -> Vec<String> {
        dev.received()
            .into_iter()
            .filter(|c| c.starts_with(":OUTPut:STATe ") && !c.ends_with('?'))
            .collect()
    }

    // Tests [@ANCHOR: bench_mcp_sa_tg_state]
    // Tests [@ANCHOR: bench_mcp_sa_level_arg]
    // Tests [@ANCHOR: bench_mcp_sa_require_tg]
    // Tests [@ANCHOR: bench_mcp_sa_set_level]
    // Tests [@ANCHOR: bench_mcp_sa_switch_on]
    // Tests [@ANCHOR: bench_mcp_sa_tg_on]
    // Tests [@ANCHOR: bench_mcp_sa_tg_level]
    #[test]
    fn the_generator_is_off_by_default_level_is_set_and_verified_before_on_and_the_range_and_cap_hold(
    ) {
        let dev = FakeDevice::sim(Sim::default());
        let st = call_with(&dev.connector(), &bench(""), "bench_sa_status", &json!({})).unwrap();
        assert_eq!(st["tracking_generator"]["state"], "off");
        assert!(
            states(&dev).is_empty(),
            "status never switches the generator"
        );
        let v = call_with(
            &dev.connector(),
            &bench(r#","max_tg_dbm":-10"#),
            "bench_sa_tg_on",
            &json!({"tg_level_dbm": -12}),
        )
        .unwrap();
        assert_eq!(
            (v["tracking_generator"].as_str(), v["level_dbm"].as_f64()),
            (Some("on"), Some(-12.0))
        );
        let sent = dev.received();
        let lv = sent.iter().position(|c| c == ":SOURce:POWer -12").unwrap();
        let on = sent.iter().position(|c| c == ":OUTPut:STATe ON").unwrap();
        assert!(
            lv < on,
            "the level is set before the output goes on: {sent:?}"
        );
        // The level can be changed without changing on/off.
        let v = call_with(
            &dev.connector(),
            &bench(""),
            "bench_sa_tg_level",
            &json!({"tg_level_dbm": -15}),
        )
        .unwrap();
        assert_eq!(
            (v["tracking_generator"].as_str(), v["level_dbm"].as_f64()),
            (Some("on"), Some(-15.0))
        );
        // Out of range, above the cap, missing: refused with nothing sent.
        let quiet = FakeDevice::sim(Sim::default());
        let cap = bench(r#","max_tg_dbm":-10"#);
        for (args, name) in [
            (json!({"tg_level_dbm": -9}), "bench_sa_tg_on"),
            (json!({"tg_level_dbm": -25}), "bench_sa_tg_on"),
            (json!({"tg_level_dbm": 3}), "bench_sa_tg_level"),
            (json!({}), "bench_sa_tg_level"),
            (json!({"tg_level_dbm": "hi"}), "bench_sa_tg_on"),
        ] {
            assert!(
                call_with(&quiet.connector(), &cap, name, &args).is_err(),
                "{name} {args}"
            );
        }
        assert!(quiet.received().is_empty());
        // Turning on at the level it already has works; above the cap it is refused and left off.
        let hot = FakeDevice::sim(Sim {
            level: -3.0,
            ..Sim::default()
        });
        let e = call_with(&hot.connector(), &cap, "bench_sa_tg_on", &json!({})).unwrap_err();
        assert!(e.contains("above max_tg_dbm"), "{e}");
        assert_eq!(
            states(&hot),
            [":OUTPut:STATe OFF"; 1].map(String::from),
            "left off, and off verified"
        );
        // No TG option: refused.
        let none = FakeDevice::sim(Sim {
            options: "Meas EMI".into(),
            ..Sim::default()
        });
        let e = call_with(&none.connector(), &bench(""), "bench_sa_tg_on", &json!({})).unwrap_err();
        assert!(e.contains("TG"), "{e}");
        assert!(states(&none).is_empty());
        // A level the unit does not keep is an error and the output is off afterwards.
        let liar = FakeDevice::sim(Sim {
            garbage: Some((":SOURce:POWer?".into(), "-1.0".into())),
            ..Sim::default()
        });
        let e = call_with(
            &liar.connector(),
            &bench(""),
            "bench_sa_tg_on",
            &json!({"tg_level_dbm": -20}),
        )
        .unwrap_err();
        assert!(e.contains("reads back"), "{e}");
        assert!(!states(&liar).contains(&":OUTPut:STATe ON".to_string()));
    }

    // Tests [@ANCHOR: bench_mcp_sa_tg_off]
    // Tests [@ANCHOR: bench_mcp_sa_ensure_tg_off]
    // Tests [@ANCHOR: bench_mcp_sa_finish]
    #[test]
    fn off_is_verified_a_stuck_generator_is_reported_loudly_and_errors_combine() {
        let dev = FakeDevice::sim(Sim {
            on: true,
            ..Sim::default()
        });
        let v = call_with(&dev.connector(), &bench(""), "bench_sa_tg_off", &json!({})).unwrap();
        assert_eq!(
            (v["tracking_generator"].as_str(), v["verified"].as_bool()),
            (Some("off"), Some(true))
        );
        let stuck = FakeDevice::sim(Sim {
            on: true,
            stuck_on: true,
            ..Sim::default()
        });
        let e = call_with(
            &stuck.connector(),
            &bench(""),
            "bench_sa_tg_off",
            &json!({}),
        )
        .unwrap_err();
        assert!(
            e.contains("SAFE STATE NOT CONFIRMED") && e.contains("front panel"),
            "{e}"
        );
        assert_eq!(states(&stuck).len(), OFF_TRIES as usize);
        assert_eq!(finish(Ok(1), None), Ok(1));
        assert_eq!(finish(Ok(1), Some(Ok(()))), Ok(1));
        assert_eq!(
            finish::<i32>(Err("w".into()), Some(Ok(()))),
            Err("w".into())
        );
        assert_eq!(finish(Ok(1), Some(Err("off".into()))), Err("off".into()));
        assert_eq!(
            finish::<i32>(Err("w".into()), Some(Err("off".into()))),
            Err("w; off".into())
        );
    }

    // Tests [@ANCHOR: bench_mcp_sa_tg_sweep]
    // Tests [@ANCHOR: bench_mcp_sa_normalize]
    // Tests [@ANCHOR: bench_mcp_sa_ref_name]
    // Tests [@ANCHOR: bench_mcp_sa_refs]
    // Tests [@ANCHOR: bench_mcp_sa_reference_comments]
    // Tests [@ANCHOR: bench_mcp_sa_s21]
    // Tests [@ANCHOR: bench_mcp_sa_check_same]
    // Tests [@ANCHOR: bench_mcp_sa_with_defaults]
    #[test]
    fn s21_is_relative_to_the_stored_through_with_the_generator_on_only_during_the_sweep() {
        let d = std::env::temp_dir().join(format!("bench_mcp_sa_test_{}", std::process::id()));
        std::env::set_var("BENCH_MCP_OUT", &d);
        let sim = Sim::default();
        let loss = sim.dut_db.clone();
        let dev = FakeDevice::sim(sim);
        let b = bench("");
        // The through: 0.5 dB of cable.
        let n = call_with(
            &dev.connector(),
            &b,
            "bench_sa_tg_normalize",
            &sweep_args_json(json!({"name": "thru-1", "tg_level_dbm": -10})),
        )
        .unwrap();
        assert_eq!(
            (n["name"].as_str(), n["tracking_generator"].as_str()),
            (Some("thru-1"), Some("off (verified)"))
        );
        let sent = dev.received();
        let on = sent.iter().position(|c| c == ":OUTPut:STATe ON").unwrap();
        let sweep = sent
            .iter()
            .position(|c| c == ":INITiate:IMMediate")
            .unwrap();
        let off = sent.iter().rposition(|c| c == ":OUTPut:STATe OFF").unwrap();
        assert!(on < sweep && sweep < off, "{sent:?}");
        // The device: 20 dB down, plus the 10 dB dip at point 300 (mid band).
        *loss.lock().unwrap() = 20.5;
        let v = call_with(
            &dev.connector(),
            &b,
            "bench_sa_tg_s21",
            &json!({"reference": "thru-1"}),
        )
        .unwrap();
        assert_eq!(v["s21"]["relative_to"], "through reference");
        assert!(
            (v["s21"]["max_db"].as_f64().unwrap() + 20.0).abs() < 1e-6,
            "{v}"
        );
        assert!(
            (v["s21"]["min_db"].as_f64().unwrap() + 30.0).abs() < 1e-6,
            "{v}"
        );
        assert_eq!(v["s21"]["min_at_hz"].as_f64(), Some(1e6 + 300.0 * 1e6));
        assert_eq!(
            v["tg_level_dbm"].as_f64(),
            Some(-10.0),
            "taken from the reference"
        );
        assert_eq!(v["tracking_generator"], "off (verified)");
        let rows = std::fs::read_to_string(v["file"].as_str().unwrap()).unwrap();
        assert!(rows.starts_with("freq_hz,s21_db,measured_dbm\n"));
        assert_eq!(rows.lines().count(), 752);
        assert_eq!(
            states(&dev).last().map(String::as_str),
            Some(":OUTPut:STATe OFF")
        );
        // Without a reference: the level minus the generator level, said so.
        let u = call_with(
            &dev.connector(),
            &b,
            "bench_sa_tg_s21",
            &sweep_args_json(json!({"tg_level_dbm": -10, "leave_tg_on": true})),
        )
        .unwrap();
        assert!(u["s21"]["relative_to"]
            .as_str()
            .unwrap()
            .contains("uncalibrated"));
        assert!((u["s21"]["max_db"].as_f64().unwrap() + 20.5).abs() < 1e-6);
        assert_eq!(u["tracking_generator"], "on");
        assert_eq!(
            states(&dev).last().map(String::as_str),
            Some(":OUTPut:STATe ON")
        );
        // A reference file from a past session works the same way.
        let csv = n["file"].as_str().unwrap().to_string();
        let f = call_with(
            &dev.connector(),
            &b,
            "bench_sa_tg_s21",
            &json!({"reference_file": csv}),
        )
        .unwrap();
        assert!((f["s21"]["max_db"].as_f64().unwrap() + 20.0).abs() < 1e-6);
        // Errors: unknown reference, different settings, bad names, both references.
        let e = call_with(
            &dev.connector(),
            &b,
            "bench_sa_tg_s21",
            &json!({"reference": "nope"}),
        )
        .unwrap_err();
        assert!(e.contains("bench_sa_tg_normalize"), "{e}");
        let e = call_with(
            &dev.connector(),
            &b,
            "bench_sa_tg_s21",
            &json!({"reference": "thru-1", "rbw_hz": 300000}),
        )
        .unwrap_err();
        assert!(e.contains("rbw_hz"), "{e}");
        assert_eq!(
            states(&dev).last().map(String::as_str),
            Some(":OUTPut:STATe OFF"),
            "off again after a refused comparison"
        );
        let e = call_with(
            &dev.connector(),
            &b,
            "bench_sa_tg_s21",
            &json!({"reference": "a", "reference_file": "b"}),
        )
        .unwrap_err();
        assert!(e.contains("not both"));
        assert!(call_with(
            &dev.connector(),
            &b,
            "bench_sa_tg_normalize",
            &sweep_args_json(json!({"name": "a/b"}))
        )
        .is_err());
        assert_eq!(ref_name(None).unwrap(), "default");
        assert!(ref_name(Some("")).is_err() && ref_name(Some(&"x".repeat(41))).is_err());
    }

    // Tests [@ANCHOR: bench_mcp_sa_parse_reference]
    // Tests [@ANCHOR: bench_mcp_sa_reference]
    #[test]
    fn a_reference_file_without_its_settings_or_with_a_bad_row_is_refused() {
        let good = |extra: &str| {
            let mut t = String::from("# tg_level_dbm=-20\n# start_hz=0\n# stop_hz=1000\n# rbw_hz=1000\n# vbw_hz=1000\n# attenuation_db=10\n# detector=POSitive\n# average_count=1\nfreq_hz,level_dbm\n");
            for i in 0..751 {
                t.push_str(&format!("{i}.0,-40.0\n"));
            }
            t.push_str(extra);
            t
        };
        let r = parse_reference(&good("")).unwrap();
        assert_eq!(
            (r.tg_level_dbm, r.trace.dbm.len(), r.trace.average),
            (-20.0, 751, 1)
        );
        assert!(parse_reference(&good("1.0,abc\n")).is_err());
        assert!(parse_reference(&good("1.0,-1\n")).is_err(), "752 points");
        assert!(parse_reference(&good("").replace("# rbw_hz=1000\n", ""))
            .unwrap_err()
            .contains("rbw_hz"));
        assert!(parse_reference("").is_err());
    }

    // Tests [@ANCHOR: bench_mcp_sa_sweep]
    #[test]
    fn a_sweep_with_tg_off_after_switches_it_off_and_proves_it() {
        std::env::set_var(
            "BENCH_MCP_OUT",
            std::env::temp_dir().join(format!("bench_mcp_sa_test_{}", std::process::id())),
        );
        let dev = FakeDevice::sim(Sim {
            on: true,
            ..Sim::default()
        });
        let v = call_with(
            &dev.connector(),
            &bench(""),
            "bench_sa_sweep",
            &json!({"start_hz": 1e6, "stop_hz": 2e6, "tg_off_after": true}),
        )
        .unwrap();
        assert_eq!(v["tracking_generator"], "off (verified)");
        let sent = dev.received();
        assert!(
            sent.iter().rposition(|c| c == ":OUTPut:STATe OFF").unwrap()
                > sent.iter().position(|c| c == "*OPC?").unwrap()
        );
        let stuck = FakeDevice::sim(Sim {
            on: true,
            stuck_on: true,
            ..Sim::default()
        });
        let e = call_with(
            &stuck.connector(),
            &bench(""),
            "bench_sa_sweep",
            &json!({"start_hz": 1e6, "stop_hz": 2e6, "tg_off_after": true}),
        )
        .unwrap_err();
        assert!(e.contains("SAFE STATE NOT CONFIRMED"), "{e}");
        // Not asked: it reports the state and leaves it.
        let on = FakeDevice::sim(Sim {
            on: true,
            ..Sim::default()
        });
        let v = call_with(
            &on.connector(),
            &bench(""),
            "bench_sa_sweep",
            &json!({"start_hz": 1e6, "stop_hz": 2e6}),
        )
        .unwrap();
        assert_eq!(v["tracking_generator"]["state"], "on");
        assert!(states(&on).is_empty());
    }
}
