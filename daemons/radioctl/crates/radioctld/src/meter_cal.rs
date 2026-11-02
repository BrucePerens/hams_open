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
//!
//! `COMP_METER` (decibels of compression), `VD_METER` (volts) and `ID_METER` (amperes) use the
//! scale points of the same kind that Hamlib 4.6.2 carries as its Icom default and for the IC-7300
//! (`rigs/icom/icom.c` tables `icom_default_comp_meter_cal`, `_vd_` and `_id_`, and
//! `rigs/icom/ic7300.c` `IC7300_COMP_METER_CAL`, `_VD_` and `_ID_`; read for facts only, the numbers
//! below are those points). They are one model's points applied to every CI-V rig, so they are
//! just as unverified as the SWR and ALC ones, and unlike the SWR they are not continued beyond
//! the last point: a deflection above it reads that last value (what Hamlib does too).

/// True only when the points below have been checked against a radio and a known load.
pub const CALIBRATION_VERIFIED: bool = false;

/// Printed (deflection, ratio) points of the SWR meter scale.
const SWR_POINTS: [(f64, f64); 4] = [(0.0, 1.0), (48.0, 1.5), (80.0, 2.0), (120.0, 3.0)];

/// The deflection at which the ALC meter reads its maximum.
const ALC_FULL_SCALE: f64 = 120.0;

/// Deflection to decibels of compression (Hamlib's IC-7300 points).
const COMP_POINTS: [(f64, f64); 3] = [(0.0, 0.0), (130.0, 15.0), (241.0, 30.0)];

/// Deflection to supply volts (Hamlib's IC-7300 points).
const VD_POINTS: [(f64, f64); 3] = [(0.0, 0.0), (13.0, 10.0), (241.0, 16.0)];

/// Deflection to drain amperes (Hamlib's IC-7300 points).
const ID_POINTS: [(f64, f64); 4] = [(0.0, 0.0), (97.0, 10.0), (146.0, 15.0), (241.0, 25.0)];

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

// [@ANCHOR: radioctld_meter_cal_clamped]
/// The value at `raw` on the line through `points` (ascending deflections), the first or the last
/// value outside them. Unverified, like every table here.
fn clamped(points: &[(f64, f64)], raw: u16) -> f64 {
    let x = f64::from(raw.min(255));
    let (Some(first), Some(last)) = (points.first(), points.last()) else {
        return 0.0;
    };
    if x <= first.0 {
        return first.1;
    }
    points
        .windows(2)
        .find(|pair| x <= pair[1].0)
        .map_or(last.1, |pair| interpolate(pair[0], pair[1], x))
}

// [@ANCHOR: radioctld_meter_cal_comp_db]
/// Compression in decibels (0 to 30) for a raw COMP meter deflection. Unverified estimate.
pub fn comp_db(raw: u16) -> f64 {
    clamped(&COMP_POINTS, raw)
}

// [@ANCHOR: radioctld_meter_cal_vd_volts]
/// Supply voltage in volts (0 to 16) for a raw Vd meter deflection. Unverified estimate.
pub fn vd_volts(raw: u16) -> f64 {
    clamped(&VD_POINTS, raw)
}

// [@ANCHOR: radioctld_meter_cal_id_amps]
/// Drain current in amperes (0 to 25) for a raw Id meter deflection. Unverified estimate.
pub fn id_amps(raw: u16) -> f64 {
    clamped(&ID_POINTS, raw)
}

// [@ANCHOR: radioctld_meter_cal_level]
/// The `rigctl` value of `meter` for a raw deflection: SWR ratio, ALC fraction 0 to 1, COMP
/// decibels, Vd volts, Id amperes. Unverified estimate.
pub fn level(meter: radioctl_core::units::Meter, raw: u16) -> f64 {
    use radioctl_core::units::Meter;
    match meter {
        Meter::Swr => swr_ratio(raw),
        Meter::Alc => alc_fraction(raw),
        Meter::Comp => comp_db(raw),
        Meter::Vd => vd_volts(raw),
        Meter::Id => id_amps(raw),
    }
}

// [@ANCHOR: radioctld_meter_cal_range]
/// The largest value `rigctl` is told for `meter` (the top of its advertised range).
pub fn full_scale(meter: radioctl_core::units::Meter) -> f64 {
    use radioctl_core::units::Meter;
    match meter {
        Meter::Swr => SWR_CEILING,
        Meter::Alc => 1.0,
        Meter::Comp => COMP_POINTS[2].1,
        Meter::Vd => VD_POINTS[2].1,
        Meter::Id => ID_POINTS[3].1,
    }
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

    // Tests [@ANCHOR: radioctld_meter_cal_clamped]
    // Tests [@ANCHOR: radioctld_meter_cal_comp_db]
    // Tests [@ANCHOR: radioctld_meter_cal_vd_volts]
    // Tests [@ANCHOR: radioctld_meter_cal_id_amps]
    // Tests [@ANCHOR: radioctld_meter_cal_level]
    // Tests [@ANCHOR: radioctld_meter_cal_range]
    #[test]
    fn comp_vd_and_id_follow_their_points_and_stop_at_the_last() {
        use radioctl_core::units::Meter;
        for (raw, db) in [(0, 0.0), (130, 15.0), (241, 30.0), (255, 30.0)] {
            assert!((comp_db(raw) - db).abs() < 1e-9, "comp {raw}");
        }
        for (raw, v) in [(0, 0.0), (13, 10.0), (241, 16.0), (255, 16.0)] {
            assert!((vd_volts(raw) - v).abs() < 1e-9, "vd {raw}");
        }
        for (raw, a) in [(0, 0.0), (97, 10.0), (146, 15.0), (241, 25.0), (255, 25.0)] {
            assert!((id_amps(raw) - a).abs() < 1e-9, "id {raw}");
        }
        assert!((id_amps(90) - 90.0 * 10.0 / 97.0).abs() < 1e-9);
        for meter in Meter::ALL {
            let top = full_scale(meter);
            let mut last = 0.0;
            for raw in 0..=300u16 {
                let v = level(meter, raw);
                assert!(v >= last && v <= top, "{meter:?} {raw}");
                last = v;
            }
        }
    }
}
