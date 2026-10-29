// SPDX-License-Identifier: AGPL-3.0-or-later
//! The transmit tools. They key the rig, and they exist only for a rig whose config carries the owner's attestation that its
//! antenna port is on a dummy load. Order of every keyed tool:
//!
//! 1. the arguments against the attestation (frequency range, mode, power cap, duration), then the rig (its transmit range, PTT,
//!    split, XIT, the power setting and the cap) -- nothing is sent that changes anything;
//! 2. `dry_run` returns the plan here; 3. `confirm_dummy_load` must be true; 4. the load plausibility check must say "consistent
//!    with a load"; 5. the operator alert, if the owner configured one; 6. frequency, mode and power are set and read back;
//! 7. the low-power probe key reads the SWR (an open or shorted connection receives nothing, so the check above cannot see it);
//! 8. the keyed interval with the watchdog armed; 9. release, read-back, power restored, log line.
//!
//! The software cannot know what is connected. Saying so is part of every description and of the limits page.

use super::attest::Transmit;
use super::audio_in::AudioSource;
use super::control::{parse_dump_ranges, MODES};
use super::guard::Spawner;
use super::load::Verdict;
use super::loadtools;
use super::proto::Client;
use super::status::{bounded, tolerate};
use super::tx::{self, Keying, Purpose, Sample};
use super::{rigd, Config};
use crate::config::Instrument;
use serde_json::{json, Value};
use std::time::Duration;

const CANNOT_KNOW: &str = "The software cannot know what is connected to the rig. The owner's attestation in the config that the antenna port is on a dummy load, and the limits in it, are the control; the load plausibility check and the SWR probe are evidence, not proof. Compliance with the licence terms for any transmission is the owner's responsibility.";

/// Sounds an alert for the operator. Production plays it through the sound tools; tests record the request, because no test makes sound.
pub trait Alerter {
    fn alert(&self, message: &str) -> Result<Value, String>;
}

/// The operator alert through the `audio` module's `bench_audio_alert`.
pub struct AudioAlert<'a> {
    pub insts: &'a [Instrument],
    pub instrument: String,
}

impl Alerter for AudioAlert<'_> {
    fn alert(&self, message: &str) -> Result<Value, String> {
        // [@ANCHOR: hamlib_tx_alert]
        crate::audio::call(
            self.insts,
            "bench_audio_alert",
            &json!({"instrument": self.instrument, "severity": "warning", "message": message, "rediscover": false}),
        )
    }
}

/// What the keyed tools need besides the rig: the watchdog starter, the alert and the audio for the load check.
pub struct Deps<'a> {
    pub spawner: &'a dyn Spawner,
    pub alert: Option<&'a dyn Alerter>,
    pub audio: Option<&'a dyn AudioSource>,
}

fn attested(cfg: &Config) -> Result<&Transmit, String> {
    cfg.transmit.as_ref().ok_or_else(|| "this rig has no transmit attestation in the bench config: no tool transmits with it. Nothing was sent.".to_string())
}

/// The lowest RFPOWER setting the rig accepts, from the `level_gran` line of `dump_state` (RFPOWER is level number 12).
pub fn rfpower_min(dump: &[String]) -> Option<f64> {
    // [@ANCHOR: hamlib_tx_min_power]
    let line = dump.iter().find_map(|l| l.strip_prefix("level_gran="))?;
    let entry = line.split(';').find_map(|e| e.strip_prefix("12="))?;
    let mut f = entry.split(',').filter_map(|x| x.parse::<f64>().ok());
    let (lo, hi) = (f.next()?, f.next()?);
    (hi > lo && (0.0..1.0).contains(&lo)).then_some(lo)
}

/// "Power [0.0..1.0]: 0.05" or "Power mW: 50000": the number after the colon on the line that starts with Power.
fn power_value(r: &super::proto::Reply) -> Result<f64, String> {
    r.lines
        .iter()
        .find(|l| l.starts_with("Power"))
        .and_then(|l| l.split_once(':'))
        .and_then(|(_, v)| v.trim().parse::<f64>().ok())
        .filter(|v| v.is_finite())
        .ok_or_else(|| format!("rigctld's power conversion gave no number: {:?}", r.lines))
}

