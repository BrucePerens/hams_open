// SPDX-License-Identifier: LGPL-3.0-or-later
//! Follow-up to `ratet27_compare_textbook_u_vectors_to_chip.rs`'s negative single-frequency result:
//! that test used an *assumed exact* pitch and found no correspondence, but per
//! `examples/ambe_chip_validate_dstar.rs`'s own doc comment, this chip's encoder is known to never
//! fully converge to steady state on a pure tone -- so a pure-tone test may simply not give the
//! chip and this crate's own pitch estimator the same effective pitch to compare. This tool instead
//! runs real recorded speech through **both** paths in lock-step, frame by frame: this crate's own
//! full encode pipeline (`estimate_omega0` -- the same per-frame grid-search technique as
//! `tests/ambe_real_speech_round_trip.rs` -- feeding `encode_prioritized_bits` with properly
//! chained `FrameState` history) and the real chip (same PCM, same frame order), decoding the
//! chip's response through `ratet27_fec::decode_block`. Dumps one TSV row per successfully-encoded
//! frame with every block's value from both sides, for offline correlation analysis (e.g. does the
//! chip's decoded `g0` value correlate with this crate's own `u_hat_0`, or with the dequantized
//! pitch `extract_fundamental_frequency_quantizer` reports, across hundreds of real frames --
//! stronger evidence than any single frequency's exact bit match).
//!
//! Usage: `cargo run --release --example ratet27_speech_u_vector_correlation -- <host:port> <wav_path> [frame_limit] > out.tsv`
use ham_digital_modes::ambe::bit_prioritization::extract_fundamental_frequency_quantizer;
use ham_digital_modes::ambe::pitch::PitchAnalysisFrame;
use ham_digital_modes::ambe::pitch_refinement::{refine_pitch, RefinementFrame};
use ham_digital_modes::ambe::ratet27_fec::decode_block;
use ham_digital_modes::ambe::ratet27_wire_format::Block;
use ham_digital_modes::ambe::{encode_prioritized_bits, FrameState};
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
const MARGIN: usize = 200;

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
fn read_wav_mono_i16(path: &str) -> Vec<i16> {
    let data = std::fs::read(path).unwrap_or_else(|e| panic!("{path}: {e}"));
    assert_eq!(&data[8..12], b"WAVE", "{path}: not a RIFF/WAVE file");
    assert_eq!(&data[36..40], b"data", "{path}: not a standard 44-byte-header PCM WAV");
    data[44..].chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]])).collect()
}
fn estimate_omega0(raw: &[f64], center: usize) -> f64 {
    let analysis = PitchAnalysisFrame::new(raw, center);
    let mut best_period = 21.0;
    let mut best_error = f64::INFINITY;
    let mut p = 21.0;
    while p <= 122.0 {
        let e = analysis.error_function(p);
        if e < best_error {
            best_error = e;
            best_period = p;
        }
        p += 0.5;
    }
    let refinement = RefinementFrame::new(raw, center);
    refine_pitch(&refinement, best_period)
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let host = args.get(1).cloned().unwrap_or_else(|| "192.168.10.189:2460".to_string());
    let wav_path = args.get(2).expect("usage: <host:port> <wav_path> [frame_limit]");
    let frame_limit: Option<usize> = args.get(3).map(|s| s.parse().unwrap());

    let samples_i16 = read_wav_mono_i16(wav_path);
    let raw: Vec<f64> = samples_i16.iter().map(|&s| s as f64).collect();
    let num_frames = (raw.len() - MARGIN) / FRAME_SAMPLES - 1;
    let num_frames = frame_limit.map_or(num_frames, |lim| lim.min(num_frames));

    let sock = UdpSocket::bind("0.0.0.0:0").expect("bind local UDP socket");
    sock.connect(&host).unwrap_or_else(|e| panic!("connect to {host}: {e}"));
    sock.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
    let mut buf = [0u8; 512];
    sock.send(&build_control_ratep(RATEP_P25_FEC)).expect("send RATEP config");
    let n = sock.recv(&mut buf).expect("RATEP config response");
    parse_packet(&buf[..n]).expect("valid DVSI packet");

    let send_recv_retrying = |sock: &UdpSocket, buf: &mut [u8; 512], pkt: &[u8]| -> usize {
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
    };

    let mut encoder_state = FrameState::initial();
    println!("frame_idx\tomega0\tb0_extracted\tu0\tu1\tu2\tu3\tu4\tu5\tu6\tu7\tchip_g0\tchip_g1\tchip_g2\tchip_u4\tchip_u5\tchip_u6\tchip_c7");

    for frame_idx in 0..num_frames {
        let center = MARGIN + frame_idx * FRAME_SAMPLES;
        let omega0_hat = estimate_omega0(&raw, center);
        let frame = RefinementFrame::new(&raw, center);
        let Some((u, next_state)) =
            encode_prioritized_bits(&frame, omega0_hat, 0.02, &encoder_state, false)
        else {
            continue;
        };
        encoder_state = next_state;

        let pcm_i16: Vec<i16> = samples_i16[center..center + FRAME_SAMPLES].to_vec();
        let n = send_recv_retrying(&sock, &mut buf, &build_speech(&pcm_i16));
        let (ptype, _payload) = parse_packet(&buf[..n]).expect("valid packet");
        assert_eq!(ptype, TYPE_CHANNEL);
        let bits_bytes = &buf[BITS_OFFSET..BITS_OFFSET + FRAME_BYTES];
        let mut wire_frame_bits = [false; 144];
        for (byte_idx, &byte) in bits_bytes.iter().enumerate() {
            for bit_idx in 0..8 {
                wire_frame_bits[byte_idx * 8 + bit_idx] = (byte >> (7 - bit_idx)) & 1 == 1;
            }
        }
        let (g0, _) = decode_block(&wire_frame_bits, Block::Golay { index: 0 });
        let (g1, _) = decode_block(&wire_frame_bits, Block::Golay { index: 1 });
        let (g2, _) = decode_block(&wire_frame_bits, Block::Golay { index: 2 });
        let (u4, _) = decode_block(&wire_frame_bits, Block::Hamming { index: 0 });
        let (u5, _) = decode_block(&wire_frame_bits, Block::Hamming { index: 1 });
        let (u6, _) = decode_block(&wire_frame_bits, Block::Hamming { index: 2 });
        let (c7, _) = decode_block(&wire_frame_bits, Block::Raw);
        let b0_extracted = extract_fundamental_frequency_quantizer(&u);

        println!(
            "{frame_idx}\t{omega0_hat:.6}\t{b0_extracted}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{g0}\t{g1}\t{g2}\t{u4}\t{u5}\t{u6}\t{c7}",
            u[0], u[1], u[2], u[3], u[4], u[5], u[6], u[7]
        );
    }
}
