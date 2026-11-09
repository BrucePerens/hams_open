// SPDX-License-Identifier: AGPL-3.0-or-later
//! The waveform model of the logic-analyzer tools: a list of timestamped changes of up to 16 pins, read from the VCD file the Glasgow's
//! `analyzer` applet writes, written back as VCD or CSV, windowed around a trigger and sampled at a chosen rate. Pure code: no device,
//! no files. Entries: `docs/GLASGOW_LOGIC_TOOLS.md`.

use serde_json::{json, Value};

/// The most pins one capture holds (the analyzer applet's limit).
pub const MAX_PINS: usize = 16;

/// One change of the pins: at `t_ns` the pins take `value` (bit n = pin n). Changes with the same timestamp are one entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Edge {
    pub t_ns: u64,
    pub value: u32,
}

/// A captured waveform. `init` is the state at time 0, `edges` are strictly increasing in time, `end_ns` is when the capture ended.
/// `origin_ns` is where time 0 of this wave lies in the original capture (non-zero after `window`); `overrun_ns` is set when the
/// analyzer reported a FIFO overrun (samples after it are lost).
#[derive(Clone, Debug, PartialEq)]
pub struct Wave {
    // [@ANCHOR: gla_wave_struct]
    pub names: Vec<String>,
    pub init: u32,
    pub edges: Vec<Edge>,
    pub end_ns: u64,
    pub origin_ns: u64,
    pub overrun_ns: Option<u64>,
}

/// A wave sampled at a fixed rate: sample i is the pin state at `i * 1e9 / rate_hz` ns.
#[derive(Clone, Debug, PartialEq)]
pub struct Sampled {
    // [@ANCHOR: gla_sampled_struct]
    pub rate_hz: f64,
    pub data: Vec<u32>,
    /// Pulses that began and ended between two samples: present in the source, invisible at this rate.
    pub missed_pulses: u64,
}

impl Wave {
    /// The pin state at time `t_ns` (the last change at or before it).
    pub fn value_at(&self, t_ns: u64) -> u32 {
        // [@ANCHOR: gla_wave_value_at]
        match self.edges.partition_point(|e| e.t_ns <= t_ns) {
            0 => self.init,
            n => self.edges[n - 1].value,
        }
    }

    /// The pin state just before the change at index `i` (what a clocked receiver sees at that edge).
    pub fn value_before(&self, i: usize) -> u32 {
        // [@ANCHOR: gla_wave_value_before]
        if i == 0 {
            self.init
        } else {
            self.edges[i - 1].value
        }
    }

    /// Index of a pin by name, or by number text ("0", "A0" matching a name "A0"), or by position.
    pub fn pin_index(&self, pin: &Value) -> Result<usize, String> {
        // [@ANCHOR: gla_wave_pin_index]
        if let Some(n) = pin.as_u64() {
            return if (n as usize) < self.names.len() {
                Ok(n as usize)
            } else {
                Err(format!("pin {n} is outside 0..{}", self.names.len()))
            };
        }
        let s = pin
            .as_str()
            .ok_or("a pin is a name such as \"A0\" or its position as a number")?;
        if let Some(i) = self.names.iter().position(|n| n == s) {
            return Ok(i);
        }
        Err(format!(
            "no captured pin named {s:?}; the pins are {}",
            self.names.join(", ")
        ))
    }
}

