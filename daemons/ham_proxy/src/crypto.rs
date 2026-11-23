//! OpenSSL behind `hams_net_core::crypto::Crypto`.

use hams_net_core::crypto::{BadKey, Crypto};
use openssl::base64::{decode_block, encode_block};
use openssl::hash::{hash, MessageDigest};
use openssl::pkey::{Id, PKey};
use openssl::rand::rand_bytes;
use openssl::sign::Verifier;

/// The core's primitives from the system OpenSSL.
pub struct OpensslCrypto;

impl Crypto for OpensslCrypto {
    fn sha256(&self, data: &[u8]) -> Option<Vec<u8>> {
        hash(MessageDigest::sha256(), data).ok().map(|d| d.to_vec())
    }

    fn sha512(&self, data: &[u8]) -> Option<Vec<u8>> {
        hash(MessageDigest::sha512(), data).ok().map(|d| d.to_vec())
    }

    // [@ANCHOR: ham_proxy_openssl_ed25519_verify]
    fn ed25519_verify(&self, key: &[u8], sig: &[u8], msg: &[u8]) -> Result<bool, BadKey> {
        let pkey = PKey::public_key_from_raw_bytes(key, Id::ED25519).map_err(|_| BadKey)?;
        let mut v = Verifier::new_without_digest(&pkey).map_err(|_| BadKey)?;
        Ok(v.verify_oneshot(sig, msg).unwrap_or(false))
    }

    fn base64_encode(&self, data: &[u8]) -> String {
        encode_block(data)
    }

    fn base64_decode(&self, text: &str) -> Option<Vec<u8>> {
        decode_block(text).ok()
    }

    // [@ANCHOR: ham_proxy_openssl_random16]
    fn random16(&self) -> Option<[u8; 16]> {
        let mut b = [0u8; 16];
        rand_bytes(&mut b).ok()?;
        Some(b)
    }
}
