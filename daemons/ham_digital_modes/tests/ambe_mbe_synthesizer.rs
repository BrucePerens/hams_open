// SPDX-License-Identifier: LGPL-3.0-or-later
//! Behavioural tests of the mode-independent `MbeSynthesizer` (float and fixed-point) and of
//! `SynthesisState::synthesize_repeated_frame_scaled`: determinism, repeat-frame scaling, fixed vs
//! float agreement, and silence/invalid-input handling, all on hand-built harmonic parameters.

use ham_digital_modes::ambe::fixed::general::mbe_synthesis::MbeSynthesizer as FixedSynth;
use ham_digital_modes::ambe::float::mbe_synthesis::MbeSynthesizer as FloatSynth;

/// 0.1 rad pitch gives 29 harmonics. Index 0 is the unused padding the API expects.
const W0: f64 = 0.1;
const L: usize = 29;

fn voiced() -> Vec<bool> {
    (0..=L).map(|l| l % 3 != 0).collect()
}

fn ml() -> Vec<f64> {
    (0..=L).map(|l| 3000.0 / (l.max(1) as f64)).collect()
}

fn ml_q16(ml: &[f64]) -> Vec<i32> {
    ml.iter().map(|m| (m * 65536.0).round() as i32).collect()
}

fn fixed_to_f64(x: &[i64]) -> Vec<f64> {
    x.iter().map(|&v| v as f64 / 65536.0).collect()
}

fn rms(x: &[f64]) -> f64 {
    (x.iter().map(|v| v * v).sum::<f64>() / x.len() as f64).sqrt()
}

fn snr_db(reference: &[f64], test: &[f64]) -> f64 {
    let noise: f64 = reference
        .iter()
        .zip(test)
        .map(|(r, t)| (r - t) * (r - t))
        .sum();
    let signal: f64 = reference.iter().map(|r| r * r).sum();
    10.0 * (signal / noise.max(1e-30)).log10()
}

fn float_run(frames: usize) -> Vec<Vec<f64>> {
    let mut s = FloatSynth::new();
    (0..frames)
        .map(|_| {
            s.synthesize_speech(W0, &voiced(), &ml(), 0, 0)
                .unwrap()
                .to_vec()
        })
        .collect()
}

// Tests [@ANCHOR: ham_digital_modes:float_mbe_synthesis_MbeSynthesizer_synthesize_speech]
// Tests [@ANCHOR: ham_digital_modes:float_mbe_synthesis_MbeSynthesizer_errors_for]
// Tests [@ANCHOR: ham_digital_modes:float_mbe_synthesis_MbeSynthesizer_lift]
#[test]
fn float_synthesis_is_deterministic_and_audible() {
    let a = float_run(4);
    let b = float_run(4);
    assert_eq!(
        a, b,
        "two fresh synthesizers on the same input must agree exactly"
    );
    assert!(a.iter().flatten().all(|v| v.is_finite()));
    assert!(
        rms(&a[3]) > 10.0,
        "voiced harmonics at this level are audible: {}",
        rms(&a[3])
    );
}

/// A repeat with scale 1.0 on a fresh pair of synthesizers equals the plain repeat, and a half scale
/// roughly halves the level (the lift filter is linear, the unvoiced part is scaled with the rest).
// Tests [@ANCHOR: ham_digital_modes:float_tia_102_baba_synthesis_SynthesisState_synthesize_repeated_frame_scaled]
#[test]
fn float_repeat_scaled_unity_equals_repeat_and_half_halves_level() {
    let prime = |s: &mut FloatSynth| {
        for _ in 0..3 {
            s.synthesize_speech(W0, &voiced(), &ml(), 0, 0).unwrap();
        }
    };
    let (mut plain, mut unity, mut half) =
        (FloatSynth::new(), FloatSynth::new(), FloatSynth::new());
    prime(&mut plain);
    prime(&mut unity);
    prime(&mut half);
    let p = plain.synthesize_repeat().unwrap();
    let u = unity.synthesize_repeat_scaled(1.0).unwrap();
    assert_eq!(p, u);
    // The first scaled frame still cross-fades from the previous full-level frame, so the level
    // settles to the scale on the second consecutive scaled repeat.
    half.synthesize_repeat_scaled(0.5).unwrap();
    let h = half.synthesize_repeat_scaled(0.5).unwrap();
    let p = plain.synthesize_repeat().unwrap();
    let ratio = rms(&h) / rms(&p);
    assert!(
        (ratio - 0.5).abs() < 0.05,
        "half scale gave level ratio {ratio}"
    );
}

/// The stored last frame is untouched by a scaled repeat, so a later full-scale repeat still sees the
/// original amplitudes (the cumulative-fade contract in the doc comment).
// Tests [@ANCHOR: ham_digital_modes:float_tia_102_baba_synthesis_SynthesisState_synthesize_repeated_frame_scaled]
#[test]
fn float_repeat_scaled_does_not_alter_the_stored_frame() {
    let mut a = FloatSynth::new();
    let mut b = FloatSynth::new();
    for s in [&mut a, &mut b] {
        s.synthesize_speech(W0, &voiced(), &ml(), 0, 0).unwrap();
    }
    a.synthesize_repeat_scaled(0.1).unwrap();
    a.synthesize_repeat().unwrap();
    let after_fade = a.synthesize_repeat().unwrap();
    b.synthesize_repeat().unwrap();
    let plain = b.synthesize_repeat().unwrap();
    // The overlap state differs after the faded frame, so compare levels once it has settled.
    let ratio = rms(&after_fade) / rms(&plain);
    assert!((ratio - 1.0).abs() < 0.2, "level ratio {ratio}");
}

