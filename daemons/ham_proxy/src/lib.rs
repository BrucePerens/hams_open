//! Ham-side proxy prototype. The ham side speaks integrity-only TLS (RFC 9150 `TLS_SHA256_SHA256` or TLS 1.2
//! `NULL-SHA256`): authenticated and integrity-protected, not encrypted. The site side is ordinary verified TLS.
//! Half-duplex request/response relay: one request head (no body) is read from the ham, sent to the site, and the
//! response is streamed back until the site closes. A prototype for measuring, not a finished proxy.

pub mod indicator;

use openssl::error::ErrorStack;
use openssl::pkey::{PKey, Private};
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
/// Serve one ham connection without signing (see [`serve_one_signed`]).
pub fn serve_one(
    tcp: TcpStream,
    acceptor: &SslAcceptor,
    site: &SslConnector,
    site_addr: &str,
    site_name: &str,
) -> Result<(), ProxyError> {
    serve_one_signed(tcp, acceptor, site, site_addr, site_name, None)
}

const MAX_HTML: usize = 8 * 1024 * 1024;

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

// [@ANCHOR: ham_proxy_request_identity]
/// Ask the site for an unencoded body so HTML can be inspected.
fn request_identity(head: &[u8]) -> Vec<u8> {
    let text = String::from_utf8_lossy(head);
    let mut lines: Vec<&str> = text
        .trim_end_matches("\r\n")
        .split("\r\n")
        .filter(|l| !l.to_ascii_lowercase().starts_with("accept-encoding:"))
        .collect();
    lines.push("Accept-Encoding: identity");
    let mut out = lines.join("\r\n").into_bytes();
    out.extend_from_slice(b"\r\n\r\n");
    out
}

// [@ANCHOR: ham_proxy_http_response]
fn http_response(status: &str, extra: &[(&str, String)], body: &[u8]) -> Vec<u8> {
    let mut h = format!("HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\n");
    for (k, v) in extra {
        h.push_str(&format!("{k}: {v}\r\n"));
    }
    h.push_str(&format!(
        "Content-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    ));
    let mut out = h.into_bytes();
    out.extend_from_slice(body);
    out
}

// [@ANCHOR: ham_proxy_transform_html]
/// Turn a buffered HTML response into what the ham gets: banner exactly once, signed header, and a block page
/// if a password field would be sent in clear. `ts` is the unix time to sign.
pub fn transform_html(
    head: &str,
    body: &[u8],
    host: &str,
    key: Option<&PKey<Private>>,
    ts: u64,
) -> Vec<u8> {
    let bad = || http_response("502 Bad Gateway", &[], indicator::BLOCK_PAGE.as_bytes());
    let chunked = indicator::header(head, "transfer-encoding")
        .is_some_and(|v| v.to_ascii_lowercase().contains("chunked"));
    let body = if chunked {
        match indicator::dechunk(body) {
            Some(b) => b,
            None => return bad(),
        }
    } else {
        body.to_vec()
    };
    if indicator::header(head, "content-encoding")
        .is_some_and(|v| !v.eq_ignore_ascii_case("identity"))
    {
        return bad();
    }
    let (status, page) = if indicator::has_unhandled_password(&body) {
        (
            "403 Forbidden".to_string(),
            indicator::BLOCK_PAGE.as_bytes().to_vec(),
        )
    } else {
        let line = head.lines().next().unwrap_or("HTTP/1.1 200 OK");
        (
            line.split_once(' ').map_or("200 OK", |x| x.1).to_string(),
            indicator::inject(&body),
        )
    };
    let mut extra = Vec::new();
    if let Some(k) = key {
        if let Ok(v) = indicator::sign_header(k, host, ts, &page) {
            extra.push((indicator::HEADER, v));
        }
    }
    http_response(&status, &extra, &page)
}

fn is_html(head: &str) -> bool {
    indicator::header(head, "content-type").is_some_and(|v| {
        let v = v.to_ascii_lowercase();
        v.contains("text/html") || v.contains("application/xhtml+xml")
    })
}

// [@ANCHOR: ham_proxy_serve_one_signed]
/// Serve one ham connection: handshake (integrity-only), read the request head, relay it to `site_addr` over
/// verified TLS for `site_name`, and send the response back. HTML gets the indicator (and a signed header when
/// `key` is set) or a block page; every other response is streamed byte for byte.
pub fn serve_one_signed(
    tcp: TcpStream,
    acceptor: &SslAcceptor,
    site: &SslConnector,
    site_addr: &str,
    site_name: &str,
    key: Option<&PKey<Private>>,
) -> Result<(), ProxyError> {
    let mut ham = acceptor.accept(tcp)?;
    require_integrity_only(&ham)?;
    let head = read_head(&mut ham)?;
    let up_tcp = TcpStream::connect(site_addr)?;
    let mut up = site.connect(site_name, up_tcp)?;
    up.write_all(&request_identity(&head))?;
    let mut buf = Vec::new();
    let mut tmp = [0u8; 4096];
    let end = loop {
        if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break Some(i + 4);
        }
        let n = up.read(&mut tmp)?;
        if n == 0 || buf.len() > MAX_HEAD {
            break None;
        }
        buf.extend_from_slice(&tmp[..n]);
    };
    let head_txt = end.map(|e| String::from_utf8_lossy(&buf[..e - 4]).to_string());
    let Some((end, head_txt)) = end.zip(head_txt).filter(|(_, h)| is_html(h)) else {
        ham.write_all(&buf)?;
        io::copy(&mut up, &mut ham)?;
        ham.shutdown().ok();
        return Ok(());
    };
    let mut body = buf[end..].to_vec();
    (&mut up)
        .take((MAX_HTML + 1) as u64)
        .read_to_end(&mut body)?;
    let out = if body.len() > MAX_HTML {
        http_response("502 Bad Gateway", &[], indicator::BLOCK_PAGE.as_bytes())
    } else {
        transform_html(&head_txt, &body, site_name, key, now())
    };
    ham.write_all(&out)?;
    ham.shutdown().ok();
    Ok(())
}
