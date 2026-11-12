// SPDX-License-Identifier: AGPL-3.0-or-later
//! The analyzer side of the Siglent SSA3000X driver: one sweep, the trace, peaks, the CSV file, markers and the screenshot.

use super::{ask, ask_f64, flag, number, open, Connector, READ_TRIES, REPLY_TIMEOUT_S};
use crate::config::Instrument;
use crate::link::Link;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// Points in a trace: fixed by the unit (no SCPI command sets it).
pub const POINTS: usize = 751;
/// The longest total sweep time (sweep time times the average count) a tool will start, seconds.
const MAX_SWEEP_S: f64 = 120.0;

// [@ANCHOR: bench_mcp_sa_span]
/// The frequency range of a sweep, Hz.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Span {
    pub start_hz: f64,
    pub stop_hz: f64,
}

// [@ANCHOR: bench_mcp_sa_sweep_req]
/// A checked sweep request. `None` fields are left as the instrument has them.
#[derive(Clone, Debug, PartialEq)]
pub struct SweepReq {
    pub span: Span,
    pub rbw_hz: Option<f64>,
    pub vbw_hz: Option<f64>,
    pub ref_level_dbm: Option<f64>,
    pub attenuation_db: Option<u32>,
    /// The SCPI detector name (`POSitive`, `NEGative`, `SAMPle`, `AVERage`, `NORMal`).
    pub detector: Option<&'static str>,
    /// Sweeps averaged (1 = a single sweep).
    pub average: u32,
    /// Put the unit back in continuous sweep afterwards.
    pub continuous_after: bool,
}

// [@ANCHOR: bench_mcp_sa_trace]
/// One trace and the settings the unit reported for it.
#[derive(Clone, Debug, PartialEq)]
pub struct Trace {
    pub start_hz: f64,
    pub stop_hz: f64,
    pub dbm: Vec<f64>,
    pub rbw_hz: f64,
    pub vbw_hz: f64,
    pub ref_level_dbm: f64,
    pub attenuation_db: f64,
    pub detector: String,
    pub sweep_time_s: f64,
    pub average: u32,
}

// [@ANCHOR: bench_mcp_sa_peak]
/// A peak: frequency and level.
#[derive(Clone, Debug, PartialEq)]
pub struct Peak {
    pub freq_hz: f64,
    pub level_dbm: f64,
}

/// The sweep arguments (`start_hz`+`stop_hz` or `center_hz`+`span_hz`, `rbw_hz`, `vbw_hz`, `ref_level_dbm`,
/// `attenuation_db`, `detector`, `average_count`, `continuous_after`) checked against the model's top frequency `max_hz` and
/// the unit's ranges before anything is sent.
pub fn sweep_args(args: &Value, max_hz: f64) -> Result<SweepReq, String> {
    // [@ANCHOR: bench_mcp_sa_sweep_args]
    let top = "the model's top frequency";
    let n = |k: &str, lo: f64, hi: f64, why: &str| number(args, k, lo, hi, why);
    let (start, stop, center, span) = (
        n("start_hz", 0.0, max_hz, top)?,
        n("stop_hz", 0.0, max_hz, top)?,
        n("center_hz", 0.0, max_hz, top)?,
        n("span_hz", 100.0, max_hz, top)?,
    );
    let (start_hz, stop_hz) =
        match (start, stop, center, span) {
            (Some(a), Some(b), None, None) => (a, b),
            (None, None, Some(c), Some(s)) => (c - s / 2.0, c + s / 2.0),
            _ => return Err(
                "give start_hz and stop_hz, or center_hz and span_hz (not a mixture, not neither)"
                    .into(),
            ),
        };
    if start_hz < 0.0 || stop_hz > max_hz + 0.5 || stop_hz - start_hz < 100.0 {
        return Err(format!(
            "the range {start_hz}..{stop_hz} Hz must lie inside 0..{max_hz} Hz ({top}) and be at least 100 Hz wide"
        ));
    }
    let detector = match args.get("detector") {
        None | Some(Value::Null) => None,
        Some(v) => Some(match v.as_str() {
            Some("positive") => "POSitive",
            Some("negative") => "NEGative",
            Some("sample") => "SAMPle",
            Some("average") => "AVERage",
            Some("normal") => "NORMal",
            _ => {
                return Err("detector must be positive, negative, sample, average or normal".into())
            }
        }),
    };
    let int = |k: &str, lo: f64, hi: f64, why: &str| -> Result<Option<u32>, String> {
        match n(k, lo, hi, why)? {
            Some(x) if x.fract() != 0.0 => Err(format!("{k} must be a whole number")),
            x => Ok(x.map(|x| x as u32)),
        }
    };
    Ok(SweepReq {
        span: Span { start_hz, stop_hz },
        rbw_hz: n("rbw_hz", 10.0, 1e6, "the unit's resolution bandwidths")?,
        vbw_hz: n("vbw_hz", 1.0, 1e6, "the unit's video bandwidths")?,
        ref_level_dbm: n(
            "ref_level_dbm",
            -100.0,
            30.0,
            "the unit's reference level range",
        )?,
        attenuation_db: int("attenuation_db", 0.0, 50.0, "the unit's attenuator")?,
        detector,
        average: int("average_count", 1.0, 999.0, "the unit's average count")?.unwrap_or(1),
        continuous_after: flag(args, "continuous_after", true)?,
    })
}

