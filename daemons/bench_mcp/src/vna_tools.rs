// SPDX-License-Identifier: AGPL-3.0-or-later
//! The VNWA's other functions as separate tools: the CW generator (on, off, level, AM), the frequency meter, and calibration (measure each
//! standard, build the calibration, apply it to a sweep). Everything that puts RF on the TX port needs tier read_config.

use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use vnwa_open::audio::Complex;
use vnwa_open::cal::{Calibration, KitDefinition, Setup};
use vnwa_open::sweep::{Config, Point, Vna};
use vnwa_open::usb::Vnwa;

fn num(args: &Value, key: &str) -> Result<Option<f64>, String> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v
            .as_f64()
            .filter(|x| x.is_finite())
            .map(Some)
            .ok_or(format!("{key} must be a number")),
    }
}

fn level_db(args: &Value) -> Result<f64, String> {
    let l =
        num(args, "level_db")?.ok_or("level_db is required (0 to -50: a negative attenuation)")?;
    vnwa_open::generator::level_for_attenuation(l)?;
    Ok(l)
}

fn freq_hz(args: &Value) -> Result<f64, String> {
    let f = num(args, "freq_hz")?.ok_or("freq_hz is required")?;
    if !(f > 0.0 && f <= 1.3e9) {
        return Err(format!("freq_hz {f} is outside 0 to 1.3e9"));
    }
    Ok(f)
}

pub fn generator_on(args: &Value) -> Result<Value, String> {
    let (f, l) = (freq_hz(args)?, level_db(args)?);
    let usb = Vnwa::open()?;
    vnwa_open::generator::on(&usb, f, l)?;
    Ok(
        json!({"generator": "on", "freq_hz": f, "level_db": l, "note": "CW on the TX port at the unit's own level (about -17 dBm at 0 dB); verified on a scope from 0 to -40 dB; switch off with bench_vna_generator_off"}),
    )
}

/// Switches the generator off. A unit that is modulating (AM or FM) refuses the request; a USB reset of its control chip (no power change) ends the
/// modulation (measured on the unit, 2026-10-29), and only if that fails is the unit cold reset through its hub, with the longer ladder for modulation
/// lock-ups, when the instrument has a hub; otherwise the error says what to do.
pub fn generator_off(cfg: &Config) -> Result<Value, String> {
    let usb = Vnwa::open()?;
    // [@ANCHOR: bench_vna_modulation_stop]
    match vnwa_open::generator::off(&usb) {
        Ok(()) => Ok(json!({"generator": "off"})),
        Err(e) if !vnwa_open::generator::answers(&usb) => {
            drop(usb);
            if vnwa_open::generator::end_modulation().is_ok() {
                return Ok(
                    json!({"generator": "off", "ended_modulation_by_usb_reset": true, "note": "the unit was modulating and refused the switch-off; a USB reset of its control chip ended it (no power cycle)"}),
                );
            }
            let Some(hub) = cfg.hub.clone() else {
                return Err(format!(
                    "the unit refuses the switch-off request ({e}) and a USB reset did not help: power its USB port off and on. No hub is configured for this VNA (\"hub\" and \"hub_port\" in its entry of the bench config)"
                ));
            };
            vnwa_open::power::cold_reset_after_modulation(&hub)?;
            let usb = Vnwa::open()?;
            if !vnwa_open::generator::answers(&usb) {
                return Err("the unit was power cycled but still does not answer".into());
            }
            Ok(
                json!({"generator": "off", "recovered_by_power_cycle": true, "note": "a USB reset did not end the stall, so the unit was cold reset through its hub; it lost all settings"}),
            )
        }
        Err(e) => Err(e),
    }
}

pub fn generator_level(args: &Value) -> Result<Value, String> {
    let l = level_db(args)?;
    let usb = Vnwa::open()?;
    vnwa_open::generator::set_attenuation(&usb, l)?;
    Ok(json!({"generator": "level set", "level_db": l}))
}

fn mod_args(args: &Value) -> Result<(f64, f64, f64), String> {
    let f = freq_hz(args)?;
    let level = match num(args, "level_db")? {
        Some(l) if (-50.0..=0.0).contains(&l) => l,
        Some(l) => return Err(format!("level_db {l} is outside -50 to 0")),
        None => -20.0,
    };
    let hz = num(args, "mod_hz")?.unwrap_or(500.0);
    if !(vnwa_open::generator::MIN_MOD_HZ..=vnwa_open::generator::MAX_MOD_HZ).contains(&hz) {
        return Err(format!("mod_hz {hz} is outside 1 to 3000"));
    }
    Ok((f, level, hz))
}

