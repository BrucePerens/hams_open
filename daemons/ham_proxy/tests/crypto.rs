#![allow(clippy::unwrap_used, clippy::expect_used)]
//! `OpensslCrypto`: the core's Ed25519 check and random source, and the login nonce that uses them.
use ham_proxy::crypto::OpensslCrypto;
use ham_proxy::login::Challenges;
use hams_net_core::crypto::{BadKey, Crypto};
use openssl::pkey::PKey;
use openssl::sign::Signer;

// Tests [@ANCHOR: ham_proxy_openssl_ed25519_verify]
#[test]
fn ed25519_accepts_a_good_signature_only() {
    let k = PKey::generate_ed25519().unwrap();
    let raw = k.raw_public_key().unwrap();
    let sig = Signer::new_without_digest(&k)
        .unwrap()
        .sign_oneshot_to_vec(b"msg")
        .unwrap();
    assert_eq!(OpensslCrypto.ed25519_verify(&raw, &sig, b"msg"), Ok(true));
    assert_eq!(
        OpensslCrypto.ed25519_verify(&raw, &sig, b"other"),
        Ok(false)
    );
    assert_eq!(
        OpensslCrypto.ed25519_verify(&raw[..5], &sig, b"msg"),
        Err(BadKey)
    );
}

// Tests [@ANCHOR: ham_proxy_openssl_random16]
// Tests [@ANCHOR: ham_proxy_login_issue]
#[test]
fn nonces_are_fresh_128_bit_hex() {
    assert_ne!(OpensslCrypto.random16(), OpensslCrypto.random16());
    let ch = Challenges::default();
    let (a, b) = (ch.issue(10).unwrap(), ch.issue(10).unwrap());
    assert!(a.len() == 32 && a.bytes().all(|c| c.is_ascii_hexdigit()) && a != b);
    assert!(ch.take(&a, 11) && !ch.take(&a, 11));
}
