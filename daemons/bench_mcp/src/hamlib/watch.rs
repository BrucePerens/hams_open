// SPDX-License-Identifier: AGPL-3.0-or-later
//! `bench_hamlib_watch`: what the rig's dial does for a few seconds, by polling. A person turns the knob, or software tunes the rig,
//! and the changes of frequency, mode, VFO and PTT come back with their times. (rigctld's own event channel, `set_trn`, is
//! deprecated in the Hamlib version this was written against, so the tool polls.) Read only.

use super::proto::Client;
use super::status::{bounded, tolerate};
use serde_json::{json, Value};
use std::time::{Duration, Instant};

const MAX_EVENTS: usize = 200;

/// One poll of the values worth watching: name and the text of its value (or `unavailable`).
fn poll(c: &mut Client) -> Result<Vec<(&'static str, String)>, String> {
    // [@ANCHOR: hamlib_watch_poll]
    let mut v = vec![];
    v.push((
        "frequency_hz",
        match tolerate(c.read_number("get_freq", Some("Frequency")))? {
            Ok(f) => format!("{f:.0}"),
            Err(_) => "unavailable".into(),
        },
    ));
    match tolerate(c.read("get_mode"))? {
        Ok(r) => {
            v.push(("mode", r.kv("Mode").unwrap_or("?").to_string()));
            v.push(("passband_hz", r.kv("Passband").unwrap_or("?").to_string()));
        }
        Err(_) => v.push(("mode", "unavailable".into())),
    }
    v.push((
        "vfo",
        match tolerate(c.read("get_vfo"))? {
            Ok(r) => r.kv("VFO").unwrap_or("?").to_string(),
            Err(_) => "unavailable".into(),
        },
    ));
    v.push((
        "ptt",
        match tolerate(c.read("get_ptt"))? {
            Ok(r) => match r.kv("PTT") {
                Some("0") => "receive".into(),
                Some(_) => "TRANSMIT".into(),
                None => "unknown".into(),
            },
            Err(_) => "unavailable".into(),
        },
    ));
    Ok(v)
}

pub fn watch(c: &mut Client, args: &Value) -> Result<Value, String> {
    // [@ANCHOR: hamlib_watch]
    let seconds = bounded(args, "seconds", 10.0, 1.0, 120.0)?;
    let every = bounded(args, "interval_ms", 500.0, 100.0, 5000.0)?;
    let began = Instant::now();
    let initial = poll(c)?;
    let mut last = initial.clone();
    let mut events: Vec<Value> = vec![];
    let mut polls = 1;
    let mut truncated = false;
    while began.elapsed().as_secs_f64() < seconds {
        std::thread::sleep(Duration::from_millis(every as u64));
        let now = poll(c)?;
        polls += 1;
        let t = (began.elapsed().as_secs_f64() * 100.0).round() / 100.0;
        for ((name, old), (_, new)) in last.iter().zip(&now) {
            if old != new {
                if events.len() >= MAX_EVENTS {
                    truncated = true;
                } else {
                    events.push(json!({"t_s": t, "what": name, "from": old, "to": new}));
                }
            }
        }
        last = now;
    }
    Ok(json!({
        "seconds": (began.elapsed().as_secs_f64() * 100.0).round() / 100.0, "polls": polls, "interval_ms": every,
        "initial": initial.iter().map(|(k, v)| (k.to_string(), json!(v))).collect::<serde_json::Map<_, _>>(),
        "events": events, "events_truncated": truncated,
        "note": "polled, so a change that reverts between two polls is not seen; frequency_hz is a text of the dial reading in Hz",
    }))
}
