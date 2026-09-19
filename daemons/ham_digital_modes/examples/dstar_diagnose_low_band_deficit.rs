// SPDX-License-Identifier: LGPL-3.0-or-later
//! Offline diagnosis of D-STAR float synthesis being ~13 dB too quiet below 700 Hz vs the chip
//! (see `ambe_chip_pcm_vs_float_synthesis_half_rate.rs`). Replays the channel frames it dumped to
//! `/tmp/dstar_channel_payloads.hex` and the chip audio in `/tmp/dstar_chip_decoded.wav`, prints
//! voicing/amplitude statistics, and re-synthesizes under variants to see which one restores the
//! low-band level.
//!
//! **Findings (live chip, 200 frames)**: float synthesis is faithful to the dequantized `Ml`
//! (float/Ml ~constant across harmonics), but the chip's voiced lines fall ~50x relative to `Ml` from
//! k=1 to k=12, and the chip's pitch is a consistent ~1.032x this crate's `f0_from_b0` (p10 1.020, p90
//! 1.040). So D-STAR's mbelib-derived pitch formula and spectral-shape tables (DG/PRBA/HOC) differ from
//! the real chip; they need oracle fitting via the chip's decode direction.
//!
//! Usage: `cargo run --release --example dstar_diagnose_low_band_deficit`

use ham_digital_modes::ambe::float::dstar::decode::{dequantize, parse_frame, DStarDecoderState, DequantizedFrame};
use ham_digital_modes::ambe::float::dstar::interleave::wire_bytes_to_frame;
use ham_digital_modes::ambe::float::mbe_synthesis::MbeSynthesizer;
use rustfft::{num_complex::Complex64, FftPlanner};

fn psd_low(frames: &[[f64; 160]]) -> (f64, f64) {
    let mut planner = FftPlanner::<f64>::new();
    let fft = planner.plan_fft_forward(512);
    let (mut lo, mut mid) = (0.0, 0.0);
    for f in frames {
        let mut buf: Vec<Complex64> = f.iter().map(|&s| Complex64::new(s, 0.0)).collect();
        buf.resize(512, Complex64::new(0.0, 0.0));
        fft.process(&mut buf);
        lo += (0..26).map(|b| buf[b].norm_sqr()).sum::<f64>(); // 0-400 Hz
        mid += (26..64).map(|b| buf[b].norm_sqr()).sum::<f64>(); // 400-1000 Hz
    }
    (10.0 * lo.log10(), 10.0 * mid.log10())
}

