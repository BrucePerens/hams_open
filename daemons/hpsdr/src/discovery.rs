// SPDX-License-Identifier: AGPL-3.0-or-later
//! Discovery: find radios by UDP broadcast (or by unicast to a known address).
//!
//! Protocol 1: Metis v1.33 "Metis Discover Packet format"; Protocol 2: spec v4.4 "Discovery Packet".
//! Both use UDP port 1024 and the same destination, `255.255.255.255:1024` (Metis notes the subnet
//! broadcast address is not implemented in the firmware).

use crate::error::HpsdrError;
use crate::types::{Board, Protocol};
use crate::{p1, p2};
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::time::Duration;
use tokio::net::UdpSocket;
use tokio::runtime::Handle;
use tokio::time::Instant as TokioInstant;

/// A radio that answered discovery.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Discovered {
    /// Wire protocol it speaks.
    pub protocol: Protocol,
    /// Where it answered from (use this address for everything else).
    pub addr: SocketAddr,
    /// MAC address.
    pub mac: [u8; 6],
    /// Board type.
    pub board: Board,
    /// Gateware version byte.
    pub code_version: u8,
    /// Already streaming to a host (status 0x03).
    pub busy: bool,
    /// Protocol version implemented (Protocol 2 byte 12).
    pub protocol_version: Option<u8>,
    /// Number of DDCs implemented (Protocol 2 byte 20).
    pub num_ddcs: Option<u8>,
    /// Protocol 2 radio wants frequencies as phase words (byte 21); false for Protocol 1.
    pub phase_word: bool,
}

// [@ANCHOR: hpsdr_discovery_broadcast_target]
/// The limited broadcast address and discovery port.
pub fn broadcast_target() -> SocketAddr {
    SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::BROADCAST, p1::PORT))
}

// [@ANCHOR: hpsdr_discovery_parse_reply]
/// Classify and parse one reply datagram received from `from`.
pub fn parse_reply(from: SocketAddr, d: &[u8]) -> Result<Discovered, HpsdrError> {
    if d.len() >= 2 && d[..2] != [0xEF, 0xFE] {
        let r = p2::parse_reply(d)?;
        return Ok(Discovered {
            protocol: Protocol::P2,
            addr: from,
            mac: r.mac,
            board: r.board,
            code_version: r.firmware,
            busy: r.running,
            protocol_version: Some(r.protocol_version),
            num_ddcs: Some(r.num_ddcs),
            phase_word: r.phase_word,
        });
    }
    let r = p1::parse_reply(d)?;
    Ok(Discovered {
        protocol: Protocol::P1,
        addr: from,
        mac: r.mac,
        board: r.board,
        code_version: r.code_version,
        busy: r.running,
        protocol_version: None,
        num_ddcs: None,
        phase_word: false,
    })
}

// [@ANCHOR: hpsdr_discovery_udp_socket]
/// Open a non-blocking UDP socket registered with the runtime behind `handle`.
pub(crate) fn udp_socket(handle: &Handle, bind: SocketAddr) -> Result<UdpSocket, HpsdrError> {
    let std_sock = std::net::UdpSocket::bind(bind)?;
    std_sock.set_nonblocking(true)?;
    let _guard = handle.enter();
    Ok(UdpSocket::from_std(std_sock)?)
}

// [@ANCHOR: hpsdr_discovery_discover]
/// Send the discovery request to every address in `targets` and collect replies for `wait`. Use
/// [`broadcast_target`] to find radios; a unicast address finds one radio. Replies that do not parse
/// are skipped; each radio is listed once.
pub async fn discover(
    handle: &Handle,
    targets: &[SocketAddr],
    wait: Duration,
) -> Result<Vec<Discovered>, HpsdrError> {
    let sock = udp_socket(handle, SocketAddr::from((Ipv4Addr::UNSPECIFIED, 0)))?;
    sock.set_broadcast(true)?;
    for t in targets {
        // A Protocol 1 radio answers the Metis packet and ignores the other; a Protocol 2 radio the reverse.
        sock.send_to(&p1::discovery_packet(), t).await?;
        sock.send_to(&p2::discovery_packet(), t).await?;
    }
    let deadline = TokioInstant::now() + wait;
    let mut found: Vec<Discovered> = Vec::new();
    let mut buf = [0u8; 2048];
    while let Ok(r) = tokio::time::timeout_at(deadline, sock.recv_from(&mut buf)).await {
        let (n, from) = r?;
        if let Ok(d) = parse_reply(from, &buf[..n]) {
            if !found.iter().any(|f| f.mac == d.mac && f.addr == d.addr) {
                found.push(d);
            }
        }
    }
    Ok(found)
}
