// SPDX-License-Identifier: AGPL-3.0-or-later
//! The tools behind `bench_gla_*`: capture and analyse a logic-analyzer recording and plan and play a pattern, all through the Glasgow
//! backend's own tools (`bench_glasgow_logic_capture`, `bench_glasgow_pattern`), which keep the voltage, tier, time and tri-state rules.
//! This file adds no way to reach the device: it asks the backend and works on the files and answers that come back.
//! Entries: `docs/GLASGOW_LOGIC_TOOLS.md`.

use super::{decode, export, pattern, stats, trigger, wave};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

/// How the tools reach the Glasgow backend: one call by tool name. The server wires it to `glasgow::call`; the tests to a fake that
/// writes known waveforms.
pub trait Backend {
    // [@ANCHOR: gla_backend_trait]
    fn call(&self, tool: &str, args: &Value) -> Result<Value, String>;
}

/// Everything a tool call needs besides its arguments.
pub struct Ctx<'a> {
    // [@ANCHOR: gla_ctx_struct]
    pub backend: &'a dyn Backend,
    /// Where captures and exports are written and read (the server's `BENCH_MCP_OUT`).
    pub out_dir: PathBuf,
    /// The config's `max_capture_s`: no capture or pattern runs longer.
    pub max_capture_s: f64,
}

/// The analyzer's timestamp clock, hertz.
pub const CLOCK_HZ: f64 = 48e6;

/// The sample rates offered by automatic choice (1-2-5 steps, then the clock itself).
const RATES: &[f64] = &[
    1e3, 2e3, 5e3, 1e4, 2e4, 5e4, 1e5, 2e5, 5e5, 1e6, 2e6, 5e6, 1e7, 2e7, 24e6, 48e6,
];

/// Pin names for a pin spec such as `A0:3` or `A0:1,B0`: `A0 A1 A2 A3`.
pub fn pin_names(spec: &str) -> Result<Vec<String>, String> {
    // [@ANCHOR: gla_pin_names]
    let mut out = vec![];
    for part in spec.split(',') {
        let b = part.as_bytes();
        let ok = b.len() >= 2 && matches!(b[0], b'A' | b'B') && (b'0'..=b'7').contains(&b[1]);
        if !ok {
            return Err(format!("pin spec part {part:?} is not like A0 or A0:3"));
        }
        let (port, lo) = (b[0] as char, b[1] - b'0');
        let hi = match &part[2..] {
            "" => lo,
            r if r.starts_with(':')
                && r.len() == 2
                && (b'0'..=b'7').contains(&r.as_bytes()[1])
                && r.as_bytes()[1] - b'0' >= lo =>
            {
                r.as_bytes()[1] - b'0'
            }
            _ => return Err(format!("pin spec part {part:?} is not like A0 or A0:3")),
        };
        out.extend((lo..=hi).map(|n| format!("{port}{n}")));
    }
    if out.is_empty() || out.len() > wave::MAX_PINS {
        return Err(format!(
            "{} pins; a capture holds 1 to {}",
            out.len(),
            wave::MAX_PINS
        ));
    }
    Ok(out)
}

/// The slowest sample rate from the offered list that shows every pulse of the wave with at least 4 samples, capped by the memory limit
/// (`wave::MAX_SAMPLES`); the fastest rate that fits when none is slow enough.
pub fn auto_rate(w: &wave::Wave) -> f64 {
    // [@ANCHOR: gla_auto_rate]
    let mut shortest = u64::MAX;
    for pin in 0..w.names.len() {
        let mut last: Option<u64> = None;
        let mut prev = (w.init >> pin) & 1;
        for e in &w.edges {
            let v = (e.value >> pin) & 1;
            if v == prev {
                continue;
            }
            prev = v;
            if let Some(t) = last {
                shortest = shortest.min(e.t_ns - t);
            }
            last = Some(e.t_ns);
        }
    }
    let fits = |r: f64| (w.end_ns as f64 / 1e9 * r) < wave::MAX_SAMPLES as f64;
    let want = if shortest == u64::MAX {
        0.0
    } else {
        4.0 * 1e9 / shortest as f64
    };
    let mut sorted = RATES.to_vec();
    sorted.sort_by(|a, b| a.total_cmp(b));
    sorted
        .iter()
        .copied()
        .find(|r| *r >= want && fits(*r))
        .or_else(|| sorted.iter().rev().copied().find(|r| fits(*r)))
        .unwrap_or(1e3)
}

