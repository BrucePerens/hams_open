//! The "signed, not encrypted" indicator for pages relayed over a ham link: banner injection, blocking of
//! password fields that are not handled by the signature login, and a signed integrity header the browser
//! extension verifies. Pure functions over bytes; the relay in `lib.rs` calls them.
//!
//! Wire format of the header (spec for the extension): `X-Ham-Integrity: v1; ts=<unix>; host=<site>;
//! sha256=<hex of the body as delivered>; sig=<hex Ed25519 signature>`, signed over the bytes
//! `"ham-integrity-v1\n{ts}\n{host}\n{sha256}\n"`.

use openssl::hash::{hash, MessageDigest};
use openssl::pkey::{PKey, Private, Public};
use openssl::sign::{Signer, Verifier};

/// Name of the response header carrying the signed integrity record.
pub const HEADER: &str = "X-Ham-Integrity";
/// Marker id of the injected banner; its presence means the page already carries the indicator.
pub const BANNER_ID: &str = "ham-integrity-banner";
/// Attribute a password input carries when the extension/proxy signature login handles it.
pub const LOGIN_ATTR: &str = "data-ham-login";

const BANNER: &str =
    "<div id=\"ham-integrity-banner\" style=\"position:sticky;top:0;z-index:2147483647;\
background:#fc0;color:#000;font:14px sans-serif;padding:4px 8px;text-align:center\">\
Signed, not encrypted: this page crossed a ham radio link. Do not send secrets.</div>";

/// Page shown instead of a page whose password field would cross the air in clear.
pub const BLOCK_PAGE: &str = "<!doctype html><html><body><div id=\"ham-integrity-banner\">Signed, not \
encrypted: do not send secrets.</div><h1>Blocked</h1><p>This page asks for a password on a ham link, which is \
not encrypted. Use the signature login at <a href=\"/.ham/login\">/.ham/login</a>.</p></body></html>";

// [@ANCHOR: ham_proxy_find_ci]
fn find_ci(h: &[u8], needle: &str, from: usize) -> Option<usize> {
    let n = needle.as_bytes();
    if n.is_empty() || h.len() < n.len() {
        return None;
    }
    (from..=h.len() - n.len()).find(|&i| h[i..i + n.len()].eq_ignore_ascii_case(n))
}

/// Insert the banner after the opening `<body ...>` tag (or at the start), unless it is already there.
pub fn inject(html: &[u8]) -> Vec<u8> {
    if find_ci(html, BANNER_ID, 0).is_some() {
        return html.to_vec();
    }
    let at = find_ci(html, "<body", 0)
        .and_then(|i| html[i..].iter().position(|&b| b == b'>').map(|j| i + j + 1))
        .unwrap_or(0);
    let mut out = Vec::with_capacity(html.len() + BANNER.len());
    out.extend_from_slice(&html[..at]);
    out.extend_from_slice(BANNER.as_bytes());
    out.extend_from_slice(&html[at..]);
    out
}

/// True if some `<input type=password>` is not marked as handled by the signature login.
pub fn has_unhandled_password(html: &[u8]) -> bool {
    let mut pos = 0;
    while let Some(i) = find_ci(html, "<input", pos) {
        let end = html[i..]
            .iter()
            .position(|&b| b == b'>')
            .map_or(html.len(), |j| i + j);
        let tag = String::from_utf8_lossy(&html[i..end]).to_ascii_lowercase();
        let compact: String = tag.chars().filter(|c| !" \t\r\n'\"".contains(*c)).collect();
        if compact.contains("type=password") && !tag.contains(LOGIN_ATTR) {
            return true;
        }
        pos = end.max(i + 1);
    }
    false
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn unhex(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) || !s.is_ascii() {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok())
        .collect()
}

fn message(ts: u64, host: &str, sha: &str) -> Vec<u8> {
    format!("ham-integrity-v1\n{ts}\n{host}\n{sha}\n").into_bytes()
}

/// Build the header value for `body` served for `host` at unix time `ts`, signed with an Ed25519 key.
pub fn sign_header(
    key: &PKey<Private>,
    host: &str,
    ts: u64,
    body: &[u8],
) -> Result<String, openssl::error::ErrorStack> {
    let sha = hex(&hash(MessageDigest::sha256(), body)?);
    let sig = Signer::new_without_digest(key)?.sign_oneshot_to_vec(&message(ts, host, &sha))?;
    Ok(format!(
        "v1; ts={ts}; host={host}; sha256={sha}; sig={}",
        hex(&sig)
    ))
}

