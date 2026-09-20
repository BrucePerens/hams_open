// SPDX-License-Identifier: LGPL-3.0-or-later
//! Bit-prioritization cross-validation helper (see `docs/references/tia_102_baba_cross_validation.md`): for every
//! harmonic count `L = 9..=56` draws random quantizer values within their allocated widths, prioritizes them with
//! this crate ([`prioritize_bits`], Fig. 22) and writes
//!
//! * `<prefix>.b`: one line per frame, `L b0 b1 b2 b3 .. b_{L+1} sync` (the form the scratch driver around the
//!   OP25 `imbe_vocoder` encoder's frame packing takes), and
//! * `<prefix>.u`: our eight bit vectors `u0..u7` (hex) for the same frames.
//!
//! The external packer's output for the `.b` file must equal the `.u` file, and its unpacker must return the `.b`
//! values from the `.u` file.
//!
//! Usage: `cargo run --release --example tia_102_baba_oracle_bits_dump -- <prefix> [draws_per_L]`

use ham_digital_modes::ambe::float::tia_102_baba::bit_prioritization::prioritize_bits;
use ham_digital_modes::ambe::float::tia_102_baba::parameter_encoding::dequantize_fundamental_frequency;
use ham_digital_modes::ambe::float::tia_102_baba::tables::{gain_bit_allocation, higher_order_bit_allocation};
use ham_digital_modes::ambe::float::tia_102_baba::vuv::{frequency_bands_count, harmonics_count};
use std::fmt::Write as _;

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let prefix = &args[1];
    let draws: usize = args.get(2).map(|s| s.parse().unwrap()).unwrap_or(40);
    let mut rng = Rng(0x2545_f491_4f6c_dd1d);
    let (mut b_txt, mut u_txt) = (String::new(), String::new());
    for target_l in 9u32..=56 {
        // Every b0 that maps to this L (Eq. 46-47).
        let b0s: Vec<u32> =
            (0u32..=207).filter(|&b| harmonics_count(dequantize_fundamental_frequency(b)) == target_l).collect();
        for d in 0..draws {
            let b0 = b0s[d % b0s.len()];
            let l = target_l;
            let k = frequency_bands_count(l);
            let gw: [u8; 5] = std::array::from_fn(|i| gain_bit_allocation(l, i as u32 + 2).unwrap().0);
            let ho_all = higher_order_bit_allocation(l).unwrap();
            let pick = |rng: &mut Rng, w: u8| -> u32 {
                match d % 5 {
                    0 => 0,
                    1 => (1u32 << w) - 1,
                    _ => (rng.next() as u32) & ((1u32 << w) - 1),
                }
            };
            let b1 = (rng.next() as u32) & ((1u32 << k) - 1);
            let b2 = (rng.next() % 64) as u32;
            let sync = d % 2 == 1;
            let gain: Vec<u32> = gw.iter().map(|&w| pick(&mut rng, w)).collect();
            let hoc_all: Vec<u32> = ho_all.iter().map(|&w| if w == 0 { 0 } else { pick(&mut rng, w) }).collect();
            let gain_vector: [(u32, u8); 5] = std::array::from_fn(|i| (gain[i], gw[i]));
            let higher_order: Vec<(u32, u8)> =
                ho_all.iter().zip(&hoc_all).filter(|(&w, _)| w > 0).map(|(&w, &v)| (v, w)).collect();
            let u = prioritize_bits(b0, b1, k, b2, gain_vector, &higher_order, sync).expect("prioritize");
            let _ = write!(b_txt, "{l} {b0} {b1} {b2}");
            for v in gain.iter().chain(hoc_all.iter()) {
                let _ = write!(b_txt, " {v}");
            }
            let _ = writeln!(b_txt, " {}", sync as u8);
            let _ = writeln!(u_txt, "{}", u.iter().map(|x| format!("{x:x}")).collect::<Vec<_>>().join(" "));
        }
    }
    std::fs::write(format!("{prefix}.b"), b_txt).unwrap();
    std::fs::write(format!("{prefix}.u"), u_txt).unwrap();
}
