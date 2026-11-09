// SPDX-License-Identifier: AGPL-3.0-or-later
//! Frames on a Unix stream socket, with optional descriptor passing. `send_frame` writes the
//! length-prefixed frame with `sendmsg`, attaching at most one descriptor as SCM_RIGHTS to the
//! first chunk. `recv_frame` reads the header, bounds the length, reads the body with `recvmsg`
//! and collects any descriptors that arrived; a frame that is too long is refused before any
//! body byte is read. Both sides use a receive timeout so a stalled peer cannot hold a thread.

use std::io::{self, IoSlice, IoSliceMut};
use std::os::fd::{AsRawFd, BorrowedFd, FromRawFd, OwnedFd, RawFd};
use std::time::{Duration, Instant};

use nix::sys::socket::{
    cmsg_space, recvmsg, sendmsg, setsockopt, sockopt, ControlMessage, ControlMessageOwned,
    MsgFlags,
};
use nix::sys::time::TimeVal;

use crate::wire::{frame_body_len, frame_encode, WireError, MAX_FRAME};

/// How long either side waits for the other before giving up.
pub const IO_TIMEOUT: Duration = Duration::from_secs(5);

// [@ANCHOR: relay_broker_set_timeouts]
/// Applies `IO_TIMEOUT` as the receive and send timeout of a socket.
pub fn set_timeouts(socket: BorrowedFd<'_>) -> io::Result<()> {
    let tv = TimeVal::new(IO_TIMEOUT.as_secs() as i64, 0);
    setsockopt(&socket, sockopt::ReceiveTimeout, &tv).map_err(io::Error::from)?;
    setsockopt(&socket, sockopt::SendTimeout, &tv).map_err(io::Error::from)
}

// [@ANCHOR: relay_broker_send_frame]
/// Writes `body` as one frame, passing `fd` (if any) with the first bytes.
pub fn send_frame(
    socket: BorrowedFd<'_>,
    body: &[u8],
    fd: Option<BorrowedFd<'_>>,
) -> io::Result<()> {
    let frame = frame_encode(body);
    let raw: [RawFd; 1] = [fd.map_or(-1, |f| f.as_raw_fd())];
    let cmsgs: Vec<ControlMessage<'_>> = if fd.is_some() {
        vec![ControlMessage::ScmRights(&raw)]
    } else {
        Vec::new()
    };
    let mut sent = 0usize;
    while sent < frame.len() {
        let rest = frame.get(sent..).unwrap_or_default();
        let iov = [IoSlice::new(rest)];
        let cm: &[ControlMessage<'_>] = if sent == 0 { &cmsgs } else { &[] };
        let n = sendmsg::<()>(socket.as_raw_fd(), &iov, cm, MsgFlags::MSG_NOSIGNAL, None)
            .map_err(io::Error::from)?;
        if n == 0 {
            return Err(io::Error::from(io::ErrorKind::WriteZero));
        }
        sent = sent.saturating_add(n);
    }
    Ok(())
}

// [@ANCHOR: relay_broker_recv_exact]
/// Fills `buf` with `recvmsg`, appending any passed descriptors to `fds`. Descriptors beyond
/// the first four are closed on the spot, never leaked. The whole fill must finish by
/// `deadline`: the socket's own receive timeout restarts on every byte, so alone it would let a
/// peer that sends one byte just inside each timeout hold the (single-threaded) broker for
/// hours. Before each `recvmsg` the socket timeout is cut to the time that remains.
fn recv_exact(
    socket: BorrowedFd<'_>,
    buf: &mut [u8],
    fds: &mut Vec<OwnedFd>,
    deadline: Instant,
) -> io::Result<()> {
    let mut filled = 0usize;
    while filled < buf.len() {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(io::Error::from(io::ErrorKind::TimedOut));
        }
        // A zero timeval means "wait forever", so the value never goes below one microsecond.
        let secs = i64::try_from(remaining.as_secs()).unwrap_or(i64::MAX);
        let micros = i64::from(remaining.subsec_micros()).max(i64::from(secs == 0));
        setsockopt(
            &socket,
            sockopt::ReceiveTimeout,
            &TimeVal::new(secs, micros),
        )
        .map_err(io::Error::from)?;
        let mut space = vec![0u8; cmsg_space::<[RawFd; 4]>()];
        let mut iov = [IoSliceMut::new(buf.get_mut(filled..).unwrap_or_default())];
        let msg = recvmsg::<()>(
            socket.as_raw_fd(),
            &mut iov,
            Some(&mut space),
            MsgFlags::MSG_CMSG_CLOEXEC,
        )
        .map_err(io::Error::from)?;
        for cmsg in msg.cmsgs().map_err(io::Error::from)? {
            if let ControlMessageOwned::ScmRights(received) = cmsg {
                for raw in received {
                    // Wrapped in an OwnedFd at once, so every received descriptor is closed
                    // when dropped, kept or not.
                    let owned = adopt_received_fd(raw);
                    if fds.len() < 4 {
                        fds.push(owned);
                    }
                }
            }
        }
        if msg.bytes == 0 {
            return Err(io::Error::from(io::ErrorKind::UnexpectedEof));
        }
        filled = filled.saturating_add(msg.bytes);
    }
    Ok(())
}

