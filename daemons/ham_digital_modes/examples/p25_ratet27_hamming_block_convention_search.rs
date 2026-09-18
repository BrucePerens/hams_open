// SPDX-License-Identifier: LGPL-3.0-or-later
//! Zero-chip-time convention search using a much stronger constraint than
//! `p25_ratet27_c7_pitch_bit_convention_search.rs`'s 2-bit check.
//!
//! Per advisor review: the two confirmed redundancy groups {8, 92, 127} and {68, 103, 127} found
//! by `p25_ratet27_pairflip_diagnose_hit.rs` are NOT a 3-way majority vote (IMBE has no repetition
//! code). They are weight-3 codewords of a Hamming(15,11) block: Hamming distance is only 3, so a
//! 2-bit error is always "corrected" onto the third bit of some weight-3 codeword containing it.
//! For columns h_a, h_b, h_c of the parity-check matrix H with h_a XOR h_b XOR h_c = 0 (a weight-3
//! codeword), flipping any 2-of-3 produces a syndrome equal to the third column, so the decoder
//! "corrects" the one not flipped -- explaining the observed byte-identical PCM across all
//! pairwise/triple flip combinations, with each single flip alone showing no effect. This requires
//! bits 8, 68, 92, 103, and 127 to all belong to the SAME 15-bit Hamming block (since 127 appears
//! in both triples, and a wire bit belongs to exactly one FEC block). The disqualified full sweep
//! separately (unverified) suggested (32, 127) as a further pair -- included here as a secondary
//! candidate, not a confirmed fact.
//!
//! This tool searches the same convention space as the c7 tool (TIA-102.BAAA-A Table 5-1 interleave
//! variants, plus natural contiguous order) for any convention that places {8, 68, 92, 103, 127}
//! (and separately, {8, 32, 68, 92, 103, 127}) all in the same Hamming(15,11) block (TIA_BLOCK index
//! 4, 5, or 6). Six (or five) wire positions landing in the same 15-bit block is a far more
//! discriminating constraint than the c7 tool's 2-bit check, which found no match at all.
const BLOCK_SIZES: [usize; 8] = [23, 23, 23, 23, 15, 15, 15, 7];

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

const CONFIRMED_5: [usize; 5] = [8, 68, 92, 103, 127];
const CONFIRMED_PLUS_UNVERIFIED_32: [usize; 6] = [8, 32, 68, 92, 103, 127];

// Inverse of bits_index_to_raw_k from the c7 convention-search tool: given a raw-packet bit index k
// (this crate's usual channel-field bit numbering) and a hypothesized (reverse_bytes, lsb_first)
// byte/bit-order convention, recovers the "bits[]" index j (== Table 5-1 row number under this
// tool's model, i.e. table_i).
fn raw_k_to_bits_index(k: usize, reverse_bytes: bool, lsb_first: bool) -> usize {
    let source_byte = k / 8;
    let k_bit_from_msb = k % 8;
    let byte_out = if reverse_bytes { 17 - source_byte } else { source_byte };
    let bitpos_out = if lsb_first { 7 - k_bit_from_msb } else { k_bit_from_msb };
    byte_out * 8 + bitpos_out
}

fn block_and_index_table(table_i: usize, dibit_swap: bool, index_reversed: bool) -> (usize, usize) {
    let src_i = if dibit_swap { table_i ^ 1 } else { table_i };
    let block = TIA_BLOCK[src_i];
    let mut index = TIA_INDEX[src_i];
    if index_reversed {
        index = BLOCK_SIZES[block] - 1 - index;
    }
    (block, index)
}

fn block_and_index_natural(j: usize, index_reversed: bool) -> (usize, usize) {
    let mut acc = 0;
    for (block, &size) in BLOCK_SIZES.iter().enumerate() {
        if j < acc + size {
            let offset = j - acc;
            let index = if index_reversed { offset } else { size - 1 - offset };
            return (block, index);
        }
        acc += size;
    }
    unreachable!("j={j} out of range");
}

