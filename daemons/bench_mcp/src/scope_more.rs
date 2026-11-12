// SPDX-License-Identifier: AGPL-3.0-or-later
//! More Siglent SDS1000X-E functions: channel math and cursors computed on the host from the waveform memory (the scope's own math and
//! cursor commands were not probed), and the acquisition, channel and display settings with a read-back of what the scope reports.

use crate::link::Link;
use crate::scope::{ch, read_waveform, stamp};
use serde_json::{json, Value};
use std::path::Path;

/// The trigger mode before a tool froze the scope for a coherent two-channel frame.
fn run_state(s: &mut Link) -> Result<String, String> {
    Ok(Link::value(&s.query("TRIG_MODE?")?).to_string())
}

/// `bench_scope_math`: add, subtract, multiply or divide channel 1 by channel 2, sample by sample. A running scope is stopped for the reading
/// so both channels come from the same frame, then put back in the mode it was in.
pub fn math(s: &mut Link, args: &Value, out: &Path) -> Result<Value, String> {
    // [@ANCHOR: bench_mcp_scope_math]
    let op = args.get("op").and_then(Value::as_str).unwrap_or("");
    if !matches!(op, "add" | "sub" | "mul" | "div") {
        return Err("op must be add, sub, mul or div (channel 1 op channel 2)".into());
    }
    let before = run_state(s)?;
    let frozen = before != "STOP";
    if frozen {
        s.send("TRIG_MODE STOP")?;
    }
    let read = (|| {
        let a = read_waveform(s, 1)?;
        let b = read_waveform(s, 2)?;
        Ok::<_, String>((a, b))
    })();
    if frozen && matches!(before.as_str(), "AUTO" | "NORM" | "SINGLE") {
        // The scope must not be left stopped by a failed read either.
        s.send(&format!("TRIG_MODE {before}"))?;
    }
    let ((a, sara_a), (b, sara_b)) = read?;
    if (sara_a - sara_b).abs() > 1e-6 * sara_a {
        return Err(format!(
            "the channels were read at different sample rates ({sara_a} and {sara_b} Sa/s)"
        ));
    }
    let (r, undefined) = combine(op, &a, &b);
    let st = summary(&r);
    let path = out.join(format!("math_{op}_{}.f32", stamp()));
    let bytes: Vec<u8> = r.iter().flat_map(|v| v.to_le_bytes()).collect();
    std::fs::write(&path, bytes).map_err(|e| format!("write {}: {e}", path.display()))?;
    Ok(json!({
        "op": format!("C1 {op} C2"), "samples": r.len(), "sample_interval_s": 1.0 / sara_a, "frame_frozen_for_the_reading": frozen,
        "undefined_points": undefined, "min": st.0, "max": st.1, "mean": st.2, "rms": st.3, "file": path,
        "file_format": "little-endian f32; volts, or volts squared for mul, or a ratio for div (0 where channel 2 was under 1 uV)",
    }))
}

/// The combined samples (as many as the shorter channel has) and how many divisions were by a channel-2 value under 1 uV (those give 0).
// [@ANCHOR: bench_mcp_scope_combine]
pub(crate) fn combine(op: &str, a: &[f32], b: &[f32]) -> (Vec<f32>, usize) {
    let mut undefined = 0;
    let r = a
        .iter()
        .zip(b)
        .map(|(x, y)| match op {
            "add" => x + y,
            "sub" => x - y,
            "mul" => x * y,
            _ if y.abs() < 1e-6 => {
                undefined += 1;
                0.0
            }
            _ => x / y,
        })
        .collect();
    (r, undefined)
}

fn summary(v: &[f32]) -> (f32, f32, f64, f64) {
    // [@ANCHOR: bench_mcp_scope_more_summary]
    let (mn, mx) = v
        .iter()
        .fold((f32::MAX, f32::MIN), |(a, b), &x| (a.min(x), b.max(x)));
    let n = v.len().max(1) as f64;
    let mean = v.iter().map(|&x| f64::from(x)).sum::<f64>() / n;
    let rms = (v.iter().map(|&x| f64::from(x).powi(2)).sum::<f64>() / n).sqrt();
    (mn, mx, mean, rms)
}

