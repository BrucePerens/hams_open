// SPDX-License-Identifier: AGPL-3.0-or-later
//! DG8SAQ VNWA 3E vector network analyser through the `vnwa_open` crate: USB control transfers to the unit and the audio of its two
//! codecs, no vendor software. The sweep tool puts RF on the TX port, so it needs tier `read_config`. Results are raw
//! (uncalibrated) ratios: S21 from the RX channel and S11 from the reflection channel, in the vendor program's convention.

use serde_json::{json, Value};
use std::path::Path;
use vnwa_open::sweep::{check, Config, Point, Vna};

/// The most points one call may take (each is about 0.2 s).
const MAX_POINTS: usize = 401;
/// The unit's range: the vendor program sweeps up to 1.3 GHz.
const MAX_HZ: f64 = 1.3e9;

#[derive(Debug)]
pub struct SweepArgs {
    pub start_hz: f64,
    pub stop_hz: f64,
    pub points: usize,
    pub config: Config,
}

fn number(args: &Value, key: &str) -> Result<Option<f64>, String> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v
            .as_f64()
            .filter(|x| x.is_finite())
            .map(Some)
            .ok_or(format!("{key} must be a number")),
    }
}

/// The arguments of one sweep call on top of the instrument's configured settings.
pub fn sweep_args(args: &Value, base: &Config) -> Result<SweepArgs, String> {
    // [@ANCHOR: bench_mcp_vna_sweep_args]
    let start_hz = number(args, "start_hz")?.ok_or("start_hz is required")?;
    let stop_hz = number(args, "stop_hz")?.unwrap_or(start_hz);
    let points = match number(args, "points")? {
        None => 1,
        Some(p) if p.fract() == 0.0 && (1.0..=MAX_POINTS as f64).contains(&p) => p as usize,
        Some(p) => {
            return Err(format!(
                "points {p} must be a whole number from 1 to {MAX_POINTS}"
            ))
        }
    };
    for (name, f) in [("start_hz", start_hz), ("stop_hz", stop_hz)] {
        if !(f > 0.0 && f <= MAX_HZ) {
            return Err(format!("{name} {f} is outside 0 to {MAX_HZ} Hz"));
        }
    }
    let mut config = base.clone();
    if let Some(x) = number(args, "if_hz")? {
        config.if_hz = x;
    }
    if let Some(x) = number(args, "point_ms")? {
        config.point_ms = x;
    }
    if let Some(x) = number(args, "rf_level")? {
        if x.fract() != 0.0 || !(0.0..=16383.0).contains(&x) {
            return Err(format!(
                "rf_level {x} must be a whole number from 0 to 16383"
            ));
        }
        config.rf_level = x as u16;
    }
    check(&config)?;
    Ok(SweepArgs {
        start_hz,
        stop_hz,
        points,
        config,
    })
}

fn db(c: vnwa_open::audio::Complex) -> f64 {
    20.0 * c.abs().log10()
}

/// The CSV text of a sweep: frequency, then S21 and S11 as real and imaginary parts, dB and degrees.
pub fn csv(points: &[Point]) -> String {
    // [@ANCHOR: bench_mcp_vna_csv]
    let mut out =
        String::from("freq_hz,s21_re,s21_im,s21_db,s21_deg,s11_re,s11_im,s11_db,s11_deg\n");
    for p in points {
        out.push_str(&format!(
            "{},{:.6},{:.6},{:.3},{:.3},{:.6},{:.6},{:.3},{:.3}\n",
            p.freq_hz,
            p.s21.re,
            p.s21.im,
            db(p.s21),
            p.s21.phase_deg(),
            p.s11.re,
            p.s11.im,
            db(p.s11),
            p.s11.phase_deg()
        ));
    }
    out
}

pub fn identify(cfg: &Config) -> Value {
    json!({
        "idn": format!("DG8SAQ VNWA ({:04x}:{:04x}) driven by vnwa_open", vnwa_open::VID, vnwa_open::PID),
        "codec_s11": cfg.codec_s11,
        "codec_s21": cfg.codec_s21,
        "hub": cfg.hub.as_ref().map(|h| format!("{} port {}", h.hub, h.port)),
    })
}

