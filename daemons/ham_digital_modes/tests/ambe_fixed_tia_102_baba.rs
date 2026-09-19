// SPDX-License-Identifier: LGPL-3.0-or-later
//! Cross-checks `ambe::fixed::tia_102_baba::parameter_encoding` against its floating-point sibling --
//! kept as an integration test (outside `src/ambe/fixed`) for the same reason as
//! `tests/ambe_fixed_general.rs`: no `f64` token may appear inside `src/ambe/fixed` itself.

use ham_digital_modes::ambe::fixed::tia_102_baba::parameter_encoding::{
    dequantize_fundamental_frequency_q16, harmonics_count_from_b0,
};
use ham_digital_modes::ambe::float::tia_102_baba::parameter_encoding::dequantize_fundamental_frequency;
use ham_digital_modes::ambe::float::tia_102_baba::vuv::harmonics_count;

const B0_MAX: u32 = 207; // The spec's own stated valid range, TIA-102.BABA_2003.pdf section 6.1.

/// Every one of the 208 real `b_hat_0` values -- an exact check, not a sampled sweep, since the
/// fixed-point side is itself an exact table over exactly this domain (see `b0_table`'s own doc
/// comment for why that's a valid design, not a shortcut).
#[test]
fn dequantize_fundamental_frequency_q16_matches_the_float_sibling_exactly_for_every_b0() {
    for b0 in 0..=B0_MAX {
        let expected = (dequantize_fundamental_frequency(b0) * 65536.0).round() as i32;
        let actual = dequantize_fundamental_frequency_q16(b0);
        assert_eq!(actual, expected, "b0={b0}");
    }
}

#[test]
fn harmonics_count_from_b0_matches_the_float_sibling_exactly_for_every_b0() {
    for b0 in 0..=B0_MAX {
        let omega0_tilde = dequantize_fundamental_frequency(b0);
        let expected = harmonics_count(omega0_tilde);
        let actual = harmonics_count_from_b0(b0);
        assert_eq!(actual, expected, "b0={b0}: omega0_tilde={omega0_tilde}");
    }
}

/// `L~` must stay within the spec's own documented range across the full real `b_hat_0` domain --
/// the same sanity property `ambe::float::ratet27`'s own tests hold the floating-point side to.
#[test]
fn harmonics_count_from_b0_stays_within_the_specs_own_9_to_56_range() {
    for b0 in 0..=B0_MAX {
        let l_hat = harmonics_count_from_b0(b0);
        assert!((9..=56).contains(&l_hat), "b0={b0}: L~={l_hat} out of range 9..=56");
    }
}

/// A corrupted frame's `b_hat_0` could in principle carry any 8-bit value up to 255, past the
/// spec's own stated `0..=207` bound -- both functions must clamp rather than panic or read out of
/// bounds.
#[test]
fn both_functions_never_panic_across_the_full_8_bit_range() {
    for b0 in 0..=255u32 {
        let _ = dequantize_fundamental_frequency_q16(b0);
        let _ = harmonics_count_from_b0(b0);
    }
}
