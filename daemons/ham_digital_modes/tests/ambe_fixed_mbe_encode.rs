// SPDX-License-Identifier: LGPL-3.0-or-later
//! Compares `ambe::fixed::general::mbe_encode::quantize_speech` with the float
//! `ambe::float::mbe_encode::quantize_speech` on thousands of random target frames (L in 9..=56, random previous
//! decoder states, random voicing patterns, smooth random log-amplitude spectra), for D-STAR and (feature-gated)
//! AMBE+2, and round-trips the chosen indices through the fixed and float dequantizers to compare the
//! reconstruction error.
//!
//! Measured (4000 trials per mode and per target kind, fixed seeds, `--nocapture` prints the lines). "Random" targets
//! are smooth random spectra (mostly not representable by the codebooks); "realistic" targets are the float
//! decoder's own reconstruction of random valid indices plus +-0.15 log2 noise.
//! - D-STAR random: b1 same 99.80%, b2..b8 all same 99.75%, all eight same 99.55% (per-field disagreements out of
//!   4000: b1 8, b2 0, b3 2, b4 3, b5 2, b6 1, b7 2, b8 0). Realistic: all eight same 99.80%.
//! - AMBE+2 random: b1 99.88%, b2..b8 99.85%, all eight 99.72%. Realistic: all eight 99.75%.
//! - Every disagreement in b2..b8 is a near-tie: the fixed index is the float's own second-nearest row (checked by
//!   re-running the float search with the float's winning row removed; zero exceptions). `b1` (the V/UV pattern)
//!   disagrees only where two amplitude-weighted scores are within a few Q16.16 LSBs or a slot boundary
//!   `floor(h*16*f0)` falls on the other side of an integer.
//! - Reconstruction (RMS of the reconstructed log2 amplitude minus the target, over all harmonics): random targets
//!   float 3.425 / fixed 3.425 (D-STAR), 2.233 / 2.233 (AMBE+2); realistic targets 0.0861 / 0.0861 and 0.0863 /
//!   0.0863. Mean absolute fixed-vs-float reconstruction difference 3e-4 (random) and 4e-5 (realistic) log2 units.
//!   The large random-target RMS is the codebooks' limit (e.g. the gain-delta table cannot reach every gain), and is
//!   identical for both implementations.

use ham_digital_modes::ambe::fixed::general::fixed_ops::{div_q16, mul_q16, TWO_PI_Q16_16};
use ham_digital_modes::ambe::fixed::general::mbe_encode as fx;
use ham_digital_modes::ambe::fixed::general::mbe_speech::MbeDecoderState;
use ham_digital_modes::ambe::float::mbe_encode as fl;

const TRIALS: usize = 4000;

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn unit(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }
    fn range(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * self.unit()
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

/// Owned copy of a mode's float tables so a winning row can be removed to find the second-nearest.
#[derive(Clone)]
struct OwnedTables {
    vuv: Vec<[bool; 8]>,
    dg: Vec<f64>,
    prba24: Vec<[f64; 3]>,
    prba58: Vec<[f64; 4]>,
    lmprbl: Vec<[u32; 4]>,
    hoc: [Vec<[f64; 4]>; 4],
    even: bool,
}
impl OwnedTables {
    fn view(&self) -> fl::ModeTables<'_> {
        fl::ModeTables {
            vuv: &self.vuv,
            dg: &self.dg,
            prba24: &self.prba24,
            prba58: &self.prba58,
            lmprbl: &self.lmprbl,
            hoc: [&self.hoc[0], &self.hoc[1], &self.hoc[2], &self.hoc[3]],
            hoc_b8_even_only: self.even,
        }
    }
}

const FAR: f64 = 1.0e9;

