// SPDX-License-Identifier: AGPL-3.0-or-later
//! The instrument list, read from the JSON file named by `BENCH_MCP_CONFIG`:
//!
//! ```json
//! {"instruments": {
//!   "scope": {"driver": "siglent_sds", "tier": "read_config", "host": "192.168.10.51", "port": 5025},
//!   "psu":   {"driver": "atm6301", "tier": "read_config", "serial": "/dev/serial/by-id/usb-EXAMPLE_supply-if00-port0", "baud": 115200,
//!             "max_volts": 5.5, "max_amps": 1.0, "max_on_s": 600}
//! }, "reserved_serial": ["/dev/serial/by-id/usb-EXAMPLE_relay_board-if00-port0", "usb:1-4.2"]}
//! (`reserved_serial`: devices no bench tool opens; see `serial_guard.rs`)
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
    ("elad", Kind::Elad),
    ("example", Kind::Example),
    ("glasgow", Kind::Glasgow),
    ("limesdr", Kind::Sdr),
    ("nanovna", Kind::Vna),
    ("procedure", Kind::Procedure),
    ("radioctl", Kind::Radioctl),
    ("rotator", Kind::Rotator),
    ("rtlsdr", Kind::Sdr),
    ("rx888", Kind::Rx888),
    ("siglent_sds", Kind::Scope),
    ("siglent_ssa", Kind::Sa),
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

// [@ANCHOR: bench_mcp_config_tier]
#[derive(PartialEq, PartialOrd, Clone, Copy, Debug)]
pub enum Tier {
    ReadOnly = 0,
    ReadConfig = 1,
    Full = 2,
}

// [@ANCHOR: bench_mcp_config_kind]
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
    /// A radio driven through the `radioctld` daemon (`radioctl` module): receivers, and transmitters into an attested dummy load.
    Radioctl,
    /// An antenna rotator driven through a `radioctld` rotator listener (`rotator` module).
    Rotator,
    /// A Siglent SSA3000X spectrum analyzer with tracking generator (`sa` module), SCPI over the LAN.
    Sa,
    /// An ELAD FDM-DUO, FDM-S2 or FDM-S1 receiver through the `elad_usb` crate (receive only).
    Elad,
    /// The skeleton instrument (`example` module): an imaginary SCPI signal source, the starting point for a new driver.
    Example,
    /// The procedure runner (`procedure` module): programs that call bench tools in loops, next to the server.
    Procedure,
    /// The RX-888 MkII HF receiver through ka9q-radio's `radiod` (`rx888` module), receive only.
    Rx888,
    /// The Glasgow Interface Explorer (`glasgow` module): digital I/O (UART, I2C, SPI, JTAG, SWD, logic analyzer, GPIO) through the pinned `glasgow` tool.
    Glasgow,
}

/// The longest `max_on_s` a config may ask for (one day).
pub const MAX_ON_S_LIMIT: f64 = 86_400.0;

// [@ANCHOR: bench_mcp_config_instrument]
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
    /// Scope only: the pause after each failed connection attempt (`connect_waits_s`), seconds; one more attempt than there are pauses.
    pub connect_waits_s: Vec<f64>,
    /// VNA only: the settings of `vnwa_open::sweep::Config` (defaults for the other kinds, unused).
    pub vna: vnwa_open::sweep::Config,
    /// NanoVNA only (driver `nanovna`).
    pub nanovna: crate::nanovna::Config,
    pub sdr: crate::sdr::Config,
    pub usb: crate::usb_hub::Config,
    pub audio: crate::audio::Config,
    pub radioctl: crate::radioctl::Config,
    pub rotator: crate::rotator::Config,
    pub elad: crate::elad::Config,
    pub example: crate::example::Config,
    pub sa: crate::sa::Config,
    pub procedure: crate::procedure::Config,
    pub rx888: crate::rx888::Config,
    pub glasgow: crate::glasgow::Config,
}