pub fn generator_am(args: &Value) -> Result<Value, String> {
    let (f, level, hz) = mod_args(args)?;
    let depth = num(args, "depth_pct")?.unwrap_or(100.0);
    // Validated before the unit is touched.
    vnwa_open::generator::am_fields(
        vnwa_open::generator::level_for_attenuation(level)?,
        hz,
        depth,
    )?;
    let usb = Vnwa::open()?;
    vnwa_open::generator::on(&usb, f, level)?;
    vnwa_open::generator::modulate(
        &usb,
        f,
        level,
        vnwa_open::generator::Modulation::Am {
            mod_hz: hz,
            depth_pct: depth,
        },
    )?;
    Ok(json!({
        "generator": "AM on", "freq_hz": f, "level_db": level, "mod_hz": hz, "depth_pct": depth,
        "verified": "depth within 3 percentage points and frequency within 5 Hz on a scope at 10 MHz, 500 to 3000 Hz and 10 to 100 %",
        "warning": "the unit's microcontroller now modulates in real time and refuses the switch-off request until it is USB reset: bench_vna_generator_off does that (about 3 s, no power cycle) and falls back to a power cycle through the hub only if that fails. Nothing else may use the unit meanwhile."
    }))
}

pub fn generator_fm(args: &Value) -> Result<Value, String> {
    let (f, level, hz) = mod_args(args)?;
    let dev = num(args, "deviation_hz")?.ok_or("deviation_hz is required")?;
    vnwa_open::generator::fm_fields(hz, dev)?;
    let usb = Vnwa::open()?;
    vnwa_open::generator::on(&usb, f, level)?;
    vnwa_open::generator::modulate(
        &usb,
        f,
        level,
        vnwa_open::generator::Modulation::Fm {
            mod_hz: hz,
            deviation_hz: dev,
        },
    )?;
    Ok(json!({
        "generator": "FM on", "freq_hz": f, "level_db": level, "mod_hz": hz, "deviation_hz": dev,
        "verified": "deviation within 3 % on a scope (line pattern against Bessel functions) at 1 and 2 kHz, 10 and 20 kHz deviation",
        "warning": "the unit modulates in real time and refuses the switch-off request until it is USB reset: bench_vna_generator_off does that."
    }))
}

pub fn meter(cfg: &Config, args: &Value) -> Result<Value, String> {
    let center = freq_hz(args)?;
    let ms = num(args, "point_ms")?.unwrap_or(60.0);
    if !(0.2..=100.0).contains(&ms) {
        return Err("point_ms must be 0.2 to 100".into());
    }
    let mut vna = Vna::open(Config {
        if_hz: 4000.0,
        ..cfg.clone()
    })?;
    let r = vna.meter(center, ms)?;
    Ok(json!({
        "center_hz": r.center_hz, "tx_dds_hz": r.tx_actual_hz, "offset_hz": r.offset_hz, "offset_codec1_hz": r.offset_s11_hz, "offset_codec2_hz": r.offset_s21_hz,
        "stderr_hz": r.stderr_hz, "frequency_hz": r.frequency_hz(), "measurement_s": r.seconds * 31.0 / 28.0,
        "note": "the offset of the RX signal from the centre, valid within +/-25 Hz; a TX-to-RX loopback reads 0. The sign of the offset (higher RX frequency = positive) and the absolute accuracy against a standard are NOT verified; the reference is the unit's 12 MHz TCXO (about 0.3 ppm per the vendor)",
        "recovery": {"remeasured": vna.recovery().remeasured, "transfer_resends": vna.recovery().transfer_resends, "power_cycles": vna.recovery().power_cycles, "points_measured_again_for_audio_overruns": vna.recovery().overrun_retries, "audio_overruns_reported": vna.recovery().overruns_reported},
    }))
}

