// SPDX-License-Identifier: LGPL-3.0-or-later
//! Cross-checks `ambe::fixed::general::voiced_synthesis` against its floating-point sibling
//! `ambe::float::tia_102_baba::voiced_synthesis`, at this port's documented 40 dB SNR bar (see
//! `ambe_fixed_unvoiced_synthesis.rs`'s own doc comment for why a per-value relative-error check
//! isn't used for full PCM frames). Since both `NoiseState`s are already confirmed byte-identical
//! (`ambe_fixed_unvoiced_synthesis.rs`'s own test), the phase-dither draws are identical inputs on
//! both sides -- any real divergence here is the phase-accumulator/synthesis arithmetic itself, not
//! dither disagreement.
//!
//! See `run_scenario`'s own doc comment for why `omega0` is quantized identically for both sides
//! before every comparison here, `trig::PI_Q48`'s own doc comment for a real, unbounded drift bug this
//! port's phase conversion once had (fixed) and how it was found, and
//! `synthesize_characterizes_the_annex_a_initial_omega0_quantization_offset` for the one real, bounded,
//! and documented (not chased) limitation that remains even so. The long multi-frame runs below
//! (particularly the naturally-varying-pitch one) are also this port's own regression coverage for the
//! reason `psi`/`phi` are stored as wrapping phases rather than growing Q16.16 radians in the first
//! place: a naive growing-radian accumulator would overflow within a few hundred frames, long before a
//! real call ends.

use ham_digital_modes::ambe::fixed::general::unvoiced_synthesis::NoiseState as FixedNoiseState;
use ham_digital_modes::ambe::fixed::general::voiced_synthesis as fixed_v;
use ham_digital_modes::ambe::float::tia_102_baba::parameter_encoding::dequantize_fundamental_frequency;
use ham_digital_modes::ambe::float::tia_102_baba::unvoiced_synthesis::NoiseState as FloatNoiseState;
use ham_digital_modes::ambe::float::tia_102_baba::voiced_synthesis as float_v;

const MIN_SNR_DB: f64 = 40.0;

fn to_q16(v: f64) -> i32 {
    (v * 65536.0).round() as i32
}

