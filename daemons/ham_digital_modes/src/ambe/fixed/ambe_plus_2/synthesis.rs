// SPDX-License-Identifier: LGPL-3.0-or-later
//! Fixed-point port of [`crate::ambe::float::ambe_plus_2::synthesis`]: AMBE+2 half-rate
//! bits-to-PCM decoding. `parse_frame` -> `extract_raw_parameters` -> fixed `dequantize` -> the
//! shared fixed [`MbeSynthesizer`]. Erasures repeat the previous frame, silence frames produce
//! zeros, tone frames (DTMF / single tone / call progress) go through the fixed
//! [`ToneSynthesizer`], and the mbelib bad-frame policy matches the float decoder. Output is
//! `[i64; N]` Q16.16 PCM.

use crate::ambe::float::mbe_synthesis::{BadFrameAction, ErrorPolicy};
use super::decode::{
    classify_b0, classify_tone_idx, decode_tone_idx, dequantize, extract_raw_parameters, CallProgressTone,
    DequantizedFrame, ToneIdentity,
};
use crate::ambe::fixed::general::mbe_speech::MbeDecoderState;
use crate::ambe::fixed::general::mbe_synthesis::MbeSynthesizer;
use crate::ambe::fixed::general::tone_synthesis::{
    ambe_plus_2_dual_tone_peak_q16, ambe_plus_2_single_tone_peak_q16, ToneSynthesizer, CALL_BUSY_HZ, CALL_DIAL_HZ,
    CALL_RING_HZ,
};
use crate::ambe::fixed::general::unvoiced_synthesis::N;
use crate::ambe::float::ambe_plus_2::decode::FrameKind;
use crate::ambe::float::ambe_plus_2::parse_frame;

/// `round(31.25 * 65536)`, exact: a single tone's frequency is `tone_idx * 31.25` Hz.
const HZ_PER_INDEX_Q16_16: i32 = 2_048_000;

pub struct AmbePlus2SynthesisDecoder {
    dequant: MbeDecoderState,
    synth: MbeSynthesizer,
    tone: ToneSynthesizer,
    /// Consecutive frames repeated because of channel errors (mbelib's `repeat`).
    repeats: u32,
    error_policy: ErrorPolicy,
}

impl AmbePlus2SynthesisDecoder {
    pub fn new() -> Self {
        Self {
            dequant: MbeDecoderState::initial(),
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

    /// Decodes one logical 72-bit frame (see the float `interleave::interleaved_to_frame`) into
    /// 20 ms of Q16.16 PCM.
    pub fn decode_frame(&mut self, logical_frame: u128) -> Option<[i64; N]> {
        let parsed = parse_frame(logical_frame);
        let raw = extract_raw_parameters(parsed.d);
        if classify_b0(raw.b0) == FrameKind::Speech && self.error_policy.is_bad(parsed.epsilon_c0, parsed.epsilon_c1) {
            self.repeats += 1;
            match self.error_policy.bad_frame_action(self.repeats) {
                BadFrameAction::Repeat => return self.synth.synthesize_repeat(),
                BadFrameAction::Mute => {
                    self.dequant = MbeDecoderState::initial();
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
                self.synth.synthesize_speech(p.w0_q32, &p.voiced, &p.ml_q16, parsed.epsilon_c0, parsed.epsilon_c1)
            }
            DequantizedFrame::Erasure => self.synth.synthesize_repeat(),
            DequantizedFrame::Silence { .. } => Some(self.synth.synthesize_silence()),
            DequantizedFrame::Tone { .. } => {
                let tone_idx = decode_tone_idx(parsed.d);
                let dual = ambe_plus_2_dual_tone_peak_q16();
                Some(match tone_idx.map(|idx| (idx, classify_tone_idx(idx))) {
                    Some((idx, ToneIdentity::SingleTone { .. })) => {
                        let hz_q16 = idx as i32 * HZ_PER_INDEX_Q16_16;
                        self.tone.synthesize(&[hz_q16], ambe_plus_2_single_tone_peak_q16())
                    }
                    Some((_, ToneIdentity::Dtmf { row, col })) => self.tone.dtmf(row, col, dual),
                    Some((_, ToneIdentity::CallProgress(kind))) => match kind {
                        CallProgressTone::Dial => self.tone.synthesize_hz(&CALL_DIAL_HZ, dual),
                        CallProgressTone::Ring => self.tone.synthesize_hz(&CALL_RING_HZ, dual),
                        CallProgressTone::Busy => self.tone.synthesize_hz(&CALL_BUSY_HZ, dual),
                        CallProgressTone::Inactive => [0; N],
                    },
                    Some((_, ToneIdentity::Reserved(_))) | None => [0; N],
                })
            }
        }
    }
}

impl Default for AmbePlus2SynthesisDecoder {
    fn default() -> Self {
        Self::new()
    }
}