// [@ANCHOR: relay_broker_adopt_received_fd]
/// Wraps a descriptor that `recvmsg` just received as SCM_RIGHTS. This is the crate's only
/// `unsafe` block: nix 0.31 returns passed descriptors as plain integers, and
/// `OwnedFd::from_raw_fd` is `unsafe` only because the caller must promise sole ownership.
#[allow(unsafe_code)]
fn adopt_received_fd(raw: RawFd) -> OwnedFd {
    // SAFETY: `raw` was created by the kernel for this process in the recvmsg call that just
    // returned, nothing else holds it, and it is wrapped before any other code can see it.
    unsafe { OwnedFd::from_raw_fd(raw) }
}

// [@ANCHOR: relay_broker_recv_frame]
/// Reads one frame: the body bytes and any descriptors passed with it. The announced length is
/// checked against `MAX_FRAME` before the body is read, so an oversize frame costs the peer a
/// closed connection and the broker four bytes. The whole frame, header and body together, must
/// arrive within `IO_TIMEOUT`, however the peer paces its bytes.
pub fn recv_frame(socket: BorrowedFd<'_>) -> io::Result<(Vec<u8>, Vec<OwnedFd>)> {
    let deadline = Instant::now() + IO_TIMEOUT;
    let mut fds = Vec::new();
    let mut header = [0u8; 4];
    recv_exact(socket, &mut header, &mut fds, deadline)?;
    let len = frame_body_len(header).map_err(|e: WireError| {
        io::Error::new(io::ErrorKind::InvalidData, format!("frame length: {e:?}"))
    })?;
    debug_assert!(len <= MAX_FRAME);
    let mut body = vec![0u8; len];
    recv_exact(socket, &mut body, &mut fds, deadline)?;
    Ok((body, fds))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::os::fd::AsFd;
    use std::os::unix::net::UnixStream;

    // Tests [@ANCHOR: relay_broker_send_frame]
    // Tests [@ANCHOR: relay_broker_recv_frame]
    // Tests [@ANCHOR: relay_broker_recv_exact]
    // Tests [@ANCHOR: relay_broker_adopt_received_fd]
    // Tests [@ANCHOR: relay_broker_set_timeouts]
    #[test]
    fn a_frame_and_a_descriptor_cross_a_socketpair_and_oversize_is_refused() {
        let (a, b) = UnixStream::pair().unwrap();
        set_timeouts(a.as_fd()).unwrap();
        set_timeouts(b.as_fd()).unwrap();
        let (mut w, r) = std::os::unix::net::UnixStream::pair().unwrap();
        send_frame(a.as_fd(), b"hello", Some(r.as_fd())).unwrap();
        let (body, fds) = recv_frame(b.as_fd()).unwrap();
        assert_eq!(body, b"hello");
        assert_eq!(fds.len(), 1);
        w.write_all(b"via passed fd").unwrap();
        let mut got = [0u8; 13];
        let mut passed = std::os::unix::net::UnixStream::from(fds.into_iter().next().unwrap());
        passed.read_exact(&mut got).unwrap();
        assert_eq!(&got, b"via passed fd");

        send_frame(a.as_fd(), &[7u8; MAX_FRAME], None).unwrap();
        let (body, fds) = recv_frame(b.as_fd()).unwrap();
        assert_eq!(body.len(), MAX_FRAME);
        assert!(fds.is_empty());

        let (mut raw_w, raw_r) = UnixStream::pair().unwrap();
        raw_w.write_all(&[0, 1, 0, 0, 9, 9]).unwrap();
        let err = recv_frame(raw_r.as_fd()).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
        let (mut raw_w, raw_r) = UnixStream::pair().unwrap();
        raw_w.write_all(&[0, 0, 0, 0]).unwrap();
        assert!(recv_frame(raw_r.as_fd()).is_err(), "zero length refused");
        let (raw_w, raw_r) = UnixStream::pair().unwrap();
        drop(raw_w);
        assert_eq!(
            recv_frame(raw_r.as_fd()).unwrap_err().kind(),
            io::ErrorKind::UnexpectedEof
        );
        let (mut raw_w, raw_r) = UnixStream::pair().unwrap();
        raw_w.write_all(&[0, 0, 0, 5, 1, 2]).unwrap();
        drop(raw_w);
        assert!(recv_frame(raw_r.as_fd()).is_err(), "truncated body");
    }

    // Tests [@ANCHOR: relay_broker_recv_frame]
    // Tests [@ANCHOR: relay_broker_recv_exact]
    #[test]
    fn a_peer_dribbling_one_byte_per_second_cannot_outlast_the_whole_frame_deadline() {
        use std::time::Instant;
        let (mut raw_w, raw_r) = UnixStream::pair().unwrap();
        // Announces a 100-byte body, then sends one byte a second: each byte lands inside the
        // 5 s socket timeout, so only a deadline on the whole frame can stop it.
        let feeder = std::thread::spawn(move || {
            for byte in [0u8, 0, 0, 100]
                .into_iter()
                .chain(std::iter::repeat_n(1u8, 20))
            {
                if raw_w.write_all(&[byte]).is_err() {
                    return;
                }
                std::thread::sleep(Duration::from_secs(1));
            }
        });
        let started = Instant::now();
        let err = recv_frame(raw_r.as_fd()).unwrap_err();
        let waited = started.elapsed();
        assert!(
            matches!(
                err.kind(),
                io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
            ),
            "{err:?}"
        );
        assert!(
            waited < IO_TIMEOUT + Duration::from_secs(2),
            "held for {waited:?}; the frame has {IO_TIMEOUT:?} in total"
        );
        drop(raw_r);
        feeder.join().unwrap();
    }
}
