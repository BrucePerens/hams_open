// SPDX-License-Identifier: LGPL-3.0-or-later
//! Zero-chip-time convention search: uses the two confirmed-unprotected wire bit positions from
//! `p25_ratet27_bitflip_oracle.rs` (131 and 143) as ground truth to find which framing convention
//! is correct, instead of guessing and re-testing against the chip.
//!
//! **Why 131/143 are ground truth, not just "2 of 7"**: this crate's own, independently-verified
//! `bit_prioritization::extract_fundamental_frequency_quantizer` formula (cross-checked against
//! GopherTrunk's independent implementation) is `b_hat_0 = ((u[0]>>6)<<2) | ((u[7]>>1)&0b11)` --
//! two of the pitch quantizer's bits live in `u[7]` (`c7`, the 7 raw/unprotected/unmodulated bits),
//! at c7's own internal bit-index 1 and 2 (0-indexed from the LSB). For a steady 200 Hz voiced test
//! tone, flipping either pitch bit shifts every harmonic, hence the 22 dB spectral change the bit-
//! flip oracle found; flipping the other 5 raw bits (low-order spectral-amplitude LSBs) has no
//! detectable effect on a signal whose non-fundamental bands are already near the amplitude floor --
//! so exactly 2 hits is the textbook-predicted count, not a shortfall.
//!
//! This tool tries every combination of: byte order x bit direction (this crate's usual 4
//! hypotheses), whether Table 5-1's own dibit convention (wire bit `2s`=Bit1, `2s+1`=Bit0) needs
//! swapping, whether `TIA_INDEX`'s MSB-first convention (`6`=MSB for c7) needs reversing to LSB-
//! first, and whether the chip's raw serial data is OTA-interleaved (Table 5-1 applies) or natural
//! contiguous codeword order (already ruled out for Golay validity by the sliding-window scan, but
//! cheap to also check here for c7 specifically). For each, it computes where c7's bit-index-1 and
//! bit-index-2 land in the wire frame (in this crate's own raw-packet bit numbering, i.e. the same
//! `k` `p25_ratet27_bitflip_oracle.rs`'s `flip_bit_in_packet` uses) and reports any match against
//! {131, 143}.
const TOTAL_BITS: usize = 144;
const BLOCK_SIZES: [usize; 8] = [23, 23, 23, 23, 15, 15, 15, 7];
const C7_BLOCK: usize = 7;

const TIA_BLOCK: [usize; 144] = [
    0, 1, 2, 3, 4, 5, 1, 0, 3, 2, 5, 4, 0, 1, 2, 3, 4, 6, 1, 0, 3, 2, 6, 4, 0, 1, 2, 3, 4, 6, 1, 0,
    3, 2, 6, 4, 0, 1, 2, 3, 4, 6, 1, 0, 3, 2, 6, 4, 0, 1, 2, 3, 4, 6, 1, 0, 3, 2, 6, 4, 0, 1, 2, 3,
    4, 6, 1, 0, 3, 2, 6, 5, 0, 1, 2, 3, 5, 6, 1, 0, 3, 2, 6, 5, 0, 1, 2, 3, 5, 6, 1, 0, 3, 2, 6, 5,
    0, 1, 2, 3, 5, 6, 1, 0, 3, 2, 7, 5, 0, 1, 2, 3, 5, 7, 1, 0, 3, 2, 7, 5, 0, 1, 2, 4, 5, 7, 1, 0,
    4, 3, 7, 5, 0, 2, 3, 4, 5, 7, 2, 1, 4, 3, 7, 5,
];
const TIA_INDEX: [usize; 144] = [
    22, 21, 20, 19, 10, 1, 20, 21, 18, 19, 0, 9, 20, 19, 18, 17, 8, 14, 18, 19, 16, 17, 13, 7, 18,
    17, 16, 15, 6, 12, 16, 17, 14, 15, 11, 5, 16, 15, 14, 13, 4, 10, 14, 15, 12, 13, 9, 3, 14, 13,
    12, 11, 2, 8, 12, 13, 10, 11, 7, 1, 12, 11, 10, 9, 0, 6, 10, 11, 8, 9, 5, 14, 10, 9, 8, 7, 13,
    4, 8, 9, 6, 7, 3, 12, 8, 7, 6, 5, 11, 2, 6, 7, 4, 5, 1, 10, 6, 5, 4, 3, 9, 0, 4, 5, 2, 3, 6, 8,
    4, 3, 2, 1, 7, 5, 2, 3, 0, 1, 4, 6, 2, 1, 0, 14, 5, 3, 0, 1, 13, 22, 2, 4, 0, 22, 21, 12, 3, 1,
    21, 22, 11, 20, 0, 2,
];

const KNOWN_C7_PITCH_WIRE_BITS: [usize; 2] = [131, 143];

