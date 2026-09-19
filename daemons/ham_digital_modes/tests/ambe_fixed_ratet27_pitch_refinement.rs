// SPDX-License-Identifier: LGPL-3.0-or-later
//! Cross-checks `ambe::fixed::ratet27::pitch_refinement` against the float sibling on real speech:
//! both are handed the same initial pitch (from the fixed stage-1 tracker) for the same frame, and
//! the refined periods and the refinement error `E_R` at the chosen period are compared.
//!
//! By default the first 500 frames of each file are used (2000 frames); set
//! `MAX_FRAMES_PER_FILE`/`FRAME_STRIDE` to widen. Full run (all 7748 frames): refined period identical
//! to the eighth of a sample on 99.99% (one frame in 7748 differs, by one eighth), worst relative
//! `E_R` error at the chosen period 7.0e-5.

mod common;

use ham_digital_modes::ambe::fixed::ratet27::pitch as fx_pitch;
use ham_digital_modes::ambe::fixed::ratet27::pitch_refinement as fx;
use ham_digital_modes::ambe::float::ratet27::pitch_refinement as fl;
use std::f64::consts::PI;

fn env_usize(name: &str, default: usize) -> usize {
    std::env::var(name).ok().and_then(|v| v.parse().ok()).unwrap_or(default)
}

#[test]
fn fixed_pitch_refinement_matches_float_on_real_speech() {
    let max_frames = env_usize("MAX_FRAMES_PER_FILE", 500);
    let stride = env_usize("FRAME_STRIDE", 1);
    let (mut frames, mut same_p8, mut within_one_eighth) = (0usize, 0usize, 0usize);
    let mut worst_err_rel = 0.0f64;
    let mut err_pairs = 0usize;
    for path in common::OSR_FILES {
        let pcm = common::read_wav_mono_i16(path);
        let raw_i: Vec<i32> = pcm.iter().map(|&s| s as i32).collect();
        let raw_f: Vec<f64> = pcm.iter().map(|&s| s as f64).collect();
        let mut centers = common::frame_centers(raw_f.len(), 200);
        centers.truncate(max_frames + 2);
        let tables: Vec<fx_pitch::ErrorTable> =
            centers.iter().map(|&c| fx_pitch::PitchAnalysisFrame::new(&raw_i, c).error_table()).collect();
        let mut hist = [(fx_pitch::DEFAULT_PITCH_INDEX, 0i32); 2];
        for k in 0..centers.len().saturating_sub(2) {
            let (ib, cb) = fx_pitch::look_back_pitch_tracking(&tables[k], hist[0], hist[1]);
            let (jf, cf) = fx_pitch::look_ahead_pitch_tracking(&tables[k], &tables[k + 1], &tables[k + 2]);
            let idx = fx_pitch::choose_initial_pitch_estimate(ib, cb, jf, cf);
            hist = [(idx, tables[k].at(idx)), hist[0]];
            if k % stride != 0 {
                continue;
            }
            let c = centers[k];
            let ff = fl::RefinementFrame::new(&raw_f, c);
            let xf = fx::RefinementFrame::new(&raw_i, c);
            let omega_float = fl::refine_pitch(&ff, 21.0 + 0.5 * idx as f64);
            let p8_float = (2.0 * PI / omega_float * 8.0).round() as i64;
            let p8_fixed = fx::refine_pitch(&xf, idx) as i64;
            frames += 1;
            if p8_float == p8_fixed {
                same_p8 += 1;
            }
            if (p8_float - p8_fixed).abs() <= 1 {
                within_one_eighth += 1;
            }
            // E_R at the float side's chosen period, both implementations.
            let e_float = fl::refinement_error(&ff, omega_float);
            let e_fixed = fx::refinement_error(&xf, p8_float as u32) as f64 / 2f64.powi(60);
            if e_float > 1.0 {
                worst_err_rel = worst_err_rel.max(((e_fixed - e_float) / e_float).abs());
                err_pairs += 1;
            }
            // omega0 conversion sanity (Q30 vs the float value at the same period).
            let w = fx::omega0_q30_from_p8(p8_fixed as u32) as f64 / 2f64.powi(30);
            assert!((w - 2.0 * PI * 8.0 / p8_fixed as f64).abs() < 1e-8);
        }
    }
    let pct = |n: usize| 100.0 * n as f64 / frames as f64;
    eprintln!(
        "frames={frames}: refined period identical to the eighth of a sample on {:.2}%, within 1/8 sample on {:.2}%; \
         worst relative E_R error at the chosen period {:.2e} over {err_pairs} frames",
        pct(same_p8),
        pct(within_one_eighth),
        worst_err_rel,
    );
    assert!(frames >= 300);
    assert!(pct(same_p8) >= 99.0, "same period {}", pct(same_p8));
    assert!(pct(within_one_eighth) >= 99.0);
    assert!(worst_err_rel < 1e-3, "E_R relative error {worst_err_rel}");
}

#[test]
fn integer_band_arithmetic_matches_the_real_definitions() {
    // a_l = 256 (l - 1/2) / P; check ceil against a direct rational computation for every P8 the
    // refinement can reach and the harmonic range in use.
    for p8 in 159u32..=985 {
        for l in 0..=57i32 {
            let a = 1024 * (2 * l as i64 - 1);
            let expected = (a as f64 / p8 as f64).ceil() as i32;
            assert_eq!(fx::band_start(l, p8), expected, "p8={p8} l={l}");
        }
    }
    // The 16384-point window DFT index stays inside the table for every reachable bin.
    for p8 in 159u32..=985 {
        for l in 1..=57i32 {
            let (lo, hi) = (fx::band_start(l, p8), fx::band_start(l + 1, p8));
            for m in lo..hi {
                assert!(fx::window_index(m, l, p8).abs() <= 512, "p8={p8} l={l} m={m}");
            }
        }
    }
}
