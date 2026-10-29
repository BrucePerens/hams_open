// SPDX-License-Identifier: AGPL-3.0-or-later
//! Instrument calibration: the open/short/load one-port calibration of S11 and the through (response) calibration of S21, the
//! standard kinds the vendor's Help file describes ("calsweep open", "short", "load", "thru").
//!
//! One port. The reflection channel sees a true reflection coefficient G through three error terms: directivity e00, source
//! match e11 and reflection tracking t = e10 e01, so the raw value is `m = e00 + t G / (1 - e11 G)`. Writing `d = e00 e11 - t`
//! gives `m = e00 + e11 G m - d G`, linear in (e00, e11, d): three known standards (open G = +1, short G = -1, load G = 0, or
//! their modelled values) give three equations per frequency, solved here, and `G = (m - e00) / (e11 (m - e00) + t)`.
//!
//! Through. S21 is divided by the S21 measured with the ports joined (a response calibration: the cables and the unit's own
//! frequency response cancel, as does any constant gain difference between the two codecs).
//!
//! The calibration holds its frequency list; a sweep is corrected with the terms interpolated linearly in frequency, and a frequency
//! outside the calibrated range is refused (no extrapolation).

use crate::audio::Complex;

const ONE: Complex = Complex { re: 1.0, im: 0.0 };
const ZERO: Complex = Complex { re: 0.0, im: 0.0 };

/// The reflection coefficient of the ideal standards (an ideal open, short and a 50 ohm load).
pub const IDEAL_OPEN: Complex = ONE;
pub const IDEAL_SHORT: Complex = Complex { re: -1.0, im: 0.0 };
pub const IDEAL_LOAD: Complex = ZERO;

/// A point after correction; a parameter the calibration does not cover is `None`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CalibratedPoint {
    pub freq_hz: f64,
    pub s11: Option<Complex>,
    pub s21: Option<Complex>,
}

/// The one-port error terms at every calibrated frequency.
#[derive(Clone, Debug, PartialEq)]
pub struct OnePort {
    pub e00: Vec<Complex>,
    pub e11: Vec<Complex>,
    pub tracking: Vec<Complex>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Calibration {
    pub freqs: Vec<f64>,
    pub one_port: Option<OnePort>,
    /// The S21 measured with the ports joined.
    pub thru: Option<Vec<Complex>>,
}

/// Solves the 3x3 complex system by Cramer's rule; `None` when it is singular (two standards the same).
fn solve3(a: [[Complex; 3]; 3], b: [Complex; 3]) -> Option<[Complex; 3]> {
    let det = |m: [[Complex; 3]; 3]| {
        m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
            - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
            + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0])
    };
    let d = det(a);
    if d.abs() < 1e-12 {
        return None;
    }
    let mut x = [ZERO; 3];
    for (c, xc) in x.iter_mut().enumerate() {
        let mut m = a;
        for r in 0..3 {
            m[r][c] = b[r];
        }
        *xc = det(m) / d;
    }
    Some(x)
}

/// The three error terms from the raw readings of the open, short and load (with their true reflection coefficients).
pub fn solve_one_port(
    measured: [Complex; 3],
    actual: [Complex; 3],
) -> Result<(Complex, Complex, Complex), String> {
    // [@ANCHOR: vnwa_open_cal_one_port]
    // m = e00 + e11 (G m) - d G, unknowns (e00, e11, d).
    let mut a = [[ZERO; 3]; 3];
    for i in 0..3 {
        a[i] = [ONE, actual[i] * measured[i], ZERO - actual[i]];
    }
    let x =
        solve3(a, measured).ok_or("the three standards are not distinct enough to calibrate")?;
    let (e00, e11, d) = (x[0], x[1], x[2]);
    Ok((e00, e11, e00 * e11 - d))
}

