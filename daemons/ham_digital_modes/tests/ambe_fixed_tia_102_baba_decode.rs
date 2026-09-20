// SPDX-License-Identifier: LGPL-3.0-or-later
//! Cross-checks `ambe::fixed::tia_102_baba::decode::DecoderState` against its floating-point sibling
//! `ambe::float::tia_102_baba::decode::DecoderState`, decoding the *same* real encoded bits on both sides
//! -- the first true end-to-end (bits-in, PCM-out) fixed-vs-float comparison for this port, one level
//! above `ambe_fixed_tia_102_baba_synthesis.rs`'s own already-decoded-parameters-in comparison.
//!
//! Frames are built the same way `ambe::float::tia_102_baba::decode`'s own inline
//! `build_synthetic_voiced_frame` test helper does: hand-encoded via the shared (pure bit-level, not
//! duplicated for fixed point) `parameter_encoding`/`tables`/`bit_prioritization`/`encode_code_vectors`
//! machinery, not through the full pitch-estimation encoder (`encode_frame`), which needs a real
//! analysis frame this test has no reason to construct.
//!
//! This IS, in fact, an instance of the "exact-vs-quantized omega0" comparison
//! `ambe_fixed_voiced_synthesis.rs`'s own `run_scenario` had to correct for -- and deliberately so,
//! unlike that file's own fair comparison. Float's `DecoderState` dequantizes `b0` to an exact `f64`
//! internally; fixed's dequantizes the same `b0` via a Q16.16 table lookup. The two therefore land on
//! genuinely different `omega0_tilde` values (differing by up to a few `1e-6` rad, per
//! `dequantize_fundamental_frequency_q16`'s own table generator, which rounds rather than truncates),
//! and that's the realistic case: a real float decoder and a real fixed decoder given the identical
//! bits really do end up with slightly different omega0 values, because only one of them is limited to
//! Q16.16. `decode_frame_confirms_the_multi_frame_decline_is_from_omega0_quantization_not_amplitude_compounding`
//! below verifies directly that this mismatch, not amplitude-reconstruction compounding, is what drives
//! this file's own multi-frame SNR decline.

use ham_digital_modes::ambe::fixed::tia_102_baba::decode::{DecoderState as FixedDecoderState, FrameOutcome as FixedFrameOutcome};
use ham_digital_modes::ambe::float::tia_102_baba::decode::{DecoderState as FloatDecoderState, FrameOutcome as FloatFrameOutcome};
use ham_digital_modes::ambe::float::tia_102_baba::parameter_encoding as float_parameter_encoding;
use ham_digital_modes::ambe::float::tia_102_baba::synthesis::SynthesisState as FloatSynthesisState;
use ham_digital_modes::ambe::float::tia_102_baba::vuv;
use ham_digital_modes::ambe::float::tia_102_baba::{bit_prioritization, encode_code_vectors, tables};

const MIN_SNR_DB: f64 = 40.0;