fn tier_of(s: Option<&str>) -> Result<Tier, String> {
    // [@ANCHOR: bench_mcp_config_tier_of]
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
    // [@ANCHOR: bench_mcp_config_vna]
    let mut v = vnwa_open::sweep::Config::default();
    let text = |k: &str| c.get(k).and_then(|x| x.as_str()).map(String::from);
    if let Some(t) = text("codec_s11") {
        v.codec_s11 = Some(t);
    }
    if let Some(t) = text("codec_s21") {
        v.codec_s21 = Some(t);
    }
    // A present value of the wrong type is an error, not the default (a mistyped `if_hz` or `rf_level` would otherwise run
    // the unit at settings the operator did not write).
    v.if_hz = number(c, "if_hz", v.if_hz)?;
    v.point_ms = number(c, "point_ms", v.point_ms)?;
    for k in ["codec_s11", "codec_s21", "hub"] {
        if c.get(k).is_some_and(|x| !x.is_null() && !x.is_string()) {
            return Err(format!("{k} must be text"));
        }
    }
    // The power-switching hub port the unit is on (`uhubctl -l <hub> -p <port>`): both keys or neither.
    let hub_port = whole::<u8>(c, "hub_port", 0)?;
    match (text("hub"), c.get("hub_port").filter(|x| !x.is_null())) {
        (Some(hub), Some(_)) => {
            v.hub = Some(vnwa_open::power::HubPort {
                hub,
                port: hub_port,
            });
        }
        (None, None) => {}
        _ => return Err("hub and hub_port must be given together".into()),
    }
    v.retries = whole(c, "retries", v.retries)?;
    v.settle_ms = optional_number(c, "settle_ms")?;
    v.window_ms = optional_number(c, "window_ms")?;
    v.rf_level = whole(c, "rf_level", v.rf_level)?;
    vnwa_open::sweep::check(&v)?;
    Ok(v)
}