/// The spectrum analyzer: power in the audio band around the LO at each frequency, in codec counts (uncalibrated). See `vnwa_open::spectrum`.
pub fn spectrum(cfg: &Config, args: &Value, out_dir: &Path) -> Result<Value, String> {
    // [@ANCHOR: bench_vna_spectrum]
    let start = num(args, "start_hz")?.ok_or("start_hz is required")?;
    let stop = num(args, "stop_hz")?.ok_or("stop_hz is required")?;
    let points = match num(args, "points")? {
        Some(p) if p.fract() == 0.0 && (2.0..=2000.0).contains(&p) => p as usize,
        Some(p) => return Err(format!("points {p} must be a whole number from 2 to 2000")),
        None => return Err("points is required".into()),
    };
    let band = num(args, "band_hz")?.unwrap_or(5_000.0);
    let ms = num(args, "point_ms")?.unwrap_or(20.0);
    if !(5.0..=100.0).contains(&ms) {
        return Err("point_ms must be 5 to 100".into());
    }
    let tracking = match num(args, "tracking_db")? {
        None => 0,
        Some(d) => vnwa_open::generator::level_for_attenuation(d)?,
    };
    vnwa_open::spectrum::spectrum_plan(start, stop, points, band, tracking)?;
    let mut vna = Vna::open(Config {
        if_hz: vnwa_open::spectrum::SA_IF_HZ,
        point_ms: ms,
        ..cfg.clone()
    })?;
    let sp = vna.spectrum(start, stop, points, band, tracking)?;
    let mut sorted: Vec<f64> = sp.iter().map(|p| p.rms).collect();
    sorted.sort_by(|a, b| a.total_cmp(b));
    let floor = sorted[sorted.len() / 2];
    let mut peaks: Vec<(f64, f64)> = (0..sp.len())
        .filter(|&i| {
            sp[i].rms > 3.0 * floor
                && (i == 0 || sp[i].rms >= sp[i - 1].rms)
                && (i + 1 == sp.len() || sp[i].rms > sp[i + 1].rms)
        })
        .map(|i| (sp[i].freq_hz, sp[i].rms))
        .collect();
    peaks.sort_by(|a, b| b.1.total_cmp(&a.1));
    peaks.truncate(10);
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let file = out_dir.join(format!("vna_spectrum_{stamp}.csv"));
    let mut csv = String::from("freq_hz,rms_counts,db_rel_floor\n");
    for p in &sp {
        csv.push_str(&format!(
            "{:.1},{:.3},{:.2}\n",
            p.freq_hz,
            p.rms,
            20.0 * (p.rms / floor).log10()
        ));
    }
    std::fs::write(&file, csv).map_err(|e| format!("{}: {e}", file.display()))?;
    let r = vna.recovery();
    Ok(json!({
        "points": sp.len(), "start_hz": start, "stop_hz": stop, "band_hz": band, "point_ms": ms, "tracking_generator": tracking > 0,
        "noise_floor_counts": floor,
        "peaks": peaks.iter().map(|(f, a)| json!({"freq_hz": f, "rms_counts": a, "db_over_floor": 20.0 * (a / floor).log10()})).collect::<Vec<_>>(),
        "rows": if sp.len() <= 51 { sp.iter().map(|p| json!({"freq_hz": p.freq_hz, "rms_counts": p.rms})).collect::<Vec<_>>() } else { Vec::new() },
        "file": file.display().to_string(),
        "note": "UNCALIBRATED: rms of the audio within band_hz of the unit's LO, in codec counts, tracking generator off unless tracking_db is given (then the TX runs 12 kHz below the LO and a loopback shows it as a line at 12 kHz: use band_hz above 12000 to see it). The audio is real, so a signal at LO+d and one at LO-d fall together: a point is two-sided. Ghost lines (the unfiltered LO) and subharmonic responses (direct conversion) are NOT characterised here; the vendor's Help file says to use it below 100 MHz. A known 15 kHz-band tone read 8771 counts rms to 0.03 % over points, the noise floor is about 2 counts.",
        "recovery": {"remeasured": r.remeasured, "transfer_resends": r.transfer_resends, "power_cycles": r.power_cycles, "points_measured_again_for_audio_overruns": r.overrun_retries, "audio_overruns_reported": r.overruns_reported},
    }))
}

fn name_ok(s: &str) -> Result<(), String> {
    if s.is_empty()
        || s.len() > 40
        || !s
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        return Err("session must be 1 to 40 letters, digits, - or _".into());
    }
    Ok(())
}

fn cal_path(out: &Path, session: &str, what: &str) -> PathBuf {
    out.join(format!("cal_{session}_{what}"))
}

