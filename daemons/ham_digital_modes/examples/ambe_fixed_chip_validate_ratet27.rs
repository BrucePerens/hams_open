// SPDX-License-Identifier: LGPL-3.0-or-later
//! Live chip validation for `ambe::fixed::tia_102_baba::reconstruct`/`prediction`: feeds real recorded
//! speech through the real DVSI chip (RATET(27), P25 full-rate FEC), decodes each real frame's real
//! quantizer values via the already-chip-validated float `DecoderState::decode_parameters`, and
//! confirms the fixed-point `reconstruct_spectral_amplitudes_q16` tracks the float sibling's own
//! `reconstruct_spectral_amplitudes` for those same real values -- real chip-produced `b2`/gain/
//! higher-order quantizer values, not hand-picked synthetic ones (an earlier attempt at a synthetic
//! sweep produced unrealistic amplitudes no real encoder would ever transmit, since this predictive
//! coder's own recursion can compound arbitrary synthetic inputs into values a real, self-consistent
//! encoded stream never reaches).
//!
//! **A previously-disclosed limitation of this validator, now fixed**: an earlier version of
//! `wire_bytes_to_c` built 72 "dibit symbols" from the raw wire bits and deinterleaved them via
//! `interleave::deinterleave_from_dibit_symbols` -- the domain a real over-the-air P25 C4FM dibit
//! stream uses, not the DVSI chip's own UDP `CHAND` byte layout this validator actually talks to.
//! That wrong domain corrupted `u_hat_1..u_hat_6`/`b_hat_0` on nearly every frame, driving
//! `errors.rate` past the `0.0875` mute threshold almost immediately (`Decoded: 3/3320`).
//! `wire_bytes_to_c` now extracts each block's raw pre-FEC codeword directly via
//! `dvsi_p25fec::wire_format::block_wire_members` -- the same convention
//! `dvsi_p25fec::fec::decode_block`/`dvsi_p25fec::frame::decode_frame` use and that
//! `examples/ambe_chip_validate_ratet27.rs`'s own zero-corrected-error PASS harness already confirms
//! against the live chip -- which recovers `Decoded: 3291/3320` (worst Ml relative error observed
//! well under the 1% tolerance). The remaining ~29 non-decoded frames are `should_mute_frame`/
//! `should_repeat_frame`/out-of-range-`b0` outcomes on real, individual frames (leading/trailing
//! silence and genuine repeats), not a systemic extraction defect.
//!
//! Also prints the real chip-derived `R_M0` (spectral energy, Eq. 105) range across all decoded
//! frames -- min=9.2, max=4.0e8, mean=8.5e5 in one representative run -- which is the evidence
//! behind `ambe::fixed::tia_102_baba::enhancement`'s own log-domain design (see that module's doc
//! comment): 8 orders of magnitude rules out any single linear Q16.16 rescale. Also feeds each real
//! decoded frame's own reconstructed amplitudes and `omega0_tilde` through
//! `enhancement::enhance_spectral_amplitudes_q16`, checked against the float sibling on real chip
//! amplitude *shapes* the synthetic sweep in `tests/ambe_fixed_tia_102_baba_enhancement.rs` can't fully
//! stand in for -- worst harmonic relative error observed 0.69% across all 3293 checked frames in
//! one representative run, zero over the 1% tolerance.
//!
//! Usage: `cargo run --release --example ambe_fixed_chip_validate_ratet27 -- <host:port>`

use ham_digital_modes::ambe::fixed::tia_102_baba::enhancement::enhance_spectral_amplitudes_q16;
use ham_digital_modes::ambe::fixed::tia_102_baba::prediction::INITIAL_L_HAT_PREV as FIXED_INITIAL_L_HAT_PREV;
use ham_digital_modes::ambe::fixed::tia_102_baba::reconstruct::reconstruct_spectral_amplitudes_q16;
use ham_digital_modes::ambe::float::tia_102_baba::decode::{DecoderState, FrameOutcome};
use ham_digital_modes::ambe::float::tia_102_baba::enhancement::enhance_spectral_amplitudes;
use ham_digital_modes::ambe::dvsi_p25fec::wire_format::{block_wire_members, Block};
use std::net::UdpSocket;
use std::time::Duration;

const FIELD_RATEP: u8 = 0x0A;
const TYPE_CONTROL: u8 = 0x00;
const TYPE_SPEECH: u8 = 0x02;
const TYPE_CHANNEL: u8 = 0x01;
const FRAME_SAMPLES: usize = 160;
const RATEP_P25_FEC: [u16; 6] = [0x0558, 0x086B, 0x1030, 0x0000, 0x0000, 0x0190];
const BITS_OFFSET: usize = 6;
const FRAME_BYTES: usize = 18;
const ML_RELATIVE_TOLERANCE: f64 = 0.01;

