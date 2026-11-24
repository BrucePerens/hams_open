//! LAN-facing side of the proxy, for the browsers on the member's own LAN (never RF). A browser asks for a `.ham` or whitelisted site in
//! one of two ways: `CONNECT host:port` (the proxy answers 200 and terminates the browser's TLS with a leaf certificate minted on the
//! spot from the node's proxy CA, whose name constraints keep it to `.ham` and the whitelist), or plain `http://` (accepted and upgraded:
//! the request goes upstream over TLS). Upstream the request always leaves as integrity-only TLS (authenticated, never encrypted):
//! a `.ham` server is reached directly and verified by DANE; a whitelisted Internet site goes through the site gateway named by the
//! operator, which verifies the real site. Nothing here ever offers an encrypting suite on the ham side.

use crate::dane::connect_ham_server;
use crate::policy::Policy;
use crate::resolve;
use crate::{
    ham_connector, now, read_head, relay_response, request_identity, require_integrity_only,
    ProxyError,
};
use openssl::asn1::Asn1Time;
use openssl::bn::{BigNum, MsbOption};
use openssl::ec::{EcGroup, EcKey};
use openssl::hash::MessageDigest;
use openssl::nid::Nid;
use openssl::pkey::{PKey, Private};
use openssl::ssl::{SslAcceptor, SslConnector, SslMethod, SslStream};
use openssl::x509::extension::{BasicConstraints, KeyUsage, SubjectAlternativeName};
use openssl::x509::{X509NameBuilder, X509};
use std::io::Write;
use std::net::TcpStream;
use std::path::Path;

// [@ANCHOR: ham_proxy_lan_ca]
/// The node's proxy CA: certificate and private key (the key never leaves the router; see `hams-node-proxyca`).
pub struct Ca {
    cert: X509,
    key: PKey<Private>,
}

impl Ca {
    /// Load the CA from PEM files.
    pub fn load(cert: &Path, key: &Path) -> Result<Self, ProxyError> {
        Ok(Ca {
            cert: X509::from_pem(&std::fs::read(cert)?)?,
            key: PKey::private_key_from_pem(&std::fs::read(key)?)?,
        })
    }

    // [@ANCHOR: ham_proxy_lan_leaf]
    /// A leaf certificate and key for `host`, signed by this CA, valid from an hour ago for a day.
    pub fn leaf(&self, host: &str) -> Result<(X509, PKey<Private>), ProxyError> {
        let group = EcGroup::from_curve_name(Nid::X9_62_PRIME256V1)?;
        let key = PKey::from_ec_key(EcKey::generate(&group)?)?;
        let mut name = X509NameBuilder::new()?;
        name.append_entry_by_nid(Nid::COMMONNAME, host)?;
        let name = name.build();
        let mut b = X509::builder()?;
        b.set_version(2)?;
        let mut serial = BigNum::new()?;
        serial.rand(127, MsbOption::MAYBE_ZERO, false)?;
        let serial = serial.to_asn1_integer()?;
        b.set_serial_number(&serial)?;
        b.set_subject_name(&name)?;
        b.set_issuer_name(self.cert.subject_name())?;
        b.set_pubkey(&key)?;
        let t = i64::try_from(now()).unwrap_or(0);
        let (from, to) = (
            Asn1Time::from_unix(t - 3600)?,
            Asn1Time::from_unix(t + 86400)?,
        );
        b.set_not_before(&from)?;
        b.set_not_after(&to)?;
        b.append_extension(BasicConstraints::new().build()?)?;
        b.append_extension(KeyUsage::new().digital_signature().build()?)?;
        let san = SubjectAlternativeName::new()
            .dns(host)
            .build(&b.x509v3_context(Some(&self.cert), None))?;
        b.append_extension(san)?;
        b.sign(&self.key, MessageDigest::sha256())?;
        Ok((b.build(), key))
    }
}

// [@ANCHOR: ham_proxy_lan_upstream]
/// Where a request goes next.
pub enum Upstream {
    /// A whitelisted Internet site: integrity-only TLS to the site gateway at `addr` (named `name` in its certificate, verified with `connector`).
    Gateway {
        /// `host:port` of the site gateway.
        addr: String,
        /// Name in the gateway's certificate.
        name: String,
        /// Integrity-only client that trusts the gateway's CA.
        connector: SslConnector,
    },
}

impl Upstream {
    /// A gateway upstream trusting the CA file `ca`.
    pub fn gateway(addr: &str, ca: &Path) -> Result<Self, ProxyError> {
        let name = addr.rsplit_once(':').map_or(addr, |(h, _)| h).to_string();
        Ok(Upstream::Gateway {
            addr: addr.to_string(),
            name,
            connector: ham_connector(ca)?,
        })
    }
}

// [@ANCHOR: ham_proxy_lan_target]
/// What a browser asked for.
#[derive(Debug, PartialEq, Eq)]
pub struct Target {
    /// `CONNECT` (TLS follows) rather than plain HTTP.
    pub connect: bool,
    /// Lower-case host name.
    pub host: String,
    /// Upstream port.
    pub port: u16,
    /// For plain HTTP the origin-form path; empty for `CONNECT`.
    pub path: String,
}

fn dns_name(h: &str) -> bool {
    !h.is_empty()
        && h.len() < 254
        && h.bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'.' || c == b'-')
}

