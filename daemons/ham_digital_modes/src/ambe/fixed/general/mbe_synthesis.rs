// SPDX-License-Identifier: LGPL-3.0-or-later
//! Fixed-point port of [`crate::ambe::float::mbe_synthesis`]: the mode-independent MBE frame
//! synthesis D-STAR and AMBE+2 half-rate share, turning one frame's already-dequantized
//! `(w0, per-harmonic voicing, Ml)` into 20 ms of Q16.16 PCM by reusing
//! [`crate::ambe::fixed::ratet27::synthesis::SynthesisState`] (enhancement, smoothing, voiced and
//! unvoiced synthesis). See the float sibling for the mbelib background and the two deliberate
//! differences from mbelib; FEC statistics feed the same smoothing thresholds with the two Golay
//! blocks these modes carry (`epsilon_c0`, `epsilon_c1`) and zero for the vectors they lack.

use super::unvoiced_synthesis::N;
use crate::ambe::fixed::ratet27::error_estimation::estimate_errors_q16;
use crate::ambe::fixed::ratet27::synthesis::SynthesisState;

pub struct MbeSynthesizer {
    synthesis: SynthesisState,
    error_rate_prev_q16: i32,
}

impl MbeSynthesizer {
    pub fn new() -> Self {
        Self { synthesis: SynthesisState::new(), error_rate_prev_q16: 0 }
    }

    /// Synthesizes one speech frame. `voiced` and `ml_q16` are both 1-indexed by harmonic (index 0
    /// is unused padding), exactly as `SpeechParameters` carries them. Returns `None` on a length
    /// mismatch.
    pub fn synthesize_speech(
        &mut self,
        w0_q16: i32,
        voiced: &[bool],
        ml_q16: &[i32],
        epsilon_c0: u32,
        epsilon_c1: u32,
    ) -> Option<[i64; N]> {
        if voiced.len() != ml_q16.len() || voiced.len() < 2 {
            return None;
        }
        let errors = estimate_errors_q16(&[epsilon_c0, epsilon_c1, 0, 0, 0, 0, 0], self.error_rate_prev_q16);
        self.error_rate_prev_q16 = errors.rate_q16;
        self.synthesis.synthesize_frame(&ml_q16[1..], w0_q16, &voiced[1..], &errors)
    }

    /// Repeats the previous frame's parameters (an erasure); `None` before any real frame has run.
    pub fn synthesize_repeat(&mut self) -> Option<[i64; N]> {
        self.synthesis.synthesize_repeated_frame()
    }

    /// A silence frame: all zeros.
    pub fn synthesize_silence(&self) -> [i64; N] {
        [0; N]
    }
}

impl Default for MbeSynthesizer {
    fn default() -> Self {
        Self::new()
    }
}
