// SPDX-License-Identifier: AGPL-3.0-or-later
//! Siglent SDS1000X-E series oscilloscope (written against an SDS1202X-E), SCPI over TCP 5025.

use crate::link::Link;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

fn ch(args: &Value) -> Result<u8, String> {
    match args.get("channel").and_then(|c| c.as_u64()) {
        Some(c @ 1..=2) => Ok(c as u8),
        _ => Err("channel must be 1 or 2".into()),
    }
}

fn stamp() -> String {
    let t = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    t.to_string()
}

pub fn status(s: &mut Link) -> Result<Value, String> {
    let mode = s.query("TRIG_MODE?")?;
    let tdiv = s.number("TDIV?")?;
    let sara = s.number("SARA?")?;
    let mut chans = Vec::new();
    for c in 1..=2 {
        chans.push(json!({
            "channel": c,
            "vdiv_v": s.number(&format!("C{c}:VDIV?"))?,
            "offset_v": s.number(&format!("C{c}:OFST?"))?,
            "coupling": Link::value(&s.query(&format!("C{c}:CPL?"))?),
            "attenuation": s.number(&format!("C{c}:ATTN?"))?,
        }));
    }
    let m = Link::value(&mode).to_string();
    Ok(
        json!({"trigger_mode": m, "acquisition": if m == "STOP" { "stopped" } else { "running" },
              "timebase_s_per_div": tdiv, "sample_rate_sa_s": sara, "channels": chans}),
    )
}

pub fn measure(s: &mut Link, args: &Value) -> Result<Value, String> {
    let c = ch(args)?;
    let items: Vec<String> = match args.get("items").and_then(|i| i.as_array()) {
        Some(a) => a
            .iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect(),
        None => ["PKPK", "MEAN", "FREQ", "DUTY"]
            .iter()
            .map(|s| s.to_string())
            .collect(),
    };
    let mut m = serde_json::Map::new();
    for it in items {
        if !it.chars().all(|c| c.is_ascii_uppercase()) {
            return Err("bad item".into());
        }
        let r = s.query(&format!("C{c}:PAVA? {it}"))?;
        m.insert(it, json!(Link::value(&r)));
    }
    Ok(Value::Object(m))
}

struct Analysis {
    min: f32,
    max: f32,
    mean: f64,
    edges: u32,
    freq: Option<f64>,
}

/// Summary of a captured waveform. Rising edges are counted through a hysteresis band of 20% of the
/// span around the midpoint, so noise on a flat level is not counted as edges.
fn analyse(volts: &[f32], sara: f64) -> Analysis {
    // [@ANCHOR: scope_mcp_analyse]
    let (mn, mx) = volts
        .iter()
        .fold((f32::MAX, f32::MIN), |(a, b), &v| (a.min(v), b.max(v)));
    let mean = volts.iter().map(|&v| v as f64).sum::<f64>() / volts.len() as f64;
    let mid = (mn + mx) / 2.0;
    let (hi, lo) = (mid + 0.1 * (mx - mn), mid - 0.1 * (mx - mn));
    let (mut state_high, mut first, mut last, mut edges) =
        (volts[0] > mid, None::<usize>, 0usize, 0u32);
    for (i, &v) in volts.iter().enumerate() {
        if state_high && v < lo {
            state_high = false;
        } else if !state_high && v > hi {
            state_high = true;
            edges += 1;
            first.get_or_insert(i);
            last = i;
        }
    }
    let freq = match first {
        Some(f) if edges > 1 => Some(sara * (edges - 1) as f64 / (last - f) as f64),
        _ => None,
    };
    Analysis {
        min: mn,
        max: mx,
        mean,
        edges,
        freq,
    }
}