/// Parses the comma separated ASCII trace the unit returns (`:TRACe:DATA? 1`): every field must be a finite number, at
/// least two of them.
pub fn parse_trace(reply: &str) -> Result<Vec<f64>, String> {
    // [@ANCHOR: bench_mcp_sa_parse_trace]
    let mut v = Vec::new();
    for f in reply.split(',') {
        let f = f.trim();
        if f.is_empty() {
            continue;
        }
        match f.parse::<f64>() {
            Ok(x) if x.is_finite() => v.push(x),
            _ => {
                return Err(format!(
                    "the trace has a field {f:?} that is not a finite number"
                ))
            }
        }
    }
    if v.len() < 2 {
        return Err(format!("the trace has only {} points", v.len()));
    }
    Ok(v)
}

/// Frequency of point `i` of `n` between `start` and `stop`.
pub fn bin_hz(t_start: f64, t_stop: f64, n: usize, i: usize) -> f64 {
    // [@ANCHOR: bench_mcp_sa_bin_hz]
    t_start + (t_stop - t_start) * i as f64 / (n - 1) as f64
}

/// Local maxima at least `threshold_db` above the trace median, strongest first, at most `count`, none within two points of a
/// stronger one.
pub fn find_peaks(t: &Trace, count: usize, threshold_db: f64) -> Vec<Peak> {
    // [@ANCHOR: bench_mcp_sa_find_peaks]
    let v = &t.dbm;
    let mut sorted = v.clone();
    sorted.sort_by(|a, b| a.total_cmp(b));
    let floor = sorted[sorted.len() / 2] + threshold_db;
    let mut cand: Vec<usize> = (0..v.len())
        .filter(|&i| {
            v[i] >= floor && (i == 0 || v[i] >= v[i - 1]) && (i + 1 == v.len() || v[i] > v[i + 1])
        })
        .collect();
    cand.sort_by(|&a, &b| v[b].total_cmp(&v[a]));
    let mut taken: Vec<usize> = Vec::new();
    for i in cand {
        if taken.len() == count {
            break;
        }
        if taken.iter().all(|&j| i.abs_diff(j) > 2) {
            taken.push(i);
        }
    }
    taken
        .into_iter()
        .map(|i| Peak {
            freq_hz: bin_hz(t.start_hz, t.stop_hz, v.len(), i),
            level_dbm: v[i],
        })
        .collect()
}

static FILE_COUNTER: AtomicU64 = AtomicU64::new(0);

/// A new path in `out` named `<stem>_<milliseconds>_<counter>.<ext>`: two calls never get the same name.
pub fn new_path(out: &Path, stem: &str, ext: &str) -> PathBuf {
    // [@ANCHOR: bench_mcp_sa_new_path]
    let ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let n = FILE_COUNTER.fetch_add(1, Ordering::Relaxed);
    out.join(format!("{stem}_{ms}_{n}.{ext}"))
}

/// Writes a CSV: `# key=value` comment lines, the header, then one row per entry of `cols` (columns of equal length), with
/// `frequency` as the first column from `start` to `stop`. Returns the path.
pub fn write_csv(
    out: &Path,
    stem: &str,
    comments: &[(String, String)],
    header: &str,
    start: f64,
    stop: f64,
    cols: &[&[f64]],
) -> Result<PathBuf, String> {
    // [@ANCHOR: bench_mcp_sa_write_csv]
    let n = cols.first().map_or(0, |c| c.len());
    if n < 2 || cols.iter().any(|c| c.len() != n) {
        return Err("write_csv: the columns are empty or of different lengths".into());
    }
    let mut text = String::new();
    for (k, v) in comments {
        text.push_str(&format!("# {k}={v}\n"));
    }
    text.push_str(header);
    text.push('\n');
    for i in 0..n {
        text.push_str(&format!("{:.1}", bin_hz(start, stop, n, i)));
        for c in cols {
            text.push_str(&format!(",{:.3}", c[i]));
        }
        text.push('\n');
    }
    let _ = std::fs::create_dir_all(out);
    let p = new_path(out, stem, "csv");
    std::fs::write(&p, text).map_err(|e| format!("write {}: {e}", p.display()))?;
    Ok(p)
}

