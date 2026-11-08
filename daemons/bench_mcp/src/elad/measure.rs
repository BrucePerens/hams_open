// SPDX-License-Identifier: AGPL-3.0-or-later
//! The measuring tools that build on the streaming helpers of the parent module: `bench_elad_tune` (apply a tuning and front-end state
//! without streaming), `bench_elad_detect` (is there a signal near a frequency, how strong against the noise floor) and
//! `bench_elad_sweep` (a band, step by step, to a CSV). Receive only.

use super::*;

/// The usable fraction of the sample rate around the centre (the DDC's filter skirts are not trusted beyond it).
const USABLE: f64 = 0.8;
/// The most steps one sweep may take.
const MAX_STEPS: usize = 400;
/// The FFT length of one sweep step.
const SWEEP_FFT: usize = 1024;
/// Samples per sweep step (eight blocks of the sweep FFT at 50 % overlap are 15 blocks).
const SWEEP_SAMPLES: usize = SWEEP_FFT * 8;
/// Samples dropped after each retune, so the converter's old frequency does not leak into the step.
const SETTLE_SAMPLES: usize = 4096;

/// Applies a frequency, filter and attenuator state and reports what was sent. Streams nothing, so it works on an FDM-S1/S2 whose FPGA
/// image is not loaded.
pub fn tune(bench: &dyn Bench, cfg: &Config, args: &Value) -> Result<Value, String> {
    // [@ANCHOR: bench_mcp_elad_tune]
    let t = open_tuned(bench, cfg, args)?;
    let hz = t.radio.tuned_hz().unwrap_or(0.0);
    let model = t.found.model;
    let word = elad_usb::protocol::tuning_word(hz, model.adc_clock_hz());
    Ok(json!({
        "model": model.name(), "tuned_hz": hz, "tuning_word": word,
        "rate_hz": t.rate, "streaming": false,
        "front_end_switches": model.has_front_end_switches(),
        "calibration_offset_db": t.radio.level_offset_db(),
        "note": "the receiver keeps no tuning between tool calls: the streaming tools take freq_hz, filters and attenuator themselves; this call checks that the receiver accepts the state",
    }))
}

/// The offset of a peak refined below one bin by a parabola through the log power of the bin and its two neighbours.
fn refined_offset_hz(sp: &dsp::Spectrum, offset_hz: f64) -> f64 {
    // [@ANCHOR: elad_refine_peak]
    let n = sp.power_db.len();
    let i = (offset_hz / sp.bin_hz).round() as i64 + (n / 2) as i64;
    if i < 1 || i as usize + 1 >= n {
        return offset_hz;
    }
    let i = i as usize;
    let (a, b, c) = (sp.power_db[i - 1], sp.power_db[i], sp.power_db[i + 1]);
    let denom = a - 2.0 * b + c;
    if denom.abs() < 1e-12 {
        return offset_hz;
    }
    let delta = (0.5 * (a - c) / denom).clamp(-0.5, 0.5);
    sp.offset_hz(i) + delta * sp.bin_hz
}

