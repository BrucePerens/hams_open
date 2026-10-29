// SPDX-License-Identifier: AGPL-3.0-or-later
//! Abestop ATM6301 bench supply (0-60 V, 0-10 A) over its CH340 serial port, SCPI at 115200 8N1.
//! The configured `max_volts` and `max_amps` cap every setpoint, and the output is switched on only
//! if the setpoints already stored in the supply are within those caps. Switching the output on is a `full`
//! tool that needs `dry_run` or `confirm_load_connected`, and this process switches it off again after `for_s`
//! seconds (at most the configured `max_on_s`): the supply has no output timer of its own. The safe state
//! (output off, read back) is `bench_psu_safe_state`.

use crate::link::Link;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub struct Limits {
    pub max_volts: f64,
    pub max_amps: f64,
    /// The longest the output may stay on after one `bench_psu_output` call, in seconds.
    pub max_on_s: f64,
}

/// Tries at switching off and reading it back, before the failure is reported as an unconfirmed safe state.
const OFF_TRIES: usize = 3;

fn is_on(reply: &str) -> bool {
    matches!(reply.trim().to_ascii_uppercase().as_str(), "ON" | "1")
}

fn is_off(reply: &str) -> bool {
    // [@ANCHOR: bench_mcp_psu_is_off]
    matches!(reply.trim().to_ascii_uppercase().as_str(), "OFF" | "0")
}

pub fn state(l: &mut Link) -> Result<Value, String> {
    Ok(json!({
        "output": if is_on(&l.query("OUTP?")?) { "on" } else { "off" },
        "set_voltage_v": l.number("VOLT?")?,
        "set_current_a": l.number("CURR?")?,
        "ovp_v": l.number("VOLT:LIM?")?,
        "ocp_a": l.number("CURR:LIM?")?,
        "measured_voltage_v": l.number("MEAS:VOLT?")?,
        "measured_current_a": l.number("MEAS:CURR?")?,
        "error": l.query("SYST:ERR?")?,
    }))
}

pub fn measure(l: &mut Link) -> Result<Value, String> {
    let v = l.number("MEAS:VOLT?")?;
    let i = l.number("MEAS:CURR?")?;
    let p = l.number("MEAS:POW?")?;
    Ok(json!({"voltage_v": v, "current_a": i, "power_w": p, "info": l.query("MEAS:ALL:INFO?")?}))
}

fn in_range(name: &str, v: f64, max: f64) -> Result<f64, String> {
    // [@ANCHOR: bench_mcp_psu_caps]
    if v.is_finite() && (0.0..=max).contains(&v) {
        Ok(v)
    } else {
        Err(format!(
            "{name} {v} is outside 0..={max} (the cap in the server's config)"
        ))
    }
}

pub fn set(l: &mut Link, args: &Value, lim: &Limits) -> Result<Value, String> {
    let mut sent = Vec::new();
    if let Some(v) = args.get("voltage_v").and_then(|v| v.as_f64()) {
        let v = in_range("voltage_v", v, lim.max_volts)?;
        l.send(&format!("VOLT {v:.3}"))?;
        sent.push(format!("VOLT {v:.3}"));
    }
    if let Some(a) = args.get("current_a").and_then(|v| v.as_f64()) {
        let a = in_range("current_a", a, lim.max_amps)?;
        l.send(&format!("CURR {a:.3}"))?;
        sent.push(format!("CURR {a:.3}"));
    }
    if sent.is_empty() {
        return Err("give voltage_v and/or current_a".into());
    }
    std::thread::sleep(std::time::Duration::from_millis(300));
    Ok(json!({"sent": sent, "state": state(l)?}))
}

/// Switches the output off and reads it back, up to `OFF_TRIES` times. Ok only when the supply itself says off.
pub fn safe_state(l: &mut Link, timer: &Timer) -> Result<Value, String> {
    // [@ANCHOR: bench_mcp_psu_safe_state]
    // An explicit off cancels a pending timed switch-off from an earlier "on": it has nothing left to do.
    timer.generation.fetch_add(1, Ordering::SeqCst);
    let mut last = String::new();
    for _ in 0..OFF_TRIES {
        if let Err(e) = l.send("OUTP OFF") {
            last = e;
            continue;
        }
        std::thread::sleep(Duration::from_millis(300));
        match l.query("OUTP?") {
            Ok(r) if is_off(&r) => return Ok(json!({"output": "off", "verified": true})),
            Ok(r) => last = format!("it answered {r:?}"),
            Err(e) => last = e,
        }
    }
    Err(format!(
        "SAFE STATE NOT CONFIRMED: the supply output may still be ON ({last}). Do this now: press the output button on the supply or switch off its mains. Tell the owner before using it again."
    ))
}

