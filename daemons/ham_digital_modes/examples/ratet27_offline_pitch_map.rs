// SPDX-License-Identifier: LGPL-3.0-or-later
//! Offline (no chip access): decodes captured chip frames (`ratet27_dump_frames_for_mbelib`'s
//! `ratet27_frames.txt`) with the TIA linear pitch map and the chip's measured log pitch map, and compares each
//! against the chip's own decoded PCM (`ratet27_chip.raw`): decoded/repeat frame counts, frame-RMS envelope
//! correlation, and the pooled chip/ours per-harmonic energy ratio (dB) by frequency band.
//!
//! Usage: `cargo run --release --example ratet27_offline_pitch_map -- [dir=/tmp]`

use ham_digital_modes::ambe::float::tia_102_baba::decode::{DecoderState, FrameOutcome};

fn env(x: &[f64]) -> Vec<f64> {
    x.chunks_exact(160).map(|c| (c.iter().map(|s| s * s).sum::<f64>() / 160.0).sqrt()).collect()
}
fn corr(a: &[f64], b: &[f64]) -> f64 {
    let n = a.len().min(b.len());
    let (a, b) = (&a[..n], &b[..n]);
    let (ma, mb) = (a.iter().sum::<f64>() / n as f64, b.iter().sum::<f64>() / n as f64);
    let (mut c, mut va, mut vb) = (0.0, 0.0, 0.0);
    for (x, y) in a.iter().zip(b) {
        c += (x - ma) * (y - mb);
        va += (x - ma).powi(2);
        vb += (y - mb).powi(2);
    }
    c / (va.sqrt() * vb.sqrt()).max(1e-12)
}

fn main() {
    let dir = std::env::args().nth(1).unwrap_or_else(|| "/tmp".to_string());
    let frames: Vec<[u32; 8]> = std::fs::read_to_string(format!("{dir}/ratet27_frames.txt"))
        .unwrap()
        .lines()
        .map(|l| {
            let v: Vec<u32> = l.split_whitespace().take(8).map(|t| u32::from_str_radix(t, 16).unwrap()).collect();
            std::array::from_fn(|i| v[i])
        })
        .collect();
    let chip: Vec<f64> = std::fs::read(format!("{dir}/ratet27_chip.raw"))
        .unwrap()
        .chunks_exact(2)
        .map(|b| i16::from_le_bytes([b[0], b[1]]) as f64)
        .collect();
    let alphas: Vec<Option<f64>> = std::env::var("L_ALPHAS").map(|v| v.split(',').map(|x| x.parse().ok()).collect()).unwrap_or_else(|_| vec![None]);
    for (name, chip_map, alpha) in [("TIA linear map", false, None)].into_iter().chain(alphas.iter().map(|&a| ("chip log map", true, a))) {
        let mut d = if chip_map { DecoderState::new_chip() } else { DecoderState::new() };
        let mut p = if chip_map { DecoderState::new_chip() } else { DecoderState::new() };
        d.set_l_alpha(alpha);
        p.set_l_alpha(alpha);
        let (mut decoded, mut repeat, mut mute) = (0, 0, 0);
        let mut pcm = Vec::new();
        for c in &frames {
            match p.decode_parameters(*c) {
                Some(FrameOutcome::Decoded(x)) => {
                    decoded += 1;
                    p.advance_history(&x);
                }
                Some(FrameOutcome::Repeat) => repeat += 1,
                Some(FrameOutcome::Mute) => mute += 1,
                None => {}
            }
            pcm.extend(d.decode_frame(*c).unwrap_or([0.0; 160]));
        }
        if chip_map {
            let bytes: Vec<u8> = pcm.iter().flat_map(|&x| (x as f32).to_le_bytes()).collect();
            std::fs::write(format!("{dir}/ratet27_float_chipmap_{}.raw", alpha.map(|a| format!("{a}")).unwrap_or("tia".into())), bytes).unwrap();
        }
        println!(
            "{name} alpha {alpha:?}: decoded {decoded}, repeat {repeat}, mute {mute}; envelope corr vs chip {:.4}; rms chip/ours {:.3}",
            corr(&env(&chip), &env(&pcm)),
            ((chip.iter().map(|s| s * s).sum::<f64>()) / pcm.iter().map(|s| s * s).sum::<f64>()).sqrt()
        );
    }
}