impl Calibration {
    /// Builds a calibration from sweeps of the standards over `freqs`. Any part may be left out: a one-port calibration needs all
    /// of the open, short and load, a through calibration only the through.
    pub fn new(
        freqs: Vec<f64>,
        open: Option<&[Complex]>,
        short: Option<&[Complex]>,
        load: Option<&[Complex]>,
        thru: Option<&[Complex]>,
    ) -> Result<Calibration, String> {
        if freqs.len() < 2 || freqs.windows(2).any(|w| w[1] <= w[0]) {
            return Err("a calibration needs at least two rising frequencies".into());
        }
        let n = freqs.len();
        let check = |name: &str, v: &[Complex]| {
            if v.len() == n {
                Ok(())
            } else {
                Err(format!("{name}: {} points for {n} frequencies", v.len()))
            }
        };
        let one_port = match (open, short, load) {
            (Some(o), Some(s), Some(l)) => {
                check("open", o)?;
                check("short", s)?;
                check("load", l)?;
                let mut t = OnePort {
                    e00: Vec::new(),
                    e11: Vec::new(),
                    tracking: Vec::new(),
                };
                for i in 0..n {
                    let (a, b, c) =
                        solve_one_port([o[i], s[i], l[i]], [IDEAL_OPEN, IDEAL_SHORT, IDEAL_LOAD])
                            .map_err(|e| format!("at {} Hz: {e}", freqs[i]))?;
                    t.e00.push(a);
                    t.e11.push(b);
                    t.tracking.push(c);
                }
                Some(t)
            }
            (None, None, None) => None,
            _ => {
                return Err("a one-port calibration needs the open, short and load together".into())
            }
        };
        let thru = match thru {
            Some(t) => {
                check("thru", t)?;
                if t.iter().any(|z| z.abs() < 1e-9) {
                    return Err("the through measurement has a zero point: no signal".into());
                }
                Some(t.to_vec())
            }
            None => None,
        };
        if one_port.is_none() && thru.is_none() {
            return Err(
                "nothing to calibrate: give the open, short and load, or the through".into(),
            );
        }
        Ok(Calibration {
            freqs,
            one_port,
            thru,
        })
    }

    /// Index and weight of the calibrated frequencies around `f` (linear interpolation).
    fn locate(&self, f: f64) -> Result<(usize, f64), String> {
        let (lo, hi) = (self.freqs[0], *self.freqs.last().unwrap());
        // A hair of tolerance for the endpoints, so a sweep over exactly the calibrated range is accepted.
        if f < lo * (1.0 - 1e-9) || f > hi * (1.0 + 1e-9) {
            return Err(format!(
                "{f} Hz is outside the calibrated range {lo} to {hi} Hz"
            ));
        }
        let i = self
            .freqs
            .partition_point(|x| *x <= f)
            .clamp(1, self.freqs.len() - 1)
            - 1;
        let w = ((f - self.freqs[i]) / (self.freqs[i + 1] - self.freqs[i])).clamp(0.0, 1.0);
        Ok((i, w))
    }

    fn lerp(v: &[Complex], i: usize, w: f64) -> Complex {
        let k = |z: Complex, s: f64| Complex {
            re: z.re * s,
            im: z.im * s,
        };
        k(v[i], 1.0 - w) + k(v[i + 1], w)
    }

    /// The corrected S11 of a raw reading at `f`; an error if there is no one-port calibration or `f` is outside its range.
    pub fn s11(&self, f: f64, raw: Complex) -> Result<Complex, String> {
        let t = self
            .one_port
            .as_ref()
            .ok_or("this calibration has no open/short/load part")?;
        let (i, w) = self.locate(f)?;
        let (e00, e11, tr) = (
            Self::lerp(&t.e00, i, w),
            Self::lerp(&t.e11, i, w),
            Self::lerp(&t.tracking, i, w),
        );
        let m = raw - e00;
        Ok(m / (e11 * m + tr))
    }

    /// The corrected S21 of a raw reading at `f`.
    pub fn s21(&self, f: f64, raw: Complex) -> Result<Complex, String> {
        let t = self
            .thru
            .as_ref()
            .ok_or("this calibration has no through part")?;
        let (i, w) = self.locate(f)?;
        Ok(raw / Self::lerp(t, i, w))
    }

    /// Both parameters of a measured point, each corrected if the calibration has its part (`None` otherwise).
    pub fn correct(&self, p: &crate::sweep::Point) -> Result<CalibratedPoint, String> {
        Ok(CalibratedPoint {
            freq_hz: p.freq_hz,
            s11: if self.one_port.is_some() {
                Some(self.s11(p.freq_hz, p.s11)?)
            } else {
                None
            },
            s21: if self.thru.is_some() {
                Some(self.s21(p.freq_hz, p.s21)?)
            } else {
                None
            },
        })
    }

