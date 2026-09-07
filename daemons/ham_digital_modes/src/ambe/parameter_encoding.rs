//! Fundamental frequency and voiced/unvoiced decision encoding (TIA-102.BABA_2003.pdf sections
//! 6.1-6.2, Eq. 45 and 49) -- the two model parameters that get their own dedicated quantizer values
//! `b_hat_0` and `b_hat_1`, ahead of the gain vector (`b_hat_2..b_hat_7`, [`super::tables`]/
//! [`super::quantize`]) and higher-order DCT coefficients (`b_hat_8..b_hat_{L+1}`, same modules).
//!
//! Transcribed from a 600 DPI render of TIA-102.BABA_2003.pdf pages 38-40, following the same
//! discipline as every other body-text equation in this spec (Type3 digit font defeats
//! `pdftotext`).

use std::f64::consts::PI;

/// `b_hat_0`'s own fixed bit width (section 6.1's own stated "the quantizer value `b_hat_0` is
/// represented with 8 bits", Table 2) -- unlike every other quantizer value in this codec, this one
/// doesn't depend on `L_hat`.
pub const FUNDAMENTAL_FREQUENCY_BITS: u32 = 8;

/// The fundamental frequency quantizer value `b_hat_0` (Eq. 45): `omega0_hat` (already estimated to
/// quarter-sample resolution by [`super::pitch_refinement::refine_pitch`]) is encoded at half-sample
/// resolution instead, since only 8 bits are budgeted for it. The spec's own stated valid range is
/// `0 <= b_hat_0 <= 207` (leaving 48 reserved/unused values in the 8-bit range for future use) --
/// checked by the test below against the pitch range this codec actually estimates over
/// ([`super::pitch::candidate_pitches`]), not merely asserted.
pub fn quantize_fundamental_frequency(omega0_hat: f64) -> u32 {
    ((4.0 * PI / omega0_hat) - 39.0).floor() as u32
}

/// `b_hat_1` (Eq. 49): packs the `K_hat` per-band voiced/unvoiced decisions (from
/// [`super::vuv::determine_voicing`]) into a single unsigned integer, MSB-first (`v_hat_1` is the
/// most significant of the `K_hat` bits used to represent this value, `v_hat_{K_hat}` the least).
pub fn encode_voicing_decisions(voiced: &[bool]) -> u32 {
    let k_hat = voiced.len() as u32;
    voiced
        .iter()
        .enumerate()
        .map(|(idx, &v)| {
            let k = idx as u32 + 1; // 1-indexed k, matching the spec's own v_hat_k
            if v {
                1u32 << (k_hat - k)
            } else {
                0
            }
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ambe::pitch::candidate_pitches;

    #[test]
    fn quantize_fundamental_frequency_matches_eq45_at_a_hand_computed_value() {
        // omega0_hat = 4*pi/(207 + 39) = 4*pi/246, chosen so Eq. 45 lands exactly on the
        // spec's own stated upper bound b_hat_0 = 207 without any floor-boundary ambiguity.
        let omega0_hat = 4.0 * PI / 246.0;
        assert_eq!(quantize_fundamental_frequency(omega0_hat), 207);
    }

    /// Every real quarter-sample period this codec's pitch estimator can actually hand to
    /// [`quantize_fundamental_frequency`] -- not [`candidate_pitches`]'s own half-sample set
    /// directly, since [`super::pitch_refinement::refine_pitch`] (the real caller, one stage later)
    /// perturbs each candidate by one of ten quarter-sample offsets in `-9/8..=9/8`. That widens the
    /// true domain to `19.875..=123.125`, which is exactly the spec's own stated `omega0_hat`
    /// interval boundary (`2*19.875 - 39 = 0.75` floors to `0`; `2*123.125 - 39 = 207.25` floors to
    /// `207`) -- deriving the test domain this way turns the spec's own quoted `0..=207` bound into
    /// a checked consequence of the real pitch range, not a separately re-asserted fact.
    fn real_refined_pitch_range() -> impl Iterator<Item = f64> {
        let offsets = [
            -9.0 / 8.0,
            -7.0 / 8.0,
            -5.0 / 8.0,
            -3.0 / 8.0,
            -1.0 / 8.0,
            1.0 / 8.0,
            3.0 / 8.0,
            5.0 / 8.0,
            7.0 / 8.0,
            9.0 / 8.0,
        ];
        candidate_pitches().flat_map(move |p| offsets.into_iter().map(move |offset| p + offset))
    }

    #[test]
    fn quantize_fundamental_frequency_stays_within_the_specs_own_0_to_207_range_across_the_real_pitch_range(
    ) {
        // The spec's own stated valid range (section 6.1: "the value of b_hat_0 ... is limited
        // to the range 0 <= b_hat_0 <= 207"), checked against every omega0_hat this codec's own
        // pitch estimator can actually produce end to end (candidate_pitches, further refined by
        // refine_pitch's own quarter-sample offsets -- see real_refined_pitch_range above) rather
        // than merely asserted from the spec text.
        for p in real_refined_pitch_range() {
            let omega0_hat = 2.0 * PI / p;
            let b0 = quantize_fundamental_frequency(omega0_hat);
            assert!(
                b0 <= 207,
                "P={p}: expected b_hat_0 <= 207 per the spec's own stated range, got {b0}"
            );
        }
    }

    #[test]
    fn quantize_fundamental_frequency_fits_in_8_bits() {
        for p in real_refined_pitch_range() {
            let omega0_hat = 2.0 * PI / p;
            let b0 = quantize_fundamental_frequency(omega0_hat);
            assert!(b0 < (1u32 << FUNDAMENTAL_FREQUENCY_BITS));
        }
    }

    #[test]
    fn encode_voicing_decisions_matches_eq49_at_a_hand_computed_value() {
        // K_hat=4, v = [1,0,1,1] -> b_hat_1 = 1*2^3 + 0*2^2 + 1*2^1 + 1*2^0 = 8+0+2+1 = 11.
        let voiced = [true, false, true, true];
        assert_eq!(encode_voicing_decisions(&voiced), 11);
    }

    #[test]
    fn encode_voicing_decisions_of_all_voiced_is_the_all_ones_bit_pattern() {
        for k_hat in 1u32..=12 {
            let voiced = vec![true; k_hat as usize];
            let expected = (1u32 << k_hat) - 1;
            assert_eq!(encode_voicing_decisions(&voiced), expected);
        }
    }

    #[test]
    fn encode_voicing_decisions_of_all_unvoiced_is_zero() {
        let voiced = vec![false; 7];
        assert_eq!(encode_voicing_decisions(&voiced), 0);
    }
}
