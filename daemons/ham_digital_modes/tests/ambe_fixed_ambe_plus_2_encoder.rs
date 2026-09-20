// SPDX-License-Identifier: LGPL-3.0-or-later
//! Parity of the fixed-point AMBE+2 `Encoder` (`ambe::fixed::ambe_plus_2::encoder`) with the float one
//! (`ambe::float::ambe_plus_2::encoder`) on the first 150 frames of each of the four OSR speech files, plus tone round trips
//! and a check that no speech frame is emitted as a tone frame. Both streams are decoded with the float decoder.
//! `--nocapture` prints the measured numbers; the thresholds below sit just under them. Measured: the first 150 frames
//! of all four files are bit-identical (600/604 frames, decoded SNR above 200 dB); in a mid-speech window (frames
//! 600..750) 97-99% of frames are identical; over the whole of the four files (D-STAR, 7776 frames) 77% of frames are
//! identical, b0 equal 99.95%, b1 equal 99.97%, envelope correlation above 0.999. Teacher-forced (the fixed quantizer
//! fed the float encoder's own decoder state each frame) only 18 of 6004 frames differ, all codebook near-ties, so the
//! whole-file differences are the closed loop amplifying those near-ties, not arithmetic error in a single frame.
#![cfg(feature = "ambe_plus_2")]

mod common;

use common::{compare_streams, FrameView, Parity};
use ham_digital_modes::ambe::fixed::ambe_plus_2::encoder::Encoder as FixedEncoder;
use ham_digital_modes::ambe::float::ambe_plus_2::decode::{classify_b0, extract_raw_parameters, FrameKind};
use ham_digital_modes::ambe::float::ambe_plus_2::parse_frame;
use ham_digital_modes::ambe::float::ambe_plus_2::encoder::Encoder as FloatEncoder;
use ham_digital_modes::ambe::float::ambe_plus_2::synthesis::AmbePlus2SynthesisDecoder;

fn view(frame: u128) -> FrameView {
    let raw = extract_raw_parameters(parse_frame(frame).d);
    FrameView { tone: classify_b0(raw.b0) != FrameKind::Speech, b0: raw.b0, b1: raw.b1 }
}

fn encode_fixed(pcm: &[i16], flush: bool) -> Vec<u128> {
    let mut enc = FixedEncoder::new();
    let mut out = Vec::new();
    for chunk in pcm.chunks(97) {
        enc.push_samples(chunk);
        while let Some(f) = enc.next_frame() {
            out.push(f);
        }
    }
    if flush {
        out.extend(enc.finish());
    }
    out
}

fn encode_float(pcm: &[i16], flush: bool) -> Vec<u128> {
    let mut enc = FloatEncoder::new();
    let mut out = Vec::new();
    for chunk in pcm.chunks(97) {
        let c: Vec<f64> = chunk.iter().map(|&s| s as f64).collect();
        enc.push_samples(&c);
        while let Some(f) = enc.next_frame() {
            out.push(f);
        }
    }
    if flush {
        out.extend(enc.finish());
    }
    out
}

/// `frames` frames of the file starting at frame `start` (both encoders start fresh at the window).
fn speech_pcm(path: &str, start: usize, frames: usize) -> Vec<i16> {
    let pcm = common::read_wav_mono_i16(path);
    let lo = (start * 160).min(pcm.len());
    let hi = (lo + frames * 160).min(pcm.len());
    pcm[lo..hi].to_vec()
}

fn parity(name: &str, start: usize, frames: usize) -> Parity {
    let mut p = Parity::default();
    for path in common::OSR_FILES {
        let pcm = speech_pcm(path, start, frames);
        let (fx, fl) = (encode_fixed(&pcm, true), encode_float(&pcm, true));
        compare_streams(&mut p, &fx, &fl, view, || {
            let mut d = AmbePlus2SynthesisDecoder::new();
            Box::new(move |f| d.decode_frame(f))
        });
    }
    p.report(name);
    p
}

#[test]
fn speech_parity_with_the_float_encoder_first_150_frames() {
    let p = parity("AMBE+2 first 150 frames", 0, common::parity_frames());
    assert_eq!(p.count_mismatch, 0);
    assert!(p.frames >= 4 * (common::parity_frames().min(1000) - 2));
    assert_eq!(p.tone_frames_fixed, 0, "no speech frame may be emitted as a tone frame");
    assert_eq!(p.tone_frames_float, 0, "no speech frame may be emitted as a tone frame");
    assert!(p.frac(p.identical) >= 0.99, "identical frames {}", p.frac(p.identical));
    assert!(p.frac(p.b0_within_1) >= 0.999 && p.frac(p.b0_equal) >= 0.99);
    assert!(p.frac(p.b1_equal) >= 0.99);
    assert!(p.snr_db() >= 100.0, "decoded SNR {} dB", p.snr_db());
    assert!(p.min_envelope_corr() >= 0.9999);
}

