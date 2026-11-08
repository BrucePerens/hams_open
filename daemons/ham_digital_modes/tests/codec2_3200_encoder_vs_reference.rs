// SPDX-License-Identifier: LGPL-3.0-or-later
//! `EncoderMbe` against the Codec2-mod C reference encoder, on real recorded speech.
//!
//! The test vectors are the reference encoder's own bitstreams for the four Open Speech Repository
//! sentences (`tests/fixtures/osr_speech`): `tests/fixtures/codec2_3200/osr_*_codec2mod_c_encoded_bits.bin`,
//! produced by M17-Project/Codec2-mod at commit 7c33428 (2026-09-30), x86-64, gcc 13.3 `-O2`,
//! glibc 2.39 (its accurate `cosf`; Codec2-mod's README notes a less accurate `cosf` can change 1 frame
//! in 150). The January 2026 commit vendored in `vendor/codec2-mod` produces byte-identical files.
//! Provenance in `tests/fixtures/codec2_3200/README.md`.
//!
//! Bit-exactness is not the target (see `codec2_3200/mod.rs`: an encoder has design freedom, and
//! this crate's pitch estimator and integer LPC path are its own). What is checked is what a
//! listener hears: the voicing decision and the pitch index agree with the reference on most
//! frames. Measured 2026-11-06 (voicing / pitch index exact): 0010 0.977 / 0.616, 0011 0.979 /
//! 0.646, 0030 0.939 / 0.639, 0031 0.930 / 0.627. The energy index is deliberately not compared:
//! this crate's line spectral pairs come from a white-noise-corrected predictor and its energy from
//! that same model, while the C encoder sends uncorrected ones, so the two energies differ by design
//! on predictable frames. What the energy must get right is the decoded loudness, checked below. `EncoderFixed`, for comparison, agrees on
//! voicing on 0.06 to 0.15 of frames and on the pitch index on 0.25 to 0.31.

use ham_digital_modes::codec2_3200::bits::unpack_frame;
use ham_digital_modes::codec2_3200::{
    EncoderMbe, BYTES_PER_FRAME, E_BITS, SAMPLES_PER_FRAME, WO_BITS,
};

fn dir() -> String {
    format!("{}/tests/fixtures", env!("CARGO_MANIFEST_DIR"))
}

fn read_wav(path: &str) -> Vec<i16> {
    let data = std::fs::read(path).unwrap_or_else(|e| panic!("{path}: {e}"));
    assert_eq!(&data[36..40], b"data", "{path}: expected a 44-byte header");
    data[44..]
        .as_chunks::<2>()
        .0
        .iter()
        .map(|b| i16::from_le_bytes([b[0], b[1]]))
        .collect()
}

struct Agreement {
    frames: usize,
    voicing: f64,
    wo_exact: f64,
    energy_exact: f64,
}

fn compare(n: &str) -> Agreement {
    let pcm = read_wav(&format!("{}/osr_speech/OSR_us_000_{n}_8k.wav", dir()));
    let reference = std::fs::read(format!(
        "{}/codec2_3200/osr_{n}_codec2mod_c_encoded_bits.bin",
        dir()
    ))
    .unwrap();
    let mut enc = EncoderMbe::new();
    let (mut frames, mut voicing, mut wo, mut energy) = (0usize, 0usize, 0usize, 0usize);
    for (chunk, want) in pcm
        .as_chunks::<SAMPLES_PER_FRAME>()
        .0
        .iter()
        .zip(reference.as_chunks::<BYTES_PER_FRAME>().0.iter())
    {
        let got = enc.encode(chunk);
        let a = unpack_frame(&got, WO_BITS, E_BITS);
        let b = unpack_frame(want, WO_BITS, E_BITS);
        frames += 1;
        voicing += (a.voiced0 == b.voiced0 && a.voiced1 == b.voiced1) as usize;
        wo += (a.wo_index == b.wo_index) as usize;
        energy += (a.e_index == b.e_index) as usize;
    }
    assert_eq!(
        frames,
        reference.len() / BYTES_PER_FRAME,
        "{n}: frame count"
    );
    Agreement {
        frames,
        voicing: voicing as f64 / frames as f64,
        wo_exact: wo as f64 / frames as f64,
        energy_exact: energy as f64 / frames as f64,
    }
}

