// SPDX-License-Identifier: AGPL-3.0-or-later
//! The instrument list, read from the JSON file named by `BENCH_MCP_CONFIG`:
//!
//! ```json
//! {"instruments": {
//!   "scope": {"driver": "siglent_sds", "tier": "read_config", "host": "192.168.10.51", "port": 5025},
//!   "psu":   {"driver": "atm6301", "tier": "read_config", "serial": "/dev/ttyUSB0", "baud": 115200,
//!             "max_volts": 5.5, "max_amps": 1.0}
//! }}
//! ```
//!
//! Each instrument has its own permission tier (`read_only` when omitted).

use serde_json::Value;

#[derive(PartialEq, PartialOrd, Clone, Copy, Debug)]
pub enum Tier {
    ReadOnly = 0,
    ReadConfig = 1,
    Full = 2,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Kind {
    Scope,
    Psu,
    /// The DG8SAQ VNWA vector network analyser, driven directly by the `vnwa_open` crate (USB and its two codecs).
    Vna,
}

#[derive(Clone, Debug)]
pub struct Instrument {
    pub name: String,
    pub kind: Kind,
    pub driver: String,
    pub tier: Tier,
    pub host: Option<String>,
    pub port: u16,
    pub serial: Option<String>,
    pub baud: u32,
    pub max_volts: f64,
    pub max_amps: f64,
    /// VNA only: the settings of `vnwa_open::sweep::Config` (defaults for the other kinds, unused).
    pub vna: vnwa_open::sweep::Config,
}

fn tier_of(s: Option<&str>) -> Result<Tier, String> {
    match s {
        None | Some("read_only") => Ok(Tier::ReadOnly),
        Some("read_config") => Ok(Tier::ReadConfig),
        Some("full") => Ok(Tier::Full),
        Some(o) => Err(format!("unknown tier {o:?} (read_only, read_config, full)")),
    }
}

/// The VNA settings of an instrument entry: `codec_s11`, `codec_s21` (parts of the ALSA ids), `if_hz`, `point_ms`, `rf_level`,
/// `settle_ms`, `window_ms`; anything omitted keeps the default of `vnwa_open::sweep::Config`.
fn vna_config(c: &Value) -> Result<vnwa_open::sweep::Config, String> {
    let mut v = vnwa_open::sweep::Config::default();
    let text = |k: &str| c.get(k).and_then(|x| x.as_str()).map(String::from);
    let num = |k: &str| c.get(k).and_then(|x| x.as_f64());
    if let Some(t) = text("codec_s11") {
        v.codec_s11 = t;
    }
    if let Some(t) = text("codec_s21") {
        v.codec_s21 = t;
    }
    if let Some(x) = num("if_hz") {
        v.if_hz = x;
    }
    if let Some(x) = num("point_ms") {
        v.point_ms = x;
    }
    v.settle_ms = num("settle_ms");
    v.window_ms = num("window_ms");
    if let Some(x) = c.get("rf_level").and_then(|x| x.as_u64()) {
        v.rf_level = u16::try_from(x)
            .map_err(|_| "rf_level is above the 14-bit maximum 16383".to_string())?;
    }
    vnwa_open::sweep::check(&v)?;
    Ok(v)
}

pub fn parse(text: &str) -> Result<Vec<Instrument>, String> {
    // [@ANCHOR: bench_mcp_config]
    let v: Value = serde_json::from_str(text).map_err(|e| format!("config: {e}"))?;
    let map = v
        .get("instruments")
        .and_then(|i| i.as_object())
        .ok_or("config: no \"instruments\" object")?;
    let mut out = Vec::new();
    for (name, c) in map {
        let driver = c
            .get("driver")
            .and_then(|d| d.as_str())
            .ok_or(format!("{name}: no driver"))?
            .to_string();
        let kind = match driver.as_str() {
            "siglent_sds" => Kind::Scope,
            "atm6301" => Kind::Psu,
            "vnwa_open" => Kind::Vna,
            o => {
                return Err(format!(
                    "{name}: unknown driver {o:?} (siglent_sds, atm6301, vnwa_open)"
                ))
            }
        };
        let inst = Instrument {
            name: name.clone(),
            kind,
            driver,
            tier: tier_of(c.get("tier").and_then(|t| t.as_str()))
                .map_err(|e| format!("{name}: {e}"))?,
            host: c.get("host").and_then(|h| h.as_str()).map(String::from),
            port: c.get("port").and_then(|p| p.as_u64()).unwrap_or(5025) as u16,
            serial: c.get("serial").and_then(|s| s.as_str()).map(String::from),
            baud: c.get("baud").and_then(|b| b.as_u64()).unwrap_or(115200) as u32,
            max_volts: c.get("max_volts").and_then(|m| m.as_f64()).unwrap_or(5.5),
            max_amps: c.get("max_amps").and_then(|m| m.as_f64()).unwrap_or(1.0),
            vna: vna_config(c).map_err(|e| format!("{name}: {e}"))?,
        };
        match (inst.kind, &inst.host, &inst.serial) {
            (Kind::Scope, None, _) => return Err(format!("{name}: a scope needs \"host\"")),
            (Kind::Psu, _, None) => return Err(format!("{name}: a supply needs \"serial\"")),
            _ => {}
        }
        if !(inst.max_volts.is_finite()
            && inst.max_volts > 0.0
            && inst.max_amps.is_finite()
            && inst.max_amps > 0.0)
        {
            return Err(format!(
                "{name}: max_volts and max_amps must be positive numbers"
            ));
        }
        out.push(inst);
    }
    if out.is_empty() {
        return Err("config: no instruments".into());
    }
    Ok(out)
}

pub fn load() -> Result<Vec<Instrument>, String> {
    let path =
        std::env::var("BENCH_MCP_CONFIG").map_err(|_| "BENCH_MCP_CONFIG is not set".to_string())?;
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{path}: {e}"))?;
    parse(&text)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Tests [@ANCHOR: bench_mcp_config]
    #[test]
    fn parses_both_drivers_with_defaults() {
        let v = parse(
            r#"{"instruments":{"scope":{"driver":"siglent_sds","host":"10.0.0.2"},
            "psu":{"driver":"atm6301","tier":"full","serial":"/dev/ttyUSB0","max_volts":5.5}}}"#,
        )
        .unwrap();
        let s = v.iter().find(|i| i.name == "scope").unwrap();
        assert_eq!(
            (s.kind, s.tier, s.port),
            (Kind::Scope, Tier::ReadOnly, 5025)
        );
        let p = v.iter().find(|i| i.name == "psu").unwrap();
        assert_eq!(
            (p.kind, p.tier, p.baud, p.max_amps),
            (Kind::Psu, Tier::Full, 115200, 1.0)
        );
    }

    #[test]
    fn a_vnwa_instrument_takes_its_settings_with_defaults_and_refuses_bad_ones() {
        let v = parse(
            r#"{"instruments":{"vna":{"driver":"vnwa_open","tier":"read_config","if_hz":4000,"point_ms":30}}}"#,
        )
        .unwrap();
        assert_eq!(v[0].kind, Kind::Vna);
        assert_eq!(
            (v[0].vna.if_hz, v[0].vna.point_ms, v[0].vna.rf_level),
            (4000.0, 30.0, 0x3fff)
        );
        let bad = |extra: &str| {
            parse(&format!(
                r#"{{"instruments":{{"vna":{{"driver":"vnwa_open"{extra}}}}}}}"#
            ))
            .is_err()
        };
        assert!(
            bad(r#","if_hz":7000"#),
            "48 kHz must hold a whole number of IF periods"
        );
        assert!(bad(r#","point_ms":500"#));
        assert!(bad(r#","rf_level":20000"#));
        assert!(bad(r#","settle_ms":250,"window_ms":100"#));
        assert!(
            bad(r#","point_ms":10,"window_ms":0.05"#),
            "a window shorter than one IF period"
        );
    }

    #[test]
    fn rejects_bad_configs() {
        assert!(parse(r#"{"instruments":{}}"#).is_err());
        assert!(parse(r#"{"instruments":{"x":{"driver":"nope"}}}"#).is_err());
        assert!(parse(r#"{"instruments":{"x":{"driver":"atm6301"}}}"#).is_err());
        assert!(parse(
            r#"{"instruments":{"x":{"driver":"atm6301","serial":"/dev/x","max_volts":-1}}}"#
        )
        .is_err());
        assert!(parse(
            r#"{"instruments":{"x":{"driver":"atm6301","serial":"/dev/x","tier":"root"}}}"#
        )
        .is_err());
    }
}