pub fn capture(s: &mut Link, args: &Value, out: &Path) -> Result<Value, String> {
    let c = ch(args)?;
    let stopped = Link::value(&s.query("TRIG_MODE?")?) == "STOP";
    if stopped && args.get("allow_stopped").and_then(|v| v.as_bool()) != Some(true) {
        return Err("the scope is stopped, so its waveform memory is an old frozen frame. Set trigger_mode AUTO (bench_set) or pass allow_stopped=true.".into());
    }
    // Every point, no decimation: without this the scope returned a few samples on this unit (found 2026-10-28).
    s.send("WFSU SP,1,NP,0,FP,0")?;
    let vdiv = s.number(&format!("C{c}:VDIV?"))?;
    let ofst = s.number(&format!("C{c}:OFST?"))?;
    let sara = s.number("SARA?")?;
    s.set_timeout_secs(30);
    let raw = s.block(&format!("C{c}:WF? DAT2"));
    s.set_timeout_secs(10);
    let raw = raw?;
    let volts: Vec<f32> = raw
        .iter()
        .map(|&b| ((b as i8) as f32) * (vdiv as f32) / 25.0 - ofst as f32)
        .collect();
    if volts.is_empty() {
        return Err("empty waveform".into());
    }
    let st = analyse(&volts, sara);
    let path: PathBuf = out.join(format!("ch{c}_{}.f32", stamp()));
    let bytes: Vec<u8> = volts.iter().flat_map(|v| v.to_le_bytes()).collect();
    std::fs::write(&path, bytes).map_err(|e| format!("write {}: {e}", path.display()))?;
    Ok(
        json!({"channel": c, "samples": volts.len(), "sample_interval_s": 1.0 / sara, "acquisition_was_stopped": stopped,
              "min_v": st.min, "max_v": st.max, "mean_v": st.mean, "pkpk_v": st.max - st.min,
              "rising_edges": st.edges, "estimated_frequency_hz": st.freq, "file": path, "file_format": "little-endian f32 volts"}),
    )
}

/// The spectrum of the channel's waveform, computed here: the scope is put on `timebase_s` if given (more time per division means finer
/// frequency resolution, fewer samples per carrier cycle), the first `points` samples (a power of two, default 131072, at most 1048576) are
/// windowed and transformed, and the strongest peaks are listed. Levels are dBV rms at the scope input, and dBm into 50 ohms (the level
/// the 50 ohm source side would read; the scope input is 1 MOhm).
pub fn spectrum(s: &mut Link, args: &Value, out: &Path) -> Result<Value, String> {
    let c = ch(args)?;
    let window = crate::spectrum::Window::parse(
        args.get("window")
            .and_then(|v| v.as_str())
            .unwrap_or("hanning"),
    )?;
    let points = match args.get("points") {
        None | Some(Value::Null) => 131_072,
        Some(v) => v
            .as_u64()
            .filter(|p| p.is_power_of_two() && (16..=1_048_576).contains(p))
            .ok_or("points must be a power of two from 16 to 1048576")? as usize,
    };
    let count = args
        .get("peaks")
        .and_then(|v| v.as_u64())
        .unwrap_or(10)
        .clamp(1, 50) as usize;
    let range_db = args
        .get("range_db")
        .and_then(|v| v.as_f64())
        .unwrap_or(60.0);
    if let Some(m) = args.get("memory_depth").and_then(|v| v.as_str()) {
        // e.g. 7K, 70K, 700K, 7M, 14M: the scope's own memory depth names (a bad one is refused by the scope, not sent blindly)
        if m.is_empty()
            || m.len() > 6
            || !m
                .chars()
                .all(|c| c.is_ascii_digit() || c == '.' || c == 'K' || c == 'M')
        {
            return Err("memory_depth must look like 14K, 140K, 1.4M or 14M".into());
        }
        s.send(&format!("MSIZ {m}"))?;
        std::thread::sleep(std::time::Duration::from_millis(1000));
    }
    if let Some(t) = args.get("timebase_s").and_then(|v| v.as_f64()) {
        if !(t.is_finite() && t > 0.0) {
            return Err("timebase_s must be positive".into());
        }
        s.send(&format!("TDIV {t:E}S"))?;
        std::thread::sleep(std::time::Duration::from_millis(1500));
    }
    let wf = capture(
        s,
        &json!({"channel": c, "allow_stopped": args.get("allow_stopped")}),
        out,
    )?;
    let file = wf["file"].as_str().ok_or("no capture file")?.to_string();
    let bytes = std::fs::read(&file).map_err(|e| format!("{file}: {e}"))?;
    let volts: Vec<f32> = bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|b| f32::from_le_bytes(*b))
        .collect();
    let dt = wf["sample_interval_s"]
        .as_f64()
        .ok_or("no sample interval")?;
    if volts.len() < points {
        return Err(format!("the scope returned {} samples, fewer than points = {points}; lower points or raise the memory depth", volts.len()));
    }
    let sp = crate::spectrum::spectrum(&volts[..points], 1.0 / dt, window)?;
    let peaks = crate::spectrum::peaks(&sp, count, range_db);
    let _ = std::fs::remove_file(&file);
    Ok(json!({
        "channel": c, "window": format!("{window:?}"), "points": points, "sample_rate_hz": 1.0 / dt,
        "bin_hz": sp.bin_hz, "span_hz": 0.5 / dt, "enbw_bins": sp.enbw_bins,
        "note": "levels are rms at the scope input; dbm_50 is dBV + 13.01 (a 50 ohm source into 50 ohms); 8-bit samples: lines more than about 45 dB below the strongest are not reliable",
        "peaks": peaks.iter().map(|p| json!({"freq_hz": p.freq_hz, "vrms": p.vrms, "dbv": p.dbv, "dbm_50": p.dbm_50})).collect::<Vec<_>>(),
    }))
}

