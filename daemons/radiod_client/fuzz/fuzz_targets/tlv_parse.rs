// SPDX-License-Identifier: AGPL-3.0-or-later
//! `tlv::parse` and every `Entry` accessor on arbitrary datagrams: none may panic.
#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok((_kind, entries)) = radiod_client::tlv::parse(data) {
        for e in entries {
            let _ = e.uint();
            let _ = e.bool();
            let _ = e.f32();
            let _ = e.f64();
            let _ = e.text();
            let _ = e.socket();
            let _ = e.f32_vec();
        }
    }
});
