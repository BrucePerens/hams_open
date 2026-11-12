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
}
