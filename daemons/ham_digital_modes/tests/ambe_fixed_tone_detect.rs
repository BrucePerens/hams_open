// SPDX-License-Identifier: LGPL-3.0-or-later
//! Compares `ambe::fixed::general::tone_detect::detect_tone` (integer, `i16` frames) with the float
//! `ambe::float::tone_detect::detect_tone` on a broad set of synthetic frames: all 16 DTMF digits and a sweep of
//! single tones at several levels and phase offsets, unequal-level DTMF pairs, noise, silence, voiced-like harmonic
//! frames, very quiet tones, and the chip-matched non-detections at 100/300/3900 Hz. Both detectors see the
//! identical `i16` sample values. Decisions must be identical; amplitude within 0.5%; the volume field within +-1;
//! the single-tone frequency within 1 Hz.

use ham_digital_modes::ambe::fixed::general::tone_detect as fx;
use ham_digital_modes::ambe::float::tone_detect as fl;
use std::f64::consts::PI;

const ROW: [f64; 4] = [697.0, 770.0, 852.0, 941.0];
const COL: [f64; 4] = [1209.0, 1336.0, 1477.0, 1633.0];

fn sines(tones: &[(f64, f64)], offset: usize) -> Vec<i16> {
    (0..160)
        .map(|i| tones.iter().map(|&(hz, amp)| amp * (2.0 * PI * hz * (offset + i) as f64 / 8000.0).sin()).sum::<f64>().round() as i16)
        .collect()
}

struct Stats {
    frames: usize,
    detections: usize,
    worst_amp_rel: f64,
    worst_volume: i32,
    worst_hz: f64,
}

fn compare(label: &str, frame: &[i16], st: &mut Stats) {
    let float_frame: Vec<f64> = frame.iter().map(|&v| v as f64).collect();
    let want = fl::detect_tone(&float_frame);
    let got = fx::detect_tone(frame);
    st.frames += 1;
    match (want, got) {
        (None, None) => {}
        (Some(w), Some(g)) => {
            st.detections += 1;
            match (w.tone, g.tone) {
                (fl::DetectedTone::Dtmf { row: r1, col: c1 }, fx::DetectedTone::Dtmf { row: r2, col: c2 }) => assert_eq!((r1, c1), (r2, c2), "{label}"),
                (fl::DetectedTone::Single { index: i1, hz: h1 }, fx::DetectedTone::Single { index: i2, hz_q16 }) => {
                    assert_eq!(i1, i2, "{label}: index");
                    let dh = (h1 - hz_q16 as f64 / 65536.0).abs();
                    st.worst_hz = st.worst_hz.max(dh);
                    assert!(dh <= 1.0, "{label}: hz {h1} vs {}", hz_q16 as f64 / 65536.0);
                }
                other => panic!("{label}: different tone kinds {other:?}"),
            }
            let rel = (g.amplitude_q16 as f64 / 65536.0 / w.amplitude - 1.0).abs();
            st.worst_amp_rel = st.worst_amp_rel.max(rel);
            assert!(rel < 0.005, "{label}: amplitude {} vs {}", g.amplitude_q16 as f64 / 65536.0, w.amplitude);
            let dv = (fx::volume_for_amplitude_q16(g.amplitude_q16) as i32 - fl::volume_for_amplitude(w.amplitude) as i32).abs();
            st.worst_volume = st.worst_volume.max(dv);
            assert!(dv <= 1, "{label}: volume field differs by {dv}");
        }
        (w, g) => panic!("{label}: float {w:?} vs fixed {g:?}"),
    }
}

