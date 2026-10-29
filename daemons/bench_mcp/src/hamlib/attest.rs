// SPDX-License-Identifier: AGPL-3.0-or-later
//! The owner's transmit attestation and the settings of the load plausibility check, read from the rig's config entry.
//!
//! A rig can transmit through these tools only when its entry has a `transmit` block. The block is the owner's statement that the
//! rig's antenna connector is on a dummy load, with their name and the date, and the limits they accept. The software cannot see
//! what is connected: the attestation and these limits are the control, and the load check (`load.rs`) is evidence on top of them.
//!
//! ```json
//! "transmit": {"dummy_load": true, "attested_by": "K6BP", "attested_on": "2026-10-29",
//!              "cabling": "50 ohm dummy load on the rig's antenna port, 10 m of RG-58, no tuner",
//!              "max_power_w": 5, "max_seconds": 10,
//!              "frequency_ranges": [[7000000, 7010000]], "allowed_modes": ["CW", "USB"],
//!              "max_swr": 2.0, "swr_probe": true, "allow_lowering_power": true,
//!              "load_check": {"margin_db": 6, "max_spread_db": 6}}
//! ```
//!
//! Compliance with the licence terms for any transmission, even into a dummy load, is the owner's responsibility: the frequency
//! ranges are the owner's list of frequencies they may use for this.

use super::control::MODES;
use super::stations::{days, now_local};
use serde_json::Value;

/// Hard limits that no configuration can raise.
pub const HARD_MAX_SECONDS: f64 = 60.0;
pub const HARD_MAX_POWER_W: f64 = 200.0;

/// Settings of the load plausibility check; the defaults are the safe ones.
#[derive(Clone, Debug, PartialEq)]
pub struct LoadCheck {
    /// A reading this many dB above the baseline (after the spread allowance) is a signal.
    pub margin_db: f64,
    /// The same for the audio level.
    pub audio_margin_db: f64,
    /// Readings that fluctuate more than this many dB at one frequency cannot show a difference.
    pub max_spread_db: f64,
    /// S-meter readings per frequency.
    pub readings: usize,
    pub dwell_ms: f64,
    /// Reference frequencies listened to besides the transmit frequency.
    pub references: usize,
    /// Audio gain (AF level, 0 to 1) set during the check, so that audio levels compare.
    pub af: f64,
    /// Preamp setting (dB) used during the check, if the rig has one.
    pub preamp_db: Option<f64>,
}