struct Mode {
    name: &'static str,
    float_tables: OwnedTables,
    l_table: &'static [u32],
    /// The decoder's float fundamental (cycles/sample) for `b0`, and the scale dividing it for the V/UV slot.
    f0: fn(u32) -> f64,
    vuv_scale: f64,
    fixed_tables: fx::ModeTables<'static>,
    /// Fixed-point V/UV fundamental from `w0_q16`, exactly as the mode's fixed decoder computes it.
    fixed_vuv_w0: fn(i32) -> i32,
    /// Float reconstruction: `(b0, indices, prev_l, prev_log2, prev_gamma) -> log2Ml[0..=l]`.
    float_recon: fn(u32, &fx::QuantizedSpeech, u32, &[f64], f64) -> Vec<f64>,
    /// Fixed reconstruction, same shape in Q16.16.
    fixed_recon: fn(u32, &fx::QuantizedSpeech, u32, &[i32], i32) -> Vec<i32>,
    /// Minimum agreement floors (fractions) enforced for `b1` and for exact `b2..b8` together.
    min_b1: f64,
    min_rest_exact: f64,
}

fn q16(x: f64) -> i32 {
    (x * 65536.0).round() as i32
}

struct Trial {
    l: u32,
    b0: u32,
    w0: f64,
    vuv_f0: f64,
    voiced: Vec<bool>,
    ml: Vec<f64>,
    prev_l: u32,
    prev_log2: Vec<f64>,
    prev_gamma: f64,
}

fn random_trial(mode: &Mode, rng: &mut Rng, realistic: bool) -> Trial {
    let l = 9 + rng.below(48) as u32;
    let b0 = mode.l_table.iter().position(|&x| x == l).expect("L in table") as u32;
    let f0 = (mode.f0)(b0);
    let lu = l as usize;

    let tilt = rng.range(-3.0, 1.0);
    let base = rng.range(6.0, 10.0);
    let (r1, r2, p1, p2) = (rng.range(0.0, 1.5), rng.range(0.0, 1.0), rng.range(0.0, 6.3), rng.range(0.0, 6.3));
    let mut ml = vec![0.0; lu + 1];
    for (h, slot) in ml.iter_mut().enumerate().skip(1) {
        let t = h as f64 / lu as f64;
        let lg = base + tilt * t + r1 * (6.0 * t + p1).sin() + r2 * (17.0 * t + p2).sin() + rng.range(-0.3, 0.3);
        *slot = 2f64.powf(lg.clamp(0.0, 12.5));
    }
    let cutoff = rng.below(lu + 3);
    let mut voiced: Vec<bool> = (0..=lu).map(|h| h <= cutoff).collect();
    for v in voiced.iter_mut().skip(1) {
        if rng.unit() < 0.08 {
            *v = !*v;
        }
    }

    let prev_l = 9 + rng.below(48) as u32;
    let pb = rng.range(6.0, 10.0);
    let ptilt = rng.range(-3.0, 1.0);
    let prev_log2: Vec<f64> = (0..=prev_l as usize)
        .map(|h| pb + ptilt * h as f64 / prev_l as f64 + rng.range(-0.5, 0.5))
        .collect();
    let prev_gamma = rng.range(0.0, 8.0);
    if realistic {
        // Analysis-by-synthesis target: what the decoder itself reconstructs for random valid indices, plus a
        // little noise, so the target is representable and the reconstruction error is meaningful.
        let t = &mode.float_tables;
        let mut q = fx::QuantizedSpeech {
            b1: rng.below(t.vuv.len()) as u32,
            b2: rng.below(t.dg.len()) as u32,
            b3: rng.below(t.prba24.len()) as u32,
            b4: rng.below(t.prba58.len()) as u32,
            b5: rng.below(t.hoc[0].len()) as u32,
            b6: rng.below(t.hoc[1].len()) as u32,
            b7: rng.below(t.hoc[2].len()) as u32,
            b8: rng.below(t.hoc[3].len()) as u32,
        };
        if t.even {
            q.b8 &= !1;
        }
        let dec = (mode.float_recon)(b0, &q, prev_l, &prev_log2, prev_gamma);
        let unvc = 0.2046 / (f0 * 2.0 * std::f64::consts::PI).sqrt();
        for h in 1..=lu {
            let lg = dec[h] + rng.range(-0.15, 0.15);
            ml[h] = (0.693 * lg).exp() * if voiced[h] { 1.0 } else { unvc };
        }
    }
    Trial {
        l,
        b0,
        w0: f0 * 2.0 * std::f64::consts::PI,
        vuv_f0: f0 / mode.vuv_scale,
        voiced,
        ml,
        prev_l,
        prev_log2,
        prev_gamma,
    }
}