fn main() {
    let hex = std::fs::read_to_string("/tmp/dstar_channel_payloads.hex").expect("run the harness first");
    let payloads: Vec<Vec<u8>> = hex
        .lines()
        .map(|l| (0..l.len() / 2).map(|i| u8::from_str_radix(&l[2 * i..2 * i + 2], 16).unwrap()).collect())
        .collect();
    let wav = std::fs::read("/tmp/dstar_chip_decoded.wav").expect("chip wav");
    let chip: Vec<f64> = wav[44..].chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]]) as f64).collect();
    let chip_frames: Vec<[f64; 160]> = chip.chunks_exact(160).map(|c| c.try_into().unwrap()).collect();

    let mut voiced_by_l = [(0usize, 0usize); 12];
    let mut ml_by_l = [(0.0f64, 0usize); 12];
    let mut l_sum = 0u32;
    let mut speech = 0usize;

    let variants = ["as-is", "all voiced", "harmonics 1-8 voiced", "all unvoiced"];
    for (vi, variant) in variants.iter().enumerate() {
        let mut state = DStarDecoderState::initial();
        let mut synth = MbeSynthesizer::new();
        let mut frames: Vec<[f64; 160]> = Vec::new();
        for p in &payloads {
            let mut wb = [0u8; 9];
            wb.copy_from_slice(&p[2..11]);
            let parsed = parse_frame(wire_bytes_to_frame(&wb));
            if let DequantizedFrame::Speech(mut params) = dequantize(parsed.d, &mut state) {
                if vi == 0 {
                    speech += 1;
                    l_sum += params.l;
                    for l in 1..=11usize.min(params.l as usize) {
                        voiced_by_l[l].1 += 1;
                        if params.voiced[l] {
                            voiced_by_l[l].0 += 1;
                        }
                        ml_by_l[l].0 += params.ml[l];
                        ml_by_l[l].1 += 1;
                    }
                }
                match vi {
                    1 => params.voiced.iter_mut().for_each(|v| *v = true),
                    2 => params.voiced.iter_mut().enumerate().for_each(|(i, v)| *v = (1..=8).contains(&i)),
                    3 => params.voiced.iter_mut().for_each(|v| *v = false),
                    _ => {}
                }
                if let Some(f) = synth.synthesize_speech(params.w0, &params.voiced, &params.ml, parsed.epsilon_c0, parsed.epsilon_c1) {
                    frames.push(f);
                    continue;
                }
            }
            frames.push([0.0; 160]);
        }
        let (lo, mid) = psd_low(&frames);
        println!("variant {variant:>22}: 0-400 Hz {lo:.1} dB, 400-1000 Hz {mid:.1} dB");
    }
    {
        // Per-frame chip/float RMS ratio, against that frame's pitch and voiced fraction.
        let mut state = DStarDecoderState::initial();
        let mut synth = MbeSynthesizer::new();
        let mut rows: Vec<(f64, f64, f64, f64)> = Vec::new(); // (ratio, f0 Hz, voiced frac, chip rms)
        for (i, p) in payloads.iter().enumerate() {
            let mut wb = [0u8; 9];
            wb.copy_from_slice(&p[2..11]);
            let parsed = parse_frame(wire_bytes_to_frame(&wb));
            if let DequantizedFrame::Speech(params) = dequantize(parsed.d, &mut state) {
                if let Some(f) = synth.synthesize_speech(params.w0, &params.voiced, &params.ml, parsed.epsilon_c0, parsed.epsilon_c1) {
                    let rms = |x: &[f64]| (x.iter().map(|v| v * v).sum::<f64>() / x.len() as f64).sqrt();
                    let (cr, fr) = (rms(&chip_frames[i]), rms(&f));
                    if cr > 300.0 && fr > 1.0 {
                        let vf = params.voiced[1..].iter().filter(|&&v| v).count() as f64 / params.l as f64;
                        rows.push((cr / fr, params.w0 * 8000.0 / (2.0 * std::f64::consts::PI), vf, cr));
                    }
                }
            }
        }
        rows.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
        let n = rows.len();
        println!("loud frames {n}: chip/float RMS ratio p10 {:.2} median {:.2} p90 {:.2}", rows[n / 10].0, rows[n / 2].0, rows[9 * n / 10].0);
        for (lo, hi) in [(0.0, 100.0), (100.0, 140.0), (140.0, 400.0)] {
            let sel: Vec<_> = rows.iter().filter(|r| r.1 >= lo && r.1 < hi).collect();
            if !sel.is_empty() {
                println!("  f0 {lo:>3.0}-{hi:<3.0} Hz: {} frames, mean ratio {:.2}", sel.len(), sel.iter().map(|r| r.0).sum::<f64>() / sel.len() as f64);
            }
        }
        for (lo, hi) in [(0.0, 0.34), (0.34, 0.67), (0.67, 1.01)] {
            let sel: Vec<_> = rows.iter().filter(|r| r.2 >= lo && r.2 < hi).collect();
            if !sel.is_empty() {
                println!("  voiced frac {lo:.2}-{hi:.2}: {} frames, mean ratio {:.2}", sel.len(), sel.iter().map(|r| r.0).sum::<f64>() / sel.len() as f64);
            }
        }
    }
    {
        // Per-harmonic line magnitude ratio chip/float on voiced harmonics of well-voiced frames.
        let mut planner = FftPlanner::<f64>::new();
        let fft = planner.plan_fft_forward(1024);
        let spectrum = |x: &[f64]| -> Vec<f64> {
            let n = x.len() as f64;
            let mut buf: Vec<Complex64> = x
                .iter()
                .enumerate()
                .map(|(k, &s)| Complex64::new(s * (0.5 - 0.5 * (2.0 * std::f64::consts::PI * k as f64 / (n - 1.0)).cos()), 0.0))
                .collect();
            buf.resize(1024, Complex64::new(0.0, 0.0));
            fft.process(&mut buf);
            buf.iter().map(|c| c.norm()).collect()
        };
        let mut state = DStarDecoderState::initial();
        let mut synth = MbeSynthesizer::new();
        let mut ratios: Vec<Vec<f64>> = vec![Vec::new(); 16];
        let mut chip_per_ml: Vec<Vec<f64>> = vec![Vec::new(); 16];
        let mut float_per_ml: Vec<Vec<f64>> = vec![Vec::new(); 16];
        for (i, p) in payloads.iter().enumerate() {
            let mut wb = [0u8; 9];
            wb.copy_from_slice(&p[2..11]);
            let parsed = parse_frame(wire_bytes_to_frame(&wb));
            if let DequantizedFrame::Speech(params) = dequantize(parsed.d, &mut state) {
                let vf = params.voiced[1..].iter().filter(|&&v| v).count() as f64 / params.l as f64;
                if let Some(f) = synth.synthesize_speech(params.w0, &params.voiced, &params.ml, parsed.epsilon_c0, parsed.epsilon_c1) {
                    if vf < 0.6 {
                        continue;
                    }
                    let (cm, fm) = (spectrum(&chip_frames[i]), spectrum(&f));
                    let f0_bins = params.w0 / (2.0 * std::f64::consts::PI) * 1024.0;
                    for k in 1..16usize.min(params.l as usize + 1) {
                        if !params.voiced[k] {
                            continue;
                        }
                        let b = (k as f64 * f0_bins).round() as usize;
                        let peak = |m: &[f64]| (b.saturating_sub(2)..=(b + 2).min(511)).map(|j| m[j]).fold(0.0, f64::max);
                        if peak(&fm) > 1.0 {
                            ratios[k].push(peak(&cm) / peak(&fm));
                            chip_per_ml[k].push(peak(&cm) / params.ml[k]);
                            float_per_ml[k].push(peak(&fm) / params.ml[k]);
                        }
                    }
                }
            }
        }
        println!("per-harmonic chip/float line ratio on voiced harmonics of >=60%-voiced frames:");
        for (k, r) in ratios.iter().enumerate().skip(1) {
            if r.len() > 2 {
                let mut v = r.clone();
                v.sort_by(|a, b| a.partial_cmp(b).unwrap());
                let med = |x: &Vec<f64>| { let mut y = x.clone(); y.sort_by(|a, b| a.partial_cmp(b).unwrap()); y[y.len() / 2] };
                println!("  k={k:>2}: n={:>3} chip/float {:.2}   chip/Ml {:.2}   float/Ml {:.2}", v.len(), v[v.len() / 2], med(&chip_per_ml[k]), med(&float_per_ml[k]));
            }
        }
    }
    {
        // Does the chip's real pitch match this crate's f0? For each well-voiced frame, search a
        // scale s in [0.85, 1.15] maximizing the chip spectrum's harmonic sum at k*s*f0 (k=1..6).
        let mut planner = FftPlanner::<f64>::new();
        let fft = planner.plan_fft_forward(2048);
        let mut state = DStarDecoderState::initial();
        let mut scales: Vec<f64> = Vec::new();
        for (i, p) in payloads.iter().enumerate() {
            let mut wb = [0u8; 9];
            wb.copy_from_slice(&p[2..11]);
            let parsed = parse_frame(wire_bytes_to_frame(&wb));
            if let DequantizedFrame::Speech(params) = dequantize(parsed.d, &mut state) {
                let vf = params.voiced[1..].iter().filter(|&&v| v).count() as f64 / params.l as f64;
                let x = &chip_frames[i];
                let rms = (x.iter().map(|v| v * v).sum::<f64>() / 160.0).sqrt();
                if vf < 0.6 || rms < 500.0 {
                    continue;
                }
                let mut buf: Vec<Complex64> = x
                    .iter()
                    .enumerate()
                    .map(|(k, &s)| Complex64::new(s * (0.5 - 0.5 * (2.0 * std::f64::consts::PI * k as f64 / 159.0).cos()), 0.0))
                    .collect();
                buf.resize(2048, Complex64::new(0.0, 0.0));
                fft.process(&mut buf);
                let mag: Vec<f64> = buf.iter().map(|c| c.norm()).collect();
                let f0_bins = params.w0 / (2.0 * std::f64::consts::PI) * 2048.0;
                let (mut best_s, mut best_v) = (1.0, -1.0);
                let mut s = 0.85;
                while s <= 1.15 {
                    let v: f64 = (1..=6).map(|k| mag[((k as f64 * f0_bins * s).round() as usize).min(1023)]).sum();
                    if v > best_v {
                        best_v = v;
                        best_s = s;
                    }
                    s += 0.002;
                }
                scales.push(best_s);
            }
        }
        scales.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let n = scales.len();
        println!("pitch scale (chip f0 / our f0) over {n} voiced loud frames: p10 {:.3} median {:.3} p90 {:.3}", scales[n / 10], scales[n / 2], scales[9 * n / 10]);
    }
    let (clo, cmid) = psd_low(&chip_frames);
    println!("{:>30}: 0-400 Hz {clo:.1} dB, 400-1000 Hz {cmid:.1} dB", "chip");
    println!("speech frames {speech}, mean L {:.1}", l_sum as f64 / speech.max(1) as f64);
    for l in 1..12 {
        println!(
            "harmonic {l:>2}: voiced {:>5.1}%, mean Ml {:>9.2}",
            100.0 * voiced_by_l[l].0 as f64 / voiced_by_l[l].1.max(1) as f64,
            ml_by_l[l].0 / ml_by_l[l].1.max(1) as f64
        );
    }
}