impl Default for LoadCheck {
    fn default() -> LoadCheck {
        LoadCheck {
            margin_db: 6.0,
            audio_margin_db: 6.0,
            max_spread_db: 6.0,
            readings: 5,
            dwell_ms: 150.0,
            references: 6,
            af: 0.5,
            preamp_db: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Transmit {
    pub attested_by: String,
    pub attested_on: String,
    pub cabling: String,
    pub max_power_w: f64,
    pub max_seconds: f64,
    pub frequency_ranges: Vec<(f64, f64)>,
    pub allowed_modes: Vec<String>,
    /// A standing-wave ratio above this at the low-power probe refuses the test.
    pub max_swr: f64,
    pub swr_probe: bool,
    pub swr_probe_s: f64,
    /// May a tool lower the rig's power setting to the requested level (the old setting is restored)?
    pub allow_lowering_power: bool,
    pub load: LoadCheck,
}

fn number(c: &Value, key: &str, lo: f64, hi: f64) -> Result<Option<f64>, String> {
    // [@ANCHOR: hamlib_attest_number]
    match c.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v
            .as_f64()
            .filter(|x| x.is_finite() && *x >= lo && *x <= hi)
            .map(Some)
            .ok_or_else(|| format!("{key} must be a number from {lo} to {hi}")),
    }
}

/// Reads a `transmit` block. Everything the safe operation needs is required; nothing is defaulted that widens what may be done.
pub fn parse_transmit(c: &Value) -> Result<Transmit, String> {
    // [@ANCHOR: hamlib_attest_config]
    let obj = c.as_object().ok_or("transmit must be an object")?;
    if c.get("dummy_load").and_then(|v| v.as_bool()) != Some(true) {
        return Err("transmit.dummy_load must be true: the owner attests that the rig's antenna port is on a dummy load".into());
    }
    let text = |k: &str, max: usize| -> Result<String, String> {
        let t = c
            .get(k)
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .ok_or_else(|| format!("transmit.{k} is required (text)"))?;
        if t.len() > max || t.chars().any(char::is_control) {
            return Err(format!(
                "transmit.{k} must be at most {max} characters without control characters"
            ));
        }
        Ok(t.to_string())
    };
    let attested_by = text("attested_by", 80)?;
    let attested_on = text("attested_on", 10)?;
    let cabling = text("cabling", 300)?;
    let on = days(&attested_on).ok_or("transmit.attested_on must be a date like 2026-10-29")?;
    let today = days(&now_local().0).unwrap_or(i64::MAX);
    if on > today {
        return Err("transmit.attested_on is in the future".into());
    }
    let max_power_w = number(c, "max_power_w", 0.01, HARD_MAX_POWER_W)?
        .ok_or("transmit.max_power_w is required (watts, above 0)")?;
    let max_seconds = number(c, "max_seconds", 0.1, HARD_MAX_SECONDS)?
        .ok_or("transmit.max_seconds is required (seconds, up to 60)")?;
    let mut frequency_ranges = vec![];
    for pair in c.get("frequency_ranges").and_then(|r| r.as_array()).filter(|r| !r.is_empty()).ok_or("transmit.frequency_ranges is required: the list of [low, high] Hz ranges the owner may use; there is no default")? {
        let p = pair.as_array().filter(|p| p.len() == 2).ok_or("each transmit.frequency_ranges entry is [low, high]")?;
        let (lo, hi) = (p[0].as_f64().unwrap_or(f64::NAN), p[1].as_f64().unwrap_or(f64::NAN));
        if !(lo.is_finite() && hi.is_finite() && lo >= 10_000.0 && hi >= lo && hi <= 6.0e9) {
            return Err("transmit.frequency_ranges entries need 10 kHz <= low <= high <= 6 GHz".into());
        }
        frequency_ranges.push((lo, hi));
    }
    let mut allowed_modes = vec![];
    for m in c
        .get("allowed_modes")
        .and_then(|m| m.as_array())
        .filter(|m| !m.is_empty())
        .ok_or("transmit.allowed_modes is required: a list of Hamlib mode names")?
    {
        let m = m
            .as_str()
            .map(str::to_ascii_uppercase)
            .filter(|m| MODES.contains(&m.as_str()))
            .ok_or("transmit.allowed_modes has a name that is not a Hamlib mode")?;
        allowed_modes.push(m);
    }
    let mut load = LoadCheck::default();
    if let Some(l) = c.get("load_check") {
        if !l.is_object() {
            return Err("transmit.load_check must be an object".into());
        }
        load.margin_db = number(l, "margin_db", 1.0, 30.0)?.unwrap_or(load.margin_db);
        load.audio_margin_db =
            number(l, "audio_margin_db", 1.0, 30.0)?.unwrap_or(load.audio_margin_db);
        load.max_spread_db = number(l, "max_spread_db", 1.0, 30.0)?.unwrap_or(load.max_spread_db);
        load.readings = number(l, "readings", 3.0, 10.0)?.map_or(load.readings, |x| x as usize);
        load.dwell_ms = number(l, "dwell_ms", 50.0, 2000.0)?.unwrap_or(load.dwell_ms);
        load.references =
            number(l, "references", 2.0, 12.0)?.map_or(load.references, |x| x as usize);
        load.af = number(l, "af", 0.05, 1.0)?.unwrap_or(load.af);
        load.preamp_db = number(l, "preamp_db", 0.0, 60.0)?;
    }
    let known = [
        "dummy_load",
        "attested_by",
        "attested_on",
        "cabling",
        "max_power_w",
        "max_seconds",
        "frequency_ranges",
        "allowed_modes",
        "max_swr",
        "swr_probe",
        "swr_probe_s",
        "allow_lowering_power",
        "load_check",
    ];
    if let Some(k) = obj.keys().find(|k| !known.contains(&k.as_str())) {
        return Err(format!(
            "transmit.{k} is not a known key ({})",
            known.join(", ")
        ));
    }
    Ok(Transmit {
        attested_by,
        attested_on,
        cabling,
        max_power_w,
        max_seconds,
        frequency_ranges,
        allowed_modes,
        max_swr: number(c, "max_swr", 1.0, 5.0)?.unwrap_or(2.0),
        swr_probe: c.get("swr_probe").map_or(Ok(true), |v| {
            v.as_bool()
                .ok_or("transmit.swr_probe must be true or false")
        })?,
        swr_probe_s: number(c, "swr_probe_s", 0.1, 2.0)?.unwrap_or(0.5),
        allow_lowering_power: c.get("allow_lowering_power").map_or(Ok(false), |v| {
            v.as_bool()
                .ok_or("transmit.allow_lowering_power must be true or false")
        })?,
        load,
    })
}

impl Transmit {
    /// The index of the owner's frequency range that holds `hz`.
    pub fn range_of(&self, hz: f64) -> Option<usize> {
        self.frequency_ranges
            .iter()
            .position(|(lo, hi)| hz >= *lo && hz <= *hi)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    pub fn good() -> Value {
        json!({"dummy_load": true, "attested_by": "K6BP", "attested_on": "2026-10-29", "cabling": "50 ohm dummy load on the antenna port",
            "max_power_w": 5, "max_seconds": 10, "frequency_ranges": [[7000000, 7010000]], "allowed_modes": ["cw", "USB"]})
    }

    // Tests [@ANCHOR: hamlib_attest_config]
    // Tests [@ANCHOR: hamlib_attest_number]
    #[test]
    fn a_transmit_block_needs_the_attestation_and_every_limit_and_nothing_defaults_wider() {
        let t = parse_transmit(&good()).unwrap();
        assert_eq!((t.max_power_w, t.max_seconds, t.max_swr), (5.0, 10.0, 2.0));
        assert_eq!(t.allowed_modes, ["CW", "USB"]);
        assert!(
            t.swr_probe && !t.allow_lowering_power,
            "the probe is on and lowering power is off unless the owner says otherwise"
        );
        assert_eq!(t.range_of(7.005e6), Some(0));
        assert_eq!(t.range_of(14.0e6), None);
        let without = |k: &str| {
            let mut v = good();
            v.as_object_mut().unwrap().remove(k);
            parse_transmit(&v).unwrap_err()
        };
        for k in [
            "dummy_load",
            "attested_by",
            "attested_on",
            "cabling",
            "max_power_w",
            "max_seconds",
            "frequency_ranges",
            "allowed_modes",
        ] {
            assert!(without(k).contains(k), "{k}");
        }
        let with = |k: &str, v: Value| {
            let mut g = good();
            g[k] = v;
            parse_transmit(&g)
        };
        for (k, v) in [
            ("dummy_load", json!(false)),
            ("dummy_load", json!("yes")),
            ("attested_on", json!("2999-01-01")),
            ("attested_on", json!("yesterday")),
            ("max_power_w", json!(0)),
            ("max_power_w", json!(500)),
            ("max_seconds", json!(61)),
            ("max_seconds", json!(0)),
            ("frequency_ranges", json!([])),
            ("frequency_ranges", json!([[7e6]])),
            ("frequency_ranges", json!([[100, 200]])),
            ("allowed_modes", json!(["BOGUS"])),
            ("allowed_modes", json!([])),
            ("max_swr", json!(9)),
            ("swr_probe", json!("on")),
            ("cabling", json!("x\ny")),
            ("surprise", json!(1)),
            ("load_check", json!({"margin_db": 0})),
            ("load_check", json!({"readings": 1})),
        ] {
            assert!(with(k, v.clone()).is_err(), "{k} {v}");
        }
        assert!(with(
            "load_check",
            json!({"margin_db": 8, "references": 4, "preamp_db": 10})
        )
        .is_ok());
    }
}
