// SPDX-License-Identifier: AGPL-3.0-or-later
//! Siglent SDS1000X-E functions written from the programming guide (EN02E) and tested against the fake scope in `fake_scope.rs`: history,
//! reference waveforms, setups, pass/fail, custom measurements, the counter, the scope's own math, FFT and cursors, and serial trigger and
//! decode. The real unit's answers to these have not been compared with the guide (the lab list is in the to-do `bench-mcp-scope-remaining-functions`),
//! so every tool reads back what the scope holds after a change.

use crate::link::Link;
use crate::scope::{ch, read_waveform, stamp};
use crate::scope_more::{apply, item_cmds, read_items, readback, Item, Ty};
use serde_json::{json, Value};
use std::path::Path;

/// What `bench_scope_history` sets: history mode, the frame list and the current frame.
const HISTORY_ITEMS: [Item; 3] = [
    Item {
        arg: "history",
        cmd: "HSMD",
        ty: Ty::Flag,
    },
    Item {
        arg: "list",
        cmd: "HSLST",
        ty: Ty::Flag,
    },
    Item {
        arg: "frame",
        cmd: "FRAM",
        ty: Ty::Int(0, 100_000),
    },
];

/// `bench_scope_history`: turns history mode and its list on or off, selects a frame, and reads frames. With `frames` (up to 20 frame numbers) and a
/// `channel` each frame is selected, its time stamp read, and its waveform summarised (and saved as little-endian f32 volts when `save` is true).
pub fn history(s: &mut Link, args: &Value, out: &Path) -> Result<Value, String> {
    // [@ANCHOR: bench_mcp_scope_history]
    let cmds = item_cmds(args, &HISTORY_ITEMS, 1)?;
    let frames: Vec<u64> = match args.get("frames") {
        None | Some(Value::Null) => vec![],
        Some(v) => v
            .as_array()
            .filter(|a| a.len() <= 20)
            .and_then(|a| a.iter().map(Value::as_u64).collect::<Option<Vec<_>>>())
            .filter(|f| f.iter().all(|n| *n <= 100_000))
            .ok_or("frames must be up to 20 frame numbers from 0 to 100000")?,
    };
    let c = if frames.is_empty() { 1 } else { ch(args)? };
    let mut r = apply(s, cmds)?;
    let mut rows = Vec::new();
    for n in &frames {
        s.send(&format!("FRAM {n}"))?;
        std::thread::sleep(std::time::Duration::from_millis(100));
        let time = readback(s, "FTIM?");
        let (v, sara) = read_waveform(s, c)?;
        let (mn, mx) = v
            .iter()
            .fold((f32::MAX, f32::MIN), |(a, b), &x| (a.min(x), b.max(x)));
        let mut row = json!({"frame": n, "time": time, "samples": v.len(), "sample_interval_s": 1.0 / sara, "min_v": mn, "max_v": mx, "pkpk_v": mx - mn});
        if args.get("save").and_then(Value::as_bool) == Some(true) {
            let path = out.join(format!("history_ch{c}_frame{n}_{}.f32", stamp()));
            let bytes: Vec<u8> = v.iter().flat_map(|x| x.to_le_bytes()).collect();
            std::fs::write(&path, bytes).map_err(|e| format!("write {}: {e}", path.display()))?;
            row["file"] = json!(path);
        }
        rows.push(row);
    }
    r["settings"] = read_items(s, &HISTORY_ITEMS, 1);
    r["frames"] = json!(rows);
    Ok(r)
}

/// What `bench_scope_reference` sets: which reference (location, source) and, once one is stored, how it is shown.
const REFERENCE_ITEMS: [Item; 5] = [
    Item {
        arg: "location",
        cmd: "REFLA",
        ty: Ty::Words(&["REFA", "REFB", "REFC", "REFD"]),
    },
    Item {
        arg: "source",
        cmd: "REFSR",
        ty: Ty::Words(&["C1", "C2", "MATH"]),
    },
    Item {
        arg: "display",
        cmd: "REFDS",
        ty: Ty::Flag,
    },
    Item {
        arg: "position_v",
        cmd: "REFPO",
        ty: Ty::Num(-100.0, 100.0, "V"),
    },
    Item {
        arg: "scale_v",
        cmd: "REFSC",
        ty: Ty::Num(500e-6, 10.0, "V"),
    },
];

/// `bench_scope_reference`: selects a reference waveform (REFA to REFD) and its source, stores the source's screen waveform in it (`save`), shows
/// it, sets its vertical position and scale, or closes the reference function (`close`). The guide has no command that reads a reference back or
/// compares it, so there is none here: the stored trace is on the scope's screen (`bench_screenshot` shows it).
pub fn reference(s: &mut Link, args: &Value) -> Result<Value, String> {
    // [@ANCHOR: bench_mcp_scope_reference]
    let flag = |k: &str| match args.get(k) {
        None | Some(Value::Null) => Ok(false),
        Some(Value::Bool(b)) => Ok(*b),
        Some(_) => Err(format!("{k} must be true or false")),
    };
    let (save, close) = (flag("save")?, flag("close")?);
    // The location and the source come first, the store after them, and the display settings after that (the scope takes those only once a trace is stored).
    let mut cmds = item_cmds(args, &REFERENCE_ITEMS[..2], 1)?;
    if save {
        cmds.push(("REFSA".into(), "REFDS?".into()));
    }
    cmds.extend(item_cmds(args, &REFERENCE_ITEMS[2..], 1)?);
    if close {
        cmds.push(("REFCL".into(), "REFDS?".into()));
    }
    let mut r = apply(s, cmds)?;
    r["settings"] = read_items(s, &REFERENCE_ITEMS, 1);
    Ok(r)
}

/// `bench_scope_setup`: saves the scope's complete setup in one of its 20 internal slots (`save_slot`, `*SAV`) or recalls one (`recall_slot`, `*RCL`; 0 is
/// the default setup), exports it to a file in the output directory (`export`, the `PNSU?` block) or restores it from a file there (`import`, a file name
/// only, as `export` wrote it). The scope's USB file functions are not used.
pub fn setup(s: &mut Link, args: &Value, out: &Path) -> Result<Value, String> {
    // [@ANCHOR: bench_mcp_scope_setup]
    let slot = |k: &str, lo: u64| match args.get(k) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v
            .as_u64()
            .filter(|n| (lo..=20).contains(n))
            .map(Some)
            .ok_or(format!("{k} must be a whole number from {lo} to 20")),
    };
    let (save, recall) = (slot("save_slot", 1)?, slot("recall_slot", 0)?);
    let export = args.get("export").and_then(Value::as_bool) == Some(true);
    let import = match args.get("import") {
        None | Some(Value::Null) => None,
        Some(Value::String(f)) if !f.is_empty() && !f.contains(['/', '\\']) && !f.starts_with('.') => Some(out.join(f)),
        Some(_) => return Err("import must be the name of a file in the output directory, as export wrote it (no path)".into()),
    };
    let bytes = match &import {
        Some(p) => Some(std::fs::read(p).map_err(|e| format!("read {}: {e}", p.display()))?),
        None => None,
    };
    let mut r = json!({});
    if let Some(n) = save {
        s.send(&format!("*SAV {n}"))?;
        r["saved_slot"] = json!(n);
    }
    if let Some(n) = recall {
        s.send(&format!("*RCL {n}"))?;
        r["recalled_slot"] = json!(n);
    }
    if export {
        s.set_timeout_secs(30);
        let data = s.block("PNSU?");
        s.set_timeout_secs(10);
        let data = data?;
        let path = out.join(format!("setup_{}.set", stamp()));
        std::fs::write(&path, &data).map_err(|e| format!("write {}: {e}", path.display()))?;
        r["exported"] = json!({"file": path, "bytes": data.len()});
    }
    if let Some(b) = bytes {
        s.send_block("PNSU ", &b)?;
        r["imported_bytes"] = json!(b.len());
    }
    std::thread::sleep(std::time::Duration::from_millis(300));
    r["after"] =
        json!({"timebase": readback(s, "TDIV?"), "trigger_mode": readback(s, "TRIG_MODE?")});
    r["note"] = json!("a setup slot or file the scope refused shows in `after` as unchanged settings; the commands are from the SDS1000X-E programming guide and not yet checked on firmware 1.3.27");
    Ok(r)
}

/// What `bench_scope_passfail` sets, in the order the guide needs: the test on and its source, then how failures are shown and reported, then the run state.
const PASSFAIL_ITEMS: [Item; 6] = [
    Item {
        arg: "enable",
        cmd: "PFEN",
        ty: Ty::Flag,
    },
    Item {
        arg: "source",
        cmd: "PFSC",
        ty: Ty::Words(&["C1", "C2"]),
    },
    Item {
        arg: "display",
        cmd: "PFDS",
        ty: Ty::Flag,
    },
    Item {
        arg: "stop_on_fail",
        cmd: "PFFS",
        ty: Ty::Flag,
    },
    Item {
        arg: "output",
        cmd: "PFBF",
        ty: Ty::Flag,
    },
    Item {
        arg: "run",
        cmd: "PFOP",
        ty: Ty::Flag,
    },
];