/// Sets up the analyzer from `req`, runs one sweep (or `average` sweeps) with continuous sweep off, and reads the trace.
/// The unit's own readbacks are what the returned `Trace` reports, and a range the unit clamped is an error. The wait for the
/// end of the sweep is `*OPC?` with a timeout of twice the unit's sweep time plus ten seconds; a sweep that would take more
/// than 120 s is refused before it starts. Continuous sweep is restored on every path when `continuous_after`.
pub fn run_sweep(l: &mut Link, req: &SweepReq) -> Result<Trace, String> {
    // [@ANCHOR: bench_mcp_sa_run_sweep]
    let r = sweep_inner(l, req);
    if req.continuous_after {
        let _ = l.send(":INITiate:CONTinuous ON");
    }
    r
}

fn sweep_inner(l: &mut Link, req: &SweepReq) -> Result<Trace, String> {
    // [@ANCHOR: bench_mcp_sa_sweep_inner]
    l.send(":INITiate:CONTinuous OFF")?;
    l.send(":UNIT:POWer DBM")?;
    l.send(":DISPlay:WINDow:TRACe:Y:SPACing LOGarithmic")?;
    l.send(":FORMat:TRACe:DATA ASCii")?;
    // Start, stop, start again: whichever order the unit's clamping needs, the last write of each is the wanted one.
    l.send(&format!(":SENSe:FREQuency:STARt {} Hz", req.span.start_hz))?;
    l.send(&format!(":SENSe:FREQuency:STOP {} Hz", req.span.stop_hz))?;
    l.send(&format!(":SENSe:FREQuency:STARt {} Hz", req.span.start_hz))?;
    if let Some(r) = req.rbw_hz {
        l.send(":SENSe:BWIDth:RESolution:AUTO OFF")?;
        l.send(&format!(":SENSe:BWIDth:RESolution {r} Hz"))?;
    }
    if let Some(v) = req.vbw_hz {
        l.send(":SENSe:BWIDth:VIDeo:AUTO OFF")?;
        l.send(&format!(":SENSe:BWIDth:VIDeo {v} Hz"))?;
    }
    if let Some(a) = req.attenuation_db {
        l.send(":SENSe:POWer:RF:ATTenuation:AUTO OFF")?;
        l.send(&format!(":SENSe:POWer:RF:ATTenuation {a}"))?;
    }
    // After the attenuator: the unit moved the reference level when it was set first (found 2026-11-05).
    if let Some(r) = req.ref_level_dbm {
        l.send(&format!(":DISPlay:WINDow:TRACe:Y:RLEVel {r}"))?;
    }
    if let Some(d) = req.detector {
        l.send(&format!(":SENSe:DETector:TRACe1:FUNCtion {d}"))?;
    }
    if req.average > 1 {
        l.send(":TRACe1:MODE AVERage")?;
        l.send(&format!(":SENSe:AVERage:TRACe1:COUNt {}", req.average))?;
        l.send(":SENSe:AVERage:TRACe1:CLEar")?;
    } else {
        l.send(":TRACe1:MODE WRITe")?;
    }
    l.send(&format!(":SENSe:SWEep:COUNt {}", req.average))?;
    let start = ask_f64(l, ":SENSe:FREQuency:STARt?")?;
    let stop = ask_f64(l, ":SENSe:FREQuency:STOP?")?;
    if (start - req.span.start_hz).abs() > 1.0 || (stop - req.span.stop_hz).abs() > 1.0 {
        return Err(format!(
            "the analyzer holds {start}..{stop} Hz, not the requested {}..{} Hz (it clamps a range it cannot make). Next: choose a range inside the model's limits.",
            req.span.start_hz, req.span.stop_hz
        ));
    }
    let sweep_time_s = ask_f64(l, ":SENSe:SWEep:TIME?")?;
    let total = sweep_time_s * req.average as f64;
    if total > MAX_SWEEP_S {
        return Err(format!(
            "this sweep would take {total:.0} s ({sweep_time_s:.1} s times {} sweeps), more than the {MAX_SWEEP_S:.0} s limit. Next: widen the RBW, narrow the span, or average fewer sweeps.",
            req.average
        ));
    }
    let t = Trace {
        start_hz: start,
        stop_hz: stop,
        dbm: Vec::new(),
        rbw_hz: ask_f64(l, ":SENSe:BWIDth?")?,
        vbw_hz: ask_f64(l, ":SENSe:BWIDth:VIDeo?")?,
        ref_level_dbm: ask_f64(l, ":DISPlay:WINDow:TRACe:Y:RLEVel?")?,
        attenuation_db: ask_f64(l, ":SENSe:POWer:RF:ATTenuation?")?,
        detector: ask(
            l,
            ":SENSe:DETector:TRACe1:FUNCtion?",
            READ_TRIES,
            REPLY_TIMEOUT_S,
        )?,
        sweep_time_s,
        average: req.average,
    };
    for (name, want, got) in [
        ("ref_level_dbm", req.ref_level_dbm, t.ref_level_dbm),
        (
            "attenuation_db",
            req.attenuation_db.map(f64::from),
            t.attenuation_db,
        ),
    ] {
        if want.is_some_and(|w| (w - got).abs() > 0.05) {
            return Err(format!(
                "the analyzer holds {name} {got}, not the requested {}. Next: choose a value that fits the other settings (the reference level and the attenuator are coupled), or call bench_sa_status.",
                want.unwrap_or(0.0)
            ));
        }
    }
    l.send(":INITiate:IMMediate")?;
    let wait = (total * 2.0 + 10.0).ceil() as u64;
    l.set_timeout_secs(wait);
    let done = l.query("*OPC?").map_err(|e| {
        format!("the sweep did not finish within {wait} s ({e}). Next: call bench_sa_status; power-cycle the analyzer if it does not answer.")
    })?;
    if done.trim() != "1" {
        return Err(format!("*OPC? answered {done:?}, not 1"));
    }
    let reply = ask(l, ":TRACe:DATA? 1", READ_TRIES, REPLY_TIMEOUT_S)?;
    let dbm = parse_trace(&reply)?;
    if dbm.len() != POINTS {
        return Err(format!(
            "the trace has {} points, not the {POINTS} this unit gives; refusing to guess the frequency axis",
            dbm.len()
        ));
    }
    Ok(Trace { dbm, ..t })
}

