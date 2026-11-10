// SPDX-License-Identifier: AGPL-3.0-or-later
//! `bench_vna_time_domain`: the time-domain transform and the gating of a sweep made earlier by `bench_vna_sweep`. It reads the sweep's
//! CSV from the output directory, so it needs no unit and sends nothing to one.

use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use vnwa_open::audio::{vendor_convention, Complex};
use vnwa_open::timedomain::{gate, peak_time, transform, Mode, Response, Window};

/// Speed of light, m/s.
const C: f64 = 299_792_458.0;

// [@ANCHOR: bench_vna_time_text]
fn text<'a>(args: &'a Value, key: &str, default: &'a str) -> Result<&'a str, String> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(default),
        Some(v) => v.as_str().ok_or(format!("{key} must be text")),
    }
}

// [@ANCHOR: bench_vna_time_number]
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

/// The frequencies and the chosen trace of a sweep CSV from `bench_vna_sweep`, in the standard phase convention.
// [@ANCHOR: bench_vna_time_read_sweep]
fn read_sweep(file: &Path, trace: &str) -> Result<(Vec<f64>, Vec<Complex>), String> {
    let t = std::fs::read_to_string(file).map_err(|e| format!("{}: {e}", file.display()))?;
    let mut lines = t.lines();
    let head: Vec<&str> = lines.next().unwrap_or("").split(',').collect();
    let col = |n: &str| {
        head.iter().position(|h| *h == n).ok_or(format!(
            "{} has no {n} column; use the CSV that bench_vna_sweep wrote (not the calibrated one)",
            file.display()
        ))
    };
    let (cf, cr, ci) = (
        col("freq_hz")?,
        col(&format!("{trace}_re"))?,
        col(&format!("{trace}_im"))?,
    );
    let (mut f, mut z) = (Vec::new(), Vec::new());
    for (n, line) in lines.enumerate() {
        let c: Vec<&str> = line.split(',').collect();
        let get = |i: usize| -> Result<f64, String> {
            c.get(i)
                .and_then(|s| s.parse::<f64>().ok())
                .filter(|x| x.is_finite())
                .ok_or(format!(
                    "{}: line {} is not a sweep row",
                    file.display(),
                    n + 2
                ))
        };
        f.push(get(cf)?);
        z.push(vendor_convention(Complex {
            re: get(cr)?,
            im: get(ci)?,
        }));
    }
    Ok((f, z))
}

/// A sweep file name inside the output directory, never a path that leaves it.
// [@ANCHOR: bench_vna_time_file]
fn in_out_dir(out: &Path, name: &str) -> Result<PathBuf, String> {
    let n = Path::new(name);
    if n.file_name().map(Path::new) != Some(n) || !name.ends_with(".csv") {
        return Err(format!(
            "file {name:?} must be the name of a .csv sweep file in the output directory ({})",
            out.display()
        ));
    }
    Ok(out.join(n))
}

