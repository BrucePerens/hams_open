//! Forward error correction for AMBE's [23,12] Golay and [15,11] Hamming codes, per
//! TIA-102.BABA_2003.pdf section 7.3 (Eq. 81-83) -- see `ambe/mod.rs`'s own module doc for why this
//! is the one piece of the FEC/table work judged safe to transcribe and implement now.
//!
//! # Why this one and not the others
//!
//! `ambe/mod.rs` names the real risk in transcribing spec figures by eye: a single misread bit
//! produces a generator matrix that still compiles and still *looks* plausible, with nothing to
//! catch the error. What makes these two matrices different from the quantizer/codebook tables still
//! deferred: the binary Golay [23,12] code and the Hamming [15,11] code both have a well-known,
//! independently-published weight distribution (how many codewords have each possible Hamming
//! weight) that has nothing to do with this specific document -- it's a property of the abstract code
//! itself. Transcribing the matrix from a high-resolution render (400 DPI, cropped to isolate any
//! ambiguous row) and then brute-force enumerating all codewords the transcribed generator matrix
//! actually produces gives a real, independent, spec-external check: if the transcription has any
//! bit wrong, the resulting code's weight distribution will not match the known enumerator (verified
//! directly against a hand transcription error this exact process caught: an early low-resolution
//! read of the Golay matrix produced a weight-2 codeword, which is mathematically impossible for a
//! minimum-distance-7 code, immediately proving that read wrong before it was ever committed). The
//! constants below passed this check exactly -- see the tests -- which is a meaningfully stronger
//! correctness guarantee than "I looked at it carefully."

/// [23,12] Golay code generator matrix's parity submatrix (the `P` in systematic form `g_G = [I_12 |
/// P]`), transcribed from a 400 DPI render of TIA-102.BABA_2003.pdf page 58 (self-numbered page 42).
/// Row `i` (0-indexed) is the 11-bit parity pattern XORed into the codeword when data bit `i` is set;
/// each entry's low 11 bits are meaningful, MSB-first (bit 10 down to bit 0), matching the spec's own
/// "left most bit is the MSB" convention for these row vectors.
const GOLAY_PARITY: [u16; 12] = [
    0b110_0011_1010,
    0b011_0001_1101,
    0b111_1011_0100,
    0b011_1101_1010,
    0b001_1110_1101,
    0b110_1100_1100,
    0b011_0110_0110,
    0b001_1011_0011,
    0b110_1110_0011,
    0b101_0100_1011,
    0b100_1001_1111,
    0b100_0111_0101,
];

/// [15,11] Hamming code generator matrix's parity submatrix, same transcription methodology and
/// verification as [`GOLAY_PARITY`], from page 59 (self-numbered page 43). Each entry's low 4 bits
/// are meaningful, MSB-first.
const HAMMING_PARITY: [u8; 11] = [
    0b1111, 0b1110, 0b1101, 0b1100, 0b1011, 0b1010, 0b1001, 0b0111, 0b0110, 0b0101, 0b0011,
];

/// Encodes 12 data bits (held in the low 12 bits of `data`, MSB-first per the spec's own row-vector
/// convention -- bit 11 is the first/most-significant data bit) into a 23-bit Golay codeword (systematic:
/// the same 12 data bits, followed by 11 parity bits, matching `v_i = u_i . g_G` for `g_G = [I_12 | P]`).
/// The returned value's low 23 bits are meaningful, MSB-first.
pub fn golay_encode(data: u16) -> u32 {
    let data = data & 0x0FFF;
    let mut parity: u16 = 0;
    for (i, &row) in GOLAY_PARITY.iter().enumerate() {
        // Bit (11 - i) of `data` is the i'th data bit in the spec's own MSB-first row-vector order.
        if (data >> (11 - i)) & 1 == 1 {
            parity ^= row;
        }
    }
    ((data as u32) << 11) | (parity as u32)
}

/// Encodes 11 data bits (low 11 bits of `data`, MSB-first) into a 15-bit Hamming codeword, the same
/// systematic construction as [`golay_encode`].
pub fn hamming_encode(data: u16) -> u16 {
    let data = data & 0x07FF;
    let mut parity: u8 = 0;
    for (i, &row) in HAMMING_PARITY.iter().enumerate() {
        if (data >> (10 - i)) & 1 == 1 {
            parity ^= row;
        }
    }
    (data << 4) | (parity as u16)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn popcount32(mut x: u32) -> u32 {
        let mut n = 0;
        while x != 0 {
            n += x & 1;
            x >>= 1;
        }
        n
    }

    fn popcount16(mut x: u16) -> u32 {
        let mut n = 0;
        while x != 0 {
            n += (x & 1) as u32;
            x >>= 1;
        }
        n
    }

    #[test]
    fn golay_encode_of_zero_is_zero() {
        assert_eq!(golay_encode(0), 0);
    }

    #[test]
    fn hamming_encode_of_zero_is_zero() {
        assert_eq!(hamming_encode(0), 0);
    }

    #[test]
    fn golay_weight_distribution_matches_the_known_enumerator() {
        // The real, independent correctness check this module's own doc comment describes: the
        // binary Golay [23,12] code's weight distribution is a published, spec-external fact
        // (1 + 253x^7 + 506x^8 + 1288x^11 + 1288x^12 + 506x^15 + 253x^16 + x^23), not something
        // this document or this codebase gets to define. Brute-forcing all 4096 codewords and
        // checking the resulting distribution catches a wrong transcribed bit that a purely visual
        // re-check could miss (this exact check caught a real error in an earlier, lower-resolution
        // transcription attempt before it was ever committed).
        let mut counts = std::collections::BTreeMap::new();
        for data in 0u32..4096 {
            let codeword = golay_encode(data as u16);
            *counts.entry(popcount32(codeword)).or_insert(0u32) += 1;
        }
        let expected: std::collections::BTreeMap<u32, u32> = [
            (0, 1), (7, 253), (8, 506), (11, 1288), (12, 1288), (15, 506), (16, 253), (23, 1),
        ]
        .into_iter()
        .collect();
        assert_eq!(counts, expected);
    }

    #[test]
    fn hamming_weight_distribution_matches_the_known_enumerator() {
        // Same real, independent check as the Golay test above, against the [15,11] Hamming code's
        // own published weight enumerator.
        let mut counts = std::collections::BTreeMap::new();
        for data in 0u32..2048 {
            let codeword = hamming_encode(data as u16);
            *counts.entry(popcount16(codeword)).or_insert(0u32) += 1;
        }
        let expected: std::collections::BTreeMap<u32, u32> = [
            (0, 1), (3, 35), (4, 105), (5, 168), (6, 280), (7, 435), (8, 435), (9, 280), (10, 168),
            (11, 105), (12, 35), (15, 1),
        ]
        .into_iter()
        .collect();
        assert_eq!(counts, expected);
    }

    #[test]
    fn golay_encode_preserves_the_data_bits_in_the_high_12_bits() {
        // Systematic-code sanity check, independent of the weight-distribution proof above: the
        // codeword's own high 12 bits must equal the original data exactly (that's what
        // "systematic" means), for a real, non-trivial data value.
        let data: u16 = 0b1010_1100_1101;
        let codeword = golay_encode(data);
        assert_eq!((codeword >> 11) as u16 & 0x0FFF, data);
    }

    #[test]
    fn hamming_encode_preserves_the_data_bits_in_the_high_11_bits() {
        let data: u16 = 0b101_1100_1101;
        let codeword = hamming_encode(data);
        assert_eq!((codeword >> 4) & 0x07FF, data);
    }
}
