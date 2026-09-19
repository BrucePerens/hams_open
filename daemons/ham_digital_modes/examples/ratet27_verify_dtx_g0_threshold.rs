// SPDX-License-Identifier: LGPL-3.0-or-later
//! Verifies a candidate broader DTX classifier (`g0 >= DTX_SILENCE_G0`, i.e. `g0 >= 3841`) against
//! every committed RATET(27) capture dataset in `docs/references/ratet27_captures/`, before adding
//! it to `ambe::ratet27_dtx` as `is_dtx_inactive_frame`. Section 35 found `g0` reads exactly `3841`
//! for confirmed digital silence and `3844`-`3857` for confirmed-inactive background noise -- both
//! well above every voiced/active `g0` value recorded anywhere in this project's own datasets (which
//! cluster below 2400). This tool checks that claim directly and exhaustively rather than by
//! spot-check: for every dataset NOT specifically about DTX/silence/noise-floor content (i.e. every
//! dataset built from tones, sweeps, DTMF, or real speech -- content that should read as active), it
//! decodes `g0` via the crate's own real `decode_block` and reports the maximum value seen. If any
//! such "should be active" dataset ever produces `g0 >= 3841`, that's a counterexample the threshold
//! needs to account for before shipping.
//!
//! Usage: `cargo run --release --example ratet27_verify_dtx_g0_threshold`
use ham_digital_modes::ambe::ratet27_fec::decode_block;
use ham_digital_modes::ambe::ratet27_wire_format::Block;

const DTX_SILENCE_G0: u16 = 3841;

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

/// Decodes every `label\tidx\thex` row's `g0` from a capture file, ignoring any non-data lines
/// (e.g. the config-response echo lines some older captures include).
fn g0_values_from_label_hex_tsv(path: &str) -> Vec<u16> {
    let text = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("read {path}: {e}"));
    text.lines()
        .filter_map(|line| {
            let parts: Vec<&str> = line.split('\t').collect();
            if parts.len() != 3 || parts[2].len() != 36 {
                return None;
            }
            let wire_bits = hex_to_wire_bits(parts[2]);
            let (g0, _distance) = decode_block(&wire_bits, Block::Golay { index: 0 });
            Some(g0)
        })
        .collect()
}

/// Decodes `chip_g0` directly from the real-speech correlation dataset's own already-decoded column.
fn g0_values_from_speech_correlation_tsv(path: &str) -> Vec<u16> {
    let text = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("read {path}: {e}"));
    let mut lines = text.lines();
    let header: Vec<&str> = lines.next().expect("header").split('\t').collect();
    let g0_col = header.iter().position(|&h| h == "chip_g0").expect("chip_g0 column");
    lines
        .filter_map(|line| {
            let parts: Vec<&str> = line.split('\t').collect();
            parts.get(g0_col)?.parse().ok()
        })
        .collect()
}

fn main() {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/docs/references/ratet27_captures");

    // Datasets built from tone/sweep/DTMF/speech content -- expected to read as ACTIVE throughout,
    // so none of these should ever produce g0 >= DTX_SILENCE_G0.
    let active_datasets = [
        "all_stimuli_2687frames.tsv",
        "amplitude_sweep_200hz.tsv",
        "dense_pitch_sweep_57to444hz.tsv",
        "g0_long_settling_amplitude_sweep.tsv",
        "lhat_boundary_sweep.tsv",
        "lhat_controlled_test.tsv",
        "real_dtmf_sweep.tsv",
        "rms_normalized_pitch_sweep_57to444hz.tsv",
        "tone_detect_disabled_sweep.tsv",
        "tone_send_forced_sweep.tsv",
        "u4_long_settling_pitch_sweep.tsv",
        "u6_600hz_long_settling.tsv",
        "u6_converged_range_sweep.tsv",
    ];

    println!("-- Datasets expected to be ACTIVE throughout (checking none reach g0 >= {DTX_SILENCE_G0}) --");
    let mut any_counterexample = false;
    for name in active_datasets {
        let path = format!("{dir}/{name}");
        let values = g0_values_from_label_hex_tsv(&path);
        let max = values.iter().copied().max().unwrap_or(0);
        let violations: Vec<u16> = values.iter().copied().filter(|&v| v >= DTX_SILENCE_G0).collect();
        if !violations.is_empty() {
            any_counterexample = true;
        }
        println!(
            "{name:45}  n={:5}  max_g0={max:5}  violations={}",
            values.len(),
            violations.len()
        );
    }

    let speech_path = format!("{dir}/u_vector_speech_correlation_600frames.tsv");
    let speech_values = g0_values_from_speech_correlation_tsv(&speech_path);
    let speech_max = speech_values.iter().copied().max().unwrap_or(0);
    let speech_violations: Vec<u16> =
        speech_values.iter().copied().filter(|&v| v >= DTX_SILENCE_G0).collect();
    if !speech_violations.is_empty() {
        any_counterexample = true;
    }
    println!(
        "{:45}  n={:5}  max_g0={speech_max:5}  violations={}",
        "u_vector_speech_correlation_600frames.tsv",
        speech_values.len(),
        speech_violations.len()
    );

    // Datasets specifically about DTX/silence/noise-floor content -- expected to show g0 >=
    // DTX_SILENCE_G0 for their inactive portions; reported for context, not as counterexamples.
    println!("\n-- DTX/silence/noise-floor datasets (context only, not checked as counterexamples) --");
    for name in ["dtx_silence_sweep.tsv", "dtx_noise_levels_sweep.tsv"] {
        let path = format!("{dir}/{name}");
        let values = g0_values_from_label_hex_tsv(&path);
        let max = values.iter().copied().max().unwrap_or(0);
        let min = values.iter().copied().min().unwrap_or(0);
        println!("{name:45}  n={:5}  min_g0={min:5}  max_g0={max:5}", values.len());
    }

    println!(
        "\n{}",
        if any_counterexample {
            "COUNTEREXAMPLE FOUND: at least one active-content dataset reached g0 >= 3841 -- threshold needs revision."
        } else {
            "No counterexamples: every active-content dataset stayed below g0 = 3841. The >= 3841 threshold holds across every committed dataset."
        }
    );
}
