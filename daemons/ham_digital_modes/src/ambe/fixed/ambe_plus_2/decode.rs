// SPDX-License-Identifier: LGPL-3.0-or-later
//! Fixed-point port of `ambe::float::ambe_plus_2::decode`'s own `dequantize` -- built on
//! [`crate::ambe::fixed::general::mbe_speech`]'s shared MBE speech-dequantize core, since AMBE+2
//! half-rate's own body is (per that module's doc comment) almost line-for-line identical to
//! D-STAR's. Frame classification (`classify_b0`/`FrameKind`), raw bit extraction, and every
//! tone/DTMF/Call-Progress function (`decode_tone_idx`, `dtmf_digit_from_tone_idx`,
//! `classify_tone_idx`) are already pure integer arithmetic in the float sibling and are reused
//! directly -- see this module's own re-exports below.

use super::tables_q16::{
    DG_Q16_16, HOC_B5_Q16_16, HOC_B6_Q16_16, HOC_B7_Q16_16, HOC_B8_Q16_16, PRBA24_Q16_16,
    PRBA58_Q16_16, W0_TABLE_Q16_16,
};
use crate::ambe::fixed::general::fixed_ops::{mul_q16, TWO_PI_Q16_16};
use crate::ambe::fixed::general::mbe_speech::{
    dequantize_speech, MbeDecoderState, RawSpeechParameters, SpeechParameters, SpeechTables,
};
use crate::ambe::float::ambe_plus_2::decode::FrameKind;
use crate::ambe::float::ambe_plus_2::tables::{L_TABLE, LMPRBL, VUV};

// Already pure integer in the float sibling -- reused directly, not duplicated.
pub use crate::ambe::float::ambe_plus_2::decode::{
    classify_b0, classify_tone_idx, decode_tone_idx, dtmf_digit_from_tone_idx, extract_raw_parameters,
    CallProgressTone, RawParameters, ToneIdentity,
};

/// The fixed-point equivalent of `ambe::float::ambe_plus_2::decode::DequantizedFrame`.
pub enum DequantizedFrame {
    Speech(SpeechParameters),
    /// `b0=121` or `123` only -- see `FrameKind::Erasure`'s own doc comment (float sibling).
    Erasure,
    Silence { l: u32, w0_q16: i32 },
    /// `b0` in `{120, 122, 126, 127}` -- decode the actual tone/digit via `decode_tone_idx(d)` on
    /// the frame's own `d`, exactly as the float sibling's own `DequantizedFrame::Tone` doc comment
    /// describes (that decode is already pure integer and needs no fixed-point port).
    Tone { raw: RawParameters },
}

/// `w0 = 2*pi/32` (mbelib's own fixed silence-frame frequency) as Q16.16 -- `32` is a power of two,
/// so this is an exact right-shift, not a lossy division.
const SILENCE_W0_Q16_16: i32 = TWO_PI_Q16_16 >> 5;

fn speech_tables() -> SpeechTables<'static> {
    SpeechTables {
        rho_q16: crate::ambe::fixed::general::mbe_speech::POINT_65_Q16_16,
        vuv: &VUV,
        dg_q16: &DG_Q16_16,
        prba24_q16: &PRBA24_Q16_16,
        prba58_q16: &PRBA58_Q16_16,
        lmprbl: &LMPRBL,
        hoc_b5_q16: &HOC_B5_Q16_16,
        hoc_b6_q16: &HOC_B6_Q16_16,
        hoc_b7_q16: &HOC_B7_Q16_16,
        hoc_b8_q16: &HOC_B8_Q16_16,
    }
}

/// The fixed-point equivalent of `ambe::float::ambe_plus_2::decode::dequantize`.
pub fn dequantize(raw: &RawParameters, state: &mut MbeDecoderState) -> DequantizedFrame {
    match classify_b0(raw.b0) {
        FrameKind::Erasure => return DequantizedFrame::Erasure,
        FrameKind::DetectedTone | FrameKind::CallProgress | FrameKind::Tone => {
            return DequantizedFrame::Tone { raw: *raw }
        }
        FrameKind::Silence => {
            let l = 14u32;
            state.l = l;
            state.gamma_q16 = 0;
            state.log2_ml_q16 = vec![0; l as usize + 1];
            return DequantizedFrame::Silence { l, w0_q16: SILENCE_W0_Q16_16 };
        }
        FrameKind::Speech => {}
    }

    let l = L_TABLE[raw.b0 as usize];
    // W0_TABLE actually stores f0 (matching the float sibling's own table name despite the mismatch
    // -- see `ambe::float::ambe_plus_2::decode::dequantize`'s own `f0`/`w0` split); w0 = f0 * 2*pi.
    let w0_q16 = mul_q16(W0_TABLE_Q16_16[raw.b0 as usize], TWO_PI_Q16_16);
    let raw_speech = RawSpeechParameters {
        b1: raw.b1,
        b2: raw.b2,
        b3: raw.b3,
        b4: raw.b4,
        b5: raw.b5,
        b6: raw.b6,
        b7: raw.b7,
        b8: raw.b8,
    };
    let tables = speech_tables();
    let params = dequantize_speech(l, w0_q16, w0_q16, &raw_speech, &tables, state);
    DequantizedFrame::Speech(params)
}