/// The summary of a trace as JSON: the unit's own settings, the median and extremes, the strongest peaks.
pub fn trace_summary(t: &Trace, peaks: &[Peak]) -> Value {
    // [@ANCHOR: bench_mcp_sa_trace_summary]
    let mut s = t.dbm.clone();
    s.sort_by(|a, b| a.total_cmp(b));
    json!({
        "points": t.dbm.len(), "start_hz": t.start_hz, "stop_hz": t.stop_hz,
        "bin_hz": (t.stop_hz - t.start_hz) / (t.dbm.len() - 1) as f64,
        "rbw_hz": t.rbw_hz, "vbw_hz": t.vbw_hz, "ref_level_dbm": t.ref_level_dbm,
        "attenuation_db": t.attenuation_db, "detector": t.detector, "sweep_time_s": t.sweep_time_s,
        "average_count": t.average,
        "min_dbm": s[0], "median_dbm": s[s.len() / 2], "max_dbm": s[s.len() - 1],
        "peaks": peaks.iter().map(|p| json!({"freq_hz": p.freq_hz, "level_dbm": p.level_dbm})).collect::<Vec<_>>(),
    })
}

/// `bench_sa_sweep`: checks the arguments, runs the sweep, writes `sa_sweep_*.csv` into `BENCH_MCP_OUT`, optionally switches
/// the tracking generator off afterwards (also when the sweep failed).
pub fn sweep(connect: &Connector, inst: &Instrument, args: &Value) -> Result<Value, String> {
    // [@ANCHOR: bench_mcp_sa_sweep]
    let (mut l, id) = open(connect, inst)?;
    let req = sweep_args(args, id.max_hz)?;
    let count = number(args, "peaks", 0.0, 50.0, "peaks listed")?.unwrap_or(5.0) as usize;
    let thr = number(args, "peak_threshold_db", 0.0, 200.0, "dB above the median")?.unwrap_or(10.0);
    let off_after = flag(args, "tg_off_after", false)?;
    let r = run_sweep(&mut l, &req);
    let tg_state = super::tg::tg_state(&mut l).ok();
    let off = off_after.then(|| super::tg::ensure_tg_off(&mut l));
    let t = super::tg::finish(r, off)?;
    let peaks = find_peaks(&t, count, thr);
    let file = write_csv(
        &crate::out_dir(),
        "sa_sweep",
        &[],
        "freq_hz,level_dbm",
        t.start_hz,
        t.stop_hz,
        &[&t.dbm],
    )?;
    let mut v = trace_summary(&t, &peaks);
    v["file"] = json!(file);
    v["tracking_generator"] = match (off_after, tg_state) {
        (true, _) => json!("off (verified)"),
        (false, Some((on, lvl))) => json!({"state": if on {"on"} else {"off"}, "level_dbm": lvl}),
        (false, None) => Value::Null,
    };
    Ok(v)
}