fn float_quantize(t: &Trial, tables: &OwnedTables) -> fx::QuantizedSpeech {
    let q = fl::quantize_speech(
        &fl::SpeechTarget { l: t.l, w0: t.w0, vuv_f0: t.vuv_f0, voiced: &t.voiced, ml: &t.ml },
        &fl::PrevState { l: t.prev_l, log2_ml: &t.prev_log2, gamma: t.prev_gamma },
        &tables.view(),
    );
    fx::QuantizedSpeech { b1: q.b1, b2: q.b2, b3: q.b3, b4: q.b4, b5: q.b5, b6: q.b6, b7: q.b7, b8: q.b8 }
}

fn fixed_quantize(mode: &Mode, t: &Trial) -> (fx::QuantizedSpeech, Vec<i32>) {
    let f0_table_q16 = q16((mode.f0)(t.b0));
    let w0_q16 = mul_q16(f0_table_q16, TWO_PI_Q16_16);
    let vuv_f0_q16 = div_q16((mode.fixed_vuv_w0)(w0_q16), TWO_PI_Q16_16);
    let ml_q16: Vec<i32> = t.ml.iter().map(|&m| q16(m)).collect();
    let log2_target = fx::target_log2_ml_q16(w0_q16, &t.voiced, &ml_q16);
    let prev_q16: Vec<i32> = t.prev_log2.iter().map(|&v| q16(v)).collect();
    let q = fx::quantize_speech(
        &fx::SpeechTarget { l: t.l, vuv_f0_q16, voiced: &t.voiced, ml_q16: &ml_q16, log2_ml_q16: &log2_target },
        &fx::PrevState { l: t.prev_l, log2_ml_q16: &prev_q16, gamma_q16: q16(t.prev_gamma) },
        &mode.fixed_tables,
    );
    (q, log2_target)
}

/// The float target in the decoder's log2 domain (what the float quantizer aims at).
fn float_target_log2(t: &Trial) -> Vec<f64> {
    let unvc = 0.2046 / t.w0.sqrt();
    (0..=t.l as usize)
        .map(|h| {
            if h == 0 {
                return 0.0;
            }
            let m = t.ml[h].max(1e-3);
            let eff = if t.voiced[h] { m } else { m / unvc };
            eff.ln() / 0.693
        })
        .collect()
}

/// Index of the second-nearest row for field `which` (2..=8 for b2..b8), by poisoning the float's winner.
fn float_second(t: &Trial, tables: &OwnedTables, first: &fx::QuantizedSpeech, which: usize) -> u32 {
    let mut poisoned = tables.clone();
    match which {
        2 => poisoned.dg[first.b2 as usize] = FAR,
        3 => poisoned.prba24[first.b3 as usize] = [FAR; 3],
        4 => poisoned.prba58[first.b4 as usize] = [FAR; 4],
        5 => poisoned.hoc[0][first.b5 as usize] = [FAR; 4],
        6 => poisoned.hoc[1][first.b6 as usize] = [FAR; 4],
        7 => poisoned.hoc[2][first.b7 as usize] = [FAR; 4],
        _ => poisoned.hoc[3][first.b8 as usize] = [FAR; 4],
    }
    let q = float_quantize(t, &poisoned);
    [0, q.b1, q.b2, q.b3, q.b4, q.b5, q.b6, q.b7, q.b8][which]
}

