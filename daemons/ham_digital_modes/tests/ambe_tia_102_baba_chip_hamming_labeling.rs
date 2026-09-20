// SPDX-License-Identifier: LGPL-3.0-or-later
//! Regression test: `DecoderState::decode_parameters` (float and fixed) must FEC-decode `c4..c6` with
//! the chip's real Hamming labeling (`dvsi_p25fec::fec::HAMMING_PARITY_CHIP`), not the textbook labeling.
//! Found via live chip data: on 200 clean loopback frames the textbook labeling reported false
//! corrected errors in 576 of 600 Hamming words (and decoded 377 of them to different data than the
//! chip-real labeling, which reported zero errors in all 600), corrupting `u_hat_4..u_hat_6` and
//! feeding false error counts to the frame-repeat/mute logic. A frame captured from the real chip is
//! error-free, so every FEC block must report zero corrected errors.

use ham_digital_modes::ambe::fixed::tia_102_baba::decode::{DecoderState as FixedDecoder, FrameOutcome as FixedOutcome};
use ham_digital_modes::ambe::float::tia_102_baba::decode::{DecoderState as FloatDecoder, FrameOutcome as FloatOutcome};

/// Real `c_hat_0..c_hat_7` captured from the live DVSI chip (`ratet27_dump_frames_for_mbelib`, first frames
/// of `OSR_us_000_0010_8k.wav`).
const REAL_CHIP_FRAMES: [[u32; 8]; 3] = [
    [0x200f68, 0x2553ea, 0xcf4b3, 0x20031d, 0x350, 0x3d3b, 0x70a6, 0x1b],
    [0x54dfd9, 0x6bf9a6, 0x2afcfa, 0x0, 0x1bc9, 0x1fe8, 0x20ca, 0x4],
    [0x20c838, 0x577e8b, 0x597605, 0x40063a, 0x1b64, 0x5bc0, 0x3121, 0x17],
];

#[test]
fn clean_chip_frames_report_zero_corrected_errors_in_float_and_fixed_decoders() {
    for (i, c) in REAL_CHIP_FRAMES.iter().enumerate() {
        if let Some(FloatOutcome::Decoded(p)) = FloatDecoder::new_chip_wire().decode_parameters(*c) {
            assert_eq!(p.errors.total, 0, "float frame {i}: clean chip frame reported corrected errors");
        }
        if let Some(FixedOutcome::Decoded(p)) = FixedDecoder::new_chip_wire().decode_parameters(*c) {
            assert_eq!(p.errors.total, 0, "fixed frame {i}: clean chip frame reported corrected errors");
        }
    }
    // At least the first frame must be a genuine decode (not a repeat/mute), so the loop above isn't vacuous.
    assert!(matches!(FloatDecoder::new_chip_wire().decode_parameters(REAL_CHIP_FRAMES[0]), Some(FloatOutcome::Decoded(_))));
    assert!(matches!(FixedDecoder::new_chip_wire().decode_parameters(REAL_CHIP_FRAMES[0]), Some(FixedOutcome::Decoded(_))));
}

/// The chip pitch map spans b0 0..=255, whose Eq. 47 harmonic count leaves Annex F/G's 9..=56 range at both ends; the
/// decoder must clamp instead of panicking (bug-hunt finding F1).
#[test]
fn chip_pitch_map_decoder_never_panics_on_any_b0() {
    let mut state = 0x1234_5678_9abc_def0u64;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    let mut d = FloatDecoder::new_chip();
    let _ = d.decode_frame([0; 8]);
    for _ in 0..3000 {
        let c: [u32; 8] = std::array::from_fn(|i| {
            let bits = [23, 23, 23, 23, 15, 15, 15, 7][i];
            (next() as u32) & ((1u32 << bits) - 1)
        });
        let _ = d.decode_frame(c);
    }
    // Every b0 explicitly: u0 top 6 bits and u7 bits 2:1.
    for b0 in 0u32..=255 {
        let u0 = ham_digital_modes::ambe::general::fec::golay_encode(((b0 >> 2) << 6) as u16);
        let c = [u0, 0, 0, 0, 0, 0, 0, (b0 & 3) << 1];
        let _ = FloatDecoder::new_chip().decode_frame(c);
    }
}