/// How long the output may stay on: `for_s` from the arguments, else the whole `max_on_s`; never above it.
pub fn on_seconds(args: &Value, lim: &Limits) -> Result<f64, String> {
    // [@ANCHOR: bench_mcp_psu_on_seconds]
    match args.get("for_s") {
        None | Some(Value::Null) => Ok(lim.max_on_s),
        Some(v) => {
            let s = v.as_f64().ok_or("for_s must be a number of seconds")?;
            if s.is_finite() && (1.0..=lim.max_on_s).contains(&s) {
                Ok(s)
            } else {
                Err(format!(
                    "for_s {s} is outside 1..={} (max_on_s in the server's config)",
                    lim.max_on_s
                ))
            }
        }
    }
}

/// Refuses unless the setpoints stored in the supply are within the caps (a front-panel value may have been left there).
fn check_stored_setpoints(l: &mut Link, lim: &Limits) -> Result<(f64, f64), String> {
    // [@ANCHOR: bench_mcp_psu_stored_setpoints]
    let (v, a) = (l.number("VOLT?")?, l.number("CURR?")?);
    if !(v.is_finite() && a.is_finite()) || v > lim.max_volts || a > lim.max_amps {
        return Err(format!(
            "refused: the supply is set to {v} V / {a} A, above the caps {} V / {} A. Set lower values with bench_set first.",
            lim.max_volts, lim.max_amps
        ));
    }
    Ok((v, a))
}

/// What `bench_psu_output` would do, from the supply's own stored setpoints. Reads only; nothing is switched or set.
pub fn plan_on(l: &mut Link, lim: &Limits, for_s: f64) -> Result<Value, String> {
    // [@ANCHOR: bench_mcp_psu_plan_on]
    let stored = check_stored_setpoints(l, lim);
    let (volts, amps, refusal) = match &stored {
        Ok((v, a)) => (Some(*v), Some(*a), None),
        Err(e) => (None, None, Some(e.clone())),
    };
    Ok(
        json!({"dry_run": true, "sent": [], "would_be_refused": refusal,
        "stored_voltage_v": volts, "stored_current_a": amps,
        "would": ["OUTP ON", format!("OUTP OFF by this server after {for_s} s"), "OUTP? (must read OFF)"],
        "note": "nothing was switched; the output stays as it is. The real call also needs confirm_load_connected: true from the person at the bench."}),
    )
}

/// One supply's switch-off timer state: its instrument lock, and a counter that increases whenever an output
/// command supersedes a pending timed switch-off.
#[derive(Clone, Default)]
pub struct Timer {
    pub lock: Arc<Mutex<()>>,
    pub generation: Arc<AtomicU64>,
}

/// Switches the output on, with the stored setpoints checked first, and arms the switch-off after `for_s`.
/// `reopen` gives the timer its own link to the supply; `timer.lock` is the instrument's lock.
pub fn output_on(
    l: &mut Link,
    lim: &Limits,
    for_s: f64,
    timer: &Timer,
    reopen: impl Fn() -> Result<Link, String> + Send + 'static,
) -> Result<Value, String> {
    // [@ANCHOR: bench_mcp_psu_output_on]
    check_stored_setpoints(l, lim)?;
    let generation = timer.generation.fetch_add(1, Ordering::SeqCst) + 1;
    l.send("OUTP ON")?;
    arm_switch_off(generation, for_s, timer.clone(), reopen);
    std::thread::sleep(Duration::from_millis(500));
    let mut v = state(l)?;
    v["off_after_s"] = json!(for_s);
    Ok(v)
}

