// SPDX-License-Identifier: LGPL-3.0-or-later
//! One-off manual cross-validation tool, not part of the automated test
//! suite: encodes a real 8kHz mono 16-bit WAV with `codec2_3200::Encoder`
//! and writes the raw packed bitstream (`BYTES_PER_FRAME` bytes per
//! 20ms frame, back-to-back) to a file, so it can be fed to the real
//! vendored Codec2-mod C decoder for a genuine encode-in-Rust,
//! decode-in-C interoperability check. Deliberately kept out of
//! `build.rs`/`Cargo.toml` -- linking the vendored LGPL-2.1-only decoder
//! into this crate's own build, even for tests, would recreate exactly
//! the licensing entanglement this independent implementation exists to
//! avoid (see `src/codec2_3200/mod.rs`'s own doc comment); the C-side
//! decode half of this check runs as a fully separate, standalone build
//! outside this crate, the same way the reference-data-capture harness
//! used to build `tests/fixtures/codec2_3200/` did.
//!
//! Usage: `cargo run --example codec2_encode_wav -- input.wav output.bin`

use ham_digital_modes::codec2_3200::{Encoder, BYTES_PER_FRAME, SAMPLES_PER_FRAME};

fn read_wav_mono_i16(path: &str) -> Vec<i16> {
    let data = std::fs::read(path).unwrap_or_else(|e| panic!("{path}: {e}"));
    assert!(&data[36..40] == b"data", "{path}: not a standard 44-byte-header PCM WAV");
    data[44..].chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]])).collect()
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    assert_eq!(args.len(), 3, "usage: {} <input.wav> <output.bin>", args[0]);

    let samples = read_wav_mono_i16(&args[1]);
    let mut encoder = Encoder::new();
    let mut out = Vec::new();

    let n_frames = samples.len() / SAMPLES_PER_FRAME;
    for f in 0..n_frames {
        let frame: [i16; SAMPLES_PER_FRAME] = samples[f * SAMPLES_PER_FRAME..(f + 1) * SAMPLES_PER_FRAME].try_into().unwrap();
        let bits = encoder.encode(&frame);
        out.extend_from_slice(&bits);
    }

    std::fs::write(&args[2], &out).unwrap_or_else(|e| panic!("{}: {e}", args[2]));
    eprintln!("{} frames, {} bytes -> {}", n_frames, n_frames * BYTES_PER_FRAME, args[2]);
}