/// The value at `t` seconds from the first sample, by linear interpolation; `None` outside the record.
// [@ANCHOR: bench_mcp_scope_value_at]
pub(crate) fn value_at(v: &[f32], sara: f64, t: f64) -> Option<f64> {
    let x = t * sara;
    if !x.is_finite() || x < 0.0 || x > (v.len() - 1) as f64 {
        return None;
    }
    let i = x.floor() as usize;
    let frac = x - i as f64;
    let next = *v.get(i + 1).unwrap_or(&v[i]);
    Some(f64::from(v[i]) * (1.0 - frac) + f64::from(next) * frac)
}

/// `bench_scope_cursors`: the value of a channel at up to eight times (seconds from the first sample of the waveform memory), and with
/// two times the differences and the frequency 1 / delta t.
pub fn cursors(s: &mut Link, args: &Value) -> Result<Value, String> {
    // [@ANCHOR: bench_mcp_scope_cursors]
    let c = ch(args)?;
    let times: Vec<f64> = args
        .get("times_s")
        .and_then(Value::as_array)
        .ok_or("times_s is required: one to eight times in seconds from the first sample")?
        .iter()
        .map(|v| {
            v.as_f64()
                .filter(|x| x.is_finite())
                .ok_or("times_s must be numbers")
        })
        .collect::<Result<_, _>>()?;
    if times.is_empty() || times.len() > 8 {
        return Err("times_s must hold one to eight times".into());
    }
    let stopped = run_state(s)? == "STOP";
    if stopped && args.get("allow_stopped").and_then(Value::as_bool) != Some(true) {
        return Err("the scope is stopped, so its waveform memory is an old frozen frame. Set trigger_mode AUTO (bench_set) or pass allow_stopped=true.".into());
    }
    let (v, sara) = read_waveform(s, c)?;
    let record_s = (v.len() - 1) as f64 / sara;
    let vals: Vec<Option<f64>> = times.iter().map(|&t| value_at(&v, sara, t)).collect();
    let rows: Vec<Value> = times
        .iter()
        .zip(&vals)
        .map(|(t, y)| json!({"time_s": t, "volts": y}))
        .collect();
    let mut r = json!({"channel": c, "record_s": record_s, "sample_interval_s": 1.0 / sara, "cursors": rows,
        "note": "time is measured from the first sample of the waveform memory, not from the trigger; the value is a linear interpolation"});
    if let (2, Some(Some(a)), Some(Some(b))) = (times.len(), vals.first(), vals.get(1)) {
        let dt = times[1] - times[0];
        r["delta_time_s"] = json!(dt);
        r["delta_volts"] = json!(b - a);
        r["frequency_hz"] = json!(if dt != 0.0 {
            Some(1.0 / dt.abs())
        } else {
            None
        });
    }
    Ok(r)
}

/// Asks the scope for a setting and returns what it answers, or says it did not answer.
pub(crate) fn readback(s: &mut Link, query: &str) -> Value {
    // [@ANCHOR: bench_mcp_scope_more_readback]
    s.set_timeout_secs(3);
    let r = s.query(query);
    s.set_timeout_secs(10);
    match r {
        Ok(t) if !t.trim().is_empty() => json!(t.trim()),
        Ok(_) => json!("(empty reply)"),
        Err(e) => json!(format!("no answer to {query}: {e}")),
    }
}

/// Sends the commands one by one and reads each setting back so the answer shows what the scope accepted.
pub(crate) fn apply(s: &mut Link, cmds: Vec<(String, String)>) -> Result<Value, String> {
    // [@ANCHOR: bench_mcp_scope_more_apply]
    let mut sent = Vec::new();
    let mut back = serde_json::Map::new();
    for (cmd, query) in cmds {
        s.send(&cmd)?;
        sent.push(cmd);
        std::thread::sleep(std::time::Duration::from_millis(100));
        back.insert(query.clone(), readback(s, &query));
    }
    Ok(json!({"sent": sent, "readback": back,
        "note": "a setting the scope did not accept shows its old value or no answer in readback; these commands are written from the SDS1000X-E programming guide and have not all been confirmed on firmware 1.3.27"}))
}

