// SPDX-License-Identifier: LGPL-3.0-or-later
//! Cross-checks the fixed-point AMBE+2 half-rate bits-to-PCM decoder
//! (`ambe::fixed::ambe_plus_2::synthesis::AmbePlus2SynthesisDecoder`) against the floating-point one
//! (`ambe::float::ambe_plus_2::synthesis`) on the same logical 72-bit frames: real speech (frames
//! built by the float encoder from the Open Speech Repository fixture), tone frames (single tone,
//! DTMF, call progress), erasure/silence frames, and the mbelib bad-frame policy.
#![cfg(feature = "ambe_plus_2")]

mod fixed_synthesis_common;
use fixed_synthesis_common::*;

use ham_digital_modes::ambe::fixed::ambe_plus_2::synthesis::AmbePlus2SynthesisDecoder as FixedDecoder;
use ham_digital_modes::ambe::float::ambe_plus_2::decode::RawParameters;
use ham_digital_modes::ambe::float::ambe_plus_2::encode::{build_frame, build_tone_frame};
use ham_digital_modes::ambe::float::ambe_plus_2::encoder::Encoder;
use ham_digital_modes::ambe::float::ambe_plus_2::parse_frame;
use ham_digital_modes::ambe::float::ambe_plus_2::synthesis::AmbePlus2SynthesisDecoder as FloatDecoder;

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

/// Decodes `frames` on both sides, returning per-frame `(float, fixed)` PCM (`None` where both sides
/// return `None`; the two sides returning different `Option`s fails the test).
fn decode_both_opt(frames: &[u128]) -> Vec<Option<(Vec<f64>, Vec<f64>)>> {
    let mut float = FloatDecoder::new();
    let mut fixed = FixedDecoder::new();
    frames
        .iter()
        .enumerate()
        .map(|(i, &f)| match (float.decode_frame(f), fixed.decode_frame(f)) {
            (Some(a), Some(b)) => Some((a.to_vec(), from_q16_i64(&b))),
            (None, None) => None,
            (a, b) => panic!("frame {i}: float {:?} vs fixed {:?}", a.is_some(), b.is_some()),
        })
        .collect()
}

fn decode_both(frames: &[u128]) -> Vec<(Vec<f64>, Vec<f64>)> {
    decode_both_opt(frames).into_iter().map(|p| p.expect("frame decoded")).collect()
}

fn concat_snr(pairs: &[(Vec<f64>, Vec<f64>)]) -> f64 {
    let a: Vec<f64> = pairs.iter().flat_map(|p| p.0.iter().copied()).collect();
    let b: Vec<f64> = pairs.iter().flat_map(|p| p.1.iter().copied()).collect();
    snr_db(&a, &b)
}

/// Measured (release build, 40 frames of `OSR_us_000_0010_8k.wav`, the same fixture and encoder path
/// as `ambe_fixed_dstar_synthesis.rs`): first frame 38.6 dB, per-frame 17 to 67 dB (the low-level
/// unvoiced-dominated frames sit at 59-67 dB), 18.3 dB over all 40 frames. The first frame is just
/// under the 40 dB single-frame bar (D-STAR's is 55 dB) because the pitch quantization error already
/// shows in the first frame here; with the pitch matched the whole 40 frames agree at 56.2 dB.
/// See `ambe_fixed_dstar_synthesis.rs` for the analysis of why the multi-frame figure is below the 40 dB single-frame bar: the
/// fixed decoder's Q16.16 pitch quantization compounds in the harmonic phase accumulators, not
/// synthesis arithmetic, as
/// `speech_snr_decline_is_pitch_quantization_not_synthesis_arithmetic` verifies.
const SPEECH_FIRST_FRAME_MIN_SNR_DB: f64 = 35.0;
const SPEECH_40_FRAME_MIN_SNR_DB: f64 = 15.0;

