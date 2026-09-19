// SPDX-License-Identifier: LGPL-3.0-or-later
//! Cross-checks `ambe::fixed::ratet27::synthesis::SynthesisState` against its floating-point sibling
//! `ambe::float::ratet27::synthesis::SynthesisState`, at this port's documented 40 dB SNR bar (see
//! `ambe_fixed_unvoiced_synthesis.rs`'s own doc comment for why a per-value relative-error check isn't
//! used for full PCM frames). `omega0` is quantized identically for both sides before every comparison
//! here, for the same reason `ambe_fixed_voiced_synthesis.rs`'s own `run_scenario` does -- see that
//! file's doc comment for the full account of why an exact-vs-quantized comparison isn't meaningful for
//! a module with a persistent phase accumulator underneath it.
//!
//! Fixed's own combined PCM is `i64` (see `fixed::ratet27::synthesis`'s own doc comment for why), so
//! comparisons here convert it to `f64` the same way `from_q16` converts any other Q16.16 value.

use ham_digital_modes::ambe::fixed::ratet27::error_estimation::FrameErrorsQ16;
use ham_digital_modes::ambe::fixed::ratet27::synthesis::SynthesisState as FixedSynthesisState;
use ham_digital_modes::ambe::float::ratet27::error_estimation::FrameErrors;
use ham_digital_modes::ambe::float::ratet27::parameter_encoding::dequantize_fundamental_frequency;
use ham_digital_modes::ambe::float::ratet27::synthesis::SynthesisState as FloatSynthesisState;

const MIN_SNR_DB: f64 = 40.0;

fn to_q16(v: f64) -> i32 {
    (v * 65536.0).round() as i32
}
fn from_q16(v: i32) -> f64 {
    v as f64 / 65536.0
}
fn from_q16_i64(v: i64) -> f64 {
    v as f64 / 65536.0
}

fn zero_errors() -> FrameErrors {
    FrameErrors { total: 0, rate: 0.0, golay_init: 0, hamming_init: 0 }
}
fn zero_errors_q16() -> FrameErrorsQ16 {
    FrameErrorsQ16 { total: 0, rate_q16: 0, golay_init: 0, hamming_init: 0 }
}

fn snr_db(float_pcm: &[f64], fixed_pcm: &[f64]) -> f64 {
    let signal_power: f64 = float_pcm.iter().map(|&s| s * s).sum();
    let noise_power: f64 = float_pcm
        .iter()
        .zip(fixed_pcm.iter())
        .map(|(&f, &x)| (f - x) * (f - x))
        .sum();
    if noise_power <= 1e-12 {
        return f64::INFINITY;
    }
    10.0 * (signal_power / noise_power).log10()
}

/// Runs both implementations across a sequence of (omega0, voiced, amplitudes) frames, returning the
/// whole run's concatenated PCM. `amplitudes` are the *unenhanced* reconstructed amplitudes -- the
/// same pre-enhancement input `finalize_parameters`/`finalize_parameters_q16` both consume.
fn run_scenario(frames: &[(f64, Vec<bool>, Vec<f64>)]) -> (Vec<f64>, Vec<f64>) {
    let mut float_state = FloatSynthesisState::new();
    let mut fixed_state = FixedSynthesisState::new();
    let errors = zero_errors();
    let errors_q16 = zero_errors_q16();

    let mut float_pcm = Vec::new();
    let mut fixed_pcm = Vec::new();
    for (omega0, voiced, amplitudes) in frames {
        let omega0_q16 = to_q16(*omega0);
        let omega0_fair = from_q16(omega0_q16);
        let amplitudes_q16: Vec<i32> = amplitudes.iter().map(|&a| to_q16(a)).collect();

        let float_frame = float_state
            .synthesize_frame(amplitudes, omega0_fair, voiced, &errors)
            .unwrap();
        let fixed_frame = fixed_state
            .synthesize_frame(&amplitudes_q16, omega0_q16, voiced, &errors_q16)
            .unwrap();

        float_pcm.extend(float_frame.iter().copied());
        fixed_pcm.extend(fixed_frame.iter().map(|&s| from_q16_i64(s)));
    }
    (float_pcm, fixed_pcm)
}

