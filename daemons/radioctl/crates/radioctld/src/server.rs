// SPDX-License-Identifier: AGPL-3.0-or-later
//! The TCP server for one rig or rotator: accept, check the allow-list, bound the number of
//! clients, and run one task per client that reads lines, asks the service ([`LineService`]: a
//! rig's [`RigService`] or a rotator's [`crate::rotctl::RotatorService`]), and writes the answers.
//!
//! Bounds (all from the configuration): the client count, the length of a line, how long a
//! connection may be silent, how long a started line may take, how long a write may take. A client
//! that breaks one is disconnected; a disconnecting client that owned the transmitter unkeys the
//! rig. The server creates no runtime: every task runs on the `Handle` passed to [`serve`].

use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::runtime::Handle;
use tokio::sync::{watch, OwnedSemaphorePermit, Semaphore};
use tokio::task::JoinHandle;
use tokio::time::{sleep, timeout, Instant};

use crate::access::AllowList;
use crate::command::{parse_line, Line, LineBuffer};
use crate::config::ServerConfig;
use crate::reply::vfo_echo;
use crate::reply::{render, Body, Header, EINVAL};
use crate::service::RigService;

const READ_BUFFER: usize = 1024;
const WRITE_TIMEOUT: Duration = Duration::from_secs(10);
const ACCEPT_ERROR_PAUSE: Duration = Duration::from_millis(100);
const SHUTDOWN_WAIT: Duration = Duration::from_secs(3);

/// A boxed future, for the object-safe [`LineService`].
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// What a server answers request lines with. One client task calls `answer` for every complete
/// line, in order, and `disconnect` once when the client is gone.
pub trait LineService: Send + Sync {
    /// The text to send for one request line, and whether the connection closes after it.
    fn answer<'a>(&'a self, client: u64, line: &'a Line) -> BoxFuture<'a, (String, bool)>;
    /// The client left (or was dropped); anything it held is released.
    fn disconnect(&self, client: u64) -> BoxFuture<'_, ()>;
}

impl LineService for RigService {
    // [@ANCHOR: radioctld_server_rig_answer]
    /// [`answer`] for a rig.
    fn answer<'a>(&'a self, client: u64, line: &'a Line) -> BoxFuture<'a, (String, bool)> {
        Box::pin(answer(self, client, line))
    }

    // [@ANCHOR: radioctld_server_rig_disconnect]
    /// Releases the transmitter if the client held it (unkeying the rig).
    fn disconnect(&self, client: u64) -> BoxFuture<'_, ()> {
        Box::pin(RigService::disconnect(self, client))
    }
}

/// The bounds a server enforces.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// Most simultaneous clients.
    pub max_clients: usize,
    /// A connection silent for this long is closed.
    pub idle_timeout: Duration,
    /// A started line must end within this.
    pub line_timeout: Duration,
    /// Longest request line, in bytes.
    pub max_line_bytes: usize,
}

impl Limits {
    // [@ANCHOR: radioctld_server_limits_from_config]
    /// The limits of a configuration.
    pub fn from_config(config: &ServerConfig) -> Limits {
        Limits {
            max_clients: config.max_clients,
            idle_timeout: config.idle_timeout,
            line_timeout: config.line_timeout,
            max_line_bytes: config.max_line_bytes,
        }
    }
}

/// Counters of a server.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ServerStats {
    /// Connections served.
    pub accepted: u64,
    /// Connections refused by the allow-list.
    pub denied: u64,
    /// Connections refused because the client limit was reached.
    pub busy: u64,
    /// Clients connected now.
    pub active: u64,
    /// Clients dropped for an over-long line.
    pub line_too_long: u64,
    /// Clients dropped for silence, a stalled line, or a stalled write.
    pub timeouts: u64,
}

#[derive(Default)]
struct Counters {
    accepted: AtomicU64,
    denied: AtomicU64,
    busy: AtomicU64,
    active: AtomicU64,
    line_too_long: AtomicU64,
    timeouts: AtomicU64,
    next_client: AtomicU64,
}

struct Shared {
    service: Arc<dyn LineService>,
    access: Arc<AllowList>,
    limits: Limits,
    counters: Counters,
    shutdown: watch::Receiver<bool>,
}