#[test]
fn speech_frames_agree_with_float_on_the_first_frame_and_track_thereafter() {
    let frames = encode_speech(40);
    let pairs = decode_both(&frames);
    let first = snr_db(&pairs[0].0, &pairs[0].1);
    let per_frame: Vec<f64> = pairs.iter().map(|p| snr_db(&p.0, &p.1)).collect();
    let total = concat_snr(&pairs);
    eprintln!("ambe+2 speech: first-frame SNR {first:.1} dB, 40-frame SNR {total:.1} dB");
    eprintln!("ambe+2 per-frame SNR: {:?}", per_frame.iter().map(|s| (s * 10.0).round() / 10.0).collect::<Vec<_>>());
    let loud = pairs.iter().map(|p| rms(&p.0)).fold(0.0, f64::max);
    assert!(loud > 100.0, "float speech unexpectedly quiet ({loud})");
    assert!(first >= SPEECH_FIRST_FRAME_MIN_SNR_DB, "first frame {first}");
    assert!(total >= SPEECH_40_FRAME_MIN_SNR_DB, "40-frame {total}");
    let fl: Vec<f64> = pairs.iter().flat_map(|p| p.0.iter().copied()).collect();
    let fx: Vec<f64> = pairs.iter().flat_map(|p| p.1.iter().copied()).collect();
    let level_db = 20.0 * (rms(&fx) / rms(&fl)).log10();
    assert!(level_db.abs() < 1.0, "level differs by {level_db} dB");
}

/// Re-runs the float side with the *fixed* decoder's own (Q16.16-quantized) pitch substituted for its
/// exact one, everything else float.
#[test]
fn speech_snr_decline_is_pitch_quantization_not_synthesis_arithmetic() {
    use ham_digital_modes::ambe::fixed::ambe_plus_2::decode::{
        dequantize as fixed_dequantize, extract_raw_parameters, DequantizedFrame as FixedDq,
    };
    use ham_digital_modes::ambe::fixed::general::mbe_speech::MbeDecoderState;
    use ham_digital_modes::ambe::float::ambe_plus_2::decode::{
        dequantize as float_dequantize, DecoderState, DequantizedFrame as FloatDq,
    };
    use ham_digital_modes::ambe::float::mbe_synthesis::MbeSynthesizer as FloatSynth;

    let frames = encode_speech(40);
    let mut fixed = FixedDecoder::new();
    let mut fixed_state = MbeDecoderState::initial();
    let mut float_state = DecoderState::initial();
    let mut synth = FloatSynth::new();
    let (mut reference, mut test) = (Vec::new(), Vec::new());
    for &f in &frames {
        let parsed = parse_frame(f);
        let raw = extract_raw_parameters(parsed.d);
        let fixed_pcm = fixed.decode_frame(f).unwrap();
        let (FixedDq::Speech(fp), FloatDq::Speech(p)) =
            (fixed_dequantize(&raw, &mut fixed_state), float_dequantize(&raw, &mut float_state))
        else {
            panic!("speech test frames expected");
        };
        let w0 = fp.w0_q16 as f64 / 65536.0;
        let pcm = synth.synthesize_speech(w0, &p.voiced, &p.ml, parsed.epsilon_c0, parsed.epsilon_c1).unwrap();
        reference.extend(pcm);
        test.extend(from_q16_i64(&fixed_pcm));
    }
    let snr = snr_db(&reference, &test);
    eprintln!("ambe+2 speech vs float-with-quantized-pitch: {snr:.1} dB over 40 frames");
    assert!(snr >= 40.0, "{snr}");
}

/// Measured: single tones and DTMF/call-progress pairs agree at better than 90 dB (residual: sine
/// table interpolation and Q16.16 output rounding).
const TONE_MIN_SNR_DB: f64 = 90.0;

fn check_tone_run(frame: u128, frames: usize, label: &str) -> f64 {
    let snr = concat_snr(&decode_both(&vec![frame; frames]));
    eprintln!("ambe+2 tone {label}: {snr:.1} dB over {frames} frames");
    snr
}

#[test]
fn single_tones_match_float_and_have_the_measured_level() {
    for tone_idx in [0x05u8, 0x08, 0x20, 0x3C, 0x64, 0x7A] {
        let frame = build_tone_frame(tone_idx, false, 0x800);
        let snr = check_tone_run(frame, 5, &format!("single {tone_idx:#x}"));
        assert!(snr >= TONE_MIN_SNR_DB, "tone {tone_idx:#x}: {snr} dB");
    }
    // Total level 24000 rms: a single tone's peak is 24000*sqrt(2).
    let pcm = from_q16_i64(&FixedDecoder::new().decode_frame(build_tone_frame(0x20, false, 0x800)).unwrap());
    let peak = pcm.iter().fold(0.0_f64, |m, &s| m.max(s.abs()));
    assert!((peak / 33941.1 - 1.0).abs() < 0.01, "peak {peak}");
}