/// Parses the VCD text of the analyzer applet (1-bit wires, one scope). `x` on a wire is the applet's overrun marker.
pub fn parse_vcd(text: &str) -> Result<Wave, String> {
    // [@ANCHOR: gla_parse_vcd]
    let mut ids: Vec<(String, usize)> = vec![];
    let mut names: Vec<String> = vec![];
    let mut scale_ns = 1.0f64;
    let (mut in_defs, mut t, mut cur, mut init) = (true, 0u64, 0u32, 0u32);
    let mut edges: Vec<Edge> = vec![];
    let mut overrun = None;
    let mut toks = text.split_whitespace().peekable();
    while let Some(tok) = toks.next() {
        if in_defs {
            match tok {
                "$timescale" => {
                    let mut body = String::new();
                    for x in toks.by_ref() {
                        if x == "$end" {
                            break;
                        }
                        body.push_str(x);
                    }
                    let digits: String = body.chars().take_while(|c| c.is_ascii_digit()).collect();
                    let unit = &body[digits.len()..];
                    let mult = match unit {
                        "ns" => 1.0,
                        "ps" => 0.001,
                        "us" => 1e3,
                        "ms" => 1e6,
                        "s" => 1e9,
                        o => {
                            return Err(format!(
                                "VCD timescale unit {o:?} is not s, ms, us, ns or ps"
                            ))
                        }
                    };
                    scale_ns = digits
                        .parse::<f64>()
                        .map_err(|_| "VCD timescale has no number")?
                        * mult;
                }
                "$var" => {
                    let f: Vec<&str> = toks.by_ref().take_while(|x| *x != "$end").collect();
                    if f.len() < 4 {
                        return Err("a $var line is too short".into());
                    }
                    if f[1] != "1" {
                        return Err(format!(
                            "wire {} is {} bits wide; only 1-bit wires are supported",
                            f[3], f[1]
                        ));
                    }
                    if names.len() >= MAX_PINS {
                        return Err(format!("more than {MAX_PINS} wires"));
                    }
                    ids.push((f[2].to_string(), names.len()));
                    names.push(f[3].to_string());
                }
                "$enddefinitions" => {
                    for x in toks.by_ref() {
                        if x == "$end" {
                            break;
                        }
                    }
                    in_defs = false;
                }
                _ => {}
            }
            continue;
        }
        if tok.starts_with('$') {
            if matches!(
                tok,
                "$dumpvars" | "$end" | "$dumpall" | "$dumpon" | "$dumpoff"
            ) {
                continue;
            }
            for x in toks.by_ref() {
                if x == "$end" {
                    break;
                }
            }
            continue;
        }
        if let Some(ts) = tok.strip_prefix('#') {
            let v: u64 = ts.parse().map_err(|_| format!("bad VCD time {tok:?}"))?;
            let nt = (v as f64 * scale_ns).round() as u64;
            if nt < t {
                return Err(format!("VCD time goes backwards at {tok}"));
            }
            t = nt;
            continue;
        }
        // A token that starts with a multi-byte character is not a value change; `split_at(1)` would panic inside it.
        let Some((val, id)) = tok.split_at_checked(1) else {
            return Err(format!("VCD change {tok:?} does not start with a value"));
        };
        let Some(&(_, bit)) = ids.iter().find(|(i, _)| i == id) else {
            return Err(format!("VCD change {tok:?} names an unknown id"));
        };
        let level = match val {
            "0" => 0u32,
            "1" => 1,
            "x" | "X" | "z" | "Z" => {
                if overrun.is_none() {
                    overrun = Some(t);
                }
                continue;
            }
            _ => return Err(format!("VCD value {tok:?} is not 0, 1 or x")),
        };
        let next = (cur & !(1 << bit)) | (level << bit);
        if t == 0 && edges.is_empty() {
            // values at time 0 (the $dumpvars block) are the initial state
            init = next;
            cur = next;
            continue;
        }
        if next == cur {
            continue;
        }
        cur = next;
        match edges.last_mut() {
            Some(last) if last.t_ns == t => last.value = cur,
            _ => edges.push(Edge {
                t_ns: t,
                value: cur,
            }),
        }
    }
    if names.is_empty() {
        return Err("the VCD declares no wires".into());
    }
    let end_ns = edges.last().map(|e| e.t_ns).unwrap_or(0).max(t);
    // A change that returns the pins to the previous state within one timestamp leaves a no-op entry: drop those.
    let mut prev = init;
    edges.retain(|e| {
        let keep = e.value != prev;
        prev = e.value;
        keep
    });
    Ok(Wave {
        names,
        init,
        edges,
        end_ns,
        origin_ns: 0,
        overrun_ns: overrun,
    })
}

/// The wave as VCD text (1 ns timescale, scope `glasgow`), the format GTKWave, PulseView and the Glasgow's own tools read.
pub fn write_vcd(w: &Wave) -> String {
    // [@ANCHOR: gla_write_vcd]
    let id = |i: usize| ((b'!' + i as u8) as char).to_string();
    let mut s = String::from("$timescale 1 ns $end\n$scope module glasgow $end\n");
    for (i, n) in w.names.iter().enumerate() {
        s.push_str(&format!("$var wire 1 {} {} $end\n", id(i), n));
    }
    s.push_str("$upscope $end\n$enddefinitions $end\n#0\n$dumpvars\n");
    for i in 0..w.names.len() {
        s.push_str(&format!("{}{}\n", (w.init >> i) & 1, id(i)));
    }
    s.push_str("$end\n");
    let mut prev = w.init;
    for e in &w.edges {
        s.push_str(&format!("#{}\n", e.t_ns));
        for i in 0..w.names.len() {
            if ((e.value ^ prev) >> i) & 1 == 1 {
                s.push_str(&format!("{}{}\n", (e.value >> i) & 1, id(i)));
            }
        }
        prev = e.value;
    }
    if w.end_ns > w.edges.last().map(|e| e.t_ns).unwrap_or(0) {
        s.push_str(&format!("#{}\n", w.end_ns));
    }
    s
}