pub fn screenshot(s: &mut Link, out: &Path) -> Result<Value, String> {
    let raw = s.raw_until_idle("SCDP", 1500, 30)?;
    if raw.len() < 100 {
        return Err(format!("screenshot: only {} bytes", raw.len()));
    }
    let ext = if raw.starts_with(b"BM") {
        "bmp"
    } else if raw.starts_with(b"\x89PNG") {
        "png"
    } else {
        "bin"
    };
    let p = out.join(format!("screen_{}.{ext}", stamp()));
    std::fs::write(&p, &raw).map_err(|e| e.to_string())?;
    Ok(json!({"file": p, "bytes": raw.len()}))
}

pub fn set(s: &mut Link, args: &Value) -> Result<Value, String> {
    let mut done = Vec::new();
    let mut cmd = |s: &mut Link, c: String| -> Result<(), String> {
        s.send(&c)?;
        done.push(c);
        Ok(())
    };
    if let Some(v) = args.get("timebase_s").and_then(|v| v.as_f64()) {
        cmd(s, format!("TDIV {v:E}S"))?;
    }
    if ["vdiv_v", "offset_v", "coupling", "attenuation"]
        .iter()
        .any(|k| args.get(k).is_some())
    {
        let c = ch(args)?;
        if let Some(v) = args.get("vdiv_v").and_then(|v| v.as_f64()) {
            cmd(s, format!("C{c}:VDIV {v:E}V"))?;
        }
        if let Some(v) = args.get("offset_v").and_then(|v| v.as_f64()) {
            cmd(s, format!("C{c}:OFST {v:E}V"))?;
        }
        if let Some(v) = args.get("coupling").and_then(|v| v.as_str()) {
            if !["A1M", "D1M", "GND"].contains(&v) {
                return Err("bad coupling".into());
            }
            cmd(s, format!("C{c}:CPL {v}"))?;
        }
        if let Some(v) = args.get("attenuation").and_then(|v| v.as_f64()) {
            cmd(s, format!("C{c}:ATTN {v}"))?;
        }
    }
    if let Some(v) = args.get("trigger_mode").and_then(|v| v.as_str()) {
        if !["AUTO", "NORM", "SINGLE", "STOP"].contains(&v) {
            return Err("bad trigger_mode".into());
        }
        cmd(s, format!("TRIG_MODE {v}"))?;
    }
    if let Some(t) = args.get("trigger_type").and_then(|v| v.as_str()) {
        // The types this unit (firmware 1.3.27) accepted when probed: edge, slope (SLEW), pulse (GLIT), interval (INTV), runt (RUNT), TV. The
        // pattern, window, drop-out and serial-bus triggers were not accepted with the tokens tried (licensed options or other names).
        if !["EDGE", "SLEW", "GLIT", "INTV", "RUNT", "TV"].contains(&t) {
            return Err("trigger_type must be EDGE, SLEW (slope), GLIT (pulse), INTV (interval), RUNT or TV".into());
        }
        let c = ch(args)?;
        cmd(s, format!("TRSE {t},SR,C{c}"))?;
    }
    std::thread::sleep(std::time::Duration::from_millis(500));
    Ok(json!({"sent": done, "status": status(s)?}))
}

