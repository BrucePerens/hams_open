// SPDX-License-Identifier: LGPL-3.0-or-later
//! Quick check: decode a real D-STAR-mode frame captured from the live chip (via AMBEServer3003,
//! RATEP configured to the confirmed D-STAR rate-control-word) through this crate's own
//! `ambe_dstar::decode`, and print the recovered parameters for a sanity look.

use ham_digital_modes::ambe_dstar::decode::{
    dequantize, extract_raw_parameters, parse_frame, DStarDecoderState,
};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let hex = args
        .get(1)
        .map(|s| s.as_str())
        .unwrap_or("2741847b437fbdaeac");
    let bytes: Vec<u8> = (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
        .collect();
    assert_eq!(bytes.len(), 9, "D-STAR frame must be 9 bytes");

    let mut frame: u128 = 0;
    for &b in &bytes {
        frame = (frame << 8) | b as u128;
    }

    let parsed = parse_frame(frame);
    println!(
        "epsilon_c0={} epsilon_c1={} d={:049b}",
        parsed.epsilon_c0, parsed.epsilon_c1, parsed.d
    );

    let raw = extract_raw_parameters(parsed.d);
    println!(
        "b0={} b1={} b2={} b3={} b4={} b5={} b6={} b7={} b8={}",
        raw.b0, raw.b1, raw.b2, raw.b3, raw.b4, raw.b5, raw.b6, raw.b7, raw.b8
    );

    let mut state = DStarDecoderState::initial();
    let params = dequantize(&raw, &mut state);
    println!(
        "l={} w0={:.5} (pitch period {:.1} samples, f0={:.1}Hz)",
        params.l,
        params.w0,
        2.0 * std::f64::consts::PI / params.w0,
        params.w0 / (2.0 * std::f64::consts::PI) * 8000.0
    );
    println!("voiced={:?}", &params.voiced[1..]);
    println!("ml={:?}", &params.ml[1..]);
}
