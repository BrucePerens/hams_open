// SPDX-License-Identifier: LGPL-3.0-or-later
//! One-off manual cross-validation tool, not part of the automated test
//! suite: encodes a headerless 8kHz mono 16-bit raw PCM file with
//! `codec2_1600::Encoder` and writes the raw packed bitstream
//! (`BYTES_PER_FRAME` bytes per 40ms frame, back-to-back) to a file, so
//! it can be fed to a real unmodified plain-upstream-Codec2 `c2dec 1600`
//! for a genuine encode-in-Rust, decode-in-C interoperability check.
//! Deliberately kept out of the automated build -- same reasoning
//! `examples/codec2_encode_wav.rs`'s own doc comment gives for
//! `codec2_3200`.
//!
//! Usage: `cargo run --example codec2_1600_encode_raw -- input.raw output.bin`

use ham_digital_modes::codec2_1600::{Encoder, BYTES_PER_FRAME, SAMPLES_PER_FRAME};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    assert_eq!(args.len(), 3, "usage: {} <input.raw> <output.bin>", args[0]);

    let data = std::fs::read(&args[1]).unwrap_or_else(|e| panic!("{}: {e}", args[1]));
    let samples: Vec<i16> = data
        .chunks_exact(2)
        .map(|b| i16::from_le_bytes([b[0], b[1]]))
        .collect();

    let mut encoder = Encoder::new();
    let mut out = Vec::new();
    let n_frames = samples.len() / SAMPLES_PER_FRAME;
    for f in 0..n_frames {
        let frame: [i16; SAMPLES_PER_FRAME] = samples[f * SAMPLES_PER_FRAME..(f + 1) * SAMPLES_PER_FRAME]
            .try_into()
            .unwrap();
        out.extend_from_slice(&encoder.encode(&frame));
    }
    eprintln!(
        "encoded {} samples as {} frames ({} bytes) into {}",
        samples.len(),
        n_frames,
        out.len(),
        args[2]
    );
    assert_eq!(out.len(), n_frames * BYTES_PER_FRAME);
    std::fs::write(&args[2], out).unwrap_or_else(|e| panic!("{}: {e}", args[2]));
}