/// What the link can carry. The analyzer sends one record per pin change: 1 to 3 delay bytes (7 bits each, in 48 MHz ticks), one event
/// byte and ceil(pins/8) data bytes. `link_mbps` is the USB speed (12 or 480). The figures are a model; the measured limits of a real
/// device are in the manual page and are labelled as measured there.
pub fn limits(npins: u32, link_mbps: f64) -> Value {
    // [@ANCHOR: gla_limits]
    let data = npins.div_ceil(8) as f64;
    let full = link_mbps < 400.0;
    // Usable bulk bytes per second. Full speed: 1.04 MiB/s device-to-host (benchmark source mode) and 0.92 MiB/s host-to-device (sink),
    // measured on the revC3 unit on 2026-11-10 with the full-speed fallback gateware (64-byte IN packets; the stock gateware overflows
    // every read at 12 Mb/s). High speed: about 40 MB/s through the FX2, a model that a 480 Mb/s link has to confirm.
    let bulk = if full { 1.04 * 1048576.0 } else { 40e6 };
    let rows: Vec<Value> = [
        (0.0, "edges closer than 2.7 us (delta below 128 ticks)"),
        (1.0, "edges 2.7 us to 340 us apart"),
        (2.0, "edges 340 us to 44 ms apart"),
    ]
    .iter()
    .map(|(extra, what)| {
        let bytes = 1.0 + extra + 1.0 + data;
        json!({"spacing": what, "bytes_per_edge": bytes, "max_edges_per_s": (bulk / bytes).floor()})
    })
    .collect();
    json!({"model": true, "pins": npins, "link_mbps": link_mbps, "bulk_bytes_per_s": bulk,
        "capture_works": !full, "capture_works_in_full_speed_fallback": full, "bulk_bytes_per_s_source": if full { "measured, glasgow benchmark source mode with the full-speed fallback gateware at 12 Mb/s on the revC3 unit, 2026-11-10" } else { "model, not yet measured" },
        "note": if full { "At 12 Mb/s (full speed) the stock gateware cannot stream: it sends 512-byte packets that a full-speed link cannot carry, so a capture is refused unless the instrument's config sets \"full_speed_fallback\": true. In that FULL-SPEED FALLBACK mode the gateware sends 64-byte packets and the figures below (about 1 MiB/s) are the ceiling; a signal with more edges per second than the rows show overruns and the capture fails." } else { "Edge rate is what limits a capture, not the time stamp clock: a quiet signal can be captured for a long time, a 5 MHz clock on 4 pins fills the link." },
        "timestamp_clock_hz": CLOCK_HZ, "timestamp_resolution_ns": 1e9 / CLOCK_HZ, "rows": rows,
        "trigger": "none in the device (revision C): the capture is continuous, the trigger and the pre-trigger buffer are found in the recorded data by the host",
        "sample_rate": "the recorded data has the clock's resolution; a lower sample_rate_hz resamples it on the host and reports the pulses it can no longer show"})
}

fn read_wave(path: &Path) -> Result<wave::Wave, String> {
    // [@ANCHOR: gla_read_wave]
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    wave::parse_vcd(&text)
}

/// A file name inside the output directory: no directory part, no `..`.
fn inside(ctx: &Ctx, name: &str) -> Result<PathBuf, String> {
    // [@ANCHOR: gla_inside]
    if name.is_empty() || name.contains('/') || name.contains('\\') || name.starts_with('.') {
        return Err(format!(
            "file {name:?} must be a plain file name in the output directory {}",
            ctx.out_dir.display()
        ));
    }
    Ok(ctx.out_dir.join(name))
}

