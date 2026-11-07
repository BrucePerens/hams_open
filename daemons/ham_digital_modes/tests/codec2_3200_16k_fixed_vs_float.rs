// SPDX-License-Identifier: LGPL-3.0-or-later
//! The fixed-point 16 kHz decode (`DecoderFixed::decode_16k_fixed`) against the float one
//! (`Decoder::decode_16k`) on real speech: the four Open Speech Repository sentences encoded with
//! `EncoderMbe`. The band above 4 kHz is the spectral bridge's (the envelope-driven version in
//! both since 2026-11-07); unvoiced frames carry noise with independent random phases in each, so
//! the waveforms differ by design and the comparison is of the 4-8 kHz band's energy, frame by
//! frame (20 ms), on frames where the original has speech.

use ham_digital_modes::codec2_3200::{Decoder, DecoderFixed, EncoderMbe, SAMPLES_PER_FRAME};
use rustfft::num_complex::Complex32;

fn read_wav(path: &str) -> Vec<i16> {
    let data = std::fs::read(path).unwrap_or_else(|e| panic!("{path}: {e}"));
    assert_eq!(&data[36..40], b"data", "{path}: expected a 44-byte header");
    data[44..]
        .chunks_exact(2)
        .map(|b| i16::from_le_bytes([b[0], b[1]]))
        .collect()
}

/// Energy (dB) of 4.5-8 kHz in one 320-sample (20 ms) frame at 16 kHz.
fn high_band_db(frame: &[i16]) -> f64 {
    let mut buf: Vec<Complex32> = frame
        .iter()
        .enumerate()
        .map(|(i, &s)| {
            let w = 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / frame.len() as f32).cos();
            Complex32::new(s as f32 * w, 0.0)
        })
        .collect();
    rustfft::FftPlanner::new()
        .plan_fft_forward(buf.len())
        .process(&mut buf);
    let n = buf.len();
    10.0 * (buf[n * 9 / 32..n / 2]
        .iter()
        .map(|c| c.norm_sqr() as f64)
        .sum::<f64>()
        + 1.0)
        .log10()
}

// Tests [@ANCHOR: SpectralBridgeStateFixed::synthesize_subframe_sb_from_envelope_fixed]
#[test]
fn fixed_and_float_16k_decodes_put_the_same_energy_above_4_khz() {
    for n in ["0010", "0011", "0030", "0031"] {
        let pcm = read_wav(&format!(
            "{}/tests/fixtures/osr_speech/OSR_us_000_{n}_8k.wav",
            env!("CARGO_MANIFEST_DIR")
        ));
        let mut enc = EncoderMbe::new();
        let (mut df, mut dq) = (Decoder::new(), DecoderFixed::new());
        let (mut of, mut oq) = (Vec::new(), Vec::new());
        let mut active = Vec::new();
        for c in pcm.chunks_exact(SAMPLES_PER_FRAME) {
            let bits = enc.encode(c.try_into().unwrap());
            of.extend_from_slice(&df.decode_16k(&bits));
            oq.extend_from_slice(&dq.decode_16k_fixed(&bits));
            let e = c.iter().map(|&s| (s as f64).powi(2)).sum::<f64>() / c.len() as f64;
            active.push(10.0 * (e + 1.0).log10() > 50.0);
        }
        let (mut sum, mut abs, mut cnt) = (0.0, 0.0, 0.0);
        for (f, &on) in active.iter().enumerate() {
            if !on {
                continue;
            }
            let d = high_band_db(&oq[f * 320..f * 320 + 320])
                - high_band_db(&of[f * 320..f * 320 + 320]);
            sum += d;
            abs += d.abs();
            cnt += 1.0;
        }
        let (mean, mean_abs) = (sum / cnt, abs / cnt);
        println!("OSR {n}: fixed - float, 4.5-8 kHz energy: mean {mean:+.3} dB, mean absolute {mean_abs:.3} dB over {cnt} frames");
        // Measured 2026-11-07: mean -0.059 to +0.015 dB, mean absolute 0.16 to 0.32 dB.
        assert!(mean.abs() < 0.25, "OSR {n}: mean {mean:+.3} dB");
        assert!(mean_abs < 1.0, "OSR {n}: mean absolute {mean_abs:.3} dB");
    }
}