fn sdr_config(driver: &str, c: &Value) -> Result<crate::sdr::Config, String> {
    // [@ANCHOR: bench_mcp_config_sdr]
    let text = |k: &str| c.get(k).and_then(|x| x.as_str()).map(String::from);
    let cfg = crate::sdr::Config {
        driver: driver.to_string(),
        serial: if matches!(driver, "bladerf" | "limesdr" | "rtlsdr") {
            text("serial")
        } else {
            None
        },
        xb200: text("xb200"),
        direct_samp: text("direct_samp"),
    };
    if matches!(driver, "bladerf" | "limesdr" | "rtlsdr") {
        crate::sdr::check_config(&cfg)?;
    } else if cfg.xb200.is_some() {
        return Err("xb200 applies to the bladeRF only".into());
    } else if cfg.direct_samp.is_some() {
        return Err("direct_samp applies to the RTL-SDR only".into());
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

/// The `connect_waits_s` of a scope entry: up to eight pauses of 0 to 3600 s between connection attempts; 2, 5, 10 and 20 s when omitted.
/// A number field that is absent (or null) takes the default; one that is present must be a number. A mistyped supply limit
/// (`"max_volts": "3.3"`) must not fall back to the default limit.
// [@ANCHOR: bench_mcp_config_number]
fn number(c: &Value, key: &str, default: f64) -> Result<f64, String> {
    match c.get(key) {
        None | Some(Value::Null) => Ok(default),
        Some(v) => v.as_f64().ok_or(format!("{key} must be a number")),
    }
}

/// As `number` but an absent field is `None`, not a default.
// [@ANCHOR: bench_mcp_config_optional_number]
fn optional_number(c: &Value, key: &str) -> Result<Option<f64>, String> {
    match c.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v
            .as_f64()
            .map(Some)
            .ok_or(format!("{key} must be a number")),
    }
}

/// As `number` for a whole number that must fit the field type (a port of 70561 must not become 5025).
// [@ANCHOR: bench_mcp_config_whole]
fn whole<T: TryFrom<u64>>(c: &Value, key: &str, default: T) -> Result<T, String> {
    match c.get(key) {
        None | Some(Value::Null) => Ok(default),
        Some(v) => v
            .as_u64()
            .and_then(|n| T::try_from(n).ok())
            .ok_or(format!("{key} must be a whole number in range")),
    }
}

fn connect_waits(c: &Value) -> Result<Vec<f64>, String> {
    // [@ANCHOR: bench_mcp_config_connect_waits]
    let Some(v) = c.get("connect_waits_s") else {
        return Ok(vec![2.0, 5.0, 10.0, 20.0]);
    };
    v.as_array()
        .filter(|a| a.len() <= 8)
        .and_then(|a| {
            a.iter()
                .map(|x| x.as_f64().filter(|w| (0.0..=3600.0).contains(w)))
                .collect()
        })
        .ok_or_else(|| {
            "connect_waits_s must be up to eight numbers from 0 to 3600 (seconds)".to_string()
        })
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
        if driver == "hamlib" {
            return Err(format!("{name}: the \"hamlib\" driver was removed; use \"driver\": \"radioctl\" (a radioctl rig description in \"rig\" instead of the Hamlib \"model\" number; see the README section Radioctl rigs)"));
        }
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
            port: whole(c, "port", 5025).map_err(|e| format!("{name}: {e}"))?,
            serial: c.get("serial").and_then(|s| s.as_str()).map(String::from),
            baud: whole(c, "baud", 115200).map_err(|e| format!("{name}: {e}"))?,
            max_volts: number(c, "max_volts", 5.5).map_err(|e| format!("{name}: {e}"))?,
            max_amps: number(c, "max_amps", 1.0).map_err(|e| format!("{name}: {e}"))?,
            max_on_s: number(c, "max_on_s", 600.0).map_err(|e| format!("{name}: {e}"))?,
            connect_waits_s: connect_waits(c).map_err(|e| format!("{name}: {e}"))?,
            vna: vna_config(c).map_err(|e| format!("{name}: {e}"))?,
            sdr: sdr_config(&driver_name, c).map_err(|e| format!("{name}: {e}"))?,
            usb: crate::usb_hub::parse_config(c).map_err(|e| format!("{name}: {e}"))?,
            nanovna: module_config!(nanovna, driver_name, c, name),
            audio: module_config!(audio, driver_name, c, name),
            radioctl: module_config!(radioctl, driver_name, c, name),
            rotator: module_config!(rotator, driver_name, c, name),
            elad: module_config!(elad, driver_name, c, name),
            example: module_config!(example, driver_name, c, name),
            sa: if driver_name == "siglent_ssa" {
                crate::sa::parse_config(c).map_err(|e| format!("{name}: {e}"))?
            } else {
                crate::sa::Config::default()
            },
            procedure: module_config!(procedure, driver_name, c, name),
            rx888: module_config!(rx888, driver_name, c, name),
            glasgow: module_config!(glasgow, driver_name, c, name),
        };
        match (inst.kind, &inst.host, &inst.serial) {
            (Kind::Scope, None, _) => return Err(format!("{name}: a scope needs \"host\"")),
            (Kind::Example, None, _) => {
                return Err(format!("{name}: an example instrument needs \"host\""))
            }
            (Kind::Sa, None, _) => {
                return Err(format!("{name}: a spectrum analyzer needs \"host\""))
            }
            (Kind::Psu, _, None) => return Err(format!("{name}: a supply needs \"serial\"")),
            _ => {}
        }
        if !(inst.max_volts.is_finite()
            && inst.max_volts > 0.0
            && inst.max_amps.is_finite()
            && inst.max_amps > 0.0
            && inst.max_on_s.is_finite()
            && (1.0..=MAX_ON_S_LIMIT).contains(&inst.max_on_s))
        {
            return Err(format!(
                "{name}: max_volts and max_amps must be positive numbers and max_on_s from 1 to 86400"
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
    // [@ANCHOR: bench_mcp_config_load]
    let path =
        std::env::var("BENCH_MCP_CONFIG").map_err(|_| "BENCH_MCP_CONFIG is not set".to_string())?;
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{path}: {e}"))?;
    let top: Value = serde_json::from_str(&text).map_err(|e| format!("config: {e}"))?;
    crate::serial_guard::set_reserved(crate::serial_guard::parse_reserved(&top)?);
    parse(&text)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Tests [@ANCHOR: bench_mcp_config_number]
    // Tests [@ANCHOR: bench_mcp_config_optional_number]
    // Tests [@ANCHOR: bench_mcp_config_whole]
    #[test]
    fn number_optional_number_and_whole_take_defaults_but_refuse_wrong_types() {
        let c: Value = serde_json::from_str(r#"{"a":1.5,"n":null,"s":"3","w":70561}"#).unwrap();
        assert_eq!(number(&c, "a", 9.0), Ok(1.5));
        assert_eq!(number(&c, "n", 9.0), Ok(9.0));
        assert_eq!(number(&c, "missing", 9.0), Ok(9.0));
        assert!(number(&c, "s", 9.0).is_err());
        assert_eq!(optional_number(&c, "a"), Ok(Some(1.5)));
        assert_eq!(optional_number(&c, "n"), Ok(None));
        assert_eq!(optional_number(&c, "missing"), Ok(None));
        assert!(optional_number(&c, "s").is_err());
        assert_eq!(whole::<u32>(&c, "w", 5), Ok(70561));
        assert_eq!(whole::<u16>(&c, "missing", 5025), Ok(5025));
        assert!(whole::<u16>(&c, "w", 5025).is_err());
        assert!(whole::<u16>(&c, "a", 5025).is_err());
    }

    // Tests [@ANCHOR: bench_mcp_config]
    #[test]
    fn a_mistyped_or_out_of_range_limit_port_or_baud_is_an_error_not_a_default() {
        let psu = |extra: &str| {
            parse(&format!(
                r#"{{"instruments":{{"p":{{"driver":"atm6301","serial":"/dev/ttyUSB0"{extra}}}}}}}"#
            ))
        };
        assert!(psu("").is_ok());
        for bad in [
            r#","max_volts":"3.3""#,
            r#","max_amps":true"#,
            r#","max_on_s":"60""#,
            r#","baud":4294976896"#,
            r#","baud":"9600""#,
        ] {
            assert!(psu(bad).is_err(), "{bad}");
        }
        let scope = |extra: &str| {
            parse(&format!(
                r#"{{"instruments":{{"s":{{"driver":"siglent_sds","host":"h"{extra}}}}}}}"#
            ))
        };
        assert_eq!(scope(r#","port":5026"#).unwrap()[0].port, 5026);
        assert!(scope(r#","port":70561"#).is_err());
        assert!(scope(r#","port":"5025""#).is_err());
    }

    // Tests [@ANCHOR: bench_mcp_config_vna]
    #[test]
    fn a_mistyped_vna_setting_is_an_error_not_the_default() {
        let vna = |extra: &str| {
            parse(&format!(
                r#"{{"instruments":{{"vna":{{"driver":"vnwa_open"{extra}}}}}}}"#
            ))
        };
        assert!(vna(r#","hub":"1-1","hub_port":2"#).is_ok());
        for bad in [
            r#","if_hz":"4000""#,
            r#","point_ms":"30""#,
            r#","rf_level":"100""#,
            r#","retries":"3""#,
            r#","settle_ms":"5""#,
            r#","hub":5,"hub_port":"2""#,
            r#","hub":"1-1","hub_port":300"#,
        ] {
            assert!(vna(bad).is_err(), "{bad}");
        }
    }

    // Tests [@ANCHOR: bench_mcp_config_tier_of]
    // Tests [@ANCHOR: bench_mcp_config]
    // Tests [@ANCHOR: bench_mcp_fake_scope_kind]
    // Tests [@ANCHOR: bench_mcp_config_kind]
    // Tests [@ANCHOR: bench_mcp_config_tier]
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

    // Tests [@ANCHOR: bench_mcp_config_connect_waits]
    // Tests [@ANCHOR: bench_mcp_config]
    #[test]
    fn connect_waits_default_to_37_seconds_and_are_bounded() {
        let waits = |extra: &str| {
            parse(&format!(
                r#"{{"instruments":{{"s":{{"driver":"siglent_sds","host":"h"{extra}}}}}}}"#
            ))
            .map(|v| v[0].connect_waits_s.clone())
        };
        assert_eq!(waits("").unwrap(), [2.0, 5.0, 10.0, 20.0]);
        assert_eq!(
            waits(r#","connect_waits_s":[300,600,1800]"#).unwrap(),
            [300.0, 600.0, 1800.0]
        );
        assert_eq!(
            waits(r#","connect_waits_s":[]"#).unwrap(),
            Vec::<f64>::new()
        );
        for bad in ["[-1]", "[3601]", r#"["5"]"#, "5", "[1,1,1,1,1,1,1,1,1]"] {
            let e = waits(&format!(r#","connect_waits_s":{bad}"#)).unwrap_err();
            assert!(e.contains("connect_waits_s must be"), "{bad}: {e}");
        }
    }

    // Tests [@ANCHOR: bench_mcp_config_vna]
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

    // Tests [@ANCHOR: bench_mcp_config_tier_of]
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
        for bad in ["1e30", "86401", "0.5", "-5"] {
            let e = parse(&format!(
                r#"{{"instruments":{{"x":{{"driver":"atm6301","serial":"/dev/x","max_on_s":{bad}}}}}}}"#
            ))
            .unwrap_err();
            assert!(e.contains("max_on_s from 1 to 86400"), "{bad}: {e}");
        }
        assert!(parse(
            r#"{"instruments":{"x":{"driver":"atm6301","serial":"/dev/x","max_on_s":86400}}}"#
        )
        .is_ok());
    }

    // Tests [@ANCHOR: bench_mcp_config_vna]
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

    // Tests [@ANCHOR: bench_mcp_config_sdr]
    #[test]
    fn an_sdr_entry_takes_its_board_id_and_transverter_by_driver_and_refuses_what_does_not_fit() {
        let sdr = |extra: &str, driver: &str| {
            parse(&format!(
                r#"{{"instruments":{{"r":{{"driver":"{driver}"{extra}}}}}}}"#
            ))
        };
        let ok = sdr(r#","serial":"ca3e","xb200":"auto""#, "bladerf").unwrap();
        assert_eq!(ok[0].kind, Kind::Sdr);
        assert_eq!(ok[0].sdr.serial.as_deref(), Some("ca3e"));
        assert_eq!(ok[0].sdr.xb200.as_deref(), Some("auto"));
        assert_eq!(sdr("", "limesdr").unwrap()[0].sdr.driver, "limesdr");
        assert!(sdr(r#","xb200":"auto""#, "limesdr")
            .unwrap_err()
            .contains("bladeRF only"));
        let rtl = sdr(r#","serial":"ca3e","direct_samp":"2""#, "rtlsdr").unwrap();
        assert_eq!(rtl[0].kind, Kind::Sdr);
        assert_eq!(rtl[0].sdr.direct_samp.as_deref(), Some("2"));
        assert!(sdr(r#","direct_samp":"2""#, "limesdr").is_err());
        assert!(sdr(r#","direct_samp":"5""#, "rtlsdr").is_err());
        assert!(sdr(r#","xb200":"auto""#, "rtlsdr").is_err());
        assert!(sdr(r#","serial":"not hex""#, "bladerf").is_err());
        assert!(sdr(r#","xb200":"enabled""#, "bladerf").is_err());
        // A non-SDR driver does not read the SDR keys at all.
        let scope =
            parse(r#"{"instruments":{"s":{"driver":"siglent_sds","host":"h","serial":"zz"}}}"#)
                .unwrap();
        assert_eq!(scope[0].sdr.serial, None);
    }

    // Tests [@ANCHOR: bench_mcp_config_load]
    #[test]
    fn the_config_is_read_from_the_file_named_by_the_environment() {
        let dir = crate::testdir::TestDir::new("bench_mcp_config_load");
        let file = dir.join("bench.json");
        std::fs::write(
            &file,
            r#"{"instruments":{"scope":{"driver":"siglent_sds","host":"10.0.0.2"}}}"#,
        )
        .unwrap();
        let bad = dir.join("bad.json");
        std::fs::write(&bad, r#"{"instruments":{"scope":{"driver":"siglent_sds","host":"h"}},"reserved_serial":["ttyUSB0"]}"#).unwrap();
        // This is the only test in this process that reads the variable (the end-to-end tests set it in their child processes).
        std::env::remove_var("BENCH_MCP_CONFIG");
        assert_eq!(load().unwrap_err(), "BENCH_MCP_CONFIG is not set");
        std::env::set_var("BENCH_MCP_CONFIG", dir.join("missing.json"));
        assert!(load().unwrap_err().contains("missing.json"));
        std::env::set_var("BENCH_MCP_CONFIG", &file);
        // Tests [@ANCHOR: bench_mcp_serial_guard_set_reserved]
        // Tests [@ANCHOR: bench_mcp_serial_guard_check_reserved_open]
        std::env::set_var("BENCH_MCP_CONFIG", &bad);
        assert!(load().unwrap_err().contains("reserved_serial"));
        std::fs::write(&file, r#"{"instruments":{"scope":{"driver":"siglent_sds","host":"10.0.0.2"}},"reserved_serial":["/dev/serial/by-id/usb-guard-test-only-never-exists"]}"#).unwrap();
        std::env::set_var("BENCH_MCP_CONFIG", &file);
        let v = load().unwrap();
        // The list from the file is now the process's: serial opens of that device are refused before anything is touched.
        let entry = "/dev/serial/by-id/usb-guard-test-only-never-exists";
        assert!(crate::serial_guard::check_reserved_open(entry)
            .unwrap_err()
            .contains("reserved"));
        assert!(crate::link::Serial::open(entry, 115200)
            .err()
            .unwrap()
            .contains("reserved_serial"));
        assert!(crate::link::Link::serial(entry, 115200)
            .err()
            .unwrap()
            .contains("reserved_serial"));
        std::env::remove_var("BENCH_MCP_CONFIG");
        std::fs::remove_dir_all(&dir).ok();
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].name, "scope");
    }
}
