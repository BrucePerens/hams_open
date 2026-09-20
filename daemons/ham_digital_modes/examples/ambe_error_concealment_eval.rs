// SPDX-License-Identifier: LGPL-3.0-or-later
//! Audio quality of the damaged-frame policies under channel errors (no chip needed). Real speech is encoded, random bit errors
//! are injected into the 72-bit wire frames at several bit error rates (uniform and in bursts), and each policy's decode is
//! compared with the error-free decode: mean log-envelope distance over active frames (dB, lower is better), mean "excess
//! loudness" (dB by which a damaged frame is louder than the clean one beyond 3 dB: the bursts and chirps that are most
//! objectionable), the worst 1% of such excursions, and the fraction of active frames that were muted.
//!
//! `cargo run --release --example ambe_error_concealment_eval [frames-per-speaker]` (D-STAR; the AMBE+2 decoders share the policy code).

use ham_digital_modes::ambe::float::dstar::encoder::Encoder;
use ham_digital_modes::ambe::float::dstar::interleave::{frame_to_wire_bytes as ds_to_wire, wire_bytes_to_frame as ds_from_wire};
use ham_digital_modes::ambe::float::dstar::synthesis::DStarSynthesisDecoder;
use ham_digital_modes::ambe::float::concealment::ConcealParams;
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

/// Spectral envelope: energy in 16 bands with geometrically spaced edges (about 80-3800 Hz), in dB. Unlike the fine spectrum this
/// ignores where the harmonics fall, so a one-step pitch error (1.5%, below normal frame-to-frame jitter) costs almost nothing while
/// a wrong gain or spectral shape costs a lot, which is what listeners notice.
fn spectrum_db(x: &[f64]) -> Vec<f64> {
    let mut planner = FftPlanner::<f64>::new();
    let fft = planner.plan_fft_forward(256);
    let mut b: Vec<Complex64> = (0..256).map(|i| Complex64::new(if i < x.len() { x[i] * (0.5 - 0.5 * (2.0 * std::f64::consts::PI * i as f64 / 255.0).cos()) } else { 0.0 }, 0.0)).collect();
    fft.process(&mut b);
    let edge = |k: usize| (3.0 * (122.0f64 / 3.0).powf(k as f64 / 16.0)).round() as usize;
    (0..16).map(|k| { let (lo, hi) = (edge(k), edge(k + 1).max(edge(k) + 1)); 10.0 * (b[lo..hi].iter().map(|c| c.norm_sqr()).sum::<f64>() / (hi - lo) as f64 + 1e-3).log10() }).collect()
}

fn is_a2() -> bool {
    std::env::var("MODE").as_deref() == Ok("ambe_plus_2")
}

fn frame_to_wire_bytes(frame: u128) -> [u8; 9] {
    if is_a2() {
        #[cfg(feature = "ambe_plus_2")]
        {
            let wire = ham_digital_modes::ambe::float::ambe_plus_2::interleave::frame_to_interleaved(frame);
            return std::array::from_fn(|i| ((wire >> (8 * (8 - i))) & 0xFF) as u8);
        }
    }
    ds_to_wire(frame)
}