#[test]
fn dtmf_digits_match_float_at_24000_per_tone() {
    for nibble in 0..16u8 {
        let snr = check_tone_run(build_tone_frame(0x80 | nibble, false, 0x800), 5, &format!("dtmf {nibble:#x}"));
        assert!(snr >= TONE_MIN_SNR_DB, "dtmf {nibble:#x}: {snr} dB");
    }
    let pcm = from_q16_i64(&FixedDecoder::new().decode_frame(build_tone_frame(0x81, false, 0x800)).unwrap());
    // Two 24000-peak tones: rms 24000 in total.
    assert!((rms(&pcm) / 24000.0 - 1.0).abs() < 0.03, "rms {}", rms(&pcm));
}

#[test]
fn call_progress_tones_match_float() {
    for (tone_idx, name) in [(0xA0u8, "dial"), (0xA1, "ring"), (0xA2, "busy")] {
        let snr = check_tone_run(build_tone_frame(tone_idx, true, 0x800), 5, name);
        assert!(snr >= TONE_MIN_SNR_DB, "{name}: {snr} dB");
    }
}

#[test]
fn inactive_and_reserved_tone_codes_are_silent_like_float() {
    for (tone_idx, call_progress) in [(0xFFu8, true), (0x00, false), (0x01, false)] {
        let pairs = decode_both(&[build_tone_frame(tone_idx, call_progress, 0x800)]);
        assert!(pairs[0].0.iter().all(|&s| s == 0.0), "float {tone_idx:#x} must be silent");
        assert!(pairs[0].1.iter().all(|&s| s == 0.0), "fixed {tone_idx:#x} must be silent");
    }
}

fn raw(b0: u32) -> RawParameters {
    RawParameters { b0, b1: 5, b2: 10, b3: 100, b4: 50, b5: 3, b6: 4, b7: 5, b8: 2 }
}

#[test]
fn erasure_repeats_and_silence_is_zero_like_float() {
    let speech = encode_speech(3);
    let seq = [
        build_frame(&raw(121)), // erasure before any real frame: None on both sides
        speech[0],
        build_frame(&raw(121)), // erasure repeats the last frame
        build_frame(&raw(123)),
        build_frame(&raw(124)), // silence
        speech[1],
    ];
    let out = decode_both_opt(&seq);
    assert!(out[0].is_none());
    for i in [1usize, 2, 3, 5] {
        let (fl, fx) = out[i].as_ref().unwrap();
        assert!(rms(fx) > 1.0, "frame {i} should sound");
        let snr = snr_db(fl, fx);
        eprintln!("ambe+2 erasure seq frame {i}: {snr:.1} dB");
        assert!(snr >= 12.0, "frame {i}: {snr} dB");
    }
    let (fl, fx) = out[4].as_ref().unwrap();
    assert!(fl.iter().all(|&s| s == 0.0) && fx.iter().all(|&s| s == 0.0));
}

#[test]
fn tone_then_speech_then_tone_restarts_the_tone_phase_like_float() {
    let speech = encode_speech(8);
    let tone = build_tone_frame(0x28, false, 0x800);
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
    for &f in &speech[4..8] {
        seq.push(corrupt(f));
    }
    seq.extend(&speech[8..10]);
    seq.push(corrupt(speech[10]));
    seq.push(speech[11]);

    let pairs = decode_both(&seq);
    for (i, (fl, fx)) in pairs.iter().enumerate() {
        let float_silent = fl.iter().all(|&s| s == 0.0);
        let fixed_silent = fx.iter().all(|&s| s == 0.0);
        assert_eq!(float_silent, fixed_silent, "frame {i}: silence disagrees");
        eprintln!("ambe+2 bad-frame seq frame {i}: SNR {:.1} dB (silent={float_silent})", snr_db(fl, fx));
    }
    assert!(pairs[7].0.iter().all(|&s| s == 0.0) && pairs[7].1.iter().all(|&s| s == 0.0));
    for (i, (fl, fx)) in pairs.iter().enumerate().take(7).skip(4) {
        assert!(rms(fx) > 1.0, "repeat {i} should still sound");
        let snr = snr_db(fl, fx);
        assert!(snr >= 12.0, "repeat frame {i}: {snr} dB");
    }
    let recovered = snr_db(&pairs[8].0, &pairs[8].1);
    assert!(recovered >= 30.0, "first frame after mute: {recovered} dB");
}
