// SPDX-License-Identifier: AGPL-3.0-or-later
//! The OpenHPSDR packet parsers (Protocol 1 and 2, discovery) fed arbitrary datagrams. The first
//! input byte is the number of receivers the data parsers are told to expect.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Some((num_rx, bytes)) = data.split_first() else {
        return;
    };
    let from: std::net::SocketAddr = std::net::SocketAddr::from(([192, 168, 1, 2], 1024));
    let _ = hpsdr::p1::parse_reply(bytes);
    let _ = hpsdr::p1::parse_ep2(bytes);
    let _ = hpsdr::p1::parse_ep6(bytes, *num_rx);
    let _ = hpsdr::p2::parse_reply(bytes);
    let _ = hpsdr::p2::parse_hp_status(bytes);
    let _ = hpsdr::p2::parse_ddc_packet(bytes);
    let _ = hpsdr::discovery::parse_reply(from, bytes);
});
