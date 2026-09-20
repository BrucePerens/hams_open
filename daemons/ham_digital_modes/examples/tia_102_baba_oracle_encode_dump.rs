// SPDX-License-Identifier: LGPL-3.0-or-later
//! Encoder cross-validation helper: encodes raw 16-bit little-endian 8 kHz PCM with this crate's TIA-102.BABA
//! [`Encoder`] and prints, per frame, the same stage-by-stage line format as the scratch C++ driver that wraps the
//! OP25 `imbe_vocoder` encoder (an external test oracle; see `docs/references/tia_102_baba_cross_validation.md`), so
//! the two dumps can be compared frame by frame:
//!
//! ```text
//! F <k> pitchQ1 <2*P_hat_I> ep <E*4096> w0 <omega0_hat> L <L> K <K>
//! V <per-harmonic voicing bits>
//! B <b0 b1 b2 b3 ... b_{L+1}>          (zero-width higher-order entries printed as 0)
//! U <u0..u7 hex, before FEC>
//! ```
//!
//! Optional environment variables: `ORACLE_P` (evaluate the later stages at a given period instead of this crate's own
//! estimate), `DUMP_E` (print the `E(P)` table), `DUMP_ER` (print the refinement errors), `DUMP_V` (print the voicing
//! measures).
//!
//! Usage: `cargo run --release --example tia_102_baba_oracle_encode_dump -- <in.raw> <max_frames> [center_offset]`

