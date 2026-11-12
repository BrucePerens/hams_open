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
}
