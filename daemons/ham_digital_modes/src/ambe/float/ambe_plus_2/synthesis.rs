// SPDX-License-Identifier: LGPL-3.0-or-later
//! AMBE+2 half-rate bits-to-PCM decoding: `parse_frame` -> `extract_raw_parameters` -> `dequantize`
//! -> the shared [`crate::ambe::float::mbe_synthesis::MbeSynthesizer`]. Erasures repeat the previous
//! frame, silence frames produce zeros, and tone frames (DTMF / single tone / call progress) are
//! decoded via `decode_tone_idx`/`classify_tone_idx` and synthesized as sinusoids via
//! [`ToneSynthesizer`].

use crate::ambe::float::concealment::{ConcealParams, Concealer, Decision, Descriptor};
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
    concealer: Concealer,
}

impl AmbePlus2SynthesisDecoder {
    pub fn new() -> Self {
        Self {
            dequant: DecoderState::initial(),
            synth: MbeSynthesizer::new(),
            tone: ToneSynthesizer::new(),
            repeats: 0,
            error_policy: ErrorPolicy::default(),
            concealer: Concealer::new(ConcealParams::default()),
        }
    }

    /// Selects how channel errors are handled (see [`ErrorPolicy`]); the default is [`ErrorPolicy::Concealing`].
    pub fn with_error_policy(mut self, policy: ErrorPolicy) -> Self {
        self.error_policy = policy;
        self
    }

    /// Replaces the concealment constants (used to tune them; the defaults are the tuned set).
    pub fn with_conceal_params(mut self, params: ConcealParams) -> Self {
        self.concealer = Concealer::new(params);
        self
    }

    /// The raw (unprotected) bits considered for single-bit correction: the low bit of the voicing pattern and of the gain, the
    /// three low pitch bits and the low bits of the first spectral vectors (`d[35..44)`; see `examples/ambe_bit_sensitivity.rs`).
    const CORRECTABLE_BITS: [usize; 9] = [35, 36, 37, 38, 39, 40, 41, 42, 43];

    fn decode_concealing(&mut self, parsed: &super::ParsedFrame) -> Option<[f64; N]> {
        let mut candidates: Vec<(u32, Option<Descriptor>)> = Vec::new();
        let mut readings = Vec::new();
        for bit in std::iter::once(None).chain(Self::CORRECTABLE_BITS.iter().map(|&b| Some(b))) {
            let d = bit.map_or(parsed.d, |b| parsed.d ^ (1u64 << (48 - b)));
            let mut state = DecoderState { l: self.dequant.l, log2_ml: self.dequant.log2_ml.clone(), gamma: self.dequant.gamma };
            match dequantize(&extract_raw_parameters(d), &mut state) {
                DequantizedFrame::Speech(p) => {
                    candidates.push((bit.is_some() as u32, Some(Descriptor::new(p.w0, &p.voiced, &p.ml))));
                    readings.push(Some((p, state)));
                }
                _ => {
                    candidates.push((bit.is_some() as u32, None));
                    readings.push(None);
                }
            }
        }
        match self.concealer.decide(&candidates, parsed.epsilon_c0 + parsed.epsilon_c1) {
            Decision::Accept(i) => {
                let (p, state) = readings.swap_remove(i)?;
                self.dequant = state;
                self.tone.reset();
                self.synth.synthesize_speech(p.w0, &p.voiced, &p.ml, parsed.epsilon_c0, parsed.epsilon_c1)
            }
            Decision::Repeat { scale, reset } => {
                if reset {
                    self.dequant = DecoderState::initial();
                    self.synth = MbeSynthesizer::new();
                    return Some([0.0; N]);
                }
                if scale == 0.0 {
                    return Some([0.0; N]);
                }
                self.synth.synthesize_repeat_scaled(scale)
            }
        }
    }

    /// Decodes one logical 72-bit frame (see `interleave::interleaved_to_frame`) into 20 ms of PCM.
    pub fn decode_frame(&mut self, logical_frame: u128) -> Option<[f64; N]> {
        let parsed = parse_frame(logical_frame);
        let raw = extract_raw_parameters(parsed.d);
        if self.error_policy == ErrorPolicy::Concealing && classify_b0(raw.b0) == FrameKind::Speech {
            return self.decode_concealing(&parsed);
        }
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
