// SPDX-License-Identifier: AGPL-3.0-or-later
//! The models Hamlib knows and what one of them can do, read from the output of `rigctl -l` and `rigctl -m <n> -u` (the program is
//! run; Hamlib is not linked). Nothing here needs a rig.

use crate::audio::Run;
use serde_json::{json, Value};

#[derive(Clone, Debug, PartialEq)]
pub struct Model {
    pub number: u32,
    pub manufacturer: String,
    pub name: String,
    pub version: String,
    pub status: String,
}

/// Parses the table of `rigctl -l`. The column positions come from the header line, so a model name with spaces stays whole.
pub fn parse_list(text: &str) -> Result<Vec<Model>, String> {
    // [@ANCHOR: hamlib_models_list]
    let mut lines = text.lines();
    let header = lines
        .find(|l| l.contains("Mfg") && l.contains("Model"))
        .ok_or("rigctl -l gave no table (no header line with Mfg and Model)")?;
    let col = |w: &str| {
        header
            .find(w)
            .ok_or_else(|| format!("rigctl -l header lacks the column {w}"))
    };
    let (c_mfg, c_model, c_ver, c_status, c_macro) = (
        col("Mfg")?,
        col("Model")?,
        col("Version")?,
        col("Status")?,
        col("Macro")?,
    );
    let mut out = Vec::new();
    for l in lines {
        let Some(number) = l
            .split_whitespace()
            .next()
            .and_then(|t| t.parse::<u32>().ok())
        else {
            continue;
        };
        let cut = |a: usize, b: usize| l.get(a..b.min(l.len())).unwrap_or("").trim().to_string();
        out.push(Model {
            number,
            manufacturer: cut(c_mfg, c_model),
            name: cut(c_model, c_ver),
            version: cut(c_ver, c_status),
            status: cut(c_status, c_macro),
        });
    }
    if out.is_empty() {
        return Err("rigctl -l listed no models".into());
    }
    Ok(out)
}

/// What `rigctl -m N -u` says about a model.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Caps {
    pub model_name: String,
    pub manufacturer: String,
    pub status: String,
    pub rig_type: String,
    pub ptt_type: String,
    pub dcd_type: String,
    pub port_type: String,
    /// The `Serial speed:` line of a serial rig, as printed.
    pub serial: String,
    pub get_functions: Vec<String>,
    pub set_functions: Vec<String>,
    pub get_levels: Vec<String>,
    pub set_levels: Vec<String>,
    pub modes: Vec<String>,
    pub vfos: Vec<String>,
    pub preamp_db: Vec<String>,
    pub attenuator_db: Vec<String>,
    pub agc_levels: String,
    pub rx_ranges_hz: Vec<(f64, f64)>,
    /// (low Hz, high Hz, low power W, high power W)
    pub tx_ranges: Vec<(f64, f64, f64, f64)>,
}

fn words(s: &str) -> Vec<String> {
    s.split_whitespace().map(String::from).collect()
}

/// A list of `NAME(range)` entries reduced to the names.
fn names(s: &str) -> Vec<String> {
    s.split_whitespace()
        .map(|t| t.split('(').next().unwrap_or(t).to_string())
        .filter(|t| !t.is_empty())
        .collect()
}

fn hz_range(line: &str) -> Option<(f64, f64)> {
    let (a, b) = line.trim().split_once(" Hz - ")?;
    Some((
        a.trim().parse().ok()?,
        b.trim().strip_suffix(" Hz")?.trim().parse().ok()?,
    ))
}

