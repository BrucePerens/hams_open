// SPDX-License-Identifier: AGPL-3.0-or-later
//! UNVERIFIED conversion of the raw Icom CI-V transmitter meters (0 to 255 deflection, `15 11` to
//! `15 16`) into what a `rigctl` client expects: `l SWR` (a ratio), `l ALC` (a 0 to 1 fraction),
//! `COMP_METER` (decibels), `VD_METER` (volts), `ID_METER` (amperes) and the power bar.
//!
//! Each model has its own scale points, printed in its CI-V command table. The IC-9100, IC-7300,
//! IC-7610 and IC-705 points below are those tables' printed points (read for facts only; see the
//! `capability.read-meters` fact of each description for the document and page). Any other rig is
//! served from [`DEFAULT`]: the IC-7850 points (SWR 0 = 1.0, 48 = 1.5, 80 = 2.0, 120 = 3.0; ALC
//! 0 to 120) for SWR and ALC, and Hamlib 4.6.2's IC-7300 points (read for facts only) for
//! COMP, Vd and Id: an estimate. The IC-9100 prints no Vd or Id meter, so it too falls back to the
//! default for those two. Beyond the last printed point nothing is documented: the SWR continues
//! the last segment's slope up to the 10.0 that `\dump_caps` advertises, the ALC stops at 1.0, the
//! others read their last value. No radio has verified any of this; [`CALIBRATION_VERIFIED`] stays
//! false until a bench measurement with a known load confirms the points, and the raw values
//! remain available through the library's `Op::GetMeter`.

/// True only when the points below have been checked against a radio and a known load.
pub const CALIBRATION_VERIFIED: bool = false;

/// The scale points of one model: ascending `(raw deflection, value)` pairs.
#[derive(Debug, Clone, Copy)]
pub struct MeterCal {
    swr: &'static [(f64, f64)],
    /// The deflection at which the ALC meter reads its maximum.
    alc_full: f64,
    comp: &'static [(f64, f64)],
    vd: &'static [(f64, f64)],
    id: &'static [(f64, f64)],
    /// Deflection to the fraction (0 to 1) of full output power.
    power: &'static [(f64, f64)],
}

const DEFAULT_COMP: &[(f64, f64)] = &[(0.0, 0.0), (130.0, 15.0), (241.0, 30.0)];
const DEFAULT_VD: &[(f64, f64)] = &[(0.0, 0.0), (13.0, 10.0), (241.0, 16.0)];
const DEFAULT_ID: &[(f64, f64)] = &[(0.0, 0.0), (97.0, 10.0), (146.0, 15.0), (241.0, 25.0)];

/// Any rig without points of its own.
pub const DEFAULT: MeterCal = MeterCal {
    swr: &[(0.0, 1.0), (48.0, 1.5), (80.0, 2.0), (120.0, 3.0)],
    alc_full: 120.0,
    comp: DEFAULT_COMP,
    vd: DEFAULT_VD,
    id: DEFAULT_ID,
    power: &[(0.0, 0.0), (255.0, 1.0)],
};

const IC9100: MeterCal = MeterCal {
    swr: &[(0.0, 1.0), (41.0, 1.5), (81.0, 2.0), (120.0, 3.0)],
    alc_full: 120.0,
    comp: &[(0.0, 0.0), (120.0, 15.0), (240.0, 30.0)],
    power: &[(0.0, 0.0), (141.0, 0.5), (215.0, 1.0)],
    ..DEFAULT
};

const IC7300: MeterCal = MeterCal {
    swr: &[(0.0, 1.0), (48.0, 1.5), (80.0, 2.0), (120.0, 3.0)],
    alc_full: 120.0,
    comp: DEFAULT_COMP,
    vd: DEFAULT_VD,
    id: DEFAULT_ID,
    power: &[(0.0, 0.0), (143.0, 0.5), (213.0, 1.0)],
};

const IC7610: MeterCal = MeterCal {
    swr: &[(0.0, 1.0), (48.0, 1.5), (80.0, 2.0), (120.0, 3.0)],
    alc_full: 120.0,
    comp: &[(0.0, 0.0), (130.0, 15.0), (241.0, 30.0)],
    vd: &[(0.0, 0.0), (151.0, 10.0), (211.0, 16.0)],
    id: &[(0.0, 0.0), (77.0, 10.0), (165.0, 20.0), (241.0, 30.0)],
    power: &[(0.0, 0.0), (143.0, 0.5), (212.0, 1.0)],
};

