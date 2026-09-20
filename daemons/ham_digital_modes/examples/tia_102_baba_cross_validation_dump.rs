// SPDX-License-Identifier: LGPL-3.0-or-later
//! Cross-validation helper for the TIA-102.BABA (P25 Phase 1 IMBE) float codec against an external decoder
//! (mbelib, ISC licence; see `docs/references/tia_102_baba_cross_validation.md`).
//!
//! Generates wire-conformant test frames (the crate's own encoder and `encode_code_vectors` already produce the
//! standard's wire layer) and dumps this crate's own decoded parameters in the same line format
//! as the scratch C driver that wraps mbelib, so the two dumps can be diffed line by line:
//!
//! ```text
//! cargo run --release --example tia_102_baba_cross_validation_dump -- speech <wav> <nframes> <out_prefix>
//! cargo run --release --example tia_102_baba_cross_validation_dump -- sweep <out_prefix>
//! ```
//!
//! Outputs `<out_prefix>.frames` (one line per frame: eight hex words `c0..c7`, TIA-conformant: textbook Hamming
//! labelling and Eq. 84-94 modulation, i.e. what a P25 receiver sees after de-interleaving; the line `RESET`
//! means "decoder state back to initial"), `<out_prefix>.ours` (our decoded parameters) and, in `speech` mode,
//! `<out_prefix>.ours.f32` (our decoded PCM, little-endian f32).

use ham_digital_modes::ambe::float::tia_102_baba::bit_prioritization::prioritize_bits;
use ham_digital_modes::ambe::float::tia_102_baba::decode::{DecoderState, FrameOutcome};
use ham_digital_modes::ambe::float::tia_102_baba::encoder::Encoder;
use ham_digital_modes::ambe::float::tia_102_baba::enhancement::enhance_spectral_amplitudes;
use ham_digital_modes::ambe::float::tia_102_baba::tables::{gain_bit_allocation, higher_order_bit_allocation};
use ham_digital_modes::ambe::float::tia_102_baba::vuv::{frequency_bands_count, harmonics_count};
use ham_digital_modes::ambe::float::tia_102_baba::{encode_code_vectors, parameter_encoding};
use std::fmt::Write as _;

fn fmt_frame(c: &[u32; 8]) -> String {
    format!("{:x} {:x} {:x} {:x} {:x} {:x} {:x} {:x}\n", c[0], c[1], c[2], c[3], c[4], c[5], c[6], c[7])
}

/// Dumps our decoded parameters for one frame in the scratch driver's line format.
fn dump_ours(out: &mut String, idx: usize, dec: &mut DecoderState, chip_c: [u32; 8]) {
    match dec.decode_parameters(chip_c) {
        Some(FrameOutcome::Decoded(p)) => {
            let b = &p.bits;
            let _ = write!(out, "F {idx} errs {} flags [] b0 {} b1 {:x} b2 {}", p.errors.total, b.b0, b.b1, b.b2);
            let _ = writeln!(out, " w0 {:.9} L {} K {}", p.omega0_tilde, p.l_hat, p.k_hat);
            out.push_str("V ");
            for &v in &p.voiced {
                out.push(if v { '1' } else { '0' });
            }
            out.push_str("\nLOG2M");
            for &m in &p.reconstructed_amplitudes {
                let _ = write!(out, " {:.7}", m.log2());
            }
            out.push_str("\nM");
            for &m in &p.reconstructed_amplitudes {
                let _ = write!(out, " {:.7}", m);
            }
            out.push_str("\nMENH");
            for &m in &enhance_spectral_amplitudes(&p.reconstructed_amplitudes, p.omega0_tilde) {
                let _ = write!(out, " {:.7}", m);
            }
            out.push('\n');
            dec.advance_history(&p);
        }
        Some(FrameOutcome::Repeat) => {
            let _ = writeln!(out, "F {idx} REPEAT");
        }
        Some(FrameOutcome::Mute) => {
            let _ = writeln!(out, "F {idx} MUTE");
        }
        None => {
            let _ = writeln!(out, "F {idx} NONE");
        }
    }
}

fn read_wav_mono_i16(path: &str) -> Vec<i16> {
    let data = std::fs::read(path).unwrap_or_else(|e| panic!("{path}: {e}"));
    assert_eq!(&data[8..12], b"WAVE", "{path}: not a RIFF/WAVE file");
    assert_eq!(&data[36..40], b"data", "{path}: not a standard 44-byte-header PCM WAV");
    data[44..].chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]])).collect()
}

