// SPDX-License-Identifier: AGPL-3.0-or-later
//! Building the rigs a configuration names: read the description, build its protocol engine, open
//! its transport, spawn the rig actor on the runtime the caller passes in. No runtime is created
//! here. A virtual rig is the same path with an in-memory transport, which is also what the tests
//! use. A TCP rig is a reconnecting network link (`radioctl_core::net::open_link`), raw or inside
//! the Kenwood TS-890S LAN session, whose password comes from the server's secret store at start
//! and is held only in the framing's `LanCredentials` (its `Debug` hides it). Nothing here is
//! verified against a real radio.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use radioctl_core::baud_probe::{probe, rates_text, ProbeResult, SerialProbePort};
use radioctl_core::capability::Capability;
use radioctl_core::description::RigDescription;
use radioctl_core::engine::{ProtocolEngine, Request};
use radioctl_core::error::RigError;
use radioctl_core::net::{open_link, Framing, NetConfig, NetMonitor, RawFraming};
use radioctl_core::op::Op;
use radioctl_core::rig::{spawn, RigHandle, RigOptions, TransmitPermit};
use radioctl_core::rigfile;
use radioctl_core::serial::{open_serial, LineControl, PortTarget, SerialConfig, SerialMonitor};
use radioctl_protocols::conformance::engine_for;
use radioctl_protocols::io::{spawn_virtual_rig, VirtualRigControl};
use radioctl_protocols::kenwood_ts890_lan::{LanCredentials, Ts890LanFraming, KEEPALIVE_EVERY};
use radioctl_protocols::virtual_rig::VirtualRig;
use radioctl_station::homeassistant::{FileSecrets, SecretStore};
use tokio::runtime::Handle;

use crate::config::{BaudSetting, RigConfig, ServerConfig, TcpFraming, TransportConfig};
use crate::service::{RigService, ServiceConfig};

/// The rate in the serial configuration the baud probe is handed; it replaces it at every open.
const PROBE_TEMPLATE_BAUD: u32 = 9600;

/// A protocol engine behind a box, so one `spawn` serves every family.
struct BoxedEngine(Box<dyn ProtocolEngine>);

impl ProtocolEngine for BoxedEngine {
    fn description(&self) -> &RigDescription {
        self.0.description()
    }

    // [@ANCHOR: radioctld_rigs_boxed_encode]
    fn encode(&mut self, op: &Op) -> Result<Request, RigError> {
        self.0.encode(op)
    }

    // [@ANCHOR: radioctld_rigs_boxed_feed]
    fn feed(&mut self, bytes: &[u8], now: Duration) -> Vec<radioctl_core::engine::Inbound> {
        self.0.feed(bytes, now)
    }
}

/// A rig that is up: its service, and the control of its virtual stand-in if it is one.
pub struct BuiltRig {
    /// The service clients talk to.
    pub service: Arc<RigService>,
    /// Control of the virtual rig behind the service (state, front panel), for virtual rigs.
    pub virtual_control: Option<VirtualRigControl>,
    /// The serial port's control lines, status and counters, for serial rigs. Kept for the life
    /// of the rig: the actor's link watch and the keyer's lines depend on it.
    pub serial_link: Option<SerialMonitor>,
    /// The network link's status and counters, for TCP rigs.
    pub net_link: Option<NetMonitor>,
    /// What the baud probe found, when the configuration asked for one (`baud = "auto"` or
    /// `baud_probe`); `None` for a fixed rate, which is the default.
    pub baud_probe: Option<ProbeResult>,
}

