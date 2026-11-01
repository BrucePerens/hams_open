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
//! secrets_dir = "/etc/radioctld/secrets"  # one file per secret, mode 0600; needed by ts890-lan
//!
//! [[rig]]
//! name = "kx3"
//! description = "rigs/elecraft-kx3.toml"   # relative to this file
//! listen = "127.0.0.1:4532"                # default: loopback, port 4532 + index
//! vfo_mode = false                         # rigctld's -o: \chk_vfo answers 1
//! transmit = false                         # true opens the rig's transmit gate at start
//! [rig.transport]
//! kind = "virtual"                         # or "serial" (path/usb + baud, lines; baud = "auto" or
//!                                          # baud_probe = [...] probes the rate with a frequency
//!                                          # read, off unless configured) or "tcp" (address,
//!                                          # framing = "raw" | "ts890-lan", lan_account,
//!                                          # lan_administrator, lan_password_secret, idle_timeout_ms)
//!                                          # or "usb" (a USB SDR board or dongle: optional vid, pid,
//!                                          # serial, transfer_timeout_ms; the description supplies
//!                                          # the ids and strings; needs a build with feature `usb`)
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

use radioctl_core::baud_probe::DEFAULT_RATES;
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
    secrets_dir: Option<String>,
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
        baud: Option<BaudSection>,
        baud_probe: Option<Vec<u32>>,
        lines: Option<String>,
    },
    Tcp {
        address: String,
        framing: Option<String>,
        lan_account: Option<String>,
        lan_administrator: Option<bool>,
        lan_password_secret: Option<String>,
        idle_timeout_ms: Option<u64>,
    },
    Usb {
        vid: Option<u16>,
        pid: Option<u16>,
        serial: Option<String>,
        transfer_timeout_ms: Option<u64>,
    },
}

/// `baud = 19200` or `baud = "auto"`.
#[derive(Deserialize)]
#[serde(untagged)]
enum BaudSection {
    Rate(u32),
    Word(String),
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
    /// The directory of secret files (one per secret, readable by the owner only), when a
    /// transport needs one. Relative to the configuration file.
    pub secrets_dir: Option<PathBuf>,
}

/// What a TCP transport wraps the CAT stream in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TcpFraming {
    /// Nothing: the CAT stream is the wire stream.
    Raw,
    /// The Kenwood TS-890S LAN session: `##CN;` connection request, `##ID` login, `ID;`
    /// keep-alive. The password is read from the secret store at start, never from this file.
    Ts890Lan {
        /// The LAN account name (not a secret).
        account: String,
        /// Whether the account is the administrator account.
        administrator: bool,
        /// The name of the secret (a file in `server.secrets_dir`) holding the password.
        password_secret: String,
    },
}

/// A serial rig's baud rate: the default is a fixed rate and nothing probes unless the file
/// says `baud = "auto"` or gives a `baud_probe` list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BaudSetting {
    /// Open the port at this rate.
    Fixed(u32),
    /// Find the rate with the read-only baud probe (`radioctl_core::baud_probe`), trying these
    /// rates in this order; the port is then opened at the first one that answered.
    Probe(Vec<u32>),
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
        /// The baud rate, fixed or probed.
        baud: BaudSetting,
        /// Whether the port has modem control lines this daemon may use.
        lines: LineControl,
    },
    /// A TCP connection (a network serial server or a radio with its own CAT port), reconnected
    /// with backoff after a loss.
    Tcp {
        /// `host:port`.
        address: String,
        /// The session around the CAT stream.
        framing: TcpFraming,
        /// Silence on the wire after which the link is declared lost and reconnected.
        idle_timeout: Option<Duration>,
    },
    /// A USB SDR board or dongle driven by vendor control transfers or HID reports
    /// (`radioctl_core::usb_link`), reopened with backoff after an unplug. The description names
    /// the vendor and product ids and strings; these keys narrow or override them.
    Usb {
        /// Vendor id override (default: the description's).
        vid: Option<u16>,
        /// Product id override (default: the description's).
        pid: Option<u16>,
        /// The serial number string, to tell two identical boards apart.
        serial: Option<String>,
        /// Longest wait for one USB transfer.
        transfer_timeout: Duration,
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
/// Validates the `[server]` section. `base_dir` resolves a relative `secrets_dir`.
fn server_config(section: ServerSection, base_dir: &Path) -> Result<(ServerConfig, bool), String> {
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
        secrets_dir: match section.secrets_dir.as_deref() {
            None => None,
            Some("") => return Err("secrets_dir is empty".to_string()),
            Some(dir) => {
                let dir = Path::new(dir);
                Some(if dir.is_absolute() {
                    dir.to_path_buf()
                } else {
                    base_dir.join(dir)
                })
            }
        },
    };
    if section.allow_non_loopback_bind && !explicit_allow {
        return Err("allow_non_loopback_bind needs an explicit allow list".to_string());
    }
    Ok((server, section.allow_non_loopback_bind))
}

