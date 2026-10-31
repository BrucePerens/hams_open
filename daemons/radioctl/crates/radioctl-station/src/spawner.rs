// SPDX-License-Identifier: AGPL-3.0-or-later
//! The ready-made [`RigSpawner`]: for each radio of the station, a rig description and where to
//! reach it (a TCP address, with an optional framing such as the TS-890S LAN login, or a serial
//! port), turned into a running actor on demand. This is the join between the radio manager and
//! the real transports and engines.
//!
//! Every connection it makes is a reconnecting one (`radioctl_core::net::open_link`, or
//! `open_serial`), the actor gets the transport's link status, and the transmit gate is left
//! closed: opening it is a decision for the application, on whose authority it is made, not for
//! the spawner. Credentials never pass through here: a [`FramingFactory`] closes over whatever the
//! application read from its secret store.

use crate::actor_link::{RigConnection, RigSpawner};
use crate::channel::BoxFuture;
use crate::error::{StationError, StationErrorKind};
use crate::model::DeviceId;
use radioctl_core::description::RigDescription;
use radioctl_core::net::{open_link, Framing, NetConfig, RawFraming};
use radioctl_core::rig::{spawn, RigOptions};
use radioctl_core::serial::{open_serial, PortTarget, SerialConfig};
use radioctl_protocols::conformance::engine_for;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::runtime::Handle;

/// Makes a fresh framing for each new connection.
pub type FramingFactory = Arc<dyn Fn() -> Box<dyn Framing> + Send + Sync>;

/// Where a radio is.
#[derive(Clone)]
pub enum RigEndpoint {
    /// A TCP address (`host:port`), with the framing it speaks (`None`: the raw CAT stream).
    Tcp {
        /// `host:port`.
        address: String,
        /// The framing, if the radio has a session of its own (the TS-890S LAN login).
        framing: Option<FramingFactory>,
        /// The network link settings (timeouts, backoff, silence limit).
        config: NetConfig,
    },
    /// A serial port.
    Serial {
        /// The port.
        target: PortTarget,
        /// The speed.
        baud: u32,
    },
}

/// One radio: its description, where it is, and how its actor is configured.
#[derive(Clone)]
pub struct RigSpec {
    /// The description (rig file) the engine is built from.
    pub description: RigDescription,
    /// Where the radio is.
    pub endpoint: RigEndpoint,
    /// The actor's options (the link watch is set by the spawner).
    pub options: RigOptions,
}

impl RigSpec {
    // [@ANCHOR: radioctl_station_rigspec_tcp]
    /// A radio at a TCP address with the raw CAT stream and default settings.
    pub fn tcp(description: RigDescription, address: impl Into<String>) -> RigSpec {
        let address = address.into();
        RigSpec {
            description,
            endpoint: RigEndpoint::Tcp {
                config: NetConfig::new(address.clone()),
                address,
                framing: None,
            },
            options: RigOptions::default(),
        }
    }

    // [@ANCHOR: radioctl_station_rigspec_serial]
    /// A radio on a serial port at `baud`, with default settings.
    pub fn serial(description: RigDescription, path: impl Into<PathBuf>, baud: u32) -> RigSpec {
        RigSpec {
            description,
            endpoint: RigEndpoint::Serial {
                target: PortTarget::Path(path.into()),
                baud,
            },
            options: RigOptions::default(),
        }
    }
}

/// Spawns the rigs of a station from their specs.
#[derive(Default)]
pub struct DescriptionSpawner {
    specs: BTreeMap<DeviceId, RigSpec>,
}

impl DescriptionSpawner {
    // [@ANCHOR: radioctl_station_dspawner_new]
    /// No radios yet.
    pub fn new() -> DescriptionSpawner {
        DescriptionSpawner::default()
    }

    // [@ANCHOR: radioctl_station_dspawner_add]
    /// Adds (or replaces) the spec of a radio.
    pub fn add(&mut self, radio: DeviceId, spec: RigSpec) {
        self.specs.insert(radio, spec);
    }
}

// [@ANCHOR: radioctl_station_dspawner_fail]
fn fail(e: impl std::fmt::Display) -> StationError {
    StationError::new(StationErrorKind::Driver, e.to_string())
}

impl RigSpawner for DescriptionSpawner {
    // [@ANCHOR: radioctl_station_dspawner_spawn]
    /// Builds the engine, opens the transport (reconnecting), and spawns the actor with the
    /// transport's link status. The first connection is one attempt; the manager's backoff spaces
    /// further ones. The transmit gate stays closed.
    fn spawn(&self, radio: &DeviceId) -> BoxFuture<'_, Result<RigConnection, StationError>> {
        let radio = radio.clone();
        Box::pin(async move {
            let spec = self.specs.get(&radio).ok_or_else(|| {
                StationError::new(
                    StationErrorKind::Unknown,
                    format!("no rig spec for {}", radio.0),
                )
            })?;
            let engine = engine_for(&spec.description).map_err(fail)?;
            let handle = Handle::current();
            let mut options = spec.options.clone();
            let (stream, status) = match &spec.endpoint {
                RigEndpoint::Tcp {
                    framing, config, ..
                } => {
                    let framing: Box<dyn Framing> = match framing {
                        Some(make) => make(),
                        None => Box::new(RawFraming),
                    };
                    let mut config = config.clone();
                    config.first_attempts = 1;
                    let link = open_link(&handle, config, framing).await.map_err(fail)?;
                    let status = link.watch();
                    let (stream, _monitor) = link.into_parts();
                    (stream, status)
                }
                RigEndpoint::Serial { target, baud } => {
                    let link = open_serial(&handle, SerialConfig::new(target.clone(), *baud))
                        .await
                        .map_err(fail)?;
                    let status = link.watch();
                    let (stream, _monitor) = link.into_parts();
                    (stream, status)
                }
            };
            options.link_watch = Some(status.clone());
            let rig = spawn(&handle, engine, stream, options).map_err(fail)?;
            Ok(RigConnection {
                handle: rig,
                watch: Some(status),
            })
        })
    }
}
