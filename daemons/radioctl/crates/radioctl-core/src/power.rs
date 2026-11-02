// SPDX-License-Identifier: AGPL-3.0-or-later
//! Transmit power: one canonical unit (watts), the conversions into it, the reading a source
//! hands to a consumer, and the display formatter (`docs/DESIGN.md` decision 26).
//!
//! Radios run milliwatts and amplifiers kilowatts, and some meters read in dBm, so every source
//! converts to watts at the source; nothing downstream sees another unit. A reading carries where
//! it came from (the rig's own meter or a named external wattmeter), how long it stays good, and
//! whether the number is a relative bar (a rig without a calibration reports 0 to 100 and says so).

use std::time::Duration;

/// Transmit power in watts: a finite, non-negative number.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Default)]
pub struct Power(f64);

impl Power {
    /// No power.
    pub const ZERO: Power = Power(0.0);

    // [@ANCHOR: radioctl_power_from_watts]
    /// Power from a figure in watts; `None` for a negative, infinite or not-a-number figure.
    pub fn from_watts(watts: f64) -> Option<Power> {
        (watts.is_finite() && watts >= 0.0).then_some(Power(watts))
    }

    // [@ANCHOR: radioctl_power_from_dbm]
    /// Power from decibels relative to one milliwatt: `P = 10^((dBm - 30) / 10)` watts.
    pub fn from_dbm(dbm: f64) -> Option<Power> {
        Power::from_watts(10f64.powf((dbm - 30.0) / 10.0))
    }

    // [@ANCHOR: radioctl_power_from_dbw]
    /// Power from decibels relative to one watt.
    pub fn from_dbw(dbw: f64) -> Option<Power> {
        Power::from_watts(10f64.powf(dbw / 10.0))
    }

    // [@ANCHOR: radioctl_power_from_dbmv_50]
    /// Power from decibels relative to one millivolt RMS across 50 ohms: `P = V^2 / 50` with
    /// `V = 10^(dBmV / 20)` millivolts, about -46.99 dBm at 0 dBmV.
    pub fn from_dbmv_50ohm(dbmv: f64) -> Option<Power> {
        let volts = 10f64.powf(dbmv / 20.0) * 1e-3;
        Power::from_watts(volts * volts / 50.0)
    }

    // [@ANCHOR: radioctl_power_watts]
    /// The figure in watts.
    pub fn watts(self) -> f64 {
        self.0
    }

    // [@ANCHOR: radioctl_power_dbm]
    /// The figure in dBm (negative infinity for no power).
    pub fn dbm(self) -> f64 {
        10.0 * self.0.log10() + 30.0
    }

    // [@ANCHOR: radioctl_power_offset_db]
    /// Power at the transmitter for a figure measured behind a coupler and attenuator that lose
    /// `offset_db` decibels (positive: the meter reads that much lower than the line). The offset
    /// is applied in the decibel domain, before any display conversion.
    pub fn with_offset_db(self, offset_db: f64) -> Option<Power> {
        Power::from_watts(self.0 * 10f64.powf(offset_db / 10.0))
    }
}

/// The unit a power meter reports in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PowerUnit {
    /// Watts.
    Watts,
    /// Decibels relative to one milliwatt.
    Dbm,
    /// Decibels relative to one watt.
    Dbw,
    /// Decibels relative to one millivolt into 50 ohms.
    DbmvOn50Ohm,
}

impl PowerUnit {
    // [@ANCHOR: radioctl_power_unit_name]
    /// The configuration name of the unit.
    pub fn name(self) -> &'static str {
        match self {
            PowerUnit::Watts => "w",
            PowerUnit::Dbm => "dbm",
            PowerUnit::Dbw => "dbw",
            PowerUnit::DbmvOn50Ohm => "dbmv50",
        }
    }

    // [@ANCHOR: radioctl_power_unit_from_name]
    /// The unit with a configuration name.
    pub fn from_name(name: &str) -> Option<PowerUnit> {
        [
            PowerUnit::Watts,
            PowerUnit::Dbm,
            PowerUnit::Dbw,
            PowerUnit::DbmvOn50Ohm,
        ]
        .into_iter()
        .find(|u| u.name() == name)
    }

    // [@ANCHOR: radioctl_power_unit_to_power]
    /// Converts a figure in this unit, measured behind `offset_db` of coupler and attenuator loss,
    /// into watts at the transmitter. The offset is added in the decibel domain for the logarithmic
    /// units and applied as a power ratio for watts.
    pub fn to_power(self, value: f64, offset_db: f64) -> Option<Power> {
        let at_meter = match self {
            PowerUnit::Watts => Power::from_watts(value)?,
            PowerUnit::Dbm => Power::from_dbm(value)?,
            PowerUnit::Dbw => Power::from_dbw(value)?,
            PowerUnit::DbmvOn50Ohm => Power::from_dbmv_50ohm(value)?,
        };
        at_meter.with_offset_db(offset_db)
    }
}