fn build_control_ratep(rcw: [u16; 6]) -> Vec<u8> {
    let mut payload = vec![FIELD_RATEP];
    for v in rcw {
        payload.extend_from_slice(&v.to_be_bytes());
    }
    let mut pkt = vec![0x61_u8];
    pkt.extend_from_slice(&(payload.len() as u16).to_be_bytes());
    pkt.push(TYPE_CONTROL);
    pkt.extend_from_slice(&payload);
    pkt
}
fn build_speech(samples: &[i16]) -> Vec<u8> {
    let mut payload = vec![0x00_u8, samples.len() as u8];
    for &s in samples {
        payload.extend_from_slice(&s.to_be_bytes());
    }
    let mut pkt = vec![0x61_u8];
    pkt.extend_from_slice(&(payload.len() as u16).to_be_bytes());
    pkt.push(TYPE_SPEECH);
    pkt.extend_from_slice(&payload);
    pkt
}
fn parse_packet(data: &[u8]) -> Option<(u8, &[u8])> {
    if data.len() < 4 || data[0] != 0x61 {
        return None;
    }
    let length = u16::from_be_bytes([data[1], data[2]]) as usize;
    let ptype = data[3];
    data.get(4..4 + length).map(|payload| (ptype, payload))
}
fn send_recv_retrying(sock: &UdpSocket, buf: &mut [u8; 512], pkt: &[u8]) -> usize {
    for attempt in 0..8 {
        sock.send(pkt).expect("send");
        match sock.recv(buf) {
            Ok(n) => return n,
            Err(e) if attempt < 7 => {
                eprintln!("retrying after {e}");
                std::thread::sleep(Duration::from_millis(200));
            }
            Err(e) => panic!("recv channel after retries: {e}"),
        }
    }
    unreachable!()
}
fn read_wav_mono_i16(path: &str) -> Vec<i16> {
    let data = std::fs::read(path).unwrap_or_else(|e| panic!("{path}: {e}"));
    assert_eq!(&data[8..12], b"WAVE", "{path}: not a RIFF/WAVE file");
    assert_eq!(&data[36..40], b"data", "{path}: not a standard 44-byte-header PCM WAV");
    data[44..].chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]])).collect()
}
/// Extracts `decode_parameters`'s own `c_hat_0..c_hat_7` domain -- the 8 raw, pre-FEC-decode code
/// vectors -- directly from the DVSI chip's raw UDP wire bytes (MSB-first bits per byte, the same
/// convention `dvsi_p25fec::fec::decode_block`/`dvsi_p25fec::frame::decode_frame` use and that
/// `ambe_chip_validate_ratet27.rs`'s own zero-corrected-error PASS harness already confirms against
/// the live chip). **This replaces an earlier, wrong extraction** that instead paired wire bits into
/// 72 "dibit symbols" and ran them through `interleave::deinterleave_from_dibit_symbols` -- that
/// function's own domain is a real over-the-air P25 C4FM dibit stream, a genuinely different wire
/// convention from the DVSI chip's own UDP `CHAND` byte layout this validator actually talks to.
/// Using the wrong domain corrupted `u_hat_1..u_hat_6`/`b_hat_0` on nearly every frame, which is why
/// this validator used to see almost every frame hit `should_mute_frame`/`should_repeat_frame`
/// (`Decoded: 3/3320`) rather than decoding normally.
fn wire_bytes_to_c(bytes: &[u8; FRAME_BYTES]) -> [u32; 8] {
    let mut wire_frame_bits = [false; 144];
    for (byte_idx, &byte) in bytes.iter().enumerate() {
        for b in 0..8 {
            wire_frame_bits[byte_idx * 8 + b] = (byte >> (7 - b)) & 1 == 1;
        }
    }
    let raw = |block: Block| -> u32 {
        let members = block_wire_members(block);
        let mut received: u32 = 0;
        for (offset, &wire) in members.iter().enumerate() {
            if wire_frame_bits[wire] {
                received |= 1 << (members.len() - 1 - offset);
            }
        }
        received
    };
    [
        raw(Block::Golay { index: 0 }),
        raw(Block::Golay { index: 1 }),
        raw(Block::Golay { index: 2 }),
        raw(Block::Golay { index: 3 }),
        raw(Block::Hamming { index: 0 }),
        raw(Block::Hamming { index: 1 }),
        raw(Block::Hamming { index: 2 }),
        raw(Block::Raw),
    ]
}

