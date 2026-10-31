// SPDX-License-Identifier: AGPL-3.0-or-later
//! The radioctld configuration file: which rigs to serve, on which addresses, over which
//! transports. Validated completely at load time; an unknown key, an unsafe bind, or a value out
//! of range is an error, never a default.
//!
//! ```toml
//! [server]                       # optional; every key has the default shown
//! allow = ["127.0.0.0/8", "::1"] # who may connect
//! allow_non_loopback_bind = false
//! max_clients = 16               # per rig
//! idle_timeout_secs = 600
//! line_timeout_secs = 10         # a started line must finish within this
//! max_line_bytes = 1024
//! request_timeout_ms = 2000
//! cache_max_age_ms = 1000        # reads newer than this are answered from the cache
//!
//! [[rig]]
//! name = "kx3"
//! description = "rigs/elecraft-kx3.toml"   # relative to this file
//! listen = "127.0.0.1:4532"                # default: loopback, port 4532 + index
//! vfo_mode = false                         # rigctld's -o: \chk_vfo answers 1
//! transmit = false                         # true opens the rig's transmit gate at start
//! [rig.transport]
//! kind = "virtual"                         # or "serial" (path/usb + baud, lines) or "tcp" (address)
//!
//! [[rotator]]
//! name = "g5500"
//! model = "yaesu-g-5500-gs232b"            # an id from data/station/rotators.toml
//! listen = "127.0.0.1:4533"                # default: loopback, port 4533 + index
//! [rotator.transport]
//! kind = "virtual"                         # or "tcp" (address)
//! ```

use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};
use std::time::Duration;

use radioctl_core::serial::{LineControl, PortTarget};
use radioctl_core::usb::UsbMatcher;
use radioctl_station::rotator::RotatorDescription;
use serde::Deserialize;

use crate::access::AllowList;

