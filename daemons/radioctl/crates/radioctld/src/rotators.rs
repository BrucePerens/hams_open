// SPDX-License-Identifier: AGPL-3.0-or-later
//! Building the rotators a configuration names: the description's protocol engine, a reconnecting
//! network link (`radioctl_core::net::open_link`) to the rotator or to a virtual rotator served
//! inside the daemon on a loopback socket, and the `Rotator` handle with the link's status so a
//! command on a down link fails at once without writing. No runtime is created here. Nothing is
//! verified against a real rotator.

use std::sync::Arc;

use radioctl_core::net::{open_link, NetConfig, NetMonitor, RawFraming};
use radioctl_station::rotator::Rotator;
use radioctl_station::virtual_rotator::{
    serve_virtual_rotator, VirtualRotator, VirtualRotatorControl,
};
use tokio::net::TcpListener;
use tokio::runtime::Handle;

use crate::config::{RotatorConfig, RotatorTransportConfig};
use crate::rotctl::RotatorService;

/// A rotator that is up: its service, and the control of its virtual stand-in if it is one.
pub struct BuiltRotator {
    /// The service clients talk to.
    pub service: Arc<RotatorService>,
    /// Control of the virtual rotator behind the service (position, commands seen, faults), for
    /// virtual rotators.
    pub virtual_control: Option<VirtualRotatorControl>,
    /// The network link's status and counters. Kept for the life of the rotator.
    pub net_link: Option<NetMonitor>,
}

// [@ANCHOR: radioctld_rotators_build_rotator]
/// Brings up one rotator on `runtime`: engine, link, handle, service. A virtual rotator is served
/// on a loopback socket of its own (port chosen by the kernel) and reached over the same
/// reconnecting link a real one is. The first connection is one attempt: a rotator that cannot be
/// reached stops the daemon from starting, as a rig does.
pub async fn build_rotator(
    runtime: &Handle,
    rotator: &RotatorConfig,
) -> Result<BuiltRotator, String> {
    let fail = |e: &dyn std::fmt::Display| format!("rotator {}: {e}", rotator.name);
    let engine = rotator.description.engine().map_err(|e| fail(&e))?;
    let mut virtual_control = None;
    let address = match &rotator.transport {
        RotatorTransportConfig::Virtual => {
            let listener = TcpListener::bind("127.0.0.1:0")
                .await
                .map_err(|e| fail(&e))?;
            let address = listener.local_addr().map_err(|e| fail(&e))?.to_string();
            let virtual_rotator = VirtualRotator::new(rotator.description.clone());
            virtual_control = Some(serve_virtual_rotator(runtime, listener, virtual_rotator));
            address
        }
        RotatorTransportConfig::Tcp { address } => address.clone(),
    };
    let mut config = NetConfig::new(address);
    config.first_attempts = 1;
    let link = open_link(runtime, config, Box::new(RawFraming))
        .await
        .map_err(|e| fail(&e))?;
    let watch = link.watch();
    let (transport, monitor) = link.into_parts();
    let handle = Rotator::new(engine, transport, Some(watch));
    Ok(BuiltRotator {
        service: Arc::new(RotatorService::new(handle)),
        virtual_control,
        net_link: Some(monitor),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    // Tests [@ANCHOR: radioctld_rotators_build_rotator]
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn an_unreachable_rotator_is_an_error_naming_it() {
        // A closed loopback port: the connection is refused at once.
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let address = listener.local_addr().expect("addr").to_string();
        drop(listener);
        let text = format!(
            "[[rotator]]\nname = \"far\"\nmodel = \"generic-rotctld\"\nlisten = \"127.0.0.1:0\"\n\
             [rotator.transport]\nkind = \"tcp\"\naddress = \"{address}\"\n"
        );
        let config = crate::config::Config::parse(&text, Path::new(".")).expect("config");
        let err = build_rotator(&Handle::current(), &config.rotators[0])
            .await
            .err()
            .expect("refused");
        assert!(err.starts_with("rotator far:"), "{err}");
    }
}
