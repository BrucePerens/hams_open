// SPDX-License-Identifier: LGPL-3.0-or-later
//! Streaming PCM-to-frame encoder for AMBE+2 half-rate, the sibling of [`crate::ambe::float::dstar::encoder`]:
//! shared pitch analysis, analysis at the quantized pitch, then the inverse of this mode's dequantization chain,
//! with a mirror of the decoder state advanced by dequantizing each emitted frame. Emits logical 72-bit frames
//! (convert with [`super::interleave::frame_to_interleaved`] for the chip's wire layout).

use super::decode::{dequantize, DecoderState, RawParameters};
use super::encode::{build_frame, build_tone_frame};
use super::quantize::quantize_pitch;
use super::tables;
use crate::ambe::float::mbe_encode::{analyze_at_pitch, quantize_speech, AnalysisState, ModeTables, PrevState, SpeechTarget};
use crate::ambe::float::tia_102_baba::encoder::FrameAnalyzer;
use crate::ambe::float::tone_detect::{detect_tone, DetectedTone};

/// The 12-bit level field a chip encoder writes for a tone of per-tone amplitude `amplitude` (measured 1 kHz sine
/// captures: amplitude 250/500/1000/2000/4000/8000/16000 -> 0x715/0x725/0xea2/0xed2/0xf12/0xf62/0xfa2). The chip's own
/// decoder ignores this field, so the mapping is by interpolation in log2(amplitude) over those points.
fn amplitude_field(amplitude: f64) -> u16 {
    const POINTS: [(f64, f64); 7] = [(250.0, 0x715 as f64), (500.0, 0x725 as f64), (1000.0, 0xea2 as f64), (2000.0, 0xed2 as f64), (4000.0, 0xf12 as f64), (8000.0, 0xf62 as f64), (16000.0, 0xfa2 as f64)];
    let x = amplitude.max(1.0).log2();
    let (mut lo, mut hi) = (POINTS[0], POINTS[POINTS.len() - 1]);
    for w in POINTS.windows(2) {
        if x >= w[0].0.log2() && x <= w[1].0.log2() {
            (lo, hi) = (w[0], w[1]);
            break;
        }
    }
    let t = ((x - lo.0.log2()) / (hi.0.log2() - lo.0.log2())).clamp(0.0, 1.0);
    (lo.1 + t * (hi.1 - lo.1)).round() as u16
}

pub struct Encoder {
    analyzer: FrameAnalyzer,
    mirror: DecoderState,
    analysis: AnalysisState,
}

impl Encoder {
    pub fn new() -> Self {
        Self { analyzer: FrameAnalyzer::new(), mirror: DecoderState::initial(), analysis: AnalysisState::new() }
    }

    // The chip encoder parks entirely unvoiced frames on b0 92-93 (119 for the quietest noise), but copying that lowers the
    // envelope correlation of our stream through the chip's decoder (0.973 -> 0.955), so it is deliberately not copied.

    pub fn set_center_offset(&mut self, samples: i32) {
        self.analyzer.set_center_offset(samples);
    }

    pub fn push_samples(&mut self, samples: &[f64]) {
        self.analyzer.push_samples(samples);
    }

    /// The next 72-bit logical frame if enough lookahead has been pushed.
    pub fn next_frame(&mut self) -> Option<u128> {
        let a = self.analyzer.next_analysis()?;
        if let Some(det) = detect_tone(&a.slot_samples) {
            let tone_idx = match det.tone {
                DetectedTone::Dtmf { row, col } => super::decode::dtmf_tone_idx(row, col),
                DetectedTone::Single { index, .. } => index as u8,
            };
            return Some(build_tone_frame(tone_idx, false, amplitude_field(det.amplitude)));
        }
        let b0 = quantize_pitch(a.omega0_hat);
        let l = tables::L_TABLE[b0 as usize];
        let f0 = tables::W0_TABLE[b0 as usize];
        let w0 = f0 * 2.0 * std::f64::consts::PI;
        let (voiced, ml) = analyze_at_pitch(&a, w0, l, &mut self.analysis);

        let mode = ModeTables {
            vuv: &tables::VUV,
            dg: &tables::DG,
            prba24: &tables::PRBA24,
            prba58: &tables::PRBA58,
            lmprbl: &tables::LMPRBL,
            hoc: [&tables::HOC_B5, &tables::HOC_B6, &tables::HOC_B7, &tables::HOC_B8],
            hoc_b8_even_only: false,
            rho: 0.65,
        };
        let q = quantize_speech(
            &SpeechTarget { l, w0, vuv_f0: f0, voiced: &voiced, ml: &ml },
            &PrevState { l: self.mirror.l, log2_ml: &self.mirror.log2_ml, gamma: self.mirror.gamma },
            &mode,
        );
        let raw = RawParameters { b0, b1: q.b1, b2: q.b2, b3: q.b3, b4: q.b4, b5: q.b5, b6: q.b6, b7: q.b7, b8: q.b8 };
        dequantize(&raw, &mut self.mirror);
        Some(build_frame(&raw))
    }

