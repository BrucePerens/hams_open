// SPDX-License-Identifier: LGPL-3.0-or-later
//! Cross-checks the streaming fixed-point `FrameAnalyzer` (`ambe::fixed::ratet27::encoder`) against the
//! float `FrameAnalyzer` on real speech: same input pushed in odd-sized chunks, same frame count and
//! timing, refined period, initial-pitch error, the frame's own samples, and (via
//! `analyze_frame_at_pitch`) the voicing and amplitudes at the frame's own refined pitch.
//!
//! By default the first 5 s of each file (about 250 frames) are used, with center offsets 0 and -37;
//! set `MAX_SECONDS` to widen. The measured numbers for a full run are stated in the module doc of
//! `src/ambe/fixed/ratet27/encoder.rs`.

mod common;

use ham_digital_modes::ambe::fixed::mbe_encode as fx_mbe;
use ham_digital_modes::ambe::fixed::ratet27::encoder as fx;
use ham_digital_modes::ambe::float::mbe_encode as fl_mbe;
use ham_digital_modes::ambe::float::ratet27::encoder as fl;
use ham_digital_modes::ambe::float::ratet27::vuv::harmonics_count;
use std::f64::consts::PI;

#[derive(Default)]
struct Stats {
    frames: usize,
    count_mismatch: usize,
    p8_same: usize,
    slot_same: usize,
    e_close: usize,
    harmonics: usize,
    voicing_same: usize,
    amp_db_worst: f64,
}

fn run(offset: i32, max_samples: usize, stats: &mut Stats) {
    for path in common::OSR_FILES {
        let pcm = common::read_wav_mono_i16(path);
        let pcm = &pcm[..pcm.len().min(max_samples)];
        let mut fixed_analyzer = fx::FrameAnalyzer::new();
        let mut float_analyzer = fl::FrameAnalyzer::new();
        fixed_analyzer.set_center_offset(offset);
        float_analyzer.set_center_offset(offset);
        let (mut fx_frames, mut fl_frames) = (Vec::new(), Vec::new());
        for chunk in pcm.chunks(97) {
            fixed_analyzer.push_samples(chunk);
            let chunk_f: Vec<f64> = chunk.iter().map(|&s| s as f64).collect();
            float_analyzer.push_samples(&chunk_f);
            while let Some(a) = fixed_analyzer.next_analysis() {
                fx_frames.push(a);
            }
            while let Some(a) = float_analyzer.next_analysis() {
                fl_frames.push(a);
            }
        }
        fixed_analyzer.finish_input();
        float_analyzer.finish_input();
        while let Some(a) = fixed_analyzer.next_analysis() {
            fx_frames.push(a);
        }
        while let Some(a) = float_analyzer.next_analysis() {
            fl_frames.push(a);
        }
        if fx_frames.len() != fl_frames.len() {
            stats.count_mismatch += 1;
        }
        let (mut fx_state, mut fl_state) = (fx_mbe::AnalysisState::new(), fl_mbe::AnalysisState::new());
        for (xa, fa) in fx_frames.iter().zip(fl_frames.iter()) {
            stats.frames += 1;
            let p8_float = (2.0 * PI / fa.omega0_hat * 8.0).round() as i64;
            let same_period = p8_float == xa.p8 as i64;
            stats.p8_same += usize::from(same_period);
            stats.slot_same +=
                usize::from(xa.slot_samples.iter().zip(fa.slot_samples.iter()).all(|(&a, &b)| a as f64 == b));
            // E(P_hat_I) can differ by more than rounding when the two sides pick initial pitches a
            // half-sample apart (which the refinement search can still resolve to the same period).
            let e_fx = xa.initial_pitch_error_q16 as f64 / 65536.0;
            stats.e_close += usize::from((e_fx - fa.initial_pitch_error).abs() < 0.001);
            // Voicing and amplitudes at the float frame's refined pitch, both sides carrying state.
            let l = harmonics_count(fa.omega0_hat);
            let (fl_v, fl_m) = fl_mbe::analyze_at_pitch(fa, fa.omega0_hat, l, &mut fl_state);
            let (fx_v, fx_m) = fx_mbe::analyze_frame_at_pitch(xa, &xa.pitch, l, &mut fx_state);
            for h in 1..=l as usize {
                stats.harmonics += 1;
                stats.voicing_same += usize::from(fl_v[h] == fx_v[h]);
                if fl_v[h] == fx_v[h] && fl_m[h] > 1.0 && same_period {
                    let db = (20.0 * ((fx_m[h] as f64 / 65536.0).max(1e-9) / fl_m[h]).log10()).abs();
                    stats.amp_db_worst = stats.amp_db_worst.max(db);
                }
            }
        }
    }
}

#[test]
fn streaming_fixed_analyzer_matches_the_float_analyzer_on_real_speech() {
    let seconds: usize = std::env::var("MAX_SECONDS").ok().and_then(|v| v.parse().ok()).unwrap_or(5);
    let mut stats = Stats::default();
    for offset in [0, -37] {
        run(offset, seconds * 8000, &mut stats);
    }
    let pct = |n: usize, d: usize| 100.0 * n as f64 / d as f64;
    eprintln!(
        "frames={} count mismatches={} refined period identical {:.2}% own samples identical {:.2}% \
         initial-pitch error within 0.001 on {:.2}%; voicing agreement {:.3}% of {} harmonics; \
         worst amplitude error {:.5} dB",
        stats.frames,
        stats.count_mismatch,
        pct(stats.p8_same, stats.frames),
        pct(stats.slot_same, stats.frames),
        pct(stats.e_close, stats.frames),
        pct(stats.voicing_same, stats.harmonics),
        stats.harmonics,
        stats.amp_db_worst,
    );
    assert!(stats.frames >= 300);
    assert_eq!(stats.count_mismatch, 0);
    assert_eq!(stats.slot_same, stats.frames);
    assert!(pct(stats.p8_same, stats.frames) >= 99.0);
    assert!(pct(stats.e_close, stats.frames) >= 99.0);
    assert!(pct(stats.voicing_same, stats.harmonics) >= 99.0);
    assert!(stats.amp_db_worst < 0.5);
}