/// The header line stored with each measured standard: the settings it was measured at.
fn meta_line(cfg: &Config, cable: &str) -> String {
    format!(
        "# if_hz={};point_ms={};rf_level={};cable={}",
        cfg.if_hz,
        cfg.point_ms,
        cfg.rf_level,
        cable.replace(['\n', '\r'], " ")
    )
}

fn parse_meta(line: &str) -> Result<Setup, String> {
    let body = line.strip_prefix("# ").ok_or("missing settings line")?;
    let mut s = Setup::default();
    for part in body.splitn(4, ';') {
        let (k, v) = part.split_once('=').ok_or("bad settings line")?;
        match k {
            "if_hz" => s.if_hz = v.parse().map_err(|_| "bad if_hz")?,
            "point_ms" => s.point_ms = v.parse().map_err(|_| "bad point_ms")?,
            "rf_level" => s.rf_level = v.parse().map_err(|_| "bad rf_level")?,
            "cable" => s.cable = v.to_string(),
            _ => return Err(format!("unknown setting {k}")),
        }
    }
    Ok(s)
}

pub fn calibrate_measure(cfg: &Config, args: &Value, out: &Path) -> Result<Value, String> {
    let session = args
        .get("session")
        .and_then(|v| v.as_str())
        .ok_or("session is required")?;
    name_ok(session)?;
    let standard = args
        .get("standard")
        .and_then(|v| v.as_str())
        .ok_or("standard is required")?;
    if !["open", "short", "load", "thru"].contains(&standard) {
        return Err("standard must be open, short, load or thru".into());
    }
    let cable = args.get("cable").and_then(|v| v.as_str()).ok_or("cable is required: say what is between the ports and the reference plane (it is stored and compared)")?;
    let a = crate::vna::sweep_args(args, cfg)?;
    if a.points < 2 {
        return Err("a calibration needs at least 2 points".into());
    }
    let mut vna = Vna::open(a.config.clone())?;
    let pts = vna.sweep(a.start_hz, a.stop_hz, a.points)?;
    let mut text = meta_line(&a.config, cable) + "\n";
    for p in &pts {
        text += &format!(
            "{:e},{:e},{:e},{:e},{:e}\n",
            p.freq_hz, p.s11.re, p.s11.im, p.s21.re, p.s21.im
        );
    }
    let file = cal_path(out, session, &format!("{standard}.csv"));
    std::fs::write(&file, text).map_err(|e| format!("{}: {e}", file.display()))?;
    Ok(
        json!({"session": session, "standard": standard, "points": pts.len(), "file": file, "note": "measure open, short, load (and thru for S21) with the same start, stop, points and cable, then bench_vna_calibrate_build"}),
    )
}

/// The settings, frequencies, S11 and S21 of one stored standard.
type Standard = (Setup, Vec<f64>, Vec<Complex>, Vec<Complex>);

/// The corrected (S11, S21) of each point; a part the calibration does not cover is None.
type Corrected = Vec<(Option<Complex>, Option<Complex>)>;

fn read_standard(file: &Path) -> Result<Standard, String> {
    let t = std::fs::read_to_string(file).map_err(|e| format!("{}: {e}", file.display()))?;
    let mut lines = t.lines();
    let setup = parse_meta(lines.next().ok_or("empty file")?)?;
    let (mut f, mut s11, mut s21) = (Vec::new(), Vec::new(), Vec::new());
    for l in lines {
        let v: Vec<f64> = l
            .split(',')
            .map(|x| x.parse::<f64>().map_err(|_| format!("bad number in {l:?}")))
            .collect::<Result<_, _>>()?;
        if v.len() != 5 {
            return Err(format!("bad row {l:?}"));
        }
        f.push(v[0]);
        s11.push(Complex { re: v[1], im: v[2] });
        s21.push(Complex { re: v[3], im: v[4] });
    }
    Ok((setup, f, s11, s21))
}