fn watts_to_fraction(c: &mut Client, w: f64, hz: f64, mode: &str) -> Result<f64, String> {
    // [@ANCHOR: hamlib_tx_power_convert]
    let r = c.read(&format!("mW2power {:.0} {hz:.0} {mode}", w * 1000.0)).map_err(|e| format!("the rig's power setting for {w} W cannot be worked out ({e}); without it the power cap cannot be enforced, so nothing is sent"))?;
    power_value(&r)
}

fn fraction_to_watts(c: &mut Client, f: f64, hz: f64, mode: &str) -> Result<f64, String> {
    let r = c.read(&format!("power2mW {f} {hz:.0} {mode}")).map_err(|e| format!("the rig's power setting {f} cannot be turned into watts ({e}); without it the power cap cannot be enforced, so nothing is sent"))?;
    Ok(power_value(&r)? / 1000.0)
}

/// Arguments of a keyed test, checked against the attestation.
#[derive(Clone, Debug, PartialEq)]
pub struct TestArgs {
    pub frequency_hz: f64,
    pub mode: String,
    pub power_w: f64,
    pub seconds: f64,
    pub every_ms: f64,
    pub dry_run: bool,
    pub confirm: bool,
    pub reason: String,
}

pub fn check_args(args: &Value, tx: &Transmit) -> Result<TestArgs, String> {
    // [@ANCHOR: hamlib_tx_args]
    let flag = |k: &str| match args.get(k) {
        None | Some(Value::Null) => Ok(false),
        Some(Value::Bool(b)) => Ok(*b),
        Some(_) => Err(format!("{k} must be true or false")),
    };
    let frequency_hz = bounded(args, "frequency_hz", f64::NAN, 1.0, 1.0e12)
        .map_err(|_| "frequency_hz is required (Hz)".to_string())?;
    if tx.range_of(frequency_hz).is_none() {
        return Err(format!(
            "{frequency_hz:.0} Hz is outside transmit.frequency_ranges, the owner's list of frequencies for dummy-load tests ({}). There is no default list; the owner lists them and answers for their licence",
            tx.frequency_ranges.iter().map(|r| format!("{:.0} to {:.0} Hz", r.0, r.1)).collect::<Vec<_>>().join(", ")
        ));
    }
    let mode = match args.get("mode").and_then(|m| m.as_str()) {
        Some(m) => m.to_ascii_uppercase(),
        None => tx.allowed_modes[0].clone(),
    };
    if !MODES.contains(&mode.as_str()) || !tx.allowed_modes.contains(&mode) {
        return Err(format!(
            "mode {mode} is not in transmit.allowed_modes ({})",
            tx.allowed_modes.join(", ")
        ));
    }
    let power_w = bounded(args, "power_w", f64::NAN, 0.001, 1000.0)
        .map_err(|_| "power_w is required (watts)".to_string())?;
    if power_w > tx.max_power_w {
        return Err(format!(
            "power_w {power_w} is above transmit.max_power_w {} (the owner's cap)",
            tx.max_power_w
        ));
    }
    let seconds = bounded(args, "seconds", f64::NAN, 0.1, 60.0).map_err(|_| {
        "seconds is required: the duration of the keyed interval (0.1 to 60)".to_string()
    })?;
    if seconds > tx.max_seconds {
        return Err(format!(
            "seconds {seconds} is above transmit.max_seconds {} (the owner's cap)",
            tx.max_seconds
        ));
    }
    let every_ms = bounded(args, "sample_every_ms", 250.0, 100.0, 2000.0)?;
    let reason = args
        .get("reason")
        .and_then(|r| r.as_str())
        .map(str::trim)
        .unwrap_or("")
        .to_string();
    if reason.len() > 120 || reason.chars().any(char::is_control) {
        return Err("reason is at most 120 characters of plain text".into());
    }
    Ok(TestArgs {
        frequency_hz,
        mode,
        power_w,
        seconds,
        every_ms,
        dry_run: flag("dry_run")?,
        confirm: flag("confirm_dummy_load")?,
        reason,
    })
}

/// What the rig said before anything was changed.
#[derive(Clone, Debug)]
pub struct Rig {
    pub f_min: f64,
    pub f_req: f64,
    pub f_prev: f64,
    pub w_prev: f64,
    pub w_full: f64,
    pub prev_freq: f64,
    pub prev_mode: Option<(String, f64)>,
}

