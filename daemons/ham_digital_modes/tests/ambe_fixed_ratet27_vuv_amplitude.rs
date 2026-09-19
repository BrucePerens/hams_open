// SPDX-License-Identifier: LGPL-3.0-or-later
//! Cross-checks the fixed-point voicing decision and spectral amplitude estimation
//! (`ambe::fixed::ratet27::{vuv, spectral_amplitude}`, `ambe::fixed::mbe_encode::analyze_at_pitch`)
//! against the float `ambe::float::mbe_encode::analyze_at_pitch` on real speech.
//!
//! Each frame is analysed at the fixed pipeline's refined pitch, converted to an arbitrary `omega0`
//! (as a D-STAR / AMBE+2 encoder would after quantizing the pitch) and rounded to Q30 (or Q16.16 in the
//! second run); the float side gets exactly that rounded `omega0`, and the frame's own initial pitch
//! error. Both sides carry their own `xi_max` / previous-band state across consecutive frames.
//!
//! Reported: per-harmonic voicing agreement, the share of frames whose whole voicing pattern is
//! identical, and the amplitude error in dB over harmonics whose float amplitude exceeds 1.0 (one PCM
//! count; below that, the fixed Q16 output and the float differ only in absolute terms, reported too),
//! over harmonics where both sides made the same voicing decision.
//!
//! Measured over all 7748 frames (179016 harmonics; float marks 33.6% voiced), `MAX_FRAMES_PER_FILE=100000`:
//! voicing agrees on 99.998% of harmonics (4 harmonics, in a single frame; 99.99% of frames
//! identical) for Q30 `omega0`, and on 100.000% for Q16.16 `omega0`; amplitudes agree to a worst
//! 0.0007 dB (mean 0.00003 dB) where voicing agrees, and to 0.0001 absolute below 1.0.
//! By default the first 500 frames of each file are used; set `MAX_FRAMES_PER_FILE` to widen.

mod common;

use ham_digital_modes::ambe::fixed::mbe_encode as fx_mbe;
use ham_digital_modes::ambe::fixed::ratet27::pitch as fx_pitch;
use ham_digital_modes::ambe::fixed::ratet27::pitch_refinement as fx_ref;
use ham_digital_modes::ambe::float::mbe_encode as fl_mbe;
use ham_digital_modes::ambe::float::ratet27::encoder::FrameAnalysis;
use ham_digital_modes::ambe::float::ratet27::pitch_refinement as fl_ref;
use ham_digital_modes::ambe::float::ratet27::vuv::harmonics_count;
use std::f64::consts::PI;

#[derive(Default)]
struct Stats {
    frames: usize,
    harmonics: usize,
    voicing_same: usize,
    voiced_count: usize,
    frames_identical: usize,
    amp_checked: usize,
    amp_db_worst: f64,
    amp_db_sum: f64,
    amp_db_over_half: usize,
    amp_small_abs_worst: f64,
    l_mismatch: usize,
}

