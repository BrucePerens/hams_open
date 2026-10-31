// SPDX-License-Identifier: AGPL-3.0-or-later
//! The instrument list, read from the JSON file named by `BENCH_MCP_CONFIG`:
//!
//! ```json
//! {"instruments": {
//!   "scope": {"driver": "siglent_sds", "tier": "read_config", "host": "192.168.10.51", "port": 5025},
//!   "psu":   {"driver": "atm6301", "tier": "read_config", "serial": "/dev/ttyUSB0", "baud": 115200,
//!             "max_volts": 5.5, "max_amps": 1.0, "max_on_s": 600}
//! }}
//! ```
//!
//! Each instrument has its own permission tier (`read_only` when omitted).

use serde_json::Value;

/// Every `driver` name an instrument entry may have and the kind of instrument it makes: one row per driver, in alphabetical
/// order so that two drivers added at once touch different lines. The checklist tests (`src/checklist.rs`) require each one to
/// appear in the README with an example entry.
pub const DRIVER_KINDS: &[(&str, Kind)] = &[
    ("atm6301", Kind::Psu),
    ("audio", Kind::Audio),
    ("bladerf", Kind::Sdr),
    ("example", Kind::Example),
    ("hamlib", Kind::Hamlib),
    ("limesdr", Kind::Sdr),
    ("nanovna", Kind::Vna),
    ("procedure", Kind::Procedure),
    ("rotator", Kind::Rotator),
    ("siglent_sds", Kind::Scope),
    ("usb_hub", Kind::UsbHub),
    ("vnwa_open", Kind::Vna),
];

/// The driver names, for messages and tests.
pub fn driver_names() -> Vec<&'static str> {
    // [@ANCHOR: bench_mcp_config_driver_names]
    DRIVER_KINDS.iter().map(|(d, _)| *d).collect()
}

/// A module's own `Config`, parsed from the entry when the entry's driver is that module, else its default. A bad entry for
/// one driver therefore never stops another from loading. The module is named like its driver.
macro_rules! module_config {
    ($module:ident, $driver:expr, $c:expr, $name:expr) => {
        if $driver == stringify!($module) {
            crate::$module::parse_config($c).map_err(|e| format!("{}: {e}", $name))?
        } else {
            crate::$module::Config::default()
        }
    };
}

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
    Sdr,
    /// The host's USB hubs and ports (`usb_hub` module): read from sysfs, switched with uhubctl.
    UsbHub,
    /// The host's sound devices (`audio` module): PipeWire, wpctl and the speech synthesizers.
    Audio,
    /// A radio driven through Hamlib's `rigctld` (`hamlib` module): receivers, and transmitters into an attested dummy load.
    Hamlib,
    /// An antenna rotator driven through Hamlib's `rotctld` (`rotator` module).
    Rotator,
    /// The skeleton instrument (`example` module): an imaginary SCPI signal source, the starting point for a new driver.
    Example,
    /// The procedure runner (`procedure` module): programs that call bench tools in loops, next to the server.
    Procedure,
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
    /// Supply only: the longest the output may stay on after one switch-on, seconds.
    pub max_on_s: f64,
    /// VNA only: the settings of `vnwa_open::sweep::Config` (defaults for the other kinds, unused).
    pub vna: vnwa_open::sweep::Config,
    /// NanoVNA only (driver `nanovna`).
    pub nanovna: crate::nanovna::Config,
    pub sdr: crate::sdr::Config,
    pub usb: crate::usb_hub::Config,
    pub audio: crate::audio::Config,
    pub hamlib: crate::hamlib::Config,
    pub rotator: crate::rotator::Config,
    pub example: crate::example::Config,
    pub procedure: crate::procedure::Config,
}

fn tier_of(s: Option<&str>) -> Result<Tier, String> {
    match s {
        None | Some("read_only") => Ok(Tier::ReadOnly),
        Some("read_config") => Ok(Tier::ReadConfig),
        Some("full") => Ok(Tier::Full),
        Some(o) => Err(format!("unknown tier {o:?} (read_only, read_config, full)")),
    }
}

