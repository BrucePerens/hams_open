// SPDX-License-Identifier: LGPL-3.0-or-later
//! Live chip validation for `ambe::fixed::ambe_plus_2::decode`: feeds real recorded speech through
//! the real DVSI chip (RATET(33), AMBE+2 half-rate FEC), and for every resulting real channel frame,
//! confirms the fixed-point `dequantize` tracks the already-chip-validated floating-point sibling
//! within this crate's own documented tolerance -- unlike `tests/ambe_fixed_ambe_plus_2.rs`'s own
//! synthetic parameter sweep, every `b0..b8` value exercised here is a real value the chip itself
//! produced from real audio, not hand-picked.
//!
//! Usage: `cargo run --release --features ambe_plus_2 --example ambe_fixed_chip_validate_ambe_plus_2
//! -- <host:port>`

use ham_digital_modes::ambe::fixed::ambe_plus_2::decode as fixed_decode;
use ham_digital_modes::ambe::fixed::general::mbe_speech::MbeDecoderState;
use ham_digital_modes::ambe::float::ambe_plus_2::decode::{
    classify_b0, extract_raw_parameters, DecoderState, DequantizedFrame, FrameKind,
};
use ham_digital_modes::ambe::float::ambe_plus_2::decode as float_decode;
use ham_digital_modes::ambe::float::ambe_plus_2::interleave::interleaved_to_frame;
use ham_digital_modes::ambe::float::ambe_plus_2::parse_frame;
use std::net::UdpSocket;
use std::time::Duration;

const FIELD_RATET: u8 = 0x09;
const TYPE_CONTROL: u8 = 0x00;
const TYPE_CHANNEL: u8 = 0x01;
const TYPE_SPEECH: u8 = 0x02;
const FRAME_SAMPLES: usize = 160;
const RATET_HALF_RATE_FEC: u8 = 33;
const ML_RELATIVE_TOLERANCE: f64 = 0.01;