pub fn self_cal(s: &mut Link, args: &Value) -> Result<Value, String> {
    if args
        .get("confirm_inputs_disconnected")
        .and_then(|v| v.as_bool())
        != Some(true)
    {
        return Err("refused: confirm_inputs_disconnected must be true, after the person at the bench has unplugged every probe".into());
    }
    s.set_timeout_secs(300);
    let r = s.query("*CAL?");
    s.set_timeout_secs(10);
    Ok(
        json!({"cal_reply": r?, "note": "0 means the scope accepted the command; the calibration may keep running for a minute or two. Wait before measuring. The scope may reset its vertical settings."}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    // Tests [@ANCHOR: scope_mcp_analyse]
    #[test]
    fn square_wave_edges_and_frequency() {
        // 1 kHz square wave sampled at 1 MSa/s, 0 V to 3 V, with a little noise on the levels.
        let v: Vec<f32> = (0..10_000)
            .map(|i| {
                if (i / 500) % 2 == 1 {
                    3.0 + 0.02 * ((i % 7) as f32 - 3.0)
                } else {
                    0.02 * ((i % 5) as f32 - 2.0)
                }
            })
            .collect();
        let a = analyse(&v, 1.0e6);
        assert_eq!(a.edges, 10);
        let f = a.freq.unwrap();
        assert!((f - 1000.0).abs() < 1.0, "{f}");
        assert!(a.max > 3.0 && a.min < 0.0);
    }

    #[test]
    fn flat_line_has_no_edges_or_frequency() {
        let a = analyse(&[-3.0f32; 1000], 1.0e9);
        assert_eq!((a.edges, a.freq), (0, None));
    }
}

#[cfg(test)]
mod spectrum_tests {
    use super::*;
    use std::io::{BufRead, BufReader, Write};

    /// A scope on a local socket that answers the queries `spectrum` makes and serves a 10 MHz, 20 mV peak sine (8-bit samples at
    /// 5 mV per division, 500 MS/s) with a 30 MHz line 26 dB down.
    fn fake_scope() -> u16 {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            let (sock, _) = listener.accept().unwrap();
            let mut out = sock.try_clone().unwrap();
            for line in BufReader::new(sock).lines() {
                let l = line.unwrap();
                let reply: Vec<u8> = match l.as_str() {
                    "TRIG_MODE?" => b"TRIG_MODE AUTO\n".to_vec(),
                    "C1:VDIV?" => b"C1:VDIV 5.00E-03V\n".to_vec(),
                    "C1:OFST?" => b"C1:OFST 0.00E+00V\n".to_vec(),
                    "SARA?" => b"SARA 5.00E+08Sa/s\n".to_vec(),
                    "C1:WF? DAT2" => {
                        let n = 200_000usize;
                        let data: Vec<u8> = (0..n)
                            .map(|i| {
                                let t = i as f64 / 500e6;
                                let v = 100.0 * (2.0 * std::f64::consts::PI * 10e6 * t).sin()
                                    + 5.0 * (2.0 * std::f64::consts::PI * 30e6 * t).sin();
                                (v.round() as i8) as u8
                            })
                            .collect();
                        let mut r = format!("C1:WF DAT2,#9{n:09}").into_bytes();
                        r.extend(data);
                        r.extend(b"\n\n");
                        r
                    }
                    _ => continue,
                };
                out.write_all(&reply).unwrap();
            }
        });
        port
    }

    // Tests [@ANCHOR: bench_mcp_spectrum]
    #[test]
    fn the_spectrum_tool_finds_the_tone_and_its_harmonic_through_a_scope_link() {
        let port = fake_scope();
        let mut link = Link::tcp("127.0.0.1", port).unwrap();
        let dir = std::env::temp_dir().join(format!("bench_spec_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let r = spectrum(
            &mut link,
            &json!({"channel": 1, "window": "hanning", "points": 131072, "peaks": 4}),
            &dir,
        )
        .unwrap();
        std::fs::remove_dir_all(&dir).ok();
        let p = r["peaks"].as_array().unwrap();
        let f0 = p[0]["freq_hz"].as_f64().unwrap();
        assert!((f0 - 10e6).abs() < 1e4, "{f0}");
        assert!(
            (p[0]["dbv"].as_f64().unwrap() - 20.0 * (0.020f64 / 2f64.sqrt()).log10()).abs() < 1.0,
            "{}",
            p[0]["dbv"]
        );
        let h = p
            .iter()
            .find(|x| (x["freq_hz"].as_f64().unwrap() - 30e6).abs() < 2e4)
            .expect("30 MHz line");
        assert!((h["dbv"].as_f64().unwrap() - p[0]["dbv"].as_f64().unwrap() + 26.0).abs() < 2.0);
        // Refusals before any waveform is read.
        let mut l2 = Link::tcp("127.0.0.1", fake_scope()).unwrap();
        assert!(spectrum(
            &mut l2,
            &json!({"channel": 1, "points": 1000}),
            Path::new("/tmp")
        )
        .is_err());
        assert!(spectrum(
            &mut l2,
            &json!({"channel": 1, "window": "flattop"}),
            Path::new("/tmp")
        )
        .is_err());
        assert!(spectrum(&mut l2, &json!({"channel": 3}), Path::new("/tmp")).is_err());
    }
}