fn fields(q: &fx::QuantizedSpeech) -> [u32; 8] {
    [q.b1, q.b2, q.b3, q.b4, q.b5, q.b6, q.b7, q.b8]
}

fn run(mode: &Mode, seed: u64, realistic: bool) {
    let mut rng = Rng(seed);
    let mut b1_same = 0usize;
    let mut all_rest_same = 0usize;
    let mut all_same = 0usize;
    let mut per_field_diff = [0usize; 8];
    let mut non_neighbour = 0usize;
    let (mut float_sq, mut fixed_sq, mut count) = (0.0f64, 0.0f64, 0usize);
    let mut cross_abs = 0.0f64;

    for _ in 0..TRIALS {
        let t = random_trial(mode, &mut rng, realistic);
        let qf = float_quantize(&t, &mode.float_tables);
        let (qx, _) = fixed_quantize(mode, &t);
        let (ff, fxd) = (fields(&qf), fields(&qx));
        if ff[0] == fxd[0] {
            b1_same += 1;
        }
        if ff[1..] == fxd[1..] {
            all_rest_same += 1;
        }
        if ff == fxd {
            all_same += 1;
        }
        for k in 0..8 {
            if ff[k] != fxd[k] {
                per_field_diff[k] += 1;
                if k >= 1 && float_second(&t, &mode.float_tables, &qf, k + 1) != fxd[k] {
                    non_neighbour += 1;
                    eprintln!("{}: b{} fixed {} float {} is not float's second-nearest", mode.name, k + 1, fxd[k], ff[k]);
                }
            }
        }

        // Round trip through each decoder, against the float target.
        let target = float_target_log2(&t);
        let rf = (mode.float_recon)(t.b0, &qf, t.prev_l, &t.prev_log2, t.prev_gamma);
        let prev_q16: Vec<i32> = t.prev_log2.iter().map(|&v| q16(v)).collect();
        let rx = (mode.fixed_recon)(t.b0, &qx, t.prev_l, &prev_q16, q16(t.prev_gamma));
        for h in 1..=t.l as usize {
            let ef = rf[h] - target[h];
            let ex = rx[h] as f64 / 65536.0 - target[h];
            float_sq += ef * ef;
            fixed_sq += ex * ex;
            cross_abs += (rx[h] as f64 / 65536.0 - rf[h]).abs();
            count += 1;
        }
    }

    let n = TRIALS as f64;
    let (float_rms, fixed_rms) = ((float_sq / count as f64).sqrt(), (fixed_sq / count as f64).sqrt());
    eprintln!(
        "{} (realistic={}): b1 same {:.2}%, b2..b8 all same {:.2}%, all 8 same {:.2}%, per-field diffs {:?}, non-neighbour diffs {}, \
         recon RMS (log2 units) float {:.4} fixed {:.4}, mean |fixed-float recon| {:.5}",
        mode.name,
        realistic,
        100.0 * b1_same as f64 / n,
        100.0 * all_rest_same as f64 / n,
        100.0 * all_same as f64 / n,
        per_field_diff,
        non_neighbour,
        float_rms,
        fixed_rms,
        cross_abs / count as f64
    );
    assert!(b1_same as f64 / n >= mode.min_b1, "{}: b1 agreement {}", mode.name, b1_same as f64 / n);
    assert!(all_rest_same as f64 / n >= mode.min_rest_exact, "{}: b2..b8 agreement {}", mode.name, all_rest_same as f64 / n);
    assert!(all_same as f64 / n >= 0.95, "{}: overall agreement {}", mode.name, all_same as f64 / n);
    assert_eq!(non_neighbour, 0, "{}: every disagreement must be a near-tie (float's second-nearest row)", mode.name);
    if realistic {
        assert!(fixed_rms < 0.6, "{}: representable targets should reconstruct closely, fixed RMS {}", mode.name, fixed_rms);
    }
    assert!(fixed_rms <= float_rms * 1.02 + 0.005, "{}: fixed reconstruction error {} vs float {}", mode.name, fixed_rms, float_rms);
}