/// `bench_scope_passfail`: the mask test. It switches the test on and picks its source, sets the mask tolerance (`x_mask_div`, `y_mask_div`, 0.04 to 4 in
/// steps of 0.04, given together), builds the mask round the source waveform (`create_mask`, only while the test is on and stopped), clears the counts
/// (`clear`), runs or stops the test, and always returns the counts (`PFDD?`: failed, passed, total). Saving and loading mask files is not offered: the
/// guide says a two-channel scope does not support it.
pub fn passfail(s: &mut Link, args: &Value) -> Result<Value, String> {
    // [@ANCHOR: bench_mcp_scope_passfail]
    let flag = |k: &str| match args.get(k) {
        None | Some(Value::Null) => Ok(false),
        Some(Value::Bool(b)) => Ok(*b),
        Some(_) => Err(format!("{k} must be true or false")),
    };
    let (create, clear) = (flag("create_mask")?, flag("clear")?);
    let tol = |k: &str| match args.get(k) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v
            .as_f64()
            .filter(|x| (0.04..=4.0).contains(x) && ((x / 0.04).round() - x / 0.04).abs() < 1e-6)
            .map(Some)
            .ok_or(format!("{k} must be from 0.04 to 4.0 in steps of 0.04")),
    };
    let mask = match (tol("x_mask_div")?, tol("y_mask_div")?) {
        (None, None) => None,
        (Some(x), Some(y)) => Some(format!("PFST XMASK,{x},YMASK,{y}")),
        _ => return Err("x_mask_div and y_mask_div go together".into()),
    };
    let mut cmds = item_cmds(args, &PASSFAIL_ITEMS[..2], 1)?;
    cmds.extend(mask.map(|m| (m, "PFST?".to_string())));
    if create {
        cmds.push(("PFCM".into(), "PFEN?".into()));
    }
    cmds.extend(item_cmds(args, &PASSFAIL_ITEMS[2..5], 1)?);
    if clear {
        cmds.push(("PACL".into(), "PFDD?".into()));
    }
    cmds.extend(item_cmds(args, &PASSFAIL_ITEMS[5..], 1)?);
    let mut r = apply(s, cmds)?;
    r["settings"] = read_items(s, &PASSFAIL_ITEMS, 1);
    r["mask_tolerance"] = readback(s, "PFST?");
    let counts = readback(s, "PFDD?");
    let nums: Vec<u64> = counts
        .as_str()
        .unwrap_or("")
        .split(',')
        .filter_map(|t| t.trim().parse().ok())
        .collect();
    r["result"] = match nums.as_slice() {
        [fail, pass, total] => json!({"failed": fail, "passed": pass, "total": total}),
        _ => json!({"unparsed": counts}),
    };
    Ok(r)
}

/// The measurement items `PACU` installs (the guide's list; `ALL` is a snapshot, read with `snapshot`).
const MEASURE_ITEMS: [&str; 28] = [
    "PKPK", "MAX", "MIN", "AMPL", "TOP", "BASE", "CMEAN", "MEAN", "STDEV", "VSTD", "RMS", "CRMS",
    "OVSN", "FPRE", "OVSP", "RPRE", "LEVELX", "DELAY", "TIMEL", "PER", "FREQ", "PWID", "NWID",
    "RISE", "FALL", "WID", "DUTY", "NDUTY",
];

/// The delay measurements `MEAD` starts.
const DELAY_TYPES: [&str; 10] = [
    "PHA", "FRR", "FRF", "FFR", "FFF", "LRR", "LRF", "LFR", "LFF", "SKEW",
];

/// What `bench_scope_measure_custom` sets for the measurement gate.
const GATE_ITEMS: [Item; 3] = [
    Item {
        arg: "gate",
        cmd: "MEGS",
        ty: Ty::Flag,
    },
    Item {
        arg: "gate_a_s",
        cmd: "MEGA",
        ty: Ty::Num(-1000.0, 1000.0, "S"),
    },
    Item {
        arg: "gate_b_s",
        cmd: "MEGB",
        ty: Ty::Num(-1000.0, 1000.0, "S"),
    },
];

/// `bench_scope_measure_custom`: the scope's own measurement list. `clear` removes the installed measurements (`MEACL`); `install` (up to 5 of
/// `{item, channel}`) installs them (`PACU`); `statistics` is `on`, `off` or `reset` (`PASTAT`); `delay` (`{type, sources}`, for example PHA and C1-C2) starts a
/// delay measurement (`MEAD`) and reads it; the gate (`gate`, `gate_a_s`, `gate_b_s`) limits what is measured. It always reads the installed
/// measurements (`PAVA? CUSTALL`) and, with statistics on, each one's statistics (`PAVA? STATn`: current, mean, min, max, standard deviation, count);
/// `snapshot` (a channel) reads every item at once (`PAVA? ALL`).
pub fn measure_custom(s: &mut Link, args: &Value) -> Result<Value, String> {
    // [@ANCHOR: bench_mcp_scope_measure_custom]
    let installs: Vec<(String, u64)> = match args.get("install") {
        None | Some(Value::Null) => vec![],
        Some(v) => v
            .as_array()
            .filter(|a| a.len() <= 5)
            .and_then(|a| {
                a.iter()
                    .map(|e| {
                        let item = e.get("item")?.as_str()?.to_uppercase();
                        let c = e.get("channel")?.as_u64().filter(|c| (1..=2).contains(c))?;
                        MEASURE_ITEMS.contains(&item.as_str()).then_some((item, c))
                    })
                    .collect()
            })
            .ok_or_else(|| {
                format!(
                    "install must be up to 5 of {{item, channel 1 or 2}}, item one of {}",
                    MEASURE_ITEMS.join(", ")
                )
            })?,
    };
    let stats = match args
        .get("statistics")
        .map(|v| v.as_str().map(str::to_uppercase))
    {
        None => None,
        Some(Some(w)) if ["ON", "OFF", "RESET"].contains(&w.as_str()) => Some(w),
        _ => return Err("statistics must be on, off or reset".into()),
    };
    let delay = match args.get("delay") {
        None | Some(Value::Null) => None,
        Some(d) => {
            let t = d
                .get("type")
                .and_then(Value::as_str)
                .map(str::to_uppercase)
                .filter(|t| DELAY_TYPES.contains(&t.as_str()));
            let src = d
                .get("sources")
                .and_then(Value::as_str)
                .map(str::to_uppercase)
                .filter(|x| x == "C1-C2" || x == "C2-C1");
            Some(t.zip(src).ok_or_else(|| {
                format!(
                    "delay must be {{type one of {}, sources C1-C2 or C2-C1}}",
                    DELAY_TYPES.join(", ")
                )
            })?)
        }
    };
    let snapshot = match args.get("snapshot") {
        None | Some(Value::Null) => None,
        Some(v) => Some(
            v.as_u64()
                .filter(|c| (1..=2).contains(c))
                .ok_or("snapshot must be a channel, 1 or 2")?,
        ),
    };
    let clear = match args.get("clear") {
        None | Some(Value::Null) => false,
        Some(v) => v.as_bool().ok_or("clear must be true or false")?,
    };
    let gate = item_cmds(args, &GATE_ITEMS, 1)?;
    let mut cmds: Vec<(String, String)> = Vec::new();
    if clear {
        cmds.push(("MEACL".into(), "PAVA? CUSTALL".into()));
    }
    cmds.extend(
        installs
            .iter()
            .map(|(i, c)| (format!("PACU {i},C{c}"), "PAVA? CUSTALL".to_string())),
    );
    cmds.extend(
        stats
            .iter()
            .map(|w| (format!("PASTAT {w}"), "PASTAT?".to_string())),
    );
    cmds.extend(gate);
    if let Some((t, src)) = &delay {
        cmds.push((format!("MEAD {t},{src}"), format!("{src}:MEAD? {t}")));
    }
    let mut r = apply(s, cmds)?;
    r["gate"] = read_items(s, &GATE_ITEMS, 1);
    // The installed measurements: `PAVA CUST1:C1,PKPK,4.08E+00V;CUST2:OFF;...`.
    let all = readback(s, "PAVA? CUSTALL");
    let mut custom = Vec::new();
    for part in all
        .as_str()
        .unwrap_or("")
        .trim_start_matches("PAVA")
        .split(';')
    {
        let Some((slot, rest)) = part.trim().split_once(':') else {
            continue;
        };
        let f: Vec<&str> = rest.split(',').collect();
        custom.push(match f.as_slice() {
            [c, item, value] => json!({"slot": slot, "channel": c, "item": item, "value": value}),
            _ => json!({"slot": slot, "off": true}),
        });
    }
    r["custom"] = if custom.is_empty() {
        json!({"unparsed": all})
    } else {
        json!(custom)
    };
    let installed = custom.iter().filter(|c| c.get("off").is_none()).count();
    if readback(s, "PASTAT?")
        .as_str()
        .is_some_and(|t| t.ends_with("ON"))
    {
        let mut rows = Vec::new();
        for n in 1..=installed {
            s.set_timeout_secs(3);
            // The guide shows the answer on two lines (`PAVA STAT1 C3` then `FREQ:cur,...`); join lines until the count arrives.
            let mut text = s.query(&format!("PAVA? STAT{n}")).unwrap_or_default();
            while !text.contains("count,") && !text.is_empty() {
                match s.line() {
                    Ok(l) => text = format!("{text} {l}"),
                    Err(_) => break,
                }
            }
            s.set_timeout_secs(10);
            let f: Vec<&str> = text.split([',', ' ']).collect();
            let mut row = json!({"slot": n, "text": text});
            for key in ["cur", "mean", "min", "max", "std-dev", "count"] {
                let at = f.iter().position(|t| t.ends_with(key));
                if let Some(v) = at.and_then(|k| f.get(k + 1)) {
                    row[key] = json!(v);
                }
            }
            rows.push(row);
        }
        r["statistics"] = json!(rows);
    }
    if let Some((t, src)) = &delay {
        r["delay"] = readback(s, &format!("{src}:MEAD? {t}"));
    }
    if let Some(c) = snapshot {
        s.set_timeout_secs(3);
        let text = s.query(&format!("C{c}:PAVA? ALL")).unwrap_or_default();
        s.set_timeout_secs(10);
        let f: Vec<&str> = text
            .trim_start_matches(|ch: char| ch != ' ')
            .trim()
            .split(',')
            .collect();
        let pairs: serde_json::Map<String, Value> = f
            .chunks(2)
            .filter(|p| p.len() == 2)
            .map(|p| (p[0].to_string(), json!(p[1])))
            .collect();
        r["snapshot"] = if pairs.is_empty() {
            json!({"no_answer": text})
        } else {
            Value::Object(pairs)
        };
    }
    Ok(r)
}

