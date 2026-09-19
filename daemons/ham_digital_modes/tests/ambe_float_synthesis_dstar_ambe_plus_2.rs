// SPDX-License-Identifier: LGPL-3.0-or-later
//! End-to-end float bits-to-PCM tests for D-STAR and AMBE+2 half-rate: speech frames synthesize
//! finite, audible, bounded PCM across many frames; tone frames (single tone, DTMF, call progress)
//! synthesize sinusoids at the right frequencies; erasure repeats the last frame and silence is zero.

use ham_digital_modes::ambe::float::dstar::decode::{decode_tone, RawParameters as DRaw};
use ham_digital_modes::ambe::float::dstar::encode::{build_frame as dstar_build_frame, pack_raw_parameters};
use ham_digital_modes::ambe::float::dstar::synthesis::DStarSynthesisDecoder;
use std::f64::consts::PI;

fn tone_power(frame: &[f64], hz: f64) -> f64 {
    let (mut re, mut im) = (0.0, 0.0);
    for (n, &s) in frame.iter().enumerate() {
        let a = 2.0 * PI * hz * n as f64 / 8000.0;
        re += s * a.cos();
        im += s * a.sin();
    }
    re * re + im * im
}

fn set_bits(d: &mut u64, msb_index: usize, width: usize, value: u64) {
    let shift = 49 - msb_index - width;
    let mask = ((1u64 << width) - 1) << shift;
    *d = (*d & !mask) | ((value << shift) & mask);
}

#[test]
fn dstar_speech_frames_synthesize_finite_audible_bounded_pcm() {
    let mut dec = DStarSynthesisDecoder::new();
    let mut loudest = 0.0_f64;
    for i in 0..30u32 {
        let raw = DRaw { b0: 40 + (i % 3), b1: 5, b2: 20, b3: 100 + i, b4: 30, b5: 8, b6: 6, b7: 5, b8: 3 };
        let pcm = dec.decode_frame(dstar_build_frame(pack_raw_parameters(&raw))).expect("speech frame");
        for &s in &pcm {
            assert!(s.is_finite() && s.abs() < 1e6, "bad sample {s}");
            loudest = loudest.max(s.abs());
        }
    }
    assert!(loudest > 1.0, "synthesis was silent (peak {loudest})");
}

#[test]
fn dstar_single_tone_frame_synthesizes_its_frequency() {
    let mut base = pack_raw_parameters(&DRaw { b0: 126, b1: 0, b2: 0, b3: 0, b4: 0, b5: 0, b6: 0, b7: 0, b8: 0 });
    let target = 32u32; // single tone: 32 * 31.25 = 1000 Hz
    let positions = [(6usize, 1), (7, 1), (8, 1), (9, 1), (10, 1), (11, 1), (42, 1), (43, 1)];
    let mut found = false;
    for pattern in 0..256u64 {
        let mut d = base;
        for (k, &(pos, w)) in positions.iter().enumerate() {
            set_bits(&mut d, pos, w, (pattern >> k) & 1);
        }
        if decode_tone(d).index == target {
            base = d;
            found = true;
            break;
        }
    }
    assert!(found, "no bit pattern encodes tone index {target}");
    let mut dec = DStarSynthesisDecoder::new();
    let pcm = dec.decode_frame(dstar_build_frame(base)).expect("tone frame");
    assert!(tone_power(&pcm, 1000.0) > 50.0 * tone_power(&pcm, 1500.0));
}

#[cfg(feature = "ambe_plus_2")]
mod ambe_plus_2 {
    use super::*;
    use ham_digital_modes::ambe::float::ambe_plus_2::decode::{decode_tone_idx, RawParameters};
    use ham_digital_modes::ambe::float::ambe_plus_2::encode::{build_frame, pack_raw_parameters};
    use ham_digital_modes::ambe::float::ambe_plus_2::synthesis::AmbePlus2SynthesisDecoder;

