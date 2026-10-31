// SPDX-License-Identifier: AGPL-3.0-or-later
//! The protocol splitter: a Kenwood TS-2000 CAT endpoint on a loopback TCP port in front of
//! one rig actor (a virtual KX3). Two "programs" connect and share the radio.
use std::error::Error;
use std::time::Duration;

use radioctl_core::rig::{spawn_current, RigOptions};
use radioctl_core::rigfile;
use radioctl_protocols::elecraft::ElecraftEngine;
use radioctl_protocols::io::spawn_virtual_rig;
use radioctl_protocols::virtual_rig::VirtualRig;
use radioctl_splitter::session::{Protocol, Splitter, SplitterConfig};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::runtime::Handle;
use tokio::time::timeout;

/// Sends one CAT command and returns the `;`-terminated answer.
async fn ask(program: &mut TcpStream, command: &str) -> Result<String, Box<dyn Error>> {
    program.write_all(command.as_bytes()).await?;
    let mut answer = Vec::new();
    let mut byte = [0u8; 1];
    while answer.last() != Some(&b';') {
        timeout(Duration::from_secs(5), program.read_exact(&mut byte)).await??;
        answer.push(byte[0]);
    }
    Ok(String::from_utf8(answer)?)
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn Error>> {
    let description = rigfile::parse(include_str!("../../../rigs/elecraft-kx3.toml"))?.description;
    let radio = VirtualRig::from_description(&description)?;
    let (transport, _front_panel) = spawn_virtual_rig(&Handle::current(), radio, Duration::ZERO);
    let rig = spawn_current(
        ElecraftEngine::new(description)?,
        transport,
        RigOptions::default(),
    )?;

    // Every client of the splitter goes through the one actor queue of `rig`.
    let splitter = Splitter::new(rig, SplitterConfig::new(Protocol::Ts2000));
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let server = splitter.serve_tcp(&Handle::current(), listener);

    let mut logger = TcpStream::connect(address).await?;
    let mut digital = TcpStream::connect(address).await?;
    println!("logger sees   {}", ask(&mut logger, "FA;").await?);
    ask(&mut digital, "FA00007074000;MD2;ID;").await.ok();
    println!("digital sees  {}", ask(&mut digital, "FA;").await?);
    println!("logger sees   {}", ask(&mut logger, "FA;").await?);

    // Keying is refused while the transmit gate is closed: the TS-2000 answers `?;`.
    println!("TX; answers   {}", ask(&mut logger, "TX;").await?);
    server.abort();
    Ok(())
}
