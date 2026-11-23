//! DANE-EE for `.ham` servers: the proxy resolves the TLSA records of `_PORT._tcp.HOST` from the resolver it is
//! given (the node's validating resolver for the signed `.ham` zone) and accepts the server only if a record
//! pins its certificate.

use crate::crypto::OpensslCrypto;
use crate::{ham_client_builder, require_integrity_only, ProxyError};
use openssl::ssl::{SslStream, SslVerifyMode};
use std::net::{TcpStream, UdpSocket};
use std::time::Duration;

pub use hams_net_core::dane::{parse_tlsa, tlsa_query, Tlsa};

/// True if a DANE-EE (3), full-certificate (0) record with matching type 0, 1 or 2 pins `cert_der`
/// (see `hams_net_core::dane::tlsa_matches`; the hashes come from OpenSSL).
pub fn tlsa_matches(cert_der: &[u8], records: &[Tlsa]) -> bool {
    hams_net_core::dane::tlsa_matches(&OpensslCrypto, cert_der, records)
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