/// Reads the rig and refuses what the attestation does not allow. Changes nothing.
pub fn preflight(c: &mut Client, tx: &Transmit, a: &TestArgs) -> Result<Rig, String> {
    // [@ANCHOR: hamlib_tx_preflight]
    let dump = tolerate(c.dump_state())?
        .map_err(|e| format!("the rig's limits cannot be read ({e}); nothing was sent"))?;
    let (_, tx_ranges) = parse_dump_ranges(&dump.lines);
    if !tx_ranges
        .iter()
        .any(|r| a.frequency_hz >= r.0 && a.frequency_hz <= r.1)
    {
        return Err(format!(
            "{:.0} Hz is outside the rig's own transmit range ({}). Nothing was sent.",
            a.frequency_hz,
            if tx_ranges.is_empty() {
                "the rig reports none".into()
            } else {
                tx_ranges
                    .iter()
                    .map(|r| format!("{:.0} to {:.0} Hz", r.0, r.1))
                    .collect::<Vec<_>>()
                    .join(", ")
            }
        ));
    }
    match tolerate(c.read("get_ptt"))? {
        Ok(r) if r.kv("PTT").is_some_and(|p| p != "0") => return Err("the rig already reports PTT on. Nothing was sent. Next: find out who keyed it; bench_hamlib_safe_state releases it.".into()),
        _ => {}
    }
    if let Ok(r) = tolerate(c.read("get_split_vfo"))? {
        if r.kv("Split").is_some_and(|s| s != "0") {
            return Err("split is on, so the transmit frequency is the other VFO's, which this tool does not check. Turn split off (bench_hamlib_set) and try again. Nothing was sent.".into());
        }
    }
    if let Ok(x) = tolerate(c.read_number("get_xit", Some("XIT")))? {
        if x != 0.0 {
            return Err(format!("XIT is {x} Hz and shifts the transmit frequency. Set it to 0 (bench_hamlib_set) and try again. Nothing was sent."));
        }
    }
    let f_min = rfpower_min(&dump.lines).ok_or("the rig's minimum power setting cannot be found (no RFPOWER range reported); the watchdog and the probe need it, so nothing is sent")?;
    let f_req = watts_to_fraction(c, a.power_w, a.frequency_hz, &a.mode)?;
    if f_req < f_min {
        return Err(format!(
            "{} W is below the rig's lowest power setting ({f_min}). Nothing was sent.",
            a.power_w
        ));
    }
    let w_set = fraction_to_watts(c, f_req.min(1.0), a.frequency_hz, &a.mode)?;
    if w_set > tx.max_power_w * 1.02 {
        return Err(format!("the setting for {} W comes to {w_set:.2} W by the rig's own table, above the cap {} W. Nothing was sent.", a.power_w, tx.max_power_w));
    }
    let f_prev = c
        .read_number("get_level RFPOWER", None)
        .map_err(|e| format!("the current power setting cannot be read ({e}); nothing was sent"))?;
    let w_prev = fraction_to_watts(c, f_prev, a.frequency_hz, &a.mode)?;
    if w_prev > tx.max_power_w * 1.02 && !tx.allow_lowering_power {
        return Err(format!("the rig's power setting is {w_prev:.1} W, above the cap {} W. Nothing was sent. Next: lower it at the rig, or the owner sets allow_lowering_power in the config so that the tool may lower it (the old setting is restored afterwards).", tx.max_power_w));
    }
    let w_full = fraction_to_watts(c, 1.0, a.frequency_hz, &a.mode)?;
    let prev_freq = c
        .read_number("get_freq", Some("Frequency"))
        .map_err(|e| e.to_string())?;
    let prev_mode = tolerate(c.read("get_mode"))?
        .ok()
        .and_then(|r| Some((r.kv("Mode")?.to_string(), r.number(Some("Passband")).ok()?)));
    Ok(Rig {
        f_min,
        f_req,
        f_prev,
        w_prev,
        w_full,
        prev_freq,
        prev_mode,
    })
}

