// SPDX-License-Identifier: AGPL-3.0-or-later
//! DG8SAQ VNWA 3E vector network analyser through the `vnwa_open` crate: USB control transfers to the unit and the audio of its two
//! codecs, no vendor software. The sweep tool puts RF on the TX port, so it needs tier `read_config`. Results are raw
//! (uncalibrated) ratios: S21 from the RX channel and S11 from the reflection channel, in the vendor program's convention.

use serde_json::{json, Value};
use std::path::Path;
use std::sync::atomic::{AtomicU32, Ordering};
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
    // The unit must answer a harmless read (request 0x3a, the generator's read-back, 2 bytes) and not only be on the bus: a
    // unit that is listed but does not answer needs a power cycle.
    let mut monitor = Value::Null;
    let (usb, answers) = match vnwa_open::usb::Vnwa::open() {
        Ok(u) => match u.read(0x3a, 0, 0, 2) {
            Ok(b) => {
                monitor = instrument_monitoring(&u);
                ("present", json!({"bytes": b, "resends": u.resends()}))
            }
            Err(e) => ("present but not answering", json!({"error": e})),
        },
        Err(e) => ("not found or no permission", json!({"error": e})),
    };
    let found = vnwa_open::topology::codecs();
    let (s11, s21) = match (&cfg.codec_s11, &cfg.codec_s21, &found) {
        (Some(a), Some(b), _) => (Some(a.clone()), Some(b.clone())),
        (_, _, Ok(c)) => (
            cfg.codec_s11.clone().or(Some(c.s11.clone())),
            cfg.codec_s21.clone().or(Some(c.s21.clone())),
        ),
        _ => (None, None),
    };
    // Are the unit's USB codecs hidden from the sound server? A warning only: a sweep never fails because of it.
    let exposure = vnwa_open::sound_server::check(
        std::path::Path::new("/sys"),
        &vnwa_open::sound_server::run_program,
    );
    Ok(json!({
        "codecs_protected_from_sound_server": vnwa_open::sound_server::protected(&exposure),
        "codecs_protection_detail": match &exposure {
            vnwa_open::sound_server::Exposure::Protected => json!("the sound server lists none of the VNWA's codecs"),
            vnwa_open::sound_server::Exposure::Exposed(c) => json!({"exposed_cards": c.iter().map(|x| json!({"description": x.description, "usb_port": x.port, "sound_server_id": x.id})).collect::<Vec<_>>()}),
            vnwa_open::sound_server::Exposure::Unknown(why) => json!(format!("could not tell: {why}")),
        },
        "codecs_protection_warning": vnwa_open::sound_server::warning(&exposure),
        "usb": usb,
        "usb_read": answers,
        "instrument_monitoring": monitor,
        "reference_clock": "internal 12 MHz TCXO (this driver never selects the external clock input; nothing may be connected to it, see docs/PROTOCOL.md)",
        "hub": cfg.hub.as_ref().map(|h| format!("{} port {}", h.hub, h.port)),
        "power_cycles_by_this_server": POWER_CYCLES.load(Ordering::Relaxed),
        "codec_s11": s11,
        "codec_s21": s21,
        "codecs_error": found.err(),
        "if_hz": cfg.if_hz,
        "point_ms": cfg.point_ms,
        "rf_level": cfg.rf_level,
        "calibrated": false,
    }))
}

/// The unit's firmware version and internal temperature sensor (the vendor's "Instrument Monitoring"). The temperature is in raw,
/// uncalibrated counts that rise as the unit warms (about 25 counts in the first minute after power-up, then slowly): only changes
/// mean anything. A failed read is reported in the value, never as a number.
pub fn instrument_monitoring(u: &vnwa_open::usb::Vnwa) -> Value {
    // [@ANCHOR: bench_vna_instrument_monitoring]
    match u.firmware_version() {
        Err(e) => json!({"error": e}),
        Ok((major, minor)) => match u.temperature_counts() {
            Ok(c) => {
                json!({"firmware": format!("v{major}.{minor}"), "temperature_counts": c, "temperature_note": "uncalibrated sensor counts, relative use only; they rise as the unit warms after power-up"})
            }
            Err(e) => json!({"firmware": format!("v{major}.{minor}"), "temperature_error": e}),
        },
    }
}

