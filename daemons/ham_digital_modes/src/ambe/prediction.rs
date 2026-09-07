//! Spectral amplitude prediction residual (TIA-102.BABA_2003.pdf section 6.2, Eq. 52-57) -- the
//! differential coding step that transmits how much the current frame's spectral envelope changed
//! from the previous frame, in the log2 domain, rather than transmitting the envelope itself.
//!
//! Transcribed from a 600 DPI render of TIA-102.BABA_2003.pdf pages 42, following the same
//! discipline as every other body-text equation in this spec (Type3 digit font defeats `pdftotext`).
//!
//! # A genuine, disclosed scope boundary: this module does not decode
//!
//! Eq. 54's own bias-corrected prediction needs `M_tilde_j(-1)`: the previous frame's *quantized and
//! reconstructed* spectral amplitudes, not this encoder's own unquantized estimate of them (that's
//! the whole point of a closed-loop predictive coder -- the encoder must predict from what the
//! decoder will actually have, which means simulating the decoder). Building that reconstruction path
//! (dequantizing the gain vector and higher-order DCT coefficients, inverse DCT, Eq. 45's log2
//! addition, per Fig. 16's own "Reconstruct" feedback block) is real, separate, not-yet-built work.
//! [`prediction_residual`] below takes `previous_m` as a plain input parameter instead of computing
//! it internally -- callers must supply real reconstructed history (or the spec's own literal
//! initialization values, [`INITIAL_L_HAT_PREV`] and an all-`1.0` amplitude array, for the very first
//! frame of a stream) rather than this module quietly assuming zero history.

/// The number of harmonics assumed for the previous frame before any real previous frame exists
/// (the spec's own stated initialization value, not zero): "upon initialization ... `L_hat(-1) = 30`".
pub const INITIAL_L_HAT_PREV: u32 = 30;

/// The prediction coefficient `rho` (Eq. 55): how strongly the current frame's spectral amplitudes
/// are predicted from the previous frame's, as a function of how many harmonics the current frame
/// has. A sparser harmonic set (fewer, lower-frequency harmonics, small `l_hat_curr`) gets weaker
/// prediction; a fuller one gets strong prediction, on the presumption that harmonic-rich frames
/// change more smoothly frame to frame.
pub fn prediction_coefficient(l_hat_curr: u32) -> f64 {
    if l_hat_curr <= 15 {
        0.4
    } else if l_hat_curr <= 24 {
        0.03 * l_hat_curr as f64 - 0.05
    } else {
        0.7
    }
}

/// `k_hat_l` (Eq. 52): harmonic `l`'s own fractional position in the *previous* frame's harmonic
/// index space, found by scaling `l` by the ratio of the two frames' own harmonic counts (since a
/// pitch change between frames means harmonic `l` in this frame doesn't line up with harmonic `l` in
/// the previous one).
fn harmonic_index_ratio(l: u32, l_hat_prev: u32, l_hat_curr: u32) -> f64 {
    (l_hat_prev as f64 / l_hat_curr as f64) * l as f64
}

/// `delta_hat_l` (Eq. 53): the fractional part of [`harmonic_index_ratio`], used to linearly
/// interpolate the prediction between the previous frame's two nearest harmonics.
fn fractional_part(k_hat_l: f64) -> f64 {
    k_hat_l - k_hat_l.floor()
}

/// `log2(M_tilde_j(-1))` for any `j >= 0`, applying the spec's own two boundary assumptions so
/// callers don't need to special-case them: Eq. 56 (`M_tilde_0(-1) = 1.0`, so `log2` is exactly
/// `0.0`) and Eq. 57 (`M_tilde_j(-1) = M_tilde_{L_hat(-1)}(-1)` for `j > L_hat(-1)`, i.e. indices
/// past the previous frame's own last real harmonic hold at that last value rather than reading past
/// the end of `previous_m`).
///
/// `previous_m` holds `M_tilde_j(-1)` for `j = 1..=l_hat_prev`, one-indexed (`previous_m[0]` is
/// `M_tilde_1(-1)`).
fn previous_log2_amplitude(previous_m: &[f64], l_hat_prev: u32, j: u32) -> f64 {
    if j == 0 {
        0.0
    } else {
        let clamped_j = j.min(l_hat_prev);
        previous_m[(clamped_j - 1) as usize].log2()
    }
}

