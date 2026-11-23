//! Ham-side proxy prototype. The ham side speaks integrity-only TLS (RFC 9150 `TLS_SHA256_SHA256` or TLS 1.2
//! `NULL-SHA256`): authenticated and integrity-protected, not encrypted. The site side is ordinary verified TLS.
//! Half-duplex request/response relay: one request head (no body) is read from the ham, sent to the site, and the
//! response is streamed back until the site closes. A prototype for measuring, not a finished proxy.

use openssl::error::ErrorStack;
use openssl::ssl::{
    SslAcceptor, SslConnector, SslContextBuilder, SslFiletype, SslMethod, SslStream, SslVerifyMode,
    SslVersion,
};
use std::io::{self, Read, Write};
use std::net::TcpStream;
use std::path::Path;

/// Suites a ham-side link may negotiate. Anything else is refused after the handshake.
pub const INTEGRITY_ONLY_SUITES: [&str; 3] =
    ["TLS_SHA256_SHA256", "TLS_SHA384_SHA384", "NULL-SHA256"];
const TLS13_SUITES: &str = "TLS_SHA256_SHA256:TLS_SHA384_SHA384";
const TLS12_CIPHERS: &str = "NULL-SHA256:@SECLEVEL=0";
const MAX_HEAD: usize = 65536;

/// Error from setting up or running the proxy.
#[derive(Debug)]
pub enum ProxyError {
    /// OpenSSL refused a setting, file or handshake.
    Tls(String),
    /// Socket error.
    Io(io::Error),
    /// The negotiated suite encrypts (or is unknown), so the ham link is refused.
    EncryptedLink(String),
    /// The request head was too long or ended early.
    BadRequest,
}

impl std::fmt::Display for ProxyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for ProxyError {}

impl From<ErrorStack> for ProxyError {
    fn from(e: ErrorStack) -> Self {
        ProxyError::Tls(e.to_string())
    }
}
impl From<io::Error> for ProxyError {
    fn from(e: io::Error) -> Self {
        ProxyError::Io(e)
    }
}
impl<S: std::fmt::Debug> From<openssl::ssl::HandshakeError<S>> for ProxyError {
    fn from(e: openssl::ssl::HandshakeError<S>) -> Self {
        ProxyError::Tls(format!("{e:?}"))
    }
}

// [@ANCHOR: ham_proxy_null_only]
fn null_only(b: &mut SslContextBuilder) -> Result<(), ErrorStack> {
    b.set_min_proto_version(Some(SslVersion::TLS1_2))?;
    b.set_cipher_list(TLS12_CIPHERS)?;
    b.set_ciphersuites(TLS13_SUITES)
}

// [@ANCHOR: ham_proxy_ham_acceptor]
/// Ham-side acceptor: offers only integrity-only suites.
pub fn ham_acceptor(cert: &Path, key: &Path) -> Result<SslAcceptor, ProxyError> {
    let mut b = SslAcceptor::mozilla_intermediate_v5(SslMethod::tls_server())?;
    null_only(&mut b)?;
    b.set_certificate_chain_file(cert)?;
    b.set_private_key_file(key, SslFiletype::PEM)?;
    b.check_private_key()?;
    Ok(b.build())
}

// [@ANCHOR: ham_proxy_ham_connector]
/// Client-side connector that offers only integrity-only suites (used by tests and the ham client).
pub fn ham_connector(ca: &Path) -> Result<SslConnector, ProxyError> {
    let mut b = SslConnector::builder(SslMethod::tls_client())?;
    null_only(&mut b)?;
    b.set_ca_file(ca)?;
    Ok(b.build())
}

// [@ANCHOR: ham_proxy_site_connector]
/// Site-side connector: ordinary verified TLS 1.2/1.3 with the default encrypting suites.
pub fn site_connector(ca: Option<&Path>) -> Result<SslConnector, ProxyError> {
    let mut b = SslConnector::builder(SslMethod::tls_client())?;
    if let Some(p) = ca {
        b.set_ca_file(p)?;
    }
    b.set_verify(SslVerifyMode::PEER);
    Ok(b.build())
}

// [@ANCHOR: ham_proxy_require_integrity_only]
/// Refuse a link whose negotiated suite encrypts.
pub fn require_integrity_only<S>(s: &SslStream<S>) -> Result<String, ProxyError> {
    let name = s
        .ssl()
        .current_cipher()
        .map(|c| c.name().to_string())
        .unwrap_or_default();
    if INTEGRITY_ONLY_SUITES.contains(&name.as_str()) {
        Ok(name)
    } else {
        Err(ProxyError::EncryptedLink(name))
    }
}

// [@ANCHOR: ham_proxy_read_head]
fn read_head<R: Read>(r: &mut R) -> Result<Vec<u8>, ProxyError> {
    let mut head = Vec::new();
    let mut buf = [0u8; 4096];
    while !head.windows(4).any(|w| w == b"\r\n\r\n") {
        let n = r.read(&mut buf)?;
        if n == 0 || head.len() + n > MAX_HEAD {
            return Err(ProxyError::BadRequest);
        }
        head.extend_from_slice(&buf[..n]);
    }
    Ok(head)
}

// [@ANCHOR: ham_proxy_serve_one]
/// Serve one ham connection: handshake (integrity-only), read the request head, relay it to `site_addr` over
/// verified TLS for `site_name`, and stream the response back.
pub fn serve_one(
    tcp: TcpStream,
    acceptor: &SslAcceptor,
    site: &SslConnector,
    site_addr: &str,
    site_name: &str,
) -> Result<(), ProxyError> {
    let mut ham = acceptor.accept(tcp)?;
    require_integrity_only(&ham)?;
    let head = read_head(&mut ham)?;
    let up_tcp = TcpStream::connect(site_addr)?;
    let mut up = site.connect(site_name, up_tcp)?;
    up.write_all(&head)?;
    io::copy(&mut up, &mut ham)?;
    ham.shutdown().ok();
    Ok(())
}
