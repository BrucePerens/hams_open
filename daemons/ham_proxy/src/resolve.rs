//! Upstream address choice for the site gateway: `.ham` names are looked up (AAAA) at the Hams.com Net resolver given with
//! `--ham-resolver`, because an AREDN node's own dnsmasq drops every AAAA answer and answers `.ham` A queries with the gateway
//! itself. Whatever the lookup gives, the gateway never connects to a loopback, private, link-local or unspecified address,
//! so a name can never lead it to the operator's home LAN.

use crate::ProxyError;
use std::net::{IpAddr, Ipv6Addr, SocketAddr, TcpStream, ToSocketAddrs, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;
use std::time::Duration;

static HAM_RESOLVER: OnceLock<String> = OnceLock::new();
static ALLOW_PRIVATE: AtomicBool = AtomicBool::new(false);

// [@ANCHOR: ham_proxy_allow_private_upstream]
/// Let the gateway connect to loopback and private addresses (`--allow-private-upstream`): for a lab with a local site only.
pub fn allow_private_upstream() {
    ALLOW_PRIVATE.store(true, Ordering::Relaxed);
}

// [@ANCHOR: ham_proxy_set_ham_resolver]
/// Use `resolver` (`ip:port`, UDP) for `.ham` names from now on; the first call wins.
pub fn set_ham_resolver(resolver: &str) {
    let _ = HAM_RESOLVER.set(resolver.to_string());
}

// [@ANCHOR: ham_proxy_aaaa_query]
/// A DNS query for the AAAA record of `host`.
pub fn aaaa_query(host: &str, id: u16) -> Vec<u8> {
    let mut q = id.to_be_bytes().to_vec();
    q.extend_from_slice(&[1, 0, 0, 1, 0, 0, 0, 0, 0, 0]);
    for label in host.trim_end_matches('.').split('.') {
        let l = &label.as_bytes()[..label.len().min(63)];
        q.push(l.len() as u8);
        q.extend_from_slice(l);
    }
    q.push(0);
    q.extend_from_slice(&[0, 28, 0, 1]); // AAAA, IN
    q
}

// [@ANCHOR: ham_proxy_resolve_skip_name]
fn skip_name(p: &[u8], mut i: usize) -> Option<usize> {
    loop {
        let l = *p.get(i)? as usize;
        if l == 0 {
            return Some(i + 1);
        }
        if l & 0xC0 == 0xC0 {
            return Some(i + 2);
        }
        i += 1 + l;
    }
}

// [@ANCHOR: ham_proxy_parse_aaaa]
/// The AAAA addresses in a DNS response; empty on an error response or one that does not parse.
pub fn parse_aaaa(p: &[u8]) -> Vec<Ipv6Addr> {
    let mut out = Vec::new();
    let u16at = |i: usize| {
        p.get(i..i + 2)
            .map(|b| u16::from_be_bytes([b[0], b[1]]) as usize)
    };
    let (Some(qd), Some(an)) = (u16at(4), u16at(6)) else {
        return out;
    };
    if p.len() < 12 || p[3] & 0x0F != 0 {
        return out;
    }
    let mut i = 12;
    for _ in 0..qd {
        let Some(j) = skip_name(p, i) else { return out };
        i = j + 4;
    }
    for _ in 0..an {
        let Some(j) = skip_name(p, i) else { return out };
        let (Some(rtype), Some(rdlen)) = (u16at(j), u16at(j + 8)) else {
            return out;
        };
        let Some(rd) = p.get(j + 10..j + 10 + rdlen) else {
            return out;
        };
        if rtype == 28 {
            if let Ok(b) = <[u8; 16]>::try_from(rd) {
                out.push(Ipv6Addr::from(b));
            }
        }
        i = j + 10 + rdlen;
    }
    out
}

// [@ANCHOR: ham_proxy_address_allowed]
/// Whether the gateway may connect to `ip`: not loopback, unspecified, private, link-local, unique-local, multicast or an IPv4-mapped address.
pub fn address_allowed(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(a) => {
            !(a.is_loopback()
                || a.is_unspecified()
                || a.is_private()
                || a.is_link_local()
                || a.is_multicast()
                || a.is_broadcast()
                || a.octets()[0] == 0
                || (a.octets()[0] == 100 && a.octets()[1] & 0xC0 == 64))
        }
        IpAddr::V6(a) => {
            let s = a.segments();
            !(a.is_loopback()
                || a.is_unspecified()
                || a.is_multicast()
                || s[0] & 0xFE00 == 0xFC00
                || s[0] & 0xFFC0 == 0xFE80
                || a.to_ipv4_mapped().is_some())
        }
    }
}

// [@ANCHOR: ham_proxy_resolve_lookup]
fn lookup(resolver: &str, host: &str) -> Result<Vec<Ipv6Addr>, ProxyError> {
    let sock = UdpSocket::bind(if resolver.starts_with('[') {
        "[::]:0"
    } else {
        "0.0.0.0:0"
    })?;
    sock.set_read_timeout(Some(Duration::from_secs(3)))?;
    sock.connect(resolver)?;
    sock.send(&aaaa_query(host, 0x4841))?;
    let mut buf = [0u8; 4096];
    let n = sock.recv(&mut buf)?;
    Ok(parse_aaaa(&buf[..n]))
}

// [@ANCHOR: ham_proxy_connect_upstream]
/// Open the TCP connection to `addr` (`host:port`): a `.ham` host through the `--ham-resolver` when one is set, anything else
/// through the system resolver; only [`address_allowed`] addresses are tried unless [`allow_private_upstream`] was called.
pub fn connect_upstream(addr: &str) -> Result<TcpStream, ProxyError> {
    let (host, port) = addr.rsplit_once(':').unwrap_or((addr, "443"));
    let port: u16 = port.parse().map_err(|_| ProxyError::BadRequest)?;
    let ips: Vec<IpAddr> = match HAM_RESOLVER.get().filter(|_| host.ends_with(".ham")) {
        Some(r) => lookup(r, host)?.into_iter().map(IpAddr::V6).collect(),
        None => addr.to_socket_addrs()?.map(|s| s.ip()).collect(),
    };
    let mut last = std::io::Error::other("no permitted address");
    for ip in ips
        .iter()
        .filter(|i| ALLOW_PRIVATE.load(Ordering::Relaxed) || address_allowed(i))
    {
        match TcpStream::connect_timeout(&SocketAddr::new(*ip, port), Duration::from_secs(10)) {
            Ok(s) => {
                crate::limit_idle(&s)?;
                return Ok(s);
            }
            Err(e) => last = e,
        }
    }
    Err(ProxyError::Io(last))
}
