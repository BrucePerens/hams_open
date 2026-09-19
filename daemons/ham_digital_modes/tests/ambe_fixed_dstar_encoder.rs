// SPDX-License-Identifier: LGPL-3.0-or-later
//! Parity of the fixed-point D-STAR `Encoder` (`ambe::fixed::dstar::encoder`) with the float one
//! (`ambe::float::dstar::encoder`) on the first 150 frames of each of the four OSR speech files, plus tone round trips
//! and a check that no speech frame is emitted as a tone frame. Both streams are decoded with the float decoder.
//! `--nocapture` prints the measured numbers; the thresholds below sit just under them. Measured: the first 150 frames
//! of all four files are bit-identical (600/604 frames, decoded SNR above 200 dB); in a mid-speech window (frames
//! 600..750) 97-99% of frames are identical; over the whole of the four files (D-STAR, 7776 frames) 77% of frames are
//! identical, b0 equal 99.95%, b1 equal 99.97%, envelope correlation above 0.999. Teacher-forced (the fixed quantizer
//! fed the float encoder's own decoder state each frame) only 18 of 6004 frames differ, all codebook near-ties, so the
//! whole-file differences are the closed loop amplifying those near-ties, not arithmetic error in a single frame.

mod common;

use common::{compare_streams, FrameView, Parity};
use ham_digital_modes::ambe::fixed::dstar::encoder::Encoder as FixedEncoder;
use ham_digital_modes::ambe::float::dstar::decode::{classify_b0, decode_tone, extract_raw_parameters, parse_frame, FrameKind};
use ham_digital_modes::ambe::float::dstar::encoder::Encoder as FloatEncoder;
use ham_digital_modes::ambe::float::dstar::synthesis::DStarSynthesisDecoder;

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
            let mut d = DStarSynthesisDecoder::new();
            Box::new(move |f| d.decode_frame(f))
        });
    }
    p.report(name);
    p
}

#[test]
fn speech_parity_with_the_float_encoder_first_150_frames() {
    let p = parity("D-STAR first 150 frames", 0, common::parity_frames());
    assert_eq!(p.count_mismatch, 0);
    assert!(p.frames >= 4 * (common::parity_frames().min(1000) - 2));
    assert_eq!(p.tone_frames_fixed, 0, "no speech frame may be emitted as a tone frame");
    assert_eq!(p.tone_frames_float, 0, "no speech frame may be emitted as a tone frame");
    assert!(p.frac(p.identical) >= 0.99, "identical frames {}", p.frac(p.identical));
    assert!(p.frac(p.b0_within_1) >= 0.999 && p.frac(p.b0_equal) >= 0.99);
    assert!(p.frac(p.b1_equal) >= 0.99);
    // One near-tie codebook decision out of ~600 frames differs (99.83% identical); that alone limits the decoded SNR to
    // about 69 dB, still far above audibility.
    assert!(p.snr_db() >= 60.0, "decoded SNR {} dB", p.snr_db());
    assert!(p.min_envelope_corr() >= 0.9999);
}

/// Mid-speech window (frames 600..750): the closed loop (each frame's prediction comes from the previous decoded frame)
/// amplifies the near-ties between the two implementations' codebook searches, so agreement here is lower than in the
/// quiet opening frames.
#[test]
fn speech_parity_with_the_float_encoder_mid_file_window() {
    let p = parity("D-STAR frames 600..750", 600, 150);
    assert_eq!(p.count_mismatch, 0);
    assert_eq!(p.tone_frames_fixed, 0);
    assert_eq!(p.tone_frames_float, 0);
    assert!(p.frac(p.identical) >= 0.95, "identical frames {}", p.frac(p.identical));
    assert!(p.frac(p.b0_within_1) >= 0.999 && p.frac(p.b0_equal) >= 0.99);
    assert!(p.frac(p.b1_equal) >= 0.99);
    assert!(p.snr_db() >= 25.0, "decoded SNR {} dB", p.snr_db());
    assert!(p.min_envelope_corr() >= 0.999);
}

fn sine(freqs: &[f64], amp: f64, frames: usize) -> Vec<i16> {
    (0..160 * frames)
        .map(|i| freqs.iter().map(|&hz| amp * (2.0 * std::f64::consts::PI * hz * i as f64 / 8000.0).sin()).sum::<f64>().round() as i16)
        .collect()
}

#[test]
fn tone_round_trip_dtmf_and_one_kilohertz() {
    // The float encoder's chip-measured cases: DTMF '5' at 4000 -> index 133, volume 186; 1 kHz at 12000 -> index 32,
    // volume 213.
    for (pcm, index, volume) in [(sine(&[770.0, 1336.0], 4000.0, 12), 133u32, 186u32), (sine(&[1000.0], 12000.0, 12), 32, 213)] {
        // Frames before the flush only: the padded tail frames are partly silence, not tone.
        let frames = encode_fixed(&pcm, false);
        assert!(frames.len() >= 8);
        assert_eq!(frames, encode_float(&pcm, false), "tone frames should be identical to the float encoder's");
        for &f in &frames {
            let d = parse_frame(f).d;
            assert_eq!(classify_b0(extract_raw_parameters(d).b0), FrameKind::Tone);
            let t = decode_tone(d);
            assert_eq!(t.index, index);
            assert!((t.volume as i32 - volume as i32).abs() <= 1, "volume {} vs {}", t.volume, volume);
        }
        let mut dec = DStarSynthesisDecoder::new();
        let last = frames.iter().map(|&f| dec.decode_frame(f).unwrap()).last().unwrap();
        assert!(last.iter().any(|&s| s.abs() > 100.0));
    }
}

/// The integer pitch quantizer agrees with the float one on every refined period the analyzer can produce
/// (`P = p8 / 8` samples, `21 <= P <= 122`, exact eighths).
#[test]
fn pitch_quantizer_matches_the_float_one_for_every_period() {
    use ham_digital_modes::ambe::fixed::dstar::encode::quantize_pitch_p8;
    use ham_digital_modes::ambe::float::dstar::quantize::quantize_pitch;
    let mut differ = 0;
    for p8 in 21 * 8..=122 * 8 {
        let w0 = 2.0 * std::f64::consts::PI / (p8 as f64 / 8.0);
        differ += usize::from(quantize_pitch_p8(p8) != quantize_pitch(w0));
    }
    assert_eq!(differ, 0);
}
