// SPDX-License-Identifier: AGPL-3.0-or-later
//! Triggers on a captured waveform. The Glasgow revision C analyzer has no hardware trigger: the tools capture continuously and
//! find the trigger afterwards, so the "pre-trigger buffer" is the part of the capture before the trigger. Entries:
//! `docs/GLASGOW_LOGIC_TOOLS.md`.

use super::wave::{window, Wave};
use serde_json::Value;

// [@ANCHOR: gla_trigger_enum]
/// What starts the window.
#[derive(Clone, Debug, PartialEq)]
pub enum Trigger {
    /// The start of the capture.
    Immediate,
    /// A change of one pin: `rising` true, false, or either (None).
    Edge { pin: usize, rising: Option<bool> },
    /// One pin at `level` for at least `min_ns` (0 = the moment it gets there).
    Level {
        pin: usize,
        level: bool,
        min_ns: u64,
    },
    /// `value & mask` equals `want` over the masked pins for at least `min_ns`.
    Pattern { mask: u32, want: u32, min_ns: u64 },
}

/// Reads the `trigger` argument: `{"type":"edge","pin":"A0","edge":"rise|fall|either"}`, `{"type":"level","pin":"A0","level":1,"min_ns":100}`,
/// `{"type":"pattern","pattern":"1x0x","min_ns":0}` (one character per captured pin, first pin first: 0, 1 or x for don't care), or none.
pub fn parse(w: &Wave, v: Option<&Value>) -> Result<Trigger, String> {
    // [@ANCHOR: gla_trigger_parse]
    let Some(v) = v.filter(|v| !v.is_null()) else {
        return Ok(Trigger::Immediate);
    };
    let min_ns = v.get("min_ns").and_then(|m| m.as_u64()).unwrap_or(0);
    match v.get("type").and_then(|t| t.as_str()) {
        Some("none") => Ok(Trigger::Immediate),
        Some("edge") => {
            let pin = w.pin_index(v.get("pin").ok_or("an edge trigger needs \"pin\"")?)?;
            let rising = match v.get("edge").and_then(|e| e.as_str()).unwrap_or("either") {
                "rise" | "rising" => Some(true),
                "fall" | "falling" => Some(false),
                "either" => None,
                o => return Err(format!("edge {o:?} must be rise, fall or either")),
            };
            Ok(Trigger::Edge { pin, rising })
        }
        Some("level") => {
            let pin = w.pin_index(v.get("pin").ok_or("a level trigger needs \"pin\"")?)?;
            let level = match v.get("level") {
                Some(Value::Bool(b)) => *b,
                Some(x) if x.as_u64() == Some(1) => true,
                Some(x) if x.as_u64() == Some(0) => false,
                _ => return Err("level must be 0 or 1".into()),
            };
            Ok(Trigger::Level { pin, level, min_ns })
        }
        Some("pattern") => {
            let p = v
                .get("pattern")
                .and_then(|p| p.as_str())
                .ok_or("a pattern trigger needs \"pattern\" such as \"1x0x\"")?;
            if p.chars().count() != w.names.len() {
                return Err(format!(
                    "the pattern has {} characters; the capture has {} pins ({})",
                    p.chars().count(),
                    w.names.len(),
                    w.names.join(", ")
                ));
            }
            let (mut mask, mut want) = (0u32, 0u32);
            for (i, c) in p.chars().enumerate() {
                match c {
                    '0' => mask |= 1 << i,
                    '1' => {
                        mask |= 1 << i;
                        want |= 1 << i;
                    }
                    'x' | 'X' => {}
                    o => return Err(format!("pattern character {o:?} must be 0, 1 or x")),
                }
            }
            if mask == 0 {
                return Err("a pattern of only x never triggers on anything: use type none".into());
            }
            Ok(Trigger::Pattern { mask, want, min_ns })
        }
        o => Err(format!(
            "trigger type {o:?} must be none, edge, level or pattern"
        )),
    }
}

/// The time of the first trigger at or after `from_ns`, or None. Level and pattern triggers with `min_ns` fire when the condition has
/// held that long (the time returned is when it began, so the pre-trigger part ends where the condition started).
pub fn find(w: &Wave, t: &Trigger, from_ns: u64) -> Option<u64> {
    // [@ANCHOR: gla_trigger_find]
    match *t {
        Trigger::Immediate => Some(from_ns),
        Trigger::Edge { pin, rising } => {
            for (i, e) in w.edges.iter().enumerate() {
                if e.t_ns < from_ns {
                    continue;
                }
                let (a, b) = ((w.value_before(i) >> pin) & 1, (e.value >> pin) & 1);
                if a != b && rising.is_none_or(|r| r == (b == 1)) {
                    return Some(e.t_ns);
                }
            }
            None
        }
        Trigger::Level { pin, level, min_ns } => {
            find_state(w, from_ns, min_ns, |v| ((v >> pin) & 1 == 1) == level)
        }
        Trigger::Pattern { mask, want, min_ns } => {
            find_state(w, from_ns, min_ns, |v| v & mask == want)
        }
    }
}

/// The start of the first span at or after `from_ns` over which `ok` holds for `min_ns` (and, with min_ns 0, any instant it holds).
fn find_state(w: &Wave, from_ns: u64, min_ns: u64, ok: impl Fn(u32) -> bool) -> Option<u64> {
    // [@ANCHOR: gla_trigger_find_state]
    let mut start = from_ns;
    let mut i = w.edges.partition_point(|e| e.t_ns <= from_ns);
    let mut v = w.value_at(from_ns);
    loop {
        if ok(v) {
            let end = w.edges.get(i).map(|e| e.t_ns).unwrap_or(w.end_ns);
            if end.saturating_sub(start) >= min_ns
                && (min_ns > 0 || end > start || i >= w.edges.len())
            {
                return Some(start);
            }
        }
        let e = w.edges.get(i)?;
        start = e.t_ns;
        v = e.value;
        i += 1;
    }
}

/// The window around the first trigger: `pre_ns` before it, `post_ns` after it (clamped to the capture). Returns the window and the
/// trigger time in the window's own time, or an error naming that no trigger occurred.
pub fn apply(w: &Wave, t: &Trigger, pre_ns: u64, post_ns: u64) -> Result<(Wave, u64), String> {
    // [@ANCHOR: gla_trigger_apply]
    let at = find(w, t, 0).ok_or_else(|| {
        format!(
            "the trigger {t:?} did not occur in the {} ns capture",
            w.end_ns
        )
    })?;
    let from = at.saturating_sub(pre_ns);
    let win = window(w, from, at.saturating_add(post_ns));
    Ok((win, at - from))
}