/// A frequency in hertz from a scope reply such as `CYMT 94.5177261kHz`, `FFTT 100.00MHz` or `1.00E+03Hz`: the value after the last space or comma, a number
/// followed by `Hz`, `kHz`, `MHz` or `GHz`. `None` for anything else (`<10Hz`, no number, another unit).
pub(crate) fn hertz(text: &str) -> Option<f64> {
    // [@ANCHOR: bench_mcp_scope_hertz]
    let t = Link::value(text);
    let end = t
        .find(|c: char| c.is_ascii_alphabetic() && !matches!(c, 'e' | 'E'))
        .unwrap_or(t.len());
    let v: f64 = t[..end].parse().ok()?;
    match &t[end..] {
        "Hz" => Some(v),
        "kHz" => Some(v * 1e3),
        "MHz" => Some(v * 1e6),
        "GHz" => Some(v * 1e9),
        _ => None,
    }
}

/// `bench_scope_counter`: the scope's hardware frequency counter (`CYMT?`, E-notation, and `CYMT_HW?`, full precision with kHz or MHz), each as the
/// scope's text and as hertz. The counter counts trigger-level crossings of the trigger source. Below 10 Hz the scope answers `10Hz` or `<10Hz`, which
/// has no hertz value here (`null`). The guide lists no histogram or DVM function for this series, so none is offered.
pub fn counter(s: &mut Link) -> Result<Value, String> {
    // [@ANCHOR: bench_mcp_scope_counter]
    let coarse = s.query("CYMT?")?;
    let full = s.query("CYMT_HW?")?;
    Ok(
        json!({"counter": coarse, "counter_hz": hertz(&coarse), "full_precision": full, "full_precision_hz": hertz(&full),
        "note": "counts the trigger source's level crossings; below 10 Hz the scope answers 10Hz or <10Hz; from the programming guide, not yet checked on firmware 1.3.27"}),
    )
}

/// The equations `DEFINE EQN` takes, per the guide: two channels combined, or a transform of one.
fn equations() -> Vec<String> {
    // [@ANCHOR: bench_mcp_scope_equations]
    let mut v = Vec::new();
    for (a, b) in [("C1", "C2"), ("C2", "C1")] {
        v.extend(["+", "-", "*", "/"].iter().map(|op| format!("{a}{op}{b}")));
    }
    for t in ["FFT", "INTG", "DIFF", "SQRT"] {
        v.extend(["C1", "C2"].iter().map(|c| format!("{t}{c}")));
    }
    v
}

/// What `bench_scope_math_native` sets besides the equation: the vertical scale and position of an arithmetic result, and the FFT's window, centre,
/// vertical scale, unit, position and screen mode.
const NATIVE_MATH_ITEMS: [Item; 9] = [
    Item {
        arg: "scale_v",
        cmd: "MTVD",
        ty: Ty::Num(500e-6, 100.0, "V"),
    },
    Item {
        arg: "position",
        cmd: "MTVP",
        ty: Ty::Int(-255, 255),
    },
    Item {
        arg: "invert",
        cmd: "MATH:INVS",
        ty: Ty::Flag,
    },
    Item {
        arg: "fft_window",
        cmd: "FFTW",
        ty: Ty::Words(&["RECT", "BLAC", "HANN", "HAMM", "FLATTOP"]),
    },
    Item {
        arg: "fft_center_hz",
        cmd: "FFTC",
        ty: Ty::Num(0.0, 1e9, "Hz"),
    },
    Item {
        arg: "fft_unit",
        cmd: "FFTU",
        ty: Ty::Words(&["VRMS", "DBM", "DBVRMS"]),
    },
    Item {
        arg: "fft_scale",
        cmd: "FFTS",
        ty: Ty::Num(0.001, 20.0, ""),
    },
    Item {
        arg: "fft_position_v",
        cmd: "FFTP",
        ty: Ty::Num(-1000.0, 1000.0, "V"),
    },
    Item {
        arg: "fft_screen",
        cmd: "FFTF",
        ty: Ty::Words(&["OFF", "ON", "EXCLU"]),
    },
];

/// `bench_scope_math_native`: the scope's own math. `equation` (`C1+C2`, `C1-C2`, `C1*C2`, `C1/C2` or the channels the other way round, or `FFTC1`, `INTGC1`,
/// `DIFFC1`, `SQRTC1`, also for C2) is set with `DEFINE`; the vertical scale and position, invert, and the FFT window, centre, scale, unit, position and screen mode
/// are set and read back. With `read` the math trace is read: an arithmetic or transform trace (not FFT) is decoded with the guide's formula (volts =
/// code * scale / 25 - position / 50 * scale), summarised and saved as little-endian f32; an FFT trace, which the guide says cannot be read, is returned as
/// its raw codes' summary with the peak's position and the frequency that position has IF the trace spans ten divisions of the FFT time-base value
/// round the centre (an assumption to be checked on the unit, so it is labelled).
pub fn math_native(s: &mut Link, args: &Value, out: &Path) -> Result<Value, String> {
    // [@ANCHOR: bench_mcp_scope_math_native]
    let eq = match args.get("equation") {
        None | Some(Value::Null) => None,
        Some(v) => Some(
            v.as_str()
                .map(|e| e.to_uppercase())
                .filter(|e| equations().contains(e))
                .ok_or_else(|| format!("equation must be one of {}", equations().join(", ")))?,
        ),
    };
    let read = match args.get("read") {
        None | Some(Value::Null) => false,
        Some(v) => v.as_bool().ok_or("read must be true or false")?,
    };
    let mut cmds: Vec<(String, String)> = eq
        .iter()
        .map(|e| (format!("DEFINE EQN,'{e}'"), "DEFINE?".to_string()))
        .collect();
    cmds.extend(item_cmds(args, &NATIVE_MATH_ITEMS, 1)?);
    let mut r = apply(s, cmds)?;
    r["settings"] = read_items(s, &NATIVE_MATH_ITEMS, 1);
    r["equation"] = readback(s, "DEFINE?");
    if read {
        let fft = r["equation"]
            .as_str()
            .is_some_and(|e| e.to_uppercase().contains("FFT"));
        s.send("WFSU SP,1,NP,0,FP,0")?;
        s.set_timeout_secs(30);
        let raw = s.block("MATH:WF? DAT2");
        s.set_timeout_secs(10);
        let raw = raw?;
        if fft {
            let (idx, code) = raw
                .iter()
                .enumerate()
                .map(|(i, b)| (i, *b as i8))
                .max_by_key(|(_, c)| *c)
                .ok_or("empty FFT trace")?;
            let span = s
                .query("FFTT?")
                .ok()
                .and_then(|t| crate::scope_guide::hertz(&t));
            let centre = s
                .query("FFTC?")
                .ok()
                .and_then(|t| crate::scope_guide::hertz(&t));
            let f = span
                .zip(centre)
                .filter(|_| raw.len() > 1)
                .map(|(span, c)| c + (idx as f64 / (raw.len() - 1) as f64 - 0.5) * 10.0 * span);
            r["fft_trace"] = json!({"points": raw.len(), "peak_index": idx, "peak_code": code, "peak_frequency_hz_assumed": f,
                "assumption": "the guide says the FFT trace cannot be read; the frequency assumes the points span 10 divisions of FFTT? round FFTC?; the level is not converted"});
        } else {
            let scale = s.number("MTVD?")?;
            let pos = s.number("MTVP?")?;
            let v: Vec<f32> = raw
                .iter()
                .map(|&b| (f64::from(b as i8) * scale / 25.0 - pos / 50.0 * scale) as f32)
                .collect();
            let (mn, mx) = v
                .iter()
                .fold((f32::MAX, f32::MIN), |(a, b), &x| (a.min(x), b.max(x)));
            let path = out.join(format!("math_native_{}.f32", stamp()));
            let bytes: Vec<u8> = v.iter().flat_map(|x| x.to_le_bytes()).collect();
            std::fs::write(&path, bytes).map_err(|e| format!("write {}: {e}", path.display()))?;
            r["trace"] = json!({"points": v.len(), "min": mn, "max": mx, "file": path, "file_format": "little-endian f32, the unit of the math result"});
        }
    }
    Ok(r)
}

/// What `bench_scope_cursor_native` sets: the cursor mode and (manual mode) type.
const CURSOR_ITEMS: [Item; 2] = [
    Item {
        arg: "mode",
        cmd: "CRMS",
        ty: Ty::Words(&["OFF", "MANUAL", "TRACK"]),
    },
    Item {
        arg: "type",
        cmd: "CRTY",
        ty: Ty::Words(&["X", "Y", "X-Y"]),
    },
];

/// The cursor positions, as (argument, the scope's name, unit): manual X1 and X2 (time), Y1 and Y2 (volts), and track X1 and X2 (time).
const CURSOR_POSITIONS: [(&str, &str, &str); 6] = [
    ("x1_s", "TREF", "S"),
    ("x2_s", "TDIF", "S"),
    ("y1_v", "VREF", "V"),
    ("y2_v", "VDIF", "V"),
    ("track_x1_s", "HREF", "S"),
    ("track_x2_s", "HDIF", "S"),
];