// [@ANCHOR: radioctld_config_tcp_framing]
/// Validates a tcp transport's `framing` and the `lan_*` keys: `raw` (the default) takes none of
/// them; `ts890-lan` needs `lan_account` and `lan_password_secret` (a secret name, never the
/// password) and a `server.secrets_dir` to read it from.
fn tcp_framing(
    framing: Option<&str>,
    lan_account: Option<String>,
    lan_administrator: Option<bool>,
    lan_password_secret: Option<String>,
    has_secrets_dir: bool,
) -> Result<TcpFraming, String> {
    match framing.unwrap_or("raw") {
        "raw" => {
            if lan_account.is_some() || lan_administrator.is_some() || lan_password_secret.is_some()
            {
                return Err(
                    "lan_account, lan_administrator and lan_password_secret need framing = \
                     \"ts890-lan\""
                        .to_string(),
                );
            }
            Ok(TcpFraming::Raw)
        }
        "ts890-lan" => {
            let account = lan_account
                .filter(|a| !a.is_empty())
                .ok_or("framing = \"ts890-lan\" needs lan_account (the LAN account name)")?;
            let password_secret = lan_password_secret
                .filter(|s| !s.is_empty())
                .ok_or("framing = \"ts890-lan\" needs lan_password_secret (a secret name)")?;
            if !has_secrets_dir {
                return Err(
                    "framing = \"ts890-lan\" needs server.secrets_dir to read the password from"
                        .to_string(),
                );
            }
            Ok(TcpFraming::Ts890Lan {
                account,
                administrator: lan_administrator.unwrap_or(false),
                password_secret,
            })
        }
        other => Err(format!("framing {other:?} is not \"raw\" or \"ts890-lan\"")),
    }
}

// [@ANCHOR: radioctld_config_baud_rate]
/// One baud rate within 300..=4000000.
fn baud_rate(rate: u32) -> Result<u32, String> {
    if !(300..=4_000_000).contains(&rate) {
        return Err(format!("baud {rate} is outside 300..=4000000"));
    }
    Ok(rate)
}

// [@ANCHOR: radioctld_config_baud_setting]
/// A serial rig's `baud` and `baud_probe`: a number is a fixed rate and probes nothing (the
/// default behaviour); `baud = "auto"` probes `radioctl_core::baud_probe::DEFAULT_RATES`;
/// `baud_probe = [...]` probes that list in that order (with `baud` absent or `"auto"`). A fixed
/// rate together with a list, no rate at all, an empty or repeating list, or a rate out of range
/// is an error.
fn baud_setting(
    baud: Option<BaudSection>,
    baud_probe: Option<Vec<u32>>,
) -> Result<BaudSetting, String> {
    // (fixed rate, "auto" given)
    let given = match baud {
        None => (None, false),
        Some(BaudSection::Rate(rate)) => (Some(baud_rate(rate)?), false),
        Some(BaudSection::Word(word)) if word == "auto" => (None, true),
        Some(BaudSection::Word(word)) => {
            return Err(format!("baud {word:?} is not a rate or \"auto\""));
        }
    };
    match (given, baud_probe) {
        ((Some(rate), _), None) => Ok(BaudSetting::Fixed(rate)),
        ((Some(_), _), Some(_)) => {
            Err("baud_probe needs baud = \"auto\" or no baud, not a fixed rate".to_string())
        }
        ((None, false), None) => {
            Err("serial transport needs baud = RATE, baud = \"auto\" or baud_probe".to_string())
        }
        ((None, true), None) => Ok(BaudSetting::Probe(DEFAULT_RATES.to_vec())),
        ((None, _), Some(rates)) => {
            if rates.is_empty() {
                return Err("baud_probe must name at least one rate".to_string());
            }
            let mut seen = Vec::with_capacity(rates.len());
            for rate in &rates {
                let rate = baud_rate(*rate)?;
                if seen.contains(&rate) {
                    return Err(format!("baud_probe names {rate} twice"));
                }
                seen.push(rate);
            }
            Ok(BaudSetting::Probe(seen))
        }
    }
}