/// Pitch is carried at Q32 radians/sample (`2^32` per radian) through voiced synthesis.
fn to_q32(v: f64) -> i64 {
    (v * 4294967296.0).round() as i64
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

fn from_q32(v: i64) -> f64 {
    v as f64 / 4294967296.0
}

/// Runs both implementations across a sequence of (omega0, voiced, amplitudes) frames, returning the
/// whole run's concatenated PCM for an SNR check.
///
/// `omega0` is round-tripped through `to_q32`/`from_q32` before being given to *either* side. This
/// matters specifically for `voiced_synthesis` (unlike every other fixed-vs-float comparison in this
/// crate): `psi`/`phi` (Eq. 139-140) are phase *accumulators* that add a per-frame increment forever,
/// so any persistent difference in the two sides' own `omega0` -- even one far too small to matter for
/// a single frame -- compounds linearly with frame count into a real, growing phase error. Feeding
/// float the *exact* `f64` omega0 while fixed necessarily works from a `to_q32`-rounded one (as an
/// earlier version of this file did) is not a meaningful test of the port: it asks fixed to track an
/// idealized, infinite-precision oscillator it was never given the precision to represent, a gap that
/// grows without bound and has nothing to do with whether the port's own arithmetic is correct
/// (confirmed directly: an independent Python simulation of the accumulation using the *same*
/// quantized omega0 matched this port's own output to within its own rounding). In the real pipeline,
/// both a float and a fixed decoder work from the *same* decoded `b0` parameter, so the fair
/// comparison -- the one this function now performs -- quantizes `omega0` once and gives both sides
/// that identical, finite-precision value, exactly as the two real implementations would receive it.
///
/// A residual, smaller, and *bounded* (not growing with frame count) gap still remains even under this
/// fair comparison, traced to `VoicedState::new()`'s own Annex A initial `omega0(-1)` constant needing
/// Q16.16 representation on this side -- see
/// `synthesize_characterizes_the_annex_a_initial_omega0_quantization_offset`'s own doc comment for the
/// full account. (An earlier version of this comment blamed a *different*, unbounded effect -- a scale
/// error in `trig::phase_from_radians_q16`'s own pi constant -- for a drift that grew with frame count;
/// that was a real, separate bug, since fixed in `trig::PI_Q48`, not a property of this comparison.)
fn run_scenario(frames: &[(f64, Vec<bool>, Vec<f64>)]) -> (Vec<f64>, Vec<f64>) {
    let mut float_state = float_v::VoicedState::new();
    let mut float_noise = FloatNoiseState::new();
    let mut fixed_state = fixed_v::VoicedState::new();
    let mut fixed_noise = FixedNoiseState::new();

    let mut float_pcm = Vec::new();
    let mut fixed_pcm = Vec::new();
    for (i, (omega0, voiced, amplitudes)) in frames.iter().enumerate() {
        if i > 0 {
            float_noise.advance_frame();
            fixed_noise.advance_frame();
        }
        let omega0_q32 = to_q32(*omega0);
        let omega0_fair = from_q32(omega0_q32);
        let float_frame = float_state.synthesize(&float_noise, omega0_fair, voiced, amplitudes).unwrap();
        let amplitudes_q16: Vec<i32> = amplitudes.iter().map(|&a| to_q16(a)).collect();
        let fixed_frame = fixed_state
            .synthesize(&fixed_noise, omega0_q32, voiced, &amplitudes_q16)
            .unwrap();
        float_pcm.extend(float_frame.iter().copied());
        fixed_pcm.extend(fixed_frame.iter().map(|&s| s as f64 / 65536.0));
    }
    (float_pcm, fixed_pcm)
}

#[test]
fn synthesize_matches_float_for_a_steady_fully_voiced_tone() {
    let omega0 = dequantize_fundamental_frequency(100);
    let voiced = vec![true; 16];
    let amplitudes: Vec<f64> = (1..=16).map(|i| 300.0 + 40.0 * i as f64).collect();
    let frames: Vec<_> = (0..6).map(|_| (omega0, voiced.clone(), amplitudes.clone())).collect();
    let (float_pcm, fixed_pcm) = run_scenario(&frames);
    let snr = snr_db(&float_pcm, &fixed_pcm);
    assert!(snr >= MIN_SNR_DB, "steady fully voiced tone SNR too low: {snr} dB");
}


#[test]
fn synthesize_matches_float_across_a_small_pitch_change_continuous_phase_branch() {
    // A small, gradual pitch change (Eq. 134-135's own continuous-phase interpolation branch --
    // exercised for l < 8 with a small omega0 jump).
    let voiced = vec![true; 10];
    let amplitudes: Vec<f64> = (1..=10).map(|i| 400.0 + 30.0 * i as f64).collect();
    let b0_values = [95u32, 96, 97, 96, 95];
    let frames: Vec<_> = b0_values
        .iter()
        .map(|&b0| (dequantize_fundamental_frequency(b0), voiced.clone(), amplitudes.clone()))
        .collect();
    let (float_pcm, fixed_pcm) = run_scenario(&frames);
    let snr = snr_db(&float_pcm, &fixed_pcm);
    assert!(snr >= MIN_SNR_DB, "small pitch change (continuous-phase) SNR too low: {snr} dB");
}

#[test]
fn synthesize_matches_float_across_a_large_pitch_jump_eq133_branch() {
    // A large, sudden pitch jump (Eq. 133's own independent-halves branch, for any harmonic with
    // |omega0_curr - omega0_prev| >= 0.1*omega0_curr, and unconditionally for l >= 8).
    let voiced = vec![true; 16];
    let amplitudes: Vec<f64> = (1..=16).map(|i| 350.0 + 25.0 * i as f64).collect();
    let frames = vec![
        (dequantize_fundamental_frequency(60), voiced.clone(), amplitudes.clone()),
        (dequantize_fundamental_frequency(150), voiced.clone(), amplitudes.clone()),
        (dequantize_fundamental_frequency(60), voiced.clone(), amplitudes.clone()),
    ];
    let (float_pcm, fixed_pcm) = run_scenario(&frames);
    let snr = snr_db(&float_pcm, &fixed_pcm);
    assert!(snr >= MIN_SNR_DB, "large pitch jump (Eq. 133) SNR too low: {snr} dB");
}

#[test]
fn synthesize_matches_float_for_voicing_transitions() {
    let omega0 = dequantize_fundamental_frequency(110);
    let amplitudes: Vec<f64> = (1..=14).map(|i| 300.0 + 20.0 * i as f64).collect();
    let zero = vec![0.0; 14];
    let silent = vec![false; 14];
    let voiced = vec![true; 14];
    let frames = vec![
        (omega0, silent.clone(), zero.clone()), // Eq. 130: silence.
        (omega0, voiced.clone(), amplitudes.clone()), // Eq. 132: silence -> voiced.
        (omega0, voiced.clone(), amplitudes.clone()), // Eq. 134/133: steady voiced.
        (omega0, silent.clone(), zero.clone()), // Eq. 131: voiced -> silence.
    ];
    let (float_pcm, fixed_pcm) = run_scenario(&frames);
    let snr = snr_db(&float_pcm, &fixed_pcm);
    assert!(snr >= MIN_SNR_DB, "voicing transitions SNR too low: {snr} dB");
}

#[test]
fn synthesize_matches_float_for_a_quiet_voiced_frame() {
    let omega0 = dequantize_fundamental_frequency(130);
    let voiced = vec![true; 12];
    let amplitudes: Vec<f64> = (1..=12).map(|i| 0.5 + 0.3 * i as f64).collect();
    let frames: Vec<_> = (0..4).map(|_| (omega0, voiced.clone(), amplitudes.clone())).collect();
    let (float_pcm, fixed_pcm) = run_scenario(&frames);
    let snr = snr_db(&float_pcm, &fixed_pcm);
    assert!(snr >= MIN_SNR_DB, "quiet voiced frame SNR too low: {snr} dB");
}

#[test]
fn synthesize_matches_float_for_many_harmonics_including_dithered_range() {
    // A high L~ (well past floor(L~/4), i.e. inside the phase-dither-eligible range Eq. 140's own
    // second branch covers) with a realistic mix of voiced/unvoiced harmonics. Pitch varies slightly
    // frame to frame (real decoded speech never holds exactly one Q16.16 value for several frames
    // running) rather than repeating one `b0` exactly. Amplitudes are kept moderate (not the louder
    // scale some other tests here use): summed constructively across many harmonics plus Eq. 127's own
    // factor of 2, a large enough per-harmonic amplitude can legitimately saturate this port's `i32`
    // Q16.16 PCM output (see `VoicedState::synthesize`'s own doc comment on its final saturating cast)
    // -- real, correctly-handled clipping, but not what *this* test means to exercise, so it stays
    // clear of that regime; `synthesize_clips_rather_than_wraps_on_a_very_loud_many_harmonic_frame`
    // below covers the saturation case on its own. `l_hat=40`, not higher: this is comfortably past
    // `floor(L~/4)` to exercise the dither branch, while staying clear of
    // `synthesize_characterizes_the_annex_a_initial_omega0_quantization_offset`'s own dedicated,
    // separately-documented cold-start territory (`l_hat` above ~45-50 starts needing that test's own
    // lower bar, for a reason unrelated to what this test means to cover).
    let l_hat = 40usize;
    let b0_values = [30u32, 31, 30, 29];
    let voiced: Vec<bool> = (0..l_hat).map(|i| i % 4 != 0).collect();
    let amplitudes: Vec<f64> = (1..=l_hat).map(|i| 20.0 + 2.0 * i as f64).collect();
    let frames: Vec<_> = b0_values
        .iter()
        .map(|&b0| (dequantize_fundamental_frequency(b0), voiced.clone(), amplitudes.clone()))
        .collect();
    let (float_pcm, fixed_pcm) = run_scenario(&frames);
    let snr = snr_db(&float_pcm, &fixed_pcm);
    assert!(snr >= MIN_SNR_DB, "many-harmonic dithered-range SNR too low: {snr} dB");
}

/// A very loud, maximum-harmonic-count frame -- individual amplitudes chosen so a real, constructive
/// alignment of harmonics at a pitch pulse peak genuinely exceeds `i32`'s Q16.16 range (`+-32768.0` in
/// real units, after Eq. 127's own factor of 2). Confirms the *type* of divergence this produces is
/// ordinary clipping (a bounded difference, comparable in scale to how far over range the true sum
/// went), not silent wraparound (a huge, effectively-random-sign difference) -- found the hard way: an
/// earlier version of `VoicedState::synthesize` used a plain `as i32` cast here, which measured as a
/// `max_diff` of `~65500` (about `i32::MAX`'s own Q16.16 scale) at `l_hat=50` with realistic-looking
/// amplitudes, the unmistakable signature of a sign-flipped wrapped sample, not precision loss. This
/// doesn't hold to the usual 40 dB bar (float has no such range limit to clip against), only to "no
/// wraparound-scale divergence."
#[test]
fn synthesize_clips_rather_than_wraps_on_a_very_loud_many_harmonic_frame() {
    let l_hat = fixed_v::MAX_HARMONICS;
    let omega0 = dequantize_fundamental_frequency(30);
    let voiced = vec![true; l_hat];
    let amplitudes: Vec<f64> = (1..=l_hat).map(|i| 150.0 + 10.0 * i as f64).collect();
    let frames = vec![(omega0, voiced, amplitudes)];
    let (float_pcm, fixed_pcm) = run_scenario(&frames);
    let max_float = float_pcm.iter().map(|s| s.abs()).fold(0.0f64, f64::max);
    let max_diff = float_pcm
        .iter()
        .zip(fixed_pcm.iter())
        .map(|(a, b)| (a - b).abs())
        .fold(0.0f64, f64::max);
    // A wrapped sample's difference from float is on the order of `i32::MAX`'s own real-unit scale
    // (`~32768`); ordinary clipping differs from float by, at most, how far over `32768` float's own
    // (unclipped) peak reached. `2.0 *` gives headroom without being anywhere near wraparound's scale.
    assert!(
        max_diff <= 2.0 * max_float,
        "divergence looks like wraparound, not clipping: max_diff={max_diff} vs max_float={max_float}"
    );
}

#[test]
fn synthesize_rejects_a_length_mismatch() {
    let mut state = fixed_v::VoicedState::new();
    let noise = FixedNoiseState::new();
    let voiced = vec![true; 5];
    let amplitudes = vec![to_q16(100.0); 6];
    assert!(state.synthesize(&noise, to_q32(0.1), &voiced, &amplitudes).is_none());
}

#[test]
fn synthesize_rejects_more_harmonics_than_max_harmonics() {
    let mut state = fixed_v::VoicedState::new();
    let noise = FixedNoiseState::new();
    let voiced = vec![true; fixed_v::MAX_HARMONICS + 1];
    let amplitudes = vec![to_q16(100.0); fixed_v::MAX_HARMONICS + 1];
    assert!(state.synthesize(&noise, to_q32(0.1), &voiced, &amplitudes).is_none());
}

/// A realistic long call: 200 frames (4 real seconds) of a sustained vowel with a gentle vibrato-like
/// pitch contour (a low-frequency sine sweep over `b0`). Also this port's own regression coverage for
/// the reason `psi`/`phi` are stored as wrapping phases rather than growing Q16.16 radians in the
/// first place: a naive growing-radian accumulator would overflow within a few hundred frames, long
/// before a real call ends. An earlier version of `trig::phase_from_radians_q16`'s own phase-per-turn
/// conversion divided by `fixed_ops::PI_Q16_16` (a Q16.16, `~2 ppm`-rounded value of pi) instead of the
/// wider [`trig::PI_Q48`] it uses now; that `~2 ppm` error acted as a *scale error on the frequency
/// itself* inside a phase accumulator that runs forever, so SNR here degraded by `~6 dB` per doubling
/// of frame count regardless of whether the pitch was held constant or genuinely varying (both landed
/// around `25 dB` at 200 frames) -- see `trig::PI_Q48`'s own doc comment for the full account. With
/// that fixed, this run's SNR no longer depends on frame count at all.
#[test]
fn synthesize_matches_float_across_a_long_run_with_naturally_varying_pitch() {
    let voiced = vec![true; 20];
    let amplitudes: Vec<f64> = (1..=20).map(|i| 250.0 + 20.0 * i as f64).collect();
    let frames: Vec<_> = (0..200)
        .map(|i| {
            let phase = 2.0 * std::f64::consts::PI * i as f64 / 37.0;
            let b0 = (90.0 + 8.0 * phase.sin()).round() as u32;
            (dequantize_fundamental_frequency(b0), voiced.clone(), amplitudes.clone())
        })
        .collect();
    let (float_pcm, fixed_pcm) = run_scenario(&frames);
    let snr = snr_db(&float_pcm, &fixed_pcm);
    assert!(snr >= MIN_SNR_DB, "long run with naturally varying pitch SNR too low: {snr} dB");
}

/// Characterizes, rather than chases, the one bounded gap between this port and its float sibling that
/// remains after `trig::PI_Q48` fixed the unbounded per-frame drift the other long-run tests above used
/// to show: `VoicedState::new()`'s own Annex A initial state sets `omega0(-1) = 0.02985 * pi`, a real
/// number float's own `VoicedState` stores exactly (a private `f64` literal) but this crate's own
/// `OMEGA0_INITIAL_Q32` can only ever approximate to its `32` fractional bits (the nearest
/// representable value). The figures below (`~0.00032 rad` at `l=1` up to `~0.018 rad` at `l=56`) are
/// from when this constant was Q16.16 (`OMEGA0_INITIAL_Q16`, cold-start SNR then ~35-40 dB); at Q32
/// the offset is 65536 times smaller and the same scenario measures 97.7 dB. Since
/// `psi_l`/`phi_l` update *every* harmonic `1..=56` on *every* call regardless of voicing (Eq. 139-140),
/// this one-time, per-harmonic quantization error is folded into the phase accumulator on the very
/// first `synthesize()` call and never resolves afterwards (confirmed directly: it persists unchanged
/// into a second identical frame) -- it also does not grow further, unlike the drift `PI_Q48` fixed, so
/// tests with a moderate harmonic count comfortably clear this port's usual 40 dB bar despite it. The
/// error is proportional to harmonic number `l` (`diff * l * N/2`, where `diff` is `OMEGA0_INITIAL_Q16`
/// as a real number minus the exact `0.02985 * pi`): `~0.00032 rad` at `l=1` up to `~0.018 rad` at
/// `l=56` -- confirmed directly against a dedicated per-harmonic isolation sweep (a single active
/// harmonic at a time, amplitude 500), whose measured peak-sample divergence matched this formula's own
/// prediction to within `~15%` at every harmonic tested from `l=1` to `l=50`, not just in the same
/// order of magnitude. This is inherent to any fixed-point implementation needing to represent the same
/// real-valued Annex A default, not a defect to chase to zero -- the same class of accepted limitation
/// as `mbe_speech.rs`'s own floor-crossing rate.
#[test]
fn synthesize_characterizes_the_annex_a_initial_omega0_quantization_offset() {
    const COLD_START_MIN_SNR_DB: f64 = 80.0;
    let l_hat = fixed_v::MAX_HARMONICS;
    let omega0 = dequantize_fundamental_frequency(30);
    let voiced = vec![true; l_hat];
    // Kept moderate, not the louder scale `synthesize_clips_rather_than_wraps_on_a_very_loud_many_
    // harmonic_frame` deliberately uses -- this test means to isolate the Annex A phase offset alone,
    // not mix it with real, correctly-handled clipping divergence.
    let amplitudes: Vec<f64> = (1..=l_hat).map(|i| 20.0 + 2.0 * i as f64).collect();
    let frames = vec![(omega0, voiced, amplitudes)];
    let (float_pcm, fixed_pcm) = run_scenario(&frames);
    let snr = snr_db(&float_pcm, &fixed_pcm);
    eprintln!("cold-start voiced SNR {snr:.1} dB");
    assert!(
        snr >= COLD_START_MIN_SNR_DB,
        "cold-start, max-harmonic-count SNR got worse than the characterized Annex A offset: {snr} dB"
    );
}
