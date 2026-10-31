// SPDX-License-Identifier: AGPL-3.0-or-later
//! Property tests standing in for fuzzing: no datagram, however malformed, may panic a packet
//! parser. Any datagram at all, and well-formed headers followed by any body.
#![allow(clippy::expect_used)]

use proptest::prelude::*;

proptest! {
    // Tests [@ANCHOR: hpsdr_p1_parse_reply]
    // Tests [@ANCHOR: hpsdr_p1_parse_ep2]
    // Tests [@ANCHOR: hpsdr_p1_parse_ep6]
    // Tests [@ANCHOR: hpsdr_p2_parse_reply]
    // Tests [@ANCHOR: hpsdr_p2_parse_hp_status]
    // Tests [@ANCHOR: hpsdr_p2_parse_ddc]
    // Tests [@ANCHOR: hpsdr_discovery_parse_reply]
    #[test]
    fn hpsdr_packet_parsers_survive_any_datagram(
        bytes in proptest::collection::vec(any::<u8>(), 0..1500),
        num_rx in any::<u8>(),
    ) {
        let from: std::net::SocketAddr = "192.168.1.2:1024".parse().expect("addr");
        let _ = hpsdr::p1::parse_reply(&bytes);
        let _ = hpsdr::p1::parse_ep2(&bytes);
        let _ = hpsdr::p1::parse_ep6(&bytes, num_rx);
        let _ = hpsdr::p2::parse_reply(&bytes);
        let _ = hpsdr::p2::parse_hp_status(&bytes);
        let _ = hpsdr::p2::parse_ddc_packet(&bytes);
        let _ = hpsdr::discovery::parse_reply(from, &bytes);
    }

    // Tests [@ANCHOR: hpsdr_p1_parse_ep6]
    // Tests [@ANCHOR: hpsdr_p2_parse_ddc]
    #[test]
    fn hpsdr_data_parsers_survive_well_formed_headers_with_any_body(
        body in proptest::collection::vec(any::<u8>(), 0..1500),
        num_rx in 1u8..=8,
        count in any::<u16>(),
    ) {
        // Protocol 1 EP6: magic, end point 6, a sequence, then whatever.
        let mut ep6 = vec![0xEF, 0xFE, 0x01, 0x06, 0, 0, 0, 1];
        ep6.extend_from_slice(&body);
        let _ = hpsdr::p1::parse_ep6(&ep6, num_rx);
        // Protocol 2 DDC: sequence, timestamp, 24 bits per sample, a sample count, then whatever.
        let mut ddc = vec![0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 24];
        ddc.extend_from_slice(&count.to_be_bytes());
        ddc.extend_from_slice(&body);
        let _ = hpsdr::p2::parse_ddc_packet(&ddc);
    }
}
