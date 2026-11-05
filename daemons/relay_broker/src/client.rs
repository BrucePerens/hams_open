// SPDX-License-Identifier: AGPL-3.0-or-later
//! The client side: one request, one response, optionally one descriptor. This is what the
//! relay's workers would link; today it serves the tests and the example client.

use std::io;
use std::os::fd::{AsFd, OwnedFd};
use std::os::unix::net::UnixStream;
use std::path::Path;

use nix::sys::socket::{getsockopt, sockopt};

use crate::transport::{recv_frame, send_frame, set_timeouts};
use crate::wire::{Request, Response};

// [@ANCHOR: relay_broker_client_call]
/// Sends `req` on `stream` and waits for the answer. The descriptor, if any, is the opened
/// device. A response that does not parse is `InvalidData`.
pub fn call(stream: &UnixStream, req: &Request) -> io::Result<(Response, Option<OwnedFd>)> {
    let fd = stream.as_fd();
    let _ = set_timeouts(fd);
    send_frame(fd, &req.encode(), None)?;
    let (body, mut fds) = recv_frame(fd)?;
    let resp = Response::decode(&body)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("response: {e:?}")))?;
    let passed = if matches!(resp, Response::Opened) {
        fds.pop()
    } else {
        None
    };
    Ok((resp, passed))
}

// [@ANCHOR: relay_broker_client_default_server_uids]
/// The accounts a broker socket may be served by when the caller names none: root. A
/// socket-activated broker (`packaging/systemd`) has its listener made by systemd, so the
/// credentials a client reads are root's even though the service itself runs as an unprivileged
/// account; a broker started directly by a service account needs that account named.
pub const DEFAULT_SERVER_UIDS: &[u32] = &[0];

// [@ANCHOR: relay_broker_client_verify_server]
/// Checks who is behind the connected socket `stream` before anything is sent on it: the peer
/// credentials (`SO_PEERCRED`, set by the kernel when the listener was created) must name one of
/// `expected_uids`. The broker authenticates the client; this is the other direction, so a process
/// that took over the socket path cannot answer `ListDevices`, `ProvisionGroups` or `OpenDevice`
/// (with a descriptor of its choice) and the relay does not depend on the installer making the
/// socket's directory root-only. An empty `expected_uids` permits no one. The refusal is
/// `PermissionDenied`; a failure to read the credentials is returned as it came.
pub fn verify_server(stream: &UnixStream, expected_uids: &[u32]) -> io::Result<()> {
    let cred = getsockopt(stream, sockopt::PeerCredentials).map_err(io::Error::from)?;
    if expected_uids.contains(&cred.uid()) {
        return Ok(());
    }
    Err(io::Error::new(
        io::ErrorKind::PermissionDenied,
        format!(
            "the broker socket is served by uid {}, which is not an expected broker account",
            cred.uid()
        ),
    ))
}

// [@ANCHOR: relay_broker_client_connect_and_call]
/// Connects to the broker socket at `path`, checks that [`DEFAULT_SERVER_UIDS`] (root) stands
/// behind it, and performs one call. Use [`connect_and_call_as`] when the broker is started by
/// another account.
pub fn connect_and_call(path: &Path, req: &Request) -> io::Result<(Response, Option<OwnedFd>)> {
    connect_and_call_as(path, req, DEFAULT_SERVER_UIDS)
}

// [@ANCHOR: relay_broker_client_connect_and_call_as]
/// Connects to the broker socket at `path`, verifies with [`verify_server`] that one of
/// `expected_uids` stands behind it, and only then sends `req` and returns the answer. A wrong
/// server is `PermissionDenied` and receives no byte.
pub fn connect_and_call_as(
    path: &Path,
    req: &Request,
    expected_uids: &[u32],
) -> io::Result<(Response, Option<OwnedFd>)> {
    let stream = UnixStream::connect(path)?;
    verify_server(&stream, expected_uids)?;
    call(&stream, req)
}
