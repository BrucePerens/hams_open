// SPDX-License-Identifier: AGPL-3.0-or-later
//! `Response::decode` on arbitrary bodies: no panic; whatever decodes must re-encode to bytes that decode to the same value.
#![no_main]
use libfuzzer_sys::fuzz_target;
use relay_broker::wire::Response;

fuzz_target!(|data: &[u8]| {
    if let Ok(resp) = Response::decode(data) {
        let again = Response::decode(&resp.encode());
        assert_eq!(again.as_ref(), Ok(&resp));
    }
});
