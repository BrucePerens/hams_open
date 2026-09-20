// SPDX-License-Identifier: LGPL-3.0-or-later
//! Decoder cross-validation helper: reads the `U <u0..u7 hex>` lines that `tia_102_baba_oracle_encode_dump` (or the
//! scratch driver around the OP25 `imbe_vocoder` encoder) prints, applies this crate's standard wire layer (FEC and
//! modulation, so the frames pass through the same code path a real receiver uses), decodes them with this crate's
//! decoder, and writes the decoded parameters (`LOG2M` lines, the unenhanced dequantized log2 amplitudes) and the PCM
//! (little-endian 16-bit, clipped) so external-encoder frames can be judged by our decoder and vice versa.
//!
//! Usage: `cargo run --release --example tia_102_baba_oracle_decode_dump -- <dump.txt> <out.params> <out.raw16>`

use ham_digital_modes::ambe::float::tia_102_baba::decode::{DecoderState, FrameOutcome};
use ham_digital_modes::ambe::float::tia_102_baba::enhancement::enhance_spectral_amplitudes;
use ham_digital_modes::ambe::float::tia_102_baba::encode_code_vectors;
use std::fmt::Write as _;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let text = std::fs::read_to_string(&args[1]).expect("input");
    let mut params_state = DecoderState::new();
    let mut pcm_state = DecoderState::new();
    let mut params = String::new();
    let mut pcm: Vec<u8> = Vec::new();
    let mut k = 0;
    for line in text.lines() {
        if line.starts_with("RESET") {
            params_state = DecoderState::new();
            pcm_state = DecoderState::new();
            continue;
        }
        let Some(rest) = line.strip_prefix("U ") else { continue };
        let v: Vec<u32> = rest.split_whitespace().map(|x| u32::from_str_radix(x, 16).unwrap()).collect();
        let c = encode_code_vectors(std::array::from_fn(|i| v[i]));
        match params_state.decode_parameters(c) {
            Some(FrameOutcome::Decoded(p)) => {
                let _ = write!(params, "F {k} b0 {} L {} LOG2M", p.bits.b0, p.l_hat);
                for &m in &p.reconstructed_amplitudes {
                    let _ = write!(params, " {:.5}", m.log2());
                }
                params.push('\n');
                let _ = write!(params, "F {k} MENH");
                for &m in &enhance_spectral_amplitudes(&p.reconstructed_amplitudes, p.omega0_tilde) {
                    let _ = write!(params, " {m:.5}");
                }
                params.push('\n');
                let _ = write!(params, "F {k} V ");
                for &v in &p.voiced {
                    params.push(if v { '1' } else { '0' });
                }
                params.push('\n');
                params_state.advance_history(&p);
            }
            _ => {
                let _ = writeln!(params, "F {k} NOT_DECODED");
            }
        }
        let out = pcm_state.decode_frame(c).unwrap_or([0.0; 160]);
        for s in out {
            pcm.extend_from_slice(&(s.round().clamp(-32768.0, 32767.0) as i16).to_le_bytes());
        }
        k += 1;
    }
    std::fs::write(&args[2], params).unwrap();
    std::fs::write(&args[3], pcm).unwrap();
}
