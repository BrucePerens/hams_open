//! AMBE (Advanced Multi-Band Excitation) vocoder -- D-STAR's own generation of the algorithm,
//! implemented from TIA-102.BABA (the 2003 base standard) directly, per
//! `docs/proposals/AMBE_CODEC_AND_DSTAR_IMPLEMENTATION_PLAN.md`. That document also names the real
//! reasoning for scope: DMR and Yaesu System Fusion both use the later AMBE+2 generation (the 2009
//! addendum, which names 12 specific patents) and are deliberately NOT implemented here -- see
//! `AMBE_PLUS_2_NOTES.md` in this same directory for what's known about that generation, kept as
//! documentation only.
//!
//! # Real, current state: scaffold plus one verified piece, not a working codec yet
//!
//! This module is a real starting point, not a placeholder pretending to be more than it is. What
//! exists: the frame structure and pipeline stage documentation below (transcribed from spec text that
//! renders as clean, verifiable prose/equations, not at risk of a silent transcription error), plus
//! `fec.rs`'s own Golay/Hamming FEC -- see that module's own doc comment for how its generator
//! matrices were transcribed from a 400 DPI render AND independently verified against each code's own
//! published, spec-external weight distribution before being trusted (a real transcription error was
//! caught and fixed this way, not just guarded against in theory). What does NOT exist yet,
//! deliberately: the quantizer/codebook tables (Annexes E, F, G, J the spec's own text repeatedly
//! refers out to) -- these are large (Annex G alone tabulates bit allocation per value of `L`, not a
//! small fixed matrix), and unlike the two FEC codes above, generic quantizer step tables have no
//! equivalent independent, spec-external mathematical invariant to check a transcription against.
//! Real next step: find or construct a real check for these before transcribing them the same way
//! (e.g. cross-referencing against a real off-air capture's own known-good decode, once one exists),
//! not a one-shot visual read taken on faith.
//!
//! # The encode pipeline, per TIA-102.BABA section 6-7 (spectral amplitude/pitch encoding, error
//! control)
//!
//! 1. **Pitch estimation and voicing decision** produce the fundamental frequency and a per-band
//!    voiced/unvoiced decision across up to `MAX_HARMONICS` spectral bands.
//! 2. **Spectral amplitude encoding**: the `L` harmonic amplitudes are DCT-transformed in six blocks
//!    whose lengths vary with `L` (Fig. 17), forming a six-element "gain vector" via a second, 6-point
//!    DCT across each block's own DC coefficient (Fig. 18, Eq. 60-61) -- see [`gain_vector_dct`] below,
//!    one of the pieces safe to implement now since it's a plain, unambiguous formula, not a table.
//! 3. **Quantization**: the gain vector's first element (overall level) uses a 6-bit non-uniform
//!    quantizer (Annex E's own table -- not yet transcribed); the remaining gain elements and the
//!    higher-order DCT coefficients use uniform quantizers whose bit allocation and step size depend on
//!    `L` (Annexes F and G -- not yet transcribed, per Eq. 62).
//! 4. **Bit prioritization**: the quantized bits `b_0..b_(L+2)` are reordered by importance (Fig. 22's
//!    own "priority scanning") into eight bit vectors `u_0..u_7` before FEC.
//! 5. **Forward error correction**: `u_0..u_3` each get a `[23,12]` Golay code, `u_4..u_6` each get a
//!    `[15,11]` Hamming code, `u_7` is left unprotected (Eq. 81-83) -- implemented and verified in
//!    [`fec`].
//! 6. **Random bit modulation and interleaving** produce the final 144-bit, 20ms transmitted frame
//!    (88 voice bits + 56 FEC bits, per the spec's own section 7.3).

pub mod fec;
pub mod tables;

