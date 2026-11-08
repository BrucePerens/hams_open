// SPDX-License-Identifier: AGPL-3.0-or-later
//! The summary of a capture: per pin the edge counts, frequency, duty cycle, pulse widths and a glitch report. Entries:
//! `docs/GLASGOW_LOGIC_TOOLS.md`.

use super::wave::Wave;
use serde_json::{json, Value};

/// The most glitches listed per pin (the count is always complete).
const GLITCH_LIST: usize = 16;

/// Summarises every pin of the wave. A pulse is the span between two consecutive changes of one pin, so the first and last spans (cut
/// by the capture's ends) are not pulses. A pulse shorter than `glitch_ns` is reported as a glitch. Frequency is from the median
/// rising-edge period (null with fewer than two rising edges), duty cycle from the complete cycles between the first and last rising edge.
pub fn summarize(w: &Wave, glitch_ns: u64) -> Value {
    // [@ANCHOR: gla_summarize]
    let mut pins = serde_json::Map::new();
    for (i, name) in w.names.iter().enumerate() {
        let mut changes: Vec<(u64, bool)> = vec![];
        let mut prev = (w.init >> i) & 1;
        for e in &w.edges {
            let v = (e.value >> i) & 1;
            if v != prev {
                changes.push((e.t_ns, v == 1));
                prev = v;
            }
        }
        let rising: Vec<u64> = changes.iter().filter(|c| c.1).map(|c| c.0).collect();
        let falling = changes.len() - rising.len();
        let mut high_ns = 0u64;
        let (mut last_t, mut level) = (0u64, (w.init & (1 << i)) != 0);
        for c in &changes {
            if level {
                high_ns += c.0 - last_t;
            }
            last_t = c.0;
            level = c.1;
        }
        if level {
            high_ns += w.end_ns - last_t;
        }
        let (mut hi_min, mut hi_max, mut lo_min, mut lo_max): (Option<u64>, u64, Option<u64>, u64) =
            (None, 0, None, 0);
        let mut glitches = vec![];
        let mut glitch_count = 0u64;
        for pair in changes.windows(2) {
            let width = pair[1].0 - pair[0].0;
            if pair[0].1 {
                hi_min = Some(hi_min.map_or(width, |m| m.min(width)));
                hi_max = hi_max.max(width);
            } else {
                lo_min = Some(lo_min.map_or(width, |m| m.min(width)));
                lo_max = lo_max.max(width);
            }
            if width < glitch_ns {
                glitch_count += 1;
                if glitches.len() < GLITCH_LIST {
                    glitches.push(json!({"t_ns": pair[0].0 + w.origin_ns, "level": pair[0].1 as u8, "width_ns": width}));
                }
            }
        }
        let mut periods: Vec<u64> = rising.windows(2).map(|p| p[1] - p[0]).collect();
        periods.sort_unstable();
        let (freq, jitter, duty) = if periods.is_empty() {
            (Value::Null, Value::Null, Value::Null)
        } else {
            let med = periods[periods.len() / 2] as f64;
            let first = rising[0];
            let last = *rising.last().unwrap_or(&first);
            let mut hi_in_cycles = 0u64;
            for pair in changes.windows(2) {
                if pair[0].1 && pair[0].0 >= first && pair[1].0 <= last {
                    hi_in_cycles += pair[1].0 - pair[0].0;
                }
            }
            (
                json!(1e9 / med),
                json!(periods[periods.len() - 1] - periods[0]),
                json!(hi_in_cycles as f64 / (last - first) as f64),
            )
        };
        pins.insert(name.clone(), json!({
            "first_level": (w.init >> i) & 1, "last_level": (w.value_at(w.end_ns) >> i) & 1,
            "edges": changes.len(), "rising": rising.len(), "falling": falling,
            "high_fraction": if w.end_ns > 0 { json!(high_ns as f64 / w.end_ns as f64) } else { Value::Null },
            "frequency_hz": freq, "period_spread_ns": jitter, "duty_cycle": duty,
            "high_pulse_ns": {"min": hi_min, "max": if hi_min.is_some() { json!(hi_max) } else { Value::Null }},
            "low_pulse_ns": {"min": lo_min, "max": if lo_min.is_some() { json!(lo_max) } else { Value::Null }},
            "glitches": {"threshold_ns": glitch_ns, "count": glitch_count, "first": glitches},
        }));
    }
    json!({"duration_ns": w.end_ns, "pins": pins})
}