/// A running server.
pub struct ServerHandle {
    local_addr: SocketAddr,
    shared: Arc<Shared>,
    stop: watch::Sender<bool>,
    accept: JoinHandle<()>,
}

// [@ANCHOR: radioctld_server_serve]
/// Starts serving `service` on `listener`. The caller binds the listener (loopback unless the
/// configuration says otherwise) and passes the runtime `handle` everything runs on.
pub fn serve(
    handle: &Handle,
    listener: TcpListener,
    service: Arc<dyn LineService>,
    access: Arc<AllowList>,
    limits: Limits,
) -> std::io::Result<ServerHandle> {
    let local_addr = listener.local_addr()?;
    let (stop, shutdown) = watch::channel(false);
    let shared = Arc::new(Shared {
        service,
        access,
        limits,
        counters: Counters::default(),
        shutdown,
    });
    let accept = handle.spawn(accept_loop(listener, Arc::clone(&shared), handle.clone()));
    Ok(ServerHandle {
        local_addr,
        shared,
        stop,
        accept,
    })
}

impl ServerHandle {
    // [@ANCHOR: radioctld_server_local_addr]
    /// The address clients connect to.
    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    // [@ANCHOR: radioctld_server_stats]
    /// A snapshot of the counters.
    pub fn stats(&self) -> ServerStats {
        let c = &self.shared.counters;
        ServerStats {
            accepted: c.accepted.load(Ordering::Relaxed),
            denied: c.denied.load(Ordering::Relaxed),
            busy: c.busy.load(Ordering::Relaxed),
            active: c.active.load(Ordering::Relaxed),
            line_too_long: c.line_too_long.load(Ordering::Relaxed),
            timeouts: c.timeouts.load(Ordering::Relaxed),
        }
    }

    // [@ANCHOR: radioctld_server_shutdown]
    /// Stops accepting, disconnects every client (a client that owned the transmitter unkeys the
    /// rig on its way out) and waits a bounded time for them to finish.
    pub async fn shutdown(self) {
        self.accept.abort();
        let _ = self.stop.send(true);
        let deadline = Instant::now() + SHUTDOWN_WAIT;
        while self.shared.counters.active.load(Ordering::Relaxed) > 0 && Instant::now() < deadline {
            sleep(Duration::from_millis(5)).await;
        }
    }
}

// [@ANCHOR: radioctld_server_accept_loop]
/// Accepts connections until aborted. Peers outside the allow-list are dropped before a byte is
/// read; connections past the client limit are dropped at once.
async fn accept_loop(listener: TcpListener, shared: Arc<Shared>, handle: Handle) {
    let slots = Arc::new(Semaphore::new(shared.limits.max_clients));
    loop {
        let (socket, peer) = match listener.accept().await {
            Ok(accepted) => accepted,
            Err(_) => {
                sleep(ACCEPT_ERROR_PAUSE).await;
                continue;
            }
        };
        if !shared.access.allows(peer.ip()) {
            shared.counters.denied.fetch_add(1, Ordering::Relaxed);
            continue;
        }
        let Ok(permit) = Arc::clone(&slots).try_acquire_owned() else {
            shared.counters.busy.fetch_add(1, Ordering::Relaxed);
            continue;
        };
        let _ = socket.set_nodelay(true);
        shared.counters.accepted.fetch_add(1, Ordering::Relaxed);
        shared.counters.active.fetch_add(1, Ordering::Relaxed);
        // The client's number is taken here, in accept order, not in the spawned task: two
        // tasks started back to back would otherwise race for it and the ids would not follow
        // the order the clients connected in.
        let client = shared.counters.next_client.fetch_add(1, Ordering::Relaxed) + 1;
        handle.spawn(run_client(socket, client, Arc::clone(&shared), permit));
    }
}