const IC705: MeterCal = MeterCal {
    swr: &[(0.0, 1.0), (48.0, 1.5), (80.0, 2.0), (120.0, 3.0)],
    alc_full: 120.0,
    comp: &[(0.0, 0.0), (130.0, 15.0), (210.0, 25.5)],
    vd: &[(0.0, 0.0), (75.0, 5.0), (241.0, 16.0)],
    id: &[(0.0, 0.0), (121.0, 2.0), (241.0, 4.0)],
    power: &[(0.0, 0.0), (143.0, 0.5), (213.0, 1.0)],
};

// [@ANCHOR: radioctld_meter_cal_for_rig]
/// The scale points for the rig description `id` (`icom-ic-9100`, ...); [`DEFAULT`] when the model
/// has none of its own.
pub fn for_rig(id: &str) -> &'static MeterCal {
    match id {
        "icom-ic-9100" => &IC9100,
        "icom-ic-7300" => &IC7300,
        "icom-ic-7610" => &IC7610,
        "icom-ic-705" => &IC705,
        _ => &DEFAULT,
    }
}

/// The largest SWR a `rigctl` client is told (the top of the advertised range).
const SWR_CEILING: f64 = 10.0;

// [@ANCHOR: radioctld_meter_cal_swr_ratio]
/// The SWR ratio (1.0 or more) for a raw 0 to 255 SWR meter deflection. Unverified estimate.
pub fn swr_ratio(cal: &MeterCal, raw: u16) -> f64 {
    let x = f64::from(raw.min(255));
    let points = cal.swr;
    let last = points.len() - 1;
    let ratio = points.windows(2).find(|pair| x <= pair[1].0).map_or_else(
        || interpolate(points[last - 1], points[last], x),
        |pair| interpolate(pair[0], pair[1], x),
    );
    ratio.min(SWR_CEILING)
}

