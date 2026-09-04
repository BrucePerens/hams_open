// SPDX-License-Identifier: LGPL-3.0-or-later
//! LPC analysis window: a standard Hann window (textbook DSP, not
//! creative expression), `NW` samples wide, centered in an `M_PITCH`-
//! sample buffer, normalized so the windowed signal's own energy sum
//! matches what `super::lpc`'s autocorrelation-based analysis expects.

use super::{FFT_ENC, M_PITCH, NW};

/// Builds the time-domain analysis window (`M_PITCH` samples, mostly
/// zero outside the centered `NW`-sample Hann taper).
pub fn make_analysis_window() -> [f32; M_PITCH] {
    let mut w = [0.0f32; M_PITCH];
    let mp2 = M_PITCH / 2;
    let nw2 = NW / 2;

    let mut energy = 0.0f32;
    for j in 0..NW {
        let phase = std::f32::consts::TAU * j as f32 / (NW - 1) as f32;
        let val = 0.5 - 0.5 * phase.cos();
        let i = mp2 - nw2 + j;
        w[i] = val;
        energy += val * val;
    }

    let scale = 1.0 / (energy * FFT_ENC as f32).sqrt();
    for v in w.iter_mut() {
        *v *= scale;
    }
    w
}

/// Q-format fractional bits for `make_analysis_window_fixed` below --
/// real measured window values (see the test that validates this
/// against `make_analysis_window` itself) never exceed ~0.0044 in
/// magnitude, so 30 fractional bits (no integer bits needed, real
/// margin under `i32`'s 2^31 range: `0.0044 * 2^30 ~ 4.7e6`, far under
/// `i32::MAX`) gives real headroom without wasting resolution the way a
/// wider integer part would.
const WINDOW_FRAC_BITS: u32 = 30;

/// `FIXED_POINT_ENCODER_IMPLEMENTATION_PUNCH_LIST.md`'s window.rs
/// stage: a fixed-point candidate for `make_analysis_window`, quantized
/// from the same construction (the Hann-window `cos()` computation
/// itself stays `f32` -- it runs once per `Encoder` lifetime, not
/// per-frame, so its own float cost is real but negligible; what
/// matters for the eventual per-frame windowing multiply to go
/// fixed-point is this function's *output* type). **Not yet wired into
/// `Encoder`** -- the per-frame consumer (`windowed[i] = s * win` in
/// `mod.rs`) still expects `f32`, and won't until `autocorrelate`'s own
/// boundary redesign (the punch list's real gate) is done. Exists now,
/// validated against the real `f32` construction, so it's ready when
/// that boundary work lands.
pub fn make_analysis_window_fixed() -> [i32; M_PITCH] {
    let w = make_analysis_window();
    std::array::from_fn(|i| {
        (w[i] as f64 * (1i64 << WINDOW_FRAC_BITS) as f64).round() as i32
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_is_zero_outside_its_centered_support() {
        let w = make_analysis_window();
        let mp2 = M_PITCH / 2;
        let nw2 = NW / 2;
        for (i, &v) in w.iter().enumerate().take(mp2 - nw2) {
            assert_eq!(v, 0.0, "sample {i} should be outside the Hann support");
        }
        for (i, &v) in w.iter().enumerate().skip(mp2 + nw2) {
            assert_eq!(v, 0.0, "sample {i} should be outside the Hann support");
        }
    }

    #[test]
    fn window_peaks_near_its_center_and_tapers_to_zero_at_its_edges() {
        let w = make_analysis_window();
        let mp2 = M_PITCH / 2;
        let nw2 = NW / 2;
        let center = w[mp2];
        assert!(
            center > 0.0,
            "center sample must be the window's peak, got {center}"
        );
        // Hann window is exactly zero at both taper edges by construction
        // (cos(0)=1 and cos(2*pi)=1 both give val=0).
        assert!(
            w[mp2 - nw2].abs() < 1e-6,
            "left edge should taper to ~0, got {}",
            w[mp2 - nw2]
        );
        assert!(
            w[mp2 + nw2 - 1].abs() < 1e-6,
            "right edge should taper to ~0, got {}",
            w[mp2 + nw2 - 1]
        );
        for (i, &v) in w.iter().enumerate().take(mp2 + nw2 - 1).skip(mp2 - nw2 + 1) {
            assert!(
                v <= center + 1e-6,
                "sample {i}={v} should not exceed the center peak {center}"
            );
        }
    }

    /// Real measured range + quantization-error check for
    /// `make_analysis_window_fixed`'s own `WINDOW_FRAC_BITS` choice --
    /// confirms the real headroom claim in that constant's own doc
    /// comment rather than just asserting it, and that dequantizing back
    /// to `f32` tracks the real float window tightly (an ordinary
    /// quantization-noise bound, not a structural-risk one -- this
    /// window has no division or iterative amplification the way
    /// Levinson-Durbin does).
    #[test]
    fn make_analysis_window_fixed_matches_the_real_float_window_within_quantization_noise() {
        let w_float = make_analysis_window();
        let w_fixed = make_analysis_window_fixed();

        let max_abs = w_float.iter().cloned().fold(0.0f32, |a, b| a.max(b.abs()));
        assert!(
            max_abs > 0.0 && max_abs < 1.0,
            "expected the real Hann window's own real measured peak (~0.0043) to stay comfortably under 1.0, got {max_abs}"
        );
        let peak_scaled = (max_abs as f64) * ((1i64 << WINDOW_FRAC_BITS) as f64);
        assert!(
            peak_scaled < i32::MAX as f64,
            "real window peak {max_abs} exceeds i32 headroom at WINDOW_FRAC_BITS={WINDOW_FRAC_BITS}"
        );

        let scale = (1i64 << WINDOW_FRAC_BITS) as f32;
        let mut max_err = 0.0f32;
        for i in 0..M_PITCH {
            let dequantized = w_fixed[i] as f32 / scale;
            max_err = max_err.max((dequantized - w_float[i]).abs());
        }
        assert!(
            max_err < 1e-8,
            "make_analysis_window_fixed diverged from the real float window by {max_err}, more than ordinary Q{WINDOW_FRAC_BITS} rounding noise should allow"
        );
    }
}
