// SPDX-License-Identifier: LGPL-3.0-or-later
//! Offline check (no chip needed; reads `ratet27_dump_frames_for_mbelib`'s `ratet27_frames.txt`): for
//! real, clean chip-produced frames, how many bit errors do the textbook Hamming code
//! (`general::fec::hamming_decode`, what `DecoderState::decode_parameters` uses) and the chip-real
//! Hamming labeling (`dvsi_p25fec::fec::hamming_decode_chip`, `AMBE_CHIP_VALIDATION_FINDINGS.md` section 23)
//! each report for `c4..c6`, and how often do the two recover different data bits? A clean loopback
//! frame has zero true errors, so a nonzero, constant "corrected" count means that decoder is using the
//! wrong code.
//!
//! Usage: `cargo run --release --example ratet27_probe_hamming_labeling -- [/tmp/ratet27_frames.txt]`

use ham_digital_modes::ambe::dvsi_p25fec::fec::hamming_decode_chip;
use ham_digital_modes::ambe::general::fec::{golay_decode, hamming_decode};

fn main() {
    let path = std::env::args().nth(1).unwrap_or_else(|| "/tmp/ratet27_frames.txt".to_string());
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
    let (mut frames, mut golay_err_frames) = (0usize, 0usize);
    let (mut textbook_err_words, mut chip_err_words, mut differing_data_words, mut words) = (0usize, 0usize, 0usize, 0usize);
    for line in text.lines() {
        let c: Vec<u32> = line.split_whitespace().take(8).map(|t| u32::from_str_radix(t, 16).unwrap()).collect();
        frames += 1;
        if (0..4).any(|i| golay_decode(c[i]).1 != 0) {
            golay_err_frames += 1;
        }
        for &w in &c[4..7] {
            let (td, te) = hamming_decode(w as u16);
            let (cd, ce) = hamming_decode_chip(w as u16);
            words += 1;
            textbook_err_words += (te != 0) as usize;
            chip_err_words += (ce != 0) as usize;
            differing_data_words += (td != cd) as usize;
        }
    }
    println!("{frames} frames; frames with any Golay(c0..c3) error: {golay_err_frames}");
    println!("Hamming words: {words}; reported errors -- textbook: {textbook_err_words}, chip-real: {chip_err_words}");
    println!("Hamming words whose recovered data differs between the two decoders: {differing_data_words}");
}
