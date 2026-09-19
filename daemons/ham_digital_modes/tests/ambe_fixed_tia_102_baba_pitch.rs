// SPDX-License-Identifier: LGPL-3.0-or-later
//! Cross-checks `ambe::fixed::tia_102_baba::pitch` against `ambe::float::tia_102_baba::pitch` on real speech
//! (the four `tests/fixtures/osr_speech` files, every frame at the codec's 160-sample stride).
//!
//! Two agreement measures are taken, both on the *initial pitch estimate* (candidate index):
//!
//! * **end to end**: each side runs its own look-back history from the start of the file, exactly as a
//!   real analyzer would, so one early disagreement can change later frames' history;
//! * **lockstep**: the fixed side is fed the float side's own previous-frame pitch/error, isolating
//!   the per-frame decision from history divergence.
//!
//! `E(P)` itself is compared over all 203 candidates of every frame.

mod common;

use ham_digital_modes::ambe::fixed::tia_102_baba::pitch as fx;
use ham_digital_modes::ambe::float::tia_102_baba::pitch as fl;

fn index_of(p: f64) -> usize {
    ((p - 21.0) / 0.5).round() as usize
}

fn q16_to_f64(v: i32) -> f64 {
    v as f64 / 65536.0
}

#[derive(Default)]
struct Stats {
    frames: usize,
    e_max_abs: f64,
    e_max_rel_above_0_1: f64,
    e_max_abs_below_0_1: f64,
    e_samples: usize,
    end_to_end_agree: usize,
    lockstep_agree: usize,
    lockstep_b_agree: usize,
    lockstep_f_agree: usize,
}

fn run_file(path: &str, stats: &mut Stats) {
    let pcm = common::read_wav_mono_i16(path);
    let raw_i: Vec<i32> = pcm.iter().map(|&s| s as i32).collect();
    let raw_f: Vec<f64> = pcm.iter().map(|&s| s as f64).collect();
    let mut centers = common::frame_centers(raw_f.len(), 200);
    // The float reference is slow (per-candidate re-summation); the first 500 frames of each file
    // (10 s of speech) keep this test to a few seconds. A full-file run (7748 frames over the four
    // files, `MAX_FRAMES_PER_FILE=100000`) measured: E max abs error 0.00002, max relative error
    // 0.00011 where |E|>0.1; initial pitch agreement 99.87% end-to-end and lockstep.
    let max_frames: usize =
        std::env::var("MAX_FRAMES_PER_FILE").ok().and_then(|v| v.parse().ok()).unwrap_or(500);
    centers.truncate(max_frames + 2);

    let mut fl_hist = [(100.0f64, 0.0f64); 2];
    let mut fx_hist = [(fx::DEFAULT_PITCH_INDEX, 0i32); 2];
    let mut lock_hist = [(100.0f64, 0.0f64); 2]; // float history, mirrored to the fixed side

    let build = |c: usize| {
        let ff = fl::PitchAnalysisFrame::new(&raw_f, c);
        let ftab: Vec<f64> = (0..fx::CANDIDATES).map(|i| ff.error_function(21.0 + 0.5 * i as f64)).collect();
        let xf = fx::PitchAnalysisFrame::new(&raw_i, c);
        (ftab, xf.error_table())
    };
    let mut tabs: Vec<(Vec<f64>, fx::ErrorTable)> = Vec::new();
    for &c in &centers {
        tabs.push(build(c));
    }
    for k in 0..centers.len().saturating_sub(2) {
        let (ft0, xt0) = &tabs[k];
        let (ft1, xt1) = &tabs[k + 1];
        let (ft2, xt2) = &tabs[k + 2];
        for (i, &f) in ft0.iter().enumerate() {
            let x = q16_to_f64(xt0.at(i));
            let abs = (f - x).abs();
            if f.abs() > 0.1 {
                stats.e_max_rel_above_0_1 = stats.e_max_rel_above_0_1.max(abs / f.abs());
            } else {
                stats.e_max_abs_below_0_1 = stats.e_max_abs_below_0_1.max(abs);
            }
            stats.e_max_abs = stats.e_max_abs.max(abs);
            stats.e_samples += 1;
        }
        let float_at = |t: &Vec<f64>| {
            let t = t.clone();
            move |p: f64| t[index_of(p)]
        };
        // Float reference decision from its own history.
        let (pb, ceb) = fl::look_back_pitch_tracking(float_at(ft0), fl_hist[0], fl_hist[1]);
        let (pf, cef) = fl::look_ahead_pitch_tracking(float_at(ft0), float_at(ft1), float_at(ft2));
        let p_fl = fl::choose_initial_pitch_estimate(pb, ceb, pf, cef);
        fl_hist = [(p_fl, ft0[index_of(p_fl)]), fl_hist[0]];
        // Fixed decision from its own history.
        let (ib, cb) = fx::look_back_pitch_tracking(xt0, fx_hist[0], fx_hist[1]);
        let (jf, cf) = fx::look_ahead_pitch_tracking(xt0, xt1, xt2);
        let i_fx = fx::choose_initial_pitch_estimate(ib, cb, jf, cf);
        fx_hist = [(i_fx, xt0.at(i_fx)), fx_hist[0]];
        if i_fx == index_of(p_fl) {
            stats.end_to_end_agree += 1;
        }
        // Lockstep: fixed decision using the float side's own (pre-update) history.
        let to_fx = |h: (f64, f64)| (index_of(h.0), (h.1 * 65536.0).round() as i32);
        let (lb, lcb) = fx::look_back_pitch_tracking(xt0, to_fx(lock_hist[0]), to_fx(lock_hist[1]));
        let (lf, lcf) = fx::look_ahead_pitch_tracking(xt0, xt1, xt2);
        let l_fx = fx::choose_initial_pitch_estimate(lb, lcb, lf, lcf);
        let (fpb, fceb) = fl::look_back_pitch_tracking(float_at(ft0), lock_hist[0], lock_hist[1]);
        let (fpf, fcef) = fl::look_ahead_pitch_tracking(float_at(ft0), float_at(ft1), float_at(ft2));
        let l_fl = fl::choose_initial_pitch_estimate(fpb, fceb, fpf, fcef);
        lock_hist = [(l_fl, ft0[index_of(l_fl)]), lock_hist[0]];
        if l_fx == index_of(l_fl) {
            stats.lockstep_agree += 1;
        }
        if lb == index_of(fpb) {
            stats.lockstep_b_agree += 1;
        }
        if lf == index_of(fpf) {
            stats.lockstep_f_agree += 1;
        }
        stats.frames += 1;
    }
}