/// The common part of `bench_gla_capture` and `bench_gla_analyze`: trigger and window, resample, summary, files, decode.
pub fn process(ctx: &Ctx, w: &wave::Wave, args: &Value, stem: &str) -> Result<Value, String> {
    // [@ANCHOR: gla_process]
    let trig = trigger::parse(w, args.get("trigger"))?;
    let rate_arg = args.get("sample_rate_hz").and_then(|r| r.as_f64());
    let ms = |k: &str| {
        args.get(k)
            .and_then(|v| v.as_f64())
            .filter(|v| v.is_finite() && *v >= 0.0)
            .map(|v| (v * 1e6) as u64)
    };
    let pre = ms("pre_trigger_ms").unwrap_or(0);
    let post = match (
        ms("post_trigger_ms"),
        args.get("samples").and_then(|s| s.as_u64()),
        rate_arg,
    ) {
        (Some(p), _, _) => p,
        (None, Some(n), Some(r)) => ((n as f64 / r * 1e9) as u64).saturating_sub(pre),
        (None, Some(_), None) => return Err("samples needs sample_rate_hz".into()),
        _ => u64::MAX / 4,
    };
    let (win, trig_at) = trigger::apply(w, &trig, pre, post)?;
    let rate = match rate_arg {
        Some(r) => r,
        None => auto_rate(&win),
    };
    let samples = wave::resample(&win, rate)?;
    let glitch = args
        .get("glitch_ns")
        .and_then(|g| g.as_u64())
        .unwrap_or((1e9 / rate).max(1e9 / CLOCK_HZ * 2.0).round() as u64);
    let mut files = serde_json::Map::new();
    let formats: Vec<String> = match args.get("formats") {
        None | Some(Value::Null) => vec!["vcd".into(), "csv".into(), "sr".into()],
        Some(v) => v
            .as_array()
            .ok_or("formats must be a list of vcd, csv, sr")?
            .iter()
            .map(|f| f.as_str().unwrap_or("").to_string())
            .collect(),
    };
    if !formats.is_empty() {
        std::fs::create_dir_all(&ctx.out_dir)
            .map_err(|e| format!("{}: {e}", ctx.out_dir.display()))?;
    }
    for f in &formats {
        let (name, bytes): (String, Vec<u8>) = match f.as_str() {
            "vcd" => (format!("{stem}.vcd"), wave::write_vcd(&win).into_bytes()),
            "csv" => (format!("{stem}.csv"), wave::write_csv(&win).into_bytes()),
            "sr" => (format!("{stem}.sr"), export::write_sr(&samples, &win.names)),
            o => return Err(format!("format {o:?} must be vcd, csv or sr")),
        };
        let p = ctx.out_dir.join(&name);
        std::fs::write(&p, &bytes).map_err(|e| format!("{}: {e}", p.display()))?;
        files.insert(
            f.clone(),
            json!({"file": p.display().to_string(), "bytes": bytes.len()}),
        );
    }
    let mut out = json!({
        "trigger": {"spec": args.get("trigger").cloned().unwrap_or(json!("none")), "at_ns_in_window": trig_at, "at_ns_in_capture": win.origin_ns + trig_at, "pre_trigger_ns": trig_at},
        "window": wave::describe(&win), "full_capture": wave::describe(w),
        "sampling": {"rate_hz": rate, "samples": samples.data.len(), "missed_pulses": samples.missed_pulses, "chosen": if rate_arg.is_some() { "by the caller" } else { "automatically: 4 samples on the shortest pulse" }},
        "summary": stats::summarize(&win, glitch), "files": files, "overrun": w.overrun_ns.is_some(),
    });
    if let Some(d) = args.get("decode").filter(|d| !d.is_null()) {
        out["decode"] = decode::run(&win, d)?;
    }
    Ok(out)
}

