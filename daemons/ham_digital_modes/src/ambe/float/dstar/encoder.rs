// SPDX-License-Identifier: LGPL-3.0-or-later
//! Streaming PCM-to-frame encoder for D-STAR's AMBE: shared pitch analysis
//! ([`crate::ambe::float::ratet27::encoder::FrameAnalyzer`]), voicing/amplitude analysis at the quantized pitch
//! ([`crate::ambe::float::mbe_encode::analyze_at_pitch`]), then the inverse of this mode's dequantization chain
//! ([`crate::ambe::float::mbe_encode::quantize_speech`]). A mirror of the decoder's own state is advanced by
//! actually dequantizing each emitted frame, so the encoder's predictions always match what a decoder will hold.
//! Emits logical 72-bit frames (see [`super::interleave::frame_to_wire_bytes`] for the chip's wire layout).

use super::decode::{dequantize, DStarDecoderState, RawParameters};
use super::encode::{build_frame, pack_raw_parameters};
use super::quantize::quantize_pitch;
use super::tables;
use crate::ambe::float::mbe_encode::{analyze_at_pitch, quantize_speech, AnalysisState, ModeTables, PrevState, SpeechTarget};
use crate::ambe::float::ratet27::encoder::FrameAnalyzer;

pub struct Encoder {
    analyzer: FrameAnalyzer,
    mirror: DStarDecoderState,
    analysis: AnalysisState,
}

impl Encoder {
    pub fn new() -> Self {
        Self { analyzer: FrameAnalyzer::new(), mirror: DStarDecoderState::initial(), analysis: AnalysisState::new() }
    }

    pub fn set_center_offset(&mut self, samples: i32) {
        self.analyzer.set_center_offset(samples);
    }

    pub fn push_samples(&mut self, samples: &[f64]) {
        self.analyzer.push_samples(samples);
    }

    /// The next 72-bit logical frame if enough lookahead has been pushed.
    pub fn next_frame(&mut self) -> Option<u128> {
        let a = self.analyzer.next_analysis()?;
        let b0 = quantize_pitch(a.omega0_hat);
        let l = tables::L_TABLE[b0 as usize];
        let f0 = super::decode::f0_from_b0(b0);
        let w0 = f0 * 2.0 * std::f64::consts::PI;
        let (voiced, ml) = analyze_at_pitch(&a, w0, l, &mut self.analysis);

        let mode = ModeTables {
            vuv: &tables::VUV,
            dg: &tables::DG,
            prba24: &tables::PRBA24,
            prba58: &tables::PRBA58,
            lmprbl: &tables::LMPRBL,
            hoc: [&tables::HOC_B5, &tables::HOC_B6, &tables::HOC_B7, &tables::HOC_B8],
            hoc_b8_even_only: true,
        };
        let q = quantize_speech(
            &SpeechTarget { l, w0, vuv_f0: f0 / super::decode::F0_CHIP_SCALE, voiced: &voiced, ml: &ml },
            &PrevState { l: self.mirror.l, log2_ml: &self.mirror.log2_ml, gamma: self.mirror.gamma },
            &mode,
        );
        let raw = RawParameters { b0, b1: q.b1, b2: q.b2, b3: q.b3, b4: q.b4, b5: q.b5, b6: q.b6, b7: q.b7, b8: q.b8 };
        let d = pack_raw_parameters(&raw);
        dequantize(d, &mut self.mirror);
        Some(build_frame(d))
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
    use crate::ambe::float::dstar::decode::{parse_frame, DequantizedFrame};

    /// Encoding a harmonic signal, then dequantizing every emitted frame with the real decoder, reproduces the
    /// signal's pitch and a spectral envelope whose level tracks the signal's (VQ quantization limits the shape).
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

        let mut state = DStarDecoderState::initial();
        let mut checked = 0;
        for (i, &frame) in frames.iter().enumerate() {
            let parsed = parse_frame(frame);
            assert_eq!(parsed.epsilon_c0 + parsed.epsilon_c1, 0, "a freshly built frame must be error free");
            if let DequantizedFrame::Speech(p) = dequantize(parsed.d, &mut state) {
                if i >= 6 && i < 24 {
                    let p_est = 2.0 * std::f64::consts::PI / p.w0;
                    assert!((p_est / period - 1.0).abs() < 0.05, "frame {i}: decoded period {p_est}");
                    // The strongest harmonics (1-8) should carry real energy.
                    let peak = p.ml[1..=8.min(p.l as usize)].iter().cloned().fold(0.0, f64::max);
                    assert!(peak > 100.0, "frame {i}: peak harmonic amplitude {peak} implausibly small");
                    checked += 1;
                }
            }
        }
        assert!(checked >= 15);
    }
}
