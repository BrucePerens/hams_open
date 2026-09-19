// SPDX-License-Identifier: LGPL-3.0-or-later
//! Cross-checks the fixed-point D-STAR bits-to-PCM decoder
//! (`ambe::fixed::dstar::synthesis::DStarSynthesisDecoder`) against the floating-point one
//! (`ambe::float::dstar::synthesis`) on the same logical 72-bit frames: real speech (frames built by
//! the float encoder from the Open Speech Repository fixture), tone frames, and the mbelib bad-frame
//! policy.

mod fixed_synthesis_common;
use fixed_synthesis_common::*;

use ham_digital_modes::ambe::fixed::dstar::synthesis::DStarSynthesisDecoder as FixedDecoder;
use ham_digital_modes::ambe::float::dstar::decode::parse_frame;
use ham_digital_modes::ambe::float::dstar::encode::build_tone_frame;
use ham_digital_modes::ambe::float::dstar::encoder::Encoder;
use ham_digital_modes::ambe::float::dstar::synthesis::DStarSynthesisDecoder as FloatDecoder;
use ham_digital_modes::ambe::float::tone_synthesis::dstar_tone_amplitude;

fn encode_speech(frames: usize) -> Vec<u128> {
    let mut enc = Encoder::new();
    enc.push_samples(&speech_samples(frames));
    let mut out = Vec::new();
    while let Some(f) = enc.next_frame() {
        out.push(f);
    }
    out.extend(enc.finish());
    out.truncate(frames);
    assert_eq!(out.len(), frames);
    out
}

/// Decodes `frames` on both sides, returning per-frame `(float, fixed)` PCM.
fn decode_both(frames: &[u128]) -> Vec<(Vec<f64>, Vec<f64>)> {
    let mut float = FloatDecoder::new();
    let mut fixed = FixedDecoder::new();
    frames
        .iter()
        .map(|&f| {
            let a = float.decode_frame(f).expect("float frame");
            let b = fixed.decode_frame(f).expect("fixed frame");
            (a.to_vec(), from_q16_i64(&b))
        })
        .collect()
}

fn concat_snr(pairs: &[(Vec<f64>, Vec<f64>)]) -> f64 {
    let a: Vec<f64> = pairs.iter().flat_map(|p| p.0.iter().copied()).collect();
    let b: Vec<f64> = pairs.iter().flat_map(|p| p.1.iter().copied()).collect();
    snr_db(&a, &b)
}

/// Measured (release build, 40 frames of `OSR_us_000_0010_8k.wav`, whose first ~1 s is quiet
/// lead-in): the first frame agrees at 60.4 dB and the whole 40 frames at 56.5 dB. Before the fixed
/// decoder carried its pitch at Q32 radians/sample (`W0_TABLE_Q32`) these were 56.5 dB and 23.0 dB:
/// a Q16.16 pitch has ~0.1% relative error at the lowest pitches, and a harmonic's phase accumulator
/// multiplies that error by the harmonic number and integrates it every sample, so the decline was
/// pitch quantization, not synthesis arithmetic (see
/// `speech_snr_decline_is_pitch_quantization_not_synthesis_arithmetic` below). The remaining gap is
/// the shared Q16.16 amplitude/table arithmetic and the interpolated sine table.
const SPEECH_FIRST_FRAME_MIN_SNR_DB: f64 = 50.0;
const SPEECH_40_FRAME_MIN_SNR_DB: f64 = 45.0;