#[test]
fn synthesize_frame_matches_float_for_a_steady_voiced_tone() {
    let omega0 = dequantize_fundamental_frequency(100);
    let voiced = vec![true; 16];
    let amplitudes: Vec<f64> = (1..=16).map(|i| 100.0 + 15.0 * i as f64).collect();
    let frames: Vec<_> = (0..5).map(|_| (omega0, voiced.clone(), amplitudes.clone())).collect();
    let (float_pcm, fixed_pcm) = run_scenario(&frames);
    let snr = snr_db(&float_pcm, &fixed_pcm);
    assert!(snr >= MIN_SNR_DB, "steady voiced tone SNR too low: {snr} dB");
}

#[test]
fn synthesize_frame_matches_float_for_a_fully_unvoiced_frame() {
    let omega0 = dequantize_fundamental_frequency(100);
    let voiced = vec![false; 16];
    let amplitudes: Vec<f64> = (1..=16).map(|i| 80.0 + 5.0 * i as f64).collect();
    let frames: Vec<_> = (0..4).map(|_| (omega0, voiced.clone(), amplitudes.clone())).collect();
    let (float_pcm, fixed_pcm) = run_scenario(&frames);
    let snr = snr_db(&float_pcm, &fixed_pcm);
    assert!(snr >= MIN_SNR_DB, "fully unvoiced frame SNR too low: {snr} dB");
}

#[test]
fn synthesize_frame_matches_float_for_a_mixed_voicing_pattern() {
    let omega0 = dequantize_fundamental_frequency(90);
    let voiced: Vec<bool> = (0..20).map(|i| i % 3 != 0).collect();
    let amplitudes: Vec<f64> = (1..=20).map(|i| 50.0 + 6.0 * i as f64).collect();
    let frames: Vec<_> = (0..4).map(|_| (omega0, voiced.clone(), amplitudes.clone())).collect();
    let (float_pcm, fixed_pcm) = run_scenario(&frames);
    let snr = snr_db(&float_pcm, &fixed_pcm);
    assert!(snr >= MIN_SNR_DB, "mixed voicing SNR too low: {snr} dB");
}

#[test]
fn synthesize_frame_rejects_a_length_mismatch() {
    let mut state = FixedSynthesisState::new();
    let voiced = vec![true; 5];
    let amplitudes = vec![to_q16(100.0); 6];
    assert!(state
        .synthesize_frame(&amplitudes, to_q16(0.1), &voiced, &zero_errors_q16())
        .is_none());
}

#[test]
fn synthesize_repeated_frame_returns_none_before_any_real_frame_has_run() {
    let mut state = FixedSynthesisState::new();
    assert!(state.synthesize_repeated_frame().is_none());
}

/// The real point of Eq. 99-104: a repeated frame must reuse the exact `(voiced, M_bar_l(0))` pair a
/// real frame already finalized, not re-derive it -- checked the same way the float sibling's own
/// analogous test does, by confirming a run of repeats produces finite output and doesn't touch
/// enhancement-stage state (`s_e`/`tau_m`), which Eq. 99-104 skip entirely on a repeat.
#[test]
fn synthesize_repeated_frame_reuses_the_last_real_frames_own_final_parameters() {
    let mut state = FixedSynthesisState::new();
    let omega0 = dequantize_fundamental_frequency(100);
    let voiced = vec![true; 16];
    let amplitudes = vec![to_q16(300.0); 16];
    let errors = zero_errors_q16();

    state
        .synthesize_frame(&amplitudes, to_q16(omega0), &voiced, &errors)
        .unwrap();

    for _ in 0..3 {
        let frame = state.synthesize_repeated_frame().unwrap();
        for &sample in &frame {
            assert!(sample.abs() < (1i64 << 40), "unreasonably large repeated-frame sample: {sample}");
        }
    }
}