#[test]
fn fixed_pitch_tracking_matches_float_on_real_speech() {
    let mut stats = Stats::default();
    for path in common::OSR_FILES {
        run_file(path, &mut stats);
    }
    let pct = |n: usize| 100.0 * n as f64 / stats.frames as f64;
    eprintln!(
        "frames={} E: max abs err {:.5} (max rel err where |E|>0.1: {:.5}, max abs err where |E|<=0.1: {:.5})\n\
         initial pitch agreement: end-to-end {:.2}%  lockstep {:.2}%  (look-back {:.2}%, look-ahead {:.2}%)",
        stats.frames,
        stats.e_max_abs,
        stats.e_max_rel_above_0_1,
        stats.e_max_abs_below_0_1,
        pct(stats.end_to_end_agree),
        pct(stats.lockstep_agree),
        pct(stats.lockstep_b_agree),
        pct(stats.lockstep_f_agree),
    );
    assert!(stats.frames >= 300);
    assert!(stats.e_max_abs < 0.001, "E(P) max abs error {}", stats.e_max_abs);
    assert!(pct(stats.end_to_end_agree) >= 99.0, "end to end {}", pct(stats.end_to_end_agree));
    assert!(pct(stats.lockstep_agree) >= 99.0, "lockstep {}", pct(stats.lockstep_agree));
}

/// Silence scores the worst error everywhere, as in the float sibling; the default history is the
/// spec's 100.0.
#[test]
fn silent_frame_has_unit_error_and_default_history_index_is_pitch_100() {
    let raw = vec![0i32; 500];
    let f = fx::PitchAnalysisFrame::new(&raw, 250);
    let t = f.error_table();
    assert!(t.0.iter().all(|&e| e == 65536));
    assert_eq!(fx::p2_of_index(fx::DEFAULT_PITCH_INDEX), 200);
}

#[test]
fn threshold_constants_are_the_nearest_q16() {
    assert_eq!(fx::q16_ratio(48, 100), (0.48f64 * 65536.0).round() as i32);
    assert_eq!(fx::q16_ratio(85, 100), (0.85f64 * 65536.0).round() as i32);
    assert_eq!(fx::q16_ratio(17, 10), (1.7f64 * 65536.0).round() as i32);
    assert_eq!(fx::q16_ratio(35, 10), (3.5f64 * 65536.0).round() as i32);
    assert_eq!(fx::q16_ratio(5, 100), (0.05f64 * 65536.0).round() as i32);
}
