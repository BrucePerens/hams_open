// SPDX-License-Identifier: AGPL-3.0-or-later
//! radioctld as a library: the same code the `radioctld` binary runs, started on the caller's
//! runtime from a configuration text, with one rigctld-protocol client talking to it.
use std::error::Error;
use std::path::Path;
use std::time::Duration;

use radioctl_core::rig::RigOptions;
use radioctld::config::Config;
use radioctld::daemon;
use radioctld::rigs::load_description;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::runtime::Handle;
use tokio::time::timeout;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn Error>> {
    let rigs = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../rigs");
    let text = r#"
[[rig]]
name = "kx3"
description = "elecraft-kx3.toml"
listen = "127.0.0.1:0"
[rig.transport]
kind = "virtual"
"#;
    let config = Config::parse(text, &rigs)?;
    let descriptions = config
        .rigs
        .iter()
        .map(|rig| load_description(&rig.description))
        .collect::<Result<Vec<_>, _>>()?;
    let daemon = daemon::start(
        &Handle::current(),
        &config,
        descriptions,
        RigOptions::default,
    )
    .await?;
    let address = daemon.rigs[0].server.local_addr();
    println!("radioctld: kx3 listening on loopback");

    let stream = TcpStream::connect(address).await?;
    let (reader, mut writer) = stream.into_split();
    let mut lines = BufReader::new(reader).lines();
    for request in ["F 14074000", "f", "t", "T 1"] {
        writer.write_all(format!("{request}\n").as_bytes()).await?;
        // Plain format: a get answers its value, a set answers `RPRT 0`, a refusal `RPRT <code>`.
        let first = timeout(Duration::from_secs(5), lines.next_line()).await??;
        println!("> {request}\n< {}", first.unwrap_or_default());
    }
    daemon.stop().await;
    Ok(())
}
