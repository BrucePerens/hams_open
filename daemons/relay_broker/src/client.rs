// SPDX-License-Identifier: AGPL-3.0-or-later
//! The client side: one request, one response, optionally one descriptor. This is what the
//! relay's workers would link; today it serves the tests and the example client.

use std::io;
use std::os::fd::{AsFd, OwnedFd};
use std::os::unix::net::UnixStream;
use std::path::Path;

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

// [@ANCHOR: relay_broker_client_connect_and_call]
/// Connects to the broker socket at `path` and performs one call.
pub fn connect_and_call(path: &Path, req: &Request) -> io::Result<(Response, Option<OwnedFd>)> {
    let stream = UnixStream::connect(path)?;
    call(&stream, req)
}
