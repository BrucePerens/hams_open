// SPDX-License-Identifier: LGPL-3.0-or-later
//! Pins the exact bitstream of `EncoderFixed` (the encoder the relay's M17 transmitter uses) and of
//! `floating_reference::Encoder` on the four Open Speech Repository recordings
//! (`tests/fixtures/osr_speech`) and on pseudo-random input.
//!
//! The expected hashes were measured on 2026-11-06 on hams_open `5d7215e`, before the encoder took
//! over Codec2-mod's September 2026 change of computing the analysis autocorrelation only over the
//! analysis window's non-zero span (`window::ANALYSIS_SPAN`, see
//! `docs/reviews/M17_CODEC2_WOJCIECH_2026-11-06.md` in hams_com). That change is bit-exact by
//! construction (every skipped term is a product with a zero window coefficient), and these
//! hashes are the proof that it stayed so. A change that alters any encoded bit fails here; if a
//! change is meant to alter encoder output, re-measure and record why.
//!
//! Re-measured 2026-11-07, on purpose: the frame energy is now the prediction error of the predictor
//! fitted to the uncorrected autocorrelation (`EncoderFixed::lpc_fields`), as in every Codec2
//! encoder. Before, the white-noise-corrected predictor was used, which overstated the energy of
//! strongly predictable vowels by up to 6 quantizer steps. Only energy indexes changed (checked:
//! voicing, pitch and line spectral pair fields of all three encoders are as before on the four
//! recordings); the energy index now matches the Codec2-mod C encoder on 99.8 to 100 percent of
//! frames (`tests/codec2_3200_encoder_vs_reference.rs`).

use ham_digital_modes::codec2_3200::{EncoderFixed, BYTES_PER_FRAME, SAMPLES_PER_FRAME};

#[cfg(feature = "std")]
use ham_digital_modes::codec2_3200::EncoderMbe;

#[cfg(feature = "std")]
use ham_digital_modes::codec2_3200::floating_reference;

const FNV_OFFSET: u64 = 0xcbf29ce484222325;
const FNV_PRIME: u64 = 0x100000001b3;

fn fnv(h: &mut u64, bytes: &[u8]) {
    for &b in bytes {
        *h = (*h ^ b as u64).wrapping_mul(FNV_PRIME);
    }
}

fn read_wav(path: &str) -> Vec<i16> {
    let data = std::fs::read(path).unwrap_or_else(|e| panic!("{path}: {e}"));
    assert_eq!(&data[36..40], b"data", "{path}: expected a 44-byte header");
    data[44..]
        .chunks_exact(2)
        .map(|b| i16::from_le_bytes([b[0], b[1]]))
        .collect()
}

const OSR: [&str; 4] = ["0010", "0011", "0030", "0031"];

fn osr_path(n: &str) -> String {
    format!(
        "{}/tests/fixtures/osr_speech/OSR_us_000_{n}_8k.wav",
        env!("CARGO_MANIFEST_DIR")
    )
}

/// Pseudo-random speech-range input, 3000 frames (an LCG, so the same everywhere).
fn pseudo_random_frames() -> Vec<[i16; SAMPLES_PER_FRAME]> {
    let mut seed = 0x5eed_u64;
    (0..3000)
        .map(|k| {
            let scale = [200i32, 2000, 12000, 32767][k % 4];
            core::array::from_fn(|_| {
                seed = seed
                    .wrapping_mul(6364136223846793005)
                    .wrapping_add(1442695040888963407);
                (((seed >> 33) as i32 % (2 * scale + 1)) - scale) as i16
            })
        })
        .collect()
}

fn hash_with(mut encode: impl FnMut(&[i16; SAMPLES_PER_FRAME]) -> [u8; BYTES_PER_FRAME]) -> u64 {
    let mut h = FNV_OFFSET;
    for n in OSR {
        let pcm = read_wav(&osr_path(n));
        for chunk in pcm.chunks_exact(SAMPLES_PER_FRAME) {
            let frame: &[i16; SAMPLES_PER_FRAME] = chunk.try_into().unwrap();
            fnv(&mut h, &encode(frame));
        }
    }
    for frame in pseudo_random_frames() {
        fnv(&mut h, &encode(&frame));
    }
    h
}

// Tests [@ANCHOR: analysis_span]
#[test]
fn encoder_fixed_bitstream_on_real_speech_and_noise_is_unchanged() {
    let mut enc = EncoderFixed::new();
    let h = hash_with(|f| enc.encode(f));
    assert_eq!(
        h, 0xb818d7a678909b45,
        "EncoderFixed output changed: hash {h:016x}"
    );
}

#[cfg(feature = "std")]
#[test]
fn floating_reference_encoder_bitstream_on_real_speech_and_noise_is_unchanged() {
    let mut enc = floating_reference::Encoder::new();
    let h = hash_with(|f| enc.encode(f));
    assert_eq!(
        h, 0xc46811af01ba3fe3,
        "floating_reference::Encoder output changed: hash {h:016x}"
    );
}

/// `EncoderMbe` (the relay's M17 voice encoder since 2026-11-06), pinned the same way when it was
/// added; `tests/codec2_3200_encoder_vs_reference.rs` holds its comparison with the C reference.
#[cfg(feature = "std")]
#[test]
fn encoder_mbe_bitstream_on_real_speech_and_noise_is_unchanged() {
    let mut enc = EncoderMbe::new();
    let h = hash_with(|f| enc.encode(f));
    assert_eq!(
        h, 0x1313b0d90fd21988,
        "EncoderMbe output changed: hash {h:016x}"
    );
}