fn speech(wav: &str, n_frames: usize, prefix: &str, noisy: bool) {
    let pcm = read_wav_mono_i16(wav);
    let mut enc = Encoder::new();
    let samples: Vec<f64> = pcm.iter().map(|&s| s as f64).collect();
    enc.push_samples(&samples);
    let mut frames = Vec::new();
    while let Some(f) = enc.next_frame() {
        frames.push(f);
    }
    frames.extend(enc.finish());
    frames.truncate(n_frames);

    let mut frames_txt = String::new();
    let mut ours_txt = String::new();
    let mut params_dec = DecoderState::new();
    let mut pcm_dec = DecoderState::new();
    let mut pcm_out: Vec<u8> = Vec::new();
    let mut noise_rng = Rng(0x1234_5678_9abc_def1);
    for (i, c) in frames.iter().enumerate() {
        let mut tia = *c;
        if noisy {
            // Flip up to the correction capacity in each block (Golay 3, Hamming 1), at most 5 flips in the whole
            // frame so an external decoder's own "too many errors, repeat" rule does not trigger.
            let widths = [23u32, 23, 23, 23, 15, 15, 15];
            let hamming_cap = if std::env::var("XVAL_NO_HAMMING_ERRORS").is_ok() { 0 } else { 1 };
            let caps = [3usize, 3, 3, 3, hamming_cap, hamming_cap, hamming_cap];
            let mut budget = 5usize;
            for (blk, (&w, &cap)) in widths.iter().zip(caps.iter()).enumerate() {
                let n = ((noise_rng.next() % (cap as u64 + 1)) as usize).min(budget);
                budget -= n;
                let mut flipped = 0u32;
                while flipped.count_ones() < n as u32 {
                    flipped |= 1 << (noise_rng.next() % w as u64);
                }
                tia[blk] ^= flipped;
            }
        }
        frames_txt.push_str(&fmt_frame(&tia));
        dump_ours(&mut ours_txt, i, &mut params_dec, tia);
        let out = pcm_dec.decode_frame(tia).unwrap_or([0.0; 160]);
        for s in out {
            pcm_out.extend_from_slice(&(s as f32).to_le_bytes());
        }
    }
    std::fs::write(format!("{prefix}.frames"), frames_txt).unwrap();
    std::fs::write(format!("{prefix}.ours"), ours_txt).unwrap();
    std::fs::write(format!("{prefix}.ours.f32"), pcm_out).unwrap();
    eprintln!("{wav}: {} frames (encoder failed_frames = {})", frames.len(), enc.failed_frames);
}

/// Deterministic xorshift for reproducible sweeps.
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
}

/// Builds a `u` vector set from raw quantizer indices, respecting Annex F/G widths for `l_hat`.
fn frame_from_indices(b0: u32, b1: u32, b2: u32, gain: [u32; 5], hoc: &[u32], sync: bool) -> Option<[u32; 8]> {
    let omega0 = parameter_encoding::dequantize_fundamental_frequency(b0);
    let l_hat = harmonics_count(omega0);
    let k_hat = frequency_bands_count(l_hat);
    let gw: [u8; 5] = std::array::from_fn(|i| gain_bit_allocation(l_hat, i as u32 + 2).unwrap().0);
    let hw: Vec<u8> = higher_order_bit_allocation(l_hat)?.iter().copied().filter(|&w| w > 0).collect();
    let gain_vector: [(u32, u8); 5] = std::array::from_fn(|i| (gain[i] & ((1u32 << gw[i]) - 1), gw[i]));
    let hoc_vec: Vec<(u32, u8)> = hw
        .iter()
        .enumerate()
        .map(|(i, &w)| (hoc.get(i).copied().unwrap_or(0) & ((1u32 << w) - 1), w))
        .collect();
    prioritize_bits(b0, b1 & ((1u32 << k_hat) - 1), k_hat, b2, gain_vector, &hoc_vec, sync)
}

