// SPDX-License-Identifier: LGPL-3.0-or-later
//! Amplitude-quantization cross-validation helper (see `docs/references/tia_102_baba_cross_validation.md`): builds
//! sequences of spectral amplitude vectors `M_l` (integers, so an external fixed-point encoder can take the same
//! numbers), runs them through this crate's [`quantize_spectral_amplitudes`] (Eq. 54-64) and writes
//!
//! * `<prefix>.sa`: one line per frame, `L M_1 .. M_L`, for the scratch driver around the OP25 `imbe_vocoder`
//!   encoder's own amplitude quantizer, and
//! * `<prefix>.q`: this crate's `b2 b3 .. b_{L+1}` for the same frame (zero-width higher-order entries printed as
//!   0), predicting from its own reconstructed history (`chain`) or from the initial state every frame (`reset`).
//!
//! Usage: `cargo run --release --example tia_102_baba_oracle_quant_dump -- <prefix> <chain|reset> [frames]`

use ham_digital_modes::ambe::float::tia_102_baba::parameter_encoding::dequantize_fundamental_frequency;
use ham_digital_modes::ambe::float::tia_102_baba::prediction::INITIAL_L_HAT_PREV;
use ham_digital_modes::ambe::float::tia_102_baba::quantize_spectral_amplitudes;
use ham_digital_modes::ambe::float::tia_102_baba::tables::higher_order_bit_allocation;
use ham_digital_modes::ambe::float::tia_102_baba::vuv::harmonics_count;
use std::fmt::Write as _;

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn uniform(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }
    /// Roughly Gaussian, unit variance.
    fn normal(&mut self) -> f64 {
        (0..12).map(|_| self.uniform()).sum::<f64>() - 6.0
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let prefix = &args[1];
    let chain = args[2] == "chain";
    let frames: usize = args.get(3).map(|s| s.parse().unwrap()).unwrap_or(2000);
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    let (mut sa_txt, mut q_txt) = (String::new(), String::new());
    let mut prev_l = INITIAL_L_HAT_PREV;
    let mut prev_m: Vec<f64> = vec![1.0; prev_l as usize];
    let mut b0 = 60u32;
    let mut level = 8.0; // log2 of the spectral level
    for _ in 0..frames {
        // Harmonic count: a slow walk with occasional jumps, so every L and every L transition occurs.
        if rng.next().is_multiple_of(5) {
            b0 = (rng.next() % 208) as u32;
        } else {
            b0 = (b0 as i64 + (rng.next() % 9) as i64 - 4).clamp(0, 207) as u32;
        }
        let l = harmonics_count(dequantize_fundamental_frequency(b0));
        level = (level + 0.6 * rng.normal()).clamp(3.0, 11.0);
        let slope = -0.02 - 0.06 * rng.uniform();
        let m: Vec<f64> = (1..=l)
            .map(|h| {
                let v = level + slope * h as f64 * 3.0 + 0.9 * rng.normal();
                v.exp2().round().clamp(1.0, 8000.0)
            })
            .collect();
        let (pl, pm) = if chain { (prev_l, prev_m.clone()) } else { (INITIAL_L_HAT_PREV, vec![1.0; INITIAL_L_HAT_PREV as usize]) };
        let q = quantize_spectral_amplitudes(&m, l, pl, &pm).expect("quantize");
        let _ = write!(sa_txt, "{l}");
        for v in &m {
            let _ = write!(sa_txt, " {}", *v as i64);
        }
        sa_txt.push('\n');
        let _ = write!(q_txt, "{}", q.b2);
        for (v, _) in q.gain_vector.iter() {
            let _ = write!(q_txt, " {v}");
        }
        let mut hi = q.higher_order.iter();
        for &w in higher_order_bit_allocation(l).unwrap() {
            let v = if w > 0 { hi.next().map(|x| x.0).unwrap_or(0) } else { 0 };
            let _ = write!(q_txt, " {v}");
        }
        q_txt.push('\n');
        prev_l = l;
        prev_m = q.reconstructed;
    }
    std::fs::write(format!("{prefix}.sa"), sa_txt).unwrap();
    std::fs::write(format!("{prefix}.q"), q_txt).unwrap();
}
