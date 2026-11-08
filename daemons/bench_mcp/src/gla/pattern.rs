// SPDX-License-Identifier: AGPL-3.0-or-later
//! The pattern generator's planner: turns a square wave, PWM, pulse train or bit pattern into the step list `bench_glasgow_pattern`
//! plays, and reports what will really come out. The backend times steps from the host (0.5 ms shortest hold, about a millisecond of
//! jitter), so the planner refuses what that cannot do instead of approximating it silently. It drives nothing itself.
//! Entries: `docs/GLASGOW_LOGIC_TOOLS.md`.

use serde_json::{json, Value};

/// The shortest hold the backend plays, milliseconds.
pub const MIN_HOLD_MS: f64 = 0.5;
/// The longest single hold, milliseconds.
pub const MAX_HOLD_MS: f64 = 5000.0;
/// The most steps one round holds.
pub const MAX_STEPS: usize = 256;
/// The most rounds.
pub const MAX_REPEAT: u64 = 100_000;

/// A planned pattern: `steps` of (pin mask, hold in ms) played `repeat` times; `achieved` says what will come out.
#[derive(Clone, Debug, PartialEq)]
pub struct Plan {
    // [@ANCHOR: gla_pattern_plan_struct]
    pub steps: Vec<(u32, f64)>,
    pub repeat: u64,
    pub total_s: f64,
    pub achieved: Value,
}

fn q(ms: f64) -> f64 {
    // [@ANCHOR: gla_pattern_quantize]
    (ms * 10.0).round() / 10.0
}

fn hold(ms: f64, what: &str) -> Result<f64, String> {
    // [@ANCHOR: gla_pattern_hold]
    let h = q(ms);
    if !ms.is_finite() || h < MIN_HOLD_MS || h > MAX_HOLD_MS {
        return Err(format!("{what} is {ms} ms; the host-timed generator plays holds of {MIN_HOLD_MS} to {MAX_HOLD_MS} ms (a square wave up to about 1 kHz at 50 %). Faster edges need the FPGA-timed generator, which this tool does not have"));
    }
    Ok(h)
}

fn num(spec: &Value, key: &str, lo: f64, hi: f64, def: Option<f64>) -> Result<f64, String> {
    // [@ANCHOR: gla_pattern_num]
    match (spec.get(key).and_then(|v| v.as_f64()), def) {
        (Some(v), _) if v.is_finite() && (lo..=hi).contains(&v) => Ok(v),
        (Some(v), _) => Err(format!("{key} {v} must be {lo} to {hi}")),
        (None, Some(d)) => Ok(d),
        (None, None) => Err(format!("{key} is required")),
    }
}