/// Mid-speech window (frames 600..750): the closed loop (each frame's prediction comes from the previous decoded frame)
/// amplifies the near-ties between the two implementations' codebook searches, so agreement here is lower than in the
/// quiet opening frames.
#[test]
fn speech_parity_with_the_float_encoder_mid_file_window() {
    let p = parity("AMBE+2 frames 600..750", 600, 150);
    assert_eq!(p.count_mismatch, 0);
    assert_eq!(p.tone_frames_fixed, 0);
    assert_eq!(p.tone_frames_float, 0);
    // Measured with the input high-pass filter (Eq. 3) in both encoders: 91.3% identical, b0 equal 99.8%, b1 equal 100%,
    // decoded SNR 15.7 dB, envelope correlation above 0.9995. The float filter keeps its state in `f64` and the fixed one
    // in Q16, so a rare 1-LSB rounding difference in the filtered input flips one near-tie codebook decision and the
    // closed loop carries it forward; the pitch and voicing decisions and the envelope still agree.
    assert!(p.frac(p.identical) >= 0.88, "identical frames {}", p.frac(p.identical));
    // One frame in 600 lands on a different pitch index (a tie between two candidate periods).
    assert!(p.frac(p.b0_within_1) >= 0.995 && p.frac(p.b0_equal) >= 0.99);
    assert!(p.frac(p.b1_equal) >= 0.99);
    assert!(p.snr_db() >= 12.0, "decoded SNR {} dB", p.snr_db());
    assert!(p.min_envelope_corr() >= 0.999);
}

fn sine(freqs: &[f64], amp: f64, frames: usize) -> Vec<i16> {
    (0..160 * frames)
        .map(|i| freqs.iter().map(|&hz| amp * (2.0 * std::f64::consts::PI * hz * i as f64 / 8000.0).sin()).sum::<f64>().round() as i16)
        .collect()
}

#[test]
fn tone_round_trip_dtmf_and_one_kilohertz() {
    use ham_digital_modes::ambe::float::ambe_plus_2::decode::{classify_tone_idx, decode_tone_idx, dtmf_digit_from_tone_idx, ToneIdentity};
    // Frames before the flush only: the padded tail frames are partly silence, not tone.
    let dtmf = encode_fixed(&sine(&[770.0, 1336.0], 4000.0, 12), false);
    assert!(dtmf.len() >= 8);
    assert_eq!(dtmf, encode_float(&sine(&[770.0, 1336.0], 4000.0, 12), false), "tone frames should equal the float encoder's");
    for &f in &dtmf {
        let idx = decode_tone_idx(parse_frame(f).d).expect("tone frame");
        assert_eq!(dtmf_digit_from_tone_idx(idx), Some((1, 1))); // digit 5
    }
    let single = encode_fixed(&sine(&[1000.0], 4000.0, 12), false);
    assert!(single.len() >= 8);
    assert_eq!(single, encode_float(&sine(&[1000.0], 4000.0, 12), false));
    for &f in &single {
        let idx = decode_tone_idx(parse_frame(f).d).expect("tone frame");
        assert!(matches!(classify_tone_idx(idx), ToneIdentity::SingleTone { hz } if (hz - 1000.0).abs() < 16.0));
    }
    let mut dec = AmbePlus2SynthesisDecoder::new();
    let last = single.iter().map(|&f| dec.decode_frame(f).unwrap()).last().unwrap();
    assert!(last.iter().any(|&s| s.abs() > 100.0));
}

/// The integer pitch quantizer agrees with the float one on every refined period the analyzer can produce
/// (`P = p8 / 8` samples, `21 <= P <= 122`, exact eighths).
#[test]
fn pitch_quantizer_matches_the_float_one_for_every_period() {
    use ham_digital_modes::ambe::fixed::ambe_plus_2::encode::quantize_pitch_p8;
    use ham_digital_modes::ambe::float::ambe_plus_2::quantize::quantize_pitch;
    let mut differ = 0;
    for p8 in 21 * 8..=122 * 8 {
        let w0 = 2.0 * std::f64::consts::PI / (p8 as f64 / 8.0);
        differ += usize::from(quantize_pitch_p8(p8) != quantize_pitch(w0));
    }
    assert_eq!(differ, 0);
}