fn set_verified(
    c: &mut Client,
    cmd: &str,
    get: &str,
    key: Option<&str>,
    want: f64,
    tol: f64,
    what: &str,
) -> Result<(), String> {
    // [@ANCHOR: hamlib_tx_set_verified]
    c.command(cmd).map_err(|e| format!("{what}: {e}"))?;
    let back = c
        .read_number(get, key)
        .map_err(|e| format!("{what} read back: {e}"))?;
    if (back - want).abs() > tol {
        return Err(format!("{what}: the rig reads back {back}, not {want}"));
    }
    Ok(())
}

/// Puts frequency, mode and power back. Returns what could not be restored.
fn restore(c: &mut Client, rig: &Rig) -> Vec<String> {
    // [@ANCHOR: hamlib_tx_restore]
    let mut failed = vec![];
    if set_verified(
        c,
        &format!("set_level RFPOWER {}", rig.f_prev),
        "get_level RFPOWER",
        None,
        rig.f_prev,
        0.01,
        "power",
    )
    .is_err()
    {
        failed.push(format!("power setting {}", rig.f_prev));
    }
    if let Some((m, w)) = &rig.prev_mode {
        if c.command(&format!("set_mode {m} {w:.0}")).is_err() {
            failed.push(format!("mode {m}"));
        }
    }
    if c.command(&format!("set_freq {:.0}", rig.prev_freq))
        .is_err()
    {
        failed.push(format!("frequency {:.0} Hz", rig.prev_freq));
    }
    failed
}

fn summary(samples: &[Sample]) -> Value {
    // [@ANCHOR: hamlib_tx_summary]
    let stat = |f: &dyn Fn(&Sample) -> Option<f64>| -> Value {
        let v: Vec<f64> = samples.iter().filter_map(f).collect();
        if v.is_empty() {
            return Value::Null;
        }
        let (lo, hi) = (
            v.iter().cloned().fold(f64::INFINITY, f64::min),
            v.iter().cloned().fold(f64::NEG_INFINITY, f64::max),
        );
        json!({"min": (lo * 100.0).round() / 100.0, "max": (hi * 100.0).round() / 100.0, "mean": (v.iter().sum::<f64>() / v.len() as f64 * 100.0).round() / 100.0})
    };
    let power = stat(&|s| s.power_w);
    let swr = stat(&|s| s.swr);
    let alc = stat(&|s| s.alc);
    let temperature = stat(&|s| s.temp);
    let not_reported: Vec<&str> = [
        ("power_w", &power),
        ("swr", &swr),
        ("alc", &alc),
        ("temperature", &temperature),
    ]
    .iter()
    .filter(|(_, v)| v.is_null())
    .map(|(n, _)| *n)
    .collect();
    json!({"samples": samples.len(), "power_w": power, "swr": swr, "alc": alc, "temperature": temperature, "not_reported": not_reported})
}

/// The plan, for `dry_run` and for the result.
fn plan_json(inst: &Instrument, tx: &Transmit, a: &TestArgs, rig: &Rig) -> Value {
    json!({
        "rig": inst.name, "frequency_hz": a.frequency_hz, "mode": a.mode, "power_w": a.power_w, "seconds": a.seconds,
        "limits": {"max_power_w": tx.max_power_w, "max_seconds": tx.max_seconds, "max_swr": tx.max_swr, "range_hz": tx.frequency_ranges},
        "rig_now": {"power_setting": rig.f_prev, "power_w": rig.w_prev, "minimum_power_setting": rig.f_min, "setting_for_request": rig.f_req},
        "attested_by": tx.attested_by, "attested_on": tx.attested_on, "cabling": tx.cabling,
    })
}

fn host_port(inst: &Instrument) -> Result<(String, u16), String> {
    rigd::endpoint(&inst.name, &inst.hamlib)
}

/// The operator alert, when the owner configured one. A configured alert that is not audible refuses: the owner asked to be warned.
fn operator_alert(
    tx: &Transmit,
    deps: &Deps,
    a: &TestArgs,
    rig: &str,
) -> Result<Option<Value>, String> {
    // [@ANCHOR: hamlib_tx_operator_alert]
    if !tx.operator_alert {
        return Ok(None);
    }
    let alerter = deps.alert.ok_or("transmit.operator_alert is on but there is no audio instrument to alert with. Nothing was sent.")?;
    let msg = format!(
        "Transmitting into the dummy load on {} for {} seconds at {} watts: {}",
        a.frequency_hz,
        a.seconds,
        a.power_w,
        if a.reason.is_empty() {
            rig.to_string()
        } else {
            a.reason.clone()
        }
    );
    let r = alerter.alert(&msg).map_err(|e| {
        format!(
            "the operator alert failed ({e}); the owner configured an alert, so nothing was sent"
        )
    })?;
    if r.get("audible") != Some(&json!(true)) {
        return Err(format!("the operator alert was not audible ({}); the owner configured an alert, so nothing was sent. Text to send by another channel: {}", r.get("error").and_then(|e| e.as_str()).unwrap_or("no sound"), r.get("text_to_send").and_then(|t| t.as_str()).unwrap_or(&msg)));
    }
    Ok(Some(r))
}

