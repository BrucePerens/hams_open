// SPDX-License-Identifier: AGPL-3.0-or-later
//! The daemon proper: bring up every configured rig and rotator and its listener on the caller's
//! runtime, and take them down again. The binary supplies the runtime and decides when to stop.

use std::sync::Arc;

use radioctl_core::description::RigDescription;
use radioctl_core::net::NetMonitor;
use radioctl_core::rig::RigOptions;
use radioctl_protocols::io::VirtualRigControl;
use radioctl_station::virtual_rotator::VirtualRotatorControl;
use tokio::net::TcpListener;
use tokio::runtime::Handle;

use crate::config::{Config, RotatorConfig};
use crate::rotctl::RotatorService;
use crate::server::{serve, Limits, ServerHandle};
use crate::service::RigService;
use radioctl_core::serial::SerialMonitor;

/// One rotator being served.
pub struct ServedRotator {
    /// The rotator's name from the configuration.
    pub name: String,
    /// The rotator's service.
    pub service: Arc<RotatorService>,
    /// The listener.
    pub server: ServerHandle,
    /// Control of the virtual rotator behind the service, for virtual rotators.
    pub virtual_control: Option<VirtualRotatorControl>,
    /// The network link's status and counters, for TCP (and virtual) rotators.
    pub net_link: Option<NetMonitor>,
}

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
    /// The network link's status and counters, for TCP rigs.
    pub net_link: Option<NetMonitor>,
}

/// The running daemon.
pub struct Daemon {
    /// Every rig being served, in configuration order.
    pub rigs: Vec<Served>,
    /// Every rotator being served, in configuration order.
    pub rotators: Vec<ServedRotator>,
}

// [@ANCHOR: radioctld_daemon_start]
/// Brings up every rig of `config` (descriptions given in the same order), then every rotator,
/// and starts listening. If anything fails, what was already started is stopped and the error
/// is returned.
pub async fn start(
    handle: &Handle,
    config: &Config,
    descriptions: Vec<RigDescription>,
    options: impl Fn() -> RigOptions,
) -> Result<Daemon, String> {
    let mut daemon = Daemon {
        rigs: Vec::new(),
        rotators: Vec::new(),
    };
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
    for rotator in &config.rotators {
        match start_rotator(handle, rotator, &access, limits).await {
            Ok(served) => daemon.rotators.push(served),
            Err(error) => {
                daemon.stop().await;
                return Err(error);
            }
        }
    }
    Ok(daemon)
}

// [@ANCHOR: radioctld_daemon_start_rotator]
/// Brings up one rotator and its listener.
async fn start_rotator(
    handle: &Handle,
    rotator: &RotatorConfig,
    access: &Arc<crate::access::AllowList>,
    limits: Limits,
) -> Result<ServedRotator, String> {
    let built = crate::rotators::build_rotator(handle, rotator).await?;
    let listener = TcpListener::bind(rotator.listen).await.map_err(|e| {
        format!(
            "rotator {}: cannot listen on {}: {e}",
            rotator.name, rotator.listen
        )
    })?;
    let server = serve(
        handle,
        listener,
        Arc::clone(&built.service) as Arc<dyn crate::server::LineService>,
        Arc::clone(access),
        limits,
    )
    .map_err(|e| format!("rotator {}: {e}", rotator.name))?;
    Ok(ServedRotator {
        name: rotator.name.clone(),
        service: built.service,
        server,
        virtual_control: built.virtual_control,
        net_link: built.net_link,
    })
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
        Arc::clone(&built.service) as Arc<dyn crate::server::LineService>,
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
        net_link: built.net_link,
    })
}

impl Daemon {
    // [@ANCHOR: radioctld_daemon_stop]
    /// Stops every listener and client, and unkeys any rig whose transmitter a client held. A
    /// rotator is left where it is: it keeps its own target, as it would if a `rotctld` stopped.
    pub async fn stop(self) {
        for served in self.rigs {
            served.server.shutdown().await;
            served.service.shutdown().await;
        }
        for served in self.rotators {
            served.server.shutdown().await;
        }
    }
}
