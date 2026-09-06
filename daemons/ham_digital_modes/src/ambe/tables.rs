//! Quantizer and bit-allocation tables from TIA-102.BABA_2003.pdf's Annexes E and J. Unlike
//! `fec.rs`'s Golay/Hamming matrices, these are plain decimal numbers and small integers in the
//! spec's own text -- confirmed real, extractable PDF text (not a scanned raster) via `pdfimages
//! -list` (zero embedded images on these pages) exactly the same way `fec.rs` confirmed it for the
//! FEC matrix pages, so there is no pixel-level ambiguity risk transcribing them the way a bit
//! matrix carries. Each table below was parsed programmatically from the real `pdftotext` output
//! (not read digit-by-digit by eye) and checked against a real structural invariant before being
//! trusted -- see each table's own doc comment for what was checked.

/// The 6-bit non-uniform quantizer for the gain vector's first element (`G_hat_1`, the overall
/// level), Annex E ("Gain Quantizer Levels"). `b_hat_2` is the index of the value in this table
/// nearest to `G_hat_1` -- see TIA-102.BABA_2003.pdf section 6.3.1: "The 6 bit value `b_hat_2` is
/// defined as the index of the quantizer value... which is nearest to `G_hat_1`."
///
/// Parsed from the spec's own text and checked against a real structural invariant before being
/// trusted: a valid non-uniform quantizer's own level table must be monotonically increasing (each
/// entry strictly greater than the last) -- confirmed for all 64 entries (see
/// `gain_quantizer_levels_is_strictly_monotonically_increasing` below) -- and must have exactly one
/// entry for every index 0 through 63 with no gaps or duplicates (also confirmed during parsing,
/// before this array was ever written down).
pub const GAIN_QUANTIZER_LEVELS: [f64; 64] = [
    -2.842205, -2.694235, -2.55826, -2.38285, -2.221042, -2.095574, -1.980845, -1.836058,
    -1.645556, -1.417658, -1.261301, -1.125631, -0.958207, -0.781591, -0.555837, -0.346976,
    -0.147249, 0.027755, 0.211495, 0.38838, 0.552873, 0.737223, 0.932197, 1.139032, 1.320955,
    1.483433, 1.648297, 1.801447, 1.942731, 2.118613, 2.321486, 2.504443, 2.653909, 2.780654,
    2.925355, 3.07639, 3.220825, 3.402869, 3.585096, 3.784606, 3.955521, 4.155636, 4.314009,
    4.44415, 4.577542, 4.735552, 4.909493, 5.085264, 5.254767, 5.411894, 5.568094, 5.738523,
    5.919215, 6.087701, 6.280685, 6.464201, 6.647736, 6.834672, 7.022583, 7.211777, 7.471016,
    7.738948, 8.124863, 8.695827,
];

/// Finds the index of the [`GAIN_QUANTIZER_LEVELS`] entry nearest to `g_hat_1`, i.e. `b_hat_2` per
/// the spec's own definition (section 6.3.1).
pub fn quantize_gain_index(g_hat_1: f64) -> u8 {
    let mut best_idx = 0usize;
    let mut best_dist = f64::INFINITY;
    for (i, &level) in GAIN_QUANTIZER_LEVELS.iter().enumerate() {
        let dist = (level - g_hat_1).abs();
        if dist < best_dist {
            best_dist = dist;
            best_idx = i;
        }
    }
    best_idx as u8
}

/// Annex J ("Log Magnitude Prediction Residual Block Lengths"): for a given number of harmonics
/// `L` (9 to 56, the real range this codec operates over per the spec's own Annex tables), returns
/// the six block lengths `[J_1..J_6]` used to split the `L` prediction-residual values into six
/// blocks before the per-block DCT (`ambe::mod`'s own Fig. 17/18 pipeline documentation -- this
/// table is the concrete data that pipeline's `J_hat_i` lengths come from).
///
/// Returns `None` for `L` outside the spec's own tabulated 9..=56 range, rather than guessing or
/// extrapolating -- matching this codebase's own "give up, don't guess" discipline
/// (`AUTO_TUNE_AND_MODE_DETECTION.md`'s own SSB-deferral reasoning is the same shape of decision).
///
/// Checked, not merely transcribed: every row's six lengths must sum to exactly `L` (each of the
/// `L` prediction-residual values belongs to exactly one of the six blocks) -- confirmed for all 48
/// rows during parsing (see `block_lengths_always_sum_to_l` below), and the six lengths are always
/// non-decreasing left to right in every real spec row (low-frequency blocks are never longer than
/// higher-frequency ones) -- also confirmed for all 48 rows.
pub fn block_lengths_for_l(l: u32) -> Option<[u32; 6]> {
    if !(9..=56).contains(&l) {
        return None;
    }
    Some(BLOCK_LENGTHS[(l - 9) as usize])
}

