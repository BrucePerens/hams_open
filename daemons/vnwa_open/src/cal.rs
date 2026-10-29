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

/// The measurement settings a calibration is valid for. A calibration holds only at the settings it was made with; applying it at
/// other ones is refused (`Calibration::check_setup`).
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Setup {
    pub if_hz: f64,
    pub point_ms: f64,
    pub rf_level: u16,
    /// The cables and adapters between the ports and the reference plane, in the operator's words (compared as written).
    pub cable: String,
}

const Z0: f64 = 50.0;

/// One standard's published model: the polynomial of its reactance and the offset line in front of it.
#[derive(Clone, Debug, PartialEq)]
pub struct Offset {
    /// One-way delay of the offset line in seconds and its loss in gigaohms per second at 1 GHz (the usual kit convention).
    pub delay_s: f64,
    pub loss_gohm_s: f64,
}

/// A calibration kit definition: open capacitance coefficients C0 to C3 (F, F/Hz, F/Hz^2, F/Hz^3), short inductance L0 to L3 (H, ...),
/// the load's resistance and inductance, an offset line for each, and the through's delay. The default is the ideal kit.
#[derive(Clone, Debug, PartialEq)]
pub struct KitDefinition {
    pub name: String,
    pub open_c: [f64; 4],
    pub open_offset: Offset,
    pub short_l: [f64; 4],
    pub short_offset: Offset,
    pub load_r_ohm: f64,
    pub load_l_h: f64,
    pub load_offset: Offset,
    pub thru_delay_s: f64,
}

impl Default for KitDefinition {
    fn default() -> KitDefinition {
        let none = Offset {
            delay_s: 0.0,
            loss_gohm_s: 0.0,
        };
        KitDefinition {
            name: "ideal".into(),
            open_c: [0.0; 4],
            open_offset: none.clone(),
            short_l: [0.0; 4],
            short_offset: none.clone(),
            load_r_ohm: Z0,
            load_l_h: 0.0,
            load_offset: none,
            thru_delay_s: 0.0,
        }
    }
}

fn poly(c: &[f64; 4], f: f64) -> f64 {
    c[0] + c[1] * f + c[2] * f * f + c[3] * f * f * f
}

/// The reflection coefficient of impedance `z` against 50 ohms.
fn gamma_of(z: Complex) -> Complex {
    let z0 = Complex { re: Z0, im: 0.0 };
    (z - z0) / (z + z0)
}

/// Moves a reflection coefficient back through an offset line (there and back).
fn through_offset(g: Complex, o: &Offset, f: f64) -> Complex {
    // [@ANCHOR: vnwa_open_cal_through_offset]
    // loss factor per way: exp(-(loss * delay / (2 Z0)) sqrt(f / 1 GHz)), phase -2 pi f delay per way
    let a = o.loss_gohm_s * 1e9 * o.delay_s / (2.0 * Z0) * (f / 1e9).sqrt();
    let p = -2.0 * std::f64::consts::PI * f * o.delay_s;
    let (mag, ang) = ((-2.0 * a).exp(), 2.0 * p);
    g * Complex {
        re: mag * ang.cos(),
        im: mag * ang.sin(),
    }
}

impl Offset {
    pub fn default_none() -> Offset {
        Offset {
            delay_s: 0.0,
            loss_gohm_s: 0.0,
        }
    }
}

impl KitDefinition {
    /// The true reflection coefficients of the open, short and load at `f`, at the reference plane.
    pub fn standards(&self, f: f64) -> [Complex; 3] {
        // [@ANCHOR: vnwa_open_cal_kit]
        let w = 2.0 * std::f64::consts::PI * f;
        // open: Z = 1 / (j w C); an ideal open (C = 0) is G = +1 exactly
        let c = poly(&self.open_c, f);
        let open = if c == 0.0 {
            Complex { re: 1.0, im: 0.0 }
        } else {
            gamma_of(Complex { re: 1.0, im: 0.0 } / Complex { re: 0.0, im: w * c })
        };
        let short = gamma_of(Complex {
            re: 0.0,
            im: w * poly(&self.short_l, f),
        });
        let load = gamma_of(Complex {
            re: self.load_r_ohm,
            im: w * self.load_l_h,
        });
        [
            through_offset(open, &self.open_offset, f),
            through_offset(short, &self.short_offset, f),
            through_offset(load, &self.load_offset, f),
        ]
    }

