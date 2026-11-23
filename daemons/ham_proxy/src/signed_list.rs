//! The signed site whitelist: the logic lives in `hams_net_core::signed_list`; this binds it to OpenSSL.

use crate::crypto::OpensslCrypto;
use crate::policy::Whitelist;

pub use hams_net_core::signed_list::{parse_utc, unhex, ListError};

/// Verify `text` (the published JSON) under the raw 32-byte Ed25519 `key` at unix time `now`, and return the whitelist it names.
pub fn verify_list(text: &str, key: &[u8], now: u64) -> Result<Whitelist, ListError> {
    hams_net_core::signed_list::verify_list(&OpensslCrypto, text, key, now)
}