pub fn calibrate_build(args: &Value, out: &Path) -> Result<Value, String> {
    // [@ANCHOR: bench_mcp_vna_cal_build]
    let session = args
        .get("session")
        .and_then(|v| v.as_str())
        .ok_or("session is required")?;
    name_ok(session)?;
    let kit = match args.get("kit_file").and_then(|v| v.as_str()) {
        Some(p) => {
            KitDefinition::from_text(&std::fs::read_to_string(p).map_err(|e| format!("{p}: {e}"))?)?
        }
        None => KitDefinition::default(),
    };
    let created = args
        .get("created")
        .and_then(|v| v.as_str())
        .map(String::from)
        .unwrap_or_else(|| {
            let t = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            format!("unix {t}")
        });
    let load = |what: &str| {
        let p = cal_path(out, session, &format!("{what}.csv"));
        if p.exists() {
            read_standard(&p).map(Some)
        } else {
            Ok(None)
        }
    };
    let (open, short, ld, thru) = (load("open")?, load("short")?, load("load")?, load("thru")?);
    let first = [&open, &short, &ld, &thru]
        .into_iter()
        .flatten()
        .next()
        .ok_or("no standards measured for this session")?;
    let (setup, freqs) = (first.0.clone(), first.1.clone());
    for (name, s) in [
        ("open", &open),
        ("short", &short),
        ("load", &ld),
        ("thru", &thru),
    ] {
        if let Some(s) = s {
            if s.0 != setup || s.1 != freqs {
                return Err(format!(
                    "{name} was measured at other settings, cable or frequencies than the others"
                ));
            }
        }
    }
    let cal = Calibration::build(
        freqs.clone(),
        open.as_ref().map(|s| s.2.as_slice()),
        short.as_ref().map(|s| s.2.as_slice()),
        ld.as_ref().map(|s| s.2.as_slice()),
        thru.as_ref().map(|s| s.3.as_slice()),
        kit,
        setup.clone(),
        created,
    )?;
    let file = out.join(format!("cal_{session}.cal"));
    std::fs::write(&file, cal.to_text()).map_err(|e| format!("{}: {e}", file.display()))?;
    Ok(json!({
        "file": file, "start_hz": freqs[0], "stop_hz": freqs[freqs.len() - 1], "points": freqs.len(),
        "s11_calibrated": cal.one_port.is_some(), "s21_calibrated": cal.thru.is_some(),
        "if_hz": setup.if_hz, "point_ms": setup.point_ms, "rf_level": setup.rf_level, "cable": setup.cable,
        "note": "apply with bench_vna_sweep calibration=<file> and the same cable text; it is refused at other IF, time per point, RF level or cable"
    }))
}