    /// The ideal through's S21 at `f` (its delay only).
    pub fn thru(&self, f: f64) -> Complex {
        let p = -2.0 * std::f64::consts::PI * f * self.thru_delay_s;
        Complex {
            re: p.cos(),
            im: p.sin(),
        }
    }

    pub fn to_text(&self) -> String {
        // [@ANCHOR: vnwa_open_kit_to_text]
        let l = |k: &str, v: &[f64]| {
            format!(
                "{k} = {}\n",
                v.iter()
                    .map(|x| format!("{x:e}"))
                    .collect::<Vec<_>>()
                    .join(" ")
            )
        };
        let mut s = format!("name = {}\n", self.name);
        s += &l("open_c", &self.open_c);
        s += &l(
            "open_offset",
            &[self.open_offset.delay_s, self.open_offset.loss_gohm_s],
        );
        s += &l("short_l", &self.short_l);
        s += &l(
            "short_offset",
            &[self.short_offset.delay_s, self.short_offset.loss_gohm_s],
        );
        s += &l("load", &[self.load_r_ohm, self.load_l_h]);
        s += &l(
            "load_offset",
            &[self.load_offset.delay_s, self.load_offset.loss_gohm_s],
        );
        s += &l("thru_delay_s", &[self.thru_delay_s]);
        s
    }

    /// Reads a kit file: `key = numbers` lines, `#` comments, any key left out keeps the ideal value; an unknown key is an error.
    pub fn from_text(text: &str) -> Result<KitDefinition, String> {
        // [@ANCHOR: vnwa_open_kit_file]
        let mut k = KitDefinition::default();
        for (n, raw) in text.lines().enumerate() {
            let line = raw.split('#').next().unwrap_or("").trim();
            if line.is_empty() {
                continue;
            }
            let (key, val) = line
                .split_once('=')
                .ok_or(format!("line {}: expected key = value", n + 1))?;
            let (key, val) = (key.trim(), val.trim());
            if key == "name" {
                k.name = val.to_string();
                continue;
            }
            let v: Vec<f64> = val
                .split_whitespace()
                .map(|x| {
                    x.parse::<f64>()
                        .map_err(|_| format!("line {}: bad number {x:?}", n + 1))
                })
                .collect::<Result<_, _>>()?;
            if v.iter().any(|x| !x.is_finite()) {
                return Err(format!("line {}: not a finite number", n + 1));
            }
            let want = |c: usize| {
                if v.len() == c {
                    Ok(())
                } else {
                    Err(format!(
                        "line {}: {key} needs {c} numbers, has {}",
                        n + 1,
                        v.len()
                    ))
                }
            };
            match key {
                "open_c" => {
                    want(4)?;
                    k.open_c.copy_from_slice(&v);
                }
                "short_l" => {
                    want(4)?;
                    k.short_l.copy_from_slice(&v);
                }
                "open_offset" | "short_offset" | "load_offset" => {
                    want(2)?;
                    let o = Offset {
                        delay_s: v[0],
                        loss_gohm_s: v[1],
                    };
                    match key {
                        "open_offset" => k.open_offset = o,
                        "short_offset" => k.short_offset = o,
                        _ => k.load_offset = o,
                    }
                }
                "load" => {
                    want(2)?;
                    if v[0] <= 0.0 {
                        return Err(format!(
                            "line {}: the load resistance must be positive",
                            n + 1
                        ));
                    }
                    k.load_r_ohm = v[0];
                    k.load_l_h = v[1];
                }
                "thru_delay_s" => {
                    want(1)?;
                    k.thru_delay_s = v[0];
                }
                _ => return Err(format!("line {}: unknown key {key:?}", n + 1)),
            }
        }
        Ok(k)
    }
}

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
    /// When it was made (text, as the caller wrote it), the kit and the settings it holds for.
    pub created: String,
    pub kit: KitDefinition,
    pub setup: Setup,
    pub freqs: Vec<f64>,
    pub one_port: Option<OnePort>,
    /// The S21 measured with the ports joined.
    pub thru: Option<Vec<Complex>>,
}