#[test]
fn speech_frames_agree_with_float_on_the_first_frame_and_track_thereafter() {
    let frames = encode_speech(40);
    let pairs = decode_both(&frames);
    let first = snr_db(&pairs[0].0, &pairs[0].1);
    let per_frame: Vec<f64> = pairs.iter().map(|p| snr_db(&p.0, &p.1)).collect();
    let total = concat_snr(&pairs);
    eprintln!("dstar speech: first-frame SNR {first:.1} dB, 40-frame SNR {total:.1} dB");
    eprintln!("dstar per-frame SNR: {:?}", per_frame.iter().map(|s| (s * 10.0).round() / 10.0).collect::<Vec<_>>());
    let loud = pairs.iter().map(|p| rms(&p.0)).fold(0.0, f64::max);
    assert!(loud > 100.0, "float speech unexpectedly quiet ({loud})");
    assert!(first >= SPEECH_FIRST_FRAME_MIN_SNR_DB, "first frame {first}");
    assert!(total >= SPEECH_40_FRAME_MIN_SNR_DB, "40-frame {total}");
    // The fixed output is at the right level (within 1 dB of the float rms over the run).
    let fl: Vec<f64> = pairs.iter().flat_map(|p| p.0.iter().copied()).collect();
    let fx: Vec<f64> = pairs.iter().flat_map(|p| p.1.iter().copied()).collect();
    let level_db = 20.0 * (rms(&fx) / rms(&fl)).log10();
    assert!(level_db.abs() < 1.0, "level differs by {level_db} dB");
}

/// Re-runs the float side with the *fixed* decoder's own (Q32-quantized) pitch substituted for its
/// exact one, everything else float. If the fixed synthesis were losing accuracy anywhere else
/// (amplitudes, enhancement, voiced/unvoiced arithmetic) this comparison would still be low.
#[test]
fn speech_snr_decline_is_pitch_quantization_not_synthesis_arithmetic() {
    use ham_digital_modes::ambe::fixed::dstar::decode::{dequantize as fixed_dequantize, DequantizedFrame as FixedDq};
    use ham_digital_modes::ambe::fixed::general::mbe_speech::MbeDecoderState;
    use ham_digital_modes::ambe::float::dstar::decode::{
        dequantize as float_dequantize, DStarDecoderState, DequantizedFrame as FloatDq,
    };
    use ham_digital_modes::ambe::float::mbe_synthesis::MbeSynthesizer as FloatSynth;

    let frames = encode_speech(40);
    let mut fixed = FixedDecoder::new();
    let mut fixed_state = MbeDecoderState::initial();
    let mut float_state = DStarDecoderState::initial();
    let mut synth = FloatSynth::new();
    let (mut reference, mut test) = (Vec::new(), Vec::new());
    for &f in &frames {
        let parsed = parse_frame(f);
        let fixed_pcm = fixed.decode_frame(f).unwrap();
        let (FixedDq::Speech(fp), FloatDq::Speech(p)) =
            (fixed_dequantize(parsed.d, &mut fixed_state), float_dequantize(parsed.d, &mut float_state))
        else {
            panic!("speech test frames expected");
        };
        let w0 = fp.w0_q32 as f64 / 4294967296.0;
        let pcm = synth.synthesize_speech(w0, &p.voiced, &p.ml, parsed.epsilon_c0, parsed.epsilon_c1).unwrap();
        reference.extend(pcm);
        test.extend(from_q16_i64(&fixed_pcm));
    }
    let snr = snr_db(&reference, &test);
    eprintln!("dstar speech vs float-with-quantized-pitch: {snr:.1} dB over 40 frames");
    assert!(snr >= 40.0, "{snr}");
}

/// Measured: single tones (indices 5..122, volumes 100..240, 5 frames each) agree at 98.7-124 dB and
/// DTMF digits at 95.8-96.0 dB (the residual is the quarter-wave sine table's interpolation error and
/// the Q16.16 rounding of the output).
const TONE_MIN_SNR_DB: f64 = 90.0;

fn check_tone_run(frame: u128, frames: usize, label: &str) -> f64 {
    let pairs = decode_both(&vec![frame; frames]);
    let snr = concat_snr(&pairs);
    eprintln!("dstar tone {label}: {snr:.1} dB over {frames} frames");
    snr
}

#[test]
fn single_tones_match_float_across_frequency_and_level() {
    for index in [5u32, 8, 32, 60, 100, 122] {
        for volume in [100u32, 150, 180, 210, 240] {
            let snr = check_tone_run(build_tone_frame(index, volume), 5, &format!("index {index} volume {volume}"));
            assert!(snr >= TONE_MIN_SNR_DB, "index {index} volume {volume}: {snr} dB");
        }
    }
}

#[test]
fn dtmf_digits_match_float() {
    for row in 0..4u32 {
        for col in 0..4u32 {
            let snr = check_tone_run(build_tone_frame(128 + row + 4 * col, 180), 5, &format!("dtmf {row},{col}"));
            assert!(snr >= TONE_MIN_SNR_DB, "dtmf {row},{col}: {snr} dB");
        }
    }
}

