// SPDX-License-Identifier: LGPL-3.0-or-later
//! One-shot diagnostic: try several plausible bit-packing conventions for unpacking a real
//! chip-captured 18-byte frame into c0..c7 (widths 23,23,23,23,15,15,15,7 = 144 bits), and run each
//! through this crate's own decoder's FEC stage. If a convention is right, a real chip's own
//! self-consistent output should decode with a low corrected-error count (its own Golay/Hamming
//! codewords are genuinely valid, independent of whether this crate's own quantization tables agree
//! with the chip's *meaning* of those bits) -- a high error count / REPEAT-classification means that
//! convention is wrong, before ever getting to whether the tables match.

use ham_digital_modes::ambe::decode::{DecoderState, FrameOutcome};
use ham_digital_modes::ambe::fec::golay_decode;
use ham_digital_modes::ambe::interleave::deinterleave_from_dibit_symbols;

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
    // Only c[0] (and c[7], never shown here) are meaningful to Golay/Hamming-decode *before*
    // demodulation -- c[1..=6] are still XORed with a u0-dependent PRN at this point (per
    // decode.rs's own documented order) and decoding them raw would show high error regardless of
    // whether the bit-order hypothesis under test is right. epsilon_0 alone is therefore the real,
    // meaningful signal here.
    let (u0, epsilon_0) = golay_decode(c[0]);
    print!("{label}: u0={:04x} epsilon_0={} ", u0, epsilon_0);

    let mut decoder = DecoderState::new();
    match decoder.decode_parameters(c) {
        Some(FrameOutcome::Decoded(params)) => {
            println!(
                "-> DECODED, errors.total={} l_hat={} k_hat={}",
                params.errors.total, params.l_hat, params.k_hat
            );
        }
        Some(FrameOutcome::Repeat) => println!("-> REPEAT"),
        Some(FrameOutcome::Mute) => println!("-> MUTE"),
        None => println!("-> None (invalid)"),
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
    // (6) Treat the raw bytes as this crate's own Annex H dibit-interleaved form (MSB-first bits,
    // grouped into 72 two-bit symbols) and deinterleave via this crate's own real
    // `interleave::deinterleave_from_dibit_symbols` before decoding -- motivated by a real empirical
    // finding (see AMBE_CHIP_VALIDATION_FINDINGS.md): bit positions that change between nearby test
    // frequencies cluster in an arithmetic sequence with stride ~12, consistent with a real
    // interleave already being applied to the chip's own raw serial output.
    {
        let mut symbols = [(false, false); 72];
        for (i, sym) in symbols.iter_mut().enumerate() {
            *sym = (msb_bits[i * 2], msb_bits[i * 2 + 1]);
        }
        let c = deinterleave_from_dibit_symbols(symbols);
        try_decode("6 deinterleaved via this crate's own Annex H table", c);
    }
    // (7) AMBETools' own real IMBE_INTERLEAVE[144] permutation (from Common/IMBEFEC.cpp, a real,
    // working P25 IMBE codec): bit[i] = data[IMBE_INTERLEAVE[i]] recovers the pre-interleave
    // c0..c7 concatenation from a real transmitted/interleaved frame.
    {
        const IMBE_INTERLEAVE: [usize; 144] = [
            0, 7, 12, 19, 24, 31, 36, 43, 48, 55, 60, 67, 72, 79, 84, 91, 96, 103, 108, 115, 120,
            127, 132, 139, 1, 6, 13, 18, 25, 30, 37, 42, 49, 54, 61, 66, 73, 78, 85, 90, 97, 102,
            109, 114, 121, 126, 133, 138, 2, 9, 14, 21, 26, 33, 38, 45, 50, 57, 62, 69, 74, 81, 86,
            93, 98, 105, 110, 117, 122, 129, 134, 141, 3, 8, 15, 20, 27, 32, 39, 44, 51, 56, 63, 68,
            75, 80, 87, 92, 99, 104, 111, 116, 123, 128, 135, 140, 4, 11, 16, 23, 28, 35, 40, 47, 52,
            59, 64, 71, 76, 83, 88, 95, 100, 107, 112, 119, 124, 131, 136, 143, 5, 10, 17, 22, 29,
            34, 41, 46, 53, 58, 65, 70, 77, 82, 89, 94, 101, 106, 113, 118, 125, 130, 137, 142,
        ];
        let mut deint_bits = vec![false; 144];
        for i in 0..144 {
            deint_bits[i] = msb_bits[IMBE_INTERLEAVE[i]];
        }
        try_decode("7 AMBETools IMBE_INTERLEAVE, msb-first, c0..c7", pack_fields(&deint_bits, &widths_fwd));

        // (7b) Same, but with lsb-first-per-byte source bits, in case the interleave table's own
        // implicit bit numbering (MSB=bit 0 of each byte, per its own WRITE_BIT/READ_BIT macros)
        // doesn't match how *we* happened to number bits when building msb_bits.
        let mut deint_bits2 = vec![false; 144];
        for i in 0..144 {
            deint_bits2[i] = lsb_bits[IMBE_INTERLEAVE[i]];
        }
        try_decode("7b AMBETools IMBE_INTERLEAVE, lsb-first-per-byte, c0..c7", pack_fields(&deint_bits2, &widths_fwd));
    }
}
