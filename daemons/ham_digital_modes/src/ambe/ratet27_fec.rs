// SPDX-License-Identifier: LGPL-3.0-or-later
//! The real DVSI chip's FEC codes for RATET(27), determined directly by sampling the live chip's
//! own encoder output rather than by black-box bit-flip relational testing -- see
//! `ratet27_wire_format`'s module doc for why relational testing (this investigation's original
//! approach) hits a hard mathematical wall: any weight-3/weight-7 codeword relationship is
//! preserved by the whole automorphism group of the abstract code (order 20160 for Hamming(15,11),
//! roughly 10^7 for Golay(23,12)), so no amount of flip-sweep data can ever pin down which specific
//! physical bit is which codeword coordinate. The fix, per `AMBE_CHIP_VALIDATION_FINDINGS.md`'s
//! own record of this pivot: capture real chip frames directly (`examples/p25_ratet27_capture_*`,
//! ~2200 distinct frames across synthetic tones, noise at 8 amplitudes, and 16 seconds of real
//! recorded speech), deinterleave via [`super::ratet27_wire_format::natural_position`], and treat
//! each block's observed natural-order bit patterns as vectors in a GF(2) linear code -- their
//! **rank** and **row-reduced basis** directly give the real generator matrix, no permutation
//! puzzle involved.
//!
//! # Results
//!
//! - **`g0`, `g1`, `g2` (Golay(23,12))**: rank exactly 12 (matching a pure, unwhitened codeword,
//!   no extra data-dependent modulation mixed into the wire bits), and the row-reduced generator
//!   basis is **bit-for-bit identical** to [`super::fec::golay_encode`]'s own systematic
//!   construction -- same data/parity split, same bit order, same `fec.rs`'s own `GOLAY_PARITY`
//!   values in every one of 36 compared rows (12 rows x 3 blocks). This chip's real Golay code for
//!   these 3 blocks needs no new implementation at all: [`super::fec::golay_encode`] and
//!   [`super::fec::golay_decode`] apply directly to each block's 23 natural-order bits.
//! - **`u4`, `u5`, `u6` (Hamming(15,11))**: rank exactly 11 each, and all three blocks share one
//!   **identical** row-reduced generator basis (one FEC routine used 3 times, as expected) -- but
//!   this basis's parity submatrix is a *different*, though equally valid, assignment of the same
//!   11 nonzero 4-bit column values `fec.rs`'s own `HAMMING_PARITY` uses, in a different data-bit
//!   order. [`HAMMING_PARITY_CHIP`] below is that real, chip-derived table, validated against 32
//!   independently-obtained empirical weight-3 codeword relationships (18 through `u4`, 7 through
//!   `u5`, 7 through `u6`, from three separate anchor-bit sweeps run earlier in this investigation)
//!   with zero mismatches -- every single one of those 32 relationships holds exactly under this
//!   generator matrix.
//! - **`c7`**: rank exactly 7 (its full width) -- confirms these 7 bits are genuinely unprotected
//!   raw data, not run through any code at all, matching the DVSI manual's own description.
//! - **`g3`**: **not yet resolved.** Despite ~2200 distinct captured frames spanning pure tones,
//!   8 different noise amplitudes, and real recorded speech, `g3`'s observed wire bits plateau at
//!   GF(2) rank 8 (not the expected 12) with 4 of its 23 natural-order bits (offsets 2-5, i.e. wire
//!   positions transformed from natural 71-74) staying exactly 0 in every single captured frame.
//!   This is a real, reproducible, stimulus-independent finding, not a sampling gap (more than 100x
//!   the frame count needed to reach full rank on every other block failed to move `g3` past rank
//!   8) -- most plausibly `g3` carries a parameter (e.g. very-high-order spectral content, or a
//!   condition tied to speech characteristics this investigation's stimuli didn't produce) that
//!   simply doesn't vary under any tested material. Resolving this needs either speech with
//!   different characteristics (non-English, sung vowels, deliberately extreme pitch) or a
//!   from-spec understanding of exactly which IMBE parameter bits land in the highest-index Golay
//!   block, to know what to specifically provoke. [`decode_block`] deliberately panics on `g3`
//!   rather than silently assuming it matches `g0`-`g2` without independent confirmation.

