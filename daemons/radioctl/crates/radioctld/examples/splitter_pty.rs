// SPDX-License-Identifier: AGPL-3.0-or-later
//! The protocol splitter on virtual serial ports (Unix). One radio, one rig actor, and one pty per
//! outside program, each speaking IC-7300 CI-V or TS-2000 CAT.
//!
//! Usage: `splitter_pty PROTOCOL RIG_ID TRANSPORT LINK...`
//!
//! - `PROTOCOL` is `civ` or `ts2000`.
//! - `RIG_ID` names a file in `rigs/` (`elecraft-kx3`).
//! - `TRANSPORT` is `virtual` (no hardware) or `PATH:BAUD` (`/dev/ttyUSB0:38400`).
//! - Each `LINK` is a path where a symlink to a new pty is made; give that path to one program as
//!   its serial port. Press Ctrl-C to stop.
//!
//! The transmit gate stays closed, so keying is refused. Run on a real radio only if its
//! description is one you trust: every shipped description is unverified.
use std::error::Error;
use std::path::{Path, PathBuf};
use std::time::Duration;

use radioctl_core::description::RigDescription;
use radioctl_core::engine::{Inbound, ProtocolEngine, Request};
use radioctl_core::error::RigError;
use radioctl_core::op::Op;
use radioctl_core::rig::{spawn_current, RigHandle, RigOptions};
use radioctl_core::rigfile;
use radioctl_core::serial::{open_serial, PortTarget, SerialConfig};
use radioctl_protocols::conformance::engine_for;
use radioctl_protocols::io::spawn_virtual_rig;
use radioctl_protocols::virtual_rig::VirtualRig;
use radioctl_splitter::pty::attach_pty;
use radioctl_splitter::session::{Protocol, Splitter, SplitterConfig};
use tokio::runtime::Handle;

/// `spawn` wants a sized engine type; this lets the engine of any family go through it.
struct AnyEngine(Box<dyn ProtocolEngine>);

impl ProtocolEngine for AnyEngine {
    fn description(&self) -> &RigDescription {
        self.0.description()
    }

    fn encode(&mut self, op: &Op) -> Result<Request, RigError> {
        self.0.encode(op)
    }

    fn feed(&mut self, bytes: &[u8], now: Duration) -> Vec<Inbound> {
        self.0.feed(bytes, now)
    }
}

/// The serial monitor of the rig, if the rig is on a serial port; held for the program's life.
static SERIAL_MONITOR: std::sync::OnceLock<radioctl_core::serial::SerialMonitor> =
    std::sync::OnceLock::new();

async fn open_rig(rig_id: &str, transport: &str) -> Result<RigHandle, Box<dyn Error>> {
    let rigs = rigs_dir();
    let text = std::fs::read_to_string(rigs.join(format!("{rig_id}.toml")))?;
    let description = rigfile::parse(&text)?.description;
    let engine = AnyEngine(engine_for(&description)?);
    let handle = Handle::current();
    if transport == "virtual" {
        let radio = VirtualRig::from_description(&description)?;
        let (stream, _front_panel) = spawn_virtual_rig(&handle, radio, Duration::ZERO);
        return Ok(spawn_current(engine, stream, RigOptions::default())?);
    }
    let (path, baud) = transport
        .rsplit_once(':')
        .ok_or("TRANSPORT is `virtual` or PATH:BAUD")?;
    let target = PortTarget::Path(PathBuf::from(path));
    let link = open_serial(&handle, SerialConfig::new(target, baud.parse()?)).await?;
    // The actor watches the port and unkeys on every reopen. The monitor (control lines, status)
    // is kept with the rig handle so the link object outlives every client of the splitter.
    let options = RigOptions {
        link_watch: Some(link.watch()),
        ..RigOptions::default()
    };
    let (transport, monitor) = link.into_parts();
    let rig = spawn_current(engine, transport, options)?;
    SERIAL_MONITOR
        .set(monitor)
        .map_err(|_| "serial monitor already set")?;
    Ok(rig)
}

/// The directory of rig descriptions: `$RADIOCTL_RIGS`, else `./rigs` when it exists (the binaries
/// copied to a bench machine run from the directory that holds `rigs/`), else the checkout this
/// program was built from.
fn rigs_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("RADIOCTL_RIGS") {
        return PathBuf::from(dir);
    }
    let here = PathBuf::from("rigs");
    if here.is_dir() {
        return here;
    }
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../rigs")
}

#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [protocol, rig_id, transport, links @ ..] = args.as_slice() else {
        return Err("usage: splitter_pty PROTOCOL RIG_ID TRANSPORT LINK...".into());
    };
    let protocol = match protocol.as_str() {
        "civ" => Protocol::Civ {
            address: 0x94,
            echo: false,
        },
        "ts2000" => Protocol::Ts2000,
        other => return Err(format!("PROTOCOL is civ or ts2000, not {other:?}").into()),
    };
    if links.is_empty() {
        return Err("give at least one LINK path".into());
    }

    let rig = open_rig(rig_id, transport).await?;
    let splitter = Splitter::new(rig, SplitterConfig::new(protocol));
    let handle = Handle::current();
    let mut endpoints = Vec::new();
    for link in links {
        let (endpoint, client) = attach_pty(&splitter, &handle, Some(PathBuf::from(link)))?;
        println!("{} -> {}", link, endpoint.slave_path().display());
        endpoints.push((endpoint, client));
    }
    tokio::signal::ctrl_c().await?;
    println!("stopping; stats: {:?}", splitter.stats());
    // Dropping the endpoints removes the symlinks.
    Ok(())
}