/// Why a header failed verification.
#[derive(Debug, PartialEq, Eq)]
pub enum VerifyError {
    /// Not a well-formed v1 header.
    Malformed,
    /// The body hash differs: the page was modified.
    BodyChanged,
    /// The record was issued for another host.
    WrongHost,
    /// The timestamp is outside the allowed window (replay of an old page).
    Stale,
    /// The signature does not verify under the proxy key.
    BadSignature,
}

/// Verify a header value against the received body, the host asked for, the proxy key and the clock.
pub fn verify_header(
    value: &str,
    key: &PKey<Public>,
    host: &str,
    body: &[u8],
    now: u64,
    max_age: u64,
) -> Result<(), VerifyError> {
    let mut parts = value.split("; ");
    if parts.next() != Some("v1") {
        return Err(VerifyError::Malformed);
    }
    let field = |name: &str, it: &mut std::str::Split<'_, &str>| {
        it.next()
            .and_then(|p| p.strip_prefix(name))
            .map(str::to_string)
    };
    let ts: u64 = field("ts=", &mut parts)
        .and_then(|v| v.parse().ok())
        .ok_or(VerifyError::Malformed)?;
    let h = field("host=", &mut parts).ok_or(VerifyError::Malformed)?;
    let sha = field("sha256=", &mut parts).ok_or(VerifyError::Malformed)?;
    let sig = field("sig=", &mut parts)
        .and_then(|s| unhex(&s))
        .ok_or(VerifyError::Malformed)?;
    let ok = Verifier::new_without_digest(key)
        .and_then(|mut v| v.verify_oneshot(&sig, &message(ts, &h, &sha)))
        .map_err(|_| VerifyError::BadSignature)?;
    if !ok {
        return Err(VerifyError::BadSignature);
    }
    let actual = hash(MessageDigest::sha256(), body).map_err(|_| VerifyError::Malformed)?;
    if hex(&actual) != sha {
        return Err(VerifyError::BodyChanged);
    }
    if h != host {
        return Err(VerifyError::WrongHost);
    }
    if now.saturating_sub(ts) > max_age || ts > now + 60 {
        return Err(VerifyError::Stale);
    }
    Ok(())
}

// [@ANCHOR: ham_proxy_dechunk]
/// Decode a chunked body; `None` if malformed.
pub fn dechunk(mut b: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    loop {
        let nl = b.windows(2).position(|w| w == b"\r\n")?;
        let size_s = std::str::from_utf8(&b[..nl])
            .ok()?
            .split(';')
            .next()?
            .trim();
        let n = usize::from_str_radix(size_s, 16).ok()?;
        b = &b[nl + 2..];
        if n == 0 {
            return Some(out);
        }
        if b.len() < n + 2 {
            return None;
        }
        out.extend_from_slice(&b[..n]);
        b = &b[n + 2..];
    }
}

/// Header lookup (case-insensitive) in a response head.
pub fn header<'a>(head: &'a str, name: &str) -> Option<&'a str> {
    head.split("\r\n").skip(1).find_map(|l| {
        let (k, v) = l.split_once(':')?;
        k.eq_ignore_ascii_case(name).then(|| v.trim())
    })
}

#[cfg(test)]
mod tests {
    use super::{dechunk, find_ci};

    #[test]
    fn find_ci_matches_any_case_from_an_offset_and_not_an_empty_needle() {
        // Tests [@ANCHOR: ham_proxy_find_ci]
        assert_eq!(find_ci(b"<HTML><BODY>", "<body", 0), Some(6));
        assert_eq!(find_ci(b"<HTML><BODY>", "<body", 7), None);
        assert_eq!(find_ci(b"abc", "", 0), None);
        assert_eq!(find_ci(b"ab", "abc", 0), None);
    }

    #[test]
    fn dechunk_joins_chunks_ignores_extensions_and_refuses_a_truncated_body() {
        // Tests [@ANCHOR: ham_proxy_dechunk]
        assert_eq!(
            dechunk(b"3\r\nabc\r\n2;x=1\r\nde\r\n0\r\n\r\n"),
            Some(b"abcde".to_vec())
        );
        assert_eq!(dechunk(b"5\r\nab"), None);
        assert_eq!(dechunk(b"zz\r\n"), None);
    }
}