/// The wave as CSV: a header `time_ns,<pin names>` and one row per change (the first row is the state at 0).
pub fn write_csv(w: &Wave) -> String {
    // [@ANCHOR: gla_write_csv]
    let row = |t: u64, v: u32| -> String {
        let mut s = t.to_string();
        for i in 0..w.names.len() {
            s.push(',');
            s.push(if (v >> i) & 1 == 1 { '1' } else { '0' });
        }
        s.push('\n');
        s
    };
    let mut s = format!("time_ns,{}\n", w.names.join(","));
    s.push_str(&row(0, w.init));
    for e in &w.edges {
        s.push_str(&row(e.t_ns, e.value));
    }
    if w.end_ns > w.edges.last().map(|e| e.t_ns).unwrap_or(0) {
        s.push_str(&row(w.end_ns, w.value_at(w.end_ns)));
    }
    s
}

/// The part of the wave from `from_ns` to `to_ns` (clamped to the capture), re-based so that time 0 is `from_ns`. The window's initial state is the state just before `from_ns`, so a change at exactly `from_ns` is kept (as a change at time 0).
pub fn window(w: &Wave, from_ns: u64, to_ns: u64) -> Wave {
    // [@ANCHOR: gla_window]
    let from = from_ns.min(w.end_ns);
    let to = to_ns.clamp(from, w.end_ns);
    // the state just before `from`, so a change exactly at `from` (a trigger edge) is the window's first change
    let init = match w.edges.partition_point(|e| e.t_ns < from) {
        0 => w.init,
        n => w.edges[n - 1].value,
    };
    let edges: Vec<Edge> = w
        .edges
        .iter()
        .filter(|e| e.t_ns >= from && e.t_ns <= to)
        .map(|e| Edge {
            t_ns: e.t_ns - from,
            value: e.value,
        })
        .collect();
    Wave {
        names: w.names.clone(),
        init,
        edges,
        end_ns: to - from,
        origin_ns: w.origin_ns + from,
        overrun_ns: w
            .overrun_ns
            .filter(|o| *o >= from && *o <= to)
            .map(|o| o - from),
    }
}

/// Largest number of samples one resampled capture may hold (memory and file size bound).
pub const MAX_SAMPLES: u64 = 8_000_000;

/// Samples the wave at `rate_hz` (1 to 48 MHz, the analyzer's own timestamp clock) and counts the pulses that fall between samples.
pub fn resample(w: &Wave, rate_hz: f64) -> Result<Sampled, String> {
    // [@ANCHOR: gla_resample]
    if !(rate_hz.is_finite() && (1.0..=48e6).contains(&rate_hz)) {
        return Err(format!("sample_rate_hz {rate_hz} must be 1 to 48000000 (the analyzer's timestamp clock is 48 MHz)"));
    }
    let period = 1e9 / rate_hz;
    let n = (w.end_ns as f64 / period).floor() as u64 + 1;
    if n > MAX_SAMPLES {
        return Err(format!("{n} samples at {rate_hz} Hz exceeds {MAX_SAMPLES}: lower sample_rate_hz or shorten the window"));
    }
    let mut data = Vec::with_capacity(n as usize);
    let mut ei = 0usize;
    let mut cur = w.init;
    for i in 0..n {
        let t = (i as f64 * period).round() as u64;
        while ei < w.edges.len() && w.edges[ei].t_ns <= t {
            cur = w.edges[ei].value;
            ei += 1;
        }
        data.push(cur);
    }
    // A pulse is the span between two consecutive changes of one pin; it is missed when no sample instant falls inside it.
    let mut missed = 0u64;
    for pin in 0..w.names.len() {
        let mut last: Option<u64> = None;
        let mut prev_v = (w.init >> pin) & 1;
        for e in &w.edges {
            let v = (e.value >> pin) & 1;
            if v == prev_v {
                continue;
            }
            prev_v = v;
            if let Some(t0) = last {
                let i = (t0 as f64 / period).ceil();
                if ((i * period).round() as u64) >= e.t_ns {
                    missed += 1;
                }
            }
            last = Some(e.t_ns);
        }
    }
    Ok(Sampled {
        rate_hz,
        data,
        missed_pulses: missed,
    })
}

/// Summary fields common to every file written: pins, duration, number of changes, origin.
pub fn describe(w: &Wave) -> Value {
    // [@ANCHOR: gla_describe]
    json!({"pins": w.names, "duration_ns": w.end_ns, "changes": w.edges.len(), "origin_ns": w.origin_ns, "overrun_at_ns": w.overrun_ns})
}
