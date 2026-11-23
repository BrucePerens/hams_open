//! Session integrity after `/.ham/login`. The ham link is signed, not encrypted, so a session cookie or token seen
//! on the air could be replayed. Instead the login agrees a per-session MAC key (ephemeral ECDH P-256 between the
//! browser and the proxy; the browser's epk is covered by its login signature) and every later request carries an
//! HMAC-SHA256 over its method, target, host, time, a nonce and the body hash. The key is used for MAC only, never
//! for encryption. The request-signature idea follows RFC 9421 (HTTP Message Signatures) but with five fixed
//! `X-Ham-*` headers instead of structured fields, to keep the head-only relay and the browser code small.
//!
//! Key: `SHA-256("ham-session-v1" 0x00 ECDH-secret nonce-ascii)`. MAC input (UTF-8, `\n` separated):
//! `ham-req-v1, METHOD, target, host, ts, nonce, body-sha256-hex`; the MAC is lowercase hex.

use crate::ProxyError;
use openssl::bn::BigNumContext;
use openssl::derive::Deriver;
use openssl::ec::{EcGroup, EcKey, EcPoint, PointConversionForm};
use openssl::hash::{hash, MessageDigest};
use openssl::memcmp;
use openssl::nid::Nid;
use openssl::pkey::{PKey, Private};
use openssl::rand::rand_bytes;
use openssl::sign::Signer;
use std::collections::HashMap;
use std::sync::Mutex;

/// Seconds a request timestamp may differ from the proxy's clock (also how long its nonce is remembered).
pub const SKEW_SECS: u64 = 60;
/// Seconds a session lives.
pub const SESSION_SECS: u64 = 8 * 3600;
const MAX_SESSIONS: usize = 4096;

/// Why a request's session proof was refused.
#[derive(Debug, PartialEq, Eq)]
pub enum Refusal {
    /// A session header is missing or malformed.
    Malformed,
    /// No such session, or it expired.
    UnknownSession,
    /// Timestamp too far from now.
    Stale,
    /// The MAC does not match.
    BadMac,
    /// This nonce was already used in this session.
    Replay,
}

struct Session {
    key: Vec<u8>,
    callsign: String,
    expires: u64,
    seen: HashMap<String, u64>,
}

/// The logged-in sessions.
#[derive(Default)]
pub struct Sessions(Mutex<HashMap<String, Session>>);

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn unhex(s: &str) -> Option<Vec<u8>> {
    if s.len() % 2 != 0 {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok())
        .collect()
}

fn group() -> Result<EcGroup, ProxyError> {
    Ok(EcGroup::from_curve_name(Nid::X9_62_PRIME256V1)?)
}

// [@ANCHOR: ham_proxy_session_derive_key]
/// The MAC key for `nonce` from our ephemeral `own` key and the peer's uncompressed public point (hex).
pub fn derive_key(
    own: &EcKey<Private>,
    peer_hex: &str,
    nonce: &str,
) -> Result<Vec<u8>, ProxyError> {
    let g = group()?;
    let bytes = unhex(peer_hex).ok_or(ProxyError::BadRequest)?;
    let mut ctx = BigNumContext::new()?;
    let pt = EcPoint::from_bytes(&g, &bytes, &mut ctx).map_err(|_| ProxyError::BadRequest)?;
    let peer =
        PKey::from_ec_key(EcKey::from_public_key(&g, &pt).map_err(|_| ProxyError::BadRequest)?)?;
    let mine = PKey::from_ec_key(own.clone())?;
    let mut d = Deriver::new(&mine)?;
    d.set_peer(&peer)?;
    let secret = d.derive_to_vec()?;
    let mut input = b"ham-session-v1\0".to_vec();
    input.extend_from_slice(&secret);
    input.extend_from_slice(nonce.as_bytes());
    Ok(hash(MessageDigest::sha256(), &input)?.to_vec())
}