// [@ANCHOR: radioctld_meter_cal_alc_fraction]
/// The ALC level as a 0 to 1 fraction for a raw 0 to 255 ALC meter deflection (1.0 at and beyond
/// the printed maximum). Unverified estimate.
pub fn alc_fraction(cal: &MeterCal, raw: u16) -> f64 {
    (f64::from(raw) / cal.alc_full).clamp(0.0, 1.0)
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
pub fn comp_db(cal: &MeterCal, raw: u16) -> f64 {
    clamped(cal.comp, raw)
}

// [@ANCHOR: radioctld_meter_cal_vd_volts]
/// Supply voltage in volts (0 to 16) for a raw Vd meter deflection. Unverified estimate.
pub fn vd_volts(cal: &MeterCal, raw: u16) -> f64 {
    clamped(cal.vd, raw)
}

// [@ANCHOR: radioctld_meter_cal_id_amps]
/// Drain current in amperes (0 to 25) for a raw Id meter deflection. Unverified estimate.
pub fn id_amps(cal: &MeterCal, raw: u16) -> f64 {
    clamped(cal.id, raw)
}

// [@ANCHOR: radioctld_meter_cal_power_fraction]
/// The output power as a 0 to 1 fraction of full power for a raw 0 to 255 deflection, the way an
/// uncalibrated `RFPOWER_METER` is served: a relative bar from the model's printed points (a
/// model without them reads the deflection over 255), not a measurement. A rig with watt
/// calibration points is served by [`power_fraction_cal`] instead. Unverified estimate.
pub fn power_fraction(cal: &MeterCal, raw: u16) -> f64 {
    clamped(cal.power, raw).clamp(0.0, 1.0)
}

// [@ANCHOR: radioctld_meter_cal_power_fraction_cal]
/// The output power as a fraction of the calibrated full scale (the last calibration point).
pub fn power_fraction_cal(cal: &radioctl_core::power::PowerCal, raw: u16) -> f64 {
    let full = cal.full_scale().watts();
    if full > 0.0 {
        (cal.watts(f64::from(raw)).watts() / full).clamp(0.0, 1.0)
    } else {
        0.0
    }
}

// [@ANCHOR: radioctld_meter_cal_level]
/// The `rigctl` value of `meter` for a raw deflection: SWR ratio, ALC fraction 0 to 1, COMP
/// decibels, Vd volts, Id amperes. Unverified estimate.
pub fn level(cal: &MeterCal, meter: radioctl_core::units::Meter, raw: u16) -> f64 {
    use radioctl_core::units::Meter;
    match meter {
        Meter::Swr => swr_ratio(cal, raw),
        Meter::Alc => alc_fraction(cal, raw),
        Meter::Comp => comp_db(cal, raw),
        Meter::Vd => vd_volts(cal, raw),
        Meter::Id => id_amps(cal, raw),
    }
}

// [@ANCHOR: radioctld_meter_cal_range]
/// The largest value `rigctl` is told for `meter` (the top of its advertised range).
pub fn full_scale(cal: &MeterCal, meter: radioctl_core::units::Meter) -> f64 {
    use radioctl_core::units::Meter;
    let top = |points: &[(f64, f64)]| points.last().map_or(0.0, |p| p.1);
    match meter {
        Meter::Swr => SWR_CEILING,
        Meter::Alc => 1.0,
        Meter::Comp => top(cal.comp),
        Meter::Vd => top(cal.vd),
        Meter::Id => top(cal.id),
    }
}

/// The value at `x` on the straight line through two points.
fn interpolate(a: (f64, f64), b: (f64, f64), x: f64) -> f64 {
    a.1 + (b.1 - a.1) * (x - a.0) / (b.0 - a.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use radioctl_core::units::Meter;

    // Tests [@ANCHOR: radioctld_meter_cal_swr_ratio]
    // Tests [@ANCHOR: radioctld_meter_cal_for_rig]
    #[test]
    fn swr_passes_through_each_models_printed_points() {
        let nine = for_rig("icom-ic-9100");
        for (raw, ratio) in [(0, 1.0), (41, 1.5), (81, 2.0), (120, 3.0)] {
            assert!((swr_ratio(nine, raw) - ratio).abs() < 1e-9, "9100 {raw}");
        }
        for id in [
            "icom-ic-7300",
            "icom-ic-7610",
            "icom-ic-705",
            "icom-ic-7850",
        ] {
            for (raw, ratio) in [(0, 1.0), (48, 1.5), (80, 2.0), (120, 3.0)] {
                assert!(
                    (swr_ratio(for_rig(id), raw) - ratio).abs() < 1e-9,
                    "{id} {raw}"
                );
            }
        }
        assert!((swr_ratio(for_rig("icom-ic-7300"), 64) - 1.75).abs() < 1e-9);
        assert_eq!(for_rig("icom-ic-7800").swr, DEFAULT.swr);
    }

    // Tests [@ANCHOR: radioctld_meter_cal_swr_ratio]
    #[test]
    fn swr_never_falls_and_stays_within_the_advertised_range() {
        for id in ["icom-ic-9100", "icom-ic-7300", "x"] {
            let cal = for_rig(id);
            let mut last = 0.0;
            for raw in 0..=300u16 {
                let ratio = swr_ratio(cal, raw);
                assert!(ratio >= last && (1.0..=10.0).contains(&ratio), "{id} {raw}");
                last = ratio;
            }
        }
        assert!((swr_ratio(&DEFAULT, 255) - 6.375).abs() < 1e-9);
    }

    // Tests [@ANCHOR: radioctld_meter_cal_power_fraction]
    // Tests [@ANCHOR: radioctld_meter_cal_power_fraction_cal]
    #[test]
    fn power_is_a_fraction_and_follows_the_model_or_a_calibration() {
        assert_eq!(power_fraction(&DEFAULT, 0), 0.0);
        assert!((power_fraction(&DEFAULT, 255) - 1.0).abs() < 1e-9);
        assert!(power_fraction(&DEFAULT, 300) <= 1.0);
        let nine = for_rig("icom-ic-9100");
        assert!((power_fraction(nine, 141) - 0.5).abs() < 1e-9);
        assert!((power_fraction(nine, 215) - 1.0).abs() < 1e-9);
        assert_eq!(power_fraction(nine, 255), 1.0);
        assert!((power_fraction(for_rig("icom-ic-7610"), 212) - 1.0).abs() < 1e-9);
        // The IC-7300's printed points (0 = 0, 143 = half, 213 = full), not the raw deflection over 255.
        let ic7300 = for_rig("icom-ic-7300");
        assert_eq!(power_fraction(ic7300, 0), 0.0);
        assert!((power_fraction(ic7300, 143) - 0.5).abs() < 1e-9);
        assert!((power_fraction(ic7300, 213) - 1.0).abs() < 1e-9);
        assert!((power_fraction(ic7300, 128) - 128.0 / 143.0 * 0.5).abs() < 1e-9);
        let cal =
            radioctl_core::power::PowerCal::new(vec![(0.0, 0.0), (100.0, 50.0), (200.0, 100.0)])
                .expect("valid");
        assert!((power_fraction_cal(&cal, 100) - 0.5).abs() < 1e-9);
        assert_eq!(power_fraction_cal(&cal, 255), 1.0);
    }

    // Tests [@ANCHOR: radioctld_meter_cal_alc_fraction]
    #[test]
    fn alc_is_a_fraction_that_stops_at_one() {
        assert_eq!(alc_fraction(&DEFAULT, 0), 0.0);
        assert!((alc_fraction(&DEFAULT, 60) - 0.5).abs() < 1e-9);
        assert_eq!(alc_fraction(&DEFAULT, 120), 1.0);
        assert_eq!(alc_fraction(&DEFAULT, 255), 1.0);
    }

    // Tests [@ANCHOR: radioctld_meter_cal_clamped]
    // Tests [@ANCHOR: radioctld_meter_cal_comp_db]
    // Tests [@ANCHOR: radioctld_meter_cal_vd_volts]
    // Tests [@ANCHOR: radioctld_meter_cal_id_amps]
    // Tests [@ANCHOR: radioctld_meter_cal_level]
    // Tests [@ANCHOR: radioctld_meter_cal_range]
    #[test]
    fn comp_vd_and_id_follow_each_models_points_and_stop_at_the_last() {
        let d = &DEFAULT;
        for (raw, db) in [(0, 0.0), (130, 15.0), (241, 30.0), (255, 30.0)] {
            assert!((comp_db(d, raw) - db).abs() < 1e-9, "comp {raw}");
        }
        for (raw, v) in [(0, 0.0), (13, 10.0), (241, 16.0), (255, 16.0)] {
            assert!((vd_volts(d, raw) - v).abs() < 1e-9, "vd {raw}");
        }
        for (raw, a) in [(0, 0.0), (97, 10.0), (146, 15.0), (241, 25.0), (255, 25.0)] {
            assert!((id_amps(d, raw) - a).abs() < 1e-9, "id {raw}");
        }
        assert!((id_amps(d, 90) - 90.0 * 10.0 / 97.0).abs() < 1e-9);
        // Per-model points: the IC-9100 COMP, the IC-7610 and IC-705 Vd and Id.
        assert!((comp_db(for_rig("icom-ic-9100"), 120) - 15.0).abs() < 1e-9);
        assert!((comp_db(for_rig("icom-ic-705"), 210) - 25.5).abs() < 1e-9);
        assert!((vd_volts(for_rig("icom-ic-7610"), 151) - 10.0).abs() < 1e-9);
        assert!((vd_volts(for_rig("icom-ic-705"), 75) - 5.0).abs() < 1e-9);
        assert!((id_amps(for_rig("icom-ic-7610"), 165) - 20.0).abs() < 1e-9);
        assert!((id_amps(for_rig("icom-ic-705"), 241) - 4.0).abs() < 1e-9);
        assert!((full_scale(for_rig("icom-ic-7610"), Meter::Id) - 30.0).abs() < 1e-9);
        assert!((full_scale(for_rig("icom-ic-705"), Meter::Comp) - 25.5).abs() < 1e-9);
        for id in [
            "icom-ic-9100",
            "icom-ic-7300",
            "icom-ic-7610",
            "icom-ic-705",
            "x",
        ] {
            let cal = for_rig(id);
            for meter in Meter::ALL {
                let top = full_scale(cal, meter);
                let mut last = 0.0;
                for raw in 0..=300u16 {
                    let v = level(cal, meter, raw);
                    assert!(v >= last && v <= top, "{id} {meter:?} {raw}");
                    last = v;
                }
            }
        }
    }
}