fn sweep(prefix: &str) {
    let mut frames_txt = String::new();
    let mut ours_txt = String::new();
    let mut idx = 0usize;
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    let mut emit = |u: [u32; 8], reset: bool, dec: &mut DecoderState| {
        if reset {
            frames_txt.push_str("RESET\n");
            *dec = DecoderState::new();
        }
        let tia = encode_code_vectors(u);
        frames_txt.push_str(&fmt_frame(&tia));
        dump_ours(&mut ours_txt, idx, dec, tia);
        idx += 1;
    };
    let mut dec = DecoderState::new();

    // 1. Every b0 across its valid range 0..=207: extreme and random field values, each from a fresh state.
    for b0 in 0u32..=207 {
        for variant in 0..4u32 {
            let (b1, b2, gain, hoc): (u32, u32, [u32; 5], Vec<u32>) = match variant {
                0 => (0, 0, [0; 5], vec![0; 60]),
                1 => (0xfff, 63, [0x3ff; 5], vec![0x3ff; 60]),
                _ => (
                    rng.next() as u32,
                    (rng.next() % 64) as u32,
                    std::array::from_fn(|_| rng.next() as u32),
                    (0..60).map(|_| rng.next() as u32).collect(),
                ),
            };
            if let Some(u) = frame_from_indices(b0, b1, b2, gain, &hoc, variant & 1 == 1) {
                emit(u, true, &mut dec);
            }
        }
    }

    // 2. Exhaustive per-field sweeps at four harmonic counts (b0 chosen so Eq. 47 gives the L): every index of
    //    b2, each gain element and each higher-order coefficient, everything else at a mid value.
    for target_l in [9u32, 20, 37, 56] {
        let b0 = (0u32..=207)
            .find(|&b| harmonics_count(parameter_encoding::dequantize_fundamental_frequency(b)) == target_l)
            .expect("an L this small/large must exist");
        let omega0 = parameter_encoding::dequantize_fundamental_frequency(b0);
        let l_hat = harmonics_count(omega0);
        let gw: [u8; 5] = std::array::from_fn(|i| gain_bit_allocation(l_hat, i as u32 + 2).unwrap().0);
        let hw: Vec<u8> = higher_order_bit_allocation(l_hat).unwrap().iter().copied().filter(|&w| w > 0).collect();
        let mid_gain: [u32; 5] = std::array::from_fn(|i| 1u32 << (gw[i] - 1));
        let mid_hoc: Vec<u32> = hw.iter().map(|&w| 1u32 << (w - 1)).collect();
        for b2 in 0u32..64 {
            if let Some(u) = frame_from_indices(b0, 0x555, b2, mid_gain, &mid_hoc, false) {
                emit(u, true, &mut dec);
            }
        }
        for g in 0..5 {
            for v in 0u32..(1 << gw[g]) {
                let mut gain = mid_gain;
                gain[g] = v;
                if let Some(u) = frame_from_indices(b0, 0x2aa, 30, gain, &mid_hoc, false) {
                    emit(u, true, &mut dec);
                }
            }
        }
        for (k, &w) in hw.iter().enumerate() {
            for v in 0u32..(1 << w) {
                let mut hoc = mid_hoc.clone();
                hoc[k] = v;
                if let Some(u) = frame_from_indices(b0, 0x2aa, 30, mid_gain, &hoc, false) {
                    emit(u, true, &mut dec);
                }
            }
        }
    }

    // 3. A long random stream with no resets: exercises Eq. 75-79's prediction across every L transition.
    emit([0; 8], true, &mut dec);
    let mut b0 = 60u32;
    for i in 0..3000u32 {
        // Random walk in b0 with occasional jumps so L changes both ways.
        if rng.next().is_multiple_of(4) {
            b0 = (rng.next() % 208) as u32;
        } else {
            b0 = (b0 as i64 + (rng.next() % 9) as i64 - 4).clamp(0, 207) as u32;
        }
        let gain: [u32; 5] = std::array::from_fn(|_| rng.next() as u32);
        let hoc: Vec<u32> = (0..60).map(|_| rng.next() as u32).collect();
        // Gain quantizer indices concentrated near the middle so amplitudes stay in a sane range.
        let b2 = 20 + (rng.next() % 16) as u32;
        if let Some(u) = frame_from_indices(b0, rng.next() as u32, b2, gain, &hoc, i & 1 == 1) {
            emit(u, false, &mut dec);
        }
    }

    std::fs::write(format!("{prefix}.frames"), frames_txt).unwrap();
    std::fs::write(format!("{prefix}.ours"), ours_txt).unwrap();
    eprintln!("sweep: {idx} frames");
}

