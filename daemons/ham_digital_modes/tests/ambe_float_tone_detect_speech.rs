// SPDX-License-Identifier: LGPL-3.0-or-later
//! Real speech must never be reported as a tone: a false detection makes the encoder emit a tone frame that decoders
//! synthesize at tone level (24000 rms for AMBE+2), which once inflated a speech stream's decoded level 2.4x.

mod common;

use ham_digital_modes::ambe::float::tone_detect::detect_tone;

#[test]
fn speech_frames_are_never_detected_as_tones() {
    for name in ["0010", "0011", "0030", "0031"] {
        let pcm = common::read_wav_mono_i16(&format!("tests/fixtures/osr_speech/OSR_us_000_{name}_8k.wav"));
        let samples: Vec<f64> = pcm.iter().map(|&s| s as f64).collect();
        for (i, frame) in samples.chunks_exact(160).enumerate() {
            assert!(detect_tone(frame).is_none(), "OSR {name}: frame {i} of real speech detected as a tone");
        }
    }
}
