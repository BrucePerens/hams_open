//! Bit prioritization (TIA-102.BABA_2003.pdf section 7.1, Fig. 22): rearranges the quantizer values
//! `b_hat_0, b_hat_1, ..., b_hat_{L+1}, b_hat_{L+2}` into the eight prioritized bit vectors
//! `u_hat_0..u_hat_7` that [`super::fec`]'s Golay/Hamming codes protect (`u_0..u_3` at 12 bits each,
//! `u_4..u_6` at 11 bits each, `u_7` at 7 bits -- summing to `super::VOICE_BITS`, 88).
//!
//! Transcribed from a 600 DPI render of TIA-102.BABA_2003.pdf pages 53-55, following the same
//! discipline as every other body-text equation in this spec (Type3 digit font defeats
//! `pdftotext`) -- with one extra layer of care this section specifically needed, recorded below.
//!
//! # A real, caught-in-the-act pdftotext digit corruption, not just the usual garbling
//!
//! Page 54's own body text, extracted with plain `pdftotext`, reads: "The next bits to be inserted
//! into the bit vectors are all of the bits of `b_hat_1` ..., followed by bit 2 and then bit 1 of
//! `b_hat_2`". Read at face value this contradicts the *same page's own earlier paragraph*, which
//! already stated `u_hat_0`'s middle three bits come from "the three most significant bits of
//! `b_hat_2`" -- if true, `b_hat_1` would be quantizing something never assigned any bits at all
//! anywhere in this document, and `b_hat_2`'s top 3 bits would be assigned twice. Re-rendered that
//! earlier paragraph at 600 DPI specifically to check, and it actually reads "the three most
//! significant bits of **`b_hat_2`**" (not `b_hat_1` as an initial plain-text skim might suggest) --
//! `pdftotext`'s own digit-glyph reconstruction conflated the Type3 font's control-character
//! encodings for `1` and `2` in that one spot. With that resolved, the whole section is internally
//! consistent (verified below, not just asserted): `b_hat_0` contributes 8 bits total (6 up front,
//! 2 at the very end), `b_hat_2` contributes 6 bits total (3 up front, 2 in the middle, 1 near the
//! end), `b_hat_1` contributes all `K_hat` of its own bits in one place (the middle), and the
//! `b_hat_3..b_hat_{L+1}` raster scan (Fig. 22) contributes the rest -- summing to exactly
//! `VOICE_BITS` (88) for every real `(L, K_hat)` pair the codec produces, confirmed against the
//! actual worked example in Fig. 22 (`L_hat = 16`, `K_hat = 6`) bit for bit before writing any of the
//! code below, not merely by re-deriving the total count.
//!
//! # The raster scan itself (Fig. 22)
//!
//! `b_hat_3` through `b_hat_{L+1}` are drawn as columns of varying height (each column's height is
//! that value's own bit allocation, from Annex F for `b_hat_3..b_hat_7` and Annex G for
//! `b_hat_8..b_hat_{L+1}`), and are scanned in one continuous top-to-bottom, left-to-right order:
//! within each bit-significance level (starting from whatever the tallest column's own MSB is, down
//! to every column's shared LSB level 0), visit each column left to right and emit that column's own
//! bit at the current level if the column is tall enough to have one. A column with a `0`-bit
//! allocation (a real value in Annex G, see `super::tables`' own doc comment) contributes no cells at
//! all, which this scan handles for free -- it's simply never "tall enough" at any level.
//!
//! The resulting flat sequence of scanned bits is inserted continuously across two separate
//! destination spans (the first 39 bits fill the last 3 bits of `u_hat_0` plus all of `u_hat_1`
//! through `u_hat_3`; the remainder fills alongside `b_hat_1`/`b_hat_2` into `u_hat_4` through the
//! top of `u_hat_7`) -- but from the *source* side there is no seam: it's one uninterrupted scan,
//! which is why [`raster_scan_bits`] takes no notion of where the two spans divide.

/// Runs Fig. 22's own raster scan over `columns` (`(value, bit_width)` pairs for `b_hat_3` through
/// `b_hat_{L+1}`, in that order -- gain-vector columns from Annex F, then higher-order-coefficient
/// columns from Annex G, omitting any real `0`-bit Annex G entries as those contribute nothing).
/// Returns the flat scanned bit sequence, MSB-of-tallest-column first.
pub fn raster_scan_bits(columns: &[(u32, u8)]) -> Vec<bool> {
    let max_width = columns.iter().map(|&(_, w)| w).max().unwrap_or(0);
    let mut bits = Vec::new();
    for level in (0..max_width).rev() {
        for &(value, width) in columns {
            if width > level {
                bits.push((value >> level) & 1 == 1);
            }
        }
    }
    bits
}

