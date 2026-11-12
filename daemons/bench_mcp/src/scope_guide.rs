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
            Some(
                (t.zip(src).ok_or_else(|| {
                    format!(
                        "delay must be {{type one of {}, sources C1-C2 or C2-C1}}",
                        DELAY_TYPES.join(", ")
                    )
                })?),
            )
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

/// `bench_scope_counter`: the scope's hardware frequency counter (`CYMT?`, E-notation, and `CYMT_HW?`, full precision with kHz or MHz), each as the
/// scope's text and as hertz. The counter counts trigger-level crossings of the trigger source. Below 10 Hz the scope answers `10Hz` or `<10Hz`, which
/// has no hertz value here (`null`). The guide lists no histogram or DVM function for this series, so none is offered.
pub fn counter(s: &mut Link) -> Result<Value, String> {
    // [@ANCHOR: bench_mcp_scope_counter]
    let hz = |text: &str| -> Option<f64> {
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
    };
    let coarse = s.query("CYMT?")?;
    let full = s.query("CYMT_HW?")?;
    Ok(
        json!({"counter": coarse, "counter_hz": hz(&coarse), "full_precision": full, "full_precision_hz": hz(&full),
        "note": "counts the trigger source's level crossings; below 10 Hz the scope answers 10Hz or <10Hz; from the programming guide, not yet checked on firmware 1.3.27"}),
    )
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
    #[test]
    fn the_counter_is_read_coarse_and_full_precision_and_a_low_frequency_has_no_hertz_value() {
        let (_f, mut l) = link();
        let r = counter(&mut l).unwrap();
        assert_eq!(
            (r["counter_hz"].as_f64(), r["full_precision_hz"].as_f64()),
            (Some(1000.0), Some(1000.0))
        );
        assert_eq!(r["full_precision"], "CYMT 1.00000kHz");
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
}