fn wire_bytes_to_frame(b: &[u8; 9]) -> u128 {
    if is_a2() {
        #[cfg(feature = "ambe_plus_2")]
        {
            let mut wire: u128 = 0;
            for &x in b {
                wire = (wire << 8) | x as u128;
            }
            return ham_digital_modes::ambe::float::ambe_plus_2::interleave::interleaved_to_frame(wire);
        }
    }
    ds_from_wire(b)
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

/// `CP="level_scale_db=4,repeat..."` overrides the concealment constants (for tuning).
fn params_from_env() -> ConcealParams {
    let mut p = ConcealParams::default();
    if let Ok(v) = std::env::var("CP") {
        for kv in v.split(',') {
            let Some((k, x)) = kv.split_once('=') else { continue };
            let x: f64 = x.parse().unwrap();
            match k {
                "level" => p.level_scale_db = x,
                "shape" => p.shape_scale_db = x,
                "voicing" => p.voicing_cost = x,
                "pitch" => p.pitch_scale = x,
                "garbage" => p.garbage_cost = x,
                "suspicion" => p.error_suspicion = x,
                "alpha" => p.ber_alpha = x,
                "floor" => p.ber_floor = x,
                "gate" => p.flip_gate = x,
                "fade" => p.fade_per_frame = x,
                "reset" => p.reset_after = x as u32,
                "widen" => p.widen_per_repeat = x,
                _ => panic!("unknown key {k}"),
            }
        }
    }
    p
}

fn decode(frames: &[u128], policy: ErrorPolicy) -> Vec<f64> {
    #[cfg(feature = "ambe_plus_2")]
    if is_a2() {
        let mut d = ham_digital_modes::ambe::float::ambe_plus_2::synthesis::AmbePlus2SynthesisDecoder::new().with_error_policy(policy).with_conceal_params(params_from_env());
        return frames.iter().flat_map(|&f| d.decode_frame(f).unwrap_or([0.0; 160])).collect();
    }
    let mut d = DStarSynthesisDecoder::new().with_error_policy(policy).with_conceal_params(params_from_env());
    frames.iter().flat_map(|&f| d.decode_frame(f).unwrap_or([0.0; 160])).collect()
}

fn main() {
    let n_frames: usize = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(400);
    let policies: Vec<(&str, ErrorPolicy)> = vec![("Clean (mbelib)", ErrorPolicy::Clean), ("ChipCompatible", ErrorPolicy::ChipCompatible), ("Concealing", ErrorPolicy::Concealing)];
    let channels: [(&str, f64, bool); 7] = [("BER 0%", 0.0, false), ("BER 0.5%", 0.005, false), ("BER 1%", 0.01, false), ("BER 2%", 0.02, false), ("BER 5%", 0.05, false), ("BER 10%", 0.10, false), ("bursty 3%", 0.03, true)];
    println!("| channel | policy | envelope distance dB | mean excess loudness dB | worst-1% excess dB | muted active frames |\n|---|---|---|---|---|---|");
    let mut clean_frames: Vec<Vec<u128>> = Vec::new();
    for f in FILES {
        let bytes = std::fs::read(f).unwrap();
        let pcm: Vec<f64> = bytes[44..].chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]]) as f64).take(n_frames * 160).collect();
        let mut fr = Vec::new();
        #[cfg(feature = "ambe_plus_2")]
        if is_a2() {
            let mut e = ham_digital_modes::ambe::float::ambe_plus_2::encoder::Encoder::new();
            e.push_samples(&pcm);
            while let Some(x) = e.next_frame() { fr.push(x) }
            fr.extend(e.finish());
            clean_frames.push(fr);
            continue;
        }
        let mut e = Encoder::new();
        e.push_samples(&pcm);
        while let Some(x) = e.next_frame() { fr.push(x) }
        fr.extend(e.finish());
        clean_frames.push(fr);
    }
    let only = std::env::var("ONLY").ok();
    let mut score = 0.0;
    for (cname, ber, burst) in channels {
        for (pname, policy) in &policies {
            if only.as_deref().is_some_and(|o| !pname.starts_with(o)) {
                continue;
            }
            let (mut lsd_sum, mut lsd_n, mut exc_sum, mut exc_n, mut muted, mut active) = (0.0, 0usize, 0.0, 0usize, 0usize, 0usize);
            let mut excursions: Vec<f64> = Vec::new();
            for (k, frames) in clean_frames.iter().enumerate() {
                let reference = decode(frames, ErrorPolicy::Clean);
                let seed: u64 = std::env::var("SEED").ok().and_then(|v| v.parse().ok()).unwrap_or(0);
                let mut rng = Lcg(1000 + k as u64 * 7 + (ber * 1e4) as u64 + seed * 1_000_003);
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
            let (dist, exc, mut_pct) = (lsd_sum / lsd_n.max(1) as f64, exc_sum / exc_n.max(1) as f64, 100.0 * muted as f64 / active.max(1) as f64);
            println!("| {cname} | {pname} | {dist:.2} | {exc:.2} | {worst:.1} | {mut_pct:.1}% |");
            // Weighted objective: damage on the clean channel counts extra (concealment must not hurt good links).
            // Weights favour the error rates links actually run at: any loss at 0-2% is heavily penalized.
            let weight = match (burst, (ber * 1000.0).round() as u32) {
                (false, 0) => 10.0,
                (false, 5) => 6.0,
                (false, 10) => 4.0,
                (false, 20) => 2.5,
                (false, 50) => 1.0,
                (false, 100) => 0.5,
                (true, _) => 1.5,
                _ => 1.0,
            };
            score += weight * (dist + 4.0 * exc + 0.15 * worst + 0.05 * mut_pct);
        }
    }
    if only.is_some() {
        println!("SCORE {score:.3}");
    }
}