// [@ANCHOR: radioctld_rigs_tcp_framing]
/// The framing of a TCP rig and the silence limit that goes with it. For the TS-890S LAN
/// session the password is read now from the secret store named by `server.secrets_dir` (the
/// file must be the owner's alone); the error for a missing or unreadable secret names the
/// secret, never a value. Without an `idle_timeout` of its own, the session's limit is three
/// keep-alive intervals (the radio itself hangs up after 10 s of silence); a raw stream has none.
pub fn tcp_framing(
    rig: &RigConfig,
    server: &ServerConfig,
    framing: &TcpFraming,
    idle_timeout: Option<Duration>,
) -> Result<(Box<dyn Framing>, Option<Duration>), String> {
    match framing {
        TcpFraming::Raw => Ok((Box::new(RawFraming), idle_timeout)),
        TcpFraming::Ts890Lan {
            account,
            administrator,
            password_secret,
        } => {
            let dir = server.secrets_dir.as_ref().ok_or_else(|| {
                format!(
                    "rig {}: the ts890-lan framing needs server.secrets_dir",
                    rig.name
                )
            })?;
            let password = FileSecrets::new(dir)
                .secret(password_secret)
                .map_err(|e| format!("rig {}: secret {password_secret}: {}", rig.name, e.detail))?;
            let credentials = LanCredentials::new(*administrator, account, password.expose())
                .map_err(|e| {
                    format!(
                        "rig {}: LAN account {account:?} or its password cannot be sent: {}",
                        rig.name, e.detail
                    )
                })?;
            Ok((
                Box::new(Ts890LanFraming::new(credentials)),
                Some(idle_timeout.unwrap_or(KEEPALIVE_EVERY * 3)),
            ))
        }
    }
}

// [@ANCHOR: radioctld_rigs_load_description]
/// Reads and validates a rig description file.
pub fn load_description(path: &Path) -> Result<RigDescription, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    rigfile::parse(&text)
        .map(|loaded| loaded.description)
        .map_err(|e| format!("{}: {e}", path.display()))
}

// [@ANCHOR: radioctld_rigs_service_config]
/// The per-rig service settings from the configuration.
pub fn service_config(rig: &RigConfig, server: &ServerConfig) -> ServiceConfig {
    ServiceConfig {
        vfo_mode: rig.vfo_mode,
        cache_max_age: server.cache_max_age,
        request_timeout: server.request_timeout,
        hamlib_model: rig.hamlib_model,
        rx_low_hz: rig.rx_low_hz,
        rx_high_hz: rig.rx_high_hz,
        tx_power_mw: rig.tx_power_mw,
    }
}

