// SPDX-License-Identifier: LGPL-3.0-or-later
//! First concrete test of the semantic (u-vector) remapping question flagged as open in
//! `AMBE_CHIP_VALIDATION_FINDINGS.md` section 23: does this crate's own textbook `u_hat_0..u_hat_7`
//! (`ambe::encode_prioritized_bits`, whose own block widths `[12,12,12,12,11,11,11,7]` exactly match
//! the real chip's `g0..g3` (12-bit Golay data)/`u4..u6` (11-bit Hamming data)/`c7` (7-bit raw)
//! natural order and sizes) correspond *directly*, block-for-block, to the real chip's own decoded
//! blocks -- i.e. is `u_hat_0 == g0`'s decoded data, `u_hat_1 == g1`'s, and so on, with no further
//! relabeling needed at the block level?
//!
//! Method: for a known-frequency pure sine (period evenly divides the 160-sample frame, matching
//! this crate's own existing chip-check convention), feed the *exact* pitch (not an estimate --
//! `omega0 = 2*pi*freq/8000`) through `encode_prioritized_bits` to get this crate's own textbook
//! `u_hat_0`, and separately decode a real captured chip frame at the same frequency through
//! `ratet27_wire_format`/`ratet27_fec` to get the chip's real `g0` data. If `u_hat_0`'s top 6 bits
//! (`bit_prioritization::extract_fundamental_frequency_quantizer`'s own documented invariant: those
//! 6 bits are always `b_hat_0`'s own MSBs, regardless of `k_hat`) match the corresponding bits of
//! the chip's decoded `g0`, that's direct, concrete evidence for the `u0=g0` hypothesis.
//!
//! Usage: `cargo run --release --example ratet27_compare_textbook_u_vectors_to_chip -- <host:port> <freq_hz>`
use ham_digital_modes::ambe::float::ratet27::pitch_refinement::RefinementFrame;
use ham_digital_modes::ambe::float::ratet27::ratet27_fec::decode_block;
use ham_digital_modes::ambe::float::ratet27::ratet27_wire_format::Block;
use ham_digital_modes::ambe::float::ratet27::{encode_prioritized_bits, FrameState};
use std::net::UdpSocket;
use std::time::Duration;

const FIELD_RATEP: u8 = 0x0A;
const TYPE_CONTROL: u8 = 0x00;
const TYPE_SPEECH: u8 = 0x02;
const TYPE_CHANNEL: u8 = 0x01;
const SAMPLE_RATE: f64 = 8000.0;
const FRAME_SAMPLES: usize = 160;
const RATEP_P25_FEC: [u16; 6] = [0x0558, 0x086B, 0x1030, 0x0000, 0x0000, 0x0190];
const SETTLING_FRAMES: usize = 40;
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
fn sine(freq: f64) -> Vec<i16> {
    let period = SAMPLE_RATE / freq;
    (0..FRAME_SAMPLES)
        .map(|n| (8000.0 * (2.0 * std::f64::consts::PI * (n as f64 % period) / period).sin()) as i16)
        .collect()
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let host = args.get(1).cloned().unwrap_or_else(|| "192.168.10.189:2460".to_string());
    let freq: f64 = args.get(2).map(|s| s.parse().unwrap()).unwrap_or(200.0);

    // --- Real chip side ---
    let sock = UdpSocket::bind("0.0.0.0:0").expect("bind local UDP socket");
    sock.connect(&host).unwrap_or_else(|e| panic!("connect to {host}: {e}"));
    sock.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
    let mut buf = [0u8; 512];
    sock.send(&build_control_ratep(RATEP_P25_FEC)).expect("send RATEP config");
    let n = sock.recv(&mut buf).expect("RATEP config response");
    parse_packet(&buf[..n]).expect("valid DVSI packet");

    let samples = sine(freq);
    let mut last_frame: Vec<u8> = Vec::new();
    for _ in 0..SETTLING_FRAMES {
        sock.send(&build_speech(&samples)).expect("send speech");
        let n = sock.recv(&mut buf).expect("recv channel");
        let (ptype, _payload) = parse_packet(&buf[..n]).expect("valid packet");
        assert_eq!(ptype, TYPE_CHANNEL);
        last_frame = buf[..n].to_vec();
    }
    let bits_bytes = &last_frame[BITS_OFFSET..BITS_OFFSET + FRAME_BYTES];
    let mut wire_frame_bits = [false; 144];
    for (byte_idx, &byte) in bits_bytes.iter().enumerate() {
        for bit_idx in 0..8 {
            wire_frame_bits[byte_idx * 8 + bit_idx] = (byte >> (7 - bit_idx)) & 1 == 1;
        }
    }
    let (chip_g0_data, g0_distance) = decode_block(&wire_frame_bits, Block::Golay { index: 0 });
    println!("chip g0: data=0b{chip_g0_data:012b} (0x{chip_g0_data:03x}) corrected_distance={g0_distance}");
    println!("chip g0 top 6 bits (bits 11..6): 0b{:06b}", chip_g0_data >> 6);

    // --- This crate's own textbook side, exact pitch (not estimated) ---
    let omega0_exact = 2.0 * std::f64::consts::PI * freq / SAMPLE_RATE;
    let raw: Vec<f64> = samples.iter().map(|&s| s as f64).collect();
    // Pad with the same periodic signal so RefinementFrame's own margin requirement is satisfied.
    let padded_period = (SAMPLE_RATE / freq).round() as usize;
    let mut padded = Vec::with_capacity(raw.len() + 2 * MARGIN);
    for i in 0..(raw.len() + 2 * MARGIN) {
        let idx = i % padded_period.max(1);
        padded.push(raw[idx % raw.len().max(1)]);
    }
    let center = MARGIN;
    let frame = RefinementFrame::new(&padded, center);
    let state = FrameState::initial();
    let result = encode_prioritized_bits(&frame, omega0_exact, 0.0, &state, false);
    match result {
        Some((u, _next_state)) => {
            println!("textbook u_hat_0: data=0b{:012b} (0x{:03x})", u[0], u[0]);
            println!("textbook u_hat_0 top 6 bits (bits 11..6): 0b{:06b}", u[0] >> 6);
            let top6_chip = u32::from(chip_g0_data >> 6);
            let top6_textbook = u[0] >> 6;
            println!(
                "\nMATCH on top 6 bits (fundamental-frequency MSBs): {}",
                top6_chip == top6_textbook
            );
        }
        None => println!("encode_prioritized_bits returned None (out-of-range l_hat or similar)"),
    }
}