fn arm_switch_off(
    generation: u64,
    secs: f64,
    timer: Timer,
    reopen: impl Fn() -> Result<Link, String> + Send + 'static,
) {
    // [@ANCHOR: bench_mcp_psu_arm_switch_off]
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs_f64(secs));
        let _held = timer.lock.lock().unwrap_or_else(|e| e.into_inner());
        if timer.generation.load(Ordering::SeqCst) != generation {
            return;
        }
        let mut last = String::new();
        for _ in 0..OFF_TRIES {
            match reopen().and_then(|mut l| safe_state(&mut l, &timer)) {
                Ok(_) => return,
                Err(e) => last = e,
            }
            std::thread::sleep(Duration::from_secs(2));
        }
        eprintln!(
            "bench_mcp: the supply output could not be switched off after its time limit: {last}"
        );
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    // Tests [@ANCHOR: bench_mcp_psu_caps]
    #[test]
    fn caps_reject_out_of_range_and_non_finite() {
        assert_eq!(in_range("v", 5.0, 5.5), Ok(5.0));
        assert!(in_range("v", 5.6, 5.5).is_err());
        assert!(in_range("v", -0.1, 5.5).is_err());
        assert!(in_range("v", f64::NAN, 5.5).is_err());
        assert!(in_range("v", f64::INFINITY, 5.5).is_err());
    }

    /// A supply that remembers its output state; `stuck_on` ignores `OUTP OFF`.
    fn fake_supply(volts: f64, stuck_on: bool) -> crate::example::testkit::FakeDevice {
        let on = std::sync::Mutex::new(false);
        crate::example::testkit::FakeDevice::start(move |c| {
            let mut on = on.lock().unwrap();
            match c {
                "OUTP ON" => *on = true,
                "OUTP OFF" if !stuck_on => *on = false,
                _ => {}
            }
            Some(match c {
                "OUTP?" => (if *on { "ON" } else { "OFF" }).to_string(),
                "VOLT?" => format!("{volts}"),
                "CURR?" => "0.5".to_string(),
                c if c.ends_with('?') => "0".to_string(),
                _ => return None,
            })
        })
    }

    fn lim() -> Limits {
        Limits {
            max_volts: 5.5,
            max_amps: 1.0,
            max_on_s: 600.0,
        }
    }

    fn count(dev: &crate::example::testkit::FakeDevice, cmd: &str) -> usize {
        dev.received().iter().filter(|c| c.as_str() == cmd).count()
    }

    // Tests [@ANCHOR: bench_mcp_psu_safe_state]
    #[test]
    fn safe_state_reads_off_back_and_fails_loudly_when_the_supply_stays_on() {
        let ok = fake_supply(3.3, false);
        let mut l = ok.link()().unwrap();
        let v = safe_state(&mut l, &Timer::default()).unwrap();
        assert_eq!(v["verified"], true);
        let stuck = fake_supply(3.3, true);
        let mut l = stuck.link()().unwrap();
        l.send("OUTP ON").unwrap();
        let e = safe_state(&mut l, &Timer::default()).unwrap_err();
        assert!(
            e.contains("SAFE STATE NOT CONFIRMED") && e.contains("mains"),
            "{e}"
        );
        assert_eq!(count(&stuck, "OUTP OFF"), OFF_TRIES);
    }

    // Tests [@ANCHOR: bench_mcp_psu_on_seconds]
    #[test]
    fn the_time_limit_defaults_to_the_cap_and_never_exceeds_it() {
        assert_eq!(on_seconds(&json!({}), &lim()), Ok(600.0));
        assert_eq!(on_seconds(&json!({"for_s": 30}), &lim()), Ok(30.0));
        for bad in [json!(0.5), json!(601), json!(-1), json!("5")] {
            assert!(on_seconds(&json!({"for_s": bad}), &lim()).is_err(), "{bad}");
        }
    }

    // Tests [@ANCHOR: bench_mcp_psu_stored_setpoints]
    // Tests [@ANCHOR: bench_mcp_psu_plan_on]
    #[test]
    fn a_dry_run_reads_the_setpoints_and_switches_nothing() {
        let over = fake_supply(12.0, false);
        let mut l = over.link()().unwrap();
        let v = plan_on(&mut l, &lim(), 60.0).unwrap();
        assert_eq!(v["dry_run"], true);
        assert!(v["would_be_refused"].as_str().unwrap().contains("refused"));
        let fine = fake_supply(3.3, false);
        let mut l = fine.link()().unwrap();
        let v = plan_on(&mut l, &lim(), 60.0).unwrap();
        assert!(v["would_be_refused"].is_null());
        assert_eq!(v["stored_voltage_v"], 3.3);
        for d in [&over, &fine] {
            assert!(!d
                .received()
                .iter()
                .any(|c| c.starts_with("OUTP ") && !c.ends_with('?')));
        }
    }

    // Tests [@ANCHOR: bench_mcp_psu_output_on]
    // Tests [@ANCHOR: bench_mcp_psu_arm_switch_off]
    #[test]
    fn the_output_is_refused_over_the_caps_and_switched_off_by_the_timer() {
        let timer = Timer::default();
        let over = fake_supply(12.0, false);
        let mut l = over.link()().unwrap();
        let e = output_on(&mut l, &lim(), 1.0, &timer, over.link()).unwrap_err();
        assert!(e.contains("above the caps"), "{e}");
        assert_eq!(count(&over, "OUTP ON"), 0);

        let dev = fake_supply(3.3, false);
        let mut l = dev.link()().unwrap();
        let v = output_on(&mut l, &lim(), 1.0, &timer, dev.link()).unwrap();
        assert_eq!(v["output"], "on");
        assert_eq!(v["off_after_s"], 1.0);
        std::thread::sleep(Duration::from_millis(2500));
        assert_eq!(count(&dev, "OUTP OFF"), 1, "{:?}", dev.received());
        let mut l = dev.link()().unwrap();
        assert_eq!(l.query("OUTP?").unwrap(), "OFF");
    }

    // Tests [@ANCHOR: bench_mcp_psu_safe_state]
    #[test]
    fn an_explicit_off_cancels_the_pending_timer() {
        let timer = Timer::default();
        let dev = fake_supply(3.3, false);
        let mut l = dev.link()().unwrap();
        output_on(&mut l, &lim(), 1.0, &timer, dev.link()).unwrap();
        safe_state(&mut l, &timer).unwrap();
        std::thread::sleep(Duration::from_millis(2000));
        assert_eq!(count(&dev, "OUTP OFF"), 1, "{:?}", dev.received());
    }

    // Tests [@ANCHOR: bench_mcp_psu_is_off]
    #[test]
    fn output_reply_spellings() {
        assert!(is_off("OFF") && is_off("0\r") && !is_off("ON") && !is_off(""));
        assert!(is_on("ON") && is_on("1") && is_on(" on\r"));
        assert!(!is_on("OFF") && !is_on("0"));
    }
}