#[test]
fn tone_level_follows_the_measured_volume_curve() {
    for volume in [120u32, 150, 180, 210] {
        let mut fixed = FixedDecoder::new();
        let pcm = from_q16_i64(&fixed.decode_frame(build_tone_frame(32, volume)).unwrap());
        let peak = pcm.iter().fold(0.0_f64, |m, &s| m.max(s.abs()));
        let expected = dstar_tone_amplitude(volume);
        assert!((peak / expected - 1.0).abs() < 0.03, "volume {volume}: peak {peak} vs {expected}");
    }
}

#[test]
fn undecodable_tone_codes_are_silent_like_float() {
    // Index 3: invalid single tone; 150: a dual-tone code with no identified digit.
    for index in [3u32, 150] {
        let pairs = decode_both(&[build_tone_frame(index, 180)]);
        assert!(pairs[0].0.iter().all(|&s| s == 0.0), "float index {index} must be silent");
        assert!(pairs[0].1.iter().all(|&s| s == 0.0), "fixed index {index} must be silent");
    }
}

#[test]
fn tone_then_speech_then_tone_restarts_the_tone_phase_like_float() {
    let speech = encode_speech(8);
    let tone = build_tone_frame(40, 180);
    let mut seq = vec![tone, tone];
    seq.extend(&speech[..3]);
    seq.extend([tone, tone]);
    let pairs = decode_both(&seq);
    for i in [0usize, 1, 5, 6] {
        let snr = snr_db(&pairs[i].0, &pairs[i].1);
        assert!(snr >= TONE_MIN_SNR_DB, "tone frame {i}: {snr} dB");
    }
}

/// Four corrected errors in total (two in each of C0 and C1), as in the float decoder's own test.
fn corrupt(frame: u128) -> u128 {
    let bad = frame ^ (1u128 << 70) ^ (1u128 << 60) ^ (1u128 << 45) ^ (1u128 << 35);
    let parsed = parse_frame(bad);
    assert!(parsed.epsilon_c0 + parsed.epsilon_c1 > 3, "corruption must exceed the error threshold");
    bad
}

#[test]
fn bad_frame_policy_matches_float_repeat_three_times_then_mute_then_recover() {
    let speech = encode_speech(12);
    let mut seq: Vec<u128> = speech[..4].to_vec();
    // Three repeats, then the fourth consecutive bad frame mutes and reinitializes.
    for &f in &speech[4..8] {
        seq.push(corrupt(f));
    }
    // Recovery: clean frames again, then one more bad frame (repeat count restarted), then clean.
    seq.extend(&speech[8..10]);
    seq.push(corrupt(speech[10]));
    seq.push(speech[11]);

    let pairs = decode_both(&seq);
    for (i, (fl, fx)) in pairs.iter().enumerate() {
        let float_silent = fl.iter().all(|&s| s == 0.0);
        let fixed_silent = fx.iter().all(|&s| s == 0.0);
        assert_eq!(float_silent, fixed_silent, "frame {i}: silence disagrees");
        eprintln!("dstar bad-frame seq frame {i}: SNR {:.1} dB (silent={float_silent})", snr_db(fl, fx));
    }
    // Frame 7 is the fourth consecutive bad frame: muted on both sides.
    assert!(pairs[7].0.iter().all(|&s| s == 0.0) && pairs[7].1.iter().all(|&s| s == 0.0));
    // The three repeats resynthesize the previous parameters and track float like ordinary frames do.
    for (i, (fl, fx)) in pairs.iter().enumerate().take(7).skip(4) {
        assert!(rms(fx) > 1.0, "repeat {i} should still sound");
        let snr = snr_db(fl, fx);
        assert!(snr >= 12.0, "repeat frame {i}: {snr} dB");
    }
    // After the mute both decoders were reinitialized, so the recovery frames start from the same
    // state on both sides and the first one agrees as well as a first frame does.
    let recovered = snr_db(&pairs[8].0, &pairs[8].1);
    assert!(recovered >= 30.0, "first frame after mute: {recovered} dB");
}