/// Where a power reading came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PowerOrigin {
    /// The rig's own power meter.
    Rig,
    /// A wattmeter attached to the shack, by its name in the station configuration.
    External(String),
}

impl PowerOrigin {
    // [@ANCHOR: radioctl_power_origin_label]
    /// `rig` or `external`, the word telemetry carries.
    pub fn label(&self) -> &'static str {
        match self {
            PowerOrigin::Rig => "rig",
            PowerOrigin::External(_) => "external",
        }
    }
}

/// One transmit-power reading.
#[derive(Debug, Clone, PartialEq)]
pub struct PowerReading {
    /// Forward power in watts, or a 0 to 100 relative bar when `relative` is set.
    pub forward: Power,
    /// Reflected power in watts, when the source measures it.
    pub reflected: Option<Power>,
    /// Where it came from.
    pub origin: PowerOrigin,
    /// How long the reading stays good after it was taken.
    pub valid_for: Duration,
    /// The figure is a relative 0 to 100 bar, not watts: the rig has no calibration.
    pub relative: bool,
    /// The meter's full scale in watts, when known, so a display can range its bar.
    pub scale: Option<Power>,
}

/// A source of transmit-power readings: the rig's meter or an external wattmeter. The sync
/// shape keeps it usable from the rig actor, a station task or a test; a source that needs I/O
/// keeps its latest reading and returns it here (see `radioctl-station`'s `PowerFeed`).
pub trait RfPowerReader: Send + Sync {
    // [@ANCHOR: radioctl_power_reader_read]
    /// The latest reading; `None` when there is none or it
    /// is no longer valid (a stale reading is no power, never the last number).
    fn read(&self) -> Option<PowerReading>;
}

/// Calibration of a rig's raw power meter: ascending (raw, watts) points, linear in between,
/// flat beyond the last. Provenance lives in the rig description's `[[facts]]`.
#[derive(Debug, Clone, PartialEq)]
pub struct PowerCal {
    points: Vec<(f64, f64)>,
}

impl PowerCal {
    // [@ANCHOR: radioctl_power_cal_new]
    /// A calibration from at least two points with strictly ascending raw values and
    /// non-negative, non-decreasing watts.
    pub fn new(points: Vec<(f64, f64)>) -> Option<PowerCal> {
        let ok = points.len() >= 2
            && points
                .windows(2)
                .all(|w| w[0].0 < w[1].0 && w[0].1 <= w[1].1)
            && points
                .iter()
                .all(|p| p.0.is_finite() && p.1.is_finite() && p.1 >= 0.0);
        ok.then_some(PowerCal { points })
    }

    // [@ANCHOR: radioctl_power_cal_watts]
    /// Watts for a raw reading.
    pub fn watts(&self, raw: f64) -> Power {
        let (first, last) = (self.points[0], self.points[self.points.len() - 1]);
        let w = if raw <= first.0 {
            first.1
        } else if raw >= last.0 {
            last.1
        } else {
            self.points
                .windows(2)
                .find(|p| raw <= p[1].0)
                .map_or(last.1, |p| {
                    p[0].1 + (p[1].1 - p[0].1) * (raw - p[0].0) / (p[1].0 - p[0].0)
                })
        };
        Power(w.max(0.0))
    }

    // [@ANCHOR: radioctl_power_cal_full_scale]
    /// The power at the last calibration point.
    pub fn full_scale(&self) -> Power {
        Power(self.points[self.points.len() - 1].1)
    }
}

/// How long a reading taken from the rig's own meter stays good.
pub const RIG_READING_VALID_FOR: Duration = Duration::from_millis(1500);

