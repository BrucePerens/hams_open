//! The signed site whitelist the gateway enforces (verification logic; the consumer supplies [`Crypto`]): the file hams.com publishes at `/ipv6/whitelist.json`, the same one the browser
//! extension verifies. `{"payload": base64, "signature": base64}`; the signature is Ed25519 over the payload bytes, the payload JSON
//! `{"version", "expires": "YYYY-MM-DDTHH:MM:SSZ", "entries": [{"host", "paths"?, ...}]}`. A bad signature, an expired list or a
//! malformed entry is refused, so a gateway that cannot read a good list permits only `.ham` names.

use crate::crypto::{BadKey, Crypto};
use crate::policy::Whitelist;

// [@ANCHOR: hams_net_core_signed_list_list_error]
/// Why a signed whitelist was refused.
#[derive(Debug, PartialEq, Eq)]
pub enum ListError {
    /// Not the `{payload, signature}` shape, or not decodable.
    Malformed,
    /// The Ed25519 signature does not verify under the key.
    BadSignature,
    /// The list's `expires` time has passed (or is not a time).
    Expired,
}

// [@ANCHOR: hams_net_core_signed_list_parse_utc]
/// Unix seconds of `YYYY-MM-DDTHH:MM:SSZ`, `None` for anything else.
pub fn parse_utc(s: &str) -> Option<u64> {
    let b = s.as_bytes();
    let ok = b.len() == 20
        && [4, 7].iter().all(|&i| b[i] == b'-')
        && b[10] == b'T'
        && [13, 16].iter().all(|&i| b[i] == b':')
        && b[19] == b'Z';
    let num = |r: std::ops::Range<usize>| s.get(r)?.parse::<u64>().ok();
    if !ok {
        return None;
    }
    let (y, m, d) = (num(0..4)?, num(5..7)?, num(8..10)?);
    let (h, mi, se) = (num(11..13)?, num(14..16)?, num(17..19)?);
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) || h > 23 || mi > 59 || se > 60 {
        return None;
    }
    // days since 1970-01-01 (proleptic Gregorian, civil-from-days inverse)
    let y = if m <= 2 { y - 1 } else { y };
    let era = y / 400;
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = (era * 146_097 + doe).checked_sub(719_468)?;
    Some(days * 86_400 + h * 3600 + mi * 60 + se)
}

// [@ANCHOR: hams_net_core_signed_list_verify_list]
/// Verify `text` (the published JSON) under the raw 32-byte Ed25519 `key` at unix time `now`, and return the whitelist it names.
pub fn verify_list(
    crypto: &dyn Crypto,
    text: &str,
    key: &[u8],
    now: u64,
) -> Result<Whitelist, ListError> {
    let doc: serde_json::Value = serde_json::from_str(text).map_err(|_| ListError::Malformed)?;
    let field = |n: &str| {
        doc.get(n)
            .and_then(|v| v.as_str())
            .and_then(|s| crypto.base64_decode(s))
    };
    let (payload, sig) = field("payload")
        .zip(field("signature"))
        .ok_or(ListError::Malformed)?;
    match crypto.ed25519_verify(key, &sig, &payload) {
        Err(BadKey) => return Err(ListError::Malformed),
        Ok(false) => return Err(ListError::BadSignature),
        Ok(true) => {}
    }
    let body: serde_json::Value =
        serde_json::from_slice(&payload).map_err(|_| ListError::Malformed)?;
    let expires = body
        .get("expires")
        .and_then(|v| v.as_str())
        .and_then(parse_utc);
    if expires.is_none_or(|e| e <= now) {
        return Err(ListError::Expired);
    }
    let entries = body
        .get("entries")
        .and_then(|v| v.as_array())
        .ok_or(ListError::Malformed)?;
    let mut w = Whitelist::default();
    for e in entries {
        let host = e
            .get("host")
            .and_then(|v| v.as_str())
            .ok_or(ListError::Malformed)?;
        let paths = match e.get("paths") {
            None => Vec::new(),
            Some(p) => p
                .as_array()
                .and_then(|a| {
                    a.iter()
                        .map(|x| x.as_str().map(str::to_string))
                        .collect::<Option<Vec<_>>>()
                })
                .ok_or(ListError::Malformed)?,
        };
        w.add_site(host, paths);
    }
    Ok(w)
}

// [@ANCHOR: hams_net_core_signed_list_list_unhex]
/// Bytes of a hex string, `None` if it is odd-length or not hex.
pub fn unhex(s: &str) -> Option<Vec<u8>> {
    s.len()
        .is_multiple_of(2)
        .then(|| {
            (0..s.len())
                .step_by(2)
                .map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok())
                .collect()
        })
        .flatten()
}
