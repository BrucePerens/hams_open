// SPDX-License-Identifier: LGPL-3.0-or-later
//! One-shot diagnostic: try several plausible bit-packing conventions for unpacking a real
//! chip-captured 18-byte frame into c0..c7 (widths 23,23,23,23,15,15,15,7 = 144 bits), and run each
//! through this crate's own decoder's FEC stage. If a convention is right, a real chip's own
//! self-consistent output should decode with a low corrected-error count (its own Golay/Hamming
//! codewords are genuinely valid, independent of whether this crate's own quantization tables agree
//! with the chip's *meaning* of those bits) -- a high error count / REPEAT-classification means that
//! convention is wrong, before ever getting to whether the tables match.

use ham_digital_modes::ambe::decode::{DecoderState, FrameOutcome};

fn bits_msb_first(bytes: &[u8; 18]) -> Vec<bool> {
    let mut bits = Vec::with_capacity(144);
    for &byte in bytes {
        for b in (0..8).rev() {
            bits.push((byte >> b) & 1 == 1);
        }
    }
    bits
}

fn bits_lsb_first_per_byte(bytes: &[u8; 18]) -> Vec<bool> {
    let mut bits = Vec::with_capacity(144);
    for &byte in bytes {
        for b in 0..8 {
            bits.push((byte >> b) & 1 == 1);
        }
    }
    bits
}

fn pack_fields(bits: &[bool], widths: &[usize; 8]) -> [u32; 8] {
    let mut c = [0u32; 8];
    let mut pos = 0;
    for (i, &w) in widths.iter().enumerate() {
        let mut val = 0u32;
        for &bit in &bits[pos..pos + w] {
            val = (val << 1) | (bit as u32);
        }
        c[i] = val;
        pos += w;
    }
    c
}

fn hex_to_bytes(s: &str) -> [u8; 18] {
    let mut out = [0u8; 18];
    for i in 0..18 {
        out[i] = u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).unwrap();
    }
    out
}

fn try_decode(label: &str, c: [u32; 8]) {
    let mut decoder = DecoderState::new();
    match decoder.decode_parameters(c) {
        Some(FrameOutcome::Decoded(params)) => {
            println!(
                "{label}: DECODED, errors.total={} epsilon_0={} epsilon_4={} l_hat={} k_hat={}",
                params.errors.total, params.errors.golay_init, params.errors.hamming_init,
                params.l_hat, params.k_hat
            );
        }
        Some(FrameOutcome::Repeat) => println!("{label}: REPEAT (high error rate)"),
        Some(FrameOutcome::Mute) => println!("{label}: MUTE (high error rate)"),
        None => println!("{label}: None (invalid)"),
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let hex = args
        .get(1)
        .map(|s| s.as_str())
        .unwrap_or("9a0cf0ab0580330e143c0d84594f405d02e0");
    let bytes = hex_to_bytes(hex);

    let widths_fwd = [23usize, 23, 23, 23, 15, 15, 15, 7];
    let mut widths_rev = widths_fwd;
    widths_rev.reverse();

    let msb_bits = bits_msb_first(&bytes);
    let lsb_bits = bits_lsb_first_per_byte(&bytes);
    let mut msb_bits_reversed = msb_bits.clone();
    msb_bits_reversed.reverse();

    // (1) MSB-first overall stream, c0..c7 in order.
    try_decode("1 msb-first, c0..c7", pack_fields(&msb_bits, &widths_fwd));
    // (2) LSB-first within each byte, byte order unchanged, c0..c7 in order.
    try_decode("2 lsb-first-per-byte, c0..c7", pack_fields(&lsb_bits, &widths_fwd));
    // (3) Whole 144-bit stream reversed (last bit first), c0..c7 in order.
    try_decode("3 whole-stream-reversed, c0..c7", pack_fields(&msb_bits_reversed, &widths_fwd));
    // (4) MSB-first overall stream, fields in reverse order (c7..c0).
    {
        let c = pack_fields(&msb_bits, &widths_rev);
        let c_fwd = [c[7], c[6], c[5], c[4], c[3], c[2], c[1], c[0]];
        try_decode("4 msb-first, c7..c0 field order", c_fwd);
    }
    // (5) Bytes reversed (byte 17 first), bits MSB-first within each byte, c0..c7 in order.
    {
        let mut rev_bytes = bytes;
        rev_bytes.reverse();
        let bits = bits_msb_first(&rev_bytes);
        try_decode("5 byte-order-reversed, msb-first, c0..c7", pack_fields(&bits, &widths_fwd));
    }
}