/// Parses the capability dump of one model.
pub fn parse_caps(text: &str) -> Result<Caps, String> {
    // [@ANCHOR: hamlib_models_caps]
    if !text.contains("Caps dump for model") {
        return Err(format!(
            "rigctl -u gave no capability dump: {}",
            text.lines().next().unwrap_or("empty output")
        ));
    }
    let mut c = Caps::default();
    let mut section = "";
    let mut pending: Option<(f64, f64)> = None;
    for raw in text.lines() {
        let l = raw.trim();
        if let Some((k, v)) = l.split_once(':') {
            let v = v.trim();
            match k {
                "Model name" => c.model_name = v.into(),
                "Mfg name" => c.manufacturer = v.into(),
                "Backend status" => c.status = v.into(),
                "Rig type" => c.rig_type = v.into(),
                "PTT type" => c.ptt_type = v.into(),
                "DCD type" => c.dcd_type = v.into(),
                "Port type" => c.port_type = v.into(),
                "Serial speed" => c.serial = v.into(),
                "Get functions" => c.get_functions = words(v),
                "Set functions" => c.set_functions = words(v),
                "Get level" => c.get_levels = names(v),
                "Set level" => c.set_levels = names(v),
                "Preamp" => c.preamp_db = words(v),
                "Attenuator" => c.attenuator_db = words(v),
                "AGC levels" => c.agc_levels = v.into(),
                "Mode list" if c.modes.is_empty() => c.modes = words(v),
                "VFO list" if c.vfos.is_empty() => c.vfos = words(v),
                "Low power" => {
                    // "5 W, High power: 100 W"
                    let low = v
                        .split_whitespace()
                        .next()
                        .and_then(|t| t.parse::<f64>().ok());
                    let high = v
                        .split("High power:")
                        .nth(1)
                        .and_then(|t| t.split_whitespace().next())
                        .and_then(|t| t.parse::<f64>().ok());
                    if let (Some(r), Some(lo), Some(hi), "tx") = (pending, low, high, section) {
                        c.tx_ranges.push((r.0, r.1, lo, hi));
                        pending = None;
                    }
                }
                _ => {}
            }
        }
        if l.starts_with("TX ranges #") && l.ends_with(':') && !l.contains("status") {
            section = "tx";
        } else if l.starts_with("RX ranges #") && l.ends_with(':') && !l.contains("status") {
            section = "rx";
        } else if l.starts_with("Tuning steps") || l.starts_with("Filters") {
            section = "";
        }
        if let Some(r) = hz_range(l) {
            match section {
                "rx" => c.rx_ranges_hz.push(r),
                "tx" => pending = Some(r),
                _ => {}
            }
        }
    }
    if c.model_name.is_empty() {
        return Err("rigctl -u output has no model name".into());
    }
    Ok(c)
}

