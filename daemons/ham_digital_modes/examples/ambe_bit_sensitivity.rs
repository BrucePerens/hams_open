// SPDX-License-Identifier: LGPL-3.0-or-later
//! How much audio damage does each of the 49 data bits cause when it is wrong? Real speech is encoded; then one data bit at a
//! time is flipped in every third frame (rebuilt as a valid frame, so this isolates the parameter's own sensitivity from the
//! channel code) and the decode is compared with the clean decode over the affected frames: mean log-envelope distance (dB) and
//! mean excess loudness (dB by which a damaged frame is louder than the clean one beyond 3 dB), plus whether the bit is
//! protected by a Golay code (`d[0..24)`) or transmitted raw (`d[24..49)`).
//!
//! `cargo run --release --features ambe_plus_2 --example ambe_bit_sensitivity [dstar|ambe_plus_2] [frames]`

use ham_digital_modes::ambe::float::dstar::encode::build_frame;
use rustfft::{num_complex::Complex64, FftPlanner};

const FILES: [&str; 4] = [
    "tests/fixtures/osr_speech/OSR_us_000_0010_8k.wav",
    "tests/fixtures/osr_speech/OSR_us_000_0011_8k.wav",
    "tests/fixtures/osr_speech/OSR_us_000_0030_8k.wav",
    "tests/fixtures/osr_speech/OSR_us_000_0031_8k.wav",
];

fn field_dstar(i: usize) -> String {
    let name = match i {
        0..=5 => format!("b0 pitch bit {}", 6 - i),
        6..=9 => format!("b2 gain bit {}", 5 - (i - 6)),
        10 | 11 => format!("b3 PRBA24 bit {}", 8 - (i - 10)),
        12..=16 => format!("b3 PRBA24 bit {}", 6 - (i - 12)),
        17..=21 => format!("b4 PRBA58 bit {}", 6 - (i - 17)),
        22 | 23 => format!("b5 HOC1 bit {}", 3 - (i - 22)),
        24 => "(unused)".to_string(),
        25 | 26 => format!("b5 HOC1 bit {}", 1 - (i - 25)),
        27..=30 => format!("b6 HOC2 bit {}", 3 - (i - 27)),
        31..=34 => format!("b7 HOC3 bit {}", 3 - (i - 31)),
        35..=37 => format!("b8 HOC4 bit {}", 3 - (i - 35)),
        38..=41 => format!("b1 V/UV bit {}", 3 - (i - 38)),
        42 | 43 => format!("b2 gain bit {}", 1 - (i - 42)),
        44 | 45 => format!("b3 PRBA24 bit {}", 1 - (i - 44)),
        46 | 47 => format!("b4 PRBA58 bit {}", 1 - (i - 46)),
        _ => "b0 pitch bit 0".to_string(),
    };
    name
}

fn field_a2(i: usize) -> String {
    match i {
        0..=3 => format!("b0 pitch bit {}", 6 - i),
        4..=7 => format!("b1 V/UV bit {}", 4 - (i - 4)),
        8..=11 => format!("b2 gain bit {}", 4 - (i - 8)),
        12..=19 => format!("b3 PRBA24 bit {}", 8 - (i - 12)),
        20..=23 => format!("b4 PRBA58 bit {}", 6 - (i - 20)),
        24..=27 => format!("b5 HOC1 bit {}", 4 - (i - 24)),
        28..=30 => format!("b6 HOC2 bit {}", 3 - (i - 28)),
        31..=33 => format!("b7 HOC3 bit {}", 3 - (i - 31)),
        34 => "b8 HOC4 bit 2".to_string(),
        35 => "b1 V/UV bit 0".to_string(),
        36 => "b2 gain bit 0".to_string(),
        37..=39 => format!("b0 pitch bit {}", 2 - (i - 37)),
        40 => "b3 PRBA24 bit 0".to_string(),
        41..=43 => format!("b4 PRBA58 bit {}", 2 - (i - 41)),
        44 => "b5 HOC1 bit 0".to_string(),
        45 => "b6 HOC2 bit 0".to_string(),
        46 => "b7 HOC3 bit 0".to_string(),
        _ => format!("b8 HOC4 bit {}", 1 - (i - 47)),
    }
}

type Decoder = Box<dyn Fn(&[u128]) -> Vec<f64>>;

fn db(x: &[f64]) -> f64 {
    10.0 * (x.iter().map(|v| v * v).sum::<f64>() / x.len() as f64 + 1e-3).log10()
}

