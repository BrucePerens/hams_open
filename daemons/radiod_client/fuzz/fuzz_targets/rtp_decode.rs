// SPDX-License-Identifier: AGPL-3.0-or-later
//! `RtpPacket::parse`, then every PCM decoder on its payload, and `SeqTracker`: none may panic.
#![no_main]
use libfuzzer_sys::fuzz_target;
use radiod_client::rtp::{AudioDecoder, Encoding, RtpPacket, SeqTracker};

fuzz_target!(|data: &[u8]| {
    if let Ok(p) = RtpPacket::parse(data) {
        let mut seq = SeqTracker::default();
        seq.push(p.seq);
        for c in data.chunks_exact(2) {
            seq.push(u16::from_be_bytes([c[0], c[1]]));
        }
        for n in 0..=40u64 {
            if let Some(enc) = Encoding::from_radiod(n) {
                for ch in [0usize, 1, 2, 3] {
                    if let Ok(mut d) = AudioDecoder::new(enc, ch) {
                        let _ = d.decode(p.payload);
                    }
                }
            }
        }
    }
});