    /// Pads with silence so every pushed sample's frame is emitted, returning the remaining frames.
    pub fn finish(&mut self) -> Vec<u128> {
        self.analyzer.finish_input();
        let mut out = Vec::new();
        while let Some(f) = self.next_frame() {
            out.push(f);
        }
        out
    }
}

impl Default for Encoder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ambe::float::ambe_plus_2::decode::{extract_raw_parameters, DequantizedFrame};
    use crate::ambe::float::ambe_plus_2::parse_frame;

    #[test]
    fn encoder_round_trip_recovers_pitch_and_level() {
        let period = 50.0;
        let signal: Vec<f64> = (0..160 * 30)
            .map(|i| {
                (1..=8)
                    .map(|h| 1500.0 / h as f64 * (2.0 * std::f64::consts::PI * h as f64 * i as f64 / period).sin())
                    .sum()
            })
            .collect();
        let mut enc = Encoder::new();
        enc.push_samples(&signal);
        let mut frames = Vec::new();
        while let Some(f) = enc.next_frame() {
            frames.push(f);
        }
        frames.extend(enc.finish());
        assert!(frames.len() >= 25);

        let mut state = DecoderState::initial();
        let mut checked = 0;
        for (i, &frame) in frames.iter().enumerate() {
            let parsed = parse_frame(frame);
            assert_eq!(parsed.epsilon_c0 + parsed.epsilon_c1, 0);
            let raw = extract_raw_parameters(parsed.d);
            if let DequantizedFrame::Speech(p) = dequantize(&raw, &mut state) {
                if (6..24).contains(&i) {
                    let p_est = 2.0 * std::f64::consts::PI / p.w0;
                    assert!((p_est / period - 1.0).abs() < 0.05, "frame {i}: decoded period {p_est}");
                    let peak = p.ml[1..=8.min(p.l as usize)].iter().cloned().fold(0.0, f64::max);
                    assert!(peak > 100.0, "frame {i}: peak harmonic amplitude {peak} implausibly small");
                    checked += 1;
                }
            }
        }
        assert!(checked >= 15);
    }

    #[test]
    fn dtmf_and_single_tones_are_emitted_as_tone_frames() {
        use crate::ambe::float::ambe_plus_2::decode::{classify_tone_idx, decode_tone_idx, dtmf_digit_from_tone_idx, ToneIdentity};
        let sine = |freqs: &[f64], amp: f64| -> Vec<f64> {
            (0..160 * 12)
                .map(|i| freqs.iter().map(|&hz| amp * (2.0 * std::f64::consts::PI * hz * i as f64 / 8000.0).sin()).sum())
                .collect()
        };
        let mut enc = Encoder::new();
        enc.push_samples(&sine(&[770.0, 1336.0], 4000.0)); // DTMF 5
        while let Some(f) = enc.next_frame() {
            let idx = decode_tone_idx(parse_frame(f).d).expect("tone frame");
            assert_eq!(dtmf_digit_from_tone_idx(idx), Some((1, 1)));
        }
        let mut enc = Encoder::new();
        enc.push_samples(&sine(&[1000.0], 4000.0));
        while let Some(f) = enc.next_frame() {
            let idx = decode_tone_idx(parse_frame(f).d).expect("tone frame");
            assert!(matches!(classify_tone_idx(idx), ToneIdentity::SingleTone { hz } if (hz - 1000.0).abs() < 16.0));
        }
    }
}
