// SPDX-License-Identifier: AGPL-3.0-or-later
//! Multicast sockets for the control/status group and the RTP group, on tokio.
//!
//! This module owns no runtime. The caller passes a [`tokio::runtime::Handle`] when a socket is
//! opened, and every receive is an `async fn` the caller awaits on its own runtime. The wire
//! format and the acknowledgement rules are sans-IO functions ([`next_tag`], [`is_ack`],
//! [`classify`]) that are tested without a socket.
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
use std::net::{Ipv4Addr, SocketAddrV4};
use std::time::Duration;
use tokio::net::UdpSocket;
use tokio::runtime::Handle;
use tokio::time::{timeout, Instant};

/// Default status/command port.
pub const STATUS_PORT: u16 = 5006;
/// Default RTP port.
pub const RTP_PORT: u16 = 5004;
/// Largest datagram `radiod` sends (its status buffer is 65,536 bytes).
const MAX_DATAGRAM: usize = 65_536;
/// How long `request` waits for an acknowledgement before it repeats the (idempotent) command.
const RETRY_INTERVAL: Duration = Duration::from_millis(200);

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
    // [@ANCHOR: radiod_client_net_error_fmt]
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
    // [@ANCHOR: radiod_client_net_error_from_io]
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

// [@ANCHOR: radiod_client_net_next_tag]
/// The tag after `tag`: it increments and never becomes 0 (a zero tag is an empty integer on
/// the wire and cannot be told from an absent one).
pub fn next_tag(tag: u32) -> u32 {
    tag.wrapping_add(1).max(1)
}

// [@ANCHOR: radiod_client_net_is_ack]
/// True when `status` is `radiod`'s reply to the command with this channel and tag.
pub fn is_ack(status: &ChannelStatus, ssrc: u32, tag: u32) -> bool {
    status.ssrc == ssrc && status.tag == Some(u64::from(tag))
}

// [@ANCHOR: radiod_client_net_classify]
/// Decode one received datagram: a status is returned, our own looped-back commands are
/// skipped, and a malformed datagram is counted in `malformed` and skipped.
pub fn classify(datagram: &[u8], malformed: &mut u64) -> Option<ChannelStatus> {
    match ChannelStatus::decode(datagram) {
        Ok(status) => status,
        Err(_) => {
            *malformed += 1;
            None
        }
    }
}

// [@ANCHOR: radiod_client_net_open_group]
/// Open a multicast receive socket bound to `port` and joined to `group` on `iface`, with a
/// TTL of 0 so nothing it sends leaves the host. The returned socket is non-blocking.
fn open_group(
    group: Ipv4Addr,
    port: u16,
    iface: Ipv4Addr,
) -> Result<std::net::UdpSocket, NetError> {
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
    s.set_nonblocking(true)?;
    Ok(s.into())
}