    fn raw(b0: u32) -> RawParameters {
        RawParameters { b0, b1: 3, b2: 10, b3: 60, b4: 20, b5: 5, b6: 4, b7: 3, b8: 2 }
    }

    /// A tone frame (`b0 = b0_code`, 120 detected-tone or 122 call-progress) carrying `tone_idx`.
    fn tone_frame(b0_code: u32, tone_idx: u8) -> u128 {
        let mut d = pack_raw_parameters(&raw(b0_code));
        set_bits(&mut d, 0, 4, (b0_code >> 3) as u64);
        set_bits(&mut d, 37, 3, (b0_code & 7) as u64);
        for &pos in &[16usize, 24, 32, 40] {
            set_bits(&mut d, pos, 4, (tone_idx & 0xF) as u64);
        }
        for &pos in &[20usize, 28] {
            set_bits(&mut d, pos, 4, (tone_idx >> 4) as u64);
        }
        assert_eq!(decode_tone_idx(d), Some(tone_idx));
        ham_digital_modes::ambe::float::ambe_plus_2::build_frame(d)
    }

    #[test]
    fn speech_frames_synthesize_finite_audible_bounded_pcm() {
        let mut dec = AmbePlus2SynthesisDecoder::new();
        let mut loudest = 0.0_f64;
        for i in 0..30u32 {
            let pcm = dec.decode_frame(build_frame(&raw(40 + (i % 3)))).expect("speech frame");
            for &s in &pcm {
                assert!(s.is_finite() && s.abs() < 1e6);
                loudest = loudest.max(s.abs());
            }
        }
        assert!(loudest > 1.0);
    }

    #[test]
    fn erasure_repeats_and_silence_is_zero() {
        let mut dec = AmbePlus2SynthesisDecoder::new();
        assert!(dec.decode_frame(build_frame(&raw(121))).is_none(), "erasure before any real frame");
        let speech = dec.decode_frame(build_frame(&raw(40))).unwrap();
        assert!(speech.iter().any(|&s| s != 0.0));
        let repeated = dec.decode_frame(build_frame(&raw(121))).expect("erasure repeats");
        assert!(repeated.iter().all(|s| s.is_finite()));
        // b0 = 124/125 are silence frames.
        let silence = dec.decode_frame(build_frame(&raw(124))).expect("silence");
        assert!(silence.iter().all(|&s| s == 0.0));
    }

    #[test]
    fn dtmf_frame_synthesizes_both_tones() {
        let mut dec = AmbePlus2SynthesisDecoder::new();
        let pcm = dec.decode_frame(tone_frame(120, 0x85)).expect("tone frame"); // digit 5: 770 + 1336
        assert!(tone_power(&pcm, 770.0) > 50.0 * tone_power(&pcm, 1000.0));
        assert!(tone_power(&pcm, 1336.0) > 50.0 * tone_power(&pcm, 1000.0));
    }

    #[test]
    fn single_tone_and_call_progress_frames_synthesize_their_frequencies() {
        let mut dec = AmbePlus2SynthesisDecoder::new();
        let pcm = dec.decode_frame(tone_frame(120, 32)).expect("single tone"); // 32 * 31.25 = 1000 Hz
        assert!(tone_power(&pcm, 1000.0) > 50.0 * tone_power(&pcm, 1500.0));
        let dial = dec.decode_frame(tone_frame(122, 0xA0)).expect("dial tone"); // 350 + 440
        assert!(tone_power(&dial, 350.0) > 20.0 * tone_power(&dial, 1000.0));
        assert!(tone_power(&dial, 440.0) > 20.0 * tone_power(&dial, 1000.0));
        let busy = dec.decode_frame(tone_frame(122, 0xA2)).expect("busy tone"); // 480 + 620
        assert!(tone_power(&busy, 480.0) > 20.0 * tone_power(&busy, 1000.0));
        assert!(tone_power(&busy, 620.0) > 20.0 * tone_power(&busy, 1000.0));
    }
}