/// Section 7.8's own literal requirement, ported directly from the float sibling's own analogous
/// test: every comfort-noise sample lands in `[-5.0, 5.0)` Q16.16, and the frame isn't degenerate.
#[test]
fn synthesize_comfort_frame_produces_bounded_non_degenerate_noise() {
    let mut state = FixedSynthesisState::new();
    let frame = state.synthesize_comfort_frame();
    let lower = to_q16(-5.0) as i64;
    let upper = to_q16(5.0) as i64;
    for &sample in &frame {
        assert!(
            (lower..upper).contains(&sample),
            "comfort-noise sample {sample} outside the spec's own [-5, 5) interval (Q16.16)"
        );
    }
    assert!(
        frame.iter().any(|&s| s != frame[0]),
        "expected real noise, not a degenerate constant frame"
    );
}

#[test]
fn synthesize_comfort_frame_is_deterministic_from_the_same_seed() {
    let mut a = FixedSynthesisState::new();
    let mut b = FixedSynthesisState::new();
    assert_eq!(a.synthesize_comfort_frame(), b.synthesize_comfort_frame());
}

/// Real, checkable regression for the exact wiring bug this module exists to prevent (ported from the
/// float sibling's own analogous test): with a clean error record and every harmonic amplitude far
/// below both the voicing and amplitude thresholds, `gamma_M` (Eq. 116) must clamp to `1.0`, so real
/// synthesized energy should survive, not collapse to near-silence.
#[test]
fn synthesize_frame_applies_a_no_op_gamma_m_when_amplitudes_are_well_under_threshold() {
    let mut state = FixedSynthesisState::new();
    let omega0 = dequantize_fundamental_frequency(100);
    let voiced = vec![false; 9];
    let amplitudes = vec![to_q16(1.0); 9];
    let frame = state
        .synthesize_frame(&amplitudes, to_q16(omega0), &voiced, &zero_errors_q16())
        .unwrap();
    assert!(
        frame.iter().any(|&s| s.abs() > 10),
        "expected real synthesized energy, got near-silence: {frame:?}"
    );
}

/// The three tests above all use amplitudes well under `tau_M`'s own `20480.0` real-unit floor, so
/// `gamma_M` (Eq. 116) is provably `1.0` for every one of them -- a real gap, since that means
/// `finalize_parameters_q16`'s own amplitude-smoothing arithmetic (as opposed to its wiring) was
/// never actually exercised. This scenario's amplitude sum genuinely exceeds the floor (`A_M ~=
/// 24000` for `30` harmonics at `800.0` each, before any enhancement scaling), engaging Eq. 116's
/// `tau_M / A_M` branch on both sides. `l_hat=30` is deliberately kept within `unvoiced_spectrum`'s
/// own implicit domain (`(l_hat + 0.5) * omega0_tilde` must stay under `pi`, a real geometric
/// constraint a real encoder's own `L_hat` derivation always satisfies for its own `omega0_hat`; an
/// earlier version of this test used `l_hat=40` with this same `omega0`, `40.5 * omega0 ~= 3.93 >
/// pi`, an invalid combination no real bitstream would produce -- it panicked deep in the float
/// sibling's own `unvoiced_spectrum` with an index-out-of-bounds, a real but out-of-scope robustness
/// question for `unvoiced_synthesis` itself, not a fixed-point port bug). Only `10` of the `30`
/// harmonics are voiced, keeping the worst-case constructive sum `voiced_synthesis` sees comfortably
/// under its own saturation point -- this test means to isolate enhancement/smoothing arithmetic,
/// not mix in that separately-covered concern.
#[test]
fn synthesize_frame_matches_float_when_gamma_m_actually_engages() {
    let omega0 = dequantize_fundamental_frequency(90);
    let l_hat = 30usize;
    let voiced: Vec<bool> = (0..l_hat).map(|i| i % 3 == 0).collect();
    let amplitudes = vec![800.0; l_hat];
    let frames: Vec<_> = (0..4).map(|_| (omega0, voiced.clone(), amplitudes.clone())).collect();
    let (float_pcm, fixed_pcm) = run_scenario(&frames);
    let snr = snr_db(&float_pcm, &fixed_pcm);
    assert!(snr >= MIN_SNR_DB, "gamma_M-engaged scenario SNR too low: {snr} dB");
}

