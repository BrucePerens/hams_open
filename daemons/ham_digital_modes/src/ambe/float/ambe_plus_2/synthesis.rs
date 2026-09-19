// SPDX-License-Identifier: LGPL-3.0-or-later
//! AMBE+2 half-rate bits-to-PCM decoding: `parse_frame` -> `extract_raw_parameters` -> `dequantize`
//! -> the shared [`crate::ambe::float::mbe_synthesis::MbeSynthesizer`]. Erasures repeat the previous
//! frame, silence frames produce zeros, and tone frames (DTMF / single tone / call progress) are
//! decoded via `decode_tone_idx`/`classify_tone_idx` and synthesized as sinusoids via
//! [`ToneSynthesizer`].

use crate::ambe::float::mbe_synthesis::{BadFrameAction, ErrorPolicy};
use super::decode::{
    classify_b0, classify_tone_idx, decode_tone_idx, dequantize, extract_raw_parameters, CallProgressTone,
    DecoderState, DequantizedFrame, FrameKind, ToneIdentity,
};
use super::parse_frame;
use crate::ambe::float::mbe_synthesis::MbeSynthesizer;
use crate::ambe::float::tia_102_baba::unvoiced_synthesis::N;
use crate::ambe::float::tone_synthesis::{
    ToneSynthesizer, AMBE_PLUS_2_TONE_RMS, CALL_BUSY_HZ, CALL_DIAL_HZ, CALL_RING_HZ,
};

pub struct AmbePlus2SynthesisDecoder {
    dequant: DecoderState,
    synth: MbeSynthesizer,
    tone: ToneSynthesizer,
    /// Consecutive frames repeated because of channel errors (mbelib's `repeat`).
    repeats: u32,
    error_policy: ErrorPolicy,
}

impl AmbePlus2SynthesisDecoder {
    pub fn new() -> Self {
        Self {
            dequant: DecoderState::initial(),
            synth: MbeSynthesizer::new(),
            tone: ToneSynthesizer::new(),
            repeats: 0,
            error_policy: ErrorPolicy::Clean,
        }
    }

    /// Selects how channel errors are handled (see [`ErrorPolicy`]); the default is the clean policy.
    pub fn with_error_policy(mut self, policy: ErrorPolicy) -> Self {
        self.error_policy = policy;
        self
    }

    /// Decodes one logical 72-bit frame (see `interleave::interleaved_to_frame`) into 20 ms of PCM.
    pub fn decode_frame(&mut self, logical_frame: u128) -> Option<[f64; N]> {
        let parsed = parse_frame(logical_frame);
        let raw = extract_raw_parameters(parsed.d);
        // mbelib's bad-frame policy (`mbe_processAmbe2450Dataf`): a speech frame with more than 3 corrected errors
        // reuses the previous frame's parameters without touching the predictor state, and after 3 such repeats in a
        // row the decoder mutes (silence) and reinitializes.
        if classify_b0(raw.b0) == FrameKind::Speech && self.error_policy.is_bad(parsed.epsilon_c0, parsed.epsilon_c1) {
            self.repeats += 1;
            match self.error_policy.bad_frame_action(self.repeats) {
                BadFrameAction::Repeat => return self.synth.synthesize_repeat(),
                BadFrameAction::Mute => {
                    self.dequant = DecoderState::initial();
                    self.synth = MbeSynthesizer::new();
                    return Some(self.synth.synthesize_silence());
                }
                BadFrameAction::Decode => {}
            }
        } else {
            self.repeats = 0;
        }
        match dequantize(&raw, &mut self.dequant) {
            DequantizedFrame::Speech(p) => {
                self.tone.reset();
                self.synth
                    .synthesize_speech(p.w0, &p.voiced, &p.ml, parsed.epsilon_c0, parsed.epsilon_c1)
            }
            DequantizedFrame::Erasure => self.synth.synthesize_repeat(),
            DequantizedFrame::Silence { .. } => Some(self.synth.synthesize_silence()),
            DequantizedFrame::Tone { .. } => Some(match decode_tone_idx(parsed.d).map(classify_tone_idx) {
                Some(ToneIdentity::SingleTone { hz }) => self.tone.synthesize(&[hz], AMBE_PLUS_2_TONE_RMS * std::f64::consts::SQRT_2),
                Some(ToneIdentity::Dtmf { row, col }) => self.tone.dtmf(row, col, AMBE_PLUS_2_TONE_RMS),
                Some(ToneIdentity::CallProgress(kind)) => match kind {
                    CallProgressTone::Dial => self.tone.synthesize(&CALL_DIAL_HZ, AMBE_PLUS_2_TONE_RMS),
                    CallProgressTone::Ring => self.tone.synthesize(&CALL_RING_HZ, AMBE_PLUS_2_TONE_RMS),
                    CallProgressTone::Busy => self.tone.synthesize(&CALL_BUSY_HZ, AMBE_PLUS_2_TONE_RMS),
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
