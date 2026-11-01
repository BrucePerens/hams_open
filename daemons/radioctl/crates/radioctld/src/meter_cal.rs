// SPDX-License-Identifier: AGPL-3.0-or-later
//! UNVERIFIED conversion of the raw Icom CI-V transmitter meters (0 to 255 deflection, `15 12`
//! SWR and `15 13` ALC) into what a `rigctl` client expects from `l SWR` (a ratio) and `l ALC`
//! (a 0 to 1 fraction).
//!
//! Source: the scale points printed in the owner's manual CI-V command tables of Icom's IC-7850
//! (SWR meter 0 = 1.0, 48 = 1.5, 80 = 2.0, 120 = 3.0; ALC meter 0 = 0, 120 = maximum). Other models
//! print other points (the IC-9100 and IC-7410 print 41 and 81 for 1.5 and 2.0), so every rig is
//! served from these one set of points and the result is an estimate. Beyond the last printed
//! point nothing is documented: the SWR continues the last segment's slope up to the 10.0 that
//! `\dump_caps` advertises, and the ALC stops at 1.0. No radio has verified any of this;
//! [`CALIBRATION_VERIFIED`] stays false until a bench measurement with a known load replaces the
//! points, and the raw values remain available through the library's `Op::GetMeter`.

/// True only when the points below have been checked against a radio and a known load.
pub const CALIBRATION_VERIFIED: bool = false;

/// Printed (deflection, ratio) points of the SWR meter scale.
const SWR_POINTS: [(f64, f64); 4] = [(0.0, 1.0), (48.0, 1.5), (80.0, 2.0), (120.0, 3.0)];

/// The deflection at which the ALC meter reads its maximum.
const ALC_FULL_SCALE: f64 = 120.0;

/// The largest SWR a `rigctl` client is told (the top of the advertised range).
const SWR_CEILING: f64 = 10.0;

// [@ANCHOR: radioctld_meter_cal_swr_ratio]
/// The SWR ratio (1.0 or more) for a raw 0 to 255 SWR meter deflection. Unverified estimate.
pub fn swr_ratio(raw: u16) -> f64 {
    let x = f64::from(raw.min(255));
    let last = SWR_POINTS.len() - 1;
    let ratio = SWR_POINTS
        .windows(2)
        .find(|pair| x <= pair[1].0)
        .map_or_else(
            || interpolate(SWR_POINTS[last - 1], SWR_POINTS[last], x),
            |pair| interpolate(pair[0], pair[1], x),
        );
    ratio.min(SWR_CEILING)
}

// [@ANCHOR: radioctld_meter_cal_alc_fraction]
/// The ALC level as a 0 to 1 fraction for a raw 0 to 255 ALC meter deflection (1.0 at and beyond
/// the printed maximum). Unverified estimate.
pub fn alc_fraction(raw: u16) -> f64 {
    (f64::from(raw) / ALC_FULL_SCALE).clamp(0.0, 1.0)
}

/// The value at `x` on the straight line through two points.
fn interpolate(a: (f64, f64), b: (f64, f64), x: f64) -> f64 {
    a.1 + (b.1 - a.1) * (x - a.0) / (b.0 - a.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Tests [@ANCHOR: radioctld_meter_cal_swr_ratio]
    #[test]
    fn swr_passes_through_the_printed_points() {
        for (raw, ratio) in [(0, 1.0), (48, 1.5), (80, 2.0), (120, 3.0)] {
            assert!((swr_ratio(raw) - ratio).abs() < 1e-9, "{raw}");
        }
        assert!((swr_ratio(64) - 1.75).abs() < 1e-9);
    }

    // Tests [@ANCHOR: radioctld_meter_cal_swr_ratio]
    #[test]
    fn swr_never_falls_and_stays_within_the_advertised_range() {
        let mut last = 0.0;
        for raw in 0..=300u16 {
            let ratio = swr_ratio(raw);
            assert!(ratio >= last && (1.0..=10.0).contains(&ratio), "{raw}");
            last = ratio;
        }
        assert!((swr_ratio(255) - 6.375).abs() < 1e-9);
    }

    // Tests [@ANCHOR: radioctld_meter_cal_alc_fraction]
    #[test]
    fn alc_is_a_fraction_that_stops_at_one() {
        assert_eq!(alc_fraction(0), 0.0);
        assert!((alc_fraction(60) - 0.5).abs() < 1e-9);
        assert_eq!(alc_fraction(120), 1.0);
        assert_eq!(alc_fraction(255), 1.0);
    }
}
