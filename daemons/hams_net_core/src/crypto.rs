//! The few cryptographic primitives the core needs, supplied by the consumer (OpenSSL in `ham_proxy`, the platform
//! library in the phone apps), so the core itself stays free of native dependencies. Nothing here encrypts: hashes,
//! a signature check, base64 and random bytes only.

/// The key given to [`Crypto::ed25519_verify`] cannot be used (wrong length or not a point).
#[derive(Debug, PartialEq, Eq)]
pub struct BadKey;

/// Primitives the DANE, signed-list and login code call.
pub trait Crypto {
    /// SHA-256 of `data`.
    fn sha256(&self, data: &[u8]) -> Option<Vec<u8>>;
    /// SHA-512 of `data`.
    fn sha512(&self, data: &[u8]) -> Option<Vec<u8>>;
    /// Ed25519 check of `sig` over `msg` under the raw 32-byte `key`: `Err(BadKey)` when the key is unusable, `Ok(false)`
    /// for a signature that does not verify.
    fn ed25519_verify(&self, key: &[u8], sig: &[u8], msg: &[u8]) -> Result<bool, BadKey>;
    /// Standard base64 (with padding, no line breaks) of `data`.
    fn base64_encode(&self, data: &[u8]) -> String;
    /// Bytes of standard base64 text, `None` if it does not decode.
    fn base64_decode(&self, text: &str) -> Option<Vec<u8>>;
    /// 16 random bytes from the system generator, `None` if it fails.
    fn random16(&self) -> Option<[u8; 16]>;
}