/// The load plausibility check as a gate. Anything but "consistent with a load" refuses.
fn gate(c: &mut Client, inst: &Instrument, deps: &Deps, a: &TestArgs) -> Result<Value, String> {
    // [@ANCHOR: hamlib_tx_gate]
    let (d, report) = loadtools::gate(c, &inst.hamlib, deps.audio, &inst.name, a.frequency_hz)?;
    if d.verdict != Verdict::Consistent {
        return Err(format!(
            "not transmitting: the load plausibility check says {}: {}. Next: {}",
            d.verdict.word(),
            d.reasons.join("; "),
            if d.verdict == Verdict::NoBaseline {
                "the owner records a baseline with the dummy load connected (bench_hamlib_load_baseline)"
            } else {
                "look at what is on the rig's antenna port; a dummy load does not hear time stations"
            }
        ));
    }
    Ok(
        json!({"verdict": d.verdict.word(), "compared_with_baseline": d.compared, "reasons": d.reasons, "readings": report["readings"]}),
    )
}

/// One keyed interval with everything around it checked: used by the test and by each step of a sweep.
#[allow(clippy::too_many_arguments)]
fn keyed(
    c: &mut Client,
    inst: &Instrument,
    deps: &Deps,
    rig: &Rig,
    a: &TestArgs,
    seconds: f64,
    f: f64,
    kind: &str,
    tool: &str,
) -> Result<tx::KeyedRun, String> {
    // [@ANCHOR: hamlib_tx_keyed]
    let tx_cfg = attested(&inst.hamlib)?;
    let (host, port) = host_port(inst)?;
    let dir = rigd::dir(&inst.hamlib);
    let power_w = fraction_to_watts(c, f.min(1.0), a.frequency_hz, &a.mode)?;
    set_verified(
        c,
        &format!("set_level RFPOWER {f}"),
        "get_level RFPOWER",
        None,
        f,
        0.01,
        "setting the power",
    )?;
    let k = Keying {
        rig: &inst.name,
        host: &host,
        port,
        dir: &dir,
        spawner: deps.spawner,
        min_power: Some(rig.f_min),
        purpose: Purpose {
            tool: tool.into(),
            reason: a.reason.clone(),
            by: tx_cfg.attested_by.clone(),
            frequency_hz: a.frequency_hz,
            mode: a.mode.clone(),
            power_w,
            kind: kind.into(),
        },
        limit_swr: Some(tx_cfg.max_swr),
        limit_power_w: Some(tx_cfg.max_power_w),
        previous_rfpower: Some(rig.f_prev),
    };
    tx::key(
        c,
        &k,
        seconds,
        Duration::from_millis(a.every_ms as u64),
        Some(rig.w_full),
    )
}

/// The low-power probe: an open or shorted connection receives nothing, so only a reflected-power reading can show it.
fn probe(
    c: &mut Client,
    inst: &Instrument,
    deps: &Deps,
    rig: &Rig,
    a: &TestArgs,
    tool: &str,
) -> Result<Value, String> {
    // [@ANCHOR: hamlib_tx_probe]
    let tx_cfg = attested(&inst.hamlib)?;
    if !tx_cfg.swr_probe {
        return Ok(
            json!({"done": false, "note": "the owner turned the SWR probe off in the config: an open or shorted connection is not checked"}),
        );
    }
    let run = keyed(
        c,
        inst,
        deps,
        rig,
        a,
        tx_cfg.swr_probe_s,
        rig.f_min,
        "probe",
        tool,
    )?;
    if let Some(why) = &run.aborted {
        return Err(format!("the low-power probe was stopped: {why}. Nothing further was sent. Next: check the dummy load and the cable."));
    }
    let worst = run
        .samples
        .iter()
        .filter_map(|s| s.swr)
        .fold(None, |m: Option<f64>, x| Some(m.map_or(x, |m| m.max(x))));
    match worst {
        None => Err("the rig reported no usable SWR during the low-power probe (the dummy rig and some radios report 0 or nothing), so an open or shorted connection cannot be ruled out. Nothing further was sent. The owner may turn the probe off with swr_probe: false and accepts that risk.".into()),
        Some(s) if s > tx_cfg.max_swr => Err(format!("SWR {s:.2} at the low-power probe is over the limit {:.2}: the load is open, shorted or wrong. Nothing further was sent.", tx_cfg.max_swr)),
        Some(s) => Ok(json!({"done": true, "swr": (s * 100.0).round() / 100.0, "power_setting": rig.f_min})),
    }
}