/// How a tool argument is checked and written into a scope command.
#[derive(Clone, Copy)]
pub(crate) enum Ty {
    /// A JSON boolean, sent as ON or OFF.
    Flag,
    /// One of these words (compared in upper case; a whole number counts as its digits), sent as it is.
    Words(&'static [&'static str]),
    /// A number from the first to the second value, sent in E-notation followed by the unit (`S`, `V`, `Hz` or none).
    Num(f64, f64, &'static str),
    /// A whole number from the first to the second value.
    Int(u64, u64),
}

/// One setting of a scope tool: the tool argument that names it, the command it is written with (a `{c}` stands for the channel) and how the value is checked.
pub(crate) struct Item {
    pub arg: &'static str,
    pub cmd: &'static str,
    pub ty: Ty,
}

/// The commands (command, query to read it back) the arguments ask for, after checking every one of them. Nothing is sent here, so a bad value
/// anywhere refuses the whole call before the scope sees a command.
pub(crate) fn item_cmds(
    args: &Value,
    items: &[Item],
    c: u8,
) -> Result<Vec<(String, String)>, String> {
    // [@ANCHOR: bench_mcp_scope_item_cmds]
    let mut cmds = Vec::new();
    for it in items {
        let v = match args.get(it.arg) {
            None | Some(Value::Null) => continue,
            Some(v) => v,
        };
        let bad = |what: &str| format!("{} must be {what}", it.arg);
        let text = match it.ty {
            Ty::Flag => match v.as_bool() {
                Some(b) => if b { "ON" } else { "OFF" }.to_string(),
                None => return Err(bad("true or false")),
            },
            Ty::Words(w) => match v
                .as_str()
                .map(str::to_uppercase)
                .or_else(|| v.as_u64().map(|n| n.to_string()))
            {
                Some(x) if w.contains(&x.as_str()) => x,
                _ => return Err(bad(&format!("one of {}", w.join(", ")))),
            },
            Ty::Num(lo, hi, unit) => match v
                .as_f64()
                .filter(|x| x.is_finite() && (lo..=hi).contains(x))
            {
                Some(x) => format!("{x:E}{unit}"),
                None => return Err(bad(&format!("a number from {lo} to {hi}"))),
            },
            Ty::Int(lo, hi) => match v.as_u64().filter(|x| (lo..=hi).contains(x)) {
                Some(x) => x.to_string(),
                None => return Err(bad(&format!("a whole number from {lo} to {hi}"))),
            },
        };
        let head = it.cmd.replace("{c}", &c.to_string());
        cmds.push((format!("{head} {text}"), format!("{head}?")));
    }
    Ok(cmds)
}

/// What the scope reports for each setting of the table, by argument name (the read-back of every setting, not only those just changed).
pub(crate) fn read_items(s: &mut Link, items: &[Item], c: u8) -> Value {
    // [@ANCHOR: bench_mcp_scope_read_items]
    let mut m = serde_json::Map::new();
    for it in items {
        m.insert(
            it.arg.to_string(),
            readback(s, &format!("{}?", it.cmd.replace("{c}", &c.to_string()))),
        );
    }
    Value::Object(m)
}