fn run(q16_omega0: bool, stats: &mut Stats) {
    let max_frames: usize =
        std::env::var("MAX_FRAMES_PER_FILE").ok().and_then(|v| v.parse().ok()).unwrap_or(500);
    for path in common::OSR_FILES {
        let pcm = common::read_wav_mono_i16(path);
        let raw_i: Vec<i32> = pcm.iter().map(|&s| s as i32).collect();
        let raw_f: Vec<f64> = pcm.iter().map(|&s| s as f64).collect();
        let mut centers = common::frame_centers(raw_f.len(), 200);
        centers.truncate(max_frames + 2);
        let tables: Vec<fx_pitch::ErrorTable> =
            centers.iter().map(|&c| fx_pitch::PitchAnalysisFrame::new(&raw_i, c).error_table()).collect();
        let mut hist = [(fx_pitch::DEFAULT_PITCH_INDEX, 0i32); 2];
        let mut fx_state = fx_mbe::AnalysisState::new();
        let mut fl_state = fl_mbe::AnalysisState::new();
        for k in 0..centers.len().saturating_sub(2) {
            let (ib, cb) = fx_pitch::look_back_pitch_tracking(&tables[k], hist[0], hist[1]);
            let (jf, cf) = fx_pitch::look_ahead_pitch_tracking(&tables[k], &tables[k + 1], &tables[k + 2]);
            let idx = fx_pitch::choose_initial_pitch_estimate(ib, cb, jf, cf);
            let e_init_q16 = tables[k].at(idx);
            hist = [(idx, e_init_q16), hist[0]];

            let c = centers[k];
            let xf = fx_ref::RefinementFrame::new(&raw_i, c);
            let p8 = fx_ref::refine_pitch(&xf, idx);
            // An arbitrary omega0 (not on the eighth-sample grid), rounded as the fixed side holds it.
            let w0_true = 2.0 * PI / (p8 as f64 / 8.0) * 1.0037;
            let (pitch, w0) = if q16_omega0 {
                let q = (w0_true * 65536.0).round() as i32;
                (fx_ref::Pitch::from_omega0_q16(q), q as f64 / 65536.0)
            } else {
                let q = (w0_true * 2f64.powi(30)).round() as i64;
                (fx_ref::Pitch::from_omega0_q30(q), q as f64 / 2f64.powi(30))
            };
            let l = harmonics_count(w0);
            if pitch.harmonics_count() != l {
                stats.l_mismatch += 1;
            }
            let analysis = FrameAnalysis {
                omega0_hat: w0,
                initial_pitch_error: e_init_q16 as f64 / 65536.0,
                refinement: fl_ref::RefinementFrame::new(&raw_f, c),
                slot_samples: Vec::new(),
            };
            let (fl_voiced, fl_ml) = fl_mbe::analyze_at_pitch(&analysis, w0, l, &mut fl_state);
            let (fx_voiced, fx_ml) = fx_mbe::analyze_at_pitch(&xf, e_init_q16, &pitch, l, &mut fx_state);

            stats.frames += 1;
            let mut identical = true;
            for h in 1..=l as usize {
                stats.harmonics += 1;
                stats.voiced_count += usize::from(fl_voiced[h]);
                if fl_voiced[h] == fx_voiced[h] {
                    stats.voicing_same += 1;
                } else {
                    identical = false;
                }
                let a_f = fl_ml[h];
                let a_x = fx_ml[h] as f64 / 65536.0;
                if fl_voiced[h] != fx_voiced[h] {
                    // A borderline voicing flip switches estimator (voiced vs unvoiced), which is a
                    // decision difference already counted above, not an amplitude-arithmetic error.
                } else if a_f > 1.0 {
                    let db = (20.0 * (a_x.max(1e-9) / a_f).log10()).abs();
                    stats.amp_checked += 1;
                    stats.amp_db_sum += db;
                    stats.amp_db_worst = stats.amp_db_worst.max(db);
                    if db > 0.5 {
                        stats.amp_db_over_half += 1;
                    }
                } else {
                    stats.amp_small_abs_worst = stats.amp_small_abs_worst.max((a_x - a_f).abs());
                }
            }
            if identical {
                stats.frames_identical += 1;
            }
        }
    }
}

fn report(label: &str, s: &Stats) {
    eprintln!(
        "{label}: frames={} harmonics={} voicing agreement {:.3}% per harmonic (float marks {:.1}% of harmonics voiced), {:.2}% of frames identical; \
         amplitude (|A|>1.0, {} harmonics): worst {:.5} dB, mean {:.6} dB, {} above 0.5 dB; \
         worst absolute error where |A|<=1.0: {:.5}; L mismatches {}",
        s.frames,
        s.harmonics,
        100.0 * s.voicing_same as f64 / s.harmonics as f64,
        100.0 * s.voiced_count as f64 / s.harmonics as f64,
        100.0 * s.frames_identical as f64 / s.frames as f64,
        s.amp_checked,
        s.amp_db_worst,
        s.amp_db_sum / s.amp_checked.max(1) as f64,
        s.amp_db_over_half,
        s.amp_small_abs_worst,
        s.l_mismatch,
    );
}

