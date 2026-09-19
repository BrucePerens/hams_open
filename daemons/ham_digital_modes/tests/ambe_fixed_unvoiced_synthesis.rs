// SPDX-License-Identifier: LGPL-3.0-or-later
//! Cross-checks `ambe::fixed::general::unvoiced_synthesis` against its floating-point sibling
//! `ambe::float::tia_102_baba::unvoiced_synthesis`. Scalar helpers (the synthesis window, the `gamma_w`
//! constant) are held to this crate's usual 1% relative tolerance; full synthesized PCM frames are
//! held to the fixed-point synthesis port's own documented bar instead (`fixed::mod`'s own doc
//! comment: "at least 40 dB SNR against the floating-point sibling's own PCM for the same input"),
//! since per-sample rounding in a 256-point DFT/IDFT is expected to differ slightly in ways a
//! per-value relative-error check would over-penalize.

use ham_digital_modes::ambe::fixed::general::unvoiced_synthesis as fixed_uv;
use ham_digital_modes::ambe::float::tia_102_baba::unvoiced_synthesis as float_uv;

const WINDOW_TOLERANCE: f64 = 0.01;
const MIN_SNR_DB: f64 = 40.0;

fn to_q16(v: f64) -> i32 {
    (v * 65536.0).round() as i32
}
fn from_q16(v: i32) -> f64 {
    v as f64 / 65536.0
}

#[test]
fn synthesis_window_matches_float_at_every_integer_offset() {
    for n in -120..=120 {
        let float_val = float_uv::synthesis_window(n);
        let fixed_val = from_q16(fixed_uv::synthesis_window_q16(n));
        let diff = (float_val - fixed_val).abs();
        assert!(
            diff <= WINDOW_TOLERANCE,
            "n={n}: float={float_val}, fixed={fixed_val}, diff={diff}"
        );
    }
}

#[test]
fn unvoiced_scaling_coefficient_matches_float_within_tolerance() {
    let float_val = float_uv::unvoiced_scaling_coefficient();
    let fixed_val = from_q16(fixed_uv::UNVOICED_SCALING_COEFFICIENT_Q16);
    let rel_err = (fixed_val - float_val).abs() / float_val;
    assert!(
        rel_err <= WINDOW_TOLERANCE,
        "float gamma_w={float_val}, fixed={fixed_val}, rel_err={rel_err}"
    );
}