    /// The calibration as text: a header line, the frequency count, then one line per frequency with the one-port terms and the
    /// through, each as re im (a part that is absent is written as the word `none` on its own line).
    pub fn to_text(&self) -> String {
        let mut s = format!("vnwa_open calibration 1\n{}\n", self.freqs.len());
        let z = |c: &Complex| format!("{:e} {:e}", c.re, c.im);
        s.push_str(if self.one_port.is_some() {
            "onport yes\n"
        } else {
            "onport no\n"
        });
        s.push_str(if self.thru.is_some() {
            "thru yes\n"
        } else {
            "thru no\n"
        });
        for i in 0..self.freqs.len() {
            let mut line = format!("{:e}", self.freqs[i]);
            if let Some(t) = &self.one_port {
                line += &format!(" {} {} {}", z(&t.e00[i]), z(&t.e11[i]), z(&t.tracking[i]));
            }
            if let Some(t) = &self.thru {
                line += &format!(" {}", z(&t[i]));
            }
            s.push_str(&line);
            s.push('\n');
        }
        s
    }

    /// Reads what `to_text` wrote; any malformed line is an error that names it.
    pub fn from_text(text: &str) -> Result<Calibration, String> {
        // [@ANCHOR: vnwa_open_cal_file]
        let mut lines = text.lines();
        if lines.next() != Some("vnwa_open calibration 1") {
            return Err("not a vnwa_open calibration file".into());
        }
        let n: usize = lines
            .next()
            .and_then(|l| l.parse().ok())
            .ok_or("missing point count")?;
        let flag = |l: Option<&str>, key: &str| match l {
            Some(x) if x == format!("{key} yes") => Ok(true),
            Some(x) if x == format!("{key} no") => Ok(false),
            _ => Err(format!("missing {key} line")),
        };
        let (has_one, has_thru) = (flag(lines.next(), "onport")?, flag(lines.next(), "thru")?);
        let (mut freqs, mut e00, mut e11, mut tr, mut th) =
            (Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new());
        for (ln, line) in lines.enumerate() {
            let v: Vec<f64> = line
                .split_whitespace()
                .map(|x| {
                    x.parse::<f64>()
                        .map_err(|_| format!("line {}: bad number {x:?}", ln + 5))
                })
                .collect::<Result<_, _>>()?;
            let want = 1 + if has_one { 6 } else { 0 } + if has_thru { 2 } else { 0 };
            if v.len() != want || v.iter().any(|x| !x.is_finite()) {
                return Err(format!(
                    "line {}: {} numbers, expected {want}",
                    ln + 5,
                    v.len()
                ));
            }
            let c = |k: usize| Complex {
                re: v[k],
                im: v[k + 1],
            };
            freqs.push(v[0]);
            let mut k = 1;
            if has_one {
                e00.push(c(k));
                e11.push(c(k + 2));
                tr.push(c(k + 4));
                k += 6;
            }
            if has_thru {
                th.push(c(k));
            }
        }
        if freqs.len() != n {
            return Err(format!("{} points, header says {n}", freqs.len()));
        }
        if freqs.len() < 2 || freqs.windows(2).any(|w| w[1] <= w[0]) {
            return Err("frequencies are not rising".into());
        }
        Ok(Calibration {
            freqs,
            one_port: has_one.then_some(OnePort {
                e00,
                e11,
                tracking: tr,
            }),
            thru: has_thru.then_some(th),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(re: f64, im: f64) -> Complex {
        Complex { re, im }
    }

    /// A made-up unit: frequency-dependent directivity, source match and tracking.
    fn terms(f: f64) -> (Complex, Complex, Complex) {
        let x = f / 1e8;
        (
            c(0.03 + 0.01 * x, -0.02 * x),
            c(0.08 - 0.02 * x, 0.05 + 0.01 * x),
            c(0.9 - 0.1 * x, 0.2 * x),
        )
    }

    fn raw(f: f64, g: Complex) -> Complex {
        let (e00, e11, t) = terms(f);
        e00 + t * g / (ONE - e11 * g)
    }

    fn freqs() -> Vec<f64> {
        (0..11).map(|i| 1e6 + i as f64 * 15e6).collect()
    }

    // Tests [@ANCHOR: vnwa_open_cal_one_port]
    #[test]
    fn the_one_port_terms_are_recovered_and_a_known_load_is_corrected() {
        let f = freqs();
        let sweep = |g: Complex| f.iter().map(|x| raw(*x, g)).collect::<Vec<_>>();
        let cal = Calibration::new(
            f.clone(),
            Some(&sweep(IDEAL_OPEN)),
            Some(&sweep(IDEAL_SHORT)),
            Some(&sweep(IDEAL_LOAD)),
            None,
        )
        .unwrap();
        let t = cal.one_port.as_ref().unwrap();
        for (i, x) in f.iter().enumerate() {
            let (e00, e11, tr) = terms(*x);
            assert!((t.e00[i] - e00).abs() < 1e-12, "e00 at {x}");
            assert!((t.e11[i] - e11).abs() < 1e-12, "e11 at {x}");
            assert!((t.tracking[i] - tr).abs() < 1e-12, "tracking at {x}");
        }
        // A 75 ohm load (G = 0.2), a 33 ohm load (G = -0.2048) and a reactive one, at calibrated and in-between frequencies.
        for g in [c(0.2, 0.0), c(-0.2048, 0.0), c(0.3, -0.4)] {
            for x in [1e6, 8e6, 52.5e6, 100e6, 151e6] {
                let got = cal.s11(x, raw(x, g)).unwrap();
                // In-between frequencies are interpolated, so the error is small but not zero; at the grid it is exact.
                let tol = if f.contains(&x) { 1e-12 } else { 2e-3 };
                assert!((got - g).abs() < tol, "G {g:?} at {x}: {got:?}");
            }
        }
    }

    #[test]
    fn identical_standards_and_a_wrong_pairing_are_refused() {
        let f = freqs();
        let s: Vec<Complex> = f.iter().map(|x| raw(*x, IDEAL_OPEN)).collect();
        let e = Calibration::new(f.clone(), Some(&s), Some(&s), Some(&s), None).unwrap_err();
        assert!(e.contains("not distinct"), "{e}");
        assert!(Calibration::new(f.clone(), Some(&s), None, None, None).is_err());
        assert!(Calibration::new(f.clone(), None, None, None, None).is_err());
        assert!(Calibration::new(vec![1.0], None, None, None, Some(&s[..1])).is_err());
        assert!(Calibration::new(f, None, None, None, Some(&s[..3])).is_err());
    }

    #[test]
    fn the_through_divides_out_the_path_and_the_range_is_enforced() {
        let f = freqs();
        // A path with a ripple and a 0.8 % gain offset: the through measurement carries all of it.
        let path = |x: f64| {
            c(
                0.4 * (1.0 + 0.1 * (x / 2e7).sin()) * 1.008,
                -0.3 * x / 1.5e8,
            )
        };
        let thru: Vec<Complex> = f.iter().map(|x| path(*x)).collect();
        let cal = Calibration::new(f.clone(), None, None, None, Some(&thru)).unwrap();
        let dut = c(0.5, -0.5);
        for x in &f {
            let got = cal.s21(*x, path(*x) * dut).unwrap();
            assert!((got - dut).abs() < 1e-12);
        }
        assert!(cal.s21(0.5e6, ONE).unwrap_err().contains("outside"));
        assert!(cal.s21(2e8, ONE).unwrap_err().contains("outside"));
        assert!(cal
            .s11(10e6, ONE)
            .unwrap_err()
            .contains("no open/short/load"));
    }

    // Tests [@ANCHOR: vnwa_open_cal_file]
    #[test]
    fn a_calibration_survives_the_file_and_a_damaged_file_is_refused() {
        let f = freqs();
        let sweep = |g: Complex| f.iter().map(|x| raw(*x, g)).collect::<Vec<_>>();
        let thru: Vec<Complex> = f.iter().map(|x| c(0.4 + x / 1e9, -0.1)).collect();
        for cal in [
            Calibration::new(
                f.clone(),
                Some(&sweep(IDEAL_OPEN)),
                Some(&sweep(IDEAL_SHORT)),
                Some(&sweep(IDEAL_LOAD)),
                Some(&thru),
            )
            .unwrap(),
            Calibration::new(f.clone(), None, None, None, Some(&thru)).unwrap(),
            Calibration::new(
                f.clone(),
                Some(&sweep(IDEAL_OPEN)),
                Some(&sweep(IDEAL_SHORT)),
                Some(&sweep(IDEAL_LOAD)),
                None,
            )
            .unwrap(),
        ] {
            let text = cal.to_text();
            assert_eq!(Calibration::from_text(&text).unwrap(), cal);
        }
        let good = Calibration::new(f.clone(), None, None, None, Some(&thru))
            .unwrap()
            .to_text();
        for bad in [
            good.replacen("calibration 1", "calibration 2", 1),
            good.replacen("11\n", "12\n", 1),
            good.replace("thru yes", "thru maybe"),
            good.lines().take(8).collect::<Vec<_>>().join("\n"),
            good.replacen('e', "x", 3),
        ] {
            assert!(Calibration::from_text(&bad).is_err(), "{bad}");
        }
    }
}