/// `bench_hamlib_tx_key_test`.
pub fn key_test(
    c: &mut Client,
    inst: &Instrument,
    deps: &Deps,
    args: &Value,
) -> Result<Value, String> {
    // [@ANCHOR: hamlib_tx_key_test]
    let tx_cfg = attested(&inst.hamlib)?;
    let a = check_args(args, tx_cfg)?;
    let rig = preflight(c, tx_cfg, &a)?;
    let plan = plan_json(inst, tx_cfg, &a, &rig);
    if a.dry_run {
        return Ok(json!({
            "dry_run": true, "sent": [], "plan": plan,
            "would": [
                "check the load plausibility check against the baseline (receive only)",
                if tx_cfg.operator_alert { "sound the operator alert" } else { "no operator alert (the owner has not configured one)" },
                "set frequency, mode and power and read them back",
                if tx_cfg.swr_probe { "key at the rig's minimum power for a moment and read the SWR" } else { "no low-power probe (turned off by the owner)" },
                "start the watchdog process, key, read the meters, release PTT and read it back",
                "put power, mode and frequency back; write the keyed-interval log",
            ],
            "note": CANNOT_KNOW,
        }));
    }
    if !a.confirm {
        return Err(format!("this tool transmits at {} W for {} s on {:.0} Hz. Call it with confirm_dummy_load: true only when the person at the bench has said the rig's antenna port is on the dummy load ({:?} in the config); dry_run: true shows the plan. {CANNOT_KNOW}", a.power_w, a.seconds, a.frequency_hz, tx_cfg.cabling));
    }
    if a.reason.is_empty() {
        return Err("reason is required: who is asking and why (up to 120 characters); it goes into the keyed-interval log".into());
    }
    let dir = rigd::dir(&inst.hamlib);
    let gate_report = gate(c, inst, deps, &a).inspect_err(|e| {
        tx::log_event(
            &dir,
            &inst.name,
            "refused",
            json!({"tool": "bench_hamlib_tx_key_test", "reason": a.reason, "why": e}),
        )
    })?;
    let alert = operator_alert(tx_cfg, deps, &a, &inst.name)?;
    let result = (|| -> Result<(Value, tx::KeyedRun), String> {
        set_verified(
            c,
            &format!("set_freq {:.0}", a.frequency_hz),
            "get_freq",
            Some("Frequency"),
            a.frequency_hz,
            1.0,
            "setting the frequency",
        )?;
        c.command(&format!("set_mode {} 0", a.mode))
            .map_err(|e| format!("setting the mode: {e}"))?;
        let back = c.read("get_mode").map_err(|e| e.to_string())?;
        if back.kv("Mode").map(|m| m.to_ascii_uppercase()).as_deref() != Some(a.mode.as_str()) {
            return Err(format!(
                "the rig reports mode {:?}, not {}; nothing was sent",
                back.kv("Mode"),
                a.mode
            ));
        }
        let probe_report = probe(c, inst, deps, &rig, &a, "bench_hamlib_tx_key_test")?;
        let run = keyed(
            c,
            inst,
            deps,
            &rig,
            &a,
            a.seconds,
            rig.f_req,
            "test",
            "bench_hamlib_tx_key_test",
        )?;
        Ok((probe_report, run))
    })();
    let not_restored = restore(c, &rig);
    let (probe_report, run) = result.inspect_err(|e| {
        tx::log_event(
            &dir,
            &inst.name,
            "stopped",
            json!({"tool": "bench_hamlib_tx_key_test", "reason": a.reason, "why": e}),
        )
    })?;
    Ok(json!({
        "plan": plan, "load_check": gate_report, "operator_alert": alert, "swr_probe": probe_report,
        "keyed": {"seconds": (run.seconds_keyed * 100.0).round() / 100.0, "aborted": run.aborted, "ptt_read_back_off": run.ptt_read_back, "meters": summary(&run.samples), "samples": run.samples.iter().take(80).map(|s| s.json()).collect::<Vec<_>>()},
        "rig_settings_not_restored": not_restored, "keyed_interval_log": tx::log_file(&dir, &inst.name),
        "note": CANNOT_KNOW,
    }))
}

