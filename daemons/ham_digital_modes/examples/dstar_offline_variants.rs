// SPDX-License-Identifier: LGPL-3.0-or-later
//! Offline (no chip access) comparison of D-STAR synthesis variants against previously captured chip PCM.
//! For each capture directory (written by `ambe_chip_pcm_vs_float_synthesis_half_rate`, files
//! `dstar_channel_payloads.hex` and `dstar_chip_decoded.wav`), decodes the same frames with (A) the shipping
//! path (`DStarSynthesisDecoder`: RATET(27)-style enhancement + smoothing) and (B) raw dequantized parameters
//! straight into synthesis (no enhancement/smoothing), and reports frame-RMS envelope correlation and the mean
//! chip/ours per-harmonic energy ratio (dB) by harmonic index, pooled over all directories.
//!
//! Usage: `cargo run --release --example dstar_offline_variants -- <dir> [<dir>...]`

use ham_digital_modes::ambe::float::dstar::decode::{dequantize, parse_frame, DStarDecoderState, DequantizedFrame};
use ham_digital_modes::ambe::float::dstar::interleave::wire_bytes_to_frame;
use ham_digital_modes::ambe::float::dstar::synthesis::DStarSynthesisDecoder;
use ham_digital_modes::ambe::float::tia_102_baba::synthesis::SynthesisState;
use rustfft::{num_complex::Complex64, FftPlanner};

fn read_wav(path: &str) -> Vec<f64> {
    let data = std::fs::read(path).unwrap_or_else(|e| panic!("{path}: {e}"));
    data[44..].chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]]) as f64).collect()
}
fn env(x: &[f64]) -> Vec<f64> {
    x.chunks_exact(160).map(|c| (c.iter().map(|s| s * s).sum::<f64>() / 160.0).sqrt()).collect()
}
fn corr(a: &[f64], b: &[f64]) -> f64 {
    let n = a.len().min(b.len()) as f64;
    let (ma, mb) = (a.iter().take(n as usize).sum::<f64>() / n, b.iter().take(n as usize).sum::<f64>() / n);
    let (mut c, mut va, mut vb) = (0.0, 0.0, 0.0);
    for (x, y) in a.iter().zip(b) {
        c += (x - ma) * (y - mb);
        va += (x - ma).powi(2);
        vb += (y - mb).powi(2);
    }
    c / (va.sqrt() * vb.sqrt())
}
fn power_spectrum(seg: &[f64]) -> Vec<f64> {
    let mut planner = FftPlanner::<f64>::new();
    let fft = planner.plan_fft_forward(4096);
    let n = seg.len();
    let mut buf: Vec<Complex64> = seg
        .iter()
        .enumerate()
        .map(|(i, &s)| Complex64::new(s * (0.5 - 0.5 * (2.0 * std::f64::consts::PI * i as f64 / (n as f64 - 1.0)).cos()), 0.0))
        .collect();
    buf.resize(4096, Complex64::new(0.0, 0.0));
    fft.process(&mut buf);
    buf[..2048].iter().map(|c| c.norm_sqr()).collect()
}

fn main() {
    let dirs: Vec<String> = std::env::args().skip(1).collect();
    let mut sums = [[0.0f64; 17]; 2];
    let mut counts = [[0usize; 17]; 2];
    for dir in &dirs {
        let hex = std::fs::read_to_string(format!("{dir}/dstar_channel_payloads.hex")).unwrap();
        let chip = read_wav(&format!("{dir}/dstar_chip_decoded.wav"));
        let mut a = DStarSynthesisDecoder::new();
        let mut b_dequant = DStarDecoderState::initial();
        let mut b_synth = SynthesisState::new();
        let (mut pcm_a, mut pcm_b) = (Vec::new(), Vec::new());
        let mut f0s = Vec::new();
        for line in hex.lines() {
            let bytes: Vec<u8> = (0..line.len() / 2).map(|i| u8::from_str_radix(&line[2 * i..2 * i + 2], 16).unwrap()).collect();
            let mut wire = [0u8; 9];
            wire.copy_from_slice(&bytes[bytes.len() - 9..]);
            let frame = wire_bytes_to_frame(&wire);
            pcm_a.extend(a.decode_frame(frame).unwrap_or([0.0; 160]));
            let parsed = parse_frame(frame);
            match dequantize(parsed.d, &mut b_dequant) {
                DequantizedFrame::Speech(p) => {
                    f0s.push(Some(p.w0 * 8000.0 / (2.0 * std::f64::consts::PI)));
                    pcm_b.extend(
                        b_synth
                            .synthesize_frame_unenhanced(&p.ml[1..], p.w0, &p.voiced[1..])
                            .unwrap_or([0.0; 160]),
                    )
                }
                _ => {
                    f0s.push(None);
                    pcm_b.extend([0.0; 160])
                }
            }
        }
        println!(
            "{dir}: envelope corr chip vs shipping {:.4}, vs raw-params {:.4}",
            corr(&env(&chip), &env(&pcm_a)),
            corr(&env(&chip), &env(&pcm_b))
        );
        for i in 1..f0s.len().saturating_sub(1) {
            let Some(f0) = f0s[i] else { continue };
            if f0s[i - 1].is_none() || f0s[i + 1].is_none() || (f0s[i - 1].unwrap() / f0 - 1.0).abs() > 0.03 || (f0s[i + 1].unwrap() / f0 - 1.0).abs() > 0.03 {
                continue;
            }
            let span = |x: &[f64]| power_spectrum(&x[(i - 1) * 160..(i + 2) * 160]);
            let pc = span(&chip);
            for (v, pcm) in [(0usize, &pcm_a), (1usize, &pcm_b)] {
                let po = span(pcm);
                for k in 1..=16usize {
                    let (lo, hi) = (((k as f64 - 0.3) * f0 * 4096.0 / 8000.0) as usize, ((k as f64 + 0.3) * f0 * 4096.0 / 8000.0) as usize);
                    if hi >= 2048 {
                        continue;
                    }
                    let e = |p: &[f64]| p[lo..=hi].iter().sum::<f64>() + 1e-9;
                    sums[v][k] += 10.0 * (e(&pc) / e(&po)).log10();
                    counts[v][k] += 1;
                }
            }
        }
    }
    println!("k | chip/shipping dB | chip/raw-params dB");
    for k in 1..=16 {
        println!("{k:2} | {:6.1} | {:6.1}", sums[0][k] / counts[0][k].max(1) as f64, sums[1][k] / counts[1][k].max(1) as f64);
    }
}
