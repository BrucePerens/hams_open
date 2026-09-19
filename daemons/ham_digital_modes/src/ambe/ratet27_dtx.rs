// SPDX-License-Identifier: LGPL-3.0-or-later
//! The real DVSI chip's `DTX_ENABLE` (Discontinuous Transmission / Voice Activity Detection)
//! silence-classification behavior within RATET(27), determined by feeding pure digital silence and
//! loud voiced content to the live chip with `ECMODE_IN`'s `DTX_ENABLE` bit forced on -- see
//! `AMBE_CHIP_VALIDATION_FINDINGS.md` sections 26, 28, and 31 for the full experimental record.
//!
//! **Corrected from an earlier version of this module, which is worth recording rather than
//! silently fixing**: an initial single-frame check found `g0`, `g2`, and `c7` all reading clean
//! constants for confirmed DTX-silence, and this module originally required all three to match. A
//! fresh, more careful 10-frame live check (`examples/ambe_chip_validate_ratet27_dtx.rs`) found
//! `g2` and `c7` are *not* reliably constant during confirmed silence (`g2` took 7 different values,
//! `c7` took 3, across just 10 frames) -- only `g0` is: exactly `3841` in 10/10 fresh silence
//! frames and exactly `1597` in 10/10 fresh loud-tone frames, zero overlap. **`g0` alone is the
//! real, robust discriminator; `g2`/`c7` were not.**
//!
//! This module implements only [`is_dtx_silence_frame`], a direct classifier on `g0` alone -- it
//! does not claim `g0`'s specific silence value carries a meaningful "background noise level"
//! (DVSI's own manual claim for this feature, tested and found inconclusive in section 26's own
//! noise-level sweep).

/// `g0`'s confirmed, robust constant value for a genuine DTX-silence frame -- verified stable
/// across 10 fresh live frames with zero exceptions, and zero overlap with voiced-frame values.
pub const DTX_SILENCE_G0: u16 = 3841;

/// Classifies a decoded RATET(27) frame (with `DTX_ENABLE` on) as a genuine DTX-silence frame by
/// matching [`DTX_SILENCE_G0`] exactly. `g2`/`c7` are deliberately not checked -- an earlier version
/// of this classifier required them too, but they were found unreliable during confirmed silence
/// (see this module's own doc comment); `g0` alone is the field actually shown robust.
pub fn is_dtx_silence_frame(g0: u16) -> bool {
    g0 == DTX_SILENCE_G0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_dtx_silence_frame_matches_the_confirmed_constant() {
        assert!(is_dtx_silence_frame(DTX_SILENCE_G0));
    }

    #[test]
    fn is_dtx_silence_frame_rejects_other_values() {
        assert!(!is_dtx_silence_frame(DTX_SILENCE_G0 + 1));
        assert!(!is_dtx_silence_frame(0));
    }

    /// Real chip-captured `g0` values from a fresh 10-frame live check
    /// (`examples/ambe_chip_validate_ratet27_dtx.rs`): exactly `3841` for every one of 10 silence
    /// frames, exactly `1597` for every one of 10 loud-tone frames, zero exceptions either way.
    #[test]
    fn matches_every_real_captured_silence_frame_and_rejects_every_voiced_one() {
        for _ in 0..10 {
            assert!(is_dtx_silence_frame(3841));
        }
        for _ in 0..10 {
            assert!(!is_dtx_silence_frame(1597));
        }
    }
}