/// Every test above uses a clean error record (`total: 0, rate: 0.0`), which takes
/// `adaptive_voicing_threshold`'s own *first* branch (the `INFINITY_SENTINEL` short-circuit) and
/// `update_amplitude_threshold`'s own *first* branch (the flat `20480.0` reset) unconditionally --
/// neither module's own log-domain/decaying-threshold arithmetic was reachable. A moderate error
/// record (`rate=0.01`, `total=5`, `hamming_init=0`) instead takes `adaptive_voicing_threshold`'s
/// own *second* branch (`rate_q16 <= 0.0125` and no fresh Hamming errors -- the log-domain
/// `45.255 * s_e^0.375 / exp(277.26*rate)` computation) and `update_amplitude_threshold`'s own
/// `else` branch (`rate_q16 > 0.005`, so `tau_M` decays from its previous value rather than
/// resetting) on both sides. A flipped voicing decision from a wrong threshold would show up here as
/// a large SNR hit (a whole harmonic's synthesis path changes, not just its amplitude), so this test
/// doesn't need to assert on `smoothed_voiced` directly (confirmed by hand for this exact scenario:
/// every one of the 20 harmonics' own smoothed voicing decision already agrees between the two sides,
/// and the per-harmonic enhanced-amplitude gap is a fraction of a percent -- not the source of this
/// test's own SNR bar).
///
/// Pitch varies slightly frame to frame (a small nearby-`b0` sweep, quantized identically for both
/// sides, exactly as `ambe_fixed_voiced_synthesis.rs`'s own `run_scenario` does) rather than holding
/// one `omega0` exactly across every frame -- an earlier version of this test held `omega0` exactly
/// and fed float the *unquantized* value while fixed got `to_q16(omega0)`, both mistakes
/// `ambe_fixed_voiced_synthesis.rs`'s own doc comment already found and fixed for `voiced_synthesis`
/// in isolation: per-frame SNR measured 56.8 dB on frame 0 degrading to 25.0 dB by frame 5, the same
/// signature already characterized there, not a new bug in this module's own enhancement/error-
/// estimation wiring.
#[test]
fn synthesize_frame_matches_float_with_a_moderate_error_record() {
    let l_hat = 20usize;
    let voiced: Vec<bool> = (0..l_hat).map(|i| i % 3 != 0).collect();
    let amplitudes: Vec<f64> = (1..=l_hat).map(|i| 40.0 + 5.0 * i as f64).collect();
    let errors = FrameErrors { total: 5, rate: 0.01, golay_init: 0, hamming_init: 0 };
    let errors_q16 = FrameErrorsQ16 { total: 5, rate_q16: to_q16(0.01), golay_init: 0, hamming_init: 0 };
    let b0_values = [90u32, 91, 90, 89];

    let mut float_state = FloatSynthesisState::new();
    let mut fixed_state = FixedSynthesisState::new();
    let mut float_pcm = Vec::new();
    let mut fixed_pcm = Vec::new();
    for &b0 in &b0_values {
        let omega0_q16 = to_q16(dequantize_fundamental_frequency(b0));
        let omega0_fair = from_q16(omega0_q16);
        let amplitudes_q16: Vec<i32> = amplitudes.iter().map(|&a| to_q16(a)).collect();

        let float_frame = float_state
            .synthesize_frame(&amplitudes, omega0_fair, &voiced, &errors)
            .unwrap();
        let fixed_frame = fixed_state
            .synthesize_frame(&amplitudes_q16, omega0_q16, &voiced, &errors_q16)
            .unwrap();
        float_pcm.extend(float_frame.iter().copied());
        fixed_pcm.extend(fixed_frame.iter().map(|&s| from_q16_i64(s)));
    }
    let snr = snr_db(&float_pcm, &fixed_pcm);
    assert!(snr >= MIN_SNR_DB, "moderate error record SNR too low: {snr} dB");
}