use ham_digital_modes::ambe::float::tia_102_baba::bit_prioritization::deprioritize_bits;
use ham_digital_modes::ambe::float::tia_102_baba::encoder::Encoder;
use ham_digital_modes::ambe::float::tia_102_baba::parameter_encoding::{
    decode_voicing_decisions_per_harmonic, dequantize_fundamental_frequency,
};
use ham_digital_modes::ambe::float::tia_102_baba::tables::{gain_bit_allocation, higher_order_bit_allocation};
use ham_digital_modes::ambe::float::tia_102_baba::vuv::{frequency_bands_count, harmonics_count};
use ham_digital_modes::ambe::float::tia_102_baba::decode_code_vectors;
use ham_digital_modes::ambe::float::tia_102_baba::pitch::PitchAnalysisFrame;
use ham_digital_modes::ambe::float::tia_102_baba::encoder::HighPassFilter;
use ham_digital_modes::ambe::float::tia_102_baba::pitch_refinement::{refinement_error, RefinementFrame};
use ham_digital_modes::ambe::float::tia_102_baba::vuv::{energy_dependent_function, update_xi_max, voicing_measure, voicing_threshold, xi_hf, xi_lf};
use std::fmt::Write as _;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let data = std::fs::read(&args[1]).expect("input");
    let max_frames: usize = args[2].parse().unwrap();
    let offset: i32 = args.get(3).map(|s| s.parse().unwrap()).unwrap_or(0);
    let pcm: Vec<f64> = data.chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]]) as f64).collect();
    let mut enc = Encoder::new();
    enc.set_center_offset(offset);
    enc.push_samples(&pcm);
    let mut frames = Vec::new();
    let mut infos = Vec::new();
    while let Some(f) = enc.next_frame() {
        frames.push(f);
        infos.push(enc.last_analysis.unwrap());
    }
    let mut out = String::new();
    // Optional: per-oracle-frame refined periods (one per line), so V/UV measures can be evaluated at the
    // oracle's own fundamental frequency instead of ours.
    let oracle_p: Vec<f64> = std::env::var("ORACLE_P")
        .ok()
        .map(|f| std::fs::read_to_string(f).unwrap().lines().map(|l| l.trim().parse().unwrap()).collect())
        .unwrap_or_default();
    let mut xi_max_state = 20000.0f64;
    let mut prev_v: Vec<bool> = Vec::new();
    let mut raw = vec![0.0; 200];
    let mut hp = HighPassFilter::default();
    raw.extend(pcm.iter().map(|&x| hp.step(x).round()));
    raw.extend(std::iter::repeat_n(0.0, 400));
    for (k, (c, (p_i, w0, e))) in frames.iter().zip(infos.iter()).enumerate().take(max_frames) {
        let (u, _) = decode_code_vectors(*c);
        let b0_guess = ((u[0] >> 6) << 2) | ((u[7] >> 1) & 3);
        let l = harmonics_count(dequantize_fundamental_frequency(b0_guess));
        let kk = frequency_bands_count(l);
        let gw: [u8; 5] = std::array::from_fn(|i| gain_bit_allocation(l, i as u32 + 2).unwrap().0);
        let ho_all = higher_order_bit_allocation(l).unwrap();
        let hw: Vec<u8> = ho_all.iter().copied().filter(|&w| w > 0).collect();
        let d = deprioritize_bits(u, kk, gw, &hw).expect("deprioritize");
        let _ = writeln!(out, "F {k} pitchQ1 {} ep {} w0 {:.6} L {l} K {kk}", (2.0 * p_i).round() as i64, (e * 4096.0).round() as i64, w0);
        out.push_str("V ");
        for v in decode_voicing_decisions_per_harmonic(d.b1, kk, l) {
            out.push(if v { '1' } else { '0' });
        }
        let _ = write!(out, "\nB {} {} {}", d.b0, d.b1, d.b2);
        for (v, _) in d.gain_vector.iter() {
            let _ = write!(out, " {v}");
        }
        let mut hi = d.higher_order.iter();
        for &w in ho_all {
            let v = if w > 0 { hi.next().map(|x| x.0).unwrap_or(0) } else { 0 };
            let _ = write!(out, " {v}");
        }
        if std::env::var("DUMP_E").is_ok() {
            let center = (200 + k * 160) as i64 + offset as i64;
            let frame = PitchAnalysisFrame::new(&raw, center as usize);
            out.push_str("\nE");
            for i in 0..203 {
                let p = 21.0 + 0.5 * i as f64;
                let _ = write!(out, " {}", (frame.error_function(p) * 4096.0).round() as i64);
            }
        }
        if std::env::var("DUMP_ER").is_ok() {
            let rf = RefinementFrame::new(&raw, 200 + k * 160 + offset as usize);
            out.push_str("\nR");
            for i in -9..=9 {
                let p = p_i + i as f64 / 8.0;
                let _ = write!(out, " {:.6e}", refinement_error(&rf, 2.0 * std::f64::consts::PI / p));
            }
        }
        if std::env::var("DUMP_V").is_ok() {
            let rf = RefinementFrame::new(&raw, 200 + k * 160 + offset as usize);
            let w0v = oracle_p.get(k + 2).map(|p| 2.0 * std::f64::consts::PI / p).unwrap_or(*w0);
            let (lf, hf) = (xi_lf(&rf), xi_hf(&rf));
            xi_max_state = update_xi_max(xi_max_state, lf + hf);
            let m = energy_dependent_function(xi_max_state, lf + hf, lf, hf);
            let _ = write!(out, "\nVM {lf:.1} {hf:.1} {xi_max_state:.1} {m:.4}");
            let mut cur = Vec::new();
            for kb in 1..=kk {
                let dk = voicing_measure(&rf, kb, l, w0v, kb == kk);
                let pv = prev_v.get((kb - 1) as usize).copied().unwrap_or(false);
                let th = voicing_threshold(kb, w0v, *e, pv, m);
                cur.push(dk < th);
                let _ = write!(out, "\nVB {} {:.4} {:.4}", kb - 1, dk, th);
            }
            prev_v = cur;
        }
        let _ = writeln!(out, "\nU {}", u.iter().map(|x| format!("{x:x}")).collect::<Vec<_>>().join(" "));
    }
    print!("{out}");
}
