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
//! 0.646, 0030 0.939 / 0.639, 0031 0.930 / 0.627. Since the energy fix of 2026-11-07 the energy
//! index agrees too (0.998 to 1.000; before it, 0.45 to 0.69). `EncoderFixed`, for comparison, agrees on
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
        .chunks_exact(2)
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
        .chunks_exact(SAMPLES_PER_FRAME)
        .zip(reference.chunks_exact(BYTES_PER_FRAME))
    {
        let got = enc.encode(chunk.try_into().unwrap());
        let a = unpack_frame(&got, WO_BITS, E_BITS);
        let b = unpack_frame(want.try_into().unwrap(), WO_BITS, E_BITS);
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
        // The energy is computed as the reference computes it (2026-11-07): it agrees on all but a
        // handful of frames where the two Levinson-Durbin recursions round across a quantizer step.
        assert!(
            a.energy_exact >= 0.99,
            "OSR {n}: energy index agreement {:.3}",
            a.energy_exact
        );
        assert!(
            a.wo_exact >= 0.55,
            "OSR {n}: pitch index agreement {:.3}",
            a.wo_exact
        );
    }
}
