// SPDX-License-Identifier: AGPL-3.0-or-later
//! The connection loop. One request per connection: authenticate the peer with SO_PEERCRED
//! before reading anything, read one bounded frame, parse, handle, answer, close. The listener
//! normally comes from systemd socket activation (fd 3); `run` exits after `idle` with no
//! connection, and socket activation starts the broker again on the next connect.

use std::io;
use std::os::fd::{AsFd, FromRawFd, OwnedFd};
use std::os::unix::net::{UnixListener, UnixStream};
use std::time::{Duration, Instant};

use nix::poll::{poll, PollFd, PollFlags, PollTimeout};

use crate::broker::Broker;
use crate::transport::{recv_frame, send_frame, set_timeouts};
use crate::wire::{Request, Response, Status, WireError};

/// What happened on one connection, for the caller's bookkeeping and the tests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// The peer was not on the allow-list; nothing was read.
    PeerRejected,
    /// The request did not parse; a `Malformed`/`UnsupportedVersion` answer was sent.
    Malformed,
    /// A request was handled and answered (allowed or denied by policy).
    Answered,
    /// I/O failed (timeout, peer gone, oversize frame).
    IoError,
}

// [@ANCHOR: relay_broker_serve_connection]
/// Serves one connection to completion. Never panics; every failure is an `Outcome`.
pub fn serve_connection(broker: &mut Broker, stream: &UnixStream) -> Outcome {
    let fd = stream.as_fd();
    let _ = set_timeouts(fd);
    let peer = match broker.os().peer_credentials(fd) {
        Ok(p) => p,
        Err(_) => return Outcome::PeerRejected,
    };
    if !broker.allow.permits(peer) {
        broker.log(
            peer,
            "peer_rejected",
            "rejected",
            format!("gid {}", peer.gid),
        );
        return Outcome::PeerRejected;
    }
    let (body, received_fds) = match recv_frame(fd) {
        Ok(x) => x,
        Err(e) => {
            broker.log(peer, "malformed", "rejected", e.to_string());
            return Outcome::IoError;
        }
    };
    // A client has no business sending descriptors; whatever arrived is closed here.
    drop(received_fds);
    let (response, passed): (Response, Option<OwnedFd>) = match Request::decode(&body) {
        Ok(req) => broker.handle(peer, &req, Instant::now()),
        Err(WireError::Version(v)) => {
            broker.log(peer, "malformed", "rejected", format!("version {v}"));
            let r = Response::error(Status::UnsupportedVersion, "unsupported protocol version");
            let _ = send_frame(fd, &r.encode(), None);
            return Outcome::Malformed;
        }
        Err(e) => {
            broker.log(peer, "malformed", "rejected", format!("{e:?}"));
            let r = Response::error(Status::Malformed, "malformed request");
            let _ = send_frame(fd, &r.encode(), None);
            return Outcome::Malformed;
        }
    };
    match send_frame(fd, &response.encode(), passed.as_ref().map(|f| f.as_fd())) {
        Ok(()) => Outcome::Answered,
        Err(_) => Outcome::IoError,
    }
}

// [@ANCHOR: relay_broker_run]
/// Accepts connections until `idle` passes with none, or until `max_connections` have been
/// served (`None` for no limit; the tests use it). Returns the number served.
pub fn run(
    broker: &mut Broker,
    listener: &UnixListener,
    idle: Duration,
    max_connections: Option<usize>,
) -> io::Result<usize> {
    let mut served = 0usize;
    loop {
        if max_connections.is_some_and(|m| served >= m) {
            return Ok(served);
        }
        let mut fds = [PollFd::new(listener.as_fd(), PollFlags::POLLIN)];
        let timeout = PollTimeout::try_from(idle).unwrap_or(PollTimeout::MAX);
        let ready = match poll(&mut fds, timeout) {
            Ok(n) => n,
            Err(nix::errno::Errno::EINTR) => continue,
            Err(e) => return Err(io::Error::from(e)),
        };
        if ready == 0 {
            return Ok(served);
        }
        match listener.accept() {
            Ok((stream, _)) => {
                let _ = serve_connection(broker, &stream);
                served = served.saturating_add(1);
            }
            Err(e)
                if e.kind() == io::ErrorKind::WouldBlock
                    || e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
}

// [@ANCHOR: relay_broker_activated_listener]
/// The listener systemd passed (`LISTEN_PID` is this process and `LISTEN_FDS` is 1), if any.
/// Adopting descriptor 3 (SD_LISTEN_FDS_START) is the crate's second and last `unsafe` block.
#[allow(unsafe_code)]
pub fn activated_listener() -> Option<UnixListener> {
    let pid: u32 = std::env::var("LISTEN_PID").ok()?.parse().ok()?;
    let n: u32 = std::env::var("LISTEN_FDS").ok()?.parse().ok()?;
    if pid != std::process::id() || n != 1 {
        return None;
    }
    // SAFETY: systemd's protocol guarantees that when LISTEN_PID names this process and
    // LISTEN_FDS is 1, descriptor 3 is the listening socket handed to us and nothing else in
    // this process has opened or will open descriptor 3 before this point (main calls this
    // first). Ownership is taken exactly once.
    let owned = unsafe { OwnedFd::from_raw_fd(3) };
    let listener = UnixListener::from(owned);
    listener.set_nonblocking(false).ok()?;
    Some(listener)
}