/// Makes `vnwa_open`'s own power cycles (the recovery inside a measurement, the generator-off recovery after a modulation) run the USB hub module's
/// implementation, the same code as `bench_vna_power_cycle` and `bench_usb_port_power`. Idempotent.
pub fn install_power_hook() {
    // [@ANCHOR: bench_vna_power_hook]
    let _ = vnwa_open::power::set_cycle_hook(power_hook(
        std::sync::Arc::new(crate::usb_hub::uhubctl::ProcessRunner),
        std::path::PathBuf::from("/sys"),
    ));
}

/// The hook over a given runner and sysfs root (tests give fakes).
pub fn power_hook(
    run: std::sync::Arc<dyn crate::usb_hub::uhubctl::Runner + Send + Sync>,
    sysfs: std::path::PathBuf,
) -> vnwa_open::power::CycleHook {
    Box::new(move |hub, ladder| {
        let port = crate::usb_hub::power::PortRef {
            hub: hub.hub.clone(),
            port: u32::from(hub.port),
            exact: false,
        };
        let schedule: Vec<crate::usb_hub::power::Opts> = ladder
            .iter()
            .map(|(off, back)| crate::usb_hub::power::Opts::new(*off, *back))
            .collect();
        let mut ready = vnwa_open::power::unit_present;
        crate::usb_hub::power::cycle_escalating(
            run.as_ref(),
            &sysfs,
            &port,
            &schedule,
            Some(&mut ready),
        )
        .map(|_| ())
    })
}

/// Cold resets done by this server process, for `status`.
static POWER_CYCLES: AtomicU32 = AtomicU32::new(0);