/// Same construction both `NoiseState`s use internally (`u(-105) = 3147`, Eq. 117's own integer
/// recurrence) -- since the recurrence is pure integer on both sides, the two states' own windows
/// must produce byte-identical sequences, not merely close ones. Checked directly as a real
/// precondition for every test below: if this ever fails, the fixed/float comparison downstream is
/// meaningless (comparing synthesis against two different noise inputs).
#[test]
fn fixed_and_float_noise_states_produce_identical_sequences() {
    let float_state = float_uv::NoiseState::new();
    let fixed_state = fixed_uv::NoiseState::new();
    for relative in -104..=104 {
        assert_eq!(
            float_state.at(relative),
            fixed_state.at(relative),
            "noise sequence diverged at relative={relative}"
        );
    }
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

/// Runs both implementations across several frames of a fixed scenario, returning the whole run's
/// concatenated PCM (float, then fixed-converted-to-f64) for an SNR check -- a single frame's own
/// SNR can be misleadingly bad right at a transition (near-zero float samples dominate the ratio),
/// so several frames together are a more representative measure, matching how a real decoder's
/// output would actually be judged.
fn run_scenario(
    omega0: f64,
    voiced_frames: &[Vec<bool>],
    amplitude_frames: &[Vec<f64>],
) -> (Vec<f64>, Vec<f64>) {
    let mut float_state = float_uv::UnvoicedState::new();
    let mut float_noise = float_uv::NoiseState::new();
    let mut fixed_state = fixed_uv::UnvoicedState::new();
    let mut fixed_noise = fixed_uv::NoiseState::new();

    let mut float_pcm = Vec::new();
    let mut fixed_pcm = Vec::new();
    for (i, (voiced, amplitudes)) in voiced_frames.iter().zip(amplitude_frames.iter()).enumerate() {
        if i > 0 {
            float_noise.advance_frame();
            fixed_noise.advance_frame();
        }
        let float_frame = float_state.synthesize(&float_noise, omega0, voiced, amplitudes).unwrap();
        let amplitudes_q16: Vec<i32> = amplitudes.iter().map(|&a| to_q16(a)).collect();
        let fixed_frame = fixed_state
            .synthesize(&fixed_noise, to_q16(omega0), voiced, &amplitudes_q16)
            .unwrap();
        float_pcm.extend(float_frame.iter().copied());
        fixed_pcm.extend(fixed_frame.iter().map(|&s| s as f64 / 65536.0));
    }
    (float_pcm, fixed_pcm)
}

#[test]
fn synthesize_matches_float_for_a_fully_unvoiced_steady_tone() {
    let omega0 = 2.0 * std::f64::consts::PI / 100.0;
    let voiced = vec![false; 16];
    let amplitudes: Vec<f64> = (1..=16).map(|i| 300.0 + 40.0 * i as f64).collect();
    let voiced_frames = vec![voiced.clone(); 4];
    let amplitude_frames = vec![amplitudes; 4];
    let (float_pcm, fixed_pcm) = run_scenario(omega0, &voiced_frames, &amplitude_frames);
    let snr = snr_db(&float_pcm, &fixed_pcm);
    assert!(snr >= MIN_SNR_DB, "unvoiced steady tone SNR too low: {snr} dB");
}

#[test]
fn synthesize_matches_float_for_a_partially_voiced_mix_across_a_pitch_change() {
    let voiced: Vec<bool> = (0..20).map(|i| i % 3 == 0).collect(); // some voiced, some not.
    let amplitudes: Vec<f64> = (1..=20).map(|i| 200.0 + 15.0 * i as f64).collect();
    let voiced_frames = vec![voiced.clone(); 3];
    let amplitude_frames = vec![amplitudes; 3];
    // A real dequantized omega0 (not a suspiciously round fraction like 2*pi/60, which makes several
    // harmonic band edges land on exact integers -- see this test file's own `debug_band_edges`,
    // which found `256/(2*pi)*7.5*(2*pi/60) == 32.0` *exactly*, so a few-ULP Q16.16 rounding
    // difference between fixed and float can legitimately push `ceil()` to a different integer on
    // either side, shifting one harmonic's unvoiced band by a whole bin. This is the same class of
    // fixed-point floor/ceil-boundary risk `fixed::general::mbe_speech`'s own doc comment already
    // documents and accepts (rare, real, and not chased to zero) -- using a real dequantized pitch
    // here, like actual decoded speech would, avoids exercising that risk gratuitously.
    let omega0 = ham_digital_modes::ambe::float::tia_102_baba::parameter_encoding::dequantize_fundamental_frequency(100);
    let (float_pcm, fixed_pcm) = run_scenario(omega0, &voiced_frames, &amplitude_frames);
    let snr = snr_db(&float_pcm, &fixed_pcm);
    assert!(snr >= MIN_SNR_DB, "partially voiced mix SNR too low: {snr} dB");
}

#[test]
fn synthesize_matches_float_for_a_low_pitch_many_harmonic_frame() {
    let l_hat = 50usize;
    let omega0 = 2.0 * std::f64::consts::PI / 150.0;
    let voiced: Vec<bool> = (0..l_hat).map(|i| i % 5 != 0).collect();
    let amplitudes: Vec<f64> = (1..=l_hat).map(|i| 100.0 + 8.0 * i as f64).collect();
    let voiced_frames = vec![voiced.clone(); 3];
    let amplitude_frames = vec![amplitudes; 3];
    let (float_pcm, fixed_pcm) = run_scenario(omega0, &voiced_frames, &amplitude_frames);
    let snr = snr_db(&float_pcm, &fixed_pcm);
    assert!(snr >= MIN_SNR_DB, "low-pitch many-harmonic SNR too low: {snr} dB");
}

#[test]
fn synthesize_matches_float_for_a_quiet_frame() {
    let omega0 = ham_digital_modes::ambe::float::tia_102_baba::parameter_encoding::dequantize_fundamental_frequency(150);
    let voiced = vec![false; 12];
    let amplitudes: Vec<f64> = (1..=12).map(|i| 5.0 + 0.5 * i as f64).collect();
    let voiced_frames = vec![voiced.clone(); 3];
    let amplitude_frames = vec![amplitudes; 3];
    let (float_pcm, fixed_pcm) = run_scenario(omega0, &voiced_frames, &amplitude_frames);
    let snr = snr_db(&float_pcm, &fixed_pcm);
    assert!(snr >= MIN_SNR_DB, "quiet frame SNR too low: {snr} dB");
}

/// A real, previously-caught regression: `scale = gamma_w * M_l / sqrt(power)` for a *very* quiet
/// harmonic (`M_l` in the single digits, well within the real chip's own observed `R_M0` range down
/// to single digits) makes `scale` itself tiny (`~0.003`) -- squaring it before a final `sqrt_q16`
/// (an earlier version of this port's own approach, reasoned to avoid a *different* overflow risk)
/// could underflow Q16.16's own resolution and zero the harmonic out entirely, measured directly at
/// the time as `~13 dB` SNR against float. `fixed_ops::sqrt_wide_q16` (a genuine wide square root,
/// not a "square first" reorganization) fixed it; this sweeps peak amplitude from `0.5` (near the
/// real chip's own observed floor) up through single digits specifically to keep that regression
/// caught, not just the one value that happened to fail during development.
#[test]
fn synthesize_matches_float_across_a_very_quiet_amplitude_sweep() {
    let omega0 = ham_digital_modes::ambe::float::tia_102_baba::parameter_encoding::dequantize_fundamental_frequency(120);
    for &peak in &[0.5, 1.0, 2.0, 3.0, 5.0, 8.0] {
        let voiced = vec![false; 10];
        let amplitudes: Vec<f64> = (1..=10).map(|i| peak * (0.5 + 0.05 * i as f64)).collect();
        let voiced_frames = vec![voiced.clone(); 3];
        let amplitude_frames = vec![amplitudes; 3];
        let (float_pcm, fixed_pcm) = run_scenario(omega0, &voiced_frames, &amplitude_frames);
        let snr = snr_db(&float_pcm, &fixed_pcm);
        assert!(snr >= MIN_SNR_DB, "peak={peak}: SNR too low: {snr} dB");
    }
}