/// `bench_sa_marker`: marker `marker` (1 to 4, default 1) on trace 1, switched on, optionally searched (`search`: max, next,
/// left, right) or placed (`freq_hz`), then its frequency and level read back. The two options are exclusive.
pub fn marker(connect: &Connector, inst: &Instrument, args: &Value) -> Result<Value, String> {
    // [@ANCHOR: bench_mcp_sa_marker]
    let m = number(args, "marker", 1.0, 4.0, "markers 1 to 4")?.unwrap_or(1.0) as u32;
    let search = match args.get("search") {
        None | Some(Value::Null) => None,
        Some(v) => Some(match v.as_str() {
            Some("max") => "MAXimum",
            Some("next") => "MAXimum:NEXT",
            Some("left") => "MAXimum:LEFT",
            Some("right") => "MAXimum:RIGHt",
            _ => return Err("search must be max, next, left or right".into()),
        }),
    };
    let (mut l, id) = open(connect, inst)?;
    let freq = number(args, "freq_hz", 0.0, id.max_hz, "the model's top frequency")?;
    if search.is_some() && freq.is_some() {
        return Err("give search or freq_hz, not both".into());
    }
    l.send(&format!(":CALCulate:MARKer{m}:STATe ON"))?;
    l.send(&format!(":CALCulate:MARKer{m}:MODE POSition"))?;
    l.send(&format!(":CALCulate:MARKer{m}:TRACe 1"))?;
    if let Some(s) = search {
        l.send(&format!(":CALCulate:MARKer{m}:{s}"))?;
    }
    if let Some(f) = freq {
        l.send(&format!(":CALCulate:MARKer{m}:X {f} Hz"))?;
    }
    Ok(json!({
        "marker": m, "trace": 1,
        "freq_hz": ask_f64(&mut l, &format!(":CALCulate:MARKer{m}:X?"))?,
        "level_dbm": ask_f64(&mut l, &format!(":CALCulate:MARKer{m}:Y?"))?,
    }))
}

/// `bench_sa_screenshot`: reads the screen with `:HCOPy:SDUMp:DATA?` (a headerless 24-bit BMP of 1024 x 600), checks the BMP
/// header's own size against what arrived, and saves `sa_screen_*.bmp`.
pub fn screenshot(connect: &Connector, inst: &Instrument) -> Result<Value, String> {
    // [@ANCHOR: bench_mcp_sa_screenshot]
    let (mut l, _) = open(connect, inst)?;
    let raw = l.raw_until_idle(":HCOPy:SDUMp:DATA?", 1500, 40)?;
    let (w, h, size) = bmp_header(&raw)?;
    if raw.len() < size {
        return Err(format!(
            "screenshot: the BMP says {size} bytes and {} arrived. Next: call it again (do not use USB).",
            raw.len()
        ));
    }
    let p = new_path(&crate::out_dir(), "sa_screen", "bmp");
    std::fs::write(&p, &raw[..size]).map_err(|e| format!("write {}: {e}", p.display()))?;
    Ok(json!({"file": p, "bytes": size, "width": w, "height": h, "format": "BMP"}))
}

/// Width, height and total size from a BMP header; an error for anything that is not a plausible BMP.
pub fn bmp_header(raw: &[u8]) -> Result<(u32, u32, usize), String> {
    // [@ANCHOR: bench_mcp_sa_bmp_header]
    if raw.len() < 54 || &raw[..2] != b"BM" {
        return Err(format!(
            "screenshot: {} bytes that do not start like a BMP. Next: call bench_sa_status, then try again.",
            raw.len()
        ));
    }
    let u = |o: usize| u32::from_le_bytes([raw[o], raw[o + 1], raw[o + 2], raw[o + 3]]);
    let (size, w, h) = (u(2) as usize, u(18), u(22));
    if size < 54 || w == 0 || h == 0 || w > 4096 || h > 4096 {
        return Err("screenshot: the BMP header is not plausible".into());
    }
    Ok((w, h, size))
}

#[cfg(test)]
mod tests {
    use super::super::testkit::*;
    use super::super::tests::bench;
    use super::super::*;
    use super::*;

    /// Where the tools under test write: the output directory every test of this process shares (removed when the process exits).
    fn out() -> PathBuf {
        crate::out_dir()
    }

