// SPDX-License-Identifier: AGPL-3.0-or-later
//! Multicast sockets for the control/status group and the RTP group.
//!
//! Both sockets use a time-to-live of 0, so nothing leaves the host, and a specific interface
//! address (use `127.0.0.1` for loopback). `radiod` sends status to port 5006 and RTP to 5004 of
//! the groups named in its configuration.

use crate::command::{Command, CommandError, Policy};
use crate::status::ChannelStatus;
use crate::tlv::TlvError;
use socket2::{Domain, Protocol, Socket, Type};
use std::fmt;
use std::io;
use std::net::{Ipv4Addr, SocketAddrV4, UdpSocket};
use std::time::{Duration, Instant};

/// Default status/command port.
pub const STATUS_PORT: u16 = 5006;
/// Default RTP port.
pub const RTP_PORT: u16 = 5004;

/// Network or protocol failure.
#[derive(Debug)]
pub enum NetError {
    /// Socket error.
    Io(io::Error),
    /// The command was refused before sending.
    Command(CommandError),
    /// A received datagram was malformed (counted and skipped by the receive helpers).
    Tlv(TlvError),
    /// Group is not an IPv4 multicast address.
    NotMulticast(Ipv4Addr),
}

impl fmt::Display for NetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "io: {e}"),
            Self::Command(e) => write!(f, "command: {e}"),
            Self::Tlv(e) => write!(f, "tlv: {e}"),
            Self::NotMulticast(a) => write!(f, "{a} is not a multicast address"),
        }
    }
}

impl std::error::Error for NetError {}

impl From<io::Error> for NetError {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

fn open_group(group: Ipv4Addr, port: u16, iface: Ipv4Addr) -> Result<UdpSocket, NetError> {
    if !group.is_multicast() {
        return Err(NetError::NotMulticast(group));
    }
    let s = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;
    s.set_reuse_address(true)?;
    s.set_reuse_port(true)?;
    s.bind(&SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, port).into())?;
    s.join_multicast_v4(&group, &iface)?;
    s.set_multicast_if_v4(&iface)?;
    s.set_multicast_ttl_v4(0)?;
    s.set_multicast_loop_v4(true)?;
    s.set_recv_buffer_size(4 << 20)?;
    Ok(s.into())
}

/// The control/status link to one `radiod` instance.
#[derive(Debug)]
pub struct Control {
    sock: UdpSocket,
    dest: SocketAddrV4,
    policy: Policy,
    next_tag: u32,
    /// Datagrams skipped because they did not parse.
    pub malformed: u64,
}

impl Control {
    /// Join the status group and prepare to send commands to it.
    pub fn open(group: Ipv4Addr, iface: Ipv4Addr, policy: Policy) -> Result<Self, NetError> {
        let sock = open_group(group, STATUS_PORT, iface)?;
        Ok(Self {
            sock,
            dest: SocketAddrV4::new(group, STATUS_PORT),
            policy,
            next_tag: 1,
            malformed: 0,
        })
    }

    /// Validate and send a command; returns the tag `radiod` will echo in its reply.
    pub fn send(&mut self, cmd: &Command) -> Result<u32, NetError> {
        let tag = self.next_tag;
        self.next_tag = self.next_tag.wrapping_add(1).max(1);
        let pkt = cmd.encode(&self.policy, tag).map_err(NetError::Command)?;
        self.sock.send_to(&pkt, self.dest)?;
        Ok(tag)
    }

    /// Wait up to `timeout` for the next status datagram, skipping our own commands and
    /// malformed datagrams. `Ok(None)` on timeout.
    pub fn recv_status(&mut self, timeout: Duration) -> Result<Option<ChannelStatus>, NetError> {
        let deadline = Instant::now() + timeout;
        let mut buf = vec![0u8; 65_536];
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Ok(None);
            }
            self.sock.set_read_timeout(Some(left))?;
            let n = match self.sock.recv(&mut buf) {
                Ok(n) => n,
                Err(e)
                    if e.kind() == io::ErrorKind::WouldBlock
                        || e.kind() == io::ErrorKind::TimedOut =>
                {
                    return Ok(None)
                }
                Err(e) => return Err(e.into()),
            };
            match ChannelStatus::decode(&buf[..n]) {
                Ok(Some(s)) => return Ok(Some(s)),
                Ok(None) => {}
                Err(_) => self.malformed += 1,
            }
        }
    }

    /// Send `cmd` and wait for the status carrying its tag (the only acknowledgement `radiod`
    /// gives). Retries the send every 200 ms up to `timeout`; sends are idempotent.
    pub fn request(
        &mut self,
        cmd: &Command,
        timeout: Duration,
    ) -> Result<Option<ChannelStatus>, NetError> {
        let deadline = Instant::now() + timeout;
        let want = cmd.ssrc().get();
        while Instant::now() < deadline {
            let tag = self.send(cmd)?;
            let retry = Instant::now() + Duration::from_millis(200);
            while Instant::now() < retry {
                let left = retry.saturating_duration_since(Instant::now());
                if let Some(s) = self.recv_status(left)? {
                    if s.ssrc == want && s.tag == Some(u64::from(tag)) {
                        return Ok(Some(s));
                    }
                }
            }
        }
        Ok(None)
    }
}

/// An RTP receiver for one multicast group.
#[derive(Debug)]
pub struct RtpSocket {
    sock: UdpSocket,
}

impl RtpSocket {
    /// Join the data group.
    pub fn open(group: Ipv4Addr, iface: Ipv4Addr) -> Result<Self, NetError> {
        Ok(Self {
            sock: open_group(group, RTP_PORT, iface)?,
        })
    }
    /// Receive one datagram into `buf`; `Ok(None)` on timeout.
    pub fn recv(&self, buf: &mut [u8], timeout: Duration) -> Result<Option<usize>, NetError> {
        self.sock
            .set_read_timeout(Some(timeout.max(Duration::from_millis(1))))?;
        match self.sock.recv(buf) {
            Ok(n) => Ok(Some(n)),
            Err(e)
                if e.kind() == io::ErrorKind::WouldBlock || e.kind() == io::ErrorKind::TimedOut =>
            {
                Ok(None)
            }
            Err(e) => Err(e.into()),
        }
    }
}