/// `bench_hamlib_tx_power_sweep`: steps of power, each its own keyed interval with its own watchdog.
pub fn power_sweep(
    c: &mut Client,
    inst: &Instrument,
    deps: &Deps,
    args: &Value,
) -> Result<Value, String> {
    // [@ANCHOR: hamlib_tx_sweep]
    let tx_cfg = attested(&inst.hamlib)?;
    let steps: Vec<f64> = args
        .get("powers_w")
        .and_then(|p| p.as_array())
        .ok_or("powers_w is required: a list of 1 to 8 power levels in watts, lowest first")?
        .iter()
        .map(|p| {
            p.as_f64()
                .filter(|p| p.is_finite() && *p > 0.0)
                .ok_or("each of powers_w must be a positive number")
        })
        .collect::<Result<_, _>>()?;
    if steps.is_empty() || steps.len() > 8 {
        return Err("powers_w takes 1 to 8 power levels".into());
    }
    if steps.windows(2).any(|w| w[1] <= w[0]) {
        return Err("powers_w must rise: lowest first, each higher than the one before".into());
    }
    let dwell = bounded(args, "dwell_s", 1.0, 0.1, 60.0)?;
    let total = dwell * steps.len() as f64;
    if total > 60.0 {
        return Err(format!(
            "{} steps of {dwell} s make {total} s of keyed time; the most in one call is 60 s",
            steps.len()
        ));
    }
    // The checks of a single test, for the highest step; every step is also checked against the cap.
    let mut first = args.clone();
    first["power_w"] = json!(steps[steps.len() - 1]);
    first["seconds"] = json!(dwell);
    let a = check_args(&first, tx_cfg)?;
    let rig = preflight(c, tx_cfg, &a)?;
    let plan = plan_json(inst, tx_cfg, &a, &rig);
    if a.dry_run {
        return Ok(
            json!({"dry_run": true, "sent": [], "plan": plan, "steps_w": steps, "dwell_s": dwell, "total_keyed_seconds": total, "note": CANNOT_KNOW}),
        );
    }
    if !a.confirm {
        return Err(format!("this tool transmits in {} steps up to {} W on {:.0} Hz. Call it with confirm_dummy_load: true only when the person at the bench has said the rig's antenna port is on the dummy load; dry_run: true shows the plan. {CANNOT_KNOW}", steps.len(), steps[steps.len() - 1], a.frequency_hz));
    }
    if a.reason.is_empty() {
        return Err("reason is required: who is asking and why (up to 120 characters)".into());
    }
    let dir = rigd::dir(&inst.hamlib);
    let gate_report = gate(c, inst, deps, &a).inspect_err(|e| {
        tx::log_event(
            &dir,
            &inst.name,
            "refused",
            json!({"tool": "bench_hamlib_tx_power_sweep", "reason": a.reason, "why": e}),
        )
    })?;
    let alert = operator_alert(tx_cfg, deps, &a, &inst.name)?;
    let result = (|| -> Result<(Value, Vec<Value>), String> {
        set_verified(
            c,
            &format!("set_freq {:.0}", a.frequency_hz),
            "get_freq",
            Some("Frequency"),
            a.frequency_hz,
            1.0,
            "setting the frequency",
        )?;
        c.command(&format!("set_mode {} 0", a.mode))
            .map_err(|e| format!("setting the mode: {e}"))?;
        let probe_report = probe(c, inst, deps, &rig, &a, "bench_hamlib_tx_power_sweep")?;
        let mut out = vec![];
        for w in &steps {
            let f = watts_to_fraction(c, *w, a.frequency_hz, &a.mode)?;
            let sa = TestArgs {
                power_w: *w,
                ..a.clone()
            };
            let run = keyed(
                c,
                inst,
                deps,
                &rig,
                &sa,
                dwell,
                f.max(rig.f_min),
                "sweep step",
                "bench_hamlib_tx_power_sweep",
            )?;
            out.push(json!({"power_w": w, "setting": f, "meters": summary(&run.samples), "aborted": run.aborted, "ptt_read_back_off": run.ptt_read_back}));
            if run.aborted.is_some() {
                break;
            }
        }
        Ok((probe_report, out))
    })();
    let not_restored = restore(c, &rig);
    let (probe_report, out) = result.inspect_err(|e| {
        tx::log_event(
            &dir,
            &inst.name,
            "stopped",
            json!({"tool": "bench_hamlib_tx_power_sweep", "reason": a.reason, "why": e}),
        )
    })?;
    Ok(
        json!({"plan": plan, "load_check": gate_report, "operator_alert": alert, "swr_probe": probe_report, "steps": out, "rig_settings_not_restored": not_restored, "keyed_interval_log": tx::log_file(&dir, &inst.name), "note": CANNOT_KNOW}),
    )
}