    // Tests [@ANCHOR: bench_mcp_sa_sweep_args]
    // Tests [@ANCHOR: bench_mcp_sa_span]
    // Tests [@ANCHOR: bench_mcp_sa_sweep_req]
    #[test]
    fn sweep_arguments_are_checked_before_anything_is_sent() {
        let ok = sweep_args(&json!({"center_hz": 100e6, "span_hz": 1e6, "rbw_hz": 1000, "detector": "sample", "average_count": 4}), 2.1e9).unwrap();
        assert_eq!(
            ok.span,
            Span {
                start_hz: 99.5e6,
                stop_hz: 100.5e6
            }
        );
        assert_eq!(
            (ok.rbw_hz, ok.average, ok.detector),
            (Some(1000.0), 4, Some("SAMPle"))
        );
        assert!(ok.continuous_after);
        assert!(sweep_args(&json!({"start_hz": 1e6, "stop_hz": 2e6}), 2.1e9).is_ok());
        for bad in [
            json!({}),
            json!({"start_hz": 1e6}),
            json!({"start_hz": 1e6, "stop_hz": 2e6, "span_hz": 10}),
            json!({"start_hz": 2e6, "stop_hz": 1e6}),
            json!({"start_hz": 1e6, "stop_hz": 3e9}),
            json!({"center_hz": 1e3, "span_hz": 1e6}),
            json!({"start_hz": 0, "stop_hz": 50}),
            json!({"start_hz": 1e6, "stop_hz": 2e6, "rbw_hz": 5}),
            json!({"start_hz": 1e6, "stop_hz": 2e6, "attenuation_db": 55}),
            json!({"start_hz": 1e6, "stop_hz": 2e6, "attenuation_db": 2.5}),
            json!({"start_hz": 1e6, "stop_hz": 2e6, "average_count": 0}),
            json!({"start_hz": 1e6, "stop_hz": 2e6, "detector": "quasi"}),
            json!({"start_hz": 1e6, "stop_hz": 2e6, "ref_level_dbm": 40}),
            json!({"start_hz": 1e6, "stop_hz": 2e6, "continuous_after": "yes"}),
        ] {
            assert!(sweep_args(&bad, 2.1e9).is_err(), "{bad}");
        }
    }

    // Tests [@ANCHOR: bench_mcp_sa_parse_trace]
    #[test]
    fn a_trace_must_be_all_finite_numbers() {
        assert_eq!(parse_trace("-1.0E+00, -2.5,").unwrap(), [-1.0, -2.5]);
        for bad in ["", "1.0", "1.0,x", "1.0,NaN", "1,inf"] {
            assert!(parse_trace(bad).is_err(), "{bad}");
        }
    }

    // Tests [@ANCHOR: bench_mcp_sa_bin_hz]
    // Tests [@ANCHOR: bench_mcp_sa_find_peaks]
    // Tests [@ANCHOR: bench_mcp_sa_peak]
    // Tests [@ANCHOR: bench_mcp_audio_analysis_peak_type]
    #[test]
    fn peaks_are_local_maxima_above_the_median_strongest_first_and_not_adjacent() {
        let mut v = vec![-90.0; 751];
        v[100] = -30.0;
        v[101] = -35.0;
        v[400] = -50.0;
        v[700] = -85.0;
        let t = Trace {
            start_hz: 0.0,
            stop_hz: 750.0,
            dbm: v,
            rbw_hz: 1.0,
            vbw_hz: 1.0,
            ref_level_dbm: 0.0,
            attenuation_db: 0.0,
            detector: "POSitive".into(),
            sweep_time_s: 1.0,
            average: 1,
        };
        assert_eq!(bin_hz(0.0, 750.0, 751, 100), 100.0);
        let p = find_peaks(&t, 5, 10.0);
        assert_eq!(
            p,
            [
                Peak {
                    freq_hz: 100.0,
                    level_dbm: -30.0
                },
                Peak {
                    freq_hz: 400.0,
                    level_dbm: -50.0
                }
            ]
        );
        assert_eq!(find_peaks(&t, 1, 10.0).len(), 1);
        assert!(find_peaks(&t, 0, 10.0).is_empty());
    }

    // Tests [@ANCHOR: bench_mcp_sa_new_path]
    // Tests [@ANCHOR: bench_mcp_sa_write_csv]
    #[test]
    fn csv_files_have_comments_a_header_one_row_per_point_and_never_share_a_name() {
        let d = out();
        let a: Vec<f64> = (0..5).map(|i| i as f64).collect();
        let p = write_csv(
            &d,
            "t",
            &[("k".into(), "v".into())],
            "freq_hz,x",
            100.0,
            200.0,
            &[&a],
        )
        .unwrap();
        let text = std::fs::read_to_string(&p).unwrap();
        assert_eq!(text.lines().next(), Some("# k=v"));
        assert_eq!(text.lines().nth(1), Some("freq_hz,x"));
        assert_eq!(text.lines().nth(2), Some("100.0,0.000"));
        assert_eq!(text.lines().last(), Some("200.0,4.000"));
        assert_ne!(new_path(&d, "t", "csv"), new_path(&d, "t", "csv"));
        assert!(write_csv(&d, "t", &[], "h", 0.0, 1.0, &[&a, &a[..2]]).is_err());
    }