// [@ANCHOR: radioctl_power_rig_reading]
/// The reading for a raw 0 to 255 deflection of the rig's own power meter: watts and the full
/// scale when the description has a calibration, else a 0 to 100 relative bar flagged `relative`
/// (never watts without a calibration).
pub fn rig_reading(raw: u16, cal: Option<&PowerCal>) -> PowerReading {
    let raw = f64::from(raw.min(255));
    match cal {
        Some(cal) => PowerReading {
            forward: cal.watts(raw),
            reflected: None,
            origin: PowerOrigin::Rig,
            valid_for: RIG_READING_VALID_FOR,
            relative: false,
            scale: Some(cal.full_scale()),
        },
        None => PowerReading {
            forward: Power(raw / 255.0 * 100.0),
            reflected: None,
            origin: PowerOrigin::Rig,
            valid_for: RIG_READING_VALID_FOR,
            relative: true,
            scale: Some(Power(100.0)),
        },
    }
}

/// The unit a displayed power is shown in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisplayUnit {
    /// Milliwatts.
    Milliwatts,
    /// Watts.
    Watts,
    /// Kilowatts.
    Kilowatts,
}

/// Hysteresis around the unit thresholds (fraction).
const HYSTERESIS: f64 = 0.05;

// [@ANCHOR: radioctl_power_display_unit]
/// The display unit for `watts`, given the unit shown last. Up-switches happen at exactly 1 W and
/// 1000 W; down-switches only once the value is 5 percent below the threshold (0.95 W, 950 W), so
/// a reading wobbling around a threshold does not flip the unit.
pub fn display_unit(watts: f64, previous: Option<DisplayUnit>) -> DisplayUnit {
    use DisplayUnit::*;
    let up = |t: f64| watts >= t;
    let down = |t: f64| watts < t * (1.0 - HYSTERESIS);
    match previous {
        None => {
            if up(1000.0) {
                Kilowatts
            } else if up(1.0) {
                Watts
            } else {
                Milliwatts
            }
        }
        Some(Kilowatts) => {
            if down(1000.0) {
                display_unit(watts, None)
            } else {
                Kilowatts
            }
        }
        Some(Watts) => {
            if up(1000.0) {
                Kilowatts
            } else if down(1.0) {
                Milliwatts
            } else {
                Watts
            }
        }
        Some(Milliwatts) => {
            if up(1000.0) {
                Kilowatts
            } else if up(1.0) {
                Watts
            } else {
                Milliwatts
            }
        }
    }
}