/// Looks for signals around `freq_hz`: the peaks at least `threshold_db` over the noise floor (the median bin), each with its refined
/// frequency, level in dBFS and SNR in a one-bin bandwidth.
pub fn detect(bench: &dyn Bench, cfg: &Config, args: &Value) -> Result<Value, String> {
    // [@ANCHOR: bench_mcp_elad_detect]
    let n = fft_len(args)?;
    let threshold = number(
        args,
        "threshold_db",
        3.0,
        60.0,
        Some(10.0),
        "the margin over the noise floor",
    )?;
    let max_peaks = number(args, "max_peaks", 1.0, 20.0, Some(5.0), "peaks to report")? as usize;
    let centre = number(args, "freq_hz", 0.0, 1e9, None, "the tuning range")?;
    let found = bench
        .find()?
        .into_iter()
        .next()
        .ok_or_else(|| next_step("no ELAD receiver is on the USB bus"))?;
    let rate = rate_of(args, &found)?;
    let target = (n * 16).min(MAX_SAMPLES).max(n * 2);
    let a = acquire(
        bench,
        cfg,
        args,
        target,
        (target as f64 / f64::from(rate)).max(0.5) + 3.0,
    )?;
    let sp = dsp::welch(&a.samples, n, f64::from(rate))?;
    let floor = dsp::noise_floor_db(&sp);
    let usable = USABLE * f64::from(rate) / 2.0;
    let signals: Vec<Value> = dsp::peaks(&sp, max_peaks * 2, threshold, 3, 3)
        .iter()
        .filter(|p| p.offset_hz.abs() <= usable)
        .take(max_peaks)
        .map(|p| {
            let off = refined_offset_hz(&sp, p.offset_hz);
            json!({"freq_hz": centre + off, "offset_hz": off, "power_dbfs": p.power_db, "snr_db": p.power_db - floor})
        })
        .collect();
    let mut out = summary(&a);
    out["detected"] = json!(!signals.is_empty());
    out["threshold_db"] = json!(threshold);
    out["noise_floor_dbfs"] = json!(floor);
    out["bin_hz"] = json!(sp.bin_hz);
    out["usable_half_span_hz"] = json!(usable);
    out["signals"] = json!(signals);
    out["note"] = json!("snr_db is the peak bin over the median bin (a one-bin bandwidth); the DC bins at the centre are skipped; peaks outside the usable 80 % of the span are ignored");
    Ok(out)
}

/// What a sweep needs from its arguments, checked before the receiver is opened.
struct SweepPlan {
    start: f64,
    stop: f64,
    step: f64,
    centres: Vec<f64>,
}

fn plan(cfg: &Config, args: &Value, rate: u32) -> Result<SweepPlan, String> {
    // [@ANCHOR: elad_sweep_plan]
    let start = number(args, "start_hz", 0.0, 1e9, None, "the tuning range")?;
    let stop = number(args, "stop_hz", 0.0, 1e9, None, "the tuning range")?;
    if stop <= start {
        return Err(format!("stop_hz {stop} must be above start_hz {start}"));
    }
    let max_step = USABLE * f64::from(rate);
    let step = number(
        args,
        "step_hz",
        1.0,
        max_step,
        Some(0.4 * f64::from(rate)),
        "80 % of the sample rate, the usable span",
    )?;
    let count = ((stop - start) / step).ceil() as usize + 1;
    if count > MAX_STEPS {
        return Err(format!(
            "{start} to {stop} Hz in {step} Hz steps is {count} steps; at most {MAX_STEPS} (raise step_hz or rate_hz, or sweep in parts)"
        ));
    }
    let per_step = (SWEEP_SAMPLES + SETTLE_SAMPLES) as f64 / f64::from(rate) + 0.02;
    let budget = 6.0 * cfg.max_capture_s;
    if per_step * count as f64 > budget {
        return Err(format!(
            "{count} steps of about {per_step:.2} s would take {:.0} s; the limit is {budget:.0} s (6 times max_capture_s): raise rate_hz or step_hz, or sweep in parts",
            per_step * count as f64
        ));
    }
    let centres = (0..count)
        .map(|i| start + step * i as f64 + step / 2.0)
        .collect();
    Ok(SweepPlan {
        start,
        stop,
        step,
        centres,
    })
}