/// Plans the pattern of `spec` for `npins` driven pins, at most `max_total_s` seconds in all. `type`: `square` or `pwm` (`pin`,
/// `frequency_hz`, `duty` 0..1 default 0.5, and `cycles` or `duration_s`), `pulses` (`pin`, `width_ms`, `gap_ms`, `count`), `bits`
/// (`pin`, `bits` as text of 0 and 1 up to 256, `bit_ms`, `repeat`), `steps` (`steps` of {value, hold_ms}, `repeat`). Pins other than
/// `pin` stay low.
pub fn plan(spec: &Value, npins: usize, max_total_s: f64) -> Result<Plan, String> {
    // [@ANCHOR: gla_pattern_plan]
    let pin = |spec: &Value| -> Result<u32, String> {
        let p = num(spec, "pin", 0.0, 31.0, Some(0.0))? as usize;
        if p >= npins {
            return Err(format!(
                "pin {p} is outside the {npins} driven pins (0 is the first)"
            ));
        }
        Ok(1u32 << p)
    };
    let kind = spec
        .get("type")
        .and_then(|t| t.as_str())
        .ok_or("type is required: square, pwm, pulses, bits or steps")?;
    let (steps, repeat, achieved): (Vec<(u32, f64)>, u64, Value) = match kind {
        "square" | "pwm" => {
            let m = pin(spec)?;
            let f = num(spec, "frequency_hz", 0.1, 2000.0, None)?;
            let duty = num(spec, "duty", 0.0, 1.0, Some(0.5))?;
            let period = 1000.0 / f;
            let (hi, lo) = (
                hold(period * duty, "the high time")?,
                hold(period * (1.0 - duty), "the low time")?,
            );
            let cycles = match spec.get("cycles") {
                Some(_) => num(spec, "cycles", 1.0, MAX_REPEAT as f64, None)? as u64,
                None => (num(spec, "duration_s", 0.001, max_total_s, None)? * 1000.0 / (hi + lo))
                    .floor()
                    .max(1.0) as u64,
            };
            (
                vec![(m, hi), (0, lo)],
                cycles,
                json!({"frequency_hz": 1000.0 / (hi + lo), "duty": hi / (hi + lo), "high_ms": hi, "low_ms": lo, "cycles": cycles}),
            )
        }
        "pulses" => {
            let m = pin(spec)?;
            let (w, g) = (
                hold(num(spec, "width_ms", 0.0, 1e6, None)?, "the pulse width")?,
                hold(num(spec, "gap_ms", 0.0, 1e6, None)?, "the gap")?,
            );
            let n = num(spec, "count", 1.0, MAX_REPEAT as f64, None)? as u64;
            (
                vec![(m, w), (0, g)],
                n,
                json!({"width_ms": w, "gap_ms": g, "pulses": n}),
            )
        }
        "bits" => {
            let m = pin(spec)?;
            let text = spec
                .get("bits")
                .and_then(|b| b.as_str())
                .ok_or("bits is required: text such as \"10110010\"")?;
            if text.is_empty() || text.len() > 256 || !text.chars().all(|c| c == '0' || c == '1') {
                return Err("bits must be 1 to 256 characters of 0 and 1".into());
            }
            let bit_ms = hold(num(spec, "bit_ms", 0.5, MAX_HOLD_MS, None)?, "the bit time")?;
            let mut steps: Vec<(u32, f64)> = vec![];
            for c in text.chars() {
                let v = if c == '1' { m } else { 0 };
                match steps.last_mut() {
                    Some(l) if l.0 == v && l.1 + bit_ms <= MAX_HOLD_MS => l.1 += bit_ms,
                    _ => steps.push((v, bit_ms)),
                }
            }
            let n = num(spec, "repeat", 1.0, MAX_REPEAT as f64, Some(1.0))? as u64;
            (
                steps,
                n,
                json!({"bits": text.len(), "bit_ms": bit_ms, "bit_rate_hz": 1000.0 / bit_ms, "repeat": n}),
            )
        }
        "steps" => {
            let list = spec
                .get("steps")
                .and_then(|s| s.as_array())
                .ok_or("steps is required: a list of {value, hold_ms}")?;
            let mut steps = vec![];
            for s in list {
                let v = num(s, "value", 0.0, ((1u64 << npins) - 1) as f64, None)? as u32;
                steps.push((v, hold(num(s, "hold_ms", 0.0, 1e6, None)?, "a step")?));
            }
            let n = num(spec, "repeat", 1.0, MAX_REPEAT as f64, Some(1.0))? as u64;
            (steps, n, json!({"steps": list.len(), "repeat": n}))
        }
        o => {
            return Err(format!(
                "type {o:?} must be square, pwm, pulses, bits or steps"
            ))
        }
    };
    if steps.is_empty() || steps.len() > MAX_STEPS {
        return Err(format!(
            "{} steps; a round holds 1 to {MAX_STEPS}",
            steps.len()
        ));
    }
    let total_s = steps.iter().map(|s| s.1).sum::<f64>() * repeat as f64 / 1000.0;
    if total_s > max_total_s {
        return Err(format!("the pattern would run {total_s:.2} s; the limit is {max_total_s} s (the config's max_capture_s)"));
    }
    Ok(Plan {
        steps,
        repeat,
        total_s,
        achieved,
    })
}

/// The arguments for `bench_glasgow_pattern`: the pins, the steps and the repeat count; `confirm` and `dry_run` are passed on by the caller.
pub fn backend_args(p: &Plan, pins: &str) -> Value {
    // [@ANCHOR: gla_pattern_backend_args]
    json!({"pins": pins, "steps": p.steps.iter().map(|(v, h)| json!({"value": v, "hold_ms": h})).collect::<Vec<_>>(), "repeat": p.repeat})
}