// Maps a "bits[]" index (as produced by this crate's usual frame_to_bits(bytes, reverse_bytes,
// lsb_first)) back to the raw-packet bit index k used by flip_bit_in_packet/parse_packet elsewhere
// in this investigation (byte k/8 within the 18-byte channel field, MSB-first: bit (7-(k%8))).
fn bits_index_to_raw_k(j: usize, reverse_bytes: bool, lsb_first: bool) -> usize {
    let byte_out = j / 8;
    let bitpos_out = j % 8;
    let source_byte = if reverse_bytes { 17 - byte_out } else { byte_out };
    let k_bit_from_msb = if lsb_first { 7 - bitpos_out } else { bitpos_out };
    source_byte * 8 + k_bit_from_msb
}

fn main() {
    println!("Searching for the framing convention that places c7's pitch-LSB bits (internal index 1 and 2) at wire positions {KNOWN_C7_PITCH_WIRE_BITS:?}\n");

    let mut any_match = false;

    // --- Hypothesis family A: TIA-102.BAAA-A Table 5-1 OTA interleave applies to the chip's raw
    // serial data. table_index[i] gives (block, index) for "wire bit i" in Table 5-1's own row
    // numbering (dibit symbol s -> row 2s=Bit1, 2s+1=Bit0). Variants: dibit swap, index direction.
    for &dibit_swap in &[false, true] {
        for &index_reversed in &[false, true] {
            // Build wire_pos_of[c7_index] using Table 5-1's OWN row numbering (0..143, transmission
            // order), before mapping into this crate's raw-packet bit numbering.
            let mut table_wire_pos_of_c7 = [usize::MAX; 7];
            for table_i in 0..TOTAL_BITS {
                let src_i = if dibit_swap { table_i ^ 1 } else { table_i };
                let block = TIA_BLOCK[src_i];
                let mut index = TIA_INDEX[src_i];
                if index_reversed {
                    index = BLOCK_SIZES[block] - 1 - index;
                }
                if block == C7_BLOCK {
                    table_wire_pos_of_c7[index] = table_i;
                }
            }
            // Table 5-1's row numbering IS the "bits[]" index under the (reverse_bytes=false,
            // lsb_first=false) hypothesis (this crate's plain MSB-first-per-byte convention) --
            // then re-map through each of the 4 byte/bit-order hypotheses to get raw-packet k.
            for &(reverse_bytes, lsb_first) in &[(false, false), (false, true), (true, false), (true, true)] {
                let k1 = bits_index_to_raw_k(table_wire_pos_of_c7[1], reverse_bytes, lsb_first);
                let k2 = bits_index_to_raw_k(table_wire_pos_of_c7[2], reverse_bytes, lsb_first);
                let mut found = [k1, k2];
                found.sort_unstable();
                let matched = found == KNOWN_C7_PITCH_WIRE_BITS;
                if matched {
                    any_match = true;
                }
                println!(
                    "[TIA table] dibit_swap={dibit_swap:<5} index_reversed={index_reversed:<5} reverse_bytes={reverse_bytes:<5} lsb_first={lsb_first:<5} -> c7[1]={k1:>3} c7[2]={k2:>3}{}",
                    if matched { "   <=== MATCH" } else { "" }
                );
            }
        }
    }

    println!();

    // --- Hypothesis family B: no OTA interleave at all -- natural contiguous codeword order
    // (c0[22..0], c1[22..0], ..., c7[6..0]), already ruled out for general Golay validity by the
    // sliding-window scan, but cheap to also check here specifically for c7's pitch bits.
    for &index_reversed in &[false, true] {
        let c7_start: usize = BLOCK_SIZES[..C7_BLOCK].iter().sum();
        let index_to_offset = |index: usize| -> usize {
            if index_reversed {
                index
            } else {
                BLOCK_SIZES[C7_BLOCK] - 1 - index
            }
        };
        let natural_pos_1 = c7_start + index_to_offset(1);
        let natural_pos_2 = c7_start + index_to_offset(2);
        for &(reverse_bytes, lsb_first) in &[(false, false), (false, true), (true, false), (true, true)] {
            let k1 = bits_index_to_raw_k(natural_pos_1, reverse_bytes, lsb_first);
            let k2 = bits_index_to_raw_k(natural_pos_2, reverse_bytes, lsb_first);
            let mut found = [k1, k2];
            found.sort_unstable();
            let matched = found == KNOWN_C7_PITCH_WIRE_BITS;
            if matched {
                any_match = true;
            }
            println!(
                "[natural order] index_reversed={index_reversed:<5} reverse_bytes={reverse_bytes:<5} lsb_first={lsb_first:<5} -> c7[1]={k1:>3} c7[2]={k2:>3}{}",
                if matched { "   <=== MATCH" } else { "" }
            );
        }
    }

    println!();
    if any_match {
        println!("At least one convention matches -- see MATCH line(s) above.");
    } else {
        println!("No convention in this search matches {KNOWN_C7_PITCH_WIRE_BITS:?}. The chip's real interleave differs from Table 5-1 under all tried variants (or c7's own bit-index-1/2 assignment differs from textbook here too) -- but {{131, 143}} remains a real, hard constraint on any future candidate table.");
    }
}
