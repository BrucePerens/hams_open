// SPDX-License-Identifier: LGPL-3.0-or-later
//! Streaming fixed-point PCM-to-frame encoder for AMBE+2 half-rate, the sibling of
//! [`crate::ambe::fixed::dstar::encoder`] and of [`crate::ambe::float::ambe_plus_2::encoder::Encoder`] (same public
//! surface, same u128 logical frames; input is 16-bit PCM). Pitch is the nearest entry of the 120-entry `W0_TABLE`
//! ([`super::encode::quantize_pitch_p8`]); tone frames carry the 12-bit level field of
//! [`super::encode::amplitude_field_q16`].

use super::decode::dequantize;
use crate::ambe::fixed::general::mbe_speech::MbeDecoderState;
use super::encode::{amplitude_field_q16, build_frame, build_tone_frame, dtmf_tone_idx, mode_tables, quantize_pitch_p8, RawParameters};
use super::tables_q16::{W0_TABLE_Q16_16, W0_TABLE_Q32};
use crate::ambe::fixed::general::tone_detect::{detect_tone, DetectedTone};
use crate::ambe::fixed::mbe_encode::{analyze_and_quantize, AnalysisState};
use crate::ambe::fixed::tia_102_baba::encoder::FrameAnalyzer;
use crate::ambe::float::ambe_plus_2::tables::L_TABLE;

pub struct Encoder {
    analyzer: FrameAnalyzer,
    mirror: MbeDecoderState,
    analysis: AnalysisState,
}

impl Encoder {
    pub fn new() -> Self {
        Self { analyzer: FrameAnalyzer::new(), mirror: MbeDecoderState::initial(), analysis: AnalysisState::new() }
    }

    pub fn set_center_offset(&mut self, samples: i32) {
        self.analyzer.set_center_offset(samples);
    }

    pub fn push_samples(&mut self, samples: &[i16]) {
        self.analyzer.push_samples(samples);
    }

    /// The next 72-bit logical frame if enough lookahead has been pushed.
    pub fn next_frame(&mut self) -> Option<u128> {
        let a = self.analyzer.next_analysis()?;
        let slot: Vec<i16> = a.slot_samples.iter().map(|&s| s as i16).collect(); // lossless: pushed as i16
        if let Some(det) = detect_tone(&slot) {
            let tone_idx = match det.tone {
                DetectedTone::Dtmf { row, col } => dtmf_tone_idx(row, col),
                DetectedTone::Single { index, .. } => index as u8,
            };
            return Some(build_tone_frame(tone_idx, false, amplitude_field_q16(det.amplitude_q16)));
        }
        let b0 = quantize_pitch_p8(a.p8);
        let l = L_TABLE[b0 as usize];
        let q = analyze_and_quantize(
            &a,
            l,
            W0_TABLE_Q16_16[b0 as usize],
            W0_TABLE_Q32[b0 as usize],
            &mode_tables(),
            &self.mirror,
            &mut self.analysis,
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