#[test]
fn repeat_before_any_frame_is_none_and_bad_lengths_are_none() {
    let mut f = FloatSynth::new();
    assert!(f.synthesize_repeat().is_none());
    assert!(f.synthesize_repeat_scaled(0.5).is_none());
    assert!(f
        .synthesize_speech(W0, &voiced(), &ml()[..10], 0, 0)
        .is_none());
    assert!(f.synthesize_speech(W0, &[false], &[0.0], 0, 0).is_none());
    let mut x = FixedSynth::new();
    assert!(x.synthesize_repeat().is_none());
    assert!(x.synthesize_repeat_scaled(32768).is_none());
    assert!(x
        .synthesize_speech(
            (W0 * 4294967296.0) as i64,
            &voiced(),
            &ml_q16(&ml())[..10],
            0,
            0
        )
        .is_none());
    assert!(x.synthesize_speech(0, &[false], &[0], 0, 0).is_none());
}

#[test]
fn silence_is_all_zero_in_both_variants() {
    assert!(FloatSynth::new()
        .synthesize_silence()
        .iter()
        .all(|&v| v == 0.0));
    assert!(FixedSynth::new()
        .synthesize_silence()
        .iter()
        .all(|&v| v == 0));
    // Silence does not disturb the stored frame: a repeat afterwards still works.
    let mut f = FloatSynth::new();
    f.synthesize_speech(W0, &voiced(), &ml(), 0, 0).unwrap();
    let _ = f.synthesize_silence();
    assert!(f.synthesize_repeat().is_some());
}

/// All-zero amplitudes synthesize to (near) silence rather than noise or NaN.
#[test]
fn zero_amplitudes_give_silence() {
    let mut f = FloatSynth::new();
    let z = vec![0.0; L + 1];
    for _ in 0..3 {
        let out = f.synthesize_speech(W0, &voiced(), &z, 0, 0).unwrap();
        assert!(
            out.iter().all(|v| v.abs() < 1e-6),
            "float rms {}",
            rms(&out)
        );
    }
    let mut x = FixedSynth::new();
    let zq = vec![0i32; L + 1];
    for _ in 0..3 {
        let out = x
            .synthesize_speech((W0 * 4294967296.0) as i64, &voiced(), &zq, 0, 0)
            .unwrap();
        assert!(out.iter().all(|&v| v.abs() < 66), "fixed output not silent");
    }
}

// Tests [@ANCHOR: ham_digital_modes:fixed_general_mbe_synthesis_MbeSynthesizer_synthesize_speech]
// Tests [@ANCHOR: ham_digital_modes:fixed_general_mbe_synthesis_MbeSynthesizer_lift]
// Tests [@ANCHOR: ham_digital_modes:fixed_general_mbe_synthesis_MbeSynthesizer_new]
#[test]
fn fixed_is_deterministic_and_tracks_float() {
    let w0_q32 = (W0 * 4294967296.0) as i64;
    let q = ml_q16(&ml());
    let run = || {
        let mut s = FixedSynth::new();
        (0..4)
            .map(|_| {
                s.synthesize_speech(w0_q32, &voiced(), &q, 0, 0)
                    .unwrap()
                    .to_vec()
            })
            .collect::<Vec<_>>()
    };
    let a = run();
    assert_eq!(a, run());
    let float = float_run(4);
    let (r, t): (Vec<f64>, Vec<f64>) = (
        float.iter().flatten().copied().collect(),
        a.iter().flat_map(|f| fixed_to_f64(f)).collect(),
    );
    let snr = snr_db(&r, &t);
    assert!(snr > 40.0, "fixed vs float SNR {snr} dB");
}

// Tests [@ANCHOR: ham_digital_modes:fixed_tia_102_baba_synthesis_SynthesisState_synthesize_repeated_frame_scaled]
#[test]
fn fixed_repeat_scaled_tracks_float_and_halves_level() {
    let w0_q32 = (W0 * 4294967296.0) as i64;
    let q = ml_q16(&ml());
    let mut x = FixedSynth::new();
    let mut f = FloatSynth::new();
    for _ in 0..3 {
        x.synthesize_speech(w0_q32, &voiced(), &q, 0, 0).unwrap();
        f.synthesize_speech(W0, &voiced(), &ml(), 0, 0).unwrap();
    }
    x.synthesize_repeat_scaled(32768).unwrap();
    f.synthesize_repeat_scaled(0.5).unwrap();
    let xr = fixed_to_f64(&x.synthesize_repeat_scaled(32768).unwrap());
    let fr = f.synthesize_repeat_scaled(0.5).unwrap();
    let snr = snr_db(&fr, &xr);
    assert!(snr > 40.0, "fixed vs float scaled-repeat SNR {snr} dB");
    let mut u = FixedSynth::new();
    for _ in 0..3 {
        u.synthesize_speech(w0_q32, &voiced(), &q, 0, 0).unwrap();
    }
    u.synthesize_repeat_scaled(65536).unwrap();
    let full = fixed_to_f64(&u.synthesize_repeat_scaled(65536).unwrap());
    let ratio = rms(&xr) / rms(&full);
    assert!((ratio - 0.5).abs() < 0.05, "fixed half scale ratio {ratio}");
}