/// `bench_scope_cursor_native`: the scope's own cursors on a channel. `mode` is OFF, MANUAL or TRACK and `type` X, Y or X-Y (`CRMS`, `CRTY`); the positions
/// `x1_s`, `x2_s`, `y1_v`, `y2_v` (manual) or `track_x1_s`, `track_x2_s` (track), relative to the screen centre and the trace given by `channel`, are set with
/// `CRST`. With the cursors on it reads their values (`CRVA? HREL` and `CRVA? VREL`: the time difference, its reciprocal and the two times, the voltage
/// difference and the two voltages). The host-side `bench_scope_cursors` reads values at times from the waveform memory instead.
pub fn cursor_native(s: &mut Link, args: &Value) -> Result<Value, String> {
    // [@ANCHOR: bench_mcp_scope_cursor_native]
    let c = ch(args)?;
    let mut pairs = Vec::new();
    for (arg, name, unit) in CURSOR_POSITIONS {
        match args.get(arg) {
            None | Some(Value::Null) => {}
            Some(v) => {
                let x = v
                    .as_f64()
                    .filter(|x| x.is_finite() && x.abs() <= 1000.0)
                    .ok_or(format!("{arg} must be a number from -1000 to 1000"))?;
                pairs.push(format!("{name},{x:E}{unit}"));
            }
        }
    }
    let mut cmds = item_cmds(args, &CURSOR_ITEMS, 1)?;
    if !pairs.is_empty() {
        cmds.push((
            format!("C{c}:CRST {}", pairs.join(",")),
            format!("C{c}:CRST? TREF,TDIF,VREF,VDIF,HREF,HDIF"),
        ));
    }
    let mut r = apply(s, cmds)?;
    r["settings"] = read_items(s, &CURSOR_ITEMS, 1);
    let number = |t: &str| -> Option<f64> {
        t.trim_end_matches(|ch: char| !(ch.is_ascii_digit() || ch == '.'))
            .parse()
            .ok()
    };
    let mut values = serde_json::Map::new();
    for (kind, names) in [
        ("HREL", &["delta_s", "inverse_hz", "x1_s", "x2_s"][..]),
        ("VREL", &["delta_v", "y1_v", "y2_v"][..]),
    ] {
        s.set_timeout_secs(3);
        let text = s.query(&format!("C{c}:CRVA? {kind}")).unwrap_or_default();
        s.set_timeout_secs(10);
        let fields: Vec<&str> = text.split([' ', ',']).skip(2).collect();
        let nums: Vec<Option<f64>> = fields.iter().map(|f| number(f)).collect();
        let named: serde_json::Map<String, Value> = names
            .iter()
            .zip(&nums)
            .map(|(k, v)| (k.to_string(), json!(v)))
            .collect();
        values.insert(
            kind.to_lowercase(),
            if nums.len() == names.len() {
                Value::Object(named)
            } else {
                json!({"no_answer": text})
            },
        );
    }
    r["values"] = Value::Object(values);
    Ok(r)
}

/// How one serial-trigger parameter's value is checked.
#[derive(Clone, Copy)]
enum Sv {
    /// A source, `C1` or `C2`, optionally followed by `,` and a threshold in volts.
    Src,
    /// One of these words, or a number written as its digits.
    Words(&'static [&'static str]),
    /// A whole number in this range.
    Int(i64, i64),
    /// A number in this range, sent in E-notation followed by the unit (`S` or `V`).
    Num(f64, f64, &'static str),
    /// A JSON boolean, sent as ON or OFF.
    Flag,
    /// A data pattern of `0`, `1` and `X`, up to 96 of them.
    Bits,
    /// A baud rate: one of the standard words, or any whole number in the range (sent as `CUSTOM,<n>`).
    Baud(&'static [&'static str], i64, i64),
}

/// The serial-trigger parameters of the guide: bus, parameter (the command is `TR<BUS>:<PARAM>`), value. The argument name is the parameter in lower case with `:` as `_`.
const SERIAL_PARAMS: &[(&str, &str, Sv)] = &[
    ("IIC", "SCL", Sv::Src),
    ("IIC", "SDA", Sv::Src),
    (
        "IIC",
        "CON",
        Sv::Words(&[
            "START", "STOP", "RESTART", "NOACK", "EEPROM", "7ADDA", "10ADDA", "DALENTH",
        ]),
    ),
    ("IIC", "ADDR", Sv::Int(0, 1024)),
    ("IIC", "DATA", Sv::Int(0, 256)),
    ("IIC", "DAT2", Sv::Int(0, 256)),
    ("IIC", "QUAL", Sv::Words(&["EQUAL", "MORE", "LESS"])),
    ("IIC", "RW", Sv::Words(&["READ", "WRITE", "DONT_CARE"])),
    ("IIC", "ALEN", Sv::Words(&["7BIT", "10BIT"])),
    ("IIC", "DLEN", Sv::Int(1, 12)),
    ("SPI", "CLK", Sv::Src),
    ("SPI", "CLK:EDGE", Sv::Words(&["RISING", "FALLING"])),
    ("SPI", "CLK:TIM", Sv::Num(100e-9, 5e-3, "S")),
    ("SPI", "MOSI", Sv::Src),
    ("SPI", "MISO", Sv::Src),
    ("SPI", "CSTP", Sv::Words(&["CS", "NCS", "TIMEOUT"])),
    ("SPI", "CS", Sv::Src),
    ("SPI", "NCS", Sv::Src),
    ("SPI", "TRTY", Sv::Words(&["MOSI", "MISO"])),
    ("SPI", "DATA", Sv::Bits),
    ("SPI", "DLEN", Sv::Int(4, 96)),
    ("SPI", "BIT", Sv::Words(&["MSB", "LSB"])),
    ("UART", "RX", Sv::Src),
    ("UART", "TX", Sv::Src),
    ("UART", "TRTY", Sv::Words(&["RX", "TX"])),
    (
        "UART",
        "CON",
        Sv::Words(&["START", "STOP", "DATA", "ERROR"]),
    ),
    ("UART", "QUAL", Sv::Words(&["EQUAL", "MORE", "LESS"])),
    ("UART", "DATA", Sv::Int(0, 256)),
    (
        "UART",
        "BAUD",
        Sv::Baud(
            &[
                "600", "1200", "2400", "4800", "9600", "19200", "38400", "57600", "115200",
            ],
            300,
            5_000_000,
        ),
    ),
    ("UART", "DLEN", Sv::Int(5, 8)),
    ("UART", "PAR", Sv::Words(&["NONE", "ODD", "EVEN"])),
    ("UART", "POL", Sv::Words(&["LOW", "HIGH"])),
    ("UART", "STOP", Sv::Words(&["1", "1.5", "2"])),
    ("UART", "BIT", Sv::Words(&["LSB", "MSB"])),
    ("CAN", "CANH", Sv::Src),
    (
        "CAN",
        "CON",
        Sv::Words(&["START", "REMOTE", "ID", "ID_AND_DATA", "ERROR"]),
    ),
    ("CAN", "ID", Sv::Int(0, 536_870_912)),
    ("CAN", "IDL", Sv::Words(&["11BITS", "29BITS"])),
    ("CAN", "DATA", Sv::Int(0, 256)),
    ("CAN", "DAT2", Sv::Int(0, 256)),
    (
        "CAN",
        "BAUD",
        Sv::Baud(
            &[
                "5K", "10K", "20K", "50K", "100K", "125K", "250K", "500K", "800K", "1M",
            ],
            5000,
            1_000_000,
        ),
    ),
    ("LIN", "SRC", Sv::Src),
    (
        "LIN",
        "CON",
        Sv::Words(&["BREAK", "ID", "ID_AND_DATA", "DATA_ERROR"]),
    ),
    ("LIN", "ID", Sv::Int(0, 255)),
    ("LIN", "DATA", Sv::Int(0, 256)),
    ("LIN", "DAT2", Sv::Int(0, 256)),
    (
        "LIN",
        "BAUD",
        Sv::Baud(
            &["600", "1200", "2400", "4800", "9600", "19200"],
            300,
            20_000,
        ),
    ),
];

/// The text a serial-trigger value is sent as, or why it is refused.
fn serial_value(kind: Sv, v: &Value) -> Result<String, String> {
    // [@ANCHOR: bench_mcp_scope_serial_value]
    let word = |v: &Value| {
        v.as_str()
            .map(str::to_uppercase)
            .or_else(|| v.as_f64().map(|n| n.to_string()))
    };
    let range = |lo: f64, hi: f64| {
        v.as_f64()
            .filter(|x| x.is_finite() && (lo..=hi).contains(x))
    };
    match kind {
        Sv::Src => {
            let text = v
                .as_str()
                .ok_or("a source is a string such as C1 or C1,0.2V")?;
            let (src, thr) = text
                .split_once(',')
                .map_or((text, None), |(a, b)| (a, Some(b)));
            let src = src.trim().to_uppercase();
            if src != "C1" && src != "C2" {
                return Err("the source must be C1 or C2".into());
            }
            match thr {
                None => Ok(src),
                Some(t) => {
                    let t = t.trim().to_lowercase();
                    let (num, scale) = if let Some(n) = t.strip_suffix("mv") {
                        (n, 1e-3)
                    } else {
                        (t.strip_suffix('v').unwrap_or(&t), 1.0)
                    };
                    let x = num
                        .trim()
                        .parse::<f64>()
                        .ok()
                        .filter(|x| x.abs() * scale <= 100.0)
                        .ok_or("the threshold must be a voltage such as 0.2V")?;
                    Ok(format!("{src},{:E}V", x * scale))
                }
            }
        }
        Sv::Words(w) => word(v)
            .filter(|x| w.contains(&x.as_str()))
            .ok_or(format!("must be one of {}", w.join(", "))),
        Sv::Int(lo, hi) => v
            .as_i64()
            .filter(|x| (lo..=hi).contains(x))
            .map(|x| x.to_string())
            .ok_or(format!("must be a whole number from {lo} to {hi}")),
        Sv::Num(lo, hi, unit) => range(lo, hi)
            .map(|x| format!("{x:E}{unit}"))
            .ok_or(format!("must be a number from {lo} to {hi} ({unit})")),
        Sv::Flag => v
            .as_bool()
            .map(|b| if b { "ON" } else { "OFF" }.to_string())
            .ok_or("must be true or false".to_string()),
        Sv::Bits => {
            let bits: Vec<char> = v
                .as_str()
                .ok_or("must be a string of 0, 1 and X")?
                .chars()
                .filter(|c| *c != ',' && *c != ' ')
                .map(|c| c.to_ascii_uppercase())
                .collect();
            if bits.is_empty()
                || bits.len() > 96
                || bits.iter().any(|c| !matches!(c, '0' | '1' | 'X'))
            {
                return Err("must be 1 to 96 of 0, 1 and X".into());
            }
            Ok(bits
                .iter()
                .map(char::to_string)
                .collect::<Vec<_>>()
                .join(","))
        }
        Sv::Baud(w, lo, hi) => match word(v) {
            Some(x) if w.contains(&x.as_str()) => Ok(x),
            _ => v
                .as_i64()
                .filter(|x| (lo..=hi).contains(x))
                .map(|x| format!("CUSTOM,{x}"))
                .ok_or(format!(
                    "must be one of {} or a whole number from {lo} to {hi}",
                    w.join(", ")
                )),
        },
    }
}