// [@ANCHOR: radioctl_power_format]
/// The text for a power: three significant digits, a fixed number of decimals per unit range so
/// the field does not jump. Milliwatts `250 mW`, `5.00 mW`... below 10 mW two decimals; below
/// 100 mW one; else none. Watts `100 W`, `12.5 W`, `1.50 W` (the same rule on the watt figure).
/// Kilowatts always two decimals `1.50 kW`. `None` is `--`. Returns the text and the unit used,
/// to pass back as `previous`.
pub fn format_power(power: Option<Power>, previous: Option<DisplayUnit>) -> (String, DisplayUnit) {
    let Some(p) = power else {
        return ("--".to_string(), previous.unwrap_or(DisplayUnit::Watts));
    };
    let w = p.watts();
    let unit = display_unit(w, previous);
    let (value, name) = match unit {
        DisplayUnit::Milliwatts => (w * 1000.0, "mW"),
        DisplayUnit::Watts => (w, "W"),
        DisplayUnit::Kilowatts => (w / 1000.0, "kW"),
    };
    let text = if unit == DisplayUnit::Kilowatts {
        format!("{value:.2} {name}")
    } else {
        // Three significant digits; rounding that carries to the next decade drops a decimal.
        let decimals = if value < 9.995 {
            2
        } else if value < 99.95 {
            1
        } else {
            0
        };
        format!("{value:.decimals$} {name}")
    };
    (text, unit)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() <= 1e-9 * b.abs().max(1e-9)
    }

    // Tests [@ANCHOR: radioctl_power_from_dbm]
    // Tests [@ANCHOR: radioctl_power_watts]
    #[test]
    fn dbm_points_convert_to_watts() {
        for (dbm, w) in [
            (0.0, 0.001),
            (30.0, 1.0),
            (40.0, 10.0),
            (60.0, 1000.0),
            (-10.0, 0.0001),
            (-30.0, 1e-6),
        ] {
            assert!(
                close(Power::from_dbm(dbm).expect("finite").watts(), w),
                "{dbm}"
            );
        }
    }

    // Tests [@ANCHOR: radioctl_power_dbm]
    #[test]
    fn dbm_round_trips() {
        for dbm in [-20.0, 0.0, 17.5, 47.0, 63.0] {
            let back = Power::from_dbm(dbm).expect("finite").dbm();
            assert!((back - dbm).abs() < 1e-9, "{dbm}");
        }
        assert_eq!(Power::ZERO.dbm(), f64::NEG_INFINITY);
    }

    // Tests [@ANCHOR: radioctl_power_from_dbw]
    // Tests [@ANCHOR: radioctl_power_from_dbmv_50]
    #[test]
    fn dbw_and_dbmv_convert() {
        assert!(close(Power::from_dbw(0.0).expect("ok").watts(), 1.0));
        assert!(close(Power::from_dbw(30.0).expect("ok").watts(), 1000.0));
        // 0 dBmV is 1 mV across 50 ohms: 20 nanowatts, about -46.99 dBm.
        let p = Power::from_dbmv_50ohm(0.0).expect("ok");
        assert!(close(p.watts(), 2e-8));
        assert!((p.dbm() + 46.9897).abs() < 1e-3);
        // 46.9897 dBmV is 0 dBm (0.2236 V across 50 ohms).
        assert!((Power::from_dbmv_50ohm(46.9897).expect("ok").dbm()).abs() < 1e-3);
    }

    // Tests [@ANCHOR: radioctl_power_from_watts]
    #[test]
    fn bad_figures_are_not_power() {
        assert!(Power::from_watts(-1.0).is_none());
        assert!(Power::from_watts(f64::NAN).is_none());
        assert!(Power::from_watts(f64::INFINITY).is_none());
        assert!(Power::from_dbm(f64::NAN).is_none());
        assert!(Power::from_dbm(f64::NEG_INFINITY).expect("zero").watts() == 0.0);
    }

    // Tests [@ANCHOR: radioctl_power_offset_db]
    // Tests [@ANCHOR: radioctl_power_unit_to_power]
    #[test]
    fn the_coupler_offset_is_applied_before_conversion() {
        // 10 dBm at the meter behind 40 dB of coupler and attenuator is 50 dBm: 100 W.
        let p = PowerUnit::Dbm.to_power(10.0, 40.0).expect("ok");
        assert!(close(p.watts(), 100.0));
        // The same loss on a watts figure: 1 mW behind 30 dB is 1 W.
        let p = PowerUnit::Watts.to_power(0.001, 30.0).expect("ok");
        assert!(close(p.watts(), 1.0));
        // dBW and dBmV take it the same way.
        assert!(close(
            PowerUnit::Dbw.to_power(-10.0, 30.0).expect("ok").watts(),
            100.0
        ));
        let q = PowerUnit::DbmvOn50Ohm.to_power(46.9897, 30.0).expect("ok");
        assert!((q.dbm() - 30.0).abs() < 1e-3);
        // A QRP level stays in range.
        assert!(close(
            PowerUnit::Dbm.to_power(-20.0, 0.0).expect("ok").watts(),
            1e-5
        ));
    }

    // Tests [@ANCHOR: radioctl_power_unit_name]
    // Tests [@ANCHOR: radioctl_power_unit_from_name]
    // Tests [@ANCHOR: radioctl_power_origin_label]
    #[test]
    fn unit_and_origin_names_round_trip() {
        for u in [
            PowerUnit::Watts,
            PowerUnit::Dbm,
            PowerUnit::Dbw,
            PowerUnit::DbmvOn50Ohm,
        ] {
            assert_eq!(PowerUnit::from_name(u.name()), Some(u));
        }
        assert_eq!(PowerUnit::from_name("mw"), None);
        assert_eq!(PowerOrigin::Rig.label(), "rig");
        assert_eq!(PowerOrigin::External("lp".into()).label(), "external");
    }

    // Tests [@ANCHOR: radioctl_power_cal_new]
    // Tests [@ANCHOR: radioctl_power_cal_watts]
    // Tests [@ANCHOR: radioctl_power_cal_full_scale]
    #[test]
    fn a_calibration_interpolates_and_stops_at_the_ends() {
        let cal = PowerCal::new(vec![(0.0, 0.0), (100.0, 50.0), (200.0, 100.0)]).expect("valid");
        assert_eq!(cal.watts(0.0).watts(), 0.0);
        assert!(close(cal.watts(50.0).watts(), 25.0));
        assert!(close(cal.watts(150.0).watts(), 75.0));
        assert!(close(cal.watts(255.0).watts(), 100.0));
        assert!(close(cal.full_scale().watts(), 100.0));
        assert!(PowerCal::new(vec![(0.0, 0.0)]).is_none());
        assert!(PowerCal::new(vec![(0.0, 0.0), (0.0, 1.0)]).is_none());
        assert!(PowerCal::new(vec![(0.0, 5.0), (1.0, 1.0)]).is_none());
    }

    fn text(w: f64, prev: Option<DisplayUnit>) -> String {
        format_power(Power::from_watts(w), prev).0
    }

    // Tests [@ANCHOR: radioctl_power_format]
    #[test]
    fn the_formatter_picks_unit_and_decimals() {
        assert_eq!(text(0.25, None), "250 mW");
        assert_eq!(text(0.005, None), "5.00 mW");
        assert_eq!(text(0.0125, None), "12.5 mW");
        assert_eq!(text(1.0, None), "1.00 W");
        assert_eq!(text(12.5, None), "12.5 W");
        assert_eq!(text(100.0, None), "100 W");
        assert_eq!(text(999.0, None), "999 W");
        assert_eq!(text(1000.0, None), "1.00 kW");
        assert_eq!(text(1500.0, None), "1.50 kW");
        assert_eq!(text(0.0, None), "0.00 mW");
        assert_eq!(format_power(None, None).0, "--");
        // Rounding that would carry past three digits drops a decimal.
        assert_eq!(text(99.96, Some(DisplayUnit::Watts)), "100 W");
    }

    // Tests [@ANCHOR: radioctl_power_display_unit]
    #[test]
    fn the_unit_does_not_flicker_around_a_threshold() {
        let mut unit = None;
        let mut seen = Vec::new();
        for w in [0.5, 0.99, 1.0, 0.99, 0.96, 1.01, 0.94, 0.5] {
            let (_, u) = format_power(Power::from_watts(w), unit);
            unit = Some(u);
            seen.push(u);
        }
        use DisplayUnit::*;
        assert_eq!(
            seen,
            [Milliwatts, Milliwatts, Watts, Watts, Watts, Watts, Milliwatts, Milliwatts]
        );
        let mut unit = Some(DisplayUnit::Watts);
        for (w, want) in [
            (999.0, Watts),
            (1000.0, Kilowatts),
            (990.0, Kilowatts),
            (951.0, Kilowatts),
            (949.0, Watts),
        ] {
            unit = Some(display_unit(w, unit));
            assert_eq!(unit, Some(want), "{w}");
        }
        // A jump of several decades skips the middle unit.
        assert_eq!(display_unit(2000.0, Some(Milliwatts)), Kilowatts);
        assert_eq!(display_unit(0.1, Some(Kilowatts)), Milliwatts);
    }

    // Tests [@ANCHOR: radioctl_power_rig_reading]
    #[test]
    fn a_rig_reading_is_watts_only_with_a_calibration() {
        let cal = PowerCal::new(vec![(0.0, 0.0), (143.0, 100.0), (213.0, 200.0)]).expect("valid");
        let r = rig_reading(143, Some(&cal));
        assert!(!r.relative && close(r.forward.watts(), 100.0));
        assert_eq!(r.scale.map(Power::watts), Some(200.0));
        let r = rig_reading(255, None);
        assert!(r.relative && close(r.forward.watts(), 100.0));
        assert!(rig_reading(0, None).forward.watts() == 0.0);
        assert_eq!(r.origin, PowerOrigin::Rig);
    }

    struct Fixed;
    impl RfPowerReader for Fixed {
        fn read(&self) -> Option<PowerReading> {
            Some(PowerReading {
                forward: Power::from_watts(5.0)?,
                reflected: None,
                origin: PowerOrigin::Rig,
                valid_for: Duration::from_secs(1),
                relative: false,
                scale: None,
            })
        }
    }

    // Tests [@ANCHOR: radioctl_power_reader_read]
    #[test]
    fn a_reader_hands_back_a_reading() {
        let r = Fixed.read().expect("reading");
        assert_eq!(r.forward.watts(), 5.0);
        assert_eq!(r.origin.label(), "rig");
    }
}