/// What is connected, and the settings a sweep starts from.
pub fn status(cfg: &Config) -> Result<Value, String> {
    let usb = vnwa_open::usb::Vnwa::open()
        .map(|_| "present")
        .unwrap_or("not found or no permission");
    let found = vnwa_open::topology::codecs();
    let (s11, s21) = match (&cfg.codec_s11, &cfg.codec_s21, &found) {
        (Some(a), Some(b), _) => (Some(a.clone()), Some(b.clone())),
        (_, _, Ok(c)) => (
            cfg.codec_s11.clone().or(Some(c.s11.clone())),
            cfg.codec_s21.clone().or(Some(c.s21.clone())),
        ),
        _ => (None, None),
    };
    Ok(json!({
        "usb": usb,
        "codec_s11": s11,
        "codec_s21": s21,
        "codecs_error": found.err(),
        "if_hz": cfg.if_hz,
        "point_ms": cfg.point_ms,
        "rf_level": cfg.rf_level,
        "calibrated": false,
    }))
}

pub fn sweep(cfg: &Config, args: &Value, out_dir: &Path) -> Result<Value, String> {
    let a = sweep_args(args, cfg)?;
    let mut vna = Vna::open(a.config.clone())?;
    let points = vna.sweep(a.start_hz, a.stop_hz, a.points)?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let file = out_dir.join(format!("vna_sweep_{stamp}.csv"));
    std::fs::write(&file, csv(&points)).map_err(|e| format!("{}: {e}", file.display()))?;
    let (lo, hi) = points.iter().fold((f64::MAX, f64::MIN), |(lo, hi), p| {
        (lo.min(db(p.s21)), hi.max(db(p.s21)))
    });
    let rows: Vec<Value> = if points.len() <= 51 {
        points
            .iter()
            .map(|p| json!({"freq_hz": p.freq_hz, "s21_db": db(p.s21), "s21_deg": p.s21.phase_deg(), "s11_db": db(p.s11), "s11_deg": p.s11.phase_deg()}))
            .collect()
    } else {
        Vec::new()
    };
    Ok(json!({
        "points": points.len(),
        "start_hz": a.start_hz,
        "stop_hz": a.stop_hz,
        "if_hz": a.config.if_hz,
        "calibrated": false,
        "note": "raw ratios of the unit's own channels (cable, bridge and front-end not removed); S21 from the RX channel, S11 from the reflection channel",
        "s21_db_min": lo,
        "s21_db_max": hi,
        "recovery": {"remeasured": vna.recovery().remeasured, "transfer_resends": vna.recovery().transfer_resends, "power_cycles": vna.recovery().power_cycles},
        "max_if_error_hz": points.iter().map(|p| p.if_error_hz).fold(0.0, f64::max),
        "max_level_change": points.iter().map(|p| p.level_change).fold(0.0, f64::max),
        "file": file.display().to_string(),
        "rows": rows,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use vnwa_open::audio::Complex;

    // Tests [@ANCHOR: bench_mcp_vna_sweep_args]
    #[test]
    fn sweep_arguments_are_checked_and_override_the_instrument_settings() {
        let base = Config::default();
        let a = sweep_args(
            &json!({"start_hz": 1e6, "stop_hz": 100e6, "points": 11, "if_hz": 4000}),
            &base,
        )
        .unwrap();
        assert_eq!(
            (a.start_hz, a.stop_hz, a.points, a.config.if_hz),
            (1e6, 100e6, 11, 4000.0)
        );
        assert_eq!(
            sweep_args(&json!({"start_hz": 5e6}), &base).unwrap().points,
            1
        );
        for bad in [
            json!({}),
            json!({"start_hz": 0}),
            json!({"start_hz": 2e9}),
            json!({"start_hz": 1e6, "stop_hz": -5.0}),
            json!({"start_hz": 1e6, "points": 0}),
            json!({"start_hz": 1e6, "points": 402}),
            json!({"start_hz": 1e6, "points": 2.5}),
            json!({"start_hz": 1e6, "if_hz": 7000}),
            json!({"start_hz": 1e6, "point_ms": 0.01}),
            json!({"start_hz": 1e6, "rf_level": 20000}),
            json!({"start_hz": "1e6"}),
        ] {
            assert!(sweep_args(&bad, &base).is_err(), "{bad}");
        }
    }

    // Tests [@ANCHOR: bench_mcp_vna_csv]
    #[test]
    fn the_csv_has_a_header_and_one_row_per_point() {
        let p = Point {
            if_error_hz: 0.0,
            level_change: 0.0,
            freq_hz: 10e6,
            s21: Complex { re: -0.5, im: 0.0 },
            s11: Complex { re: 0.0, im: 1.0 },
        };
        let text = csv(&[p, p]);
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 3);
        assert!(lines[0].starts_with("freq_hz,s21_re"));
        assert_eq!(
            lines[1],
            "10000000,-0.500000,0.000000,-6.021,180.000,0.000000,1.000000,0.000,90.000"
        );
    }
}