// Tests [@ANCHOR: EncoderMbe::encode]
#[test]
fn encoder_mbe_voicing_and_pitch_agree_with_the_c_reference_on_real_speech() {
    for n in ["0010", "0011", "0030", "0031"] {
        let a = compare(n);
        println!(
            "OSR {n}: {} frames, voicing {:.3}, pitch index exact {:.3}, energy index exact {:.3}",
            a.frames, a.voicing, a.wo_exact, a.energy_exact
        );
        assert!(
            a.voicing >= 0.90,
            "OSR {n}: voicing agreement {:.3}",
            a.voicing
        );
        assert!(
            a.wo_exact >= 0.55,
            "OSR {n}: pitch index agreement {:.3}",
            a.wo_exact
        );
    }
}

/// Decoded loudness against the original, frame by frame (20 ms, active frames), for `EncoderMbe`
/// through `Decoder`. The decoder rebuilds each frame's level from the transmitted energy and the
/// envelope of the transmitted line spectral pairs, so this checks that the energy belongs to the
/// same (white-noise-corrected) model the envelope does. Measured 2026-11-07, mean error on the four
/// recordings (0010, 0011, 0030, 0031):
///
/// | Energy sent | Mean decoded loudness error |
/// |---|---|
/// | corrected predictor on corrected R (this crate) | -1.02, -0.98, -0.48, -0.95 dB |
/// | corrected predictor on uncorrected R (before 2026-11-07) | -1.45, -1.47, -0.80, -1.22 dB |
/// | uncorrected minimum error (what the C encoder sends with its own envelope) | -3.46, -3.82, -2.69, -2.83 dB |
///
/// The bound (2 dB) fails the last pairing, which was briefly committed on 2026-11-07 before this
/// test caught it (`EncoderFixed::lpc_fields`).
// Tests [@ANCHOR: EncoderFixed::lpc_fields]
#[test]
fn decoded_loudness_follows_the_original_speech() {
    use ham_digital_modes::codec2_3200::Decoder;
    let db = |s: &[i16]| {
        10.0 * (s.iter().map(|&x| (x as f64).powi(2)).sum::<f64>() / s.len() as f64 + 1.0).log10()
    };
    for n in ["0010", "0011", "0030", "0031"] {
        let pcm = read_wav(&format!("{}/osr_speech/OSR_us_000_{n}_8k.wav", dir()));
        let mut enc = EncoderMbe::new();
        let mut dec = Decoder::new();
        let mut out = Vec::new();
        for c in pcm.as_chunks::<SAMPLES_PER_FRAME>().0 {
            out.extend_from_slice(&dec.decode(&enc.encode(c)));
        }
        let frames = pcm.len() / SAMPLES_PER_FRAME;
        // The codec's own delay, found as the lag with the smallest mean loudness error.
        let err_at = |lag: usize| -> (f64, f64) {
            let (mut sum, mut abs, mut cnt) = (0.0, 0.0, 0.0);
            for f in 4..frames - 4 {
                let a = db(&pcm[f * 160..f * 160 + 160]);
                if a < 50.0 {
                    continue;
                }
                let d = db(&out[f * 160 + lag..f * 160 + lag + 160]) - a;
                sum += d;
                abs += d.abs();
                cnt += 1.0;
            }
            (sum / cnt, abs / cnt)
        };
        let (mean, mean_abs) = (100..240)
            .map(err_at)
            .min_by(|x, y| x.1.partial_cmp(&y.1).unwrap())
            .unwrap();
        println!(
            "OSR {n}: decoded loudness error mean {mean:+.2} dB, mean absolute {mean_abs:.2} dB"
        );
        assert!(
            mean.abs() < 2.0,
            "OSR {n}: mean loudness error {mean:+.2} dB"
        );
    }
}