/// `bench_hamlib_models`: the list (filtered by `search`, at most `limit`), or the capabilities of one `model`.
pub fn tool(run: &dyn Run, args: &Value) -> Result<Value, String> {
    // [@ANCHOR: hamlib_models_tool]
    if let Some(n) = args.get("model") {
        let n = n
            .as_u64()
            .filter(|n| (1..=1_000_000).contains(n))
            .ok_or("model must be a whole number from 1 to 1000000")?;
        let (ok, text) = run.run("rigctl", &["-m".into(), n.to_string(), "-u".into()])?;
        if !ok && !text.contains("Caps dump") {
            return Err(format!("rigctl does not know model {n}: {}. Next: search the list with bench_hamlib_models and the search argument.", text.lines().next().unwrap_or("no output")));
        }
        let c = parse_caps(&text)?;
        return Ok(json!({
            "model": n, "name": c.model_name, "manufacturer": c.manufacturer, "backend_status": c.status, "rig_type": c.rig_type,
            "ptt_type": c.ptt_type, "dcd_type": c.dcd_type, "port_type": c.port_type, "serial": c.serial,
            "can_read_levels": c.get_levels, "can_set_levels": c.set_levels,
            "can_read_functions": c.get_functions, "can_set_functions": c.set_functions,
            "modes": c.modes, "vfos": c.vfos, "preamp_db": c.preamp_db, "attenuator_db": c.attenuator_db, "agc_levels": c.agc_levels,
            "receive_ranges_hz": c.rx_ranges_hz,
            "transmit_ranges": c.tx_ranges.iter().map(|r| json!({"low_hz": r.0, "high_hz": r.1, "low_power_w": r.2, "high_power_w": r.3})).collect::<Vec<_>>(),
            "note": "This is what the Hamlib backend declares for the model, not what a particular unit does; a rig answers -11 for what its firmware lacks.",
        }));
    }
    let limit = match args.get("limit") {
        None | Some(Value::Null) => 50,
        Some(v) => v
            .as_u64()
            .filter(|n| (1..=500).contains(n))
            .ok_or("limit must be a whole number from 1 to 500")? as usize,
    };
    let (_, text) = run.run("rigctl", &["-l".into()])?;
    let all = parse_list(&text)?;
    let search = args
        .get("search")
        .and_then(|s| s.as_str())
        .map(|s| s.to_ascii_lowercase());
    let hits: Vec<&Model> = all
        .iter()
        .filter(|m| {
            search.as_ref().is_none_or(|s| {
                format!("{} {} {}", m.manufacturer, m.name, m.number)
                    .to_ascii_lowercase()
                    .contains(s.as_str())
            })
        })
        .collect();
    Ok(json!({
        "total_models": all.len(), "matching": hits.len(), "shown": hits.len().min(limit),
        "models": hits.iter().take(limit).map(|m| json!({"model": m.number, "manufacturer": m.manufacturer, "name": m.name, "backend_version": m.version, "status": m.status})).collect::<Vec<_>>(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIST: &str = include_str!("../../tests/hamlib_fixtures/rigctl_list.txt");
    const CAPS: &str = include_str!("../../tests/hamlib_fixtures/rigctl_caps_dummy.txt");

    struct Script(&'static str);
    impl Run for Script {
        fn run(&self, _p: &str, _a: &[String]) -> Result<(bool, String), String> {
            Ok((true, self.0.to_string()))
        }
    }

    // Tests [@ANCHOR: hamlib_models_list]
    #[test]
    fn the_model_table_is_read_by_the_columns_of_its_header() {
        let m = parse_list(LIST).unwrap();
        assert_eq!(
            m[0],
            Model {
                number: 1,
                manufacturer: "Hamlib".into(),
                name: "Dummy".into(),
                version: "20240709.0".into(),
                status: "Stable".into()
            }
        );
        assert_eq!(m[1].name, "NET rigctl");
        assert!(parse_list("nothing").is_err());
    }

    // Tests [@ANCHOR: hamlib_models_caps]
    #[test]
    fn capabilities_name_the_levels_functions_modes_and_ranges() {
        let c = parse_caps(CAPS).unwrap();
        assert_eq!(
            (c.model_name.as_str(), c.manufacturer.as_str()),
            ("Dummy", "Hamlib")
        );
        assert!(
            c.get_levels.contains(&"STRENGTH".to_string())
                && c.set_levels.contains(&"RFPOWER".to_string())
        );
        assert!(
            !c.set_levels.contains(&"STRENGTH".to_string()),
            "STRENGTH is read only"
        );
        assert!(c.get_functions.contains(&"NB".to_string()));
        assert_eq!(
            c.modes,
            ["AM", "CW", "USB", "LSB", "RTTY", "FM", "WFM", "CWR", "RTTYR"]
        );
        assert_eq!(c.rx_ranges_hz, [(150_000.0, 1_500_000_000.0)]);
        assert_eq!(c.tx_ranges, [(150_000.0, 1_500_000_000.0, 5.0, 100.0)]);
        assert!(parse_caps("rigctl: Unknown rig model").is_err());
    }

    // Tests [@ANCHOR: hamlib_models_tool]
    #[test]
    fn the_tool_searches_the_list_and_describes_one_model() {
        let v = tool(&Script(LIST), &json!({"search": "net"})).unwrap();
        assert_eq!(v["matching"], 1);
        assert_eq!(v["models"][0]["model"], 2);
        let v = tool(&Script(CAPS), &json!({"model": 1})).unwrap();
        assert_eq!(v["transmit_ranges"][0]["high_power_w"], 100.0);
        assert!(tool(&Script(CAPS), &json!({"model": 0})).is_err());
        assert!(tool(&Script(LIST), &json!({"limit": 0})).is_err());
    }
}