/// The tool: impulse or step response of S11 or S21 of a stored sweep, optionally with a gate applied and the gated response written.
pub fn time_domain(args: &Value, out: &Path) -> Result<Value, String> {
    // [@ANCHOR: bench_vna_time_domain]
    let file = in_out_dir(
        out,
        args.get("file")
            .and_then(Value::as_str)
            .ok_or("file is required: the name of a CSV that bench_vna_sweep wrote")?,
    )?;
    let trace = text(args, "trace", "s11")?;
    if !matches!(trace, "s11" | "s21") {
        return Err("trace must be s11 or s21".into());
    }
    let window = Window::from_name(text(args, "window", "hamming")?)?;
    let mode = match text(args, "mode", "lowpass")? {
        "lowpass" => Mode::LowPass,
        "bandpass" => Mode::BandPass,
        o => return Err(format!("mode {o:?} must be lowpass or bandpass")),
    };
    let response = match text(args, "response", "impulse")? {
        "impulse" => Response::Impulse,
        "step" => Response::Step,
        o => return Err(format!("response {o:?} must be impulse or step")),
    };
    let (freqs, data) = read_sweep(&file, trace)?;
    if freqs.len() < 4 {
        return Err("the sweep has fewer than four points".into());
    }
    let step = (freqs[freqs.len() - 1] - freqs[0]) / (freqs.len() - 1) as f64;
    let n = number(args, "time_points")?.unwrap_or(512.0);
    if !(8.0..=8192.0).contains(&n) || n.fract() != 0.0 {
        return Err("time_points must be a whole number from 8 to 8192".into());
    }
    let n = n as usize;
    let t_max = number(args, "t_max_s")?.unwrap_or(0.5 / step);
    if t_max <= 0.0 {
        return Err("t_max_s must be positive".into());
    }
    let dt = t_max / (n - 1) as f64;
    let times: Vec<f64> = (0..n).map(|i| i as f64 * dt).collect();
    let h = transform(&freqs, &data, window, mode, response, &times)?;
    let peak = peak_time(&times, &h).ok_or("no peak in the time response")?;
    let peak_mag = h.iter().map(|z| z.abs()).fold(0.0, f64::max);
    let stamp = file.file_stem().and_then(|s| s.to_str()).unwrap_or("sweep");
    let tfile = out.join(format!("{stamp}_{trace}_time.csv"));
    let mut t = String::from("time_s,re,im,magnitude\n");
    for (tm, z) in times.iter().zip(&h) {
        t += &format!("{tm:e},{:.6e},{:.6e},{:.6e}\n", z.re, z.im, z.abs());
    }
    std::fs::write(&tfile, t).map_err(|e| format!("{}: {e}", tfile.display()))?;
    let mut res = json!({
        "trace": trace, "window": format!("{window:?}"), "mode": format!("{mode:?}"), "response": format!("{response:?}"),
        "points": freqs.len(), "step_hz": step, "time_resolution_s": dt, "unaliased_until_s": 1.0 / step,
        "peak_time_s": peak, "peak_magnitude": peak_mag, "time_file": tfile.display().to_string(),
        "note": "delay is the round trip for a reflection (S11); the transform uses the sweep as measured, so a calibrated sweep is what gives true reflection sizes",
    });
    if let Some(vf) = number(args, "velocity_factor")? {
        if !(0.1..=1.0).contains(&vf) {
            return Err("velocity_factor must be from 0.1 to 1".into());
        }
        let round_trip = if trace == "s11" { 2.0 } else { 1.0 };
        res["distance_m"] = json!(C * vf * peak / round_trip);
    }
    let (g0, g1) = (number(args, "gate_start_s")?, number(args, "gate_stop_s")?);
    match (g0, g1) {
        (None, None) => {}
        (Some(a), Some(b)) => {
            let shape = Window::from_name(text(args, "gate_shape", "rectangular")?)?;
            let g = gate(&freqs, &data, mode, a, b, shape)?;
            let gfile = out.join(format!("{stamp}_{trace}_gated.csv"));
            let mut t = String::from("freq_hz,re,im,db,deg\n");
            for (f, z) in freqs.iter().zip(&g) {
                // back to the unit's convention, the one bench_vna_sweep writes
                let v = vendor_convention(*z);
                t += &format!(
                    "{f},{:.6},{:.6},{:.3},{:.3}\n",
                    v.re,
                    v.im,
                    20.0 * v.abs().max(1e-12).log10(),
                    v.im.atan2(v.re).to_degrees()
                );
            }
            std::fs::write(&gfile, t).map_err(|e| format!("{}: {e}", gfile.display()))?;
            res["gated_file"] = json!(gfile.display().to_string());
            res["gate"] = json!({"start_s": a, "stop_s": b, "shape": format!("{shape:?}")});
        }
        _ => return Err("gate_start_s and gate_stop_s go together".into()),
    }
    Ok(res)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::PI;

    /// A sweep file as bench_vna_sweep writes it: a delay of `tau` on S11 (0.5) and a through on S21, in the unit's phase convention.
    fn write_sweep(dir: &Path, name: &str, tau: f64) {
        let mut t =
            String::from("freq_hz,s21_re,s21_im,s21_db,s21_deg,s11_re,s11_im,s11_db,s11_deg\n");
        for k in 1..=100 {
            let f = k as f64 * 2e6;
            // the unit's convention is the conjugate of e^(-j 2 pi f tau)
            let (re, im) = (
                0.5 * (2.0 * PI * f * tau).cos(),
                0.5 * (2.0 * PI * f * tau).sin(),
            );
            t += &format!("{f},1,0,0,0,{re},{im},0,0\n");
        }
        std::fs::write(dir.join(name), t).unwrap();
    }

    // Tests [@ANCHOR: bench_vna_time_domain]
    // Tests [@ANCHOR: bench_vna_time_text]
    // Tests [@ANCHOR: bench_vna_time_number]
    // Tests [@ANCHOR: bench_vna_time_read_sweep]
    // Tests [@ANCHOR: bench_vna_time_file]
    #[test]
    fn a_stored_sweep_gives_its_delay_a_distance_and_a_gated_response_and_bad_input_is_refused() {
        let d = crate::testdir::TestDir::new("vna_time");
        write_sweep(d.path(), "s.csv", 6e-9);
        let r = time_domain(
            &json!({"file": "s.csv", "t_max_s": 50e-9, "time_points": 201, "velocity_factor": 0.66,
                    "gate_start_s": 0.0, "gate_stop_s": 12e-9, "gate_shape": "hanning"}),
            d.path(),
        )
        .unwrap();
        let peak = r["peak_time_s"].as_f64().unwrap();
        assert!((peak - 6e-9).abs() < 0.3e-9, "{peak}");
        let dist = r["distance_m"].as_f64().unwrap();
        assert!((dist - C * 0.66 * 6e-9 / 2.0).abs() < 0.1, "{dist}");
        assert!(Path::new(r["time_file"].as_str().unwrap()).exists());
        let gated = std::fs::read_to_string(r["gated_file"].as_str().unwrap()).unwrap();
        assert_eq!(gated.lines().count(), 101);
        for (args, want) in [
            (json!({}), "file is required"),
            (json!({"file": "../x.csv"}), "output directory"),
            (json!({"file": "s.csv", "trace": "s22"}), "s11 or s21"),
            (
                json!({"file": "s.csv", "window": "kaiser"}),
                "unknown window",
            ),
            (json!({"file": "s.csv", "mode": "x"}), "lowpass or bandpass"),
            (
                json!({"file": "s.csv", "gate_start_s": 1e-9}),
                "go together",
            ),
            (json!({"file": "s.csv", "time_points": 2}), "time_points"),
            (json!({"file": "absent.csv"}), "absent.csv"),
        ] {
            let e = time_domain(&args, d.path()).unwrap_err();
            assert!(e.contains(want), "{args}: {e}");
        }
    }
}
