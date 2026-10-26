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