/// Cold-resets the unit through its hub port (`hub` and `hub_port` in the instrument's entry), waits for it and its codecs, and
/// by default checks it with a one-point sweep at 10 MHz (`verify`, which puts RF at the unit's own level on the TX port).
/// Everything the unit holds is lost: a generator that was on is off afterwards.
pub fn power_cycle(cfg: &Config, args: &Value) -> Result<Value, String> {
    // [@ANCHOR: bench_vna_power_cycle]
    let hub = cfg.hub.clone().ok_or(
        "no hub configured for this VNA: set \"hub\" and \"hub_port\" in its entry of the bench config (for example \"3-2\" and 2)",
    )?;
    let verify = match args.get("verify") {
        None | Some(Value::Null) => true,
        Some(v) => v.as_bool().ok_or("verify must be true or false")?,
    };
    let started = std::time::Instant::now();
    // The power-off, power-on and wait for re-enumeration are the usb_hub module's (the same code as bench_usb_port_power);
    // the VNA supplies its own off times and what "ready" means (the unit and both codecs present).
    let port = crate::usb_hub::power::PortRef {
        hub: hub.hub.clone(),
        port: u32::from(hub.port),
        exact: false,
    };
    let long = vnwa_open::power::LONG_OFF_TIME;
    let schedule = [
        crate::usb_hub::power::Opts::new(
            vnwa_open::power::OFF_TIME,
            std::time::Duration::from_secs(25),
        ),
        crate::usb_hub::power::Opts::new(long, vnwa_open::power::BACK_TIMEOUT),
    ];
    let mut ready = vnwa_open::power::unit_present;
    let report = crate::usb_hub::power::cycle_escalating(
        &crate::usb_hub::uhubctl::ProcessRunner,
        std::path::Path::new("/sys"),
        &port,
        &schedule,
        Some(&mut ready),
    )?;
    // The unit and its codecs are listed before they answer reliably (a request sent 3 s after enumeration stalled); give them time.
    std::thread::sleep(std::time::Duration::from_secs(8));
    POWER_CYCLES.fetch_add(1, Ordering::Relaxed);
    let codecs = vnwa_open::topology::codecs()?;
    let mut out = json!({
        "power_cycled": true,
        "back_after_s": started.elapsed().as_secs_f64(),
        "codec_s11": codecs.s11,
        "codec_s21": codecs.s21,
        "hub_power": report.json(),
        "note": "the unit lost all settings: a generator that was on is off",
    });
    if verify {
        let mut vna = Vna::open(cfg.clone())?;
        let p = vna.sweep(10e6, 10e6, 1)?;
        out["verified"] = json!({"freq_hz": p[0].freq_hz, "s21_db": db(p[0].s21), "s21_deg": p[0].s21.phase_deg(), "s11_db": db(p[0].s11)});
    }
    Ok(out)
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
    // With `calibration` (a file from bench_vna_calibrate_build) and the same `cable` text, the corrected values go to a second CSV; the
    // calibration is refused if the IF, time per point, RF level or cable differ from the ones it was made at.
    let calibrated = match args.get("calibration").and_then(|c| c.as_str()) {
        None => None,
        Some(f) => {
            let cable = args.get("cable").and_then(|c| c.as_str()).ok_or("cable is required with calibration (the text given when the standards were measured)")?;
            let (_, corrected) = crate::vna_tools::apply_calibration(f, &a.config, cable, &points)?;
            let mut t =
                String::from("freq_hz,s11_re,s11_im,s11_db,s11_deg,s21_re,s21_im,s21_db,s21_deg\n");
            let cell = |z: Option<vnwa_open::audio::Complex>| match z {
                Some(z) => format!("{:.6},{:.6},{:.3},{:.3}", z.re, z.im, db(z), z.phase_deg()),
                None => ",,,".to_string(),
            };
            for (p, (s11, s21)) in points.iter().zip(&corrected) {
                t += &format!("{},{},{}\n", p.freq_hz, cell(*s11), cell(*s21));
            }
            let cf = out_dir.join(format!("vna_sweep_{stamp}_calibrated.csv"));
            std::fs::write(&cf, t).map_err(|e| format!("{}: {e}", cf.display()))?;
            Some((cf, corrected))
        }
    };
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
        "calibrated": calibrated.is_some(),
        "calibrated_file": calibrated.as_ref().map(|c| c.0.display().to_string()),
        "calibrated_rows": calibrated.as_ref().filter(|c| c.1.len() <= 51).map(|c| c.1.iter().zip(&points).map(|((s11, s21), p)| json!({"freq_hz": p.freq_hz, "s11_db": s11.map(db), "s11_deg": s11.map(|z| z.phase_deg()), "s21_db": s21.map(db), "s21_deg": s21.map(|z| z.phase_deg())})).collect::<Vec<_>>()),
        "note": "raw ratios of the unit's own channels (cable, bridge and front-end not removed); S21 from the RX channel, S11 from the reflection channel",
        "s21_db_min": lo,
        "s21_db_max": hi,
        "recovery": {"remeasured": vna.recovery().remeasured, "transfer_resends": vna.recovery().transfer_resends, "power_cycles": vna.recovery().power_cycles, "points_measured_again_for_audio_overruns": vna.recovery().overrun_retries, "audio_overruns_reported": vna.recovery().overruns_reported},
        "max_if_error_hz": points.iter().map(|p| p.if_error_hz).fold(0.0, f64::max),
        "max_level_change": points.iter().map(|p| p.level_change).fold(0.0, f64::max),
        "recovered_failures": vna.failure_log().iter().rev().take(5).map(|f| json!({"point": f.what, "attempts": f.attempts})).collect::<Vec<_>>(),
        "temperature_counts_after": match vna.temperature_counts() {
            Ok(c) => json!(c),
            Err(e) => json!({"error": e}),
        },
        "file": file.display().to_string(),
        "rows": rows,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use vnwa_open::audio::Complex;

    // Tests [@ANCHOR: bench_vna_power_hook]
    // Tests [@ANCHOR: bench_vna_instrument_monitoring]
    // Tests [@ANCHOR: bench_vna_power_cycle]
    #[test]
    fn the_vna_power_hook_runs_the_hub_modules_ladder_and_lists_every_failed_rung() {
        let _evidence = crate::usb_hub::testkit::evidence_lock();
        struct Failing(std::sync::Mutex<Vec<String>>);
        impl crate::usb_hub::uhubctl::Runner for Failing {
            fn run(&self, program: &str, args: &[String]) -> Result<(bool, String), String> {
                self.0
                    .lock()
                    .unwrap()
                    .push(format!("{program} {}", args.join(" ")));
                Ok((false, "No compatible devices detected".into()))
            }
        }
        let run = std::sync::Arc::new(Failing(std::sync::Mutex::new(Vec::new())));
        let hook = power_hook(run.clone(), std::path::PathBuf::from("/nonexistent"));
        let ladder = [
            (
                std::time::Duration::from_millis(1),
                std::time::Duration::from_millis(5),
            ),
            (
                std::time::Duration::from_millis(2),
                std::time::Duration::from_millis(5),
            ),
        ];
        let e = hook(
            &vnwa_open::power::HubPort {
                hub: "3-2".into(),
                port: 2,
            },
            &ladder,
        )
        .unwrap_err();
        assert!(e.contains("cycle 1") && e.contains("cycle 2"), "{e}");
        let calls = run.0.lock().unwrap();
        assert!(
            calls
                .iter()
                .all(|c| c.contains("uhubctl") && c.contains("3-2")),
            "{calls:?}"
        );
    }

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