/// `bench_gla_analyze`: the same analysis on a VCD file already in the output directory (a capture of this tool, of
/// `bench_glasgow_logic_capture`, or any VCD of 1-bit wires). Needs no device.
pub fn analyze(ctx: &Ctx, args: &Value) -> Result<Value, String> {
    // [@ANCHOR: gla_analyze]
    let name = args
        .get("file")
        .and_then(|f| f.as_str())
        .ok_or("file is required: a .vcd file name in the output directory")?;
    if !name.ends_with(".vcd") {
        return Err(format!("file {name:?} must be a .vcd file"));
    }
    let p = inside(ctx, name)?;
    let w = read_wave(&p)?;
    let stem = format!("{}_analysis", name.trim_end_matches(".vcd"));
    let mut out = process(ctx, &w, args, &stem)?;
    out["source"] = json!(p.display().to_string());
    Ok(out)
}

/// `bench_gla_capture`: records `duration_s` through the backend's logic capture (the backend needs the port voltage set first and
/// refuses otherwise), then analyses the file as `analyze` does. Only the arguments of the backend capture go to it; trigger, window,
/// rate, formats and decode are applied here.
pub fn capture(ctx: &Ctx, args: &Value) -> Result<Value, String> {
    // [@ANCHOR: gla_capture]
    let pins = args.get("pins").and_then(|p| p.as_str()).unwrap_or("A0:3");
    let names = pin_names(pins)?;
    let d = args
        .get("duration_s")
        .and_then(|d| d.as_f64())
        .unwrap_or(2.0);
    if !(0.1..=ctx.max_capture_s).contains(&d) {
        return Err(format!(
            "duration_s {d} must be 0.1 to {} (the config's max_capture_s)",
            ctx.max_capture_s
        ));
    }
    let mut b = json!({"pins": pins, "duration_s": d});
    for k in ["pull", "confirm", "instrument"] {
        if let Some(v) = args.get(k) {
            b[k] = v.clone();
        }
    }
    let r = ctx.backend.call("bench_glasgow_logic_capture", &b)?;
    if r.get("dry_run").is_some() {
        return Ok(r);
    }
    let file = r
        .get("file")
        .and_then(|f| f.as_str())
        .ok_or("the backend answered without a capture file")?;
    let path = PathBuf::from(file);
    let mut w = read_wave(&path)?;
    if w.names.len() == names.len() {
        w.names = names;
    }
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "glasgow_capture".into());
    let mut out = process(ctx, &w, args, &format!("{stem}_gla"))?;
    out["backend"] = json!({"file": file, "overrun": r.get("overrun"), "pins_state": r.get("pins_state"), "stopped_by": r.get("stopped_by")});
    out["limits"] = limits(w.names.len() as u32, 480.0)["rows"].clone();
    Ok(out)
}

/// `bench_gla_pattern`: plans the pattern (`pattern::plan`) and, unless `dry_run`, has the backend play it. `confirm` is passed to the
/// backend, which refuses without it, without a set port voltage, and past its time limit, and releases the pins at the end.
pub fn pattern_tool(ctx: &Ctx, args: &Value) -> Result<Value, String> {
    // [@ANCHOR: gla_pattern_tool]
    let pins = args
        .get("pins")
        .and_then(|p| p.as_str())
        .ok_or("pins is required, e.g. A0:3")?;
    let names = pin_names(pins)?;
    if names.len() > 8 {
        return Err("a pattern drives at most 8 pins".into());
    }
    let spec = args.get("pattern").unwrap_or(args);
    let plan = pattern::plan(spec, names.len(), ctx.max_capture_s)?;
    let mut b = pattern::backend_args(&plan, pins);
    for k in ["confirm", "dry_run", "instrument"] {
        if let Some(v) = args.get(k) {
            b[k] = v.clone();
        }
    }
    let r = ctx.backend.call("bench_glasgow_pattern", &b)?;
    Ok(
        json!({"planned": {"steps": plan.steps.len(), "repeat": plan.repeat, "total_s": plan.total_s, "achieved": plan.achieved, "pins": names,
        "timing": "host-timed: holds of 0.5 ms or more, about 1 ms of jitter; measure the real output with bench_gla_capture on a wired-back pin"}, "backend": r}),
    )
}