// [@ANCHOR: radioctld_server_answer]
/// The text to send for one request line, and whether the connection closes after it.
pub async fn answer(service: &RigService, client: u64, line: &Line) -> (String, bool) {
    let text = match line {
        Line::Invalid => return (render_invalid(), false),
        Line::Text(text) => text,
    };
    let Some(parsed) = parse_line(text) else {
        return (String::new(), false);
    };
    let header = Header {
        extended: parsed.extended,
        name: parsed.name,
        vfo: vfo_echo(parsed.vfo.as_ref(), service.config().vfo_mode),
        args: parsed.args.clone(),
    };
    let outcome = match &parsed.command {
        Err(code) => Err(*code),
        Ok(command) => {
            service
                .execute(client, command, parsed.vfo.as_ref().map(|(_, sel)| *sel))
                .await
        }
    };
    let close = matches!(outcome, Ok(Body::Close));
    (render(&header, &outcome), close)
}

// [@ANCHOR: radioctld_server_render_invalid]
/// The answer to a line that is not text.
fn render_invalid() -> String {
    format!("RPRT {EINVAL}\n")
}

// [@ANCHOR: radioctld_server_write]
/// Writes a reply within the write timeout; false when the client is gone or stalled.
async fn write_reply(
    stream: &mut tokio::net::tcp::OwnedWriteHalf,
    shared: &Shared,
    text: &str,
) -> bool {
    if text.is_empty() {
        return true;
    }
    match timeout(WRITE_TIMEOUT, stream.write_all(text.as_bytes())).await {
        Ok(Ok(())) => true,
        Ok(Err(_)) => false,
        Err(_) => {
            shared.counters.timeouts.fetch_add(1, Ordering::Relaxed);
            false
        }
    }
}

// [@ANCHOR: radioctld_server_run_client]
/// The task of one client, until it leaves, misbehaves, or the server stops.
async fn run_client(
    socket: TcpStream,
    client: u64,
    shared: Arc<Shared>,
    _permit: OwnedSemaphorePermit,
) {
    let (mut reader, mut writer) = socket.into_split();
    let mut lines = LineBuffer::new(shared.limits.max_line_bytes);
    let mut shutdown = shared.shutdown.clone();
    let mut buffer = [0u8; READ_BUFFER];
    let mut line_started: Option<Instant> = None;
    'client: loop {
        let deadline = match line_started {
            Some(started) => started + shared.limits.line_timeout,
            None => Instant::now() + shared.limits.idle_timeout,
        };
        let read = tokio::select! {
            read = timeout(deadline.saturating_duration_since(Instant::now()), reader.read(&mut buffer)) => read,
            _ = shutdown.changed() => break 'client,
        };
        let n = match read {
            Ok(Ok(n)) if n > 0 => n,
            Ok(_) => break 'client,
            Err(_) => {
                shared.counters.timeouts.fetch_add(1, Ordering::Relaxed);
                break 'client;
            }
        };
        let received = buffer.get(..n).unwrap_or(&[]);
        let complete = match lines.push(received) {
            Ok(complete) => complete,
            Err(_) => {
                shared
                    .counters
                    .line_too_long
                    .fetch_add(1, Ordering::Relaxed);
                let _ = write_reply(&mut writer, &shared, &render_invalid()).await;
                break 'client;
            }
        };
        line_started = if lines.has_partial() {
            line_started.or_else(|| Some(Instant::now()))
        } else {
            None
        };
        for line in &complete {
            let (reply, close) = shared.service.answer(client, line).await;
            if !write_reply(&mut writer, &shared, &reply).await || close {
                break 'client;
            }
        }
    }
    shared.service.disconnect(client).await;
    shared.counters.active.fetch_sub(1, Ordering::Relaxed);
}

#[cfg(test)]
mod tests {
    use super::*;

    // Tests [@ANCHOR: radioctld_server_limits_from_config]
    #[test]
    fn limits_follow_the_configuration() {
        let text =
            "[[rig]]\nname = \"a\"\ndescription = \"x\"\n[rig.transport]\nkind = \"virtual\"\n";
        let config = crate::config::Config::parse(text, std::path::Path::new(".")).expect("config");
        let limits = Limits::from_config(&config.server);
        assert_eq!(limits.max_clients, 16);
        assert_eq!(limits.max_line_bytes, 1024);
        assert_eq!(limits.line_timeout, Duration::from_secs(10));
    }

    // Tests [@ANCHOR: radioctld_server_render_invalid]
    #[test]
    fn a_non_text_line_is_einval() {
        assert_eq!(render_invalid(), "RPRT -1\n");
    }
}