/// `bench_scope_acquire`: acquisition mode (sampling, peak detect, average with a count, or high resolution), memory depth, and a read of the
/// sample rate and depth. With no argument it only reads.
pub fn acquire(s: &mut Link, args: &Value) -> Result<Value, String> {
    // [@ANCHOR: bench_mcp_scope_acquire]
    let mut cmds: Vec<(String, String)> = Vec::new();
    if let Some(m) = args.get("mode").and_then(Value::as_str) {
        let c = match m {
            "sampling" => "ACQW SAMPLING".to_string(),
            "peak_detect" => "ACQW PEAK_DETECT".to_string(),
            "high_resolution" => "ACQW ERES".to_string(),
            "average" => {
                let n = args
                    .get("average_count")
                    .and_then(Value::as_u64)
                    .unwrap_or(16);
                if !(4..=1024).contains(&n) || !n.is_power_of_two() {
                    return Err("average_count must be a power of two from 4 to 1024".into());
                }
                cmds.push((format!("AVGA {n}"), "AVGA?".into()));
                "ACQW AVERAGE".to_string()
            }
            _ => {
                return Err("mode must be sampling, peak_detect, average or high_resolution".into())
            }
        };
        cmds.push((c, "ACQW?".into()));
    }
    if let Some(d) = args.get("memory_depth").and_then(Value::as_str) {
        if !["7K", "70K", "700K", "7M", "14K", "140K", "1.4M", "14M"].contains(&d) {
            return Err("memory_depth must be one of 7K, 70K, 700K, 7M (or 14K, 140K, 1.4M, 14M on the two-channel-off sheet)".into());
        }
        cmds.push((format!("MSIZ {d}"), "MSIZ?".into()));
    }
    let mut r = apply(s, cmds)?;
    r["sample_rate_sa_s"] = json!(s.number("SARA?")?);
    r["memory_depth"] = readback(s, "MSIZ?");
    r["mode"] = readback(s, "ACQW?");
    Ok(r)
}

/// `bench_scope_channel`: bandwidth limit and inversion of a channel.
pub fn channel(s: &mut Link, args: &Value) -> Result<Value, String> {
    // [@ANCHOR: bench_mcp_scope_channel]
    let c = ch(args)?;
    let mut cmds = Vec::new();
    for (key, cmd, q) in [
        ("bandwidth_limit", "BWL", "BWL?"),
        ("invert", "INVS", "INVS?"),
    ] {
        match args.get(key) {
            None | Some(Value::Null) => {}
            Some(Value::Bool(b)) => {
                let on = if *b { "ON" } else { "OFF" };
                if key == "bandwidth_limit" {
                    cmds.push((format!("BWL C{c},{on}"), q.to_string()));
                } else {
                    cmds.push((format!("C{c}:{cmd} {on}"), format!("C{c}:{q}")));
                }
            }
            Some(_) => return Err(format!("{key} must be true or false")),
        }
    }
    let mut r = apply(s, cmds)?;
    r["bandwidth_limit"] = readback(s, "BWL?");
    r["invert"] = readback(s, &format!("C{c}:INVS?"));
    Ok(r)
}

/// What `bench_scope_display` sets besides the screen saver, grid and intensity.
const DISPLAY_ITEMS: [Item; 4] = [
    Item {
        arg: "xy",
        cmd: "XYDS",
        ty: Ty::Flag,
    },
    Item {
        arg: "persistence",
        cmd: "PESU",
        ty: Ty::Words(&["OFF", "INFINITE", "1", "5", "10", "30"]),
    },
    Item {
        arg: "dot_join",
        cmd: "DTJN",
        ty: Ty::Flag,
    },
    Item {
        arg: "menu",
        cmd: "MENU",
        ty: Ty::Flag,
    },
];