/// Prints this crate's quantizer tables in a fixed text form for diffing against an external decoder's tables.
fn tables() {
    use ham_digital_modes::ambe::float::tia_102_baba::tables::{
        block_lengths_for_l, higher_order_coefficient_sigma, higher_order_step_multiplier, GAIN_QUANTIZER_LEVELS,
    };
    println!("B2 {}", GAIN_QUANTIZER_LEVELS.iter().map(|v| format!("{v:.6}")).collect::<Vec<_>>().join(" "));
    println!(
        "QUANTSTEP {}",
        (1u8..=10).map(|b| format!("{:.4}", higher_order_step_multiplier(b).unwrap())).collect::<Vec<_>>().join(" ")
    );
    println!(
        "STANDDEV {}",
        (2u32..=10).map(|k| format!("{:.4}", higher_order_coefficient_sigma(k).unwrap())).collect::<Vec<_>>().join(" ")
    );
    for l in 9u32..=56 {
        let ba: Vec<String> =
            (2..=6).map(|m| { let (w, s) = gain_bit_allocation(l, m).unwrap(); format!("{w}:{s:.6}") }).collect();
        println!("BA {l} {}", ba.join(" "));
        let ho: Vec<String> = higher_order_bit_allocation(l).unwrap().iter().map(|w| w.to_string()).collect();
        println!("HOBA {l} {}", ho.join(" "));
        let bl: Vec<String> = block_lengths_for_l(l).unwrap().iter().map(|w| w.to_string()).collect();
        println!("JI {l} {}", bl.join(" "));
    }
}

/// The small golden-vector set behind `tests/ambe_tia_102_baba_cross_validation.rs`: three chains of frames built
/// from raw quantizer indices (so nothing here depends on the analysis half of the encoder), written as wire
/// frames with a `RESET` line between chains. Feed them to an external decoder and paste its output into the test.
fn golden(prefix: &str) {
    let frames_txt = std::cell::RefCell::new(String::new());
    let emit = |b0: u32, b1: u32, b2: u32, gain: [u32; 5], hoc: &[u32], sync: bool| {
        let u = frame_from_indices(b0, b1, b2, gain, hoc, sync).expect("valid indices");
        frames_txt.borrow_mut().push_str(&fmt_frame(&encode_code_vectors(u)));
    };
    // Chain 1: L changes 54 -> 9 -> 23 -> 21, exercising Eq. 75-79's prediction across harmonic-count changes
    // (L = 23 is also the Annex F row an external decoder's table gets wrong).
    emit(199, 0x800, 37, [21, 17, 5, 9, 3], &[300, 100, 60, 30, 7, 3, 2, 1, 1, 0, 1, 1, 0, 1, 0, 1, 1, 0, 1, 1, 1, 0, 1, 1, 0, 0, 1, 1, 1, 0, 1, 0, 1, 1, 0, 1, 0, 1, 1, 0, 1, 1, 0, 1, 0, 1, 0, 1], false);
    emit(1, 0x005, 28, [400, 250, 200, 100, 120], &[380, 200, 90], true);
    emit(61, 0x0a5, 29, [12, 9, 7, 6, 5], &[7, 3, 5, 2, 9, 1, 4, 1, 3, 2, 1, 0, 1, 1, 0, 1, 1, 0, 1, 1, 0, 1], false);
    emit(55, 0x3c, 28, [15, 11, 4, 5, 6], &[2, 3, 1, 4, 2, 3, 1, 2, 1, 3, 1, 1, 0, 1, 1, 0, 1, 1, 0, 1], true);
    frames_txt.borrow_mut().push_str("RESET\n");
    // Chain 2: the largest harmonic count (L = 56) from the initial state, saturated and extreme indices.
    emit(207, 0x001, 63, [31, 0, 7, 0, 3], &[1023, 0, 511, 255, 127, 63, 31, 15, 7, 3, 1, 0, 1, 2, 3, 1, 0, 1, 2, 1, 2, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1], false);
    frames_txt.borrow_mut().push_str("RESET\n");
    // Chain 3: L = 40 (above the 36-harmonic band boundary of Eq. 50) with a mixed voicing pattern.
    emit(120, 0x2a5, 30, [9, 7, 6, 3, 2], &[4, 2, 3, 1, 2, 2, 1, 1, 2, 1, 0, 1, 1, 0, 1, 1, 0, 1, 1, 0, 1, 1, 0, 1, 1, 0, 1, 1, 0, 1, 1, 0, 1, 1], true);
    std::fs::write(format!("{prefix}.frames"), frames_txt.into_inner()).unwrap();
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("golden") => golden(&args[2]),
        Some("speech") => speech(&args[2], args[3].parse().unwrap(), &args[4], false),
        Some("noisy") => speech(&args[2], args[3].parse().unwrap(), &args[4], true),
        Some("sweep") => sweep(&args[2]),
        Some("tables") => tables(),
        _ => eprintln!("usage: speech <wav> <nframes> <out_prefix> | sweep <out_prefix>"),
    }
}
