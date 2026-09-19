// SPDX-License-Identifier: LGPL-3.0-or-later
//! Real-speech sanity test for this crate's own from-spec P25 AMBE codec (`src/ambe/`): every other
//! test and live-chip-validation harness in this crate to date has driven `encode_frame` with a
//! synthetic pure tone or synthetic harmonic signal at an analytically-known pitch (deliberately, to
//! isolate specific pipeline stages -- see e.g. `examples/ambe_chip_validate_p25.rs`'s own doc
//! comment) -- never real recorded human speech, and never with the codec's own pitch *estimator*
//! actually exercised (every existing test bypasses `pitch`/`pitch_refinement` entirely and hands
//! `encode_frame` a known-correct `omega0_hat` directly). This is a real, previously-unaddressed gap:
//! nothing before this test has confirmed the encode -> decode -> synthesis pipeline survives real
//! speech's actual dynamic range, spectral variety, and genuine (unknown, non-stationary) pitch.
//!
//! Fixture: `tests/fixtures/osr_speech/` (see that directory's own `README.md` for provenance and
//! license -- the Open Speech Repository's Harvard-sentence recordings, 8kHz 16-bit mono PCM,
//! already at this codec's own native sample rate).
//!
//! Scope, stated honestly: this test uses a **simple, per-frame grid search** over
//! `pitch::PitchAnalysisFrame::error_function`'s own spec-referenced candidate set (`21..=122` in
//! half-sample steps, per that function's own doc comment) plus `pitch_refinement::refine_pitch`'s
//! half-sample refinement, *not* the fuller cross-frame `look_back_pitch_tracking`/
//! `look_ahead_pitch_tracking`/`choose_initial_pitch_estimate` state machine those functions exist to
//! support -- no caller composes that fuller pipeline anywhere in this crate yet, and getting its
//! multi-frame history-carrying logic exactly right is a separate, real piece of work (a good next
//! step) that this test's own real goal -- exercising the *encoder/decoder/synthesis* pipeline against
//! real speech, not validating the pitch tracker's own frame-to-frame tracking quality -- doesn't
//! require getting exactly spec-correct. This test therefore checks robustness (no panics, no
//! NaN/infinite output, output level bounded), not perceptual quality or spec-exact pitch tracking.

use ham_digital_modes::ambe::float::ratet27::decode::DecoderState;
use ham_digital_modes::ambe::float::ratet27::pitch::PitchAnalysisFrame;
use ham_digital_modes::ambe::float::ratet27::pitch_refinement::{refine_pitch, RefinementFrame};
use ham_digital_modes::ambe::float::ratet27::{encode_frame, FrameState};

const FRAME_SAMPLES: usize = 160;
const MARGIN: usize = 200; // pitch analysis needs 150 samples of margin, refinement needs its own

fn read_wav_mono_i16(path: &str) -> Vec<i16> {
    let data = std::fs::read(path).unwrap_or_else(|e| panic!("{path}: {e}"));
    assert_eq!(&data[8..12], b"WAVE", "{path}: not a RIFF/WAVE file");
    assert_eq!(&data[36..40], b"data", "{path}: not a standard 44-byte-header PCM WAV");
    data[44..]
        .chunks_exact(2)
        .map(|b| i16::from_le_bytes([b[0], b[1]]))
        .collect()
}

/// A simple, honest (not spec-exact) per-frame pitch estimate: grid-search
/// `PitchAnalysisFrame::error_function` over its own documented candidate range, then refine to
/// sub-sample accuracy -- see this file's own module doc comment for why this is enough for this
/// test's real purpose without implementing the fuller cross-frame tracking state machine. Returns
/// `omega0_hat` directly (`refine_pitch`'s own return value, per its doc comment -- an angular
/// frequency, not a period).
fn estimate_omega0(raw: &[f64], center: usize) -> f64 {
    let analysis = PitchAnalysisFrame::new(raw, center);
    let mut best_period = 21.0;
    let mut best_error = f64::INFINITY;
    let mut p = 21.0;
    while p <= 122.0 {
        let e = analysis.error_function(p);
        if e < best_error {
            best_error = e;
            best_period = p;
        }
        p += 0.5;
    }
    let refinement = RefinementFrame::new(raw, center);
    refine_pitch(&refinement, best_period)
}

fn assert_frame_is_sane(pcm: &[f64], context: &str) {
    for (i, &s) in pcm.iter().enumerate() {
        assert!(
            s.is_finite(),
            "{context}: sample {i} is not finite ({s}) -- real speech must never produce NaN/Inf PCM"
        );
        assert!(
            s.abs() < 40_000.0,
            "{context}: sample {i} = {s} is wildly out of range for 16-bit-derived PCM"
        );
    }
}

fn run_one_file(path: &str) {
    let samples_i16 = read_wav_mono_i16(path);
    assert!(
        samples_i16.len() > 20 * FRAME_SAMPLES,
        "{path}: expected a real multi-second recording, got only {} samples",
        samples_i16.len()
    );
    let raw: Vec<f64> = samples_i16.iter().map(|&s| s as f64).collect();

    let num_frames = (raw.len() - MARGIN) / FRAME_SAMPLES - 1;
    let mut encoder_state = FrameState::initial();
    let mut decoder = DecoderState::new();

    let mut encoded_frames = 0usize;
    let mut decoded_frames = 0usize;

    for frame_idx in 0..num_frames {
        let center = MARGIN + frame_idx * FRAME_SAMPLES;
        let omega0_hat = estimate_omega0(&raw, center);

        let frame = RefinementFrame::new(&raw, center);
        let Some((c, next_state)) = encode_frame(&frame, omega0_hat, 0.02, &encoder_state, false) else {
            // A genuinely silent or degenerate frame (e.g. leading/trailing silence in the
            // recording) failing analysis is expected and fine -- not every 20ms window of a real
            // recording has to look like valid voiced/unvoiced speech.
            continue;
        };
        encoder_state = next_state;
        encoded_frames += 1;

        if let Some(pcm) = decoder.decode_frame(c) {
            assert_frame_is_sane(&pcm, &format!("{path} frame {frame_idx}"));
            decoded_frames += 1;
        }
    }

    assert!(
        encoded_frames > num_frames / 4,
        "{path}: only {encoded_frames}/{num_frames} frames encoded successfully -- \
         expected most real-speech frames to encode"
    );
    assert!(
        decoded_frames > 0,
        "{path}: zero frames decoded successfully out of {encoded_frames} encoded"
    );
    println!("{path}: {num_frames} total, {encoded_frames} encoded, {decoded_frames} decoded -- all sane");
}

// Expensive: the per-frame pitch grid search (~200 candidates x 301-sample error-function
// evaluations) over ~7700 total frames across all four fixtures takes tens of seconds even in
// release mode, multiples of that in the default `cargo test` debug profile -- ignored by default
// per this crate's own established convention (see e.g. `src/wspr_decode.rs`'s `#[ignore]`d tests)
// so the routine test suite stays fast. Run explicitly with
// `cargo test --release --test ambe_real_speech_round_trip -- --ignored --nocapture`.
#[test]
#[ignore]
fn full_pipeline_survives_real_speech_from_the_open_speech_repository() {
    let fixtures = [
        "tests/fixtures/osr_speech/OSR_us_000_0010_8k.wav",
        "tests/fixtures/osr_speech/OSR_us_000_0011_8k.wav",
        "tests/fixtures/osr_speech/OSR_us_000_0030_8k.wav",
        "tests/fixtures/osr_speech/OSR_us_000_0031_8k.wav",
    ];
    for path in fixtures {
        run_one_file(path);
    }
}
