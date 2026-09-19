// SPDX-License-Identifier: LGPL-3.0-or-later
//! Corrects section 34's overclaim that `u4`'s two-value dither pattern differs by exactly `53` at
//! *every* tested point: that claim came from a fixed-200Hz, varying-amplitude dataset, and checking
//! it against `rms_normalized_pitch_sweep_57to444hz.tsv` (fixed amplitude, varying frequency, 20
//! frequencies from 60Hz to 440Hz) instead shows the step size actually depends on frequency. This
//! tool decodes `u4` from that dataset using the crate's own real `decode_block` (not a
//! reimplementation), reports every distinct value and gap per frequency, and computes each
//! frequency's nominal `harmonics_count` (`L_hat`) using this crate's own real `vuv::harmonics_count`
//! for direct comparison -- see `AMBE_CHIP_VALIDATION_FINDINGS.md` section 34 for the full writeup.
//!
//! Also reports whether each frequency's nominal period evenly divides the 160-sample frame: the
//! stimulus generator (`p25_ratet27_capture_rms_normalized_pitch_sweep.rs`) computes one 160-sample
//! buffer per frequency and resends it unchanged every frame, so a period that does *not* divide 160
//! evenly produces a phase discontinuity ("click") at every frame boundary -- a possible confound
//! for any frequency-dependent finding from this dataset, flagged here rather than left implicit.
//!
//! Usage: `cargo run --release --example ratet27_analyze_u4_dither_by_frequency`
use ham_digital_modes::ambe::ratet27_fec::decode_block;
use ham_digital_modes::ambe::ratet27_wire_format::Block;
use ham_digital_modes::ambe::vuv::harmonics_count;
use std::collections::BTreeMap;
use std::f64::consts::PI;

const SAMPLE_RATE: f64 = 8000.0;
const FRAME_SAMPLES: usize = 160;

fn hex_to_wire_bits(hexstr: &str) -> [bool; 144] {
    let mut bits = [false; 144];
    let bytes: Vec<u8> = (0..hexstr.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hexstr[i..i + 2], 16).unwrap())
        .collect();
    for (byte_idx, &byte) in bytes.iter().enumerate() {
        for bit_idx in 0..8 {
            bits[byte_idx * 8 + bit_idx] = (byte >> (7 - bit_idx)) & 1 == 1;
        }
    }
    bits
}

fn main() {
    let dataset = std::env::args().nth(1).unwrap_or_else(|| "rms_normalized_pitch_sweep_57to444hz.tsv".to_string());
    let label_prefix = std::env::args().nth(2).unwrap_or_else(|| "rmsnorm_".to_string());
    let data_path = format!(
        "{}/docs/references/ratet27_captures/{dataset}",
        env!("CARGO_MANIFEST_DIR")
    );
    let text = std::fs::read_to_string(&data_path).unwrap_or_else(|e| panic!("read {data_path}: {e}"));

    let mut by_freq: BTreeMap<u32, Vec<u16>> = BTreeMap::new();
    for line in text.lines() {
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() != 3 {
            continue;
        }
        let freq: u32 = parts[0].trim_start_matches(label_prefix.as_str()).parse().expect("freq label");
        let wire_bits = hex_to_wire_bits(parts[2]);
        let (u4, _distance) = decode_block(&wire_bits, Block::Hamming { index: 0 });
        by_freq.entry(freq).or_default().push(u4);
    }

    println!("{:>5}  {:>5}  {:>10}  {:>18}  {:>8}  divides_160", "freq", "Lhat", "distinct_u4", "gaps", "n_vals");
    for (&freq, vals) in &by_freq {
        let omega0 = 2.0 * PI * (freq as f64) / SAMPLE_RATE;
        let lhat = harmonics_count(omega0);
        let mut distinct: Vec<u16> = vals.clone();
        distinct.sort_unstable();
        distinct.dedup();
        let gaps: Vec<i32> = distinct.windows(2).map(|w| w[1] as i32 - w[0] as i32).collect();
        let period = SAMPLE_RATE / freq as f64;
        let divides = (FRAME_SAMPLES as f64 / period).fract().abs() < 1e-6;
        println!(
            "{freq:5}  {lhat:5}  {:>10}  {gaps:>18?}  {:>8}  {divides}",
            format!("{distinct:?}"),
            distinct.len()
        );
    }
}
