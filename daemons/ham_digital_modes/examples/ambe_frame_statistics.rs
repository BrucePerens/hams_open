// SPDX-License-Identifier: LGPL-3.0-or-later
//! Frame-to-frame statistics of the decoded D-STAR parameters on real speech, used to set the priors of the damaged-frame
//! concealment (pitch changes slowly, voicing rarely flips, gain and spectrum move smoothly). Prints quantiles.
use ham_digital_modes::ambe::float::dstar::decode::{dequantize, parse_frame, DStarDecoderState, DequantizedFrame};
use ham_digital_modes::ambe::float::dstar::encoder::Encoder;

const FILES: [&str; 4] = [
    "tests/fixtures/osr_speech/OSR_us_000_0010_8k.wav",
    "tests/fixtures/osr_speech/OSR_us_000_0011_8k.wav",
    "tests/fixtures/osr_speech/OSR_us_000_0030_8k.wav",
    "tests/fixtures/osr_speech/OSR_us_000_0031_8k.wav",
];

fn quantiles(mut v: Vec<f64>, name: &str) {
    v.sort_by(|a, b| a.total_cmp(b));
    let q = |p: f64| v[((v.len() - 1) as f64 * p) as usize];
    println!("{name}: n={} median {:.4} p75 {:.4} p90 {:.4} p95 {:.4} p99 {:.4}", v.len(), q(0.5), q(0.75), q(0.9), q(0.95), q(0.99));
}

fn main() {
    let (mut dpitch, mut dlevel, mut dvuv, mut dspec, mut dspec_voiced) = (Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for f in FILES {
        let bytes = std::fs::read(f).unwrap();
        let pcm: Vec<f64> = bytes[44..].chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]]) as f64).take(600 * 160).collect();
        let mut e = Encoder::new();
        e.push_samples(&pcm);
        let mut fr = Vec::new();
        while let Some(x) = e.next_frame() { fr.push(x) }
        fr.extend(e.finish());
        let mut st = DStarDecoderState::initial();
        let mut prev: Option<(f64, f64, Vec<bool>, Vec<f64>)> = None;
        for &frame in &fr {
            let DequantizedFrame::Speech(p) = dequantize(parse_frame(frame).d, &mut st) else { continue };
            let l = p.ml.len() - 1;
            let level = 20.0 * (p.ml[1..].iter().map(|m| m * m).sum::<f64>() / l as f64).sqrt().max(1e-3).log10();
            let voiced_bands = p.voiced[1..].iter().filter(|&&v| v).count();
            // spectrum on 16 relative-frequency points, log amplitude
            let spec: Vec<f64> = (0..16).map(|k| { let h = 1 + (k * l) / 16; 20.0 * p.ml[h].max(1e-3).log10() }).collect();
            let vuv: Vec<bool> = (0..8).map(|b| p.voiced[1 + (b * l) / 8]).collect();
            if let Some((pw, plevel, pvuv, pspec)) = &prev {
                if level > 20.0 && *plevel > 20.0 {
                    dlevel.push((level - plevel).abs());
                    dvuv.push(vuv.iter().zip(pvuv).filter(|(a, b)| a != b).count() as f64);
                    let d = (spec.iter().zip(pspec).map(|(a, b)| (a - b).powi(2)).sum::<f64>() / 16.0).sqrt();
                    dspec.push(d);
                    if voiced_bands > 0 && pvuv.iter().any(|&v| v) {
                        dpitch.push((p.w0 / pw).ln().abs());
                        dspec_voiced.push(d);
                    }
                }
            }
            prev = Some((p.w0, level, vuv, spec));
        }
    }
    quantiles(dpitch, "|ln(w0/w0_prev)| (voiced to voiced)");
    quantiles(dlevel, "|level change| dB");
    quantiles(dvuv, "voicing bands changed (of 8)");
    quantiles(dspec, "spectral shape change dB (all)");
    quantiles(dspec_voiced, "spectral shape change dB (voiced)");
}
