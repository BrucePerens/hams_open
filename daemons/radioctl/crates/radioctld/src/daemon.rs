// SPDX-License-Identifier: AGPL-3.0-or-later
//! The daemon proper: bring up every configured rig and its listener on the caller's runtime,
//! and take them down again. The binary supplies the runtime and decides when to stop.

use std::sync::Arc;

use radioctl_core::description::RigDescription;
use radioctl_core::rig::RigOptions;
use radioctl_protocols::io::VirtualRigControl;
use tokio::net::TcpListener;
use tokio::runtime::Handle;

use crate::config::Config;
use crate::server::{serve, Limits, ServerHandle};
use crate::service::RigService;
use radioctl_core::serial::SerialMonitor;

/// One rig being served.
pub struct Served {
    /// The rig's name from the configuration.
    pub name: String,
    /// The rig's service.
    pub service: Arc<RigService>,
    /// The listener.
    pub server: ServerHandle,
    /// Control of the virtual rig behind the service, for virtual rigs.
    pub virtual_control: Option<VirtualRigControl>,
    /// The serial port's control lines and status, for serial rigs; held so the port's
    /// supervisor and the actor's link watch live as long as the rig is served.
    pub serial_link: Option<SerialMonitor>,
}

/// The running daemon.
pub struct Daemon {
    /// Every rig being served, in configuration order.
    pub rigs: Vec<Served>,
}

// [@ANCHOR: radioctld_daemon_start]
/// Brings up every rig of `config` (descriptions given in the same order) and starts listening.
/// If anything fails, what was already started is stopped and the error is returned.
pub async fn start(
    handle: &Handle,
    config: &Config,
    descriptions: Vec<RigDescription>,
    options: impl Fn() -> RigOptions,
) -> Result<Daemon, String> {
    let mut daemon = Daemon { rigs: Vec::new() };
    let access = Arc::new(config.server.allow.clone());
    let limits = Limits::from_config(&config.server);
    for (rig, description) in config.rigs.iter().zip(descriptions) {
        let result = start_one(handle, config, rig, description, &options, &access, limits).await;
        match result {
            Ok(served) => daemon.rigs.push(served),
            Err(error) => {
                daemon.stop().await;
                return Err(error);
            }
        }
    }
    Ok(daemon)
}

// [@ANCHOR: radioctld_daemon_start_one]
/// Brings up one rig and its listener.
async fn start_one(
    handle: &Handle,
    config: &Config,
    rig: &crate::config::RigConfig,
    description: RigDescription,
    options: &impl Fn() -> RigOptions,
    access: &Arc<crate::access::AllowList>,
    limits: Limits,
) -> Result<Served, String> {
    let built = crate::rigs::build_rig(handle, rig, description, &config.server, options()).await?;
    let listener = TcpListener::bind(rig.listen)
        .await
        .map_err(|e| format!("rig {}: cannot listen on {}: {e}", rig.name, rig.listen))?;
    let server = serve(
        handle,
        listener,
        Arc::clone(&built.service),
        Arc::clone(access),
        limits,
    )
    .map_err(|e| format!("rig {}: {e}", rig.name))?;
    Ok(Served {
        name: rig.name.clone(),
        service: built.service,
        server,
        virtual_control: built.virtual_control,
        serial_link: built.serial_link,
    })
}

impl Daemon {
    // [@ANCHOR: radioctld_daemon_stop]
    /// Stops every listener and client, and unkeys any rig whose transmitter a client held.
    pub async fn stop(self) {
        for served in self.rigs {
            served.server.shutdown().await;
            served.service.shutdown().await;
        }
    }
}