/// Runs the full bit prioritization (Fig. 22) for one frame, producing the eight prioritized bit
/// vectors `u_hat_0..u_hat_7` (packed as plain integers, MSB-first within each). Returns `None` if
/// the inputs don't add up to exactly `VOICE_BITS` (88) -- a real internal-consistency precondition
/// (the spec's own Annex F/G bit allocations are designed so this always holds for a genuine
/// `(L_hat, K_hat)` pair), not a spec-defined error case, so callers passing mismatched data get a
/// clear `None` instead of a silently wrong or panicking result.
///
/// - `b0`: the fundamental frequency quantizer value (8 bits, [`super::parameter_encoding::quantize_fundamental_frequency`]).
/// - `b1`, `k_hat`: the V/UV decision bits and their real bit width ([`super::parameter_encoding::encode_voicing_decisions`]).
/// - `b2`: the 6-bit gain index ([`super::tables::quantize_gain_index`]).
/// - `gain_vector`: `(value, bits)` for `b_hat_3..b_hat_7`, in order (from [`super::quantize::quantize_gain_vector_element`] and [`super::tables::gain_bit_allocation`]).
/// - `higher_order`: `(value, bits)` for `b_hat_8..b_hat_{L+1}`, `bits > 0` only (from [`super::quantize::quantize_higher_order_coefficients`] and [`super::tables::higher_order_bit_allocation`], both already filtering the same way).
/// - `sync_bit`: `b_hat_{L+2}`, the frame-to-frame alternating synchronization value ("Synchronization
///   Encoding and Decoding") -- not yet its own module, so for now this is the caller's own tracked
///   alternating-bit state.
pub fn prioritize_bits(
    b0: u32,
    b1: u32,
    k_hat: u32,
    b2: u32,
    gain_vector: [(u32, u8); 5],
    higher_order: &[(u32, u8)],
    sync_bit: bool,
) -> Option<[u32; 8]> {
    let mut bits: Vec<bool> = Vec::with_capacity(88);

    // Step 1: b0's own top 6 bits (7..2), dropping its bottom 2 (used at the very end).
    for i in (2..8).rev() {
        bits.push((b0 >> i) & 1 == 1);
    }
    // Step 2: b2's own top 3 bits (5..3).
    for i in (3..6).rev() {
        bits.push((b2 >> i) & 1 == 1);
    }

    let mut columns: Vec<(u32, u8)> = gain_vector.to_vec();
    columns.extend_from_slice(higher_order);
    let scan = raster_scan_bits(&columns);
    let first_scan_len = scan.len().min(39);

    // Step 3: the scan's own first 39 bits.
    bits.extend_from_slice(&scan[..first_scan_len]);
    // Step 4: all of b1's own k_hat bits, MSB first.
    for i in (0..k_hat).rev() {
        bits.push((b1 >> i) & 1 == 1);
    }
    // Step 5: b2's own bits 2 and 1.
    for i in (1..3).rev() {
        bits.push((b2 >> i) & 1 == 1);
    }
    // Step 6: the rest of the scan.
    bits.extend_from_slice(&scan[first_scan_len..]);
    // Step 7: b2's own bit 0.
    bits.push(b2 & 1 == 1);
    // Step 8: b0's own bits 1 and 0.
    for i in (0..2).rev() {
        bits.push((b0 >> i) & 1 == 1);
    }
    // Step 9: the sync bit.
    bits.push(sync_bit);

    if bits.len() != 88 {
        return None;
    }

    const LENGTHS: [usize; 8] = [12, 12, 12, 12, 11, 11, 11, 7];
    let mut u = [0u32; 8];
    let mut idx = 0;
    for (slot, &len) in u.iter_mut().zip(LENGTHS.iter()) {
        let mut value = 0u32;
        for _ in 0..len {
            value = (value << 1) | (bits[idx] as u32);
            idx += 1;
        }
        *slot = value;
    }
    Some(u)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ambe::tables;

    /// The real Annex F/G bit widths for the spec's own worked example, `L_hat = 16`, `K_hat = 6`
    /// (Table 6 and the higher-order bit allocation table, both already transcribed and tested in
    /// `super::tables`) -- used throughout this module's own tests so every check runs against the
    /// same ground truth Fig. 22 itself was read against.
    fn l16_widths() -> ([u8; 5], Vec<u8>) {
        let gain: [u8; 5] =
            std::array::from_fn(|i| tables::gain_bit_allocation(16, i as u32 + 2).unwrap().0);
        let higher = tables::higher_order_bit_allocation(16).unwrap().to_vec();
        (gain, higher)
    }

    #[test]
    fn l16_widths_match_fig_22s_own_column_heights() {
        let (gain, higher) = l16_widths();
        assert_eq!(gain, [6, 6, 6, 5, 5]);
        assert_eq!(higher, vec![6, 6, 5, 4, 4, 3, 3, 3, 3, 2]);
    }

    #[test]
    fn raster_scan_bits_produces_exactly_67_bits_for_the_l16_example() {
        let (gain, higher) = l16_widths();
        let columns: Vec<(u32, u8)> = gain
            .iter()
            .map(|&w| (0u32, w))
            .chain(higher.iter().map(|&w| (0u32, w)))
            .collect();
        let scan = raster_scan_bits(&columns);
        // Hand-summed directly from Fig. 22's own column heights: 6+6+6+5+5+6+6+5+4+4+3+3+3+3+2 = 67.
        assert_eq!(scan.len(), 67);
    }

    fn prioritize_zeroed(gain_vector: [(u32, u8); 5], higher_order: &[(u32, u8)]) -> [u32; 8] {
        prioritize_bits(0, 0, 6, 0, gain_vector, higher_order, false).unwrap()
    }

    /// Fig. 22's own first-scanned cell (the tallest column, `b_hat_3`, at its own MSB) is labeled
    /// `u_hat_{0,2}` directly in the figure -- setting only that one source bit and checking that
    /// only `u_hat_0`'s own bit 2 comes back set is a direct, ground-truth check against the figure
    /// itself, not a re-derivation of this module's own logic.
    #[test]
    fn b3s_own_msb_lands_on_u0_bit_2_per_fig_22() {
        let (gain, higher) = l16_widths();
        let mut gain_vector: [(u32, u8); 5] = std::array::from_fn(|i| (0, gain[i]));
        gain_vector[0] = (1 << (gain[0] - 1), gain[0]); // b_hat_3's own MSB set
        let higher_order: Vec<(u32, u8)> = higher.iter().map(|&w| (0u32, w)).collect();

        let u = prioritize_bits(0, 0, 6, 0, gain_vector, &higher_order, false).unwrap();
        assert_eq!(
            u[0],
            1 << 2,
            "expected only u0's bit 2 set, got u0={:#014b}",
            u[0]
        );
        for (i, &v) in u.iter().enumerate().skip(1) {
            assert_eq!(
                v, 0,
                "expected every other u-vector to stay zero, got u{i}={v:#x}"
            );
        }
    }

    /// Fig. 22's own last-scanned cell (`b_hat_17 = b_hat_{L+1}`'s own LSB, the last column visited
    /// at the scan's own final, lowest bit-significance level) is labeled `u_hat_{7,4}` directly in
    /// the figure -- note this is `b_hat_17`'s LSB, not its MSB (which lands earlier in the scan, on
    /// `u_hat_5`'s own bit 1, since `b_hat_17` is visited once per bit-significance level it's tall
    /// enough for, and the scan visits levels from high to low).
    #[test]
    fn b17s_own_lsb_lands_on_u7_bit_4_per_fig_22() {
        let (gain, higher) = l16_widths();
        let gain_vector: [(u32, u8); 5] = std::array::from_fn(|i| (0, gain[i]));
        let mut higher_order: Vec<(u32, u8)> = higher.iter().map(|&w| (0u32, w)).collect();
        let last = higher_order.len() - 1;
        let last_width = higher_order[last].1;
        higher_order[last] = (1, last_width); // b_hat_17's own LSB set

        let u = prioritize_bits(0, 0, 6, 0, gain_vector, &higher_order, false).unwrap();
        assert_eq!(
            u[7],
            1 << 4,
            "expected only u7's bit 4 set, got u7={:#09b}",
            u[7]
        );
        for (i, &v) in u.iter().enumerate().take(7) {
            assert_eq!(
                v, 0,
                "expected every other u-vector to stay zero, got u{i}={v:#x}"
            );
        }
    }

    #[test]
    fn b0s_own_msb_lands_on_u0_bit_11() {
        let (gain, higher) = l16_widths();
        let gain_vector: [(u32, u8); 5] = std::array::from_fn(|i| (0, gain[i]));
        let higher_order: Vec<(u32, u8)> = higher.iter().map(|&w| (0u32, w)).collect();
        let u = prioritize_bits(0b1000_0000, 0, 6, 0, gain_vector, &higher_order, false).unwrap();
        assert_eq!(u[0], 1 << 11);
    }

    #[test]
    fn b0s_own_lsb_lands_on_u7_bit_1() {
        let (gain, higher) = l16_widths();
        let gain_vector: [(u32, u8); 5] = std::array::from_fn(|i| (0, gain[i]));
        let higher_order: Vec<(u32, u8)> = higher.iter().map(|&w| (0u32, w)).collect();
        let u = prioritize_bits(0b0000_0001, 0, 6, 0, gain_vector, &higher_order, false).unwrap();
        assert_eq!(u[7], 1 << 1);
    }

    #[test]
    fn b2s_own_lsb_lands_on_u7_bit_3() {
        let (gain, higher) = l16_widths();
        let gain_vector: [(u32, u8); 5] = std::array::from_fn(|i| (0, gain[i]));
        let higher_order: Vec<(u32, u8)> = higher.iter().map(|&w| (0u32, w)).collect();
        let u = prioritize_bits(0, 0, 6, 0b0000_01, gain_vector, &higher_order, false).unwrap();
        assert_eq!(u[7], 1 << 3);
    }

    #[test]
    fn the_sync_bit_lands_on_u7_bit_0() {
        let (gain, higher) = l16_widths();
        let gain_vector: [(u32, u8); 5] = std::array::from_fn(|i| (0, gain[i]));
        let higher_order: Vec<(u32, u8)> = higher.iter().map(|&w| (0u32, w)).collect();
        let u = prioritize_bits(0, 0, 6, 0, gain_vector, &higher_order, true).unwrap();
        assert_eq!(u[7], 1);
    }

    /// `b_hat_1`'s own MSB is the very first bit inserted into the second combined segment, which
    /// the spec's own text says "begins with bit 10 of u_hat_4" -- a direct check of that claim.
    #[test]
    fn b1s_own_msb_lands_on_u4_bit_10() {
        let (gain, higher) = l16_widths();
        let gain_vector: [(u32, u8); 5] = std::array::from_fn(|i| (0, gain[i]));
        let higher_order: Vec<(u32, u8)> = higher.iter().map(|&w| (0u32, w)).collect();
        let u = prioritize_bits(0, 0b10_0000, 6, 0, gain_vector, &higher_order, false).unwrap();
        assert_eq!(u[4], 1 << 10);
    }

    #[test]
    fn prioritize_bits_always_produces_exactly_88_bits_across_all_eight_vectors() {
        let (gain, higher) = l16_widths();
        let gain_vector: [(u32, u8); 5] = std::array::from_fn(|i| (0, gain[i]));
        let higher_order: Vec<(u32, u8)> = higher.iter().map(|&w| (0u32, w)).collect();
        let u = prioritize_zeroed(gain_vector, higher_order.as_slice());
        let lengths = [12u32, 12, 12, 12, 11, 11, 11, 7];
        assert_eq!(lengths.iter().sum::<u32>(), 88);
        for (&value, &len) in u.iter().zip(lengths.iter()) {
            assert!(value < (1 << len));
        }
    }

    #[test]
    fn prioritize_bits_refuses_a_mismatched_total_bit_count() {
        // k_hat = 3 (instead of the real 6 matching l16_widths' own bit allocation) leaves the
        // flat sequence 3 bits short of 88 -- a genuine internal-consistency mismatch, not a real
        // (L_hat, K_hat) pair this codec would ever actually produce together.
        let (gain, higher) = l16_widths();
        let gain_vector: [(u32, u8); 5] = std::array::from_fn(|i| (0, gain[i]));
        let higher_order: Vec<(u32, u8)> = higher.iter().map(|&w| (0u32, w)).collect();
        assert_eq!(
            prioritize_bits(0, 0, 3, 0, gain_vector, &higher_order, false),
            None
        );
    }
}