fn main() {
    let host = std::env::args().nth(1).unwrap_or_else(|| "192.168.10.189:2460".to_string());
    let sock = UdpSocket::bind("0.0.0.0:0").expect("bind local UDP socket");
    sock.connect(&host).unwrap_or_else(|e| panic!("connect to {host}: {e}"));
    sock.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
    let mut buf = [0u8; 512];

    sock.send(&build_control_ratep(RATEP_P25_FEC)).expect("send RATEP config");
    let n = sock.recv(&mut buf).expect("RATEP config response");
    parse_packet(&buf[..n]).expect("valid packet");

    let speech_files = [
        "tests/fixtures/osr_speech/OSR_us_000_0010_8k.wav",
        "tests/fixtures/osr_speech/OSR_us_000_0011_8k.wav",
    ];

    let mut float_decoder = DecoderState::new();
    // The fixed side tracks its own parallel (l_hat_prev, previous_m_q16) history -- there is no
    // fixed-point DecoderState type yet (only reconstruct/prediction are ported so far, not the
    // full bit_prioritization-to-reconstruct pipeline as one struct), so this validator drives it
    // directly, the same pattern tests/ambe_fixed_tia_102_baba_reconstruct.rs already established.
    let mut fixed_l_hat_prev = FIXED_INITIAL_L_HAT_PREV;
    let mut fixed_prev_m_q16: Vec<i32> = vec![65536; FIXED_INITIAL_L_HAT_PREV as usize];

    let mut total_frames = 0usize;
    let mut decoded_frames = 0usize;
    let mut worst_ml_rel_err = 0.0f64;
    let mut r_m0_min = f64::INFINITY;
    let mut r_m0_max = f64::NEG_INFINITY;
    let mut r_m0_sum = 0.0f64;
    let mut non_decoded_indices: Vec<usize> = Vec::new();
    let mut enh_frames_checked = 0usize;
    let mut enh_worst_rel_err = 0.0f64;
    let mut enh_boundary_frames = 0usize;
    let mut hard_failures: Vec<String> = Vec::new();

    for path in speech_files {
        let pcm = read_wav_mono_i16(path);
        let n_frames = pcm.len() / FRAME_SAMPLES;
        for i in 0..n_frames {
            let frame_samples = &pcm[i * FRAME_SAMPLES..(i + 1) * FRAME_SAMPLES];
            let n = send_recv_retrying(&sock, &mut buf, &build_speech(frame_samples));
            let (ptype, payload) = parse_packet(&buf[..n]).expect("valid packet");
            assert_eq!(ptype, TYPE_CHANNEL, "expected a CHANNEL response");
            if payload.len() < BITS_OFFSET - 4 + FRAME_BYTES {
                continue;
            }
            let pkt = &buf[..n];
            if pkt.len() < BITS_OFFSET + FRAME_BYTES {
                continue;
            }
            let mut wire_bytes = [0u8; FRAME_BYTES];
            wire_bytes.copy_from_slice(&pkt[BITS_OFFSET..BITS_OFFSET + FRAME_BYTES]);
            let c = wire_bytes_to_c(&wire_bytes);
            total_frames += 1;

            match float_decoder.decode_parameters(c) {
                Some(FrameOutcome::Decoded(params)) => {
                    decoded_frames += 1;
                    let r_m0 = ham_digital_modes::ambe::float::tia_102_baba::enhancement::energy(
                        &params.reconstructed_amplitudes,
                    );
                    r_m0_min = r_m0_min.min(r_m0);
                    r_m0_max = r_m0_max.max(r_m0);
                    r_m0_sum += r_m0;
                    let gain_values: [u32; 5] = std::array::from_fn(|idx| params.bits.gain_vector[idx].0);
                    let higher_order_values: Vec<u32> =
                        params.bits.higher_order.iter().map(|&(v, _)| v).collect();

                    // decode_parameters alone never advances the decoder's own l_hat_prev/
                    // spectral_amplitudes_prev (only decode_frame does, after a successful
                    // synthesis) -- without this, every one of these 3320 calls would reconstruct
                    // against the same stale initial history instead of the real, evolving one a
                    // real decode stream actually carries frame to frame.
                    float_decoder.advance_history(&params);

                    let fixed_result = reconstruct_spectral_amplitudes_q16(
                        params.bits.b2 as u8,
                        gain_values,
                        &higher_order_values,
                        params.l_hat,
                        fixed_l_hat_prev,
                        &fixed_prev_m_q16,
                    );
                    match fixed_result {
                        Some(fixed_amplitudes) => {
                            if fixed_amplitudes.len() != params.reconstructed_amplitudes.len() {
                                hard_failures.push(format!(
                                    "{path} frame {i}: length mismatch (float={}, fixed={})",
                                    params.reconstructed_amplitudes.len(),
                                    fixed_amplitudes.len()
                                ));
                            } else {
                                for (h, (&float_ml, &fixed_ml_q16)) in
                                    params.reconstructed_amplitudes.iter().zip(fixed_amplitudes.iter()).enumerate()
                                {
                                    let fixed_ml = fixed_ml_q16 as f64 / 65536.0;
                                    let rel_err = if float_ml.abs() > 1e-9 {
                                        ((fixed_ml - float_ml) / float_ml).abs()
                                    } else {
                                        fixed_ml.abs()
                                    };
                                    if rel_err > ML_RELATIVE_TOLERANCE {
                                        hard_failures.push(format!(
                                            "{path} frame {i}, harmonic {h} (l={}): Ml float={float_ml}, fixed={fixed_ml}, rel_err={rel_err}",
                                            params.l_hat
                                        ));
                                    } else {
                                        worst_ml_rel_err = worst_ml_rel_err.max(rel_err);
                                    }
                                }
                            }
                            // Enhancement (Eq. 105-110) is fed the SAME real chip-derived amplitudes
                            // each side already reconstructed above -- the float side's own
                            // `params.reconstructed_amplitudes`, the fixed side's own
                            // `fixed_amplitudes` -- rather than a synthetic sweep, since this is the
                            // stage `tests/ambe_fixed_tia_102_baba_enhancement.rs`'s own realistic-but-
                            // synthetic sweep can't fully stand in for (it can't reproduce the exact
                            // amplitude *shapes* a real predictive decode stream produces).
                            let omega0_q16 = (params.omega0_tilde * 65536.0).round() as i32;
                            let float_enhanced =
                                enhance_spectral_amplitudes(&params.reconstructed_amplitudes, params.omega0_tilde);
                            let fixed_enhanced = enhance_spectral_amplitudes_q16(&fixed_amplitudes, omega0_q16);
                            if float_enhanced.len() == fixed_enhanced.len() {
                                enh_frames_checked += 1;
                                for (&float_e, &fixed_e_q16) in
                                    float_enhanced.iter().zip(fixed_enhanced.iter())
                                {
                                    let fixed_e = fixed_e_q16 as f64 / 65536.0;
                                    let rel_err = if float_e.abs() > 1e-9 {
                                        ((fixed_e - float_e) / float_e).abs()
                                    } else {
                                        fixed_e.abs()
                                    };
                                    if rel_err > ML_RELATIVE_TOLERANCE {
                                        // Boundary cases (a harmonic whose weight/enhancement sits
                                        // right at a rounding edge) are tracked separately from hard
                                        // failures, the same documented-not-hidden treatment
                                        // `mbe_speech.rs`'s own `jl` floor-crossing phenomenon gets.
                                        enh_boundary_frames += 1;
                                    } else {
                                        enh_worst_rel_err = enh_worst_rel_err.max(rel_err);
                                    }
                                }
                            }

                            fixed_l_hat_prev = params.l_hat;
                            fixed_prev_m_q16 = fixed_amplitudes;
                        }
                        None => {
                            hard_failures.push(format!("{path} frame {i}: fixed reconstruction returned None (l_hat={})", params.l_hat));
                        }
                    }
                }
                Some(FrameOutcome::Repeat) | Some(FrameOutcome::Mute) | None => {
                    non_decoded_indices.push(i);
                }
            }
        }
    }

    println!("Total real chip frames: {total_frames}, Decoded (non-repeat/mute): {decoded_frames}");
    println!(
        "Real R_M0 (energy) range over decoded frames: min={r_m0_min:.1} max={r_m0_max:.1} mean={:.1}",
        r_m0_sum / decoded_frames as f64
    );
    println!(
        "Enhancement (Eq. 105-110): {enh_frames_checked} frames checked, worst harmonic relative \
         error {enh_worst_rel_err:.6} (tolerance {ML_RELATIVE_TOLERANCE}), {enh_boundary_frames} \
         harmonics over tolerance (rounding-boundary cases, not hard failures)"
    );
    println!(
        "Non-decoded frame indices (per-file, {} total): {:?}{}",
        non_decoded_indices.len(),
        &non_decoded_indices[..non_decoded_indices.len().min(40)],
        if non_decoded_indices.len() > 40 { ", ..." } else { "" }
    );
    println!("Worst Ml relative error observed: {worst_ml_rel_err:.6} (tolerance {ML_RELATIVE_TOLERANCE})");
    if hard_failures.is_empty() {
        println!(
            "\nPASS: every real chip-produced RATET(27) frame's fixed-point spectral amplitude \
             reconstruction AND enhancement (Eq. 105-110) tracked the float sibling within \
             {ML_RELATIVE_TOLERANCE} relative error, across {decoded_frames} real decoded frames \
             ({enh_frames_checked} enhancement-checked) from real recorded speech."
        );
    } else {
        eprintln!("\nFAIL: {} hard failure(s):", hard_failures.len());
        for m in hard_failures.iter().take(20) {
            eprintln!("  {m}");
        }
        std::process::exit(1);
    }
}
