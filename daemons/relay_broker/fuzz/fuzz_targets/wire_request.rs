// SPDX-License-Identifier: AGPL-3.0-or-later
//! `Request::decode` on arbitrary bodies: no panic; whatever decodes must re-encode to bytes that decode to the same value.
#![no_main]
use libfuzzer_sys::fuzz_target;
use relay_broker::wire::{frame_body_len, Request};

fuzz_target!(|data: &[u8]| {
    if let Some(h) = data.get(..4) {
        let _ = frame_body_len([h[0], h[1], h[2], h[3]]);
    }
    if let Ok(req) = Request::decode(data) {
        let again = Request::decode(&req.encode());
        assert_eq!(again.as_ref(), Ok(&req));
    }
});