/// Solves the 3x3 complex system by Cramer's rule; `None` when it is singular (two standards the same).
fn solve3(a: [[Complex; 3]; 3], b: [Complex; 3]) -> Option<[Complex; 3]> {
    // [@ANCHOR: vnwa_open_cal_solve3]
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
        Calibration::build(
            freqs,
            open,
            short,
            load,
            thru,
            KitDefinition::default(),
            Setup::default(),
            String::new(),
        )
    }

    /// As `new`, with the kit whose published numbers define the standards, the settings the sweeps were made at and a date.
    #[allow(clippy::too_many_arguments)]
    pub fn build(
        freqs: Vec<f64>,
        open: Option<&[Complex]>,
        short: Option<&[Complex]>,
        load: Option<&[Complex]>,
        thru: Option<&[Complex]>,
        kit: KitDefinition,
        setup: Setup,
        created: String,
    ) -> Result<Calibration, String> {
        // [@ANCHOR: vnwa_open_cal_build]
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
                    let (a, b, c) = solve_one_port([o[i], s[i], l[i]], kit.standards(freqs[i]))
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
                // The measured through contains the kit's through delay: remove it so the response is relative to a zero-length through.
                Some(
                    t.iter()
                        .zip(&freqs)
                        .map(|(z, f)| *z / kit.thru(*f))
                        .collect(),
                )
            }
            None => None,
        };
        if one_port.is_none() && thru.is_none() {
            return Err(
                "nothing to calibrate: give the open, short and load, or the through".into(),
            );
        }
        Ok(Calibration {
            created,
            kit,
            setup,
            freqs,
            one_port,
            thru,
        })
    }

    /// Refuses a calibration at settings other than those it was made at (IF, time per point, RF level, cable set-up).
    pub fn check_setup(&self, now: &Setup) -> Result<(), String> {
        // [@ANCHOR: vnwa_open_cal_valid]
        let mut why = Vec::new();
        if self.setup.if_hz != now.if_hz {
            why.push(format!(
                "IF {} Hz now, {} Hz in the calibration",
                now.if_hz, self.setup.if_hz
            ));
        }
        if self.setup.point_ms != now.point_ms {
            why.push(format!(
                "{} ms per point now, {} ms in the calibration",
                now.point_ms, self.setup.point_ms
            ));
        }
        if self.setup.rf_level != now.rf_level {
            why.push(format!(
                "RF level {} now, {} in the calibration",
                now.rf_level, self.setup.rf_level
            ));
        }
        if self.setup.cable != now.cable {
            why.push(format!(
                "cable set-up {:?} now, {:?} in the calibration",
                now.cable, self.setup.cable
            ));
        }
        if why.is_empty() {
            Ok(())
        } else {
            Err(format!(
                "the calibration (made {}) is not valid at these settings: {}",
                self.created,
                why.join("; ")
            ))
        }
    }

    /// Index and weight of the calibrated frequencies around `f` (linear interpolation).
    fn locate(&self, f: f64) -> Result<(usize, f64), String> {
        // [@ANCHOR: vnwa_open_cal_locate]
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
        // [@ANCHOR: vnwa_open_cal_s11]
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
        // [@ANCHOR: vnwa_open_cal_s21]
        let t = self
            .thru
            .as_ref()
            .ok_or("this calibration has no through part")?;
        let (i, w) = self.locate(f)?;
        Ok(raw / Self::lerp(t, i, w))
    }

    /// Both parameters of a measured point, each corrected if the calibration has its part (`None` otherwise).
    pub fn correct(&self, p: &crate::sweep::Point) -> Result<CalibratedPoint, String> {
        // [@ANCHOR: vnwa_open_cal_correct]
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
        // [@ANCHOR: vnwa_open_cal_to_text]
        let one_line = |t: &str| t.replace(['\n', '\r'], " ");
        let mut s = String::from("vnwa_open calibration 2\n");
        s += &format!("created = {}\n", one_line(&self.created));
        s += &format!("cable = {}\n", one_line(&self.setup.cable));
        s += &format!(
            "settings = {:e} {:e} {}\n",
            self.setup.if_hz, self.setup.point_ms, self.setup.rf_level
        );
        for line in self.kit.to_text().lines() {
            s += &format!("kit {line}\n");
        }
        s += &format!("points = {}\n", self.freqs.len());
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
        if lines.next() != Some("vnwa_open calibration 2") {
            return Err("not a vnwa_open calibration file (version 2)".into());
        }
        let mut field = |key: &str| -> Result<String, String> {
            let l = lines.next().ok_or(format!("missing {key} line"))?;
            l.strip_prefix(&format!("{key} = "))
                .map(String::from)
                .ok_or(format!("expected {key} line, found {l:?}"))
        };
        let created = field("created")?;
        let cable = field("cable")?;
        let st: Vec<String> = field("settings")?
            .split_whitespace()
            .map(String::from)
            .collect();
        if st.len() != 3 {
            return Err("settings line needs IF, ms per point and RF level".into());
        }
        let setup = Setup {
            if_hz: st[0].parse().map_err(|_| "bad IF in settings")?,
            point_ms: st[1]
                .parse()
                .map_err(|_| "bad time per point in settings")?,
            rf_level: st[2].parse().map_err(|_| "bad RF level in settings")?,
            cable,
        };
        let mut kit_text = String::new();
        let mut rest: Vec<&str> = Vec::new();
        for l in lines.by_ref() {
            match l.strip_prefix("kit ") {
                Some(k) => {
                    kit_text += k;
                    kit_text.push('\n');
                }
                None => {
                    rest.push(l);
                    break;
                }
            }
        }
        let kit = KitDefinition::from_text(&kit_text)?;
        let n: usize = rest
            .first()
            .and_then(|l| l.strip_prefix("points = "))
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
            created,
            kit,
            setup,
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
    // Tests [@ANCHOR: vnwa_open_cal_solve3]
    // Tests [@ANCHOR: vnwa_open_cal_build]
    // Tests [@ANCHOR: vnwa_open_cal_s11]
    // Tests [@ANCHOR: vnwa_open_cal_locate]
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

    // Tests [@ANCHOR: vnwa_open_cal_solve3]
    // Tests [@ANCHOR: vnwa_open_cal_build]
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

    // Tests [@ANCHOR: vnwa_open_cal_locate]
    // Tests [@ANCHOR: vnwa_open_cal_s21]
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
    // Tests [@ANCHOR: vnwa_open_cal_to_text]
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
            good.replacen("calibration 2", "calibration 1", 1),
            good.replacen("points = 11", "points = 12", 1),
            good.replace("thru yes", "thru maybe"),
            good.lines().take(20).collect::<Vec<_>>().join("\n"),
            good.replacen("settings = ", "settings = x", 1),
            good.replacen("kit name", "kit nam", 1),
        ] {
            assert!(Calibration::from_text(&bad).is_err(), "{bad}");
        }
    }

    // Tests [@ANCHOR: vnwa_open_cal_kit]
    // Tests [@ANCHOR: vnwa_open_cal_through_offset]
    #[test]
    fn a_real_kit_is_modelled_and_the_calibration_removes_its_imperfections() {
        let kit = KitDefinition {
            name: "test kit".into(),
            open_c: [60e-15, -0.1e-24, 0.0, 0.0],
            open_offset: Offset {
                delay_s: 30e-12,
                loss_gohm_s: 1.0,
            },
            short_l: [5e-12, 0.0, 0.0, 0.0],
            short_offset: Offset {
                delay_s: 30e-12,
                loss_gohm_s: 1.0,
            },
            load_r_ohm: 50.4,
            load_l_h: 0.2e-9,
            load_offset: Offset::default_none(),
            thru_delay_s: 50e-12,
        };
        // The ideal kit gives the ideal standards; this one does not, and its open is a capacitor (phase below 0).
        let ideal = KitDefinition::default().standards(100e6);
        assert_eq!(ideal, [IDEAL_OPEN, IDEAL_SHORT, IDEAL_LOAD]);
        let real = kit.standards(100e6);
        assert!(
            (real[0] - IDEAL_OPEN).abs() > 1e-3
                && real[0].abs() <= 1.0 + 1e-9
                && real[0].phase_deg() < 0.0
        );
        assert!(real[1].abs() <= 1.0 + 1e-9 && real[2].abs() > 0.0);
        // Measure the kit's real standards through the made-up unit, calibrate with the kit definition, and correct a DUT.
        let f = freqs();
        let sweep = |k: usize| {
            f.iter()
                .map(|x| raw(*x, kit.standards(*x)[k]))
                .collect::<Vec<_>>()
        };
        let cal = Calibration::build(
            f.clone(),
            Some(&sweep(0)),
            Some(&sweep(1)),
            Some(&sweep(2)),
            None,
            kit.clone(),
            Setup::default(),
            "x".into(),
        )
        .unwrap();
        let g = c(0.3, -0.4);
        for x in &f {
            let e = (cal.s11(*x, raw(*x, g)).unwrap() - g).abs();
            assert!(e < 1e-7, "at {x}: {e}");
        }
        // Calibrating with the ideal kit instead would leave an error (the point of having the kit).
        let wrong = Calibration::new(
            f.clone(),
            Some(&sweep(0)),
            Some(&sweep(1)),
            Some(&sweep(2)),
            None,
        )
        .unwrap();
        assert!((wrong.s11(f[5], raw(f[5], g)).unwrap() - g).abs() > 1e-4);
        // The through's delay is divided out of the response.
        let thru: Vec<Complex> = f.iter().map(|x| c(0.5, 0.0) * kit.thru(*x)).collect();
        let t = Calibration::build(
            f.clone(),
            None,
            None,
            None,
            Some(&thru),
            kit.clone(),
            Setup::default(),
            "x".into(),
        )
        .unwrap();
        assert!((t.s21(f[3], c(0.5, 0.0)).unwrap() - ONE).abs() < 1e-12);
    }

    // Tests [@ANCHOR: vnwa_open_kit_file]
    // Tests [@ANCHOR: vnwa_open_kit_to_text]
    #[test]
    fn a_kit_file_round_trips_and_bad_ones_are_refused() {
        let kit = KitDefinition {
            name: "Acme 3.5 mm".into(),
            open_c: [49.4e-15, -310e-27, 23.1e-36, -0.16e-45],
            load_r_ohm: 49.9,
            thru_delay_s: 1.2e-10,
            ..KitDefinition::default()
        };
        assert_eq!(KitDefinition::from_text(&kit.to_text()).unwrap(), kit);
        let partial =
            KitDefinition::from_text("# only the load\nload = 50.2 1e-10  # ohms, henries\n")
                .unwrap();
        assert_eq!(
            (partial.load_r_ohm, partial.load_l_h, partial.open_c),
            (50.2, 1e-10, [0.0; 4])
        );
        for bad in [
            "load = 50",
            "frob = 1",
            "open_c = 1 2 3",
            "load = -5 0",
            "load = x y",
            "just words",
            "thru_delay_s = inf",
        ] {
            assert!(KitDefinition::from_text(bad).is_err(), "{bad}");
        }
    }

    // Tests [@ANCHOR: vnwa_open_cal_valid]
    #[test]
    fn a_calibration_is_refused_when_the_settings_or_cables_change_and_keeps_its_metadata() {
        let f = freqs();
        let thru: Vec<Complex> = f.iter().map(|_| c(0.4, 0.1)).collect();
        let setup = Setup {
            if_hz: 4000.0,
            point_ms: 60.0,
            rf_level: 16383,
            cable: "TX pad 10 dB, 3 ft RG-58 to RX".into(),
        };
        let kit = KitDefinition {
            name: "Acme".into(),
            load_r_ohm: 49.9,
            ..KitDefinition::default()
        };
        let cal = Calibration::build(
            f,
            None,
            None,
            None,
            Some(&thru),
            kit.clone(),
            setup.clone(),
            "2026-10-28 23:30".into(),
        )
        .unwrap();
        let back = Calibration::from_text(&cal.to_text()).unwrap();
        assert_eq!(
            (back.created.as_str(), &back.setup, &back.kit),
            ("2026-10-28 23:30", &setup, &kit)
        );
        assert!(back.check_setup(&setup).is_ok());
        for changed in [
            Setup {
                if_hz: 12000.0,
                ..setup.clone()
            },
            Setup {
                point_ms: 30.0,
                ..setup.clone()
            },
            Setup {
                rf_level: 8192,
                ..setup.clone()
            },
            Setup {
                cable: "other cable".into(),
                ..setup.clone()
            },
        ] {
            let e = back.check_setup(&changed).unwrap_err();
            assert!(e.contains("not valid") && e.contains("2026-10-28"), "{e}");
        }
    }

    // Tests [@ANCHOR: vnwa_open_cal_correct]
    #[test]
    fn a_point_is_corrected_in_each_parameter_the_calibration_covers() {
        let f = freqs();
        let sweep = |g: Complex| f.iter().map(|x| raw(*x, g)).collect::<Vec<_>>();
        let path = |x: f64| c(0.4 + 0.1 * (x / 2e7).sin(), -0.3 * x / 1.5e8);
        let thru: Vec<Complex> = f.iter().map(|x| path(*x)).collect();
        let both = Calibration::new(
            f.clone(),
            Some(&sweep(IDEAL_OPEN)),
            Some(&sweep(IDEAL_SHORT)),
            Some(&sweep(IDEAL_LOAD)),
            Some(&thru),
        )
        .unwrap();
        let (g, dut) = (c(0.3, -0.4), c(0.5, -0.5));
        let x = f[4];
        let point = crate::sweep::Point {
            freq_hz: x,
            s11: raw(x, g),
            s21: path(x) * dut,
            if_error_hz: 0.0,
            level_change: 0.0,
        };
        let got = both.correct(&point).unwrap();
        assert_eq!(got.freq_hz, x);
        assert!((got.s11.unwrap() - g).abs() < 1e-12);
        assert!((got.s21.unwrap() - dut).abs() < 1e-12);
        // A calibration with only the through leaves S11 uncorrected (None), and one with only the one-port terms leaves S21.
        let only_thru = Calibration::new(f.clone(), None, None, None, Some(&thru)).unwrap();
        let got = only_thru.correct(&point).unwrap();
        assert!(got.s11.is_none() && got.s21.is_some());
        let only_one_port = Calibration::new(
            f.clone(),
            Some(&sweep(IDEAL_OPEN)),
            Some(&sweep(IDEAL_SHORT)),
            Some(&sweep(IDEAL_LOAD)),
            None,
        )
        .unwrap();
        let got = only_one_port.correct(&point).unwrap();
        assert!(got.s11.is_some() && got.s21.is_none());
        // A frequency outside the calibrated range is an error, not a guess.
        let outside = crate::sweep::Point {
            freq_hz: 5e8,
            ..point
        };
        assert!(both.correct(&outside).is_err());
    }
}