/// Corrects `points` with the calibration in `file`; the settings and cable must be those it was made at.
pub fn apply_calibration(
    file: &str,
    cfg: &Config,
    cable: &str,
    points: &[Point],
) -> Result<(Calibration, Corrected), String> {
    // [@ANCHOR: bench_mcp_vna_cal_apply]
    let cal = Calibration::from_text(
        &std::fs::read_to_string(file).map_err(|e| format!("{file}: {e}"))?,
    )?;
    cal.check_setup(&Setup {
        if_hz: cfg.if_hz,
        point_ms: cfg.point_ms,
        rf_level: cfg.rf_level,
        cable: cable.to_string(),
    })?;
    let mut out = Vec::new();
    for p in points {
        let c = cal.correct(p)?;
        out.push((c.s11, c.s21));
    }
    Ok((cal, out))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("bench_vt_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn generator_arguments_are_checked_before_any_hardware_is_touched() {
        for bad in [
            json!({}),
            json!({"freq_hz": 10e6}),
            json!({"freq_hz": 10e6, "level_db": 3}),
            json!({"freq_hz": 10e6, "level_db": -60}),
            json!({"freq_hz": 0, "level_db": -10}),
            json!({"freq_hz": "x", "level_db": -10}),
        ] {
            assert!(generator_on(&bad).is_err(), "{bad}");
        }
        assert!(generator_level(&json!({"level_db": 1})).is_err());
        assert!(generator_am(&json!({"freq_hz": 5e9})).is_err());
        assert!(
            meter(&Config::default(), &json!({"center_hz": 1e7})).is_err(),
            "freq_hz is the key"
        );
        assert!(meter(
            &Config::default(),
            &json!({"freq_hz": 1e7, "point_ms": 500})
        )
        .is_err());
    }

    // Tests [@ANCHOR: bench_mcp_vna_cal_build]
    #[test]
    fn a_calibration_is_built_from_stored_standards_and_refuses_mixed_ones() {
        let dir = tmp("build");
        let write = |what: &str, cable: &str, if_hz: f64, g: Complex| {
            let cfg = Config {
                if_hz,
                ..Config::default()
            };
            let mut t = meta_line(&cfg, cable) + "\n";
            for f in [1e6, 2e6, 3e6] {
                t += &format!("{:e},{:e},{:e},{:e},{:e}\n", f, g.re, g.im, 0.5, 0.1);
            }
            std::fs::write(cal_path(&dir, "s1", &format!("{what}.csv")), t).unwrap();
        };
        // A unit with e00 = 0.1, tracking 1, e11 = 0 reads G + 0.1 for a reflection G.
        write("open", "cable A", 4000.0, Complex { re: 1.1, im: 0.0 });
        write("short", "cable A", 4000.0, Complex { re: -0.9, im: 0.0 });
        write("load", "cable A", 4000.0, Complex { re: 0.1, im: 0.0 });
        write("thru", "cable A", 4000.0, Complex { re: 0.0, im: 0.0 });
        let r = calibrate_build(&json!({"session": "s1"}), &dir).unwrap();
        assert_eq!(
            (
                r["s11_calibrated"].as_bool(),
                r["s21_calibrated"].as_bool(),
                r["points"].as_u64()
            ),
            (Some(true), Some(true), Some(3))
        );
        let file = r["file"].as_str().unwrap().to_string();
        // Applying it: a raw 0.35 reads as 0.25; the S21 of the through is divided out.
        let pt = Point {
            freq_hz: 2e6,
            s11: Complex { re: 0.35, im: 0.0 },
            s21: Complex { re: 0.5, im: 0.1 },
            if_error_hz: 0.0,
            level_change: 0.0,
        };
        let (_, got) = apply_calibration(
            &file,
            &Config {
                if_hz: 4000.0,
                ..Config::default()
            },
            "cable A",
            &[pt],
        )
        .unwrap();
        assert!(
            (got[0].0.unwrap().re - 0.25).abs() < 1e-9 && (got[0].1.unwrap().re - 1.0).abs() < 1e-9
        );
        // Refused at another IF, another cable, or outside the range.
        let e = apply_calibration(
            &file,
            &Config {
                if_hz: 12000.0,
                ..Config::default()
            },
            "cable A",
            &[pt],
        )
        .unwrap_err();
        assert!(e.contains("not valid") && e.contains("IF"), "{e}");
        assert!(apply_calibration(
            &file,
            &Config {
                if_hz: 4000.0,
                ..Config::default()
            },
            "cable B",
            &[pt]
        )
        .unwrap_err()
        .contains("cable"));
        let out = Point { freq_hz: 9e6, ..pt };
        assert!(apply_calibration(
            &file,
            &Config {
                if_hz: 4000.0,
                ..Config::default()
            },
            "cable A",
            &[out]
        )
        .unwrap_err()
        .contains("outside"));
        // Standards from different cables or IFs are not mixed.
        write("open", "cable B", 4000.0, Complex { re: 1.1, im: 0.0 });
        assert!(calibrate_build(&json!({"session": "s1"}), &dir)
            .unwrap_err()
            .contains("other settings"));
        assert!(calibrate_build(&json!({"session": "nope"}), &dir)
            .unwrap_err()
            .contains("no standards"));
        assert!(calibrate_build(&json!({"session": "../x"}), &dir).is_err());
        std::fs::remove_dir_all(&dir).ok();
    }

    // Tests [@ANCHOR: bench_mcp_vna_cal_apply]
    #[test]
    fn measuring_a_standard_needs_a_valid_session_standard_and_cable() {
        let dir = tmp("meas");
        for bad in [
            json!({"standard": "open", "cable": "c", "start_hz": 1e6, "stop_hz": 2e6, "points": 3}),
            json!({"session": "s", "standard": "match", "cable": "c", "start_hz": 1e6, "stop_hz": 2e6, "points": 3}),
            json!({"session": "s", "standard": "open", "start_hz": 1e6, "stop_hz": 2e6, "points": 3}),
            json!({"session": "s", "standard": "open", "cable": "c", "start_hz": 1e6, "stop_hz": 2e6, "points": 1}),
            json!({"session": "s s", "standard": "open", "cable": "c", "start_hz": 1e6, "stop_hz": 2e6, "points": 3}),
        ] {
            assert!(
                calibrate_measure(&Config::default(), &bad, &dir).is_err(),
                "{bad}"
            );
        }
        std::fs::remove_dir_all(&dir).ok();
    }
}