/// `bench_scope_trigger`: serial triggers and the pattern trigger. `serial` is `{bus, settings}` (bus i2c, spi, uart, can or lin; settings named as the guide's
/// parameters in lower case, for example `scl`, `con`, `clk_edge`, `baud`): the trigger type is set to SERIAL (`TRSE SERIAL`) and each parameter is
/// written (`TRIIC:SCL`, ...) and read back. `pattern` is `{c1, c2, state}` (each channel L, H or X; state AND, OR, NAND or NOR; `TRPA`). Which of
/// these this unit has (they may be licensed options) is shown by the read-back.
pub fn trigger(s: &mut Link, args: &Value) -> Result<Value, String> {
    // [@ANCHOR: bench_mcp_scope_trigger]
    let mut cmds: Vec<(String, String)> = Vec::new();
    let mut names: Vec<(String, String)> = Vec::new();
    if let Some(sp) = args.get("serial").filter(|v| !v.is_null()) {
        let bus = sp
            .get("bus")
            .and_then(Value::as_str)
            .map(str::to_uppercase)
            .map(|b| if b == "I2C" { "IIC".to_string() } else { b });
        let bus = bus
            .filter(|b| SERIAL_PARAMS.iter().any(|p| p.0 == b))
            .ok_or("serial.bus must be i2c, spi, uart, can or lin")?;
        let settings = sp
            .get("settings")
            .and_then(Value::as_object)
            .filter(|m| !m.is_empty())
            .ok_or("serial.settings must name at least one parameter")?;
        let table: Vec<_> = SERIAL_PARAMS.iter().filter(|p| p.0 == bus).collect();
        if let Some(bad) = settings.keys().find(|k| {
            !table
                .iter()
                .any(|p| p.1.to_lowercase().replace(':', "_") == **k)
        }) {
            let known: Vec<String> = table
                .iter()
                .map(|p| p.1.to_lowercase().replace(':', "_"))
                .collect();
            return Err(format!(
                "serial.settings.{bad} is not a {bus} parameter ({})",
                known.join(", ")
            ));
        }
        cmds.push(("TRSE SERIAL".into(), "TRSE?".into()));
        for (_, param, kind) in table {
            let arg = param.to_lowercase().replace(':', "_");
            if let Some(v) = settings.get(&arg) {
                let text =
                    serial_value(*kind, v).map_err(|e| format!("serial.settings.{arg} {e}"))?;
                cmds.push((
                    format!("TR{bus}:{param} {text}"),
                    format!("TR{bus}:{param}?"),
                ));
                names.push((arg, format!("TR{bus}:{param}?")));
            }
        }
    }
    if let Some(p) = args.get("pattern").filter(|v| !v.is_null()) {
        let state = p
            .get("state")
            .and_then(Value::as_str)
            .map(str::to_uppercase)
            .filter(|x| ["AND", "OR", "NAND", "NOR"].contains(&x.as_str()))
            .ok_or("pattern.state must be AND, OR, NAND or NOR")?;
        let mut parts = Vec::new();
        for c in ["c1", "c2"] {
            if let Some(v) = p.get(c) {
                let l = v
                    .as_str()
                    .map(str::to_uppercase)
                    .filter(|x| ["L", "H", "X"].contains(&x.as_str()))
                    .ok_or(format!("pattern.{c} must be L, H or X"))?;
                parts.push(format!("{},{l}", c.to_uppercase()));
            }
        }
        parts.push(format!("STATE,{state}"));
        cmds.push((format!("TRPA {}", parts.join(",")), "TRPA?".into()));
    }
    if cmds.is_empty() {
        return Err("give serial or pattern".into());
    }
    let mut r = apply(s, cmds)?;
    r["trigger"] = readback(s, "TRSE?");
    r["settings"] = Value::Object(
        names
            .into_iter()
            .map(|(arg, q)| (arg, readback(s, &q)))
            .collect(),
    );
    Ok(r)
}

const CH: Sv = Sv::Words(&["C1", "C2"]);
const THR: Sv = Sv::Num(-100.0, 100.0, "V");

/// The serial-decode parameters of the guide: protocol, the command that sets its bus (`B<n>:<COMMAND>`), parameter, value. The argument name is the parameter in
/// lower case. Only the analogue channels are offered as sources (the digital ones need the MSO option).
const DECODE_PARAMS: &[(&str, &str, &str, Sv)] = &[
    ("I2C", "DCIC", "DIS", Sv::Flag),
    ("I2C", "DCIC", "SCL", CH),
    ("I2C", "DCIC", "SCLT", THR),
    ("I2C", "DCIC", "SDA", CH),
    ("I2C", "DCIC", "SDAT", THR),
    ("I2C", "DCIC", "RW", Sv::Flag),
    ("SPI", "DCSP", "DIS", Sv::Flag),
    ("SPI", "DCSP", "CLK", CH),
    ("SPI", "DCSP", "CLKT", THR),
    ("SPI", "DCSP", "EDGE", Sv::Words(&["RISING", "FALLING"])),
    ("SPI", "DCSP", "MISO", CH),
    ("SPI", "DCSP", "MISOT", THR),
    ("SPI", "DCSP", "MOSI", CH),
    ("SPI", "DCSP", "MOSIT", THR),
    ("SPI", "DCSP", "CSTP", Sv::Words(&["CS", "NCS", "TIMEOUT"])),
    ("SPI", "DCSP", "CS", CH),
    ("SPI", "DCSP", "CST", THR),
    ("SPI", "DCSP", "NCS", CH),
    ("SPI", "DCSP", "NCST", THR),
    ("SPI", "DCSP", "TIM", Sv::Num(100e-9, 5e-3, "S")),
    ("SPI", "DCSP", "BIT", Sv::Words(&["MSB", "LSB"])),
    ("SPI", "DCSP", "DLEN", Sv::Int(4, 32)),
    ("UART", "DCUT", "DIS", Sv::Flag),
    ("UART", "DCUT", "RX", CH),
    ("UART", "DCUT", "RXT", THR),
    ("UART", "DCUT", "TX", CH),
    ("UART", "DCUT", "TXT", THR),
    ("UART", "DCUT", "BAUD", Sv::Int(300, 50_000_000)),
    ("UART", "DCUT", "DLEN", Sv::Int(5, 8)),
    ("UART", "DCUT", "PAR", Sv::Words(&["NONE", "EVEN", "ODD"])),
    ("UART", "DCUT", "STOP", Sv::Words(&["1", "1.5", "2"])),
    ("UART", "DCUT", "POL", Sv::Words(&["LOW", "HIGH"])),
    ("UART", "DCUT", "BIT", Sv::Words(&["MSB", "LSB"])),
    ("CAN", "DCCN", "DIS", Sv::Flag),
    ("CAN", "DCCN", "CANH", CH),
    ("CAN", "DCCN", "CANHT", THR),
    ("CAN", "DCCN", "CANL", CH),
    ("CAN", "DCCN", "CANLT", THR),
    (
        "CAN",
        "DCCN",
        "SRC",
        Sv::Words(&["CAN_H", "CAN_L", "SUB_L"]),
    ),
    ("CAN", "DCCN", "BAUD", Sv::Int(5000, 1_000_000)),
    ("LIN", "DCLN", "DIS", Sv::Flag),
    ("LIN", "DCLN", "SRC", CH),
    ("LIN", "DCLN", "SRCT", THR),
    ("LIN", "DCLN", "BAUD", Sv::Int(300, 2000)),
];