mod dstar_mode {
    use super::*;
    use ham_digital_modes::ambe::fixed::dstar::decode as fixed_decode;
    use ham_digital_modes::ambe::fixed::dstar::encode as fixed_encode;
    use ham_digital_modes::ambe::float::dstar::decode as float_decode;
    use ham_digital_modes::ambe::float::dstar::decode::{DStarDecoderState, RawParameters};
    use ham_digital_modes::ambe::float::dstar::tables;

    fn raw(b0: u32, q: &fx::QuantizedSpeech) -> RawParameters {
        RawParameters { b0, b1: q.b1, b2: q.b2, b3: q.b3, b4: q.b4, b5: q.b5, b6: q.b6, b7: q.b7, b8: q.b8 }
    }

    fn float_recon(b0: u32, q: &fx::QuantizedSpeech, prev_l: u32, prev: &[f64], gamma: f64) -> Vec<f64> {
        let mut st = DStarDecoderState { l: prev_l, log2_ml: prev.to_vec(), gamma };
        let d = ham_digital_modes::ambe::float::dstar::encode::pack_raw_parameters(&raw(b0, q));
        float_decode::dequantize(d, &mut st);
        st.log2_ml
    }

    fn fixed_recon(b0: u32, q: &fx::QuantizedSpeech, prev_l: u32, prev: &[i32], gamma: i32) -> Vec<i32> {
        let mut st = MbeDecoderState { l: prev_l, log2_ml_q16: prev.to_vec(), gamma_q16: gamma };
        let d = fixed_encode::pack_raw_parameters(&raw(b0, q));
        fixed_decode::dequantize(d, &mut st);
        st.log2_ml_q16
    }

    fn vuv_w0(w0_q16: i32) -> i32 {
        (((w0_q16 as i64) << 16) / 67_109) as i32 // the fixed decoder's unscaled pitch (F0_CHIP_SCALE = 67109/65536)
    }

    pub fn mode() -> Mode {
        Mode {
            name: "dstar",
            float_tables: OwnedTables {
                vuv: tables::VUV.to_vec(),
                dg: tables::DG.to_vec(),
                prba24: tables::PRBA24.to_vec(),
                prba58: tables::PRBA58.to_vec(),
                lmprbl: tables::LMPRBL.to_vec(),
                hoc: [tables::HOC_B5.to_vec(), tables::HOC_B6.to_vec(), tables::HOC_B7.to_vec(), tables::HOC_B8.to_vec()],
                even: true,
            },
            l_table: &tables::L_TABLE,
            f0: float_decode::f0_from_b0,
            vuv_scale: float_decode::F0_CHIP_SCALE,
            fixed_tables: fixed_encode::mode_tables(),
            fixed_vuv_w0: vuv_w0,
            float_recon,
            fixed_recon,
            min_b1: 0.99,
            min_rest_exact: 0.95,
        }
    }
}

#[test]
fn fixed_dstar_quantizer_matches_float() {
    run(&dstar_mode::mode(), 0x5EED_D57A, false);
    run(&dstar_mode::mode(), 0x5EED_D57B, true);
}

#[cfg(feature = "ambe_plus_2")]
mod ambe_plus_2_mode {
    use super::*;
    use ham_digital_modes::ambe::fixed::ambe_plus_2::decode as fixed_decode;
    use ham_digital_modes::ambe::fixed::ambe_plus_2::encode as fixed_encode;
    use ham_digital_modes::ambe::float::ambe_plus_2::decode as float_decode;
    use ham_digital_modes::ambe::float::ambe_plus_2::decode::{DecoderState, RawParameters};
    use ham_digital_modes::ambe::float::ambe_plus_2::tables;

    fn raw(b0: u32, q: &fx::QuantizedSpeech) -> RawParameters {
        RawParameters { b0, b1: q.b1, b2: q.b2, b3: q.b3, b4: q.b4, b5: q.b5, b6: q.b6, b7: q.b7, b8: q.b8 }
    }