fn from_q16(v: i32) -> f64 {
    v as f64 / 65536.0
}
fn from_q16_i64(v: i64) -> f64 {
    v as f64 / 65536.0
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

/// Hand-encodes one real, valid voiced frame's own `c_hat_0..c_hat_7` for a given `b0` -- the same
/// construction `ambe::float::tia_102_baba::decode`'s own inline `build_synthetic_voiced_frame` test
/// helper uses, generalized to a caller-chosen pitch so a scenario can vary it frame to frame (real
/// decoded speech never holds one `b0` for many frames running -- see
/// `ambe_fixed_voiced_synthesis.rs`'s own doc comment for why that matters for a module with a
/// persistent phase accumulator underneath it).
fn build_synthetic_voiced_frame(b0: u32) -> [u32; 8] {
    let omega0_tilde = float_parameter_encoding::dequantize_fundamental_frequency(b0);
    let l_hat = vuv::harmonics_count(omega0_tilde);
    let k_hat = vuv::frequency_bands_count(l_hat);

    let voiced_bands = vec![true; k_hat as usize];
    let b1 = float_parameter_encoding::encode_voicing_decisions(&voiced_bands);

    let gain: [u8; 5] =
        std::array::from_fn(|i| tables::gain_bit_allocation(l_hat, i as u32 + 2).unwrap().0);
    let higher = tables::higher_order_bit_allocation(l_hat).unwrap().to_vec();
    let b2 = 32u32; // Mid-range gain index.
    let gain_vector: [(u32, u8); 5] =
        std::array::from_fn(|i| ((1u32 << (gain[i] - 1)) & ((1 << gain[i]) - 1), gain[i]));
    let higher_order: Vec<(u32, u8)> = higher
        .iter()
        .filter(|&&w| w > 0)
        .map(|&w| (1u32 & ((1 << w) - 1), w))
        .collect();

    let u = bit_prioritization::prioritize_bits(b0, b1, k_hat, b2, gain_vector, &higher_order, false).unwrap();
    encode_code_vectors(u)
}

#[test]
fn decode_parameters_matches_float_exactly_for_a_real_synthetic_voiced_frame() {
    let c = build_synthetic_voiced_frame(50);

    let mut float_decoder = FloatDecoderState::new();
    let float_params = match float_decoder.decode_parameters(c).unwrap() {
        FloatFrameOutcome::Decoded(p) => p,
        _ => panic!("expected a real decode"),
    };

    let mut fixed_decoder = FixedDecoderState::new();
    let fixed_params = match fixed_decoder.decode_parameters(c).unwrap() {
        FixedFrameOutcome::Decoded(p) => p,
        _ => panic!("expected a real decode"),
    };

    // b0/b1/b2/gain/higher-order recovery is exact bit-level arithmetic, shared unchanged between
    // the two sides -- see this file's own doc comment. These must match exactly, not just closely.
    assert_eq!(fixed_params.bits.b0, float_params.bits.b0);
    assert_eq!(fixed_params.bits.b1, float_params.bits.b1);
    assert_eq!(fixed_params.bits.b2, float_params.bits.b2);
    assert_eq!(fixed_params.bits.gain_vector, float_params.bits.gain_vector);
    assert_eq!(fixed_params.bits.higher_order, float_params.bits.higher_order);
    assert_eq!(fixed_params.l_hat, float_params.l_hat);
    assert_eq!(fixed_params.k_hat, float_params.k_hat);
    assert_eq!(fixed_params.voiced, float_params.voiced);

    let omega0_rel_err = (from_q16(fixed_params.omega0_tilde_q16) - float_params.omega0_tilde).abs()
        / float_params.omega0_tilde;
    assert!(omega0_rel_err < 0.01, "omega0_tilde relative error too high: {omega0_rel_err}");

    assert_eq!(fixed_params.reconstructed_amplitudes_q16.len(), float_params.reconstructed_amplitudes.len());
    for (i, (&fixed_amp_q16, &float_amp)) in fixed_params
        .reconstructed_amplitudes_q16
        .iter()
        .zip(float_params.reconstructed_amplitudes.iter())
        .enumerate()
    {
        let rel_err = (from_q16(fixed_amp_q16) - float_amp).abs() / float_amp.max(1.0);
        assert!(rel_err < 0.01, "harmonic {i}: reconstructed amplitude relative error too high: {rel_err}");
    }
}

#[test]
fn decode_frame_matches_float_across_several_real_frames_with_varying_pitch() {
    let b0_values = [50u32, 51];

    let mut float_decoder = FloatDecoderState::new();
    let mut fixed_decoder = FixedDecoderState::new();
    let mut float_pcm = Vec::new();
    let mut fixed_pcm = Vec::new();
    for &b0 in &b0_values {
        let c = build_synthetic_voiced_frame(b0);
        let float_frame = float_decoder.decode_frame(c).unwrap();
        let fixed_frame = fixed_decoder.decode_frame(c).unwrap();
        float_pcm.extend(float_frame.iter().copied());
        fixed_pcm.extend(fixed_frame.iter().map(|&s| from_q16_i64(s)));
    }
    let snr = snr_db(&float_pcm, &fixed_pcm);
    assert!(snr >= MIN_SNR_DB, "end-to-end decode_frame SNR too low: {snr} dB");
}

/// Characterizes, rather than chases, a genuine but modest multi-frame SNR decline this full
/// bits-to-PCM pipeline shows that neither `ambe_fixed_tia_102_baba_synthesis.rs` (fed already-decoded,
/// already-omega0-matched parameters directly) nor `ambe_fixed_tia_102_baba_reconstruct.rs` (checking only
/// per-value amplitude relative error in isolation) individually exercises. Measured directly
/// (aggregate SNR by frame count, same nearby-`b0` sweep as the test above): with a Q16.16 pitch
/// this was 1: 45.7 dB, 2: 41.1 dB, 3: 40.3 dB, 4: 38.4 dB, 5: 36.1 dB, 8: 32.9 dB -- a real decline,
/// but far gentler than the `~6 dB`-per-doubling signature `trig::PI_Q48` fixed. Carrying the pitch at
/// Q32 radians/sample through synthesis (`OMEGA0_TILDE_Q32`) removes nearly all of it: 1 frame 82.9
/// dB, 8 frames 73.1 dB.
///
/// The cause is the omega0 quantization mismatch this file's own doc comment describes (float
/// dequantizes `b0` to an exact `f64`, fixed via a Q16.16 table), compounding in `voiced_synthesis`'s
/// persistent per-harmonic phase accumulator -- NOT `reconstruct_spectral_amplitudes_q16`'s Eq. 77
/// closed-loop amplitude prediction. An earlier version of this doc comment attributed the decline to
/// amplitude-prediction compounding instead; that was wrong, and disproven directly by
/// `decode_frame_confirms_the_multi_frame_decline_is_from_omega0_quantization_not_amplitude_compounding`
/// below: the closed-loop amplitude-prediction coefficient is well under 1 (bounding, not compounding,
/// any per-frame amplitude error), and forcing the two sides onto the *same* per-frame omega0 while
/// leaving amplitude reconstruction alone makes the decline vanish entirely (SNR stays flat at
/// 50-53 dB, even improving slightly, from frame 1 through frame 8) rather than merely shrink.
#[test]
fn decode_frame_characterizes_multi_frame_decline_from_compounding_already_accepted_tolerances() {
    const MULTI_FRAME_MIN_SNR_DB: f64 = 60.0;
    let b0_values = [50u32, 51, 50, 49, 50, 51, 50, 49];

    let mut float_decoder = FloatDecoderState::new();
    let mut fixed_decoder = FixedDecoderState::new();
    let mut float_pcm = Vec::new();
    let mut fixed_pcm = Vec::new();
    for &b0 in &b0_values {
        let c = build_synthetic_voiced_frame(b0);
        let float_frame = float_decoder.decode_frame(c).unwrap();
        let fixed_frame = fixed_decoder.decode_frame(c).unwrap();
        float_pcm.extend(float_frame.iter().copied());
        fixed_pcm.extend(fixed_frame.iter().map(|&s| from_q16_i64(s)));
    }
    let snr = snr_db(&float_pcm, &fixed_pcm);
    let first_frame = snr_db(&float_pcm[..160], &fixed_pcm[..160]);
    eprintln!("tia-102-baba decode: first-frame SNR {first_frame:.1} dB, 8-frame SNR {snr:.1} dB");
    assert!(
        snr >= MULTI_FRAME_MIN_SNR_DB,
        "multi-frame decline got worse than the characterized rate: {snr} dB"
    );
}

/// Disproves the amplitude-compounding explanation the test above's doc comment once wrongly gave for
/// its own decline, and confirms the real one: re-synthesizes float's own reconstructed amplitudes
/// (untouched) through a *fresh* float `SynthesisState`, but fed fixed's own Q16.16-quantized omega0
/// on every frame instead of float's exact one -- isolating the omega0 mismatch from every other
/// difference between the two decoders. If the omega0 mismatch is really what drives the decline, this
/// "iso" stream should track FIXED's own real PCM closely and *without* decline, since it now shares
/// fixed's exact omega0 sequence and differs from fixed's real output only in amplitude reconstruction
/// (float's own, not fixed's Q16.16 one). Measured directly: SNR(iso, fixed) is flat at 49.9, 50.6,
/// 51.4, 51.9, 52.2, 52.8 dB for frame counts 1, 2, 3, 4, 5, 8 -- no decline at all, confirming omega0
/// quantization mismatch (not amplitude-prediction compounding) is the cause of this file's own
/// `decode_frame_characterizes_multi_frame_decline_from_compounding_already_accepted_tolerances`.
#[test]
fn decode_frame_confirms_the_multi_frame_decline_is_from_omega0_quantization_not_amplitude_compounding() {
    const ISO_MIN_SNR_DB: f64 = 45.0;
    let b0_values = [50u32, 51, 50, 49, 50, 51, 50, 49];

    let mut fixed_decoder_real = FixedDecoderState::new();
    let mut float_decoder_params = FloatDecoderState::new();
    let mut fixed_decoder_params = FixedDecoderState::new();
    let mut float_synth_iso = FloatSynthesisState::new();

    let mut fixed_pcm = Vec::new();
    let mut iso_pcm = Vec::new();

    for &b0 in &b0_values {
        let c = build_synthetic_voiced_frame(b0);

        let fixed_frame = fixed_decoder_real.decode_frame(c).unwrap();
        fixed_pcm.extend(fixed_frame.iter().map(|&s| from_q16_i64(s)));

        let float_params = match float_decoder_params.decode_parameters(c).unwrap() {
            FloatFrameOutcome::Decoded(p) => p,
            _ => panic!("expected a real decode"),
        };
        float_decoder_params.advance_history(&float_params);
        let fixed_params = match fixed_decoder_params.decode_parameters(c).unwrap() {
            FixedFrameOutcome::Decoded(p) => p,
            _ => panic!("expected a real decode"),
        };
        fixed_decoder_params.advance_history(&fixed_params);

        let iso_frame = float_synth_iso
            .synthesize_frame(
                &float_params.reconstructed_amplitudes,
                fixed_params.omega0_tilde_q32 as f64 / 4294967296.0,
                &float_params.voiced,
                &float_params.errors,
            )
            .unwrap();
        iso_pcm.extend(iso_frame.iter().copied());
    }

    let snr = snr_db(&iso_pcm, &fixed_pcm);
    assert!(
        snr >= ISO_MIN_SNR_DB,
        "matching omega0 should eliminate the multi-frame decline, but iso-vs-fixed SNR was {snr} dB"
    );
}

#[test]
fn decode_frame_does_not_panic_on_an_all_zero_first_frame() {
    let mut decoder = FixedDecoderState::new();
    let c = [0u32, 0, 0, 0, 0, 0, 0, 0];
    let _ = decoder.decode_frame(c);
}

/// Real, direct test of section 7.8's own mute requirement, ported from the float sibling's own
/// analogous test: a persistently high running error rate must produce real, bounded comfort noise
/// on the fixed side too, even on the very first frame (no previous frame for a repeat to fall back
/// on, but `synthesize_comfort_frame` has no such dependency).
#[test]
fn a_persistently_high_error_rate_forces_a_mute_producing_real_comfort_noise_even_on_the_first_frame() {
    // No public constructor takes a seeded error_rate directly, so this drives the same effect via
    // several genuinely bad (all-zero, maximal-error) frames in a row, matching how a real corrupted
    // channel would actually raise error_rate_prev over successive frames rather than starting there.
    // `golay_decode`'s `[23,12,7]` code has covering radius 3 and `hamming_decode`'s `[15,11,3]` code
    // has covering radius 1 -- 3 is the true maximum corrected-error count minimum-distance decoding
    // can ever report for a single Golay word, and 1 for a single Hamming word (both codes are
    // "perfect", so every possible received word is within their own covering radius of some
    // codeword). `0xFFF`/`0x7FF` are themselves valid codewords (distance 0, verified directly), so
    // this picks patterns that actually sit at that maximum distance from every codeword instead
    // (checked directly: distances 3,3,3,3,1,1,1, `total=15` every frame, the true worst case). Even
    // at that true maximum, `estimate_errors_q16`'s own `0.95` IIR decay means `rate_q16` only crosses
    // the `0.0875` mute threshold at frame 31 (measured directly) -- so this needs 32 frames, not the
    // 10 an earlier, unverified version of this test used, which never actually reached
    // `FrameOutcome::Mute` at all (a genuinely vacuous test, caught by re-deriving this from the real
    // recursion rather than assuming a small round number would be enough).
    // The decoder demodulates c1..c6 with vectors seeded from the corrected u0 (Eq. 84-94), so the
    // worst-case words are built in the demodulated domain and then modulated back with the same vectors.
    // Hamming words: the first non-codewords found (any non-codeword of a perfect code is at distance 1).
    let m = ham_digital_modes::ambe::float::tia_102_baba::modulation::modulation_vectors(
        ham_digital_modes::ambe::general::fec::golay_decode(0xFFF).0 as u32,
    );
    let worst_hamming: Vec<u32> = (1u16..0x7FFF)
        .filter(|&w| ham_digital_modes::ambe::general::fec::hamming_decode(w).1 == 1)
        .take(3)
        .map(|w| w as u32)
        .collect();
    let bad_c = [
        0xFFFu32,
        0xFFF ^ m[1],
        0xFFF ^ m[2],
        0xFFF ^ m[3],
        worst_hamming[0] ^ m[4],
        worst_hamming[1] ^ m[5],
        worst_hamming[2] ^ m[6],
        0,
    ];
    const ITERATIONS: usize = 32;

    // A separate decoder, driven only through `decode_parameters`, verifies the sequence actually
    // reaches `FrameOutcome::Mute` at some point -- the error-rate IIR update in
    // `estimate_errors_q16` runs identically whether called here or from inside `decode_frame`, so
    // this mirrors the real decoder's own state progression exactly without double-advancing it.
    let mut probe_decoder = FixedDecoderState::new();
    let mut saw_mute = false;
    for _ in 0..ITERATIONS {
        if let Some(FixedFrameOutcome::Mute) = probe_decoder.decode_parameters(bad_c) {
            saw_mute = true;
        }
    }
    assert!(saw_mute, "bad_c never actually accumulated enough corrected-error rate to trigger a mute");

    let mut decoder = FixedDecoderState::new();
    let mut last_pcm = None;
    for _ in 0..ITERATIONS {
        last_pcm = decoder.decode_frame(bad_c);
    }
    if let Some(pcm) = last_pcm {
        for &sample in &pcm {
            assert!(sample.abs() < (1i64 << 40), "unreasonably large sample under a bad-channel run: {sample}");
        }
    }
}