#[test]
#[allow(clippy::needless_range_loop)]
fn fixed_tone_detector_matches_float_on_synthetic_stimuli() {
    let mut st = Stats { frames: 0, detections: 0, worst_amp_rel: 0.0, worst_volume: 0, worst_hz: 0.0 };

    // All 16 DTMF digits at several levels and phase offsets.
    for r in 0..4 {
        for c in 0..4 {
            for &amp in &[300.0, 1000.0, 4000.0, 12000.0] {
                for &off in &[0usize, 37, 101] {
                    compare(&format!("dtmf {r}{c} amp {amp} off {off}"), &sines(&[(ROW[r], amp), (COL[c], amp)], off), &mut st);
                }
            }
        }
    }
    // Unequal-level DTMF pairs (the row/column ratio rule) and a DTMF pair with a third tone.
    for &(ra, ca) in &[(4000.0, 2000.0), (4000.0, 1500.0), (4000.0, 1200.0), (4000.0, 500.0), (500.0, 4000.0)] {
        compare(&format!("unequal {ra}/{ca}"), &sines(&[(ROW[1], ra), (COL[2], ca)], 5), &mut st);
    }
    compare("dtmf plus third tone", &sines(&[(ROW[2], 3000.0), (COL[0], 3000.0), (2500.0, 2500.0)], 9), &mut st);

    // Single tones: 200 Hz, 400..=3800 Hz every 100 Hz, plus off-grid frequencies, at several levels.
    let mut freqs = vec![200.0, 437.5, 1234.0, 2718.0, 3333.0];
    freqs.extend((4..=38).map(|k| k as f64 * 100.0));
    for &hz in &freqs {
        for &amp in &[300.0, 1000.0, 4000.0, 12000.0, 30000.0] {
            compare(&format!("single {hz} Hz amp {amp}"), &sines(&[(hz, amp)], 11), &mut st);
        }
    }
    // Chip-matched non-detections and near-boundary frequencies.
    for &hz in &[100.0, 300.0, 3900.0, 150.0, 250.0, 280.0, 340.0, 3850.0, 3950.0] {
        compare(&format!("boundary {hz} Hz"), &sines(&[(hz, 4000.0)], 0), &mut st);
    }
    // Very quiet tones around the amplitude/energy gates.
    for &amp in &[40.0, 90.0, 110.0, 150.0] {
        compare(&format!("quiet single {amp}"), &sines(&[(1000.0, amp)], 3), &mut st);
        compare(&format!("quiet dtmf {amp}"), &sines(&[(ROW[0], amp), (COL[0], amp)], 3), &mut st);
    }
    // Silence, noise, voiced-like harmonic frames.
    compare("silence", &[0i16; 160], &mut st);
    let mut s = 0x1234_5678_9abc_def0u64;
    for &amp in &[100.0, 500.0, 2000.0, 8000.0] {
        for k in 0..6 {
            let noise: Vec<i16> = (0..160)
                .map(|_| {
                    s ^= s << 13;
                    s ^= s >> 7;
                    s ^= s << 17;
                    (((s >> 11) as f64 / (1u64 << 53) as f64 * 2.0 - 1.0) * amp).round() as i16
                })
                .collect();
            compare(&format!("noise {amp} #{k}"), &noise, &mut st);
        }
    }
    for &f0 in &[100.0, 130.0, 180.0, 240.0] {
        let tones: Vec<(f64, f64)> = (1..=8).map(|h| (f0 * h as f64, 900.0 / h as f64)).collect();
        for &off in &[0usize, 60] {
            compare(&format!("voiced {f0} Hz off {off}"), &sines(&tones, off), &mut st);
        }
    }
    // A tone in noise.
    let mut noisy = sines(&[(1500.0, 3000.0)], 2);
    for v in noisy.iter_mut() {
        s ^= s << 13;
        s ^= s >> 7;
        s ^= s << 17;
        *v = v.saturating_add((((s >> 11) as f64 / (1u64 << 53) as f64 - 0.5) * 600.0) as i16);
    }
    compare("tone in noise", &noisy, &mut st);

    eprintln!(
        "tone detect: {} frames identical decisions ({} detections); worst amplitude error {:.4}%, worst volume difference {}, worst frequency difference {:.3} Hz",
        st.frames,
        st.detections,
        st.worst_amp_rel * 100.0,
        st.worst_volume,
        st.worst_hz
    );
    assert!(st.detections > 200, "the stimulus set should produce many detections");
}
