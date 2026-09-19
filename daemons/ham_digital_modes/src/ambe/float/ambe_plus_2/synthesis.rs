// SPDX-License-Identifier: LGPL-3.0-or-later
//! AMBE+2 half-rate bits-to-PCM decoding: `parse_frame` -> `extract_raw_parameters` -> `dequantize`
//! -> the shared [`crate::ambe::float::mbe_synthesis::MbeSynthesizer`]. Erasures repeat the previous
//! frame, silence frames produce zeros, and tone frames (DTMF / single tone / call progress) are
//! decoded via `decode_tone_idx`/`classify_tone_idx` and synthesized as sinusoids via
//! [`ToneSynthesizer`].

use super::decode::{
    classify_tone_idx, decode_tone_idx, dequantize, extract_raw_parameters, CallProgressTone,
    DecoderState, DequantizedFrame, ToneIdentity,
};
use super::parse_frame;
use crate::ambe::float::mbe_synthesis::MbeSynthesizer;
use crate::ambe::float::ratet27::unvoiced_synthesis::N;
use crate::ambe::float::tone_synthesis::{
    ToneSynthesizer, CALL_BUSY_HZ, CALL_DIAL_HZ, CALL_RING_HZ, DEFAULT_TONE_PEAK,
};

pub struct AmbePlus2SynthesisDecoder {
    dequant: DecoderState,
    synth: MbeSynthesizer,
    tone: ToneSynthesizer,
}

impl AmbePlus2SynthesisDecoder {
    pub fn new() -> Self {
        Self {
            dequant: DecoderState::initial(),
            synth: MbeSynthesizer::new(),
            tone: ToneSynthesizer::new(),
        }
    }

    /// Decodes one logical 72-bit frame (see `interleave::interleaved_to_frame`) into 20 ms of PCM.
    pub fn decode_frame(&mut self, logical_frame: u128) -> Option<[f64; N]> {
        let parsed = parse_frame(logical_frame);
        let raw = extract_raw_parameters(parsed.d);
        match dequantize(&raw, &mut self.dequant) {
            DequantizedFrame::Speech(p) => {
                self.tone.reset();
                self.synth
                    .synthesize_speech(p.w0, &p.voiced, &p.ml, parsed.epsilon_c0, parsed.epsilon_c1)
            }
            DequantizedFrame::Erasure => self.synth.synthesize_repeat(),
            DequantizedFrame::Silence { .. } => Some(self.synth.synthesize_silence()),
            DequantizedFrame::Tone { .. } => Some(match decode_tone_idx(parsed.d).map(classify_tone_idx) {
                Some(ToneIdentity::SingleTone { hz }) => self.tone.synthesize(&[hz], DEFAULT_TONE_PEAK),
                Some(ToneIdentity::Dtmf { row, col }) => self.tone.dtmf(row, col, DEFAULT_TONE_PEAK),
                Some(ToneIdentity::CallProgress(kind)) => match kind {
                    CallProgressTone::Dial => self.tone.synthesize(&CALL_DIAL_HZ, DEFAULT_TONE_PEAK),
                    CallProgressTone::Ring => self.tone.synthesize(&CALL_RING_HZ, DEFAULT_TONE_PEAK),
                    CallProgressTone::Busy => self.tone.synthesize(&CALL_BUSY_HZ, DEFAULT_TONE_PEAK),
                    CallProgressTone::Inactive => [0.0; N],
                },
                Some(ToneIdentity::Reserved(_)) | None => [0.0; N],
            }),
        }
    }
}

impl Default for AmbePlus2SynthesisDecoder {
    fn default() -> Self {
        Self::new()
    }
}