/// Sweeps `start_hz` to `stop_hz`: tunes to a centre per step, keeps one step's width of bins around it and writes
/// `freq_hz,power_dbfs` to a new CSV in capture_dir, with the strongest signals and the noise floor.
pub fn sweep(bench: &dyn Bench, cfg: &Config, args: &Value) -> Result<Value, String> {
    // [@ANCHOR: bench_mcp_elad_sweep]
    let path = new_file(cfg, args)?;
    let found = bench
        .find()?
        .into_iter()
        .next()
        .ok_or_else(|| next_step("no ELAD receiver is on the USB bus"))?;
    let rate = rate_of(args, &found)?;
    let p = plan(cfg, args, rate)?;
    let (lo, hi) = found.model.rx_range_hz();
    if p.start < lo as f64 || p.stop > hi as f64 {
        return Err(format!(
            "{} to {} Hz is outside the {} range {lo} to {hi} Hz",
            p.start,
            p.stop,
            found.model.name()
        ));
    }
    let mut first = args.clone();
    first["freq_hz"] = json!(p.centres[0].min(hi as f64));
    let Tuned { mut radio, .. } = open_tuned(bench, cfg, &first)?;
    start_stream(&mut radio)?;
    let begun = Instant::now();
    let mut rows: Vec<(f64, f64, bool)> = Vec::new();
    let mut failure = None;
    for &c in &p.centres {
        let c = c.min(hi as f64);
        if let Err(e) = radio.tune(c) {
            failure = Some(next_step(&e.to_string()));
            break;
        }
        match stream_samples(
            &mut radio,
            SETTLE_SAMPLES + SWEEP_SAMPLES,
            3.0 + 8.0 * ((SETTLE_SAMPLES + SWEEP_SAMPLES) as f64 / f64::from(rate)),
        ) {
            Ok((s, _)) if s.len() >= SETTLE_SAMPLES + SWEEP_SAMPLES => {
                let sp = match dsp::welch(&s[SETTLE_SAMPLES..], SWEEP_FFT, f64::from(rate)) {
                    Ok(sp) => sp,
                    Err(e) => {
                        failure = Some(e);
                        break;
                    }
                };
                for (i, pw) in sp.power_db.iter().enumerate() {
                    let off = sp.offset_hz(i);
                    if off >= -p.step / 2.0 && off < p.step / 2.0 {
                        rows.push((c + off, *pw, off.abs() <= 2.0 * sp.bin_hz));
                    }
                }
            }
            Ok((s, _)) => {
                failure = Some(next_step(&format!(
                    "the receiver delivered {} of the {} samples a step needs at {c} Hz",
                    s.len(),
                    SETTLE_SAMPLES + SWEEP_SAMPLES
                )));
                break;
            }
            Err(e) => {
                failure = Some(e);
                break;
            }
        }
    }
    let _ = radio.stop();
    if let Some(f) = failure {
        return Err(f);
    }
    let mut csv = String::from("freq_hz,power_dbfs\n");
    for (f, pw, _) in &rows {
        csv.push_str(&format!("{f:.3},{pw:.2}\n"));
    }
    std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&path)
        .and_then(|mut f| f.write_all(csv.as_bytes()))
        .map_err(|e| format!("{}: {e}", path.display()))?;
    let mut sorted: Vec<f64> = rows.iter().map(|r| r.1).collect();
    sorted.sort_by(|a, b| a.total_cmp(b));
    let floor = sorted[sorted.len() / 2];
    let signals = sweep_peaks(&rows, floor, 10.0, 5);
    Ok(json!({
        "model": found.model.name(), "rate_hz": rate, "steps": p.centres.len(), "step_hz": p.step,
        "start_hz": p.start, "stop_hz": p.stop, "points": rows.len(),
        "elapsed_s": begun.elapsed().as_secs_f64(), "file": path.display().to_string(),
        "noise_floor_dbfs": floor, "signals": signals,
        "note": "each step keeps the bins within half a step of its centre; the bins within two bins of each step centre (the converter's DC spike, about 375 Hz either side at 192 kHz) are written but not reported as signals, so a signal exactly at a step centre is missed: sweep again with a start_hz shifted by half a step",
    }))
}

/// The strongest local maxima of a sweep at least `margin_db` over `floor`, centre bins excluded, at least five points apart.
fn sweep_peaks(rows: &[(f64, f64, bool)], floor: f64, margin_db: f64, count: usize) -> Vec<Value> {
    // [@ANCHOR: elad_sweep_peaks]
    let mut cand: Vec<usize> = (1..rows.len().saturating_sub(1))
        .filter(|&i| {
            !rows[i].2
                && rows[i].1 >= floor + margin_db
                && rows[i].1 >= rows[i - 1].1
                && rows[i].1 > rows[i + 1].1
        })
        .collect();
    cand.sort_by(|&a, &b| rows[b].1.total_cmp(&rows[a].1));
    let mut kept: Vec<usize> = Vec::new();
    for i in cand {
        if kept.iter().all(|&j| i.abs_diff(j) >= 5) {
            kept.push(i);
            if kept.len() == count {
                break;
            }
        }
    }
    kept.iter()
        .map(|&i| json!({"freq_hz": rows[i].0, "power_dbfs": rows[i].1, "snr_db": rows[i].1 - floor}))
        .collect()
}