use super::fec::golay_decode;
use super::ratet27_wire_format::{block_wire_members, Block};

/// The real DVSI chip's Hamming(15,11) parity submatrix for RATET(27)'s `u4`/`u5`/`u6` blocks, in
/// natural-offset order (row `i` is the codeword this block's chip encoder produces when only data
/// bit `i`, 0-indexed from the block's first natural offset, is set) -- derived directly from
/// ~2200 captured real chip frames (GF(2) row-reduction of the observed codeword space), and
/// cross-validated against 32 independently-obtained empirical weight-3 relationships with zero
/// mismatches (see this module's own doc comment). A genuinely different data-bit-to-column
/// assignment from `fec.rs`'s own `HAMMING_PARITY`, though the same underlying 11 nonzero 4-bit
/// column values (confirmed by the `chip_hamming_parity_uses_the_same_15_nonzero_columns_as_fec_rs`
/// test below) -- i.e. the same abstract [15,11,3] Hamming code, differently labeled.
pub const HAMMING_PARITY_CHIP: [u8; 11] =
    [0b1001, 0b1101, 0b1111, 0b1110, 0b0111, 0b1010, 0b0101, 0b1011, 0b1100, 0b0110, 0b0011];

/// Encodes 11 data bits (low 11 bits of `data`, MSB-first -- bit 10 is natural offset 0) into this
/// chip's real 15-bit Hamming codeword, systematic (`data` in the high 11 bits, parity in the low
/// 4), using [`HAMMING_PARITY_CHIP`] rather than `fec.rs`'s own `HAMMING_PARITY`.
pub fn hamming_encode_chip(data: u16) -> u16 {
    let data = data & 0x07FF;
    let mut parity: u8 = 0;
    for (i, &row) in HAMMING_PARITY_CHIP.iter().enumerate() {
        if (data >> (10 - i)) & 1 == 1 {
            parity ^= row;
        }
    }
    (data << 4) | (parity as u16)
}

/// Minimum-distance decoding for [`hamming_encode_chip`], same brute-force technique as
/// [`super::fec::hamming_decode`].
pub fn hamming_decode_chip(received: u16) -> (u16, u32) {
    let received = received & 0x7FFF;
    let mut best_data = 0u16;
    let mut best_distance = u32::MAX;
    for data in 0u16..2048 {
        let distance = (hamming_encode_chip(data) ^ received).count_ones();
        if distance < best_distance {
            best_distance = distance;
            best_data = data;
        }
    }
    (best_data, best_distance)
}