// [@ANCHOR: radiod_client_net_register]
/// Register a socket with the runtime behind `handle`.
fn register(handle: &Handle, sock: std::net::UdpSocket) -> Result<UdpSocket, NetError> {
    let _guard = handle.enter();
    Ok(UdpSocket::from_std(sock)?)
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
    // [@ANCHOR: radiod_client_net_control_open]
    /// Join the status group (port [`STATUS_PORT`]) and prepare to send commands to it. The
    /// socket is registered with the runtime behind `handle`; this crate never builds one.
    pub fn open(
        handle: &Handle,
        group: Ipv4Addr,
        iface: Ipv4Addr,
        policy: Policy,
    ) -> Result<Self, NetError> {
        Self::open_on_port(handle, group, STATUS_PORT, iface, policy)
    }

    // [@ANCHOR: radiod_client_net_control_open_on_port]
    /// As [`Control::open`], for a `radiod` configured with a status port other than 5006.
    pub fn open_on_port(
        handle: &Handle,
        group: Ipv4Addr,
        port: u16,
        iface: Ipv4Addr,
        policy: Policy,
    ) -> Result<Self, NetError> {
        let sock = register(handle, open_group(group, port, iface)?)?;
        Ok(Self {
            sock,
            dest: SocketAddrV4::new(group, port),
            policy,
            next_tag: 1,
            malformed: 0,
        })
    }

    // [@ANCHOR: radiod_client_net_control_send]
    /// Validate and send a command; returns the tag `radiod` will echo in its reply. A command
    /// the policy refuses is never put on the wire.
    pub async fn send(&mut self, cmd: &Command) -> Result<u32, NetError> {
        let tag = self.next_tag;
        let pkt = cmd.encode(&self.policy, tag).map_err(NetError::Command)?;
        self.next_tag = next_tag(tag);
        self.sock.send_to(&pkt, self.dest).await?;
        Ok(tag)
    }

    // [@ANCHOR: radiod_client_net_control_recv_status]
    /// Wait up to `wait` for the next status datagram, skipping our own commands and
    /// malformed datagrams. `Ok(None)` on timeout.
    pub async fn recv_status(&mut self, wait: Duration) -> Result<Option<ChannelStatus>, NetError> {
        let deadline = Instant::now() + wait;
        let mut buf = vec![0u8; MAX_DATAGRAM];
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Ok(None);
            }
            let n = match timeout(left, self.sock.recv(&mut buf)).await {
                Err(_elapsed) => return Ok(None),
                Ok(received) => received?,
            };
            if let Some(status) = classify(&buf[..n], &mut self.malformed) {
                return Ok(Some(status));
            }
        }
    }

    // [@ANCHOR: radiod_client_net_control_request]
    /// Send `cmd` and wait for the status carrying its tag (the only acknowledgement `radiod`
    /// gives). Repeats the send every 200 ms up to `wait`; sends are idempotent.
    pub async fn request(
        &mut self,
        cmd: &Command,
        wait: Duration,
    ) -> Result<Option<ChannelStatus>, NetError> {
        let deadline = Instant::now() + wait;
        let want = cmd.ssrc().get();
        while Instant::now() < deadline {
            let tag = self.send(cmd).await?;
            let retry = Instant::now() + RETRY_INTERVAL;
            while Instant::now() < retry {
                let left = retry.saturating_duration_since(Instant::now());
                if let Some(status) = self.recv_status(left).await? {
                    if is_ack(&status, want, tag) {
                        return Ok(Some(status));
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
    // [@ANCHOR: radiod_client_net_rtp_open]
    /// Join the data group (port [`RTP_PORT`]); the socket is registered with the runtime
    /// behind `handle`.
    pub fn open(handle: &Handle, group: Ipv4Addr, iface: Ipv4Addr) -> Result<Self, NetError> {
        Self::open_on_port(handle, group, RTP_PORT, iface)
    }

    // [@ANCHOR: radiod_client_net_rtp_open_on_port]
    /// As [`RtpSocket::open`], for a `radiod` configured with a data port other than 5004.
    pub fn open_on_port(
        handle: &Handle,
        group: Ipv4Addr,
        port: u16,
        iface: Ipv4Addr,
    ) -> Result<Self, NetError> {
        Ok(Self {
            sock: register(handle, open_group(group, port, iface)?)?,
        })
    }

    // [@ANCHOR: radiod_client_net_rtp_recv]
    /// Receive one datagram into `buf`; `Ok(None)` after `wait` with nothing received.
    pub async fn recv(&self, buf: &mut [u8], wait: Duration) -> Result<Option<usize>, NetError> {
        match timeout(wait, self.sock.recv(buf)).await {
            Err(_elapsed) => Ok(None),
            Ok(received) => Ok(Some(received?)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::{AudioChannel, Encoding, Mode, SpectrumChannel, Ssrc};
    use crate::tlv::{parse, ty, Kind};

    const LOOPBACK: Ipv4Addr = Ipv4Addr::new(127, 0, 0, 1);

    /// A port that differs per test and per process, so concurrent test runs do not share one.
    fn test_port(slot: u16) -> u16 {
        20_000 + (std::process::id() % 20_000) as u16 + slot * 3
    }

    fn group(last: u8) -> Ipv4Addr {
        Ipv4Addr::new(239, 255, 77, last)
    }

    // Test helper only: 1..=0xfffffffe are valid by construction.
    #[allow(clippy::unwrap_used)]
    fn ssrc(n: u32) -> Ssrc {
        Ssrc::new(n).unwrap()
    }

    fn spectrum(bins: u32, byte_format: bool) -> Command {
        Command::Spectrum(SpectrumChannel {
            ssrc: ssrc(5),
            center_hz: 10.0e6,
            bin_count: bins,
            bin_hz: 100.0,
            averaging: 1,
            lifetime_frames: 100,
            byte_format,
        })
    }

    /// Reads the next datagram from `peer` or `None` after `wait`.
    async fn next_datagram(peer: &UdpSocket, wait: Duration) -> Option<Vec<u8>> {
        let mut buf = vec![0u8; MAX_DATAGRAM];
        match timeout(wait, peer.recv(&mut buf)).await {
            Ok(Ok(n)) => Some(buf[..n].to_vec()),
            _ => None,
        }
    }

    // Tests [@ANCHOR: radiod_client_net_next_tag]
    // Tests [@ANCHOR: radiod_client_net_is_ack]
    // Tests [@ANCHOR: radiod_client_net_classify]
    #[test]
    fn acknowledgement_rules_are_pure() {
        assert_eq!(next_tag(1), 2);
        assert_eq!(next_tag(u32::MAX), 1, "wraps past zero");
        assert_eq!(next_tag(0), 1);
        let reply = ChannelStatus {
            ssrc: 5,
            tag: Some(9),
            ..ChannelStatus::default()
        };
        assert!(is_ack(&reply, 5, 9));
        assert!(!is_ack(&reply, 6, 9));
        assert!(!is_ack(&reply, 5, 10));
        assert!(!is_ack(&ChannelStatus::default(), 0, 1));

        let mut malformed = 0;
        let status = [0u8, ty::OUTPUT_SSRC, 1, 7, ty::EOL];
        assert_eq!(classify(&status, &mut malformed).map(|s| s.ssrc), Some(7));
        let own_command = [1u8, ty::OUTPUT_SSRC, 1, 7, ty::EOL];
        assert!(classify(&own_command, &mut malformed).is_none());
        assert_eq!(malformed, 0);
        assert!(classify(&[0u8, ty::OUTPUT_SSRC, 9], &mut malformed).is_none());
        assert!(classify(&[], &mut malformed).is_none());
        assert_eq!(malformed, 2);
    }

    // Tests [@ANCHOR: radiod_client_net_error_fmt]
    // Tests [@ANCHOR: radiod_client_net_error_from_io]
    #[test]
    fn errors_display() {
        let io: NetError = io::Error::other("boom").into();
        assert_eq!(io.to_string(), "io: boom");
        let cmd = NetError::Command(CommandError::BinCount(3));
        assert_eq!(cmd.to_string(), "command: BinCount(3)");
        assert!(NetError::Tlv(TlvError::TooShort)
            .to_string()
            .starts_with("tlv:"));
        let g = Ipv4Addr::new(10, 0, 0, 1);
        assert_eq!(
            NetError::NotMulticast(g).to_string(),
            "10.0.0.1 is not a multicast address"
        );
    }

    // Tests [@ANCHOR: radiod_client_net_open_group]
    // Tests [@ANCHOR: radiod_client_net_control_open]
    // Tests [@ANCHOR: radiod_client_net_rtp_open]
    #[tokio::test]
    async fn only_multicast_groups_open() {
        let handle = Handle::current();
        let unicast = Ipv4Addr::new(192, 0, 2, 1);
        assert!(matches!(
            Control::open(&handle, unicast, LOOPBACK, Policy::default()),
            Err(NetError::NotMulticast(_))
        ));
        assert!(matches!(
            RtpSocket::open(&handle, unicast, LOOPBACK),
            Err(NetError::NotMulticast(_))
        ));
    }

    // Tests [@ANCHOR: radiod_client_net_control_open_on_port]
    // Tests [@ANCHOR: radiod_client_net_register]
    // Tests [@ANCHOR: radiod_client_net_control_send]
    #[tokio::test]
    async fn refused_commands_never_reach_the_wire() {
        let handle = Handle::current();
        let (g, port) = (group(1), test_port(0));
        let opened = open_group(g, port, LOOPBACK).and_then(|s| register(&handle, s));
        assert!(opened.is_ok(), "multicast on loopback: {opened:?}");
        let Ok(listener) = opened else { return };
        let control = Control::open_on_port(&handle, g, port, LOOPBACK, Policy::default());
        assert!(control.is_ok(), "{control:?}");
        let Ok(mut control) = control else { return };

        // A bare poll of an unknown SSRC (no lifetime), an oversized spectrum request in both
        // formats, and a channel at a frequency outside the policy: all refused locally.
        let bare_poll = Command::Poll {
            ssrc: ssrc(777),
            lifetime_frames: 0,
        };
        let refused = [
            bare_poll,
            spectrum(17_000, false),
            spectrum(66_000, true),
            spectrum(u32::MAX, true),
            Command::Audio(AudioChannel {
                ssrc: ssrc(1),
                mode: Mode::Usb,
                frequency_hz: 1.0e12,
                encoding: Encoding::Pcm16Be,
                lifetime_frames: 10,
            }),
        ];
        for cmd in &refused {
            assert!(matches!(control.send(cmd).await, Err(NetError::Command(_))));
        }
        assert!(next_datagram(&listener, Duration::from_millis(300))
            .await
            .is_none());

        // A valid poll goes out, carries a lifetime, and the tag counter moved only for it.
        let poll = Command::Poll {
            ssrc: ssrc(777),
            lifetime_frames: 25,
        };
        assert_eq!(control.send(&poll).await.ok(), Some(1));
        let sent = next_datagram(&listener, Duration::from_secs(2)).await;
        assert!(sent.is_some());
        let sent = sent.unwrap_or_default();
        let (kind, entries) = parse(&sent).unwrap_or_else(|_| (Kind::Status, vec![]));
        assert_eq!(kind, Kind::Command);
        assert!(entries
            .iter()
            .any(|e| e.ty == ty::LIFETIME && e.uint() == Ok(25)));
        assert!(entries.iter().all(|e| e.ty != ty::OUTPUT_DATA_DEST_SOCKET));
        assert_eq!(control.send(&poll).await.ok(), Some(2));
    }

    // Tests [@ANCHOR: radiod_client_net_control_request]
    // Tests [@ANCHOR: radiod_client_net_control_recv_status]
    // Tests [@ANCHOR: radiod_client_net_control_send]
    #[tokio::test]
    async fn request_is_acknowledged_by_tag() {
        let handle = Handle::current();
        let (g, port) = (group(2), test_port(1));
        let opened = open_group(g, port, LOOPBACK).and_then(|s| register(&handle, s));
        assert!(opened.is_ok(), "multicast on loopback: {opened:?}");
        let Ok(peer) = opened else { return };
        let control = Control::open_on_port(&handle, g, port, LOOPBACK, Policy::default());
        assert!(control.is_ok(), "{control:?}");
        let Ok(mut control) = control else { return };

        // A peer that answers the way radiod does: one status per command, echoing the tag,
        // first a status about another channel, then the real reply.
        let responder = tokio::spawn(async move {
            let dest = SocketAddrV4::new(g, port);
            while let Some(pkt) = next_datagram(&peer, Duration::from_secs(3)).await {
                let Ok((Kind::Command, entries)) = parse(&pkt) else {
                    continue;
                };
                let uint = |t: u8| {
                    entries
                        .iter()
                        .find(|e| e.ty == t)
                        .and_then(|e| e.uint().ok())
                        .unwrap_or(0)
                };
                let (ssrc, tag) = (uint(ty::OUTPUT_SSRC) as u8, uint(ty::COMMAND_TAG) as u8);
                let other = [
                    0u8,
                    ty::OUTPUT_SSRC,
                    1,
                    ssrc.wrapping_add(1),
                    ty::COMMAND_TAG,
                    1,
                    tag,
                    ty::EOL,
                ];
                let reply = [
                    0u8,
                    ty::OUTPUT_SSRC,
                    1,
                    ssrc,
                    ty::COMMAND_TAG,
                    1,
                    tag,
                    ty::EOL,
                ];
                let _ = peer.send_to(&other, dest).await;
                let _ = peer.send_to(&reply, dest).await;
            }
        });

        let poll = Command::Poll {
            ssrc: ssrc(42),
            lifetime_frames: 50,
        };
        let ack = control.request(&poll, Duration::from_secs(3)).await;
        let status = ack.ok().flatten();
        assert_eq!(status.as_ref().map(|s| s.ssrc), Some(42));
        assert_eq!(status.and_then(|s| s.tag), Some(1));

        responder.abort();
    }

    // Tests [@ANCHOR: radiod_client_net_rtp_open_on_port]
    // Tests [@ANCHOR: radiod_client_net_rtp_recv]
    #[tokio::test]
    async fn rtp_socket_receives_and_times_out() {
        let handle = Handle::current();
        let (g, port) = (group(3), test_port(2));
        let rtp = RtpSocket::open_on_port(&handle, g, port, LOOPBACK);
        assert!(rtp.is_ok(), "{rtp:?}");
        let Ok(rtp) = rtp else { return };
        let mut buf = [0u8; 64];
        assert!(matches!(
            rtp.recv(&mut buf, Duration::from_millis(50)).await,
            Ok(None)
        ));
        let sender = open_group(g, port, LOOPBACK).and_then(|s| register(&handle, s));
        assert!(sender.is_ok(), "{sender:?}");
        let Ok(sender) = sender else { return };
        let sent = sender
            .send_to(&[0x80, 1, 2, 3], SocketAddrV4::new(g, port))
            .await;
        assert!(sent.is_ok());
        let got = rtp.recv(&mut buf, Duration::from_secs(2)).await;
        assert!(matches!(got, Ok(Some(4))), "{got:?}");
    }

    // Tests [@ANCHOR: radiod_client_net_control_request]
    // Tests [@ANCHOR: radiod_client_net_control_recv_status]
    #[tokio::test]
    async fn unanswered_request_times_out_and_retries() {
        let handle = Handle::current();
        let (g, port) = (group(4), test_port(3));
        let opened = open_group(g, port, LOOPBACK).and_then(|s| register(&handle, s));
        assert!(opened.is_ok(), "{opened:?}");
        let Ok(listener) = opened else { return };
        let control = Control::open_on_port(&handle, g, port, LOOPBACK, Policy::default());
        assert!(control.is_ok(), "{control:?}");
        let Ok(mut control) = control else { return };
        let poll = Command::Poll {
            ssrc: ssrc(42),
            lifetime_frames: 50,
        };
        let status = control.request(&poll, Duration::from_millis(500)).await;
        assert!(matches!(status, Ok(None)), "{status:?}");
        let mut seen = 0;
        while next_datagram(&listener, Duration::from_millis(50))
            .await
            .is_some()
        {
            seen += 1;
        }
        assert!(
            seen >= 2,
            "the command is repeated while unacknowledged: {seen}"
        );
        // The listener sees our own commands, which recv_status skips.
        let nothing = control.recv_status(Duration::from_millis(50)).await;
        assert!(matches!(nothing, Ok(None)), "{nothing:?}");
    }
}