/// `bench_scope_display`: screen saver on or off, grid, trace and grid intensity, XY mode, persistence (seconds, or `INFINITE`), dots instead of vectors and the menu.
pub fn display(s: &mut Link, args: &Value) -> Result<Value, String> {
    // [@ANCHOR: bench_mcp_scope_display]
    let mut cmds = Vec::new();
    match args.get("screen_saver") {
        None | Some(Value::Null) => {}
        Some(Value::String(v))
            if matches!(
                v.as_str(),
                "OFF" | "1MIN" | "5MIN" | "10MIN" | "15MIN" | "30MIN" | "60MIN"
            ) =>
        {
            cmds.push((format!("SCSV {v}"), "SCSV?".into()));
        }
        Some(_) => {
            return Err("screen_saver must be OFF, 1MIN, 5MIN, 10MIN, 15MIN, 30MIN or 60MIN".into())
        }
    }
    if let Some(g) = args.get("grid").and_then(Value::as_str) {
        if !matches!(g, "FULL" | "HALF" | "OFF") {
            return Err("grid must be FULL, HALF or OFF".into());
        }
        cmds.push((format!("GRDS {g}"), "GRDS?".into()));
    }
    let level = |k: &str| -> Result<Option<u64>, String> {
        match args.get(k) {
            None | Some(Value::Null) => Ok(None),
            Some(v) => v
                .as_u64()
                .filter(|n| *n <= 100)
                .map(Some)
                .ok_or(format!("{k} must be a whole number from 0 to 100")),
        }
    };
    let (trace, grid) = (level("trace_intensity")?, level("grid_intensity")?);
    if trace.is_some() || grid.is_some() {
        if let (Some(t), Some(g)) = (trace, grid) {
            cmds.push((format!("INTS GRID,{g},TRACE,{t}"), "INTS?".into()));
        } else {
            return Err("trace_intensity and grid_intensity go together".into());
        }
    }
    cmds.extend(item_cmds(args, &DISPLAY_ITEMS, 1)?);
    let mut r = apply(s, cmds)?;
    r["more"] = read_items(s, &DISPLAY_ITEMS, 1);
    r["screen_saver"] = readback(s, "SCSV?");
    r["grid"] = readback(s, "GRDS?");
    r["intensity"] = readback(s, "INTS?");
    Ok(r)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Write};
    use std::sync::{Arc, Mutex};

    /// A scope on a local socket serving two 8-bit channels (5 mV per division) of a 1 kHz sine and a constant, at 1 MSa/s.
    fn scope(mode: &'static str) -> (u16, Arc<Mutex<Vec<String>>>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let log = seen.clone();
        std::thread::spawn(move || {
            let (sock, _) = listener.accept().unwrap();
            let mut out = sock.try_clone().unwrap();
            let mut trig = mode.to_string();
            for line in BufReader::new(sock).lines() {
                let Ok(l) = line else { break };
                log.lock().unwrap().push(l.clone());
                let r: Vec<u8> = match l.as_str() {
                    "TRIG_MODE?" => format!("TRIG_MODE {trig}\n").into_bytes(),
                    "SARA?" => b"SARA 1.00E+06Sa/s\n".to_vec(),
                    "C1:VDIV?" | "C2:VDIV?" => b"C1:VDIV 5.00E-03V\n".to_vec(),
                    "C1:OFST?" | "C2:OFST?" => b"C1:OFST 0.00E+00V\n".to_vec(),
                    "C1:WF? DAT2" | "C2:WF? DAT2" => {
                        let two = l.starts_with("C2");
                        let data: Vec<u8> = (0..1000)
                            .map(|i| {
                                let v = if two {
                                    50.0
                                } else {
                                    100.0 * (2.0 * std::f64::consts::PI * i as f64 / 1000.0).sin()
                                };
                                (v.round() as i8) as u8
                            })
                            .collect();
                        let mut r = b"C1:WF DAT2,#9000001000".to_vec();
                        r.extend(data);
                        r.extend(b"\n\n");
                        r
                    }
                    "ACQW?" => b"ACQW AVERAGE\n".to_vec(),
                    "AVGA?" => b"AVGA 16\n".to_vec(),
                    "MSIZ?" => b"MSIZ 7M\n".to_vec(),
                    "BWL?" => b"BWL C1,ON,C2,OFF\n".to_vec(),
                    "C1:INVS?" => b"C1:INVS OFF\n".to_vec(),
                    "SCSV?" => b"SCSV OFF\n".to_vec(),
                    "GRDS?" => b"GRDS FULL\n".to_vec(),
                    "INTS?" => b"INTS GRID,50,TRACE,50\n".to_vec(),
                    c => {
                        if let Some(m) = c.strip_prefix("TRIG_MODE ") {
                            trig = m.to_string();
                        }
                        continue;
                    }
                };
                out.write_all(&r).unwrap();
            }
        });
        (port, seen)
    }

    // Tests [@ANCHOR: bench_mcp_scope_math]
    // Tests [@ANCHOR: bench_mcp_scope_combine]
    // Tests [@ANCHOR: bench_mcp_scope_read_waveform]
    #[test]
    fn math_reads_both_channels_from_a_frozen_frame_and_puts_the_scope_back() {
        let (port, seen) = scope("AUTO");
        let mut l = Link::tcp("127.0.0.1", port).unwrap();
        let dir = crate::testdir::scratch("scope_math");
        let r = math(&mut l, &json!({"op": "add"}), &dir).unwrap();
        // channel 2 is a constant 50 counts = 10 mV; the sine peaks at 100 counts = 20 mV
        assert!((r["max"].as_f64().unwrap() - 0.030).abs() < 1e-4, "{r}");
        assert!((r["min"].as_f64().unwrap() + 0.010).abs() < 1e-4, "{r}");
        assert_eq!(r["frame_frozen_for_the_reading"], true);
        assert_eq!(r["samples"], 1000);
        // The restore has no reply; a query after it makes sure the scope has logged it.
        l.query("TRIG_MODE?").unwrap();
        let cmds = seen.lock().unwrap().clone();
        let stop = cmds.iter().position(|c| c == "TRIG_MODE STOP").unwrap();
        let back = cmds.iter().position(|c| c == "TRIG_MODE AUTO").unwrap();
        let wf = cmds.iter().position(|c| c == "C2:WF? DAT2").unwrap();
        assert!(stop < wf && wf < back, "{cmds:?}");
        let d = math(&mut l, &json!({"op": "div"}), &dir).unwrap();
        assert_eq!(d["undefined_points"], 0);
        assert!(math(&mut l, &json!({"op": "pow"}), &dir).is_err());
        assert_eq!(
            combine("div", &[1.0, 2.0], &[0.0, 4.0]),
            (vec![0.0, 0.5], 1)
        );
        assert_eq!(combine("sub", &[3.0], &[1.0, 9.0]).0, vec![2.0]);
        assert_eq!(combine("mul", &[3.0], &[2.0]).0, vec![6.0]);
        std::fs::remove_dir_all(&dir).ok();
    }

    // Tests [@ANCHOR: bench_mcp_scope_more_summary]
    #[test]
    fn summary_gives_min_max_mean_and_rms() {
        let (mn, mx, mean, rms) = summary(&[-1.0, 1.0, -1.0, 1.0]);
        assert_eq!((mn, mx, mean), (-1.0, 1.0, 0.0));
        assert!((rms - 1.0).abs() < 1e-12);
        assert_eq!(summary(&[3.0]).2, 3.0);
    }

    // Tests [@ANCHOR: bench_mcp_scope_more_apply]
    // Tests [@ANCHOR: bench_mcp_scope_more_readback]
    #[test]
    fn apply_sends_each_command_and_reads_its_setting_back() {
        let (port, seen) = scope("AUTO");
        let mut l = Link::tcp("127.0.0.1", port).unwrap();
        let r = apply(
            &mut l,
            vec![("TRIG_MODE SINGLE".into(), "TRIG_MODE?".into())],
        )
        .unwrap();
        assert_eq!(r["sent"][0], "TRIG_MODE SINGLE");
        assert_eq!(r["readback"]["TRIG_MODE?"], "TRIG_MODE SINGLE");
        assert_eq!(
            seen.lock().unwrap().as_slice(),
            ["TRIG_MODE SINGLE", "TRIG_MODE?"]
        );
    }

    // Tests [@ANCHOR: bench_mcp_scope_cursors]
    // Tests [@ANCHOR: bench_mcp_scope_value_at]
    #[test]
    fn cursors_interpolate_and_two_of_them_give_the_differences() {
        let (port, _) = scope("AUTO");
        let mut l = Link::tcp("127.0.0.1", port).unwrap();
        let r = cursors(&mut l, &json!({"channel": 1, "times_s": [250e-6, 750e-6]})).unwrap();
        let rows = r["cursors"].as_array().unwrap();
        assert!(
            (rows[0]["volts"].as_f64().unwrap() - 0.020).abs() < 2e-4,
            "{r}"
        );
        assert!(
            (rows[1]["volts"].as_f64().unwrap() + 0.020).abs() < 2e-4,
            "{r}"
        );
        assert!((r["delta_time_s"].as_f64().unwrap() - 500e-6).abs() < 1e-12);
        assert!((r["frequency_hz"].as_f64().unwrap() - 2000.0).abs() < 1e-6);
        let out = cursors(&mut l, &json!({"channel": 1, "times_s": [5.0]})).unwrap();
        assert!(out["cursors"][0]["volts"].is_null(), "outside the record");
        assert!(cursors(&mut l, &json!({"channel": 1, "times_s": []})).is_err());
        assert!(cursors(&mut l, &json!({"channel": 1})).is_err());
        assert_eq!(value_at(&[0.0, 2.0], 1.0, 0.5), Some(1.0));
        let (port, _) = scope("STOP");
        let mut l = Link::tcp("127.0.0.1", port).unwrap();
        assert!(cursors(&mut l, &json!({"channel": 1, "times_s": [0.0]}))
            .unwrap_err()
            .contains("stopped"));
    }

    // Tests [@ANCHOR: bench_mcp_scope_acquire]
    // Tests [@ANCHOR: bench_mcp_scope_channel]
    // Tests [@ANCHOR: bench_mcp_scope_display]
    #[test]
    fn settings_send_the_documented_commands_read_them_back_and_refuse_bad_values_unsent() {
        let (port, seen) = scope("AUTO");
        let mut l = Link::tcp("127.0.0.1", port).unwrap();
        let r = acquire(
            &mut l,
            &json!({"mode": "average", "average_count": 16, "memory_depth": "7M"}),
        )
        .unwrap();
        assert_eq!(r["sent"], json!(["AVGA 16", "ACQW AVERAGE", "MSIZ 7M"]));
        assert_eq!(r["readback"]["MSIZ?"], "MSIZ 7M");
        assert_eq!(r["sample_rate_sa_s"], 1.0e6);
        let r = channel(
            &mut l,
            &json!({"channel": 1, "bandwidth_limit": true, "invert": false}),
        )
        .unwrap();
        assert_eq!(r["sent"], json!(["BWL C1,ON", "C1:INVS OFF"]));
        let r = display(&mut l, &json!({"screen_saver": "OFF", "grid": "HALF", "trace_intensity": 60, "grid_intensity": 30})).unwrap();
        assert_eq!(
            r["sent"],
            json!(["SCSV OFF", "GRDS HALF", "INTS GRID,30,TRACE,60"])
        );
        let before = seen.lock().unwrap().len();
        for e in [
            acquire(&mut l, &json!({"mode": "average", "average_count": 5})),
            acquire(&mut l, &json!({"mode": "weird"})),
            acquire(&mut l, &json!({"memory_depth": "9G"})),
            channel(&mut l, &json!({"channel": 1, "invert": "yes"})),
            channel(&mut l, &json!({"bandwidth_limit": true})),
            display(&mut l, &json!({"grid": "SOME"})),
            display(&mut l, &json!({"trace_intensity": 50})),
            display(&mut l, &json!({"screen_saver": "2MIN"})),
        ] {
            assert!(e.is_err());
        }
        assert_eq!(
            seen.lock().unwrap().len(),
            before,
            "nothing was sent for a bad value"
        );
    }

    // Tests [@ANCHOR: bench_mcp_scope_item_cmds]
    // Tests [@ANCHOR: bench_mcp_scope_read_items]
    // Tests [@ANCHOR: bench_mcp_scope_display]
    // Tests [@ANCHOR: bench_mcp_scope_acquire]
    // Tests [@ANCHOR: bench_mcp_scope_channel]
    #[test]
    fn the_display_acquire_and_channel_tools_work_against_the_fake_sds1000x_e() {
        let fake = crate::fake_scope::FakeScope::start();
        let mut l = Link::tcp("127.0.0.1", fake.port).unwrap();
        let r = display(&mut l, &json!({"xy": true, "persistence": 5, "dot_join": true, "menu": false, "grid": "HALF", "trace_intensity": 60, "grid_intensity": 30})).unwrap();
        assert_eq!(
            r["more"],
            json!({"xy": "XYDS ON", "persistence": "PESU 5", "dot_join": "DTJN ON", "menu": "MENU OFF"})
        );
        assert_eq!(r["grid"], "GRDS HALF");
        assert_eq!(r["intensity"], "INTS TRACE,60,GRID,30");
        assert_eq!(r["screen_saver"], "SCSV 30MIN");
        let r = display(
            &mut l,
            &json!({"persistence": "infinite", "screen_saver": "OFF"}),
        )
        .unwrap();
        assert_eq!(r["more"]["persistence"], "PESU INFINITE");
        assert_eq!(r["screen_saver"], "SCSV OFF");
        for bad in [
            json!({"xy": "on"}),
            json!({"persistence": 7}),
            json!({"menu": 1}),
        ] {
            assert!(display(&mut l, &bad).is_err(), "{bad}");
        }
        let sent = fake.got("XYDS").len();
        assert!(display(&mut l, &json!({"xy": true, "persistence": "forever"})).is_err());
        assert_eq!(
            fake.got("XYDS").len(),
            sent,
            "nothing is sent when one value is bad"
        );
        let r = acquire(
            &mut l,
            &json!({"mode": "average", "average_count": 64, "memory_depth": "70K"}),
        )
        .unwrap();
        assert_eq!(
            (
                r["mode"].as_str().unwrap(),
                r["memory_depth"].as_str().unwrap()
            ),
            ("ACQW AVERAGE", "MSIZ 70K")
        );
        assert_eq!(r["readback"]["AVGA?"], "AVGA 64");
        assert_eq!(r["sample_rate_sa_s"], 5.0e6);
        let r = channel(
            &mut l,
            &json!({"channel": 2, "bandwidth_limit": true, "invert": true}),
        )
        .unwrap();
        assert_eq!(r["bandwidth_limit"], "BWL C1,OFF,C2,ON");
        assert_eq!(r["invert"], "C2:INVS ON");
    }

    // Tests [@ANCHOR: bench_mcp_scope_math]
    // Tests [@ANCHOR: bench_mcp_scope_cursors]
    // Tests [@ANCHOR: bench_mcp_scope_combine]
    // Tests [@ANCHOR: bench_mcp_scope_value_at]
    #[test]
    fn host_math_and_cursors_read_the_fake_scopes_two_channels() {
        let fake = crate::fake_scope::FakeScope::start();
        let mut l = Link::tcp("127.0.0.1", fake.port).unwrap();
        let out = crate::testdir::scratch("scope_more_fake");
        let r = math(&mut l, &json!({"op": "add"}), &out).unwrap();
        // Two sines a quarter period apart add to one of 2 V * sqrt(2) peak.
        let max = r["max"].as_f64().unwrap();
        assert!((max - 2.0 * 2f64.sqrt()).abs() < 0.05, "{r}");
        l.query("*OPC?").unwrap(); // the scope has read every earlier command once it answers
        assert_eq!(
            fake.got("TRIG_MODE"),
            ["TRIG_MODE?", "TRIG_MODE STOP", "TRIG_MODE AUTO"],
            "asked, stopped for the reading, put back"
        );
        let r = cursors(&mut l, &json!({"channel": 1, "times_s": [0.0, 50e-6]})).unwrap();
        assert!(
            (r["cursors"][1]["volts"].as_f64().unwrap() - 2.0).abs() < 0.02,
            "{r}"
        );
    }
}
