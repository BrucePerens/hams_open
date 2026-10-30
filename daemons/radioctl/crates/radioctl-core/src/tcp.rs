// SPDX-License-Identifier: AGPL-3.0-or-later
//! TCP transport: a rig reached through a network serial server, a radio with its own CAT port on
//! Ethernet, or `rigctld`. A tokio `TcpStream` already is a [`crate::transport::Transport`]; this
//! module adds a bounded connect and a bounded, backed-off retry so a rig that is still booting
//! or a network that is down ends in a typed error, never a hang.

use std::time::Duration;

use tokio::net::{TcpStream, ToSocketAddrs};
use tokio::time::{sleep, timeout};

use crate::backoff::Backoff;
use crate::error::{ErrorKind, RigError};

/// How to reach a rig over TCP.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TcpConfig {
    /// Longest wait for one connection attempt.
    pub connect_timeout: Duration,
    /// How many attempts in all (at least one).
    pub attempts: u32,
    /// Waits between attempts.
    pub backoff: Backoff,
}

impl Default for TcpConfig {
    fn default() -> TcpConfig {
        TcpConfig {
            connect_timeout: Duration::from_secs(3),
            attempts: 3,
            backoff: Backoff::default(),
        }
    }
}

// [@ANCHOR: radioctl_core_tcp_connect]
/// Connects to `addr`, trying up to `config.attempts` times with the configured backoff between
/// attempts and `config.connect_timeout` for each. Nagle's algorithm is switched off: CAT frames
/// are tiny and latency matters. The last failure is reported as [`ErrorKind::LinkDown`] with the
/// number of attempts.
pub async fn connect_tcp<A: ToSocketAddrs + Copy>(
    addr: A,
    config: &TcpConfig,
) -> Result<TcpStream, RigError> {
    let attempts = config.attempts.max(1);
    let mut last = String::from("no attempt made");
    for attempt in 1..=attempts {
        match timeout(config.connect_timeout, TcpStream::connect(addr)).await {
            Ok(Ok(stream)) => {
                stream
                    .set_nodelay(true)
                    .map_err(|e| RigError::new(ErrorKind::LinkDown, format!("set_nodelay: {e}")))?;
                return Ok(stream);
            }
            Ok(Err(error)) => last = error.to_string(),
            Err(_) => last = format!("no answer within {:?}", config.connect_timeout),
        }
        if attempt < attempts {
            sleep(config.backoff.delay(attempt)).await;
        }
    }
    Err(RigError::new(
        ErrorKind::LinkDown,
        format!("tcp connect failed after {attempts} attempts: {last}"),
    ))
}
