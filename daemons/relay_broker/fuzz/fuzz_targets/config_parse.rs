// SPDX-License-Identifier: AGPL-3.0-or-later
//! `Config::parse` on arbitrary text: no panic.
#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(text) = std::str::from_utf8(data) {
        let _ = relay_broker::config::Config::parse(text);
    }
});