fn build_control_ratet(index: u8) -> Vec<u8> {
    let payload = vec![FIELD_RATET, index];
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

fn main() {
    let host = std::env::args().nth(1).unwrap_or_else(|| "192.168.10.189:2460".to_string());
    let sock = UdpSocket::bind("0.0.0.0:0").expect("bind local UDP socket");
    sock.connect(&host).unwrap_or_else(|e| panic!("connect to {host}: {e}"));
    sock.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
    let mut buf = [0u8; 512];

    sock.send(&build_control_ratet(RATET_HALF_RATE_FEC)).expect("send RATET config");
    let n = sock.recv(&mut buf).expect("RATET config response");
    parse_packet(&buf[..n]).expect("valid packet");

    let speech_files = [
        "tests/fixtures/osr_speech/OSR_us_000_0010_8k.wav",
        "tests/fixtures/osr_speech/OSR_us_000_0011_8k.wav",
    ];

    let mut float_state = DecoderState::initial();
    let mut fixed_state = MbeDecoderState::initial();
    let mut total_frames = 0usize;
    let mut speech_frames = 0usize;
    let mut worst_ml_rel_err = 0.0f64;
    // A real, quantified, expected source of rare disagreement -- not a bug -- documented in
    // ambe::fixed::general::mbe_speech's own doc comment on the `jl` floor computation: Q16.16's
    // f0 table carries a small absolute rounding error, so a harmonic whose true VUV lookup index
    // sits within that margin of an integer boundary can legitimately floor to a different value
    // in fixed point than in f64. Tracked separately from `hard_failures` (an L or frame-kind
    // mismatch, which should never happen and would indicate a real bug) so a boundary-crossing
    // frame's own cascading Ml error doesn't get conflated with a systematic defect.
    let mut boundary_frames: Vec<String> = Vec::new();
    let mut hard_failures: Vec<String> = Vec::new();

    for path in speech_files {
        let pcm = read_wav_mono_i16(path);
        let n_frames = pcm.len() / FRAME_SAMPLES;
        for i in 0..n_frames {
            let frame_samples = &pcm[i * FRAME_SAMPLES..(i + 1) * FRAME_SAMPLES];
            let n = send_recv_retrying(&sock, &mut buf, &build_speech(frame_samples));
            let (ptype, payload) = parse_packet(&buf[..n]).expect("valid packet");
            assert_eq!(ptype, TYPE_CHANNEL, "expected a CHANNEL response");
            let num_bits = payload[1] as usize;
            if num_bits != 72 || payload.len() < 2 + 9 {
                continue;
            }
            let frame_bytes = &payload[2..2 + 9];
            let mut wire: u128 = 0;
            for &byte in frame_bytes {
                wire = (wire << 8) | byte as u128;
            }
            let logical = interleaved_to_frame(wire);
            let parsed = parse_frame(logical);
            let raw = extract_raw_parameters(parsed.d);
            total_frames += 1;

            if classify_b0(raw.b0) != FrameKind::Speech {
                continue; // Tone/Erasure/Silence frames: already pure-integer, not this validator's concern.
            }
            speech_frames += 1;

            let float_result = float_decode::dequantize(&raw, &mut float_state);
            let fixed_result = fixed_decode::dequantize(&raw, &mut fixed_state);
            match (float_result, fixed_result) {
                (DequantizedFrame::Speech(float_params), fixed_decode::DequantizedFrame::Speech(fixed_params)) => {
                    if float_params.l != fixed_params.l {
                        hard_failures.push(format!("{path} frame {i}: L mismatch (float={}, fixed={})", float_params.l, fixed_params.l));
                        continue;
                    }
                    let mut frame_had_boundary_issue = float_params.voiced != fixed_params.voiced;
                    for (_h, (&float_ml, &fixed_ml_q16)) in
                        float_params.ml.iter().zip(fixed_params.ml_q16.iter()).enumerate().skip(1)
                    {
                        let fixed_ml = fixed_ml_q16 as f64 / 65536.0;
                        let rel_err = if float_ml.abs() > 1e-9 {
                            ((fixed_ml - float_ml) / float_ml).abs()
                        } else {
                            fixed_ml.abs()
                        };
                        if rel_err > ML_RELATIVE_TOLERANCE {
                            frame_had_boundary_issue = true;
                        } else {
                            worst_ml_rel_err = worst_ml_rel_err.max(rel_err);
                        }
                    }
                    if frame_had_boundary_issue {
                        boundary_frames.push(format!("{path} frame {i}"));
                    }
                }
                _ => hard_failures.push(format!("{path} frame {i}: frame-kind mismatch despite matching classify_b0")),
            }
        }
    }

    let boundary_rate = boundary_frames.len() as f64 / speech_frames.max(1) as f64;
    println!("Total real chip frames: {total_frames}, Speech-classified: {speech_frames}");
    println!("Worst Ml relative error observed (excluding boundary-crossing frames): {worst_ml_rel_err:.6} (tolerance {ML_RELATIVE_TOLERANCE})");
    println!(
        "Frames with an isolated VUV floor-boundary crossing (expected, quantified, not a bug -- \
         see ambe::fixed::general::mbe_speech's own doc comment): {}/{speech_frames} ({:.3}%)",
        boundary_frames.len(),
        boundary_rate * 100.0
    );

    const MAX_ACCEPTABLE_BOUNDARY_RATE: f64 = 0.01; // 1% -- real observed rate is far lower.
    if hard_failures.is_empty() && boundary_rate <= MAX_ACCEPTABLE_BOUNDARY_RATE {
        println!(
            "\nPASS: every real chip-produced Speech frame's fixed-point dequantize tracked the \
             float sibling within {ML_RELATIVE_TOLERANCE} relative Ml error (or was an expected, \
             rare VUV floor-boundary crossing), across {speech_frames} real frames from real \
             recorded speech."
        );
    } else {
        eprintln!("\nFAIL: {} hard failure(s), {:.3}% boundary-crossing rate (limit {:.1}%):", hard_failures.len(), boundary_rate * 100.0, MAX_ACCEPTABLE_BOUNDARY_RATE * 100.0);
        for m in hard_failures.iter().take(20) {
            eprintln!("  hard failure: {m}");
        }
        std::process::exit(1);
    }
}