/// The first port used when a rig has no `listen`.
const BASE_PORT: u16 = 4532;
/// The first port used when a rotator has no `listen` (Hamlib's `rotctld` default).
const ROTATOR_BASE_PORT: u16 = 4533;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FileDoc {
    server: Option<ServerSection>,
    #[serde(default)]
    rig: Vec<RigSection>,
    #[serde(default)]
    rotator: Vec<RotatorSection>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RotatorSection {
    name: String,
    model: String,
    listen: Option<String>,
    transport: RotatorTransportSection,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase", deny_unknown_fields)]
enum RotatorTransportSection {
    Virtual,
    Tcp { address: String },
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct ServerSection {
    allow: Option<Vec<String>>,
    #[serde(default)]
    allow_non_loopback_bind: bool,
    max_clients: Option<usize>,
    idle_timeout_secs: Option<u64>,
    line_timeout_secs: Option<u64>,
    max_line_bytes: Option<usize>,
    request_timeout_ms: Option<u64>,
    cache_max_age_ms: Option<u64>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RigSection {
    name: String,
    description: String,
    listen: Option<String>,
    #[serde(default)]
    vfo_mode: bool,
    #[serde(default)]
    transmit: bool,
    hamlib_model: Option<u32>,
    rx_low_hz: Option<u64>,
    rx_high_hz: Option<u64>,
    tx_power_mw: Option<u32>,
    max_transmit_s: Option<u64>,
    transport: TransportSection,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase", deny_unknown_fields)]
enum TransportSection {
    Virtual {
        reply_delay_ms: Option<u64>,
    },
    Serial {
        path: Option<String>,
        usb: Option<UsbSection>,
        baud: u32,
        lines: Option<String>,
    },
    Tcp {
        address: String,
    },
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct UsbSection {
    vid: u16,
    pid: u16,
    serial: Option<String>,
}

/// Server-wide settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerConfig {
    /// Who may connect.
    pub allow: AllowList,
    /// Most simultaneous clients per rig.
    pub max_clients: usize,
    /// A connection with nothing to say for this long is closed.
    pub idle_timeout: Duration,
    /// A line that has begun must end within this.
    pub line_timeout: Duration,
    /// Longest request line, in bytes.
    pub max_line_bytes: usize,
    /// Deadline for a request that has to go to the rig.
    pub request_timeout: Duration,
    /// Reads at most this old are answered from the rig's cache.
    pub cache_max_age: Duration,
}

/// How a rig is reached.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransportConfig {
    /// An in-memory virtual rig generated from the description.
    Virtual {
        /// How long the virtual rig takes to answer.
        reply_delay: Duration,
    },
    /// A serial or USB-serial port.
    Serial {
        /// The port, by path or by USB ids.
        target: PortTarget,
        /// Baud rate.
        baud: u32,
        /// Whether the port has modem control lines this daemon may use.
        lines: LineControl,
    },
    /// A TCP connection (a network serial server or a radio with its own CAT port).
    Tcp {
        /// `host:port`.
        address: String,
    },
}

/// One rig to serve.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RigConfig {
    /// The name used in logs.
    pub name: String,
    /// The rig description file.
    pub description: PathBuf,
    /// Where clients connect.
    pub listen: SocketAddr,
    /// `\chk_vfo` answers 1 and responses echo the VFO.
    pub vfo_mode: bool,
    /// Open the rig's transmit gate at start. Off unless the file says so.
    pub transmit: bool,
    /// The model number reported in `\dump_state`.
    pub hamlib_model: u32,
    /// Lowest frequency the rig is said to cover, in hertz.
    pub rx_low_hz: u64,
    /// Highest frequency the rig is said to cover, in hertz.
    pub rx_high_hz: u64,
    /// Maximum transmit power reported, in milliwatts.
    pub tx_power_mw: u32,
    /// The actor's transmit time limit, when the file sets one (1 to 3600 seconds).
    pub max_transmit: Option<Duration>,
    /// How to reach the rig.
    pub transport: TransportConfig,
}

/// How a rotator is reached.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RotatorTransportConfig {
    /// A virtual rotator of the model, served inside the daemon (for tests and demonstrations).
    Virtual,
    /// A TCP connection (a Hamlib `rotctld`, or a serial server in front of a GS-232 controller),
    /// reconnected when it is lost.
    Tcp {
        /// `host:port`.
        address: String,
    },
}

/// One rotator to serve.
#[derive(Debug, Clone, PartialEq)]
pub struct RotatorConfig {
    /// The name used in logs.
    pub name: String,
    /// The description, from the shipped rotator descriptions.
    pub description: RotatorDescription,
    /// Where clients connect.
    pub listen: SocketAddr,
    /// How to reach the rotator.
    pub transport: RotatorTransportConfig,
}

/// A validated configuration.
#[derive(Debug, Clone, PartialEq)]
pub struct Config {
    /// Server-wide settings.
    pub server: ServerConfig,
    /// The rigs, in file order.
    pub rigs: Vec<RigConfig>,
    /// The rotators, in file order.
    pub rotators: Vec<RotatorConfig>,
}

// [@ANCHOR: radioctld_config_in_range]
/// A value checked against an inclusive range, naming the key in the error.
fn in_range<T: PartialOrd + std::fmt::Display + Copy>(
    key: &str,
    value: Option<T>,
    default: T,
    low: T,
    high: T,
) -> Result<T, String> {
    let value = value.unwrap_or(default);
    if value < low || value > high {
        return Err(format!("{key} = {value} is outside {low}..={high}"));
    }
    Ok(value)
}

// [@ANCHOR: radioctld_config_server]
/// Validates the `[server]` section.
fn server_config(section: ServerSection) -> Result<(ServerConfig, bool), String> {
    let explicit_allow = section.allow.is_some();
    let allow = match &section.allow {
        Some(entries) => AllowList::parse(entries)?,
        None => AllowList::loopback_only(),
    };
    let server = ServerConfig {
        allow,
        max_clients: in_range("max_clients", section.max_clients, 16, 1, 256)?,
        idle_timeout: Duration::from_secs(in_range(
            "idle_timeout_secs",
            section.idle_timeout_secs,
            600,
            1,
            86_400,
        )?),
        line_timeout: Duration::from_secs(in_range(
            "line_timeout_secs",
            section.line_timeout_secs,
            10,
            1,
            300,
        )?),
        max_line_bytes: in_range("max_line_bytes", section.max_line_bytes, 1024, 64, 65_536)?,
        request_timeout: Duration::from_millis(in_range(
            "request_timeout_ms",
            section.request_timeout_ms,
            2000,
            100,
            30_000,
        )?),
        cache_max_age: Duration::from_millis(in_range(
            "cache_max_age_ms",
            section.cache_max_age_ms,
            1000,
            0,
            60_000,
        )?),
    };
    if section.allow_non_loopback_bind && !explicit_allow {
        return Err("allow_non_loopback_bind needs an explicit allow list".to_string());
    }
    Ok((server, section.allow_non_loopback_bind))
}

// [@ANCHOR: radioctld_config_transport]
/// Validates a rig's `[rig.transport]`.
fn transport_config(section: TransportSection) -> Result<TransportConfig, String> {
    match section {
        TransportSection::Virtual { reply_delay_ms } => Ok(TransportConfig::Virtual {
            reply_delay: Duration::from_millis(in_range(
                "reply_delay_ms",
                reply_delay_ms,
                0,
                0,
                10_000,
            )?),
        }),
        TransportSection::Serial {
            path,
            usb,
            baud,
            lines,
        } => {
            let target = match (path, usb) {
                (Some(path), None) if !path.is_empty() => PortTarget::Path(PathBuf::from(path)),
                (None, Some(usb)) => PortTarget::Usb(UsbMatcher {
                    vid: usb.vid,
                    pid: usb.pid,
                    serial: usb.serial,
                }),
                _ => return Err("serial transport needs exactly one of path and usb".to_string()),
            };
            if !(300..=4_000_000).contains(&baud) {
                return Err(format!("baud {baud} is outside 300..=4000000"));
            }
            let lines = match lines.as_deref() {
                None | Some("managed") => LineControl::Managed,
                Some("unmanaged") => LineControl::Unmanaged,
                Some(other) => {
                    return Err(format!(
                        "serial lines {other:?} is not \"managed\" or \"unmanaged\""
                    ))
                }
            };
            Ok(TransportConfig::Serial {
                target,
                baud,
                lines,
            })
        }
        TransportSection::Tcp { address } => {
            check_host_port(&address)?;
            Ok(TransportConfig::Tcp { address })
        }
    }
}

// [@ANCHOR: radioctld_config_check_name]
/// Checks a rig's or rotator's name: 1 to 32 of `[A-Za-z0-9_.-]`.
fn check_name(kind: &str, name: &str) -> Result<(), String> {
    if name.is_empty()
        || name.len() > 32
        || !name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
    {
        return Err(format!(
            "{kind} name {name:?} must be 1-32 of [A-Za-z0-9_.-]"
        ));
    }
    Ok(())
}

// [@ANCHOR: radioctld_config_listen_address]
/// The listen address of a rig or rotator: the one given, or loopback at `base + index`.
/// `non_loopback_ok` is the server's explicit permission to bind beyond this machine.
fn listen_address(
    kind: &str,
    name: &str,
    given: Option<&String>,
    base: u16,
    index: usize,
    non_loopback_ok: bool,
) -> Result<SocketAddr, String> {
    let listen: SocketAddr = match given {
        Some(text) => text
            .parse()
            .map_err(|_| format!("{kind} {name}: listen {text:?} is not ip:port"))?,
        None => {
            let port = u16::try_from(index)
                .ok()
                .and_then(|i| base.checked_add(i))
                .ok_or(format!("too many {kind}s for the default ports"))?;
            SocketAddr::new(IpAddr::from([127, 0, 0, 1]), port)
        }
    };
    if !listen.ip().is_loopback() && !non_loopback_ok {
        return Err(format!(
            "{kind} {name}: {listen} is not a loopback address; set server.allow_non_loopback_bind \
             = true with an explicit server.allow list to serve the network"
        ));
    }
    Ok(listen)
}

// [@ANCHOR: radioctld_config_rotator]
/// Validates one `[[rotator]]`: a known model from the shipped descriptions, a listen address
/// (default: loopback, port 4533 + index) and a transport.
fn rotator_config(
    section: RotatorSection,
    index: usize,
    non_loopback_ok: bool,
) -> Result<RotatorConfig, String> {
    let name = section.name;
    check_name("rotator", &name)?;
    let listen = listen_address(
        "rotator",
        &name,
        section.listen.as_ref(),
        ROTATOR_BASE_PORT,
        index,
        non_loopback_ok,
    )?;
    let known = RotatorDescription::builtin();
    let description = known
        .iter()
        .find(|d| d.id == section.model)
        .cloned()
        .ok_or_else(|| {
            let ids: Vec<&str> = known.iter().map(|d| d.id.as_str()).collect();
            format!(
                "rotator {name}: model {:?} is not one of {}",
                section.model,
                ids.join(", ")
            )
        })?;
    let transport = match section.transport {
        RotatorTransportSection::Virtual => RotatorTransportConfig::Virtual,
        RotatorTransportSection::Tcp { address } => {
            check_host_port(&address)?;
            RotatorTransportConfig::Tcp { address }
        }
    };
    Ok(RotatorConfig {
        name,
        description,
        listen,
        transport,
    })
}

// [@ANCHOR: radioctld_config_check_host_port]
/// Checks that a TCP address is `host:port`.
fn check_host_port(address: &str) -> Result<(), String> {
    if address
        .rsplit_once(':')
        .is_none_or(|(h, p)| h.is_empty() || p.parse::<u16>().is_err())
    {
        return Err(format!("tcp address {address:?} is not host:port"));
    }
    Ok(())
}

// [@ANCHOR: radioctld_config_rig]
/// Validates one `[[rig]]`. `index` picks the default port; `non_loopback_ok` is the server's
/// explicit permission to bind beyond this machine.
fn rig_config(
    section: RigSection,
    index: usize,
    base_dir: &Path,
    non_loopback_ok: bool,
) -> Result<RigConfig, String> {
    let name = section.name;
    check_name("rig", &name)?;
    let listen = listen_address(
        "rig",
        &name,
        section.listen.as_ref(),
        BASE_PORT,
        index,
        non_loopback_ok,
    )?;
    let rx_low_hz = section.rx_low_hz.unwrap_or(30_000);
    let rx_high_hz = section.rx_high_hz.unwrap_or(60_000_000);
    if rx_low_hz >= rx_high_hz || rx_high_hz > crate::command::MAX_HZ {
        return Err(format!("rig {name}: rx_low_hz must be below rx_high_hz"));
    }
    let description = Path::new(&section.description);
    Ok(RigConfig {
        listen,
        vfo_mode: section.vfo_mode,
        transmit: section.transmit,
        hamlib_model: section.hamlib_model.unwrap_or(1),
        rx_low_hz,
        rx_high_hz,
        tx_power_mw: in_range("tx_power_mw", section.tx_power_mw, 100_000, 1, 10_000_000)?,
        max_transmit: section
            .max_transmit_s
            .map(|s| in_range("max_transmit_s", Some(s), 1, 1, 3600))
            .transpose()?
            .map(Duration::from_secs),
        transport: transport_config(section.transport)?,
        description: if description.is_absolute() {
            description.to_path_buf()
        } else {
            base_dir.join(description)
        },
        name,
    })
}

impl Config {
    // [@ANCHOR: radioctld_config_parse]
    /// Parses and validates a configuration file's text. Relative description paths are taken
    /// from `base_dir` (the directory of the file).
    pub fn parse(text: &str, base_dir: &Path) -> Result<Config, String> {
        let doc: FileDoc = toml::from_str(text).map_err(|e| format!("config: {e}"))?;
        if doc.rig.is_empty() && doc.rotator.is_empty() {
            return Err("config: no [[rig]] or [[rotator]] to serve".to_string());
        }
        let (server, non_loopback_ok) = server_config(doc.server.unwrap_or_default())?;
        let mut rigs = Vec::new();
        for (index, section) in doc.rig.into_iter().enumerate() {
            rigs.push(rig_config(section, index, base_dir, non_loopback_ok)?);
        }
        let mut rotators = Vec::new();
        for (index, section) in doc.rotator.into_iter().enumerate() {
            rotators.push(rotator_config(section, index, non_loopback_ok)?);
        }
        // Names are unique within rigs and within rotators; listen addresses across both.
        for (i, rig) in rigs.iter().enumerate() {
            if rigs.iter().skip(i + 1).any(|other| other.name == rig.name) {
                return Err(format!("config: rig name {:?} is used twice", rig.name));
            }
        }
        for (i, rotator) in rotators.iter().enumerate() {
            if rotators
                .iter()
                .skip(i + 1)
                .any(|other| other.name == rotator.name)
            {
                return Err(format!(
                    "config: rotator name {:?} is used twice",
                    rotator.name
                ));
            }
        }
        let listens: Vec<SocketAddr> = rigs
            .iter()
            .map(|r| r.listen)
            .chain(rotators.iter().map(|r| r.listen))
            .collect();
        for (i, listen) in listens.iter().enumerate() {
            if listen.port() != 0 && listens.iter().skip(i + 1).any(|other| other == listen) {
                return Err(format!(
                    "config: {listen} is the address of two rigs or rotators; give one a listen \
                     address of its own"
                ));
            }
        }
        Ok(Config {
            server,
            rigs,
            rotators,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASE: &str = "/etc/radioctld";

    fn parse(text: &str) -> Result<Config, String> {
        Config::parse(text, Path::new(BASE))
    }

    const MINIMAL: &str = r#"
[[rig]]
name = "kx3"
description = "rigs/elecraft-kx3.toml"
[rig.transport]
kind = "virtual"
"#;

    // Tests [@ANCHOR: radioctld_config_parse]
    // Tests [@ANCHOR: radioctld_config_server]
    // Tests [@ANCHOR: radioctld_config_rig]
    // Tests [@ANCHOR: radioctld_config_transport]
    // Tests [@ANCHOR: radioctld_config_in_range]
    #[test]
    fn a_minimal_file_gets_safe_defaults() {
        let config = parse(MINIMAL).expect("config");
        assert_eq!(config.rigs.len(), 1);
        let rig = &config.rigs[0];
        assert_eq!(rig.listen, "127.0.0.1:4532".parse().expect("addr"));
        assert!(!rig.transmit, "the transmit gate stays closed by default");
        assert!(!rig.vfo_mode);
        assert_eq!(
            rig.description,
            Path::new(BASE).join("rigs/elecraft-kx3.toml")
        );
        assert_eq!(config.server.allow, AllowList::loopback_only());
        assert_eq!(config.server.max_line_bytes, 1024);
        assert_eq!(config.server.max_clients, 16);
    }

    // Tests [@ANCHOR: radioctld_config_rig]
    // Tests [@ANCHOR: radioctld_config_in_range]
    #[test]
    fn the_transmit_time_limit_is_optional_and_bounded() {
        let config = parse(MINIMAL).expect("config");
        assert_eq!(
            config.rigs[0].max_transmit, None,
            "the library default applies"
        );
        let with_limit = MINIMAL.replace("name = \"kx3\"", "name = \"kx3\"\nmax_transmit_s = 60");
        let config = parse(&with_limit).expect("config");
        assert_eq!(config.rigs[0].max_transmit, Some(Duration::from_secs(60)));
        for bad in ["0", "3601"] {
            let text = MINIMAL.replace(
                "name = \"kx3\"",
                &format!("name = \"kx3\"\nmax_transmit_s = {bad}"),
            );
            assert!(parse(&text).is_err(), "max_transmit_s = {bad} is refused");
        }
    }

    // Tests [@ANCHOR: radioctld_config_rig]
    // Tests [@ANCHOR: radioctld_config_server]
    #[test]
    fn serving_beyond_loopback_needs_both_the_option_and_an_allow_list() {
        let open = MINIMAL.replace("[[rig]]", "[[rig]]\nlisten = \"0.0.0.0:4532\"");
        let err = parse(&open).expect_err("refused");
        assert!(err.contains("not a loopback address"), "{err}");
        let flag_only = format!("[server]\nallow_non_loopback_bind = true\n{open}");
        let err = parse(&flag_only).expect_err("refused");
        assert!(err.contains("explicit allow list"), "{err}");
        let both = format!(
            "[server]\nallow_non_loopback_bind = true\nallow = [\"192.168.1.0/24\"]\n{open}"
        );
        let config = parse(&both).expect("allowed");
        assert!(config
            .server
            .allow
            .allows("192.168.1.9".parse().expect("ip")));
        assert!(!config.server.allow.allows("127.0.0.1".parse().expect("ip")));
    }

    // Tests [@ANCHOR: radioctld_config_parse]
    // Tests [@ANCHOR: radioctld_config_in_range]
    #[test]
    fn unknown_keys_bad_values_and_duplicates_are_errors() {
        assert!(parse("").is_err());
        assert!(parse(&MINIMAL.replace("name = \"kx3\"", "name = \"kx3\"\nbogus = 1")).is_err());
        assert!(parse(&format!("[server]\nmax_clients = 0\n{MINIMAL}")).is_err());
        assert!(parse(&format!("[server]\nmax_line_bytes = 10\n{MINIMAL}")).is_err());
        assert!(parse(&MINIMAL.replace("\"kx3\"", "\"a b\"")).is_err());
        let twice = format!("{MINIMAL}{MINIMAL}");
        assert!(parse(&twice).expect_err("dup").contains("used twice"));
        let same_port = format!(
            "{MINIMAL}{}",
            MINIMAL
                .replace("\"kx3\"", "\"other\"")
                .replace("[[rig]]", "[[rig]]\nlisten = \"127.0.0.1:4532\"")
        );
        assert!(parse(&same_port).is_err());
        let two = format!("{MINIMAL}{}", MINIMAL.replace("\"kx3\"", "\"other\""));
        let config = parse(&two).expect("two rigs");
        assert_eq!(config.rigs[1].listen.port(), 4533);
    }

    const ROTATOR: &str = r#"
[[rotator]]
name = "g5500"
model = "yaesu-g-5500-gs232b"
[rotator.transport]
kind = "virtual"
"#;

    // Tests [@ANCHOR: radioctld_config_rotator]
    // Tests [@ANCHOR: radioctld_config_check_name]
    // Tests [@ANCHOR: radioctld_config_listen_address]
    // Tests [@ANCHOR: radioctld_config_check_host_port]
    // Tests [@ANCHOR: radioctld_config_parse]
    #[test]
    fn rotators_are_validated_and_share_the_address_space_with_rigs() {
        // A file of rotators only is enough to serve.
        let config = parse(ROTATOR).expect("rotators only");
        assert!(config.rigs.is_empty());
        let rotator = &config.rotators[0];
        assert_eq!(rotator.name, "g5500");
        assert_eq!(rotator.description.id, "yaesu-g-5500-gs232b");
        assert_eq!(rotator.listen, "127.0.0.1:4533".parse().expect("addr"));
        assert_eq!(rotator.transport, RotatorTransportConfig::Virtual);
        // The second rotator takes the next port.
        let two = format!("{ROTATOR}{}", ROTATOR.replace("\"g5500\"", "\"other\""));
        assert_eq!(parse(&two).expect("two").rotators[1].listen.port(), 4534);
        // Rigs and rotators share one address space: a second rig would default to 4533.
        let clash = format!("{MINIMAL}{}{ROTATOR}", MINIMAL.replace("\"kx3\"", "\"k2\""));
        let err = parse(&clash).expect_err("clash");
        assert!(err.contains("4533") && err.contains("listen"), "{err}");
        // Ephemeral ports never clash.
        let ephemeral = format!(
            "{}{}",
            MINIMAL.replace("[[rig]]", "[[rig]]\nlisten = \"127.0.0.1:0\""),
            ROTATOR.replace("[[rotator]]", "[[rotator]]\nlisten = \"127.0.0.1:0\"")
        );
        assert!(parse(&ephemeral).is_ok());
        // Unknown model, bad name, duplicate name, non-loopback bind, bad transport.
        let err = parse(&ROTATOR.replace("yaesu-g-5500-gs232b", "acme")).expect_err("model");
        assert!(err.contains("generic-rotctld"), "{err}");
        assert!(parse(&ROTATOR.replace("\"g5500\"", "\"a b\"")).is_err());
        assert!(parse(&format!("{ROTATOR}{ROTATOR}"))
            .expect_err("dup")
            .contains("used twice"));
        assert!(
            parse(&ROTATOR.replace("[[rotator]]", "[[rotator]]\nlisten = \"0.0.0.0:4533\""))
                .expect_err("bind")
                .contains("not a loopback address")
        );
        let tcp = ROTATOR.replace(
            "kind = \"virtual\"",
            "kind = \"tcp\"\naddress = \"192.168.1.9:4533\"",
        );
        assert_eq!(
            parse(&tcp).expect("tcp").rotators[0].transport,
            RotatorTransportConfig::Tcp {
                address: "192.168.1.9:4533".to_string()
            }
        );
        assert!(
            parse(&ROTATOR.replace("kind = \"virtual\"", "kind = \"tcp\"\naddress = \"x\""))
                .is_err()
        );
        assert!(parse(&ROTATOR.replace("kind = \"virtual\"", "kind = \"serial\"")).is_err());
        assert!(parse(&ROTATOR.replace("model =", "bogus = 1\nmodel =")).is_err());
    }

    // Tests [@ANCHOR: radioctld_config_transport]
    #[test]
    fn transports_are_validated() {
        let with = |transport: &str| parse(&MINIMAL.replace("kind = \"virtual\"", transport));
        assert!(with("kind = \"serial\"\npath = \"/dev/ttyUSB0\"\nbaud = 38400").is_ok());
        assert!(
            with("kind = \"serial\"\nusb = { vid = 0x0403, pid = 0x6001 }\nbaud = 38400").is_ok()
        );
        assert!(with("kind = \"serial\"\nbaud = 38400").is_err());
        assert!(with(
            "kind = \"serial\"\npath = \"/dev/x\"\nusb = { vid = 1, pid = 2 }\nbaud = 9600"
        )
        .is_err());
        assert!(with("kind = \"serial\"\npath = \"/dev/x\"\nbaud = 5").is_err());
        // `lines` says whether the port has modem lines; managed unless told otherwise.
        let lines_of = |extra: &str| {
            let text = format!("kind = \"serial\"\npath = \"/dev/x\"\nbaud = 19200\n{extra}");
            match parse(&MINIMAL.replace("kind = \"virtual\"", &text)) {
                Ok(config) => match &config.rigs[0].transport {
                    TransportConfig::Serial { lines, .. } => Some(*lines),
                    _ => None,
                },
                Err(_) => None,
            }
        };
        assert_eq!(lines_of(""), Some(LineControl::Managed));
        assert_eq!(lines_of("lines = \"managed\""), Some(LineControl::Managed));
        assert_eq!(
            lines_of("lines = \"unmanaged\""),
            Some(LineControl::Unmanaged)
        );
        assert_eq!(lines_of("lines = \"maybe\""), None);
        assert!(with("kind = \"tcp\"\naddress = \"10.0.0.7:4532\"").is_ok());
        assert!(with("kind = \"tcp\"\naddress = \"nonsense\"").is_err());
        assert!(with("kind = \"carrier-pigeon\"").is_err());
    }
}
