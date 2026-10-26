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
}

fn tier_of(s: Option<&str>) -> Result<Tier, String> {
    match s {
        None | Some("read_only") => Ok(Tier::ReadOnly),
        Some("read_config") => Ok(Tier::ReadConfig),
        Some("full") => Ok(Tier::Full),
        Some(o) => Err(format!("unknown tier {o:?} (read_only, read_config, full)")),
    }
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
            o => {
                return Err(format!(
                    "{name}: unknown driver {o:?} (siglent_sds, atm6301)"
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
