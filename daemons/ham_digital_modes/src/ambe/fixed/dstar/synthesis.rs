// SPDX-License-Identifier: LGPL-3.0-or-later
//! Fixed-point port of [`crate::ambe::float::dstar::synthesis`]: D-STAR bits-to-PCM decoding.
//! `parse_frame` (FEC + de-whitening, pure integer, reused from the float tree) -> fixed
//! `dequantize` -> the shared fixed [`MbeSynthesizer`]; tone frames (DTMF / single tone) go through
//! the fixed [`ToneSynthesizer`]. The mbelib bad-frame policy is identical to the float decoder: a
//! speech frame with more than 3 corrected errors repeats the previous parameters (3 times at
//! most) and then mutes and reinitializes. Output is `[i64; N]` Q16.16 PCM.

use crate::ambe::float::mbe_synthesis::{BadFrameAction, ErrorPolicy};
use super::decode::{
    classify_b0, classify_tone_index, dequantize, dtmf_digit_from_tone_index, extract_raw_parameters, DequantizedFrame,
    ToneKind,
};
use crate::ambe::fixed::general::mbe_speech::MbeDecoderState;
use crate::ambe::fixed::general::mbe_synthesis::MbeSynthesizer;
use crate::ambe::fixed::general::tone_synthesis::{dstar_tone_amplitude_q16, ToneSynthesizer};
use crate::ambe::fixed::general::unvoiced_synthesis::N;
use crate::ambe::float::dstar::decode::{parse_frame, FrameKind};

pub struct DStarSynthesisDecoder {
    dequant: MbeDecoderState,
    synth: MbeSynthesizer,
    tone: ToneSynthesizer,
    /// Consecutive frames repeated because of channel errors (mbelib's `repeat`).
    repeats: u32,
    error_policy: ErrorPolicy,
}

impl DStarSynthesisDecoder {
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

    /// Decodes one logical 72-bit frame (see the float `interleave::wire_bytes_to_frame`) into 20 ms
    /// of Q16.16 PCM.
    pub fn decode_frame(&mut self, logical_frame: u128) -> Option<[i64; N]> {
        let parsed = parse_frame(logical_frame);
        let b0 = extract_raw_parameters(parsed.d).b0;
        let is_speech = classify_b0(b0) == FrameKind::Speech;
        // The chip treats the reserved pitch codes 125 and 127 as invalid frames (repeat three times, then near-silence). Normal
        // operation stays lenient: a tone frame whose uncoded pitch bit flipped is better decoded as the tone.
        let chip_invalid = self.error_policy == ErrorPolicy::ChipCompatible && matches!(b0, 125 | 127);
        if (is_speech && self.error_policy.is_bad(parsed.epsilon_c0, parsed.epsilon_c1)) || chip_invalid {
            self.repeats += 1;
            let action = match self.error_policy.bad_frame_action(self.repeats) {
                BadFrameAction::Decode if chip_invalid => BadFrameAction::Mute,
                other => other,
            };
            match action {
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
        match dequantize(parsed.d, &mut self.dequant) {
            DequantizedFrame::Speech(p) => {
                self.tone.reset();
                self.synth.synthesize_speech(p.w0_q32, &p.voiced, &p.ml_q16, parsed.epsilon_c0, parsed.epsilon_c1)
            }
            DequantizedFrame::Tone(t) => {
                let amplitude_q16 = dstar_tone_amplitude_q16(t.volume);
                Some(match classify_tone_index(t.index) {
                    ToneKind::Single { hz_q16 } => self.tone.synthesize(&[hz_q16], amplitude_q16),
                    ToneKind::Dual => match dtmf_digit_from_tone_index(t.index) {
                        Some((row, col)) => self.tone.dtmf(row, col, amplitude_q16),
                        // Dual-tone codes 144..=163: meaning unidentified, so emit silence.
                        None => [0; N],
                    },
                    ToneKind::Invalid => [0; N],
                })
            }
        }
    }
}

impl Default for DStarSynthesisDecoder {
    fn default() -> Self {
        Self::new()
    }
}