const BLOCK_LENGTHS: [[u32; 6]; 48] = [
    [1, 1, 1, 2, 2, 2], // L=9
    [1, 1, 2, 2, 2, 2], // L=10
    [1, 2, 2, 2, 2, 2], // L=11
    [2, 2, 2, 2, 2, 2], // L=12
    [2, 2, 2, 2, 2, 3], // L=13
    [2, 2, 2, 2, 3, 3], // L=14
    [2, 2, 2, 3, 3, 3], // L=15
    [2, 2, 3, 3, 3, 3], // L=16
    [2, 3, 3, 3, 3, 3], // L=17
    [3, 3, 3, 3, 3, 3], // L=18
    [3, 3, 3, 3, 3, 4], // L=19
    [3, 3, 3, 3, 4, 4], // L=20
    [3, 3, 3, 4, 4, 4], // L=21
    [3, 3, 4, 4, 4, 4], // L=22
    [3, 4, 4, 4, 4, 4], // L=23
    [4, 4, 4, 4, 4, 4], // L=24
    [4, 4, 4, 4, 4, 5], // L=25
    [4, 4, 4, 4, 5, 5], // L=26
    [4, 4, 4, 5, 5, 5], // L=27
    [4, 4, 5, 5, 5, 5], // L=28
    [4, 5, 5, 5, 5, 5], // L=29
    [5, 5, 5, 5, 5, 5], // L=30
    [5, 5, 5, 5, 5, 6], // L=31
    [5, 5, 5, 5, 6, 6], // L=32
    [5, 5, 5, 6, 6, 6], // L=33
    [5, 5, 6, 6, 6, 6], // L=34
    [5, 6, 6, 6, 6, 6], // L=35
    [6, 6, 6, 6, 6, 6], // L=36
    [6, 6, 6, 6, 6, 7], // L=37
    [6, 6, 6, 6, 7, 7], // L=38
    [6, 6, 6, 7, 7, 7], // L=39
    [6, 6, 7, 7, 7, 7], // L=40
    [6, 7, 7, 7, 7, 7], // L=41
    [7, 7, 7, 7, 7, 7], // L=42
    [7, 7, 7, 7, 7, 8], // L=43
    [7, 7, 7, 7, 8, 8], // L=44
    [7, 7, 7, 8, 8, 8], // L=45
    [7, 7, 8, 8, 8, 8], // L=46
    [7, 8, 8, 8, 8, 8], // L=47
    [8, 8, 8, 8, 8, 8], // L=48
    [8, 8, 8, 8, 8, 9], // L=49
    [8, 8, 8, 8, 9, 9], // L=50
    [8, 8, 8, 9, 9, 9], // L=51
    [8, 8, 9, 9, 9, 9], // L=52
    [8, 9, 9, 9, 9, 9], // L=53
    [9, 9, 9, 9, 9, 9], // L=54
    [9, 9, 9, 9, 9, 10], // L=55
    [9, 9, 9, 9, 10, 10], // L=56
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gain_quantizer_levels_is_strictly_monotonically_increasing() {
        for i in 1..GAIN_QUANTIZER_LEVELS.len() {
            assert!(
                GAIN_QUANTIZER_LEVELS[i] > GAIN_QUANTIZER_LEVELS[i - 1],
                "index {}: {} is not greater than index {}: {}",
                i,
                GAIN_QUANTIZER_LEVELS[i],
                i - 1,
                GAIN_QUANTIZER_LEVELS[i - 1]
            );
        }
    }

    #[test]
    fn quantize_gain_index_finds_the_real_nearest_level() {
        assert_eq!(quantize_gain_index(-2.842205), 0);
        assert_eq!(quantize_gain_index(8.695827), 63);
        assert_eq!(quantize_gain_index(0.0), 17); // nearest to 0.027755, index 17
        // Well below the table's own lowest level: still clamps to the nearest (lowest) index,
        // matching Eq. 62's own three-case clamping behavior for the OTHER gain elements -- b_hat_2
        // itself is always "nearest," so this is the expected, correct behavior here too, not an
        // unhandled edge case.
        assert_eq!(quantize_gain_index(-100.0), 0);
        assert_eq!(quantize_gain_index(100.0), 63);
    }

    #[test]
    fn block_lengths_always_sum_to_l() {
        for l in 9..=56u32 {
            let lengths = block_lengths_for_l(l).unwrap();
            let sum: u32 = lengths.iter().sum();
            assert_eq!(sum, l, "L={l}: block lengths {lengths:?} sum to {sum}, not {l}");
        }
    }

    #[test]
    fn block_lengths_are_non_decreasing_left_to_right() {
        for l in 9..=56u32 {
            let lengths = block_lengths_for_l(l).unwrap();
            for i in 1..6 {
                assert!(
                    lengths[i] >= lengths[i - 1],
                    "L={l}: block {i} ({}) is shorter than block {} ({})",
                    lengths[i],
                    i - 1,
                    lengths[i - 1]
                );
            }
        }
    }

    #[test]
    fn block_lengths_for_l_refuses_out_of_range_values_rather_than_guessing() {
        assert_eq!(block_lengths_for_l(8), None);
        assert_eq!(block_lengths_for_l(57), None);
    }
}
