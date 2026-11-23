//! `/.ham/login`: sign in over the integrity-only ham link without sending a password or any reusable secret.
//!
//! Standard used: a signed challenge, the proof of possession of a client-held ECDSA P-256 key that WebAuthn
//! and TLS client certificates also use. No custom primitive: signing is done by the member's browser or tool,
//! verifying by hams.com (`ham_member_certs`, the member certificate's recorded key) over ordinary verified TLS.
//! The core only mints and spends the challenges. Nothing here derives or uses an encryption key.
//!
//! Wire (all plain ASCII, so it fits the head-only relay):
//! * `GET /.ham/login` answers `{"nonce":HEX,"audience":"ham-login:SITE","expires_in":120}`.
//! * `GET /.ham/login?serial=HEX&nonce=HEX&sig=HEX` where `sig` is the raw r||s ECDSA signature over
//!   `hams-member-cert-possession-v1\n<audience>\n<nonce>`. Answers 200 `{"callsign":...}` or 403.
//!
//! With an `epk=HEX` parameter (the browser's ephemeral P-256 public point) the audience becomes
//! `ham-login:SITE:epk=HEX`, so the signature also covers `epk`, and the 200 answer adds `"session"` and the
//! proxy's `"epk"`: the consumer's [`SessionOpener`] then agrees the session key.
//!
//! A nonce is single use and lives [`TTL_SECS`] seconds, so a captured exchange cannot be replayed, and the
//! audience names the site, so it cannot be used for another one.

use crate::crypto::Crypto;
use std::collections::HashMap;
use std::sync::Mutex;

/// Seconds a challenge may be answered in.
pub const TTL_SECS: u64 = 120;
/// Path prefix the proxy answers itself.
pub const PATH: &str = "/.ham/login";
const MAX_PENDING: usize = 1024;

/// Why [`Challenges::issue`] could not hand out a nonce.
#[derive(Debug, PartialEq, Eq)]
pub enum IssueError {
    /// The system random generator failed.
    Random,
    /// Too many unanswered challenges (or the table is unusable).
    Full,
}

// [@ANCHOR: hams_net_core_login_challenges]
/// Outstanding challenges: nonce -> issue time.
#[derive(Default)]
pub struct Challenges {
    pending: Mutex<HashMap<String, u64>>,
}

impl Challenges {
    // [@ANCHOR: hams_net_core_login_issue]
    /// A fresh 128-bit nonce, remembered until answered or [`TTL_SECS`] old.
    pub fn issue(&self, crypto: &dyn Crypto, now: u64) -> Result<String, IssueError> {
        let b = crypto.random16().ok_or(IssueError::Random)?;
        let nonce: String = b.iter().map(|x| format!("{x:02x}")).collect();
        let mut m = self.pending.lock().map_err(|_| IssueError::Full)?;
        m.retain(|_, t| now.saturating_sub(*t) <= TTL_SECS);
        if m.len() >= MAX_PENDING {
            return Err(IssueError::Full);
        }
        m.insert(nonce.clone(), now);
        Ok(nonce)
    }

    // [@ANCHOR: hams_net_core_login_take]
    /// Spend `nonce`: true only the first time, and only while it is unexpired.
    pub fn take(&self, nonce: &str, now: u64) -> bool {
        let Ok(mut m) = self.pending.lock() else {
            return false;
        };
        m.remove(nonce)
            .is_some_and(|t| now.saturating_sub(t) <= TTL_SECS)
    }
}

/// Decides whether a signature over the login message is genuine; answers the member's callsign.
pub trait Verifier {
    /// `sig_b64` is the standard base64 of the raw signature.
    fn verify(&self, serial: &str, nonce: &str, audience: &str, sig_b64: &str) -> Option<String>;
}

/// Completes the key agreement of a login that sent an `epk`.
pub trait SessionOpener {
    /// Returns `(session id, our public point hex)`.
    fn open(
        &self,
        callsign: &str,
        peer_hex: &str,
        nonce: &str,
        now: u64,
    ) -> Result<(String, String), SessionRefused>;
}

/// The session could not be opened.
#[derive(Debug, PartialEq, Eq)]
pub struct SessionRefused;

// [@ANCHOR: hams_net_core_login_audience]
/// What the member's key signs for `site`.
pub fn audience(site: &str) -> String {
    format!("ham-login:{site}")
}

fn is_hex(s: &str) -> bool {
    !s.is_empty() && s.len() <= 1024 && s.bytes().all(|c| c.is_ascii_hexdigit())
}

fn json(status: &str, body: &str) -> Vec<u8> {
    format!(
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nCache-Control: no-store\r\n\
Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
    .into_bytes()
}

fn param<'a>(query: &'a str, name: &str) -> Option<&'a str> {
    query
        .split('&')
        .find_map(|kv| kv.split_once('=').filter(|(k, _)| *k == name).map(|x| x.1))
}

// [@ANCHOR: hams_net_core_login_respond]
/// The answer to a request for `target` (the request-line path and query) if it is a login request, else `None`
/// (relay it to the site as usual).
pub fn respond(
    target: &str,
    site: &str,
    ch: &Challenges,
    v: &dyn Verifier,
    sessions: &dyn SessionOpener,
    crypto: &dyn Crypto,
    now: u64,
) -> Option<Vec<u8>> {
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    if path != PATH {
        return None;
    }
    let mut aud = audience(site);
    if query.is_empty() {
        return Some(match ch.issue(crypto, now) {
            Ok(n) => json(
                "200 OK",
                &format!("{{\"nonce\":\"{n}\",\"audience\":\"{aud}\",\"expires_in\":{TTL_SECS}}}"),
            ),
            Err(_) => json("503 Service Unavailable", "{}"),
        });
    }
    let refuse = || json("403 Forbidden", "{\"error\":\"login refused\"}");
    let (Some(serial), Some(nonce), Some(sig)) = (
        param(query, "serial"),
        param(query, "nonce"),
        param(query, "sig"),
    ) else {
        return Some(refuse());
    };
    if ![serial, nonce, sig].iter().all(|x| is_hex(x)) || sig.len() % 2 != 0 {
        return Some(refuse());
    }
    // Spend the challenge first: a wrong signature also burns it, so a guess costs a new round trip.
    if !ch.take(nonce, now) {
        return Some(refuse());
    }
    let epk = param(query, "epk");
    if let Some(e) = epk {
        if !is_hex(e) {
            return Some(refuse());
        }
        aud = format!("{aud}:epk={e}");
    }
    let raw: Vec<u8> = (0..sig.len())
        .step_by(2)
        .filter_map(|i| u8::from_str_radix(&sig[i..i + 2], 16).ok())
        .collect();
    Some(
        match v.verify(serial, nonce, &aud, &crypto.base64_encode(&raw)) {
            Some(call) if call.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'/') => match epk
            {
                None => json("200 OK", &format!("{{\"callsign\":\"{call}\"}}")),
                Some(e) => match sessions.open(&call, e, nonce, now) {
                    Ok((sid, ours)) => json(
                        "200 OK",
                        &format!(
                            "{{\"callsign\":\"{call}\",\"session\":\"{sid}\",\"epk\":\"{ours}\"}}"
                        ),
                    ),
                    Err(_) => refuse(),
                },
            },
            _ => refuse(),
        },
    )
}