/// `bench_scope_decode`: serial decode. `on` switches decode on or off (`DCST`); `list` (OFF, D1, D2), `format` (BIN, DEC, HEX) and `bus` (1 or 2) set the
/// decode list and the bus it follows (`DCPA`); `protocol` (i2c, spi, uart, can, lin) with `settings` (the guide's parameters in lower case, such as `scl`,
/// `baud`, `dis`) configures a bus (`B<n>:DCIC`, `DCSP`, `DCUT`, `DCCN`, `DCLN`). The guide has no query for the bus settings and no way to read the decoded
/// frames over SCPI, so the decode state is read back (`DCST?`) and the decoded list is seen with `bench_screenshot`.
pub fn decode(s: &mut Link, args: &Value) -> Result<Value, String> {
    // [@ANCHOR: bench_mcp_scope_decode]
    let bus = match args.get("bus") {
        None | Some(Value::Null) => None,
        Some(v) => Some(
            v.as_u64()
                .filter(|b| (1..=2).contains(b))
                .ok_or("bus must be 1 or 2")?,
        ),
    };
    let mut cmds: Vec<String> = Vec::new();
    let on = match args.get("on") {
        None | Some(Value::Null) => None,
        Some(v) => Some(v.as_bool().ok_or("on must be true or false")?),
    };
    cmds.extend(on.map(|b| format!("DCST {}", if b { "ON" } else { "OFF" })));
    let mut common = Vec::new();
    if let Some(b) = bus {
        common.push(format!("BUS,B{b}"));
    }
    for (k, words) in [
        ("list", &["OFF", "D1", "D2"][..]),
        ("format", &["BIN", "DEC", "HEX"][..]),
    ] {
        if let Some(v) = args.get(k).filter(|v| !v.is_null()) {
            let w = v
                .as_str()
                .map(str::to_uppercase)
                .filter(|w| words.contains(&w.as_str()))
                .ok_or(format!("{k} must be one of {}", words.join(", ")))?;
            common.push(format!("{},{w}", if k == "list" { "LIST" } else { "FOMT" }));
        }
    }
    if !common.is_empty() {
        cmds.push(format!("DCPA {}", common.join(",")));
    }
    if let Some(proto) = args.get("protocol").filter(|v| !v.is_null()) {
        let proto = proto
            .as_str()
            .map(str::to_uppercase)
            .filter(|p| DECODE_PARAMS.iter().any(|d| d.0 == p))
            .ok_or("protocol must be i2c, spi, uart, can or lin")?;
        let n = bus.ok_or("protocol needs bus (1 or 2)")?;
        let settings = args
            .get("settings")
            .and_then(Value::as_object)
            .filter(|m| !m.is_empty())
            .ok_or("settings must name at least one parameter")?;
        let table: Vec<_> = DECODE_PARAMS.iter().filter(|d| d.0 == proto).collect();
        if let Some(bad) = settings
            .keys()
            .find(|k| !table.iter().any(|d| d.2.to_lowercase() == **k))
        {
            let known: Vec<String> = table.iter().map(|d| d.2.to_lowercase()).collect();
            return Err(format!(
                "settings.{bad} is not a {proto} decode parameter ({})",
                known.join(", ")
            ));
        }
        let mut pairs = Vec::new();
        for (_, _, param, kind) in &table {
            if let Some(v) = settings.get(&param.to_lowercase()) {
                let text = serial_value(*kind, v)
                    .map_err(|e| format!("settings.{} {e}", param.to_lowercase()))?;
                pairs.push(format!("{param},{text}"));
            }
        }
        cmds.push(format!("B{n}:{} {}", table[0].1, pairs.join(",")));
    } else if args.get("settings").is_some_and(|v| !v.is_null()) {
        return Err("settings needs protocol".into());
    }
    for c in &cmds {
        s.send(c)?;
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    Ok(json!({"sent": cmds, "decode": readback(s, "DCST?"),
        "note": "the guide has no query for the decode bus settings: check them on the screen (bench_screenshot); commands from the SDS1000X-E programming guide, not yet checked on firmware 1.3.27 (decode may be a licensed option)"}))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fake_scope::FakeScope;

    fn link() -> (FakeScope, Link) {
        let f = FakeScope::start();
        let l = Link::tcp("127.0.0.1", f.port).unwrap();
        (f, l)
    }

    // Tests [@ANCHOR: bench_mcp_scope_history]
    #[test]
    fn history_turns_on_reads_frames_with_their_times_and_refuses_bad_frames_unsent() {
        let (f, mut l) = link();
        let out = crate::testdir::scratch("scope_guide_history");
        let r = history(&mut l, &json!({"history": true, "list": true, "frames": [0, 40, 80], "channel": 1, "save": true}), &out).unwrap();
        assert_eq!(r["sent"], json!(["HSMD ON", "HSLST ON"]));
        assert_eq!(r["settings"]["history"], "HSMD ON");
        let rows = r["frames"].as_array().unwrap();
        // The fake's amplitude falls by 1 code per frame: 100 codes at 0.5 V per division is 2 V peak, 60 then 20 codes later.
        let pk: Vec<f64> = rows.iter().map(|x| x["pkpk_v"].as_f64().unwrap()).collect();
        assert!(
            (pk[0] - 4.0).abs() < 0.05 && (pk[1] - 2.4).abs() < 0.05 && (pk[2] - 0.8).abs() < 0.05,
            "{pk:?}"
        );
        assert_eq!(rows[1]["time"], "FTIM 00: 05: 40. 690814");
        assert!(
            std::fs::metadata(rows[0]["file"].as_str().unwrap())
                .unwrap()
                .len()
                == 4000
        );
        assert_eq!(r["settings"]["frame"], "FRAM 80");
        let sent = f.got("HSMD").len();
        for bad in [
            json!({"history": "on"}),
            json!({"frames": [1, 2], "history": true}),
            json!({"frames": [200000], "channel": 1}),
            json!({"frames": (0..21).collect::<Vec<_>>(), "channel": 1}),
        ] {
            assert!(history(&mut l, &bad, &out).is_err(), "{bad}");
        }
        assert_eq!(f.got("HSMD").len(), sent, "nothing sent for a bad call");
    }

    // Tests [@ANCHOR: bench_mcp_scope_reference]
    #[test]
    fn a_reference_is_selected_stored_shown_scaled_and_closed_in_the_guides_order() {
        let (f, mut l) = link();
        let r = reference(&mut l, &json!({"scale_v": 0.1, "location": "refb", "source": "C2", "save": true, "position_v": 0.2})).unwrap();
        assert_eq!(
            r["sent"],
            json!([
                "REFLA REFB",
                "REFSR C2",
                "REFSA",
                "REFPO 2E-1V",
                "REFSC 1E-1V"
            ])
        );
        assert_eq!(
            r["settings"]["display"], "REFDS ON",
            "storing a trace shows it"
        );
        assert_eq!(r["settings"]["scale_v"], "REFSC 1.00E-01V");
        let r = reference(&mut l, &json!({"display": false})).unwrap();
        assert_eq!(r["settings"]["display"], "REFDS OFF");
        let r = reference(&mut l, &json!({"close": true})).unwrap();
        assert_eq!(r["settings"]["display"], "REFDS OFF");
        // A display setting for a location with nothing stored is ignored by the scope: the read-back shows it.
        reference(&mut l, &json!({"location": "REFC", "display": true})).unwrap();
        assert_eq!(
            reference(&mut l, &json!({})).unwrap()["settings"]["display"],
            "REFDS OFF"
        );
        let n = f.got("REF").len();
        for bad in [
            json!({"location": "REFE"}),
            json!({"scale_v": 100}),
            json!({"save": "yes"}),
            json!({"display": true, "source": "C3"}),
        ] {
            assert!(reference(&mut l, &bad).is_err(), "{bad}");
        }
        assert_eq!(f.got("REF").len(), n);
    }

    // Tests [@ANCHOR: bench_mcp_scope_setup]
    #[test]
    fn a_setup_is_saved_recalled_exported_and_restored_from_a_file() {
        let (f, mut l) = link();
        let out = crate::testdir::scratch("scope_guide_setup");
        l.send("TDIV 5us").unwrap();
        let r = setup(&mut l, &json!({"save_slot": 3, "export": true}), &out).unwrap();
        assert_eq!(r["saved_slot"], 3);
        let file = r["exported"]["file"].as_str().unwrap().to_string();
        assert!(std::fs::read_to_string(&file)
            .unwrap()
            .contains("TDIV=5.00E-06S"));
        l.send("TDIV 1ms").unwrap();
        let name = std::path::Path::new(&file)
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .to_string();
        let r = setup(&mut l, &json!({"import": name}), &out).unwrap();
        assert_eq!(
            r["after"]["timebase"], "TDIV 5.00E-06S",
            "the file restored the setup"
        );
        l.send("TDIV 2ms").unwrap();
        assert_eq!(
            setup(&mut l, &json!({"recall_slot": 3}), &out).unwrap()["after"]["timebase"],
            "TDIV 5.00E-06S"
        );
        assert_eq!(
            setup(&mut l, &json!({"recall_slot": 0}), &out).unwrap()["after"]["timebase"],
            "TDIV 1.00E-03S",
            "slot 0 is the default"
        );
        let n = f.seen.lock().unwrap().len();
        for bad in [
            json!({"save_slot": 0}),
            json!({"recall_slot": 21}),
            json!({"import": "../x"}),
            json!({"import": "missing.set"}),
            json!({"save_slot": 2, "import": "/etc/passwd"}),
        ] {
            assert!(setup(&mut l, &bad, &out).is_err(), "{bad}");
        }
        assert_eq!(
            f.seen.lock().unwrap().len(),
            n,
            "nothing sent for a bad call"
        );
    }

    // Tests [@ANCHOR: bench_mcp_scope_passfail]
    #[test]
    fn a_mask_test_is_set_up_created_run_and_its_counts_are_read() {
        let (f, mut l) = link();
        let r = passfail(&mut l, &json!({"enable": true, "source": "C1", "x_mask_div": 0.4, "y_mask_div": 0.52, "create_mask": true, "stop_on_fail": false, "display": true})).unwrap();
        assert_eq!(
            r["sent"],
            json!([
                "PFEN ON",
                "PFSC C1",
                "PFST XMASK,0.4,YMASK,0.52",
                "PFCM",
                "PFDS ON",
                "PFFS OFF"
            ])
        );
        assert_eq!(r["mask_tolerance"], "PFST XMASK,0.4,YMASK,0.52");
        assert_eq!(
            r["result"],
            json!({"failed": 0, "passed": 0, "total": 0}),
            "stopped: nothing counted"
        );
        let r = passfail(&mut l, &json!({"run": true})).unwrap();
        assert_eq!(r["settings"]["run"], "PFOP ON");
        // While it runs, each read of the counts sees 10 more frames, 1 of them failed (the fake's rule).
        assert_eq!(r["result"], json!({"failed": 1, "passed": 9, "total": 10}));
        let r = passfail(&mut l, &json!({"clear": true})).unwrap();
        assert_eq!(r["sent"], json!(["PACL"]));
        let r = passfail(&mut l, &json!({"run": false})).unwrap();
        assert_eq!(r["settings"]["run"], "PFOP OFF");
        let n = f.seen.lock().unwrap().len();
        for bad in [
            json!({"x_mask_div": 0.4}),
            json!({"x_mask_div": 0.41, "y_mask_div": 1}),
            json!({"y_mask_div": 5, "x_mask_div": 1}),
            json!({"source": "C3"}),
            json!({"create_mask": 1}),
        ] {
            assert!(passfail(&mut l, &bad).is_err(), "{bad}");
        }
        assert_eq!(f.seen.lock().unwrap().len(), n);
    }

    // Tests [@ANCHOR: bench_mcp_scope_measure_custom]
    #[test]
    fn custom_measurements_are_installed_read_with_statistics_gates_and_delay() {
        let (f, mut l) = link();
        let r = measure_custom(&mut l, &json!({"clear": true, "install": [{"item": "pkpk", "channel": 1}, {"item": "FREQ", "channel": 2}], "statistics": "on", "gate": true, "gate_a_s": 2e-5, "gate_b_s": 1.68e-3})).unwrap();
        assert_eq!(
            r["sent"],
            json!([
                "MEACL",
                "PACU PKPK,C1",
                "PACU FREQ,C2",
                "PASTAT ON",
                "MEGS ON",
                "MEGA 2E-5S",
                "MEGB 1.68E-3S"
            ])
        );
        assert_eq!(
            r["custom"][0],
            json!({"slot": "CUST1", "channel": "C1", "item": "PKPK", "value": "4.00E+00V"})
        );
        assert_eq!(r["custom"][2], json!({"slot": "CUST3", "off": true}));
        assert_eq!(r["gate"]["gate_a_s"], "MEGA 2.00E-05S");
        // The guide shows each statistics answer on two lines; they are joined.
        let st = r["statistics"].as_array().unwrap();
        assert_eq!(st.len(), 2);
        assert_eq!(
            (
                st[1]["cur"].as_str().unwrap(),
                st[1]["count"].as_str().unwrap()
            ),
            ("1.00E+03Hz", "171")
        );
        // The same statistics when the scope answers on one line.
        let one = FakeScope::start_with(crate::fake_scope::Opts {
            stat_one_line: true,
            ..Default::default()
        });
        let mut l1 = Link::tcp("127.0.0.1", one.port).unwrap();
        let r1 = measure_custom(
            &mut l1,
            &json!({"install": [{"item": "FREQ", "channel": 1}], "statistics": "on"}),
        )
        .unwrap();
        assert_eq!(r1["statistics"][0]["mean"], "1.00E+03Hz");
        let r = measure_custom(&mut l, &json!({"delay": {"type": "PHA", "sources": "C1-C2"}, "snapshot": 1, "statistics": "off"})).unwrap();
        assert_eq!(r["delay"], "C1-C2:MEAD PHA,-8.95E+01degree");
        assert_eq!(r["snapshot"]["PKPK"], "4.00E+00V");
        assert_eq!(r["snapshot"]["DUTY"], "5.00E+01%");
        assert!(
            r.get("statistics").is_none(),
            "statistics are read only while they are on"
        );
        let n = f.seen.lock().unwrap().len();
        let too_many: Vec<Value> = (0..6)
            .map(|_| json!({"item": "PKPK", "channel": 1}))
            .collect();
        for bad in [
            json!({"install": [{"item": "BOGUS", "channel": 1}]}),
            json!({"install": too_many}),
            json!({"install": [{"item": "MAX", "channel": 3}]}),
            json!({"statistics": "maybe"}),
            json!({"delay": {"type": "PHA", "sources": "C1-C3"}}),
            json!({"snapshot": 3}),
            json!({"gate": "on"}),
        ] {
            assert!(measure_custom(&mut l, &bad).is_err(), "{bad}");
        }
        assert_eq!(f.seen.lock().unwrap().len(), n);
    }

    // Tests [@ANCHOR: bench_mcp_scope_counter]
    // Tests [@ANCHOR: bench_mcp_scope_hertz]
    #[test]
    fn the_counter_is_read_coarse_and_full_precision_and_a_low_frequency_has_no_hertz_value() {
        let (_f, mut l) = link();
        let r = counter(&mut l).unwrap();
        assert_eq!(
            (r["counter_hz"].as_f64(), r["full_precision_hz"].as_f64()),
            (Some(1000.0), Some(1000.0))
        );
        assert_eq!(r["full_precision"], "CYMT 1.00000kHz");
        assert_eq!(
            (
                hertz("FFTT 100.00MHz"),
                hertz("2.5E+07Hz"),
                hertz("<10Hz"),
                hertz("3GHz"),
                hertz("5V")
            ),
            (Some(1e8), Some(2.5e7), None, Some(3e9), None)
        );
        // A scope that answers as the guide says for a signal under 10 Hz, and the header the guide's CYMOMETER? syntax shows.
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            use std::io::{BufRead, Write};
            let (sock, _) = listener.accept().unwrap();
            let mut out = sock.try_clone().unwrap();
            for l in std::io::BufReader::new(sock).lines() {
                let reply = if l.unwrap() == "CYMT?" {
                    "CYMOMETER <10Hz\n"
                } else {
                    "CYMT 94.5177261kHz\n"
                };
                out.write_all(reply.as_bytes()).unwrap();
            }
        });
        let mut l = Link::tcp("127.0.0.1", port).unwrap();
        let r = counter(&mut l).unwrap();
        assert!(r["counter_hz"].is_null());
        assert_eq!(r["counter"], "CYMOMETER <10Hz");
        assert!((r["full_precision_hz"].as_f64().unwrap() - 94517.7261).abs() < 1e-6);
    }

    // Tests [@ANCHOR: bench_mcp_scope_math_native]
    // Tests [@ANCHOR: bench_mcp_scope_equations]
    #[test]
    fn the_scopes_own_math_is_defined_scaled_read_and_its_fft_is_set_up() {
        let (f, mut l) = link();
        let out = crate::testdir::scratch("scope_guide_math");
        let r = math_native(&mut l, &json!({"equation": "c1+c2", "scale_v": 1, "position": -50, "invert": false, "read": true}), &out).unwrap();
        assert_eq!(
            r["sent"],
            json!([
                "DEFINE EQN,'C1+C2'",
                "MTVD 1E0V",
                "MTVP -50",
                "MATH:INVS OFF"
            ])
        );
        assert_eq!(r["equation"], "DEFINE EQN,'C1+C2'");
        // The two sines add to 2 V * sqrt(2) peak; the guide's formula undoes the scale and position the fake encoded with.
        assert!(
            (r["trace"]["max"].as_f64().unwrap() - 2.0 * 2f64.sqrt()).abs() < 0.1,
            "{r}"
        );
        assert_eq!(
            std::fs::metadata(r["trace"]["file"].as_str().unwrap())
                .unwrap()
                .len(),
            1400 * 4
        );
        let r = math_native(&mut l, &json!({"equation": "FFTC1", "fft_window": "hamm", "fft_center_hz": 58e6, "fft_unit": "dbvrms", "fft_scale": 5, "fft_screen": "ON", "read": true}), &out).unwrap();
        assert_eq!(r["settings"]["fft_window"], "FFTW HAMM");
        assert_eq!(r["settings"]["fft_center_hz"], "FFTC 5.80E+07Hz");
        assert_eq!(r["settings"]["fft_unit"], "FFTU DBVRMS");
        assert_eq!(
            (
                r["fft_trace"]["points"].as_u64(),
                r["fft_trace"]["peak_index"].as_u64()
            ),
            (Some(1500), Some(750))
        );
        let f_hz = r["fft_trace"]["peak_frequency_hz_assumed"]
            .as_f64()
            .unwrap();
        assert!(
            (f_hz - (58e6 + (750.0 / 1499.0 - 0.5) * 1e9)).abs() < 1.0,
            "{f_hz}"
        );
        let n = f.seen.lock().unwrap().len();
        for bad in [
            json!({"equation": "C1^2"}),
            json!({"equation": "FFT(C1)"}),
            json!({"fft_window": "GAUSS"}),
            json!({"position": 300}),
            json!({"read": "yes"}),
            json!({"scale_v": 1000}),
        ] {
            assert!(math_native(&mut l, &bad, &out).is_err(), "{bad}");
        }
        assert_eq!(f.seen.lock().unwrap().len(), n);
    }

    // Tests [@ANCHOR: bench_mcp_scope_cursor_native]
    #[test]
    fn the_scopes_own_cursors_are_set_and_their_values_read() {
        let (f, mut l) = link();
        let r = cursor_native(&mut l, &json!({"channel": 1, "mode": "manual", "type": "x-y", "x1_s": -3e-6, "x2_s": 1e-6, "y1_v": -0.5, "y2_v": 0.25})).unwrap();
        assert_eq!(
            r["sent"],
            json!([
                "CRMS MANUAL",
                "CRTY X-Y",
                "C1:CRST TREF,-3E-6S,TDIF,1E-6S,VREF,-5E-1V,VDIF,2.5E-1V"
            ])
        );
        assert_eq!(
            r["values"]["hrel"],
            json!({"delta_s": 4e-6, "inverse_hz": 250000.0, "x1_s": -3e-6, "x2_s": 1e-6})
        );
        assert_eq!(
            r["values"]["vrel"],
            json!({"delta_v": 0.75, "y1_v": -0.5, "y2_v": 0.25})
        );
        let r = cursor_native(
            &mut l,
            &json!({"channel": 2, "mode": "TRACK", "track_x1_s": 0.0, "track_x2_s": 5e-6}),
        )
        .unwrap();
        assert_eq!(r["values"]["hrel"]["delta_s"], 5e-6);
        // Switched off, the scope gives no values.
        let r = cursor_native(&mut l, &json!({"channel": 1, "mode": "off"})).unwrap();
        assert!(r["values"]["hrel"]["no_answer"].is_string());
        let n = f.seen.lock().unwrap().len();
        for bad in [
            json!({"mode": "ON"}),
            json!({"channel": 3}),
            json!({"channel": 1, "mode": "ON"}),
            json!({"channel": 1, "x1_s": "0"}),
            json!({"channel": 1, "y1_v": 5000}),
        ] {
            assert!(cursor_native(&mut l, &bad).is_err(), "{bad}");
        }
        assert_eq!(f.seen.lock().unwrap().len(), n);
    }

    // Tests [@ANCHOR: bench_mcp_scope_trigger]
    // Tests [@ANCHOR: bench_mcp_scope_serial_value]
    #[test]
    fn serial_and_pattern_triggers_are_set_checked_and_read_back() {
        let (f, mut l) = link();
        let r = trigger(&mut l, &json!({"serial": {"bus": "i2c", "settings": {"con": "7adda", "sda": "C2,200mV", "scl": "C1", "addr": 80, "rw": "write"}}})).unwrap();
        assert_eq!(
            r["sent"],
            json!([
                "TRSE SERIAL",
                "TRIIC:SCL C1",
                "TRIIC:SDA C2,2E-1V",
                "TRIIC:CON 7ADDA",
                "TRIIC:ADDR 80",
                "TRIIC:RW WRITE"
            ])
        );
        assert_eq!(r["settings"]["sda"], "TRIIC:SDA C2,2E-1V");
        let r = trigger(&mut l, &json!({"serial": {"bus": "spi", "settings": {"clk_tim": 1e-6, "data": "1,0,x", "bit": "MSB", "clk_edge": "rising"}}})).unwrap();
        assert_eq!(
            r["sent"],
            json!([
                "TRSE SERIAL",
                "TRSPI:CLK:EDGE RISING",
                "TRSPI:CLK:TIM 1E-6S",
                "TRSPI:DATA 1,0,X",
                "TRSPI:BIT MSB"
            ])
        );
        let r = trigger(
            &mut l,
            &json!({"serial": {"bus": "uart", "settings": {"baud": 9600, "dlen": 8, "stop": 1.5}}}),
        )
        .unwrap();
        assert_eq!(
            r["sent"],
            json!(["TRSE SERIAL", "TRUART:BAUD 9600", "TRUART:DLEN 8"]
                .iter()
                .chain(&["TRUART:STOP 1.5"])
                .collect::<Vec<_>>())
        );
        let r = trigger(
            &mut l,
            &json!({"serial": {"bus": "can", "settings": {"baud": 33333, "idl": "29bits"}}}),
        )
        .unwrap();
        assert!(r["sent"]
            .as_array()
            .unwrap()
            .contains(&json!("TRCAN:BAUD CUSTOM,33333")));
        let r = trigger(
            &mut l,
            &json!({"pattern": {"c1": "h", "c2": "L", "state": "and"}}),
        )
        .unwrap();
        assert_eq!(r["sent"], json!(["TRPA C1,H,C2,L,STATE,AND"]));
        assert_eq!(r["readback"]["TRPA?"], "TRPA C1,H,C2,L,STATE,AND");
        let n = f.seen.lock().unwrap().len();
        for bad in [
            json!({}),
            json!({"serial": {"bus": "usb", "settings": {"con": "START"}}}),
            json!({"serial": {"bus": "i2c", "settings": {}}}),
            json!({"serial": {"bus": "i2c", "settings": {"baud": 1}}}),
            json!({"serial": {"bus": "i2c", "settings": {"con": "GO"}}}),
            json!({"serial": {"bus": "i2c", "settings": {"scl": "C3"}}}),
            json!({"serial": {"bus": "i2c", "settings": {"scl": "C1,abc"}}}),
            json!({"serial": {"bus": "spi", "settings": {"data": "12"}}}),
            json!({"serial": {"bus": "uart", "settings": {"baud": 100}}}),
            json!({"serial": {"bus": "i2c", "settings": {"dlen": 13}}}),
            json!({"pattern": {"c1": "H"}}),
            json!({"pattern": {"state": "XOR"}}),
            json!({"pattern": {"c3": "H", "c1": "Q", "state": "OR"}}),
        ] {
            assert!(trigger(&mut l, &bad).is_err(), "{bad}");
        }
        assert_eq!(
            f.seen.lock().unwrap().len(),
            n,
            "nothing sent for a bad call"
        );
    }

    // Tests [@ANCHOR: bench_mcp_scope_set]
    #[test]
    fn bench_set_takes_the_drop_out_trigger_and_refuses_unknown_types_unsent() {
        let (f, mut l) = link();
        let r = crate::scope::set(
            &mut l,
            &json!({"trigger_type": "DROP", "channel": 2, "timebase_s": 1e-3}),
        )
        .unwrap();
        assert_eq!(r["sent"], json!(["TDIV 1E-3S", "TRSE DROP,SR,C2"]));
        assert_eq!(f.got("TRSE"), ["TRSE DROP,SR,C2"]);
        assert!(
            crate::scope::set(&mut l, &json!({"trigger_type": "WINDOW", "channel": 1}))
                .unwrap_err()
                .contains("DROP")
        );
        assert_eq!(f.got("TRSE").len(), 1);
    }

    // Tests [@ANCHOR: bench_mcp_scope_decode]
    #[test]
    fn serial_decode_is_switched_on_configured_per_bus_and_every_value_is_checked() {
        let (f, mut l) = link();
        let r = decode(&mut l, &json!({"on": true, "bus": 1, "list": "d1", "format": "hex", "protocol": "uart", "settings": {"baud": 9600, "rx": "C1", "rxt": 0.2, "par": "odd", "stop": 2, "dis": true}})).unwrap();
        assert_eq!(
            r["sent"],
            json!([
                "DCST ON",
                "DCPA BUS,B1,LIST,D1,FOMT,HEX",
                "B1:DCUT DIS,ON,RX,C1,RXT,2E-1V,BAUD,9600,PAR,ODD,STOP,2"
            ])
        );
        assert_eq!(r["decode"], "DCST ON");
        let r = decode(&mut l, &json!({"bus": 2, "protocol": "i2c", "settings": {"scl": "C1", "sda": "C2", "rw": false}})).unwrap();
        assert_eq!(
            r["sent"],
            json!(["DCPA BUS,B2", "B2:DCIC SCL,C1,SDA,C2,RW,OFF"])
        );
        let r = decode(&mut l, &json!({"bus": 1, "protocol": "spi", "settings": {"clk": "C1", "edge": "falling", "tim": 1e-6, "dlen": 16}})).unwrap();
        assert_eq!(
            r["sent"][1],
            "B1:DCSP CLK,C1,EDGE,FALLING,TIM,1E-6S,DLEN,16"
        );
        assert_eq!(
            decode(
                &mut l,
                &json!({"bus": 1, "protocol": "can", "settings": {"src": "can_h", "baud": 500000}})
            )
            .unwrap()["sent"][1],
            "B1:DCCN SRC,CAN_H,BAUD,500000"
        );
        assert_eq!(
            decode(
                &mut l,
                &json!({"bus": 1, "protocol": "lin", "settings": {"baud": 1200}})
            )
            .unwrap()["sent"][1],
            "B1:DCLN BAUD,1200"
        );
        assert_eq!(
            decode(&mut l, &json!({"on": false})).unwrap()["decode"],
            "DCST OFF"
        );
        let n = f.seen.lock().unwrap().len();
        for bad in [
            json!({"bus": 3}),
            json!({"on": "yes"}),
            json!({"list": "D3"}),
            json!({"format": "OCT"}),
            json!({"protocol": "uart", "settings": {"baud": 9600}}),
            json!({"bus": 1, "protocol": "usb", "settings": {"baud": 9600}}),
            json!({"bus": 1, "protocol": "uart"}),
            json!({"bus": 1, "settings": {"baud": 9600}}),
            json!({"bus": 1, "protocol": "uart", "settings": {"baud": 100}}),
            json!({"bus": 1, "protocol": "uart", "settings": {"scl": "C1"}}),
            json!({"bus": 1, "protocol": "i2c", "settings": {"scl": "D0"}}),
            json!({"bus": 1, "protocol": "lin", "settings": {"baud": 9600}}),
            json!({"bus": 1, "protocol": "uart", "settings": {"rxt": 500}}),
        ] {
            assert!(decode(&mut l, &bad).is_err(), "{bad}");
        }
        assert_eq!(
            f.seen.lock().unwrap().len(),
            n,
            "nothing sent for a bad call"
        );
    }

    // Tests [@ANCHOR: bench_mcp_scope_status]
    // Tests [@ANCHOR: bench_mcp_scope_measure]
    // Tests [@ANCHOR: bench_mcp_scope_capture]
    // Tests [@ANCHOR: bench_mcp_scope_spectrum]
    // Tests [@ANCHOR: bench_mcp_scope_screenshot]
    // Tests [@ANCHOR: bench_mcp_scope_read_waveform]
    #[test]
    fn the_older_scope_tools_read_status_measurements_waveform_spectrum_and_screen_from_the_fake() {
        let (_f, mut l) = link();
        let out = crate::testdir::scratch("scope_guide_older");
        let st = crate::scope::status(&mut l).unwrap();
        assert_eq!(st["trigger_mode"], "AUTO");
        assert_eq!(st["channels"].as_array().unwrap().len(), 2);
        let m = crate::scope::measure(&mut l, &json!({"channel": 1, "items": ["PKPK", "FREQ"]}))
            .unwrap();
        assert!(m.to_string().matches("E+").count() >= 2, "{m}");
        let c = crate::scope::capture(&mut l, &json!({"channel": 1}), &out).unwrap();
        assert_eq!(c["samples"], 1000);
        assert!((c["pkpk_v"].as_f64().unwrap() - 4.0).abs() < 0.05, "{c}");
        let sp = crate::scope::spectrum(
            &mut l,
            &json!({"channel": 1, "points": 512, "window": "hanning"}),
            &out,
        )
        .unwrap();
        assert!(!sp["peaks"].as_array().unwrap().is_empty(), "{sp}");
        let shot = crate::scope::screenshot(&mut l, &out).unwrap();
        assert!(shot["file"].as_str().unwrap().ends_with(".bmp"));
    }
}