/// The prediction residual `T_hat_l` (Eq. 54) for harmonic `l`: this frame's own unquantized
/// spectral amplitude estimate `M_hat_l(0)` (from [`super::spectral_amplitude`]), minus a
/// pitch-interpolated prediction from the previous frame's reconstructed amplitudes, plus a
/// frame-wide bias correction (the sum over every current-frame harmonic's own prediction) that
/// removes the previous frame's overall level so only the *shape change* is transmitted -- see this
/// module's own doc comment on why `previous_m` must be real reconstructed history, not this
/// encoder's own unquantized estimate of it.
pub fn prediction_residual(
    l: u32,
    unquantized_m_l: f64,
    l_hat_curr: u32,
    l_hat_prev: u32,
    previous_m: &[f64],
) -> f64 {
    let rho = prediction_coefficient(l_hat_curr);

    let k_hat_l = harmonic_index_ratio(l, l_hat_prev, l_hat_curr);
    let delta_l = fractional_part(k_hat_l);
    let floor_k_l = k_hat_l.floor() as u32;
    let predicted =
        rho * (1.0 - delta_l) * previous_log2_amplitude(previous_m, l_hat_prev, floor_k_l)
            + rho * delta_l * previous_log2_amplitude(previous_m, l_hat_prev, floor_k_l + 1);

    let bias: f64 = (1..=l_hat_curr)
        .map(|lambda| {
            let k_hat_lambda = harmonic_index_ratio(lambda, l_hat_prev, l_hat_curr);
            let delta_lambda = fractional_part(k_hat_lambda);
            let floor_k_lambda = k_hat_lambda.floor() as u32;
            (1.0 - delta_lambda) * previous_log2_amplitude(previous_m, l_hat_prev, floor_k_lambda)
                + delta_lambda * previous_log2_amplitude(previous_m, l_hat_prev, floor_k_lambda + 1)
        })
        .sum();
    let bias_correction = (rho / l_hat_curr as f64) * bias;

    unquantized_m_l.log2() - predicted + bias_correction
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prediction_coefficient_matches_eq55_in_all_three_branches() {
        assert!((prediction_coefficient(15) - 0.4).abs() < 1e-12);
        assert!((prediction_coefficient(16) - (0.03 * 16.0 - 0.05)).abs() < 1e-12);
        assert!((prediction_coefficient(24) - (0.03 * 24.0 - 0.05)).abs() < 1e-12);
        assert!((prediction_coefficient(25) - 0.7).abs() < 1e-12);
    }

    #[test]
    fn previous_log2_amplitude_applies_both_spec_boundary_assumptions() {
        let previous_m = [2.0, 4.0, 8.0]; // M_tilde_1..3(-1), l_hat_prev = 3
                                          // Eq. 56: index 0 always reads as log2(1.0) = 0.0, regardless of previous_m's own contents.
        assert_eq!(previous_log2_amplitude(&previous_m, 3, 0), 0.0);
        // A real, in-range index reads straight through.
        assert_eq!(previous_log2_amplitude(&previous_m, 3, 2), 4.0f64.log2());
        // Eq. 57: an index past l_hat_prev holds at the last real value instead of panicking.
        assert_eq!(previous_log2_amplitude(&previous_m, 3, 5), 8.0f64.log2());
    }

    /// The real, checkable point of Eq. 54's own bias-correction term: when the previous frame's
    /// log2 amplitudes are all equal to the same constant (so every harmonic predicts the same
    /// value, regardless of pitch-driven index interpolation), that constant should cancel out of
    /// the residual entirely -- `T_hat_l` should reduce to plain `log2(M_hat_l(0))`, matching the
    /// spec's own stated intent that only the *shape change* (not the previous frame's overall
    /// level) gets transmitted.
    #[test]
    fn a_constant_previous_frame_level_cancels_out_of_the_residual() {
        let l_hat_curr = 20;
        let l_hat_prev = 20;
        let constant_level = 4.0; // M_tilde_j(-1) = 4.0 for every j
        let previous_m = vec![constant_level; l_hat_prev as usize];

        for l in 1..=l_hat_curr {
            let unquantized_m_l = 3.0 + l as f64 * 0.1; // any real, varying per-harmonic estimate
            let residual =
                prediction_residual(l, unquantized_m_l, l_hat_curr, l_hat_prev, &previous_m);
            let expected = unquantized_m_l.log2();
            assert!(
                (residual - expected).abs() < 1e-9,
                "harmonic {l}: expected the constant previous level to cancel out, leaving {expected}, got {residual}"
            );
        }
    }

    /// A real, different-pitch case (l_hat_prev != l_hat_curr, so harmonic_index_ratio is not the
    /// identity map): the same "constant previous level cancels" property must still hold, since
    /// Eq. 54's bias correction sums over exactly the same interpolation the main prediction term
    /// uses -- proving the cancellation isn't an artifact of the l_hat_prev == l_hat_curr case above.
    #[test]
    fn a_constant_previous_frame_level_cancels_out_even_with_a_pitch_change() {
        let l_hat_curr = 15;
        let l_hat_prev = 22;
        let constant_level = 1.5;
        let previous_m = vec![constant_level; l_hat_prev as usize];

        for l in 1..=l_hat_curr {
            let unquantized_m_l = 2.0 + l as f64 * 0.05;
            let residual =
                prediction_residual(l, unquantized_m_l, l_hat_curr, l_hat_prev, &previous_m);
            let expected = unquantized_m_l.log2();
            assert!(
                (residual - expected).abs() < 1e-9,
                "harmonic {l}: expected cancellation under a pitch change too, expected {expected}, got {residual}"
            );
        }
    }
}