// [@ANCHOR: radioctld_config_transport]
/// Validates a rig's `[rig.transport]`. `has_secrets_dir` says whether the server has a secret
/// store for a framing that needs one.
fn transport_config(
    section: TransportSection,
    has_secrets_dir: bool,
) -> Result<TransportConfig, String> {
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
            baud_probe,
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
            let baud = baud_setting(baud, baud_probe)?;
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
        TransportSection::Tcp {
            address,
            framing,
            lan_account,
            lan_administrator,
            lan_password_secret,
            idle_timeout_ms,
        } => {
            check_host_port(&address)?;
            let framing = tcp_framing(
                framing.as_deref(),
                lan_account,
                lan_administrator,
                lan_password_secret,
                has_secrets_dir,
            )?;
            let idle_timeout = idle_timeout_ms
                .map(|ms| in_range("idle_timeout_ms", Some(ms), 1000, 1000, 600_000))
                .transpose()?
                .map(Duration::from_millis);
            Ok(TransportConfig::Tcp {
                address,
                framing,
                idle_timeout,
            })
        }
        TransportSection::Usb {
            vid,
            pid,
            serial,
            transfer_timeout_ms,
        } => {
            if serial.as_deref().is_some_and(str::is_empty) {
                return Err("usb transport serial must not be empty".to_string());
            }
            if vid == Some(0) || pid == Some(0) {
                return Err("usb transport vid and pid must not be 0".to_string());
            }
            Ok(TransportConfig::Usb {
                vid,
                pid,
                serial,
                transfer_timeout: Duration::from_millis(in_range(
                    "transfer_timeout_ms",
                    transfer_timeout_ms,
                    1000,
                    50,
                    10_000,
                )?),
            })
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
    has_secrets_dir: bool,
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
        transport: transport_config(section.transport, has_secrets_dir)?,
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
        let (server, non_loopback_ok) = server_config(doc.server.unwrap_or_default(), base_dir)?;
        let has_secrets_dir = server.secrets_dir.is_some();
        let mut rigs = Vec::new();
        for (index, section) in doc.rig.into_iter().enumerate() {
            rigs.push(rig_config(
                section,
                index,
                base_dir,
                non_loopback_ok,
                has_secrets_dir,
            )?);
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
    fn a_usb_transport_takes_optional_ids_a_serial_and_a_bounded_timeout() {
        let with = |transport: &str| parse(&MINIMAL.replace("kind = \"virtual\"", transport));
        let usb =
            |config: Result<Config, String>| match config.expect("usb").rigs[0].transport.clone() {
                TransportConfig::Usb {
                    vid,
                    pid,
                    serial,
                    transfer_timeout,
                } => (vid, pid, serial, transfer_timeout),
                other => panic!("not usb: {other:?}"),
            };
        assert_eq!(
            usb(with("kind = \"usb\"")),
            (None, None, None, Duration::from_millis(1000))
        );
        assert_eq!(
            usb(with(
                "kind = \"usb\"\nvid = 0x16C0\npid = 0x05DC\nserial = \"A12\"\ntransfer_timeout_ms = 250"
            )),
            (
                Some(0x16C0),
                Some(0x05DC),
                Some("A12".to_string()),
                Duration::from_millis(250)
            )
        );
        assert!(with("kind = \"usb\"\nserial = \"\"").is_err());
        assert!(with("kind = \"usb\"\nvid = 0").is_err());
        assert!(with("kind = \"usb\"\ntransfer_timeout_ms = 10").is_err());
        assert!(with("kind = \"usb\"\ntransfer_timeout_ms = 20000").is_err());
        assert!(
            with("kind = \"usb\"\nbaud = 9600").is_err(),
            "no serial keys on a USB transport"
        );
        assert!(with("kind = \"usb\"\naddress = \"x:1\"").is_err());
    }

    // Tests [@ANCHOR: radioctld_config_transport]
    // Tests [@ANCHOR: radioctld_config_baud_rate]
    // Tests [@ANCHOR: radioctld_config_baud_setting]
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
        // The baud probe is off unless configured: a number is a fixed rate and probes nothing.
        let baud_of = |baud: &str| {
            let text = format!("kind = \"serial\"\npath = \"/dev/x\"\n{baud}");
            match parse(&MINIMAL.replace("kind = \"virtual\"", &text)) {
                Ok(config) => match &config.rigs[0].transport {
                    TransportConfig::Serial { baud, .. } => Some(baud.clone()),
                    _ => None,
                },
                Err(_) => None,
            }
        };
        assert_eq!(baud_of("baud = 19200"), Some(BaudSetting::Fixed(19200)));
        assert_eq!(
            baud_of("baud = \"auto\""),
            Some(BaudSetting::Probe(DEFAULT_RATES.to_vec()))
        );
        assert_eq!(
            baud_of("baud_probe = [115200, 19200]"),
            Some(BaudSetting::Probe(vec![115200, 19200]))
        );
        assert_eq!(
            baud_of("baud = \"auto\"\nbaud_probe = [9600]"),
            Some(BaudSetting::Probe(vec![9600]))
        );
        assert_eq!(baud_of(""), None, "a serial rig needs a rate or the probe");
        assert_eq!(baud_of("baud = \"fast\""), None);
        assert_eq!(baud_of("baud = 19200\nbaud_probe = [9600]"), None);
        assert_eq!(baud_of("baud_probe = []"), None);
        assert_eq!(baud_of("baud_probe = [9600, 9600]"), None);
        assert_eq!(baud_of("baud_probe = [9600, 5]"), None);
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

    // Tests [@ANCHOR: radioctld_config_tcp_framing]
    // Tests [@ANCHOR: radioctld_config_server]
    // Tests [@ANCHOR: radioctld_config_transport]
    #[test]
    fn the_ts890_lan_framing_needs_an_account_a_secret_name_and_a_secret_store() {
        let tcp = |extra: &str, server: &str| {
            let text = format!("kind = \"tcp\"\naddress = \"192.168.1.50:60000\"\n{extra}");
            parse(&format!(
                "{server}{}",
                MINIMAL.replace("kind = \"virtual\"", &text)
            ))
        };
        let store = "[server]\nsecrets_dir = \"secrets\"\n";
        // Raw is the default; the lan keys belong to ts890-lan only.
        match &tcp("", "").expect("raw").rigs[0].transport {
            TransportConfig::Tcp {
                framing,
                idle_timeout,
                ..
            } => {
                assert_eq!(*framing, TcpFraming::Raw);
                assert_eq!(*idle_timeout, None);
            }
            other => panic!("{other:?}"),
        }
        assert!(tcp("framing = \"raw\"", "").is_ok());
        assert!(tcp("lan_account = \"kenwood\"", store).is_err());
        assert!(tcp("framing = \"icom-lan\"", store).is_err());
        // ts890-lan: account and secret name, plus a store. The password is never in the file.
        let full = "framing = \"ts890-lan\"\nlan_account = \"kenwood\"\nlan_administrator = true\n\
                    lan_password_secret = \"ts890-password\"\nidle_timeout_ms = 12000";
        let config = tcp(full, store).expect("ts890-lan");
        assert_eq!(
            config.server.secrets_dir,
            Some(Path::new(BASE).join("secrets"))
        );
        match &config.rigs[0].transport {
            TransportConfig::Tcp {
                framing,
                idle_timeout,
                ..
            } => {
                assert_eq!(
                    *framing,
                    TcpFraming::Ts890Lan {
                        account: "kenwood".to_string(),
                        administrator: true,
                        password_secret: "ts890-password".to_string(),
                    }
                );
                assert_eq!(*idle_timeout, Some(Duration::from_millis(12_000)));
            }
            other => panic!("{other:?}"),
        }
        let err = tcp(full, "").expect_err("no store");
        assert!(err.contains("secrets_dir"), "{err}");
        assert!(tcp(
            "framing = \"ts890-lan\"\nlan_password_secret = \"x\"",
            store
        )
        .is_err());
        assert!(tcp("framing = \"ts890-lan\"\nlan_account = \"kenwood\"", store).is_err());
        assert!(tcp("framing = \"ts890-lan\"\nlan_account = \"kenwood\"\nlan_password = \"admin\"\nlan_password_secret = \"x\"", store).is_err());
        assert!(tcp("idle_timeout_ms = 10", "").is_err());
        assert!(parse(&format!("[server]\nsecrets_dir = \"\"\n{MINIMAL}")).is_err());
        assert_eq!(
            parse(&format!(
                "[server]\nsecrets_dir = \"/run/secrets\"\n{MINIMAL}"
            ))
            .expect("absolute")
            .server
            .secrets_dir,
            Some(PathBuf::from("/run/secrets"))
        );
    }
}