/// Extracts a block's natural-order bits from a full 144-bit wire frame (MSB-first within each of
/// the 18 bytes, matching every `p25_ratet27_*` chip-test tool's own `BITS_OFFSET` convention) and
/// decodes it with the appropriate real chip FEC. Returns the recovered data bits and the number of
/// bit errors corrected. `g3` is deliberately not handled -- see this module's doc comment.
pub fn decode_block(wire_frame_bits: &[bool; 144], block: Block) -> (u16, u32) {
    let members = block_wire_members(block);
    let mut received: u32 = 0;
    for (offset, &wire) in members.iter().enumerate() {
        if wire_frame_bits[wire] {
            received |= 1 << (members.len() - 1 - offset);
        }
    }
    match block {
        Block::Golay { index } => {
            assert_ne!(index, 3, "g3's real generator matrix is not yet confirmed -- see module doc");
            let (data, distance) = golay_decode(received);
            (data, distance)
        }
        Block::Hamming { .. } => {
            let (data, distance) = hamming_decode_chip(received as u16);
            (data, distance)
        }
        Block::Raw => (received as u16, 0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::fec::golay_encode;
    use super::super::ratet27_wire_format::block_for_natural;

    #[test]
    fn hamming_encode_chip_of_zero_is_zero() {
        assert_eq!(hamming_encode_chip(0), 0);
    }

    #[test]
    fn hamming_chip_weight_distribution_matches_the_known_enumerator() {
        // Same real, independent check as fec.rs's own: a different labeling of a genuine
        // [15,11,3] Hamming code has the identical weight distribution as any other labeling,
        // since permuting coordinates never changes a linear code's weight distribution -- this
        // confirms HAMMING_PARITY_CHIP really does describe a valid Hamming(15,11) code and not
        // some transcription slip in extracting it from the captured frames.
        let mut counts = std::collections::BTreeMap::new();
        for data in 0u32..2048 {
            let codeword = hamming_encode_chip(data as u16);
            *counts.entry(codeword.count_ones()).or_insert(0u32) += 1;
        }
        let expected: std::collections::BTreeMap<u32, u32> = [
            (0, 1),
            (3, 35),
            (4, 105),
            (5, 168),
            (6, 280),
            (7, 435),
            (8, 435),
            (9, 280),
            (10, 168),
            (11, 105),
            (12, 35),
            (15, 1),
        ]
        .into_iter()
        .collect();
        assert_eq!(counts, expected);
    }

    #[test]
    fn chip_hamming_parity_uses_the_same_15_nonzero_columns_as_fec_rs() {
        // HAMMING_PARITY_CHIP's 11 entries, plus the 4 implicit unit-vector parity columns
        // (0b1000, 0b0100, 0b0010, 0b0001), must be all 15 nonzero 4-bit values exactly once --
        // the defining structural property of a [15,11] Hamming code's parity-check matrix,
        // independent of which specific column goes with which data bit.
        let mut all_columns: Vec<u8> = HAMMING_PARITY_CHIP.to_vec();
        all_columns.extend_from_slice(&[0b1000, 0b0100, 0b0010, 0b0001]);
        all_columns.sort_unstable();
        let expected: Vec<u8> = (1u8..=15).collect();
        assert_eq!(all_columns, expected);
    }

    /// Every empirical weight-3 codeword relationship this investigation found by direct chip
    /// bit-flip experiment (three independent anchor sweeps: wire bit 127 for `u4`, bit 9 for
    /// `u5`, bit 11 for `u6`), re-expressed as a constraint on `HAMMING_PARITY_CHIP` -- if the
    /// derived generator matrix is right, `column(a) XOR column(b) XOR column(c)` must be zero
    /// for every one of these 32 triples. This is the same validation performed offline in Python
    /// during the investigation (zero mismatches), kept here as a permanent regression test.
    fn chip_column(offset: usize) -> u8 {
        if offset < 11 {
            HAMMING_PARITY_CHIP[offset]
        } else {
            [0b1000, 0b0100, 0b0010, 0b0001][offset - 11]
        }
    }

    fn assert_weight3_relationship(wire_triple: (usize, usize, usize), block_start: usize) {
        use super::super::ratet27_wire_format::natural_position;
        let (a, b, c) = wire_triple;
        let offsets: Vec<usize> =
            [a, b, c].iter().map(|&w| natural_position(w) - block_start).collect();
        let xor = chip_column(offsets[0]) ^ chip_column(offsets[1]) ^ chip_column(offsets[2]);
        assert_eq!(xor, 0, "triple {wire_triple:?} (block start {block_start}) not a valid codeword under HAMMING_PARITY_CHIP");
    }

    #[test]
    fn all_18_confirmed_u4_weight3_triples_are_valid_codewords_under_the_chip_generator() {
        let triples = [
            (127, 8, 92),
            (127, 20, 32),
            (127, 128, 139),
            (127, 68, 103),
            (127, 44, 104),
            (127, 56, 80),
            (127, 115, 116),
            (8, 20, 115),
            (8, 32, 116),
            (8, 44, 56),
            (8, 68, 128),
            (8, 80, 104),
            (8, 103, 139),
            (92, 20, 116),
            (92, 32, 115),
            (92, 44, 80),
            (92, 56, 104),
            (92, 68, 139),
        ];
        for t in triples {
            assert_weight3_relationship(t, 92);
        }
    }

    #[test]
    fn all_7_confirmed_u5_weight3_triples_are_valid_codewords_under_the_chip_generator() {
        let triples =
            [(9, 10, 21), (9, 22, 93), (9, 33, 117), (9, 45, 57), (9, 69, 129), (9, 81, 105), (9, 140, 141)];
        for t in triples {
            assert_weight3_relationship(t, 107);
        }
    }

    #[test]
    fn all_7_confirmed_u6_weight3_triples_are_valid_codewords_under_the_chip_generator() {
        let triples =
            [(11, 23, 118), (11, 34, 94), (11, 35, 82), (11, 46, 70), (11, 47, 59), (11, 58, 130), (11, 106, 142)];
        for t in triples {
            assert_weight3_relationship(t, 122);
        }
    }

    /// Sanity check that [`decode_block`] round-trips: encoding arbitrary data through the chip's
    /// real Golay code (for `g0`, known bit-for-bit identical to `fec.rs`) and placing it on a
    /// synthetic wire frame at `g0`'s confirmed positions must decode back to the same data with
    /// zero corrected errors.
    #[test]
    fn decode_block_round_trips_g0_with_the_real_golay_code() {
        for data in [0u16, 1, 0xABC, 0xFFF] {
            let codeword = golay_encode(data & 0x0FFF);
            let members = block_wire_members(Block::Golay { index: 0 });
            let mut wire_frame_bits = [false; 144];
            for (offset, &wire) in members.iter().enumerate() {
                wire_frame_bits[wire] = (codeword >> (members.len() - 1 - offset)) & 1 == 1;
            }
            let (decoded, distance) = decode_block(&wire_frame_bits, Block::Golay { index: 0 });
            assert_eq!(distance, 0);
            assert_eq!(decoded, data & 0x0FFF);
        }
    }

    /// Same round-trip check for a Hamming block (`u4`), using the chip's own real
    /// `HAMMING_PARITY_CHIP`-based codec.
    #[test]
    fn decode_block_round_trips_u4_with_the_real_chip_hamming_code() {
        for data in [0u16, 1, 0x2AB, 0x7FF] {
            let codeword = hamming_encode_chip(data & 0x07FF);
            let members = block_wire_members(Block::Hamming { index: 0 });
            let mut wire_frame_bits = [false; 144];
            for (offset, &wire) in members.iter().enumerate() {
                wire_frame_bits[wire] = (codeword >> (members.len() - 1 - offset)) & 1 == 1;
            }
            let (decoded, distance) = decode_block(&wire_frame_bits, Block::Hamming { index: 0 });
            assert_eq!(distance, 0);
            assert_eq!(decoded, data & 0x07FF);
        }
    }

    #[test]
    #[should_panic(expected = "g3's real generator matrix is not yet confirmed")]
    fn decode_block_refuses_g3_until_its_generator_is_confirmed() {
        let wire_frame_bits = [false; 144];
        let _ = decode_block(&wire_frame_bits, Block::Golay { index: 3 });
    }

    #[test]
    fn block_for_natural_still_agrees_with_block_wire_members_round_trip() {
        // Cross-module sanity: every wire member of a block, run through block_for_natural, must
        // report that same block back.
        for (block, _) in
            [(Block::Golay { index: 0 }, 0), (Block::Hamming { index: 1 }, 0), (Block::Raw, 0)]
        {
            for wire in block_wire_members(block) {
                use super::super::ratet27_wire_format::natural_position;
                let (found_block, _offset) = block_for_natural(natural_position(wire));
                assert_eq!(found_block, block);
            }
        }
    }
}