    fn float_recon(b0: u32, q: &fx::QuantizedSpeech, prev_l: u32, prev: &[f64], gamma: f64) -> Vec<f64> {
        let mut st = DecoderState { l: prev_l, log2_ml: prev.to_vec(), gamma };
        float_decode::dequantize(&raw(b0, q), &mut st);
        st.log2_ml
    }

    fn fixed_recon(b0: u32, q: &fx::QuantizedSpeech, prev_l: u32, prev: &[i32], gamma: i32) -> Vec<i32> {
        let mut st = MbeDecoderState { l: prev_l, log2_ml_q16: prev.to_vec(), gamma_q16: gamma };
        fixed_decode::dequantize(&raw(b0, q), &mut st);
        st.log2_ml_q16
    }

    fn table_f0(b0: u32) -> f64 {
        tables::W0_TABLE[b0 as usize]
    }

    pub fn mode() -> Mode {
        Mode {
            name: "ambe_plus_2",
            float_tables: OwnedTables {
                vuv: tables::VUV.to_vec(),
                dg: tables::DG.to_vec(),
                prba24: tables::PRBA24.to_vec(),
                prba58: tables::PRBA58.to_vec(),
                lmprbl: tables::LMPRBL.to_vec(),
                hoc: [tables::HOC_B5.to_vec(), tables::HOC_B6.to_vec(), tables::HOC_B7.to_vec(), tables::HOC_B8.to_vec()],
                even: false,
            },
            l_table: &tables::L_TABLE,
            f0: table_f0,
            vuv_scale: 1.0,
            fixed_tables: fixed_encode::mode_tables(),
            fixed_vuv_w0: |w0| w0,
            float_recon,
            fixed_recon,
            min_b1: 0.99,
            min_rest_exact: 0.95,
        }
    }

    #[test]
    fn fixed_ambe_plus_2_quantizer_matches_float() {
        run(&mode(), 0x5EED_A2B2, false);
        run(&mode(), 0x5EED_A2B3, true);
    }

    #[test]
    fn tone_level_field_matches_float_mapping() {
        // The float `amplitude_field` is private to the encoder; its documented chip points are the ground truth.
        for (a, field) in [(250i64, 0x715u16), (500, 0x725), (1000, 0xea2), (2000, 0xed2), (4000, 0xf12), (8000, 0xf62), (16000, 0xfa2)] {
            let got = fixed_encode::amplitude_field_q16(a << 16);
            assert!((got as i32 - field as i32).abs() <= 1, "amplitude {a}: {got:#x} vs {field:#x}");
        }
        assert_eq!(fixed_encode::amplitude_field_q16(10 << 16), 0x715);
        assert_eq!(fixed_encode::amplitude_field_q16(30000 << 16), 0xfa2);
    }
}

#[test]
fn log2_target_helper_matches_float_formula() {
    let mut rng = Rng(77);
    for _ in 0..500 {
        let l = 9 + rng.below(48);
        let w0 = rng.range(0.05, 0.32);
        let voiced: Vec<bool> = (0..=l).map(|_| rng.unit() < 0.5).collect();
        let ml: Vec<f64> = (0..=l).map(|_| 2f64.powf(rng.range(-3.0, 12.0))).collect();
        let got = fx::target_log2_ml_q16(q16(w0), &voiced, &ml.iter().map(|&m| q16(m)).collect::<Vec<_>>());
        let unvc = 0.2046 / w0.sqrt();
        for h in 1..=l {
            let m = ml[h].max(1e-3);
            let want = (if voiced[h] { m } else { m / unvc }).ln() / 0.693;
            let err = (got[h] as f64 / 65536.0 - want).abs();
            assert!(err < 2e-3 + 1e-4 * want.abs(), "h {h} got {} want {want}", got[h] as f64 / 65536.0);
        }
    }
}
