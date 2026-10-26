// SPDX-License-Identifier: AGPL-3.0-or-later
//! Abestop ATM6301 bench supply (0-60 V, 0-10 A) over its CH340 serial port, SCPI at 115200 8N1.
//! The configured `max_volts` and `max_amps` cap every setpoint, and the output is switched on only
//! if the setpoints already stored in the supply are within those caps.

use crate::link::Link;
use serde_json::{json, Value};

pub struct Limits {
    pub max_volts: f64,
    pub max_amps: f64,
}

fn is_on(reply: &str) -> bool {
    matches!(reply.trim().to_ascii_uppercase().as_str(), "ON" | "1")
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

/// Switch the output. Off is always allowed. On is refused unless the setpoints stored in the
/// supply are within the caps, because they may have been left there by the front panel.
pub fn output(l: &mut Link, on: bool, lim: &Limits) -> Result<Value, String> {
    if on {
        let (v, a) = (l.number("VOLT?")?, l.number("CURR?")?);
        if v > lim.max_volts || a > lim.max_amps {
            return Err(format!(
                "refused: the supply is set to {v} V / {a} A, above the caps {} V / {} A. Set lower values with bench_set first.",
                lim.max_volts, lim.max_amps
            ));
        }
    }
    l.send(if on { "OUTP ON" } else { "OUTP OFF" })?;
    std::thread::sleep(std::time::Duration::from_millis(500));
    state(l)
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

    #[test]
    fn output_reply_spellings() {
        assert!(is_on("ON") && is_on("1") && is_on(" on\r"));
        assert!(!is_on("OFF") && !is_on("0"));
    }
}
