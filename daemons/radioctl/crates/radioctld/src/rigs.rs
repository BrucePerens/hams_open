// SPDX-License-Identifier: AGPL-3.0-or-later
//! Building the rigs a configuration names: read the description, build its protocol engine, open
//! its transport, spawn the rig actor on the runtime the caller passes in. No runtime is created
//! here. A virtual rig is the same path with an in-memory transport, which is also what the tests
//! use.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use radioctl_core::description::RigDescription;
use radioctl_core::engine::{ProtocolEngine, Request};
use radioctl_core::error::RigError;
use radioctl_core::op::Op;
use radioctl_core::rig::{spawn, RigHandle, RigOptions, TransmitPermit};
use radioctl_core::rigfile;
use radioctl_core::serial::{open_serial, SerialConfig};
use radioctl_core::tcp::{connect_tcp, TcpConfig};
use radioctl_protocols::conformance::engine_for;
use radioctl_protocols::io::{spawn_virtual_rig, VirtualRigControl};
use radioctl_protocols::virtual_rig::VirtualRig;
use tokio::runtime::Handle;

use crate::config::{RigConfig, ServerConfig, TransportConfig};
use crate::service::{RigService, ServiceConfig};

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
/// Brings up one rig on `runtime`: engine, transport, actor, service. The transmit gate is
/// opened only when the configuration says `transmit = true`.
pub async fn build_rig(
    runtime: &Handle,
    rig: &RigConfig,
    description: RigDescription,
    server: &ServerConfig,
    options: RigOptions,
) -> Result<BuiltRig, String> {
    let engine = BoxedEngine(engine_for(&description).map_err(|e| format!("{}: {e}", rig.name))?);
    let fail = |e: RigError| format!("rig {}: {e}", rig.name);
    let mut virtual_control = None;
    let handle: RigHandle = match &rig.transport {
        TransportConfig::Virtual { reply_delay } => {
            let virtual_rig = VirtualRig::from_description(&description).map_err(fail)?;
            let (stream, control) = spawn_virtual_rig(runtime, virtual_rig, *reply_delay);
            virtual_control = Some(control);
            spawn(runtime, engine, stream, options).map_err(fail)?
        }
        TransportConfig::Serial { target, baud } => {
            let link = open_serial(runtime, SerialConfig::new(target.clone(), *baud))
                .await
                .map_err(fail)?;
            spawn(runtime, engine, link.transport, options).map_err(fail)?
        }
        TransportConfig::Tcp { address } => {
            let stream = connect_tcp(address.as_str(), &TcpConfig::default())
                .await
                .map_err(fail)?;
            spawn(runtime, engine, stream, options).map_err(fail)?
        }
    };
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
    })
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