// [@ANCHOR: ham_proxy_lan_parse_target]
/// Read the target of a browser's request head: `CONNECT host:port`, `GET http://host[:port]/path`, or origin-form with a `Host` header.
/// Plain `http://` (port 80 or none) is upgraded to port 443; only DNS names are accepted.
pub fn parse_target(head: &str) -> Option<Target> {
    let line = head.lines().next()?;
    let mut parts = line.split(' ');
    let (method, target) = (parts.next()?, parts.next()?);
    let split = |auth: &str, default: u16| -> Option<(String, u16)> {
        let auth = auth.to_lowercase();
        let (h, p) = match auth.rsplit_once(':') {
            Some((h, p)) => (h, p.parse::<u16>().ok()?),
            None => (auth.as_str(), default),
        };
        let h = h.trim_end_matches('.');
        dns_name(h).then(|| (h.to_string(), if p == 80 { 443 } else { p }))
    };
    if method.eq_ignore_ascii_case("CONNECT") {
        let (host, port) = split(target, 443)?;
        return Some(Target {
            connect: true,
            host,
            port,
            path: String::new(),
        });
    }
    let (auth, path) = if let Some(rest) = target.strip_prefix("http://") {
        let (a, p) = rest.split_once('/').unwrap_or((rest, ""));
        (a.to_string(), format!("/{p}"))
    } else if target.starts_with('/') {
        (
            crate::indicator::header(head, "host")?.trim().to_string(),
            target.to_string(),
        )
    } else {
        return None;
    };
    let (host, port) = split(&auth, 443)?;
    Some(Target {
        connect: false,
        host,
        port,
        path,
    })
}

// [@ANCHOR: ham_proxy_lan_upstream_head]
/// The request head sent upstream, in origin form: proxy and connection headers and every client `X-Ham-*` header dropped,
/// `Host` set to the real site, `Connection: close` added.
pub fn upstream_head(head: &str, t: &Target, path: &str) -> Vec<u8> {
    let first = head.lines().next().unwrap_or("");
    let method = first.split(' ').next().unwrap_or("GET");
    let host = if t.port == 443 {
        t.host.clone()
    } else {
        format!("{}:{}", t.host, t.port)
    };
    let mut lines = vec![format!("{method} {path} HTTP/1.1"), format!("Host: {host}")];
    for l in head.split("\r\n").skip(1) {
        let k = l.to_ascii_lowercase();
        if [
            "host:",
            "connection:",
            "proxy-",
            "x-ham-",
            "accept-encoding:",
        ]
        .iter()
        .any(|p| k.starts_with(p))
        {
            continue;
        }
        if !l.is_empty() {
            lines.push(l.to_string());
        }
    }
    lines.push("Connection: close".to_string());
    let mut out = lines.join("\r\n").into_bytes();
    out.extend_from_slice(b"\r\n\r\n");
    request_identity(&out)
}

// [@ANCHOR: ham_proxy_lan_refuse]
/// Answer a browser with `status` and a short plain-text body.
fn refuse<W: Write>(w: &mut W, status: &str) -> Result<(), ProxyError> {
    let body = b"hams.com Net: this site is not available through the ham proxy\n";
    write!(
        w,
        "HTTP/1.1 {status}\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    )?;
    w.write_all(body)?;
    Ok(())
}

fn connect_upstream(
    t: &Target,
    up: &Upstream,
    resolver: &str,
) -> Result<SslStream<TcpStream>, ProxyError> {
    if t.host == "ham" || t.host.ends_with(".ham") {
        let tcp = resolve::connect_upstream(&format!("{}:{}", t.host, t.port))?;
        return connect_ham_server(tcp, &t.host, t.port, resolver);
    }
    let Upstream::Gateway {
        addr,
        name,
        connector,
    } = up;
    let s = connector.connect(name, resolve::connect_upstream(addr)?)?;
    require_integrity_only(&s)?;
    Ok(s)
}

// [@ANCHOR: ham_proxy_lan_serve]
/// Serve one browser connection on the LAN: policy first (a name off the whitelist is refused with 403 before any upstream
/// connection), then the station-ID log line, then the request goes upstream as integrity-only TLS and the response comes back
/// through the same indicator handling as the ham-side proxy. `resolver` is the node's `.ham` resolver (`ip:port`) for DANE.
pub fn serve_lan(
    mut tcp: TcpStream,
    ca: &Ca,
    policy: &Policy,
    up: &Upstream,
    resolver: &str,
    key: Option<&PKey<Private>>,
) -> Result<(), ProxyError> {
    let head = String::from_utf8_lossy(&read_head(&mut tcp)?).to_string();
    let Some(t) = parse_target(&head) else {
        return refuse(&mut tcp, "400 Bad Request");
    };
    if !policy.permits(&t.host, "/") {
        return refuse(&mut tcp, "403 Forbidden");
    }
    if !t.connect {
        let mut upstream = connect_upstream(&t, up, resolver)?;
        policy.record(now(), &t.host, &require_integrity_only(&upstream)?)?;
        upstream.write_all(&upstream_head(&head, &t, &t.path))?;
        relay_response(&mut upstream, &mut tcp, &t.host, key)?;
        return Ok(());
    }
    tcp.write_all(b"HTTP/1.1 200 Connection established\r\n\r\n")?;
    let (cert, leaf_key) = ca.leaf(&t.host)?;
    let mut a = SslAcceptor::mozilla_intermediate_v5(SslMethod::tls_server())?;
    a.set_certificate(&cert)?;
    a.set_private_key(&leaf_key)?;
    let mut browser = a.build().accept(tcp)?;
    let inner = String::from_utf8_lossy(&read_head(&mut browser)?).to_string();
    let path = inner
        .lines()
        .next()
        .and_then(|l| l.split(' ').nth(1))
        .unwrap_or("/")
        .to_string();
    let mut upstream = connect_upstream(&t, up, resolver)?;
    policy.record(now(), &t.host, &require_integrity_only(&upstream)?)?;
    upstream.write_all(&upstream_head(&inner, &t, &path))?;
    relay_response(&mut upstream, &mut browser, &t.host, key)?;
    browser.shutdown().ok();
    Ok(())
}
