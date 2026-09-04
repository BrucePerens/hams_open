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
        assert!(center > 0.0, "center sample must be the window's peak, got {center}");
        // Hann window is exactly zero at both taper edges by construction
        // (cos(0)=1 and cos(2*pi)=1 both give val=0).
        assert!(w[mp2 - nw2].abs() < 1e-6, "left edge should taper to ~0, got {}", w[mp2 - nw2]);
        assert!(w[mp2 + nw2 - 1].abs() < 1e-6, "right edge should taper to ~0, got {}", w[mp2 + nw2 - 1]);
        for (i, &v) in w.iter().enumerate().take(mp2 + nw2 - 1).skip(mp2 - nw2 + 1) {
            assert!(v <= center + 1e-6, "sample {i}={v} should not exceed the center peak {center}");
        }
    }
}
