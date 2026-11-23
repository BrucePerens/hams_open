//! A fake `Crypto` for the core's tests: hex "base64", a counter for random bytes, and a signature that is valid
//! when it equals the message. The real primitives are covered by ham_proxy's tests against OpenSSL.
#![allow(dead_code)]
use hams_net_core::crypto::{BadKey, Crypto};
use std::sync::atomic::{AtomicU8, Ordering};

#[derive(Default)]
pub struct Fake(pub AtomicU8);

fn hex(d: &[u8]) -> String {
    d.iter().map(|b| format!("{b:02x}")).collect()
}

impl Crypto for Fake {
    fn sha256(&self, data: &[u8]) -> Option<Vec<u8>> {
        Some([b"s256:".as_slice(), data].concat())
    }
    fn sha512(&self, data: &[u8]) -> Option<Vec<u8>> {
        Some([b"s512:".as_slice(), data].concat())
    }
    fn ed25519_verify(&self, key: &[u8], sig: &[u8], msg: &[u8]) -> Result<bool, BadKey> {
        if key.len() != 32 {
            return Err(BadKey);
        }
        Ok(sig == msg)
    }
    fn base64_encode(&self, data: &[u8]) -> String {
        hex(data)
    }
    fn base64_decode(&self, text: &str) -> Option<Vec<u8>> {
        (0..text.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(text.get(i..i + 2)?, 16).ok())
            .collect()
    }
    fn random16(&self) -> Option<[u8; 16]> {
        Some([self.0.fetch_add(1, Ordering::SeqCst); 16])
    }
}