// [@ANCHOR: ham_proxy_session_mac]
/// The request MAC (lowercase hex) under `key`.
pub fn mac(
    key: &[u8],
    method: &str,
    target: &str,
    host: &str,
    ts: u64,
    nonce: &str,
    body_sha256: &str,
) -> Result<String, ProxyError> {
    let msg = format!("ham-req-v1\n{method}\n{target}\n{host}\n{ts}\n{nonce}\n{body_sha256}");
    let k = PKey::hmac(key)?;
    let mut s = Signer::new(MessageDigest::sha256(), &k)?;
    s.update(msg.as_bytes())?;
    Ok(hex(&s.sign_to_vec()?))
}

impl Sessions {
    // [@ANCHOR: ham_proxy_session_open]
    /// Complete the key agreement for a verified login: returns `(session id, our public point hex)`.
    pub fn open(
        &self,
        callsign: &str,
        peer_hex: &str,
        nonce: &str,
        now: u64,
    ) -> Result<(String, String), ProxyError> {
        let g = group()?;
        let own = EcKey::generate(&g)?;
        let key = derive_key(&own, peer_hex, nonce)?;
        let mut ctx = BigNumContext::new()?;
        let ours =
            hex(&own
                .public_key()
                .to_bytes(&g, PointConversionForm::UNCOMPRESSED, &mut ctx)?);
        let mut b = [0u8; 16];
        rand_bytes(&mut b)?;
        let sid = hex(&b);
        let mut m = self.0.lock().map_err(|_| ProxyError::BadRequest)?;
        m.retain(|_, s| s.expires > now);
        if m.len() >= MAX_SESSIONS {
            return Err(ProxyError::BadRequest);
        }
        m.insert(
            sid.clone(),
            Session {
                key,
                callsign: callsign.to_string(),
                expires: now + SESSION_SECS,
                seen: HashMap::new(),
            },
        );
        Ok((sid, ours))
    }

    // [@ANCHOR: ham_proxy_session_check]
    /// Check the `X-Ham-*` headers of a request (`head` is the whole request head). `Ok(None)` when the request
    /// carries no `X-Ham-Session` (anonymous); `Ok(Some(callsign))` for a valid proof; `Err` otherwise.
    pub fn check(&self, head: &str, now: u64) -> Result<Option<String>, Refusal> {
        let h = |n: &str| crate::indicator::header(head, n);
        let Some(sid) = h("x-ham-session") else {
            return Ok(None);
        };
        let (Some(ts), Some(nonce), Some(got)) = (h("x-ham-ts"), h("x-ham-nonce"), h("x-ham-mac"))
        else {
            return Err(Refusal::Malformed);
        };
        let body = h("x-ham-body-sha256").unwrap_or("");
        let host = h("host").unwrap_or("");
        let mut first = head.lines().next().unwrap_or("").split(' ');
        let (method, target) = (first.next().unwrap_or(""), first.next().unwrap_or(""));
        let ts: u64 = ts.parse().map_err(|_| Refusal::Malformed)?;
        if nonce.is_empty() || nonce.len() > 64 || !nonce.bytes().all(|c| c.is_ascii_alphanumeric())
        {
            return Err(Refusal::Malformed);
        }
        let mut m = self.0.lock().map_err(|_| Refusal::UnknownSession)?;
        let s = m
            .get_mut(sid)
            .filter(|s| s.expires > now)
            .ok_or(Refusal::UnknownSession)?;
        if now.abs_diff(ts) > SKEW_SECS {
            return Err(Refusal::Stale);
        }
        let want =
            mac(&s.key, method, target, host, ts, nonce, body).map_err(|_| Refusal::BadMac)?;
        if want.len() != got.len()
            || !memcmp::eq(want.as_bytes(), got.to_ascii_lowercase().as_bytes())
        {
            return Err(Refusal::BadMac);
        }
        s.seen.retain(|_, t| now.abs_diff(*t) <= 2 * SKEW_SECS);
        if s.seen.insert(nonce.to_string(), ts).is_some() {
            return Err(Refusal::Replay);
        }
        Ok(Some(s.callsign.clone()))
    }
}