/// Spectral envelope: energy in 16 bands with geometrically spaced edges (about 80-3800 Hz), in dB. Unlike the fine spectrum this
/// ignores where the harmonics fall, so a one-step pitch error (1.5%, below normal frame-to-frame jitter) costs almost nothing while
/// a wrong gain or spectral shape costs a lot, which is what listeners notice.
fn spectrum_db(x: &[f64]) -> Vec<f64> {
    let mut planner = FftPlanner::<f64>::new();
    let fft = planner.plan_fft_forward(256);
    let mut b: Vec<Complex64> = (0..256)
        .map(|i| {
            Complex64::new(
                if i < x.len() {
                    x[i] * (0.5 - 0.5 * (2.0 * std::f64::consts::PI * i as f64 / 255.0).cos())
                } else {
                    0.0
                },
                0.0,
            )
        })
        .collect();
    fft.process(&mut b);
    let edge = |k: usize| (3.0 * (122.0f64 / 3.0).powf(k as f64 / 16.0)).round() as usize;
    (0..16)
        .map(|k| {
            let (lo, hi) = (edge(k), edge(k + 1).max(edge(k) + 1));
            10.0 * (b[lo..hi].iter().map(|c| c.norm_sqr()).sum::<f64>() / (hi - lo) as f64 + 1e-3)
                .log10()
        })
        .collect()
}

fn main() {
    let which = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "dstar".to_string());
    let n_frames: usize = std::env::args()
        .nth(2)
        .and_then(|s| s.parse().ok())
        .unwrap_or(300);
    let is_dstar = which == "dstar";
    let mut per_bit = vec![(0.0f64, 0.0f64, 0usize); 49];
    for f in FILES {
        let bytes = std::fs::read(f).unwrap();
        let pcm: Vec<f64> = bytes[44..]
            .chunks_exact(2)
            .map(|b| i16::from_le_bytes([b[0], b[1]]) as f64)
            .take(n_frames * 160)
            .collect();
        let (frames, decode): (Vec<u128>, Decoder) = if is_dstar {
            use ham_digital_modes::ambe::float::dstar::{
                encoder::Encoder, synthesis::DStarSynthesisDecoder,
            };
            let mut e = Encoder::new();
            e.push_samples(&pcm);
            let mut fr = Vec::new();
            while let Some(x) = e.next_frame() {
                fr.push(x)
            }
            fr.extend(e.finish());
            (
                fr,
                Box::new(|fr: &[u128]| {
                    let mut d = DStarSynthesisDecoder::new();
                    fr.iter()
                        .flat_map(|&x| d.decode_frame(x).unwrap_or([0.0; 160]))
                        .collect()
                }),
            )
        } else {
            #[cfg(feature = "ambe_plus_2")]
            {
                use ham_digital_modes::ambe::float::ambe_plus_2::{
                    encoder::Encoder, synthesis::AmbePlus2SynthesisDecoder,
                };
                let mut e = Encoder::new();
                e.push_samples(&pcm);
                let mut fr = Vec::new();
                while let Some(x) = e.next_frame() {
                    fr.push(x)
                }
                fr.extend(e.finish());
                (
                    fr,
                    Box::new(|fr: &[u128]| {
                        let mut d = AmbePlus2SynthesisDecoder::new();
                        fr.iter()
                            .flat_map(|&x| d.decode_frame(x).unwrap_or([0.0; 160]))
                            .collect()
                    }),
                )
            }
            #[cfg(not(feature = "ambe_plus_2"))]
            panic!("build with --features ambe_plus_2")
        };
        // D-STAR and AMBE+2 half-rate share the frame-level channel code, so one parser serves both.
        let parse = |fr: u128| ham_digital_modes::ambe::float::dstar::decode::parse_frame(fr).d;
        let reference = decode(&frames);
        for (bit, slot) in per_bit.iter_mut().enumerate() {
            let flipped: Vec<u128> = frames
                .iter()
                .enumerate()
                .map(|(k, &fr)| {
                    if k % 3 == 0 {
                        build_frame(parse(fr) ^ (1u64 << (48 - bit)))
                    } else {
                        fr
                    }
                })
                .collect();
            let out = decode(&flipped);
            for k in (0..frames.len()).step_by(3) {
                let (r, o) = (
                    &reference[k * 160..(k + 1) * 160],
                    &out[k * 160..(k + 1) * 160],
                );
                if db(r) < 30.0 || k * 160 + 256 > reference.len() {
                    continue;
                }
                let (sr, so) = (
                    spectrum_db(&reference[k * 160..k * 160 + 256]),
                    spectrum_db(&out[k * 160..k * 160 + 256]),
                );
                slot.0 += (sr
                    .iter()
                    .zip(&so)
                    .map(|(a, b)| (a - b).powi(2))
                    .sum::<f64>()
                    / sr.len() as f64)
                    .sqrt();
                slot.1 += (db(o) - db(r) - 3.0).max(0.0);
                slot.2 += 1;
            }
        }
    }
    println!("| data bit | field | protected | envelope distance dB | excess loudness dB |\n|---|---|---|---|---|");
    let mut rows: Vec<(usize, f64, f64)> = per_bit
        .iter()
        .enumerate()
        .map(|(i, s)| (i, s.0 / s.2.max(1) as f64, s.1 / s.2.max(1) as f64))
        .collect();
    rows.sort_by(|a, b| b.1.total_cmp(&a.1));
    for (i, lsd, exc) in rows {
        let name = if is_dstar {
            field_dstar(i)
        } else {
            field_a2(i)
        };
        println!(
            "| d[{i}] | {name} | {} | {lsd:.2} | {exc:.2} |",
            if i < 24 { "Golay" } else { "raw" }
        );
    }
}