/// The VNA settings of an instrument entry: `codec_s11`, `codec_s21` (parts of the ALSA ids; found from the USB tree when omitted), `hub` and `hub_port` (the power-switching hub port, for cold resets), `retries`, `if_hz`, `point_ms`, `rf_level`,
/// `settle_ms`, `window_ms`; anything omitted keeps the default of `vnwa_open::sweep::Config`.
fn vna_config(c: &Value) -> Result<vnwa_open::sweep::Config, String> {
    let mut v = vnwa_open::sweep::Config::default();
    let text = |k: &str| c.get(k).and_then(|x| x.as_str()).map(String::from);
    let num = |k: &str| c.get(k).and_then(|x| x.as_f64());
    if let Some(t) = text("codec_s11") {
        v.codec_s11 = Some(t);
    }
    if let Some(t) = text("codec_s21") {
        v.codec_s21 = Some(t);
    }
    if let Some(x) = num("if_hz") {
        v.if_hz = x;
    }
    if let Some(x) = num("point_ms") {
        v.point_ms = x;
    }
    // The power-switching hub port the unit is on (`uhubctl -l <hub> -p <port>`): both keys or neither.
    match (text("hub"), c.get("hub_port").and_then(|x| x.as_u64())) {
        (Some(hub), Some(port)) => {
            let port = u8::try_from(port).map_err(|_| "hub_port must be 1 to 255".to_string())?;
            v.hub = Some(vnwa_open::power::HubPort { hub, port });
        }
        (None, None) => {}
        _ => return Err("hub and hub_port must be given together".into()),
    }
    if let Some(x) = c.get("retries").and_then(|x| x.as_u64()) {
        v.retries = u32::try_from(x).map_err(|_| "retries is too large".to_string())?;
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

fn sdr_config(driver: &str, c: &Value) -> Result<crate::sdr::Config, String> {
    let text = |k: &str| c.get(k).and_then(|x| x.as_str()).map(String::from);
    let cfg = crate::sdr::Config {
        driver: driver.to_string(),
        serial: if matches!(driver, "bladerf" | "limesdr") {
            text("serial")
        } else {
            None
        },
        xb200: text("xb200"),
    };
    if matches!(driver, "bladerf" | "limesdr") {
        crate::sdr::check_config(&cfg)?;
    } else if cfg.xb200.is_some() {
        return Err("xb200 applies to the bladeRF only".into());
    }
    Ok(cfg)
}

/// The named instances of a config. An entry with a `driver` is one instance named by its key (the old form, unchanged). An entry
/// without one whose members are all objects with a `driver` is a group (`"vna": {"vnwa": {...}, "nano": {...}}`): each member is an
/// instance named by its own key, and the group's key is only a label. Names must be unique across the whole config.
fn flatten_entries(map: &serde_json::Map<String, Value>) -> Result<Vec<(&String, &Value)>, String> {
    // [@ANCHOR: bench_mcp_config_flatten]
    let mut out: Vec<(&String, &Value)> = Vec::new();
    for (key, c) in map {
        let group = c.as_object().filter(|o| {
            !o.contains_key("driver")
                && !o.is_empty()
                && o.values().all(|m| m.get("driver").is_some())
        });
        match group {
            Some(o) => out.extend(o.iter()),
            None => out.push((key, c)),
        }
    }
    for (i, (n, _)) in out.iter().enumerate() {
        if out[..i].iter().any(|(m, _)| m == n) {
            return Err(format!(
                "config: two instances are named {n:?}; every instance name must be unique"
            ));
        }
    }
    Ok(out)
}

/// Refuses two instances that claim the same physical thing: one serial device (compared after resolving symlinks), one scope
/// host and port, one VNWA (the `vnwa_open` driver opens the one USB unit), or one power-switching hub port.
fn reject_shared_claims(insts: &[Instrument]) -> Result<(), String> {
    // [@ANCHOR: bench_mcp_config_shared_claims]
    let real = |p: &str| {
        std::fs::canonicalize(p)
            .map(|x| x.display().to_string())
            .unwrap_or_else(|_| p.to_string())
    };
    for (i, a) in insts.iter().enumerate() {
        for b in &insts[i + 1..] {
            let clash = |what: String| {
                Err(format!(
                    "{} and {} both claim {what}; each device belongs to one instance",
                    a.name, b.name
                ))
            };
            let serial = |x: &Instrument| match x.kind {
                Kind::Psu => x.serial.clone(),
                Kind::Vna if x.driver == "nanovna" => x.nanovna.serial.clone(),
                _ => None,
            };
            if let (Some(x), Some(y)) = (serial(a), serial(b)) {
                if real(&x) == real(&y) {
                    return clash(format!("the serial port {x}"));
                }
            }
            if a.kind == Kind::Scope
                && b.kind == Kind::Scope
                && a.host.is_some()
                && a.host == b.host
                && a.port == b.port
            {
                return clash(format!(
                    "the scope at {}:{}",
                    a.host.clone().unwrap(),
                    a.port
                ));
            }
            if a.driver == "vnwa_open" && b.driver == "vnwa_open" {
                return clash("the VNWA USB unit".to_string());
            }
            if a.kind == Kind::Vna && b.kind == Kind::Vna {
                if let (Some(x), Some(y)) = (&a.vna.hub, &b.vna.hub) {
                    if x.hub == y.hub && x.port == y.port {
                        return clash(format!("hub {} port {}", x.hub, x.port));
                    }
                }
            }
        }
    }
    Ok(())
}

pub fn parse(text: &str) -> Result<Vec<Instrument>, String> {
    // [@ANCHOR: bench_mcp_config]
    let v: Value = serde_json::from_str(text).map_err(|e| format!("config: {e}"))?;
    let map = v
        .get("instruments")
        .and_then(|i| i.as_object())
        .ok_or("config: no \"instruments\" object")?;
    let mut out = Vec::new();
    let entries = flatten_entries(map)?;
    for (name, c) in entries {
        let driver = c
            .get("driver")
            .and_then(|d| d.as_str())
            .ok_or(format!("{name}: no driver"))?
            .to_string();
        let kind = DRIVER_KINDS
            .iter()
            .find(|(d, _)| *d == driver)
            .map(|(_, k)| *k)
            .ok_or_else(|| {
                format!(
                    "{name}: unknown driver {driver:?} ({})",
                    driver_names().join(", ")
                )
            })?;
        let driver_name = driver.clone();
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
            max_on_s: c.get("max_on_s").and_then(|m| m.as_f64()).unwrap_or(600.0),
            vna: vna_config(c).map_err(|e| format!("{name}: {e}"))?,
            sdr: sdr_config(&driver_name, c).map_err(|e| format!("{name}: {e}"))?,
            usb: crate::usb_hub::parse_config(c).map_err(|e| format!("{name}: {e}"))?,
            nanovna: module_config!(nanovna, driver_name, c, name),
            audio: module_config!(audio, driver_name, c, name),
            hamlib: module_config!(hamlib, driver_name, c, name),
            rotator: module_config!(rotator, driver_name, c, name),
            example: module_config!(example, driver_name, c, name),
            procedure: module_config!(procedure, driver_name, c, name),
        };
        match (inst.kind, &inst.host, &inst.serial) {
            (Kind::Scope, None, _) => return Err(format!("{name}: a scope needs \"host\"")),
            (Kind::Example, None, _) => {
                return Err(format!("{name}: an example instrument needs \"host\""))
            }
            (Kind::Psu, _, None) => return Err(format!("{name}: a supply needs \"serial\"")),
            _ => {}
        }
        if !(inst.max_volts.is_finite()
            && inst.max_volts > 0.0
            && inst.max_amps.is_finite()
            && inst.max_amps > 0.0
            && inst.max_on_s.is_finite()
            && inst.max_on_s >= 1.0)
        {
            return Err(format!(
                "{name}: max_volts and max_amps must be positive numbers and max_on_s at least 1"
            ));
        }
        out.push(inst);
    }
    reject_shared_claims(&out)?;
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

    // Tests [@ANCHOR: bench_mcp_config_driver_names]
    #[test]
    fn every_listed_driver_is_known_to_parse() {
        assert_eq!(driver_names().len(), DRIVER_KINDS.len());
        for d in driver_names() {
            let text = format!(r#"{{"instruments":{{"x":{{"driver":"{d}"}}}}}}"#);
            let e = parse(&text).err().unwrap_or_default();
            assert!(!e.contains("unknown driver"), "{d}: {e}");
        }
        assert!(parse(r#"{"instruments":{"x":{"driver":"nope"}}}"#)
            .unwrap_err()
            .contains("example"));
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

    // Tests [@ANCHOR: bench_mcp_config]
    #[test]
    fn the_vna_hub_port_and_retries_are_read_and_must_come_as_a_pair() {
        let vna = |extra: &str| {
            parse(&format!(
                r#"{{"instruments":{{"v":{{"driver":"vnwa_open"{extra}}}}}}}"#
            ))
        };
        let ok = vna(r#","hub":"3-2","hub_port":2,"retries":4"#).unwrap();
        let cfg = &ok[0].vna;
        let hub = cfg.hub.as_ref().unwrap();
        assert_eq!((hub.hub.as_str(), hub.port, cfg.retries), ("3-2", 2, 4));
        assert!(vna("").unwrap()[0].vna.hub.is_none());
        assert!(vna(r#","hub":"3-2""#).unwrap_err().contains("together"));
        assert!(vna(r#","hub_port":2"#).unwrap_err().contains("together"));
        assert!(vna(r#","hub":"3-2","hub_port":300"#).is_err());
    }
}