/// `bench_hamlib_safe_state`: PTT off and read back; the power setting to the minimum unless told not to. Does not wait for any other
/// call to the rig, so it works while a keyed interval is running (and ends it).
pub fn safe_state(c: &mut Client, inst: &Instrument, args: &Value) -> Result<Value, String> {
    // [@ANCHOR: hamlib_tx_safe_state]
    let lower = match args.get("lower_power") {
        None | Some(Value::Null) => true,
        Some(Value::Bool(b)) => *b,
        Some(_) => return Err("lower_power must be true or false".into()),
    };
    let dir = rigd::dir(&inst.hamlib);
    let was_active = tx::is_active(&inst.name);
    let dump = tolerate(c.dump_state())?.ok();
    let min = dump.as_ref().and_then(|d| rfpower_min(&d.lines));
    let before = tolerate(c.read("get_ptt"))?
        .ok()
        .map(|r| r.kv("PTT").is_some_and(|p| p != "0"));
    let prev_power = tolerate(c.read_number("get_level RFPOWER", None))?.ok();
    let mut last = String::new();
    let mut verified = None;
    for _ in 0..3 {
        match c.command("set_ptt 0") {
            Err(e) => last = e.to_string(),
            Ok(_) => match c.command("get_ptt") {
                Ok(r) if r.kv("PTT") == Some("0") => {
                    verified = Some(true);
                    break;
                }
                Ok(r) => last = format!("PTT still reads {:?}", r.kv("PTT")),
                Err(e) if e.unsupported() => {
                    verified = Some(false);
                    break;
                }
                Err(e) => last = e.to_string(),
            },
        }
    }
    let Some(read_back) = verified else {
        let fresh = super::guard::unkey(&host_port(inst)?.0, host_port(inst)?.1, min)
            .map_err(|e| format!("{e}; {last}"));
        return match fresh {
            Ok(_) => Ok(json!({"ptt": "receive", "verified": true, "via": "a new connection"})),
            Err(e) => Err(format!("SAFE STATE NOT CONFIRMED: {e}. The rig may be transmitting. Do this now: switch the rig off at its front panel or pull its power or its antenna feed. Do not use it again until the owner has looked at it.")),
        };
    };
    let mut lowered = None;
    if lower {
        if let Some(m) = min {
            lowered = Some(c.command(&format!("set_level RFPOWER {m}")).is_ok());
        }
    }
    tx::log_event(
        &dir,
        &inst.name,
        "safe_state",
        json!({"ptt_before": before, "previous_rfpower": prev_power, "lowered": lowered}),
    );
    Ok(json!({
        "keyed_interval_was_running_here": was_active,
        "ptt": "receive", "verified": read_back, "ptt_was": match before { Some(true) => "on", Some(false) => "off", None => "unknown" },
        "power_setting_before": prev_power, "power_set_to_minimum": lowered.map(|_| min), "power_lowered_ok": lowered,
        "note": if read_back { "the rig reads PTT off" } else { "set_ptt 0 was accepted, but this rig cannot report PTT, so it cannot be read back: look at the rig" },
    }))
}
