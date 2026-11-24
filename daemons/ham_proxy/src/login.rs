//! `/.ham/login`: the challenge and answer logic lives in `hams_net_core::login`; this binds it to OpenSSL, the session
//! table (see [`crate::session`]) and the HTTP verifier that asks hams.com over verified TLS.

use crate::crypto::OpensslCrypto;
use crate::session::Sessions;
use crate::ProxyError;
use hams_net_core::login::{IssueError, SessionOpener, SessionRefused};
use openssl::ssl::SslConnector;
use std::io::{Read, Write};
use std::net::TcpStream;

pub use hams_net_core::login::{audience, Verifier, PATH, TTL_SECS};

/// Outstanding challenges (the core's) plus the sessions opened by a login that sent an `epk`.
#[derive(Default)]
pub struct Challenges {
    core: hams_net_core::login::Challenges,
    /// Sessions opened by a login that sent an `epk` (see [`crate::session`]).
    pub sessions: Sessions,
}

impl Challenges {
    // [@ANCHOR: ham_proxy_login_issue]
    /// A fresh 128-bit nonce, remembered until answered or [`TTL_SECS`] old.
    pub fn issue(&self, now: u64) -> Result<String, ProxyError> {
        self.core.issue(&OpensslCrypto, now).map_err(|e| match e {
            IssueError::Random => ProxyError::Tls("random generator failed".into()),
            IssueError::Full => ProxyError::BadRequest,
        })
    }

    /// Spend `nonce`: true only the first time, and only while it is unexpired.
    pub fn take(&self, nonce: &str, now: u64) -> bool {
        self.core.take(nonce, now)
    }
}

impl SessionOpener for Sessions {
    fn open(
        &self,
        callsign: &str,
        peer_hex: &str,
        nonce: &str,
        now: u64,
    ) -> Result<(String, String), SessionRefused> {
        Sessions::open(self, callsign, peer_hex, nonce, now).map_err(|_| SessionRefused)
    }
}

/// The answer to a request for `target` (the request-line path and query) if it is a login request, else `None`
/// (relay it to the site as usual).
pub fn respond(
    target: &str,
    site: &str,
    ch: &Challenges,
    v: &dyn Verifier,
    now: u64,
) -> Option<Vec<u8>> {
    hams_net_core::login::respond(target, site, &ch.core, v, &ch.sessions, &OpensslCrypto, now)
}

// [@ANCHOR: ham_proxy_login_http_verifier]
/// Verifies by asking hams.com (`POST /ham/login/verify`, Odoo JSON-RPC) over ordinary verified TLS.
pub struct HttpVerifier {
    /// Verified-TLS client towards hams.com.
    pub connector: SslConnector,
    /// `host:port` to connect to.
    pub addr: String,
    /// Host name for the certificate check and the `Host` header.
    pub host: String,
}

/// What hams.com answered to a login check.
pub enum HttpAnswer {
    /// hams.com could not be reached or did not answer sensibly.
    Unreachable,
    /// hams.com answered and refused.
    Refused,
    /// hams.com vouched for the member; `certificate_pem` is the member's public certificate when it sent one.
    Member {
        /// The member's callsign.
        callsign: String,
        /// The public member certificate, for the offline cache.
        certificate_pem: Option<String>,
    },
}

impl HttpVerifier {
    // [@ANCHOR: ham_proxy_login_http_ask]
    /// Ask hams.com; tells an answer apart from no answer.
    pub fn ask(&self, serial: &str, nonce: &str, audience: &str, sig_b64: &str) -> HttpAnswer {
        self.try_ask(serial, nonce, audience, sig_b64)
            .unwrap_or(HttpAnswer::Unreachable)
    }

    // [@ANCHOR: ham_proxy_login_http_try_ask]
    fn try_ask(
        &self,
        serial: &str,
        nonce: &str,
        audience: &str,
        sig_b64: &str,
    ) -> Option<HttpAnswer> {
        let body = serde_json::json!({"jsonrpc": "2.0", "params": {
            "serial": serial, "nonce": nonce, "audience": audience, "signature_b64": sig_b64}})
        .to_string();
        let tcp = TcpStream::connect(&self.addr).ok()?;
        let mut s = self.connector.connect(&self.host, tcp).ok()?;
        let req = format!(
            "POST /ham/login/verify HTTP/1.1\r\nHost: {}\r\nContent-Type: application/json\r\n\
Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
            self.host,
            body.len()
        );
        s.write_all(req.as_bytes()).ok()?;
        let mut out = Vec::new();
        s.take(65536).read_to_end(&mut out).ok()?;
        Some(answer_from_reply(&String::from_utf8_lossy(&out)))
    }
}

// [@ANCHOR: ham_proxy_login_answer_from_reply]
/// Turns hams.com's raw HTTP reply into an answer. Only no reply, or a gateway saying hams.com is down (502/503/504), is
/// `Unreachable`; any other reply that is not a clean success is `Refused`, so nobody can reach the offline check by making hams.com
/// answer badly.
#[must_use]
pub fn answer_from_reply(text: &str) -> HttpAnswer {
    let status = text
        .lines()
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        .unwrap_or("");
    if matches!(status, "" | "502" | "503" | "504") {
        return HttpAnswer::Unreachable;
    }
    let parsed = text
        .split_once("\r\n\r\n")
        .and_then(|(_, body)| serde_json::from_str::<serde_json::Value>(body).ok());
    let result = parsed.as_ref().and_then(|v| v.get("result"));
    match (
        result
            .and_then(|r| r.get("status"))
            .and_then(|x| x.as_str()),
        result
            .and_then(|r| r.get("callsign"))
            .and_then(|x| x.as_str()),
    ) {
        (Some("success"), Some(callsign)) => HttpAnswer::Member {
            callsign: callsign.to_string(),
            certificate_pem: result
                .and_then(|r| r.get("certificate_pem"))
                .and_then(|c| c.as_str())
                .map(str::to_string),
        },
        _ => HttpAnswer::Refused,
    }
}

impl Verifier for HttpVerifier {
    // [@ANCHOR: ham_proxy_login_http_verify]
    fn verify(&self, serial: &str, nonce: &str, audience: &str, sig_b64: &str) -> Option<String> {
        match self.ask(serial, nonce, audience, sig_b64) {
            HttpAnswer::Member { callsign, .. } => Some(callsign),
            _ => None,
        }
    }
}