    // Tests [@ANCHOR: bench_mcp_sa_run_sweep]
    // Tests [@ANCHOR: bench_mcp_sa_sweep_inner]
    // Tests [@ANCHOR: bench_mcp_sa_trace_summary]
    // Tests [@ANCHOR: bench_mcp_sa_trace]
    // Tests [@ANCHOR: bench_mcp_sa_sweep]
    #[test]
    fn a_sweep_sets_the_unit_reads_back_what_it_holds_saves_the_trace_and_restores_continuous() {
        let d = out();
        let dev = FakeDevice::sim(Sim::default());
        let v = call_with(
            &dev.connector(), &bench(""), "bench_sa_sweep",
            &json!({"start_hz": 1e6, "stop_hz": 751e6, "rbw_hz": 200, "attenuation_db": 10, "detector": "average", "average_count": 3}),
        ).unwrap();
        assert_eq!(v["points"], 751);
        assert_eq!(
            v["rbw_hz"].as_f64(),
            Some(300.0),
            "the unit rounded the RBW up and the tool reports the unit's value"
        );
        assert_eq!(v["average_count"], 3);
        assert_eq!(v["peaks"][0]["level_dbm"].as_f64(), Some(-30.0));
        assert_eq!(v["peaks"][0]["freq_hz"].as_f64(), Some(1e6 + 100.0 * 1e6));
        assert_eq!(v["tracking_generator"]["state"], "off");
        let file = PathBuf::from(v["file"].as_str().unwrap());
        assert!(file.starts_with(&d));
        let rows = std::fs::read_to_string(&file).unwrap();
        assert_eq!(rows.lines().count(), 752);
        let sent = dev.received();
        let pos = |c: &str| {
            sent.iter()
                .position(|s| s == c)
                .unwrap_or_else(|| panic!("{c} not sent: {sent:?}"))
        };
        assert!(pos(":INITiate:CONTinuous OFF") < pos(":INITiate:IMMediate"));
        assert!(pos(":INITiate:IMMediate") < pos("*OPC?") && pos("*OPC?") < pos(":TRACe:DATA? 1"));
        assert!(
            sent.contains(&":TRACe1:MODE AVERage".to_string())
                && sent.contains(&":SENSe:SWEep:COUNt 3".to_string())
        );
        assert_eq!(
            sent.iter()
                .filter(|c| c.starts_with(":OUTPut:STATe "))
                .count(),
            0,
            "the sweep does not touch the generator"
        );
        assert!(pos(":INITiate:CONTinuous ON") > pos(":TRACe:DATA? 1"));
        // continuous_after false leaves it off.
        let dev2 = FakeDevice::sim(Sim::default());
        call_with(
            &dev2.connector(),
            &bench(""),
            "bench_sa_sweep",
            &json!({"start_hz": 1e6, "stop_hz": 2e6, "continuous_after": false}),
        )
        .unwrap();
        assert!(!dev2
            .received()
            .contains(&":INITiate:CONTinuous ON".to_string()));
    }

    #[test]
    fn a_clamped_range_a_too_long_sweep_and_a_short_trace_are_errors_and_continuous_is_still_restored(
    ) {
        out();
        let sim = Sim {
            sweep_time: 90.0,
            ..Sim::default()
        };
        let long = FakeDevice::sim(sim);
        let e = call_with(
            &long.connector(),
            &bench(""),
            "bench_sa_sweep",
            &json!({"start_hz": 1e6, "stop_hz": 2e6, "average_count": 2}),
        )
        .unwrap_err();
        assert!(e.contains("120 s limit"), "{e}");
        assert!(long
            .received()
            .contains(&":INITiate:CONTinuous ON".to_string()));
        assert!(!long.received().contains(&":INITiate:IMMediate".to_string()));
        let clamp = FakeDevice::sim(Sim {
            clamp_stop: 1e9,
            ..Sim::default()
        });
        let e = call_with(
            &clamp.connector(),
            &bench(""),
            "bench_sa_sweep",
            &json!({"start_hz": 1e6, "stop_hz": 2e9}),
        )
        .unwrap_err();
        assert!(e.contains("clamps"), "{e}");
        assert!(clamp
            .received()
            .contains(&":INITiate:CONTinuous ON".to_string()));
        assert!(!clamp
            .received()
            .contains(&":INITiate:IMMediate".to_string()));
        let short = FakeDevice::start(|c| match c {
            "*IDN?" => Some("Siglent Technologies,SSA3021X,SN-0000,1.3.9.10".into()),
            ":TRACe:DATA? 1" => Some("-1.0,-2.0,-3.0".into()),
            "*OPC?" => Some("1".into()),
            ":SENSe:FREQuency:STOP?" => Some("+2.0E+06".into()),
            ":SENSe:SWEep:TIME?" => Some("+2.0E-01".into()),
            q if q.ends_with('?') => Some("+1.0E+06".into()),
            _ => None,
        });
        let e = call_with(
            &short.connector(),
            &bench(""),
            "bench_sa_sweep",
            &json!({"start_hz": 1e6, "stop_hz": 2e6}),
        )
        .unwrap_err();
        assert!(e.contains("not the 751"), "{e}");
    }

