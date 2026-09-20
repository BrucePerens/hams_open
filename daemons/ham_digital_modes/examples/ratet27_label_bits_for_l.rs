// SPDX-License-Identifier: LGPL-3.0-or-later
//! Offline: labels every data bit of a frame's `u` vectors with the parameter the TIA layout assigns it, for
//! either the TIA pitch map or the chip's measured log map (`chip`), so a bit-flip probe's chip responses
//! (`ratet27_probe_bit_layout`) can be re-scored under each `L` hypothesis without touching the chip.
//! Prints `u<v>.<bit> <label> <L>` per data bit for the frame given as eight hex `u` values.
//!
//! Usage: `cargo run --release --example ratet27_label_bits_for_l -- <tia|chip> u0 u1 u2 u3 u4 u5 u6 u7`

use ham_digital_modes::ambe::float::tia_102_baba::bit_prioritization::{deprioritize_bits, extract_fundamental_frequency_quantizer};
use ham_digital_modes::ambe::dvsi_p25fec::pitch_map::dequantize_fundamental_frequency_chip;
use ham_digital_modes::ambe::float::tia_102_baba::parameter_encoding::dequantize_fundamental_frequency;
use ham_digital_modes::ambe::float::tia_102_baba::quantize::higher_order_coefficient_positions;
use ham_digital_modes::ambe::float::tia_102_baba::tables::{gain_bit_allocation, higher_order_bit_allocation};
use ham_digital_modes::ambe::float::tia_102_baba::vuv::{frequency_bands_count, harmonics_count};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let chip = args[0] == "chip";
    let u: [u32; 8] = std::array::from_fn(|i| u32::from_str_radix(&args[1 + i], 16).unwrap());
    let widths = [12usize, 12, 12, 12, 11, 11, 11, 7];
    let b0 = extract_fundamental_frequency_quantizer(&u);
    let w0 = if chip { dequantize_fundamental_frequency_chip(b0) } else { dequantize_fundamental_frequency(b0) };
    let l_hat = harmonics_count(w0).clamp(9, 56);
    let k_hat = frequency_bands_count(l_hat);
    let gain_widths: [u8; 5] = std::array::from_fn(|i| gain_bit_allocation(l_hat, i as u32 + 2).unwrap().0);
    let alloc = higher_order_bit_allocation(l_hat).unwrap();
    let higher_widths: Vec<u8> = alloc.iter().copied().filter(|&w| w > 0).collect();
    let positions = higher_order_coefficient_positions(l_hat).unwrap();
    let nonzero: Vec<(usize, usize)> = positions.iter().zip(alloc.iter()).filter(|(_, &w)| w > 0).map(|(&p, _)| p).collect();
    let base = deprioritize_bits(u, k_hat, gain_widths, &higher_widths).unwrap();
    for v in 0..8 {
        for bit in 0..widths[v] {
            let mut um = u;
            um[v] ^= 1 << bit;
            let label = match deprioritize_bits(um, k_hat, gain_widths, &higher_widths) {
                None => "?".to_string(),
                Some(b) => {
                    if b.b0 != base.b0 {
                        format!("b0.{}", (b.b0 ^ base.b0).trailing_zeros())
                    } else if b.b2 != base.b2 {
                        format!("b2.{}", (b.b2 ^ base.b2).trailing_zeros())
                    } else if b.b1 != base.b1 {
                        format!("b1.band{}", k_hat - (b.b1 ^ base.b1).trailing_zeros())
                    } else if let Some(i) = (0..5).find(|&i| b.gain_vector[i].0 != base.gain_vector[i].0) {
                        format!("gain[{}]", i + 2)
                    } else if let Some(j) = (0..b.higher_order.len()).find(|&j| b.higher_order[j].0 != base.higher_order[j].0) {
                        format!("hoc[blk{},k{}]", nonzero[j].0 + 1, nonzero[j].1)
                    } else {
                        "other".to_string()
                    }
                }
            };
            println!("u{v}.{bit} {label} {l_hat}");
        }
    }
}