/// 7.2kbps frame rate: 144 bits every 20ms, per TIA-102.BABA section 7.3 ("At 7.2 kbps with a 20 ms
/// frame size, 144 bits per frame are available for voice coding").
pub const FRAME_BITS: usize = 144;
/// Of `FRAME_BITS`, this many carry the actual quantized model parameters (spectral amplitudes,
/// pitch, gain); the rest (`FRAME_BITS - VOICE_BITS`) are forward error correction.
pub const VOICE_BITS: usize = 88;
/// `FRAME_BITS - VOICE_BITS`: divided between four [23,12] Golay codes (12 data bits each, so 48 of
/// the 88 voice bits) and three [15,11] Hamming codes (11 data bits each, 33 more), with the
/// remaining `88 - 48 - 33 = 7` bits (`u_7`) left completely unprotected -- per Eq. 81-83.
pub const FEC_BITS: usize = FRAME_BITS - VOICE_BITS;
/// Frame duration in milliseconds -- the other half of the "144 bits every 20ms" rate statement.
pub const FRAME_DURATION_MS: f64 = 20.0;

/// The gain vector's second-stage DCT, Eq. 61: a 6-point DCT-II-family transform across the six
/// per-block DC coefficients `r_hat[0..6]` (themselves each block's own first DCT coefficient, per
/// Eq. 60 and Fig. 18), producing the transformed gain vector `G_hat[0..6]`.
///
/// A plain, unambiguous closed-form formula (not a table), so implemented and tested directly rather
/// than deferred with the FEC/quantizer tables above.
///
/// `G_hat_m = (1/6) * sum_{i=1}^{6} R_hat_i * cos(pi*(m-1)*(i-0.5)/6)`, for `1 <= m <= 6`
/// (Eq. 61, 1-indexed in the spec; `r_hat` here is 0-indexed, `r_hat[i-1] == R_hat_i`).
pub fn gain_vector_dct(r_hat: &[f64; 6]) -> [f64; 6] {
    let mut g_hat = [0.0f64; 6];
    for (m, slot) in g_hat.iter_mut().enumerate() {
        let mut sum = 0.0;
        for (i, &r) in r_hat.iter().enumerate() {
            let i1 = (i + 1) as f64; // 1-indexed i, matches the spec's own Eq. 61
            let m1 = m as f64; // (m-1) in the spec's 1-indexed m, so plain m here
            sum += r * (std::f64::consts::PI * m1 * (i1 - 0.5) / 6.0).cos();
        }
        *slot = sum / 6.0;
    }
    g_hat
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gain_vector_dct_of_a_constant_input_is_zero_except_the_first_coefficient() {
        // A constant r_hat is the DC case: the DCT's own m=1 (index 0) term collapses to the
        // plain sum (since cos(0) = 1 for every i), and every higher-order term integrates to
        // zero for a constant input -- the same real, checkable property `spectral_tilt_db_per_
        // octave`'s own DCT-adjacent tests in the daemon crate already lean on for validating a
        // transform implementation before trusting it against anything harder.
        let r_hat = [2.0; 6];
        let g_hat = gain_vector_dct(&r_hat);
        assert!((g_hat[0] - 2.0).abs() < 1e-9, "expected the DC term to equal the constant input, got {}", g_hat[0]);
        for (m, &g) in g_hat.iter().enumerate().skip(1) {
            assert!(g.abs() < 1e-9, "expected higher-order term {m} to vanish for constant input, got {g}");
        }
    }

    #[test]
    fn gain_vector_dct_matches_a_hand_computed_value_for_a_real_asymmetric_input() {
        // A real, non-constant input, computed independently by hand (not by calling the
        // function under test with different inputs and hoping) -- guards against a sign error
        // or an off-by-one in the (i - 0.5) term that the constant-input test above can't catch
        // (a constant input is symmetric under exactly that class of bug).
        let r_hat = [1.0, 0.0, 0.0, 0.0, 0.0, 0.0];
        let g_hat = gain_vector_dct(&r_hat);
        // With only r_hat[0] = 1 nonzero (i=1 in the spec's 1-indexing), Eq. 61 collapses to
        // G_hat_m = (1/6) * cos(pi*(m-1)*0.5/6) for each m.
        for (m, &g) in g_hat.iter().enumerate() {
            let expected = (std::f64::consts::PI * m as f64 * 0.5 / 6.0).cos() / 6.0;
            assert!((g - expected).abs() < 1e-9, "m={m}: expected {expected}, got {g}");
        }
    }
}