    #[test]
    fn a_reference_level_or_attenuation_the_unit_does_not_keep_is_a_coupled_error_and_nothing_is_swept(
    ) {
        out();
        for (query, setting, name) in [
            (
                ":DISPlay:WINDow:TRACe:Y:RLEVel?",
                json!({"ref_level_dbm": -10}),
                "ref_level_dbm",
            ),
            (
                ":SENSe:POWer:RF:ATTenuation?",
                json!({"attenuation_db": 10}),
                "attenuation_db",
            ),
        ] {
            let liar = FakeDevice::sim(Sim {
                garbage: Some((query.into(), "+5.0E+00".into())),
                ..Sim::default()
            });
            let mut args = json!({"start_hz": 1e6, "stop_hz": 2e6});
            args.as_object_mut()
                .unwrap()
                .extend(setting.as_object().unwrap().clone());
            let e = call_with(&liar.connector(), &bench(""), "bench_sa_sweep", &args).unwrap_err();
            assert!(e.contains(name) && e.contains("coupled"), "{e}");
            assert!(!liar.received().contains(&":INITiate:IMMediate".to_string()));
            assert!(
                liar.received()
                    .contains(&":INITiate:CONTinuous ON".to_string()),
                "continuous restored"
            );
        }
    }

    // Tests [@ANCHOR: bench_mcp_sa_marker]
    #[test]
    fn a_marker_searches_or_is_placed_and_reads_back_frequency_and_level() {
        out();
        let dev = FakeDevice::sim(Sim::default());
        call_with(
            &dev.connector(),
            &bench(""),
            "bench_sa_sweep",
            &json!({"start_hz": 0, "stop_hz": 750e6}),
        )
        .unwrap();
        let v = call_with(
            &dev.connector(),
            &bench(""),
            "bench_sa_marker",
            &json!({"marker": 2, "search": "max"}),
        )
        .unwrap();
        assert_eq!(
            (
                v["marker"].as_u64(),
                v["freq_hz"].as_f64(),
                v["level_dbm"].as_f64()
            ),
            (Some(2), Some(100e6), Some(-30.0))
        );
        assert!(dev
            .received()
            .contains(&":CALCulate:MARKer2:MAXimum".to_string()));
        let v = call_with(
            &dev.connector(),
            &bench(""),
            "bench_sa_marker",
            &json!({"freq_hz": 200e6}),
        )
        .unwrap();
        assert_eq!(v["freq_hz"].as_f64(), Some(200e6));
        for bad in [
            json!({"marker": 5}),
            json!({"search": "min"}),
            json!({"search": "max", "freq_hz": 1e6}),
            json!({"freq_hz": 3e9}),
        ] {
            assert!(
                call_with(&dev.connector(), &bench(""), "bench_sa_marker", &bad).is_err(),
                "{bad}"
            );
        }
    }

    // Tests [@ANCHOR: bench_mcp_sa_screenshot]
    // Tests [@ANCHOR: bench_mcp_sa_bmp_header]
    #[test]
    fn a_screenshot_is_saved_when_the_bmp_is_complete_and_refused_when_short_or_not_a_bmp() {
        let d = out();
        let mut bmp = vec![0u8; 54 + 3 * 8 * 4];
        bmp[..2].copy_from_slice(b"BM");
        let size = bmp.len() as u32;
        bmp[2..6].copy_from_slice(&size.to_le_bytes());
        bmp[18..22].copy_from_slice(&8u32.to_le_bytes());
        bmp[22..26].copy_from_slice(&4u32.to_le_bytes());
        assert_eq!(bmp_header(&bmp), Ok((8, 4, bmp.len())));
        let mut with_newline = bmp.clone();
        with_newline.push(b'\n');
        let port = bmp_device(with_newline);
        let connect = move |_: &Instrument| Link::tcp("127.0.0.1", port);
        let v = call_with(&connect, &bench(""), "bench_sa_screenshot", &json!({})).unwrap();
        assert_eq!(
            (
                v["width"].as_u64(),
                v["height"].as_u64(),
                v["bytes"].as_u64()
            ),
            (Some(8), Some(4), Some(size as u64))
        );
        let f = PathBuf::from(v["file"].as_str().unwrap());
        assert!(f.starts_with(&d));
        assert_eq!(std::fs::read(&f).unwrap(), bmp);
        let short = bmp_device(bmp[..60].to_vec());
        let connect = move |_: &Instrument| Link::tcp("127.0.0.1", short);
        assert!(
            call_with(&connect, &bench(""), "bench_sa_screenshot", &json!({}))
                .unwrap_err()
                .contains("arrived")
        );
        let junk = bmp_device(vec![b'x'; 200]);
        let connect = move |_: &Instrument| Link::tcp("127.0.0.1", junk);
        assert!(
            call_with(&connect, &bench(""), "bench_sa_screenshot", &json!({}))
                .unwrap_err()
                .contains("BMP")
        );
        assert!(bmp_header(b"BM").is_err());
    }
}
