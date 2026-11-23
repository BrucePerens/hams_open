//! DANE-EE for `.ham` servers: the proxy resolves the TLSA records of `_PORT._tcp.HOST` from the resolver it is
//! given (the node's validating resolver for the signed `.ham` zone) and accepts the server only if a record
//! pins its certificate.

use crate::{ham_client_builder, require_integrity_only, ProxyError};
use openssl::hash::{hash, MessageDigest};
use openssl::ssl::{SslStream, SslVerifyMode};
use std::net::{TcpStream, UdpSocket};
use std::time::Duration;

/// One TLSA record: usage, selector, matching type, data.
pub type Tlsa = (u8, u8, u8, Vec<u8>);
const TYPE_TLSA: u16 = 52;
const DANE_EE: u8 = 3;
const FULL_CERT: u8 = 0;

// [@ANCHOR: ham_proxy_tlsa_matches]
/// True if a DANE-EE (3), full-certificate (0) record with matching type 0, 1 or 2 pins `cert_der`.
/// No records, or only other kinds, means refuse.
pub fn tlsa_matches(cert_der: &[u8], records: &[Tlsa]) -> bool {
    records.iter().any(|(usage, selector, matching, data)| {
        if *usage != DANE_EE || *selector != FULL_CERT {
            return false;
        }
        match *matching {
            0 => cert_der == data.as_slice(),
            1 => hash(MessageDigest::sha256(), cert_der).is_ok_and(|d| *d == **data),
            2 => hash(MessageDigest::sha512(), cert_der).is_ok_and(|d| *d == **data),
            _ => false,
        }
    })
}

// [@ANCHOR: ham_proxy_tlsa_query]
/// A DNS query packet for the TLSA records of `_port._tcp.host`.
pub fn tlsa_query(host: &str, port: u16, id: u16) -> Vec<u8> {
    let mut q = id.to_be_bytes().to_vec();
    q.extend_from_slice(&[1, 0, 0, 1, 0, 0, 0, 0, 0, 0]); // RD, one question
    let name = format!("_{port}._tcp.{}", host.trim_end_matches('.'));
    for label in name.split('.') {
        q.push(label.len().min(63) as u8);
        q.extend_from_slice(&label.as_bytes()[..label.len().min(63)]);
    }
    q.push(0);
    q.extend_from_slice(&TYPE_TLSA.to_be_bytes());
    q.extend_from_slice(&[0, 1]); // class IN
    q
}

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

// [@ANCHOR: ham_proxy_parse_tlsa]
/// The TLSA records in a DNS response; empty on an error response or one that does not parse.
pub fn parse_tlsa(p: &[u8]) -> Vec<Tlsa> {
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
        if rtype == TYPE_TLSA as usize && rd.len() >= 3 {
            out.push((rd[0], rd[1], rd[2], rd[3..].to_vec()));
        }
        i = j + 10 + rdlen;
    }
    out
}

// [@ANCHOR: ham_proxy_resolve_tlsa]
/// Ask `resolver` (`ip:port`, UDP) for the TLSA records of `_port._tcp.host`; empty if none or no answer.
pub fn resolve_tlsa(resolver: &str, host: &str, port: u16) -> Result<Vec<Tlsa>, ProxyError> {
    let sock = UdpSocket::bind("0.0.0.0:0")?;
    sock.set_read_timeout(Some(Duration::from_secs(3)))?;
    sock.connect(resolver)?;
    sock.send(&tlsa_query(host, port, 0x4841))?;
    let mut buf = [0u8; 4096];
    let n = sock.recv(&mut buf)?;
    Ok(parse_tlsa(&buf[..n]))
}

// [@ANCHOR: ham_proxy_connect_ham_server]
/// Connect to a `.ham` server with integrity-only TLS and accept it only if its certificate is pinned by a
/// TLSA record from `resolver`. Missing records or a different certificate refuse the server.
pub fn connect_ham_server(
    tcp: TcpStream,
    host: &str,
    port: u16,
    resolver: &str,
) -> Result<SslStream<TcpStream>, ProxyError> {
    let mut b = ham_client_builder()?;
    b.set_verify(SslVerifyMode::NONE); // identity comes from DANE, not from a CA
    let mut cfg = b.build().configure()?;
    cfg.set_verify_hostname(false);
    let s = cfg.connect(host, tcp)?;
    require_integrity_only(&s)?;
    let der = s
        .ssl()
        .peer_certificate()
        .ok_or_else(|| ProxyError::Tls("no server certificate".into()))?
        .to_der()?;
    if tlsa_matches(&der, &resolve_tlsa(resolver, host, port)?) {
        Ok(s)
    } else {
        Err(ProxyError::NotPinned(host.to_string()))
    }
}
