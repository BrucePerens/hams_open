// SPDX-License-Identifier: AGPL-3.0-or-later
//! `ChannelStatus::decode` on arbitrary datagrams, then the derived views: none may panic.
#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(Some(s)) = radiod_client::status::ChannelStatus::decode(data) {
        let _ = s.carrier_lock();
        let _ = s.spectrum_db(7_074_000.0);
        let _ = s.spectrum_db(f64::NAN);
    }
});
