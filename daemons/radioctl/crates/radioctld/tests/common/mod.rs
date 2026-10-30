// SPDX-License-Identifier: AGPL-3.0-or-later
//! Shared test helpers: start the daemon the way production does (configuration text, rig
//! description files, `daemon::start` on a runtime), and talk to it over real TCP.
#![allow(dead_code, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::time::Duration;

use radioctl_core::description::RigDescription;
use radioctl_core::rig::RigOptions;
use radioctld::config::Config;
use radioctld::daemon::{self, Daemon};
use radioctld::rigs::load_description;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::runtime::Handle;
use tokio::time::timeout;

/// The rig description files shipped with the crate family.
pub fn rigs_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../rigs")
}

/// Configuration text for one virtual rig named `name`, from description `file`.
pub fn config_text(server: &str, name: &str, file: &str, transmit: bool, vfo_mode: bool) -> String {
    format!(
        "{server}\n[[rig]]\nname = \"{name}\"\ndescription = {:?}\nlisten = \"127.0.0.1:0\"\n\
         transmit = {transmit}\nvfo_mode = {vfo_mode}\n[rig.transport]\nkind = \"virtual\"\n",
        rigs_dir().join(file).display().to_string()
    )
}

/// Rig options with polling off, so the bytes a rig receives are exactly the commands sent.
pub fn quiet_options() -> RigOptions {
    let mut options = RigOptions::default();
    options.poll.enabled = false;
    options
}

/// Starts the daemon from configuration text.
pub async fn start(text: &str) -> Daemon {
    let config = Config::parse(text, Path::new(".")).expect("config");
    let descriptions = config
        .rigs
        .iter()
        .map(|r| load_description(&r.description).expect("description"))
        .collect();
    daemon::start(&Handle::current(), &config, descriptions, quiet_options)
        .await
        .expect("daemon")
}

/// Waits (bounded) for a condition on the rig's state. Sets on the Elecraft family have no reply,
/// so the virtual rig may see the bytes a moment after the client has been answered.
pub async fn eventually(what: &str, mut done: impl FnMut() -> bool) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while !done() {
        assert!(
            tokio::time::Instant::now() < deadline,
            "timed out waiting for {what}"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

/// Starts the daemon from configuration text with descriptions the caller supplies.
pub async fn start_with(text: &str, descriptions: Vec<RigDescription>) -> Daemon {
    let config = Config::parse(text, Path::new(".")).expect("config");
    daemon::start(&Handle::current(), &config, descriptions, quiet_options)
        .await
        .expect("daemon")
}

/// A client connection.
pub struct Client {
    stream: TcpStream,
}

impl Client {
    /// Connects to the first rig of `daemon`.
    pub async fn connect(daemon: &Daemon) -> Client {
        Client::connect_to(daemon.rigs[0].server.local_addr()).await
    }

    /// Connects to `addr`.
    pub async fn connect_to(addr: std::net::SocketAddr) -> Client {
        Client {
            stream: TcpStream::connect(addr).await.expect("connect"),
        }
    }

    /// Sends raw bytes.
    pub async fn send_raw(&mut self, bytes: &[u8]) {
        self.stream.write_all(bytes).await.expect("write");
    }

    /// Reads whatever arrives: waits up to two seconds for the first bytes, then takes what
    /// follows within 40 ms. Empty when the server stayed silent.
    pub async fn read_reply(&mut self) -> String {
        let mut out = Vec::new();
        let mut buffer = [0u8; 8192];
        let mut wait = Duration::from_secs(2);
        while let Ok(Ok(n)) = timeout(wait, self.stream.read(&mut buffer)).await {
            if n == 0 {
                break;
            }
            out.extend_from_slice(&buffer[..n]);
            wait = Duration::from_millis(40);
        }
        String::from_utf8_lossy(&out).into_owned()
    }

    /// Sends one request line and returns the reply.
    pub async fn ask(&mut self, line: &str) -> String {
        self.send_raw(format!("{line}\n").as_bytes()).await;
        self.read_reply().await
    }

    /// True when the server has closed the connection (reads end of stream within `wait`).
    pub async fn is_closed(&mut self, wait: Duration) -> bool {
        let mut buffer = [0u8; 64];
        matches!(
            timeout(wait, self.stream.read(&mut buffer)).await,
            Ok(Ok(0)) | Ok(Err(_))
        )
    }
}
