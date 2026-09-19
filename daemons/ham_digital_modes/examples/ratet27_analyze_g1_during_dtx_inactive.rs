// SPDX-License-Identifier: LGPL-3.0-or-later
//! Section 35 states `VOICE_ACTIVE` "isn't recoverable from the ordinary 144 wire bits without"
//! `PKT_CHANFMT`'s `ECMODE_OUT` field. Checks that claim directly against `g1` (zero extra chip
//! time, using the already-committed `dtx_silence_sweep.tsv`): `g1` is bimodal in every other
//! dataset in this project (a "low" cluster around 1200-1360, a "high" cluster around 3370-3420,
//! e.g. section 36's own `ECMODE_IN=0` baseline). If confirmed-inactive frames never produce a
//! "high" `g1` value while confirmed-active frames do, `(g0, g1)` jointly might discriminate active
//! vs. inactive somewhat better than `g0` alone, even without `ECMODE_OUT`.
//!
//! Usage: `cargo run --release --example ratet27_analyze_g1_during_dtx_inactive`
use ham_digital_modes::ambe::ratet27_fec::decode_block;
use ham_digital_modes::ambe::ratet27_wire_format::Block;
use std::collections::BTreeMap;

const G1_HIGH_CLUSTER_FLOOR: u16 = 3300;

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
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/docs/references/ratet27_captures/dtx_silence_sweep.tsv"
    );
    let text = std::fs::read_to_string(path).expect("read dtx_silence_sweep.tsv");
    let mut by_label: BTreeMap<String, Vec<(u16, u16)>> = BTreeMap::new();
    for line in text.lines() {
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() != 3 {
            continue;
        }
        let wire_bits = hex_to_wire_bits(parts[2]);
        let (g0, _) = decode_block(&wire_bits, Block::Golay { index: 0 });
        let (g1, _) = decode_block(&wire_bits, Block::Golay { index: 1 });
        by_label.entry(parts[0].to_string()).or_default().push((g0, g1));
    }

    // Labels are named by this project's own capture convention: "dtx{on,off}_{silence,tone,noise1,
    // noise2,lowlevelnoise}" -- "silence"/"lowlevelnoise" are the confirmed-inactive content here;
    // "tone" is confirmed-active; "noise1"/"noise2" vary in actual level (not separately confirmed
    // active/inactive by content name alone, reported for context only).
    println!("{:22}  {:>8}  g1_reaches_high_cluster(>= {G1_HIGH_CLUSTER_FLOOR})", "label", "n");
    for (label, vals) in &by_label {
        let reaches_high = vals.iter().any(|&(_, g1)| g1 >= G1_HIGH_CLUSTER_FLOOR);
        println!("{label:22}  {:8}  {reaches_high}", vals.len());
    }

    let inactive_labels = ["dtxon_silence", "dtxon_lowlevelnoise"];
    let active_labels = ["dtxon_tone", "dtxoff_tone"];
    let inactive_ever_high = inactive_labels.iter().any(|l| {
        by_label.get(*l).is_some_and(|vals| vals.iter().any(|&(_, g1)| g1 >= G1_HIGH_CLUSTER_FLOOR))
    });
    let active_ever_high = active_labels.iter().any(|l| {
        by_label.get(*l).is_some_and(|vals| vals.iter().any(|&(_, g1)| g1 >= G1_HIGH_CLUSTER_FLOOR))
    });
    println!(
        "\nConfirmed-inactive labels ever reach the g1 high cluster: {inactive_ever_high}\nConfirmed-active labels ever reach the g1 high cluster: {active_ever_high}"
    );
}
