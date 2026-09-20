// SPDX-License-Identifier: LGPL-3.0-or-later
//! Audio quality of the damaged-frame policies under channel errors (no chip needed). Real speech is encoded, random bit errors
//! are injected into the 72-bit wire frames at several bit error rates (uniform and in bursts), and each policy's decode is
//! compared with the error-free decode: mean log-spectral distance over active frames (dB, lower is better), mean "excess
//! loudness" (dB by which a damaged frame is louder than the clean one beyond 3 dB: the bursts and chirps that are most
//! objectionable), the worst 1% of such excursions, and the fraction of active frames that were muted.
//!
//! `cargo run --release --example ambe_error_concealment_eval [frames-per-speaker]` (D-STAR; the AMBE+2 decoders share the policy code).

use ham_digital_modes::ambe::float::dstar::encoder::Encoder;
use ham_digital_modes::ambe::float::dstar::interleave::{frame_to_wire_bytes, wire_bytes_to_frame};
use ham_digital_modes::ambe::float::dstar::synthesis::DStarSynthesisDecoder;
use ham_digital_modes::ambe::float::mbe_synthesis::ErrorPolicy;
use rustfft::{num_complex::Complex64, FftPlanner};

const FILES: [&str; 4] = [
    "tests/fixtures/osr_speech/OSR_us_000_0010_8k.wav",
    "tests/fixtures/osr_speech/OSR_us_000_0011_8k.wav",
    "tests/fixtures/osr_speech/OSR_us_000_0030_8k.wav",
    "tests/fixtures/osr_speech/OSR_us_000_0031_8k.wav",
];

struct Lcg(u64);
impl Lcg {
    fn unit(&mut self) -> f64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (self.0 >> 11) as f64 / (1u64 << 53) as f64
    }
}

fn db(x: &[f64]) -> f64 {
    10.0 * (x.iter().map(|v| v * v).sum::<f64>() / x.len() as f64 + 1e-3).log10()
}

fn spectrum_db(x: &[f64]) -> Vec<f64> {
    let mut planner = FftPlanner::<f64>::new();
    let fft = planner.plan_fft_forward(256);
    let mut b: Vec<Complex64> = (0..256).map(|i| Complex64::new(if i < x.len() { x[i] * (0.5 - 0.5 * (2.0 * std::f64::consts::PI * i as f64 / 255.0).cos()) } else { 0.0 }, 0.0)).collect();
    fft.process(&mut b);
    (3..122).map(|k| 10.0 * (b[k].norm_sqr() + 1e-3).log10()).collect()
}

fn corrupt(frame: u128, ber: f64, burst: bool, rng: &mut Lcg, in_burst: &mut bool) -> u128 {
    let mut bytes = frame_to_wire_bytes(frame);
    let p = if burst {
        // Two-state channel: bad state flips 25% of bits, entered so the long-run error rate matches `ber`.
        if *in_burst { *in_burst = rng.unit() > 0.15 } else { *in_burst = rng.unit() < ber / 0.25 * 0.15 / (1.0 - ber / 0.25) }
        if *in_burst { 0.25 } else { 0.0 }
    } else {
        ber
    };
    for byte in bytes.iter_mut() {
        for bit in 0..8 {
            if rng.unit() < p {
                *byte ^= 1 << bit;
            }
        }
    }
    wire_bytes_to_frame(&bytes)
}

fn decode(frames: &[u128], policy: ErrorPolicy) -> Vec<f64> {
    let mut d = DStarSynthesisDecoder::new().with_error_policy(policy);
    frames.iter().flat_map(|&f| d.decode_frame(f).unwrap_or([0.0; 160])).collect()
}

fn main() {
    let n_frames: usize = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(400);
    let policies: Vec<(&str, ErrorPolicy)> = vec![("Clean (mbelib)", ErrorPolicy::Clean), ("ChipCompatible", ErrorPolicy::ChipCompatible)];
    let channels: [(&str, f64, bool); 6] = [("BER 0.5%", 0.005, false), ("BER 1%", 0.01, false), ("BER 2%", 0.02, false), ("BER 5%", 0.05, false), ("BER 10%", 0.10, false), ("bursty 3%", 0.03, true)];
    println!("| channel | policy | spectral distance dB | mean excess loudness dB | worst-1% excess dB | muted active frames |\n|---|---|---|---|---|---|");
    let mut clean_frames: Vec<Vec<u128>> = Vec::new();
    for f in FILES {
        let bytes = std::fs::read(f).unwrap();
        let pcm: Vec<f64> = bytes[44..].chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]]) as f64).take(n_frames * 160).collect();
        let mut e = Encoder::new();
        e.push_samples(&pcm);
        let mut fr = Vec::new();
        while let Some(x) = e.next_frame() { fr.push(x) }
        fr.extend(e.finish());
        clean_frames.push(fr);
    }
    for (cname, ber, burst) in channels {
        for (pname, policy) in &policies {
            let (mut lsd_sum, mut lsd_n, mut exc_sum, mut exc_n, mut muted, mut active) = (0.0, 0usize, 0.0, 0usize, 0usize, 0usize);
            let mut excursions: Vec<f64> = Vec::new();
            for (k, frames) in clean_frames.iter().enumerate() {
                let reference = decode(frames, ErrorPolicy::Clean);
                let mut rng = Lcg(1000 + k as u64 * 7 + (ber * 1e4) as u64);
                let mut in_burst = false;
                let damaged: Vec<u128> = frames.iter().map(|&f| corrupt(f, ber, burst, &mut rng, &mut in_burst)).collect();
                let out = decode(&damaged, *policy);
                for i in 0..frames.len() {
                    let (r, o) = (&reference[i * 160..(i + 1) * 160], &out[i * 160..(i + 1) * 160]);
                    let (dr, dob) = (db(r), db(o));
                    let excess = (dob - dr - 3.0).max(0.0);
                    excursions.push(excess);
                    exc_sum += excess;
                    exc_n += 1;
                    if dr > 30.0 {
                        active += 1;
                        if dob < dr - 25.0 { muted += 1; }
                        if i * 160 + 256 <= reference.len() {
                            let (sr, so) = (spectrum_db(&reference[i * 160..i * 160 + 256]), spectrum_db(&out[i * 160..i * 160 + 256]));
                            lsd_sum += (sr.iter().zip(&so).map(|(a, b)| (a - b).powi(2)).sum::<f64>() / sr.len() as f64).sqrt();
                            lsd_n += 1;
                        }
                    }
                }
            }
            excursions.sort_by(|a, b| a.total_cmp(b));
            let worst = excursions[(excursions.len() as f64 * 0.99) as usize];
            println!("| {cname} | {pname} | {:.2} | {:.2} | {worst:.1} | {:.1}% |", lsd_sum / lsd_n.max(1) as f64, exc_sum / exc_n.max(1) as f64, 100.0 * muted as f64 / active.max(1) as f64);
        }
    }
}