fn check_all_same_hamming_block(positions: &[usize], mapper: impl Fn(usize) -> (usize, usize)) -> Option<(usize, Vec<usize>)> {
    let mapped: Vec<(usize, usize)> = positions.iter().map(|&k| mapper(k)).collect();
    let block0 = mapped[0].0;
    if (4..=6).contains(&block0) && mapped.iter().all(|&(b, _)| b == block0) {
        Some((block0, mapped.iter().map(|&(_, i)| i).collect()))
    } else {
        None
    }
}

fn main() {
    println!("Searching for a framing convention placing {CONFIRMED_5:?} all in one Hamming(15,11) block\n");
    let mut any_match = false;

    for &dibit_swap in &[false, true] {
        for &index_reversed in &[false, true] {
            for &(reverse_bytes, lsb_first) in &[(false, false), (false, true), (true, false), (true, true)] {
                let mapper = |k: usize| -> (usize, usize) {
                    let j = raw_k_to_bits_index(k, reverse_bytes, lsb_first);
                    block_and_index_table(j, dibit_swap, index_reversed)
                };
                if let Some((block, indices)) = check_all_same_hamming_block(&CONFIRMED_5, mapper) {
                    any_match = true;
                    println!(
                        "[TIA table] dibit_swap={dibit_swap:<5} index_reversed={index_reversed:<5} reverse_bytes={reverse_bytes:<5} lsb_first={lsb_first:<5} -> ALL 5 IN BLOCK {block}, indices={indices:?}   <=== MATCH"
                    );
                    let with32 = mapper(32);
                    println!("    bit 32 (unverified sweep hint) -> block={}, index={}{}", with32.0, with32.1, if with32.0 == block { "  (also in this block!)" } else { "" });
                }
            }
        }
    }

    for &index_reversed in &[false, true] {
        for &(reverse_bytes, lsb_first) in &[(false, false), (false, true), (true, false), (true, true)] {
            let mapper = |k: usize| -> (usize, usize) {
                let j = raw_k_to_bits_index(k, reverse_bytes, lsb_first);
                block_and_index_natural(j, index_reversed)
            };
            if let Some((block, indices)) = check_all_same_hamming_block(&CONFIRMED_5, mapper) {
                any_match = true;
                println!(
                    "[natural order] index_reversed={index_reversed:<5} reverse_bytes={reverse_bytes:<5} lsb_first={lsb_first:<5} -> ALL 5 IN BLOCK {block}, indices={indices:?}   <=== MATCH"
                );
                let with32 = mapper(32);
                println!("    bit 32 (unverified sweep hint) -> block={}, index={}{}", with32.0, with32.1, if with32.0 == block { "  (also in this block!)" } else { "" });
            }
        }
    }

    println!();
    if !any_match {
        println!("No convention in this search places all 5 confirmed positions {CONFIRMED_5:?} in one Hamming block.");
        println!("Falling back to per-position block/index dump across all conventions, for manual inspection:\n");
        for &dibit_swap in &[false, true] {
            for &index_reversed in &[false, true] {
                for &(reverse_bytes, lsb_first) in &[(false, false), (false, true), (true, false), (true, true)] {
                    let mapper = |k: usize| -> (usize, usize) {
                        let j = raw_k_to_bits_index(k, reverse_bytes, lsb_first);
                        block_and_index_table(j, dibit_swap, index_reversed)
                    };
                    let mapped: Vec<(usize, usize)> = CONFIRMED_PLUS_UNVERIFIED_32.iter().map(|&k| mapper(k)).collect();
                    println!(
                        "[TIA table] swap={dibit_swap:<5} idxrev={index_reversed:<5} revb={reverse_bytes:<5} lsb={lsb_first:<5} -> {:?}",
                        CONFIRMED_PLUS_UNVERIFIED_32.iter().zip(mapped.iter()).map(|(&k, &(b, i))| format!("{k}->({b},{i})")).collect::<Vec<_>>()
                    );
                }
            }
        }
    } else {
        println!("At least one convention places all 5 confirmed positions in a single Hamming block -- see MATCH line(s) above.");
    }
}