#[test]
fn fixed_voicing_and_amplitudes_match_float_on_real_speech_q30_omega0() {
    let mut s = Stats::default();
    run(false, &mut s);
    report("omega0 Q30", &s);
    assert!(s.frames >= 300);
    assert_eq!(s.l_mismatch, 0);
    let voiced_share = s.voiced_count as f64 / s.harmonics as f64;
    assert!((0.05..0.95).contains(&voiced_share), "degenerate voicing mix {voiced_share}");
    assert!(s.voicing_same as f64 / s.harmonics as f64 >= 0.99);
    assert!(s.amp_db_worst < 0.5, "worst amplitude error {} dB", s.amp_db_worst);
    assert!(s.amp_small_abs_worst < 0.01);
}

#[test]
fn fixed_voicing_and_amplitudes_match_float_on_real_speech_q16_omega0() {
    let mut s = Stats::default();
    run(true, &mut s);
    report("omega0 Q16.16", &s);
    assert!(s.frames >= 300);
    assert_eq!(s.l_mismatch, 0);
    assert!(s.voicing_same as f64 / s.harmonics as f64 >= 0.99);
    assert!(s.amp_db_worst < 0.5, "worst amplitude error {} dB", s.amp_db_worst);
}

/// Eq. 41 and 42 at hand-computed values (the same cases as the float sibling's own unit tests).
#[test]
fn xi_max_update_and_energy_dependent_function_match_the_equations() {
    use ham_digital_modes::ambe::fixed::ratet27::vuv::{
        energy_dependent_function_q30, update_xi_max, XI_MAX_FLOOR_Q16,
    };
    let q16 = |v: f64| (v * 65536.0).round() as i128;
    // Branch 1: xi_0 exceeds the previous maximum.
    assert_eq!(update_xi_max(q16(20000.0), q16(30000.0)), q16(25000.0));
    // Branch 2: decayed value above the floor.
    assert_eq!(update_xi_max(q16(100000.0), q16(50000.0)), q16(99500.0));
    // Branch 3: floor wins.
    assert_eq!(update_xi_max(q16(20000.0), 0), XI_MAX_FLOOR_Q16);

    let base = (0.0025 * 20000.0 + 20000.0) / (0.01 * 20000.0 + 20000.0);
    let m = energy_dependent_function_q30(q16(20000.0), q16(20000.0), q16(100.0), q16(10.0)) as f64 / 2f64.powi(30);
    assert!((m - base).abs() < 1e-8, "{m} vs {base}");
    let m2 = energy_dependent_function_q30(q16(20000.0), q16(20000.0), q16(10.0), q16(100.0)) as f64 / 2f64.powi(30);
    let expected2 = base * (10.0f64 / 500.0).sqrt();
    assert!((m2 - expected2).abs() < 1e-8, "{m2} vs {expected2}");
}

/// Eq. 31: `L_hat` at the pitch-range ends and a middle value, exact.
#[test]
fn harmonics_count_matches_eq31() {
    use ham_digital_modes::ambe::fixed::ratet27::pitch_refinement::Pitch;
    assert_eq!(Pitch::from_p8(8 * 60).harmonics_count(), 27);
    assert_eq!(Pitch::from_p8(8 * 122).harmonics_count(), 56);
    assert_eq!(Pitch::from_p8(8 * 21).harmonics_count(), 9);
    // Same results from an arbitrary omega0 form of the same periods.
    for p in [60.0f64, 122.0, 21.0] {
        let q = (2.0 * PI / p * 2f64.powi(30)).round() as i64;
        assert_eq!(Pitch::from_omega0_q30(q).harmonics_count(), harmonics_count(q as f64 / 2f64.powi(30)));
    }
}
