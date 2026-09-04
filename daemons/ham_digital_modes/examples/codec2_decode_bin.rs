// SPDX-License-Identifier: LGPL-3.0-or-later
//! One-off manual cross-validation tool, not part of the automated test
//! suite (see `codec2_encode_wav.rs`'s own doc comment for why): decodes
//! a raw packed Codec2 3200bps bitstream (`BYTES_PER_FRAME` bytes per
//! frame, back-to-back -- the same format `codec2_encode_wav.rs`
//! produces, or a real reference encoder's own output) with this crate's
//! own `Decoder`, writing the result as a WAV file for comparison
//! against the real reference C decoder's own output on the identical
//! input bitstream.
//!
//! Usage: `cargo run --example codec2_decode_bin -- input.bin output.wav`

use ham_digital_modes::codec2_3200::{Decoder, BYTES_PER_FRAME, SAMPLES_PER_FRAME, SAMPLE_RATE};

fn write_wav_mono_i16(path: &str, samples: &[i16]) {
    let data_bytes = (samples.len() * 2) as u32;
    let mut out = Vec::with_capacity(44 + data_bytes as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_bytes).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
    out.extend_from_slice(&(SAMPLE_RATE * 2).to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_bytes.to_le_bytes());
    for &s in samples {
        out.extend_from_slice(&s.to_le_bytes());
    }
    std::fs::write(path, &out).unwrap_or_else(|e| panic!("{path}: {e}"));
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    assert_eq!(args.len(), 3, "usage: {} <input.bin> <output.wav>", args[0]);

    let data = std::fs::read(&args[1]).unwrap_or_else(|e| panic!("{}: {e}", args[1]));
    let n_frames = data.len() / BYTES_PER_FRAME;

    let mut decoder = Decoder::new();
    let mut samples = Vec::with_capacity(n_frames * SAMPLES_PER_FRAME);
    for f in 0..n_frames {
        let frame: [u8; BYTES_PER_FRAME] = data[f * BYTES_PER_FRAME..(f + 1) * BYTES_PER_FRAME]
            .try_into()
            .unwrap();
        let out = decoder.decode(&frame);
        samples.extend_from_slice(&out);
    }

    write_wav_mono_i16(&args[2], &samples);

    let sumsq: f64 = samples.iter().map(|&s| (s as f64) * (s as f64)).sum();
    let rms = (sumsq / samples.len() as f64).sqrt();
    let max_abs = samples.iter().map(|&s| s.unsigned_abs()).max().unwrap_or(0);
    eprintln!(
        "decoded {n_frames} frames, {} samples, RMS={rms:.1}, max|sample|={max_abs} -> {}",
        samples.len(),
        args[2]
    );
}