// [@ANCHOR: radioctld_rigs_build_rig]
/// Brings up one rig on `runtime`: engine, transport, actor, service. The radio is sent an unkey
/// first (a killed predecessor may have left it keyed). The transmit gate is opened only when the
/// configuration says `transmit = true`.
pub async fn build_rig(
    runtime: &Handle,
    rig: &RigConfig,
    description: RigDescription,
    server: &ServerConfig,
    options: RigOptions,
) -> Result<BuiltRig, String> {
    let engine = BoxedEngine(engine_for(&description).map_err(|e| format!("{}: {e}", rig.name))?);
    let mut options = options;
    if let Some(limit) = rig.max_transmit {
        options.max_transmit = limit;
    }
    let fail = |e: RigError| format!("rig {}: {e}", rig.name);
    let mut virtual_control = None;
    let mut serial_link = None;
    let mut net_link = None;
    let mut baud_probe = None;
    let handle: RigHandle = match &rig.transport {
        TransportConfig::Virtual { reply_delay } => {
            let virtual_rig = VirtualRig::from_description(&description).map_err(fail)?;
            let (stream, control) = spawn_virtual_rig(runtime, virtual_rig, *reply_delay);
            virtual_control = Some(control);
            spawn(runtime, engine, stream, options).map_err(fail)?
        }
        TransportConfig::Serial {
            target,
            baud,
            lines,
        } => {
            let rate = match baud {
                BaudSetting::Fixed(rate) => *rate,
                BaudSetting::Probe(rates) => {
                    // The probe opens the port at each rate with the same line handling as the
                    // real link, sends the engine's own frequency read (a throwaway engine, so
                    // the one the actor gets starts clean), and stops at the first answer. It
                    // runs before the actor exists: the start-up unkey pass below still runs,
                    // at the rate found.
                    let mut probe_engine =
                        engine_for(&description).map_err(|e| format!("{}: {e}", rig.name))?;
                    // The template's rate is replaced at every open.
                    let template = serial_config(target, PROBE_TEMPLATE_BAUD, *lines);
                    let mut port = SerialProbePort::new(runtime.clone(), template);
                    let found = probe(&mut port, &mut probe_engine, rates)
                        .await
                        .map_err(|e| format!("rig {}: baud probe: {e}", rig.name))?;
                    eprintln!(
                        "radioctld: rig {}: baud probe chose {} (tried {})",
                        rig.name,
                        found.baud,
                        rates_text(&found.tried)
                    );
                    let rate = found.baud;
                    baud_probe = Some(found);
                    rate
                }
            };
            let serial = serial_config(target, rate, *lines);
            let link = open_serial(runtime, serial).await.map_err(fail)?;
            // The actor watches the port: a port loss takes the link down, a reopen unkeys the
            // rig before any other command (RigOptions::link_watch). The control lines and
            // the status stay with the service so the link object is never dropped early.
            options.link_watch = Some(link.watch());
            let (transport, serial) = link.into_parts();
            serial_link = Some(serial);
            spawn(runtime, engine, transport, options).map_err(fail)?
        }
        TransportConfig::Tcp {
            address,
            framing,
            idle_timeout,
        } => {
            let (framing, idle_timeout) = tcp_framing(rig, server, framing, *idle_timeout)?;
            let mut net = NetConfig::new(address.clone());
            net.first_attempts = 1;
            net.idle_timeout = idle_timeout;
            // One attempt at start, as for a serial port: a radio that cannot be reached, or
            // refuses the login, stops the daemon from starting. After that the link reconnects
            // without limit; the actor watches it, marks the link down meanwhile and unkeys the
            // rig when it is back (RigOptions::link_watch).
            let link = open_link(runtime, net, framing).await.map_err(fail)?;
            options.link_watch = Some(link.watch());
            let (transport, monitor) = link.into_parts();
            net_link = Some(monitor);
            spawn(runtime, engine, transport, options).map_err(fail)?
        }
    };
    // The unkey pass: a process that was killed while it had the radio keyed leaves it keyed
    // (nothing else would unkey it until its own time-out, or this actor's watchdog after a poll
    // sees it). Unkeying is always allowed and needs no gate, so every start begins with one. A
    // radio that cannot be unkeyed is not served: the failure is the first sign of a wrong
    // address or baud rate, and it may be keyed.
    if handle.capabilities().has(Capability::SetPtt) {
        handle.set_ptt(false).await.map_err(|e| {
            format!(
                "rig {}: the startup unkey failed, the radio may be keyed: {e}",
                rig.name
            )
        })?;
    }
    if rig.transmit {
        handle.open_transmit_gate(TransmitPermit::new(format!(
            "radioctld config: rig {} transmit = true",
            rig.name
        )));
    }
    let service = Arc::new(RigService::new(
        handle,
        description,
        service_config(rig, server),
    ));
    Ok(BuiltRig {
        service,
        virtual_control,
        serial_link,
        net_link,
        baud_probe,
    })
}

// [@ANCHOR: radioctld_rigs_serial_config]
/// The serial configuration of a rig at `baud`: 8N1, the configured line handling, one open
/// attempt. The probe and the real link use the same one, so the port is opened the same way.
fn serial_config(target: &PortTarget, baud: u32, lines: LineControl) -> SerialConfig {
    let mut serial = SerialConfig::new(target.clone(), baud);
    serial.lines = lines;
    serial
}

#[cfg(test)]
mod tests {
    use super::*;

    // Tests [@ANCHOR: radioctld_rigs_load_description]
    #[test]
    fn a_missing_or_broken_description_is_an_error_naming_the_file() {
        let err = load_description(Path::new("/nonexistent/rig.toml")).expect_err("missing");
        assert!(err.contains("/nonexistent/rig.toml"), "{err}");
    }
}
