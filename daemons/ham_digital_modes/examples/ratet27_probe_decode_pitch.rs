// SPDX-License-Identifier: LGPL-3.0-or-later
#![allow(dead_code)]
//! Decode-direction pitch calibration: takes one real captured chip frame, overwrites its `b0` (the 6 top data bits
//! of `u0` plus bits 2:1 of `u7`) with each target value, sends the frame to the chip's decoder repeatedly, and
//! measures the period of the chip's output by autocorrelation. Compares with the period the encoder-direction
//! fit (`ratet27_calibrate_pitch_map`) implies for that `b0`, and with the TIA map.
//!
//! Usage: `cargo run --release --example ratet27_probe_decode_pitch -- <host:port> [wav] [base_frame]`

use ham_digital_modes::ambe::dvsi_p25fec::pitch_map::dequantize_fundamental_frequency_chip;
use ham_digital_modes::ambe::dvsi_p25fec::wire_format::{block_wire_members, Block};
use ham_digital_modes::ambe::general::fec::{golay_decode, golay_encode};
use std::net::UdpSocket;
use std::time::Duration;

const FIELD_RATEP: u8 = 0x0A;
const TYPE_CONTROL: u8 = 0x00;
const TYPE_SPEECH: u8 = 0x02;
const TYPE_CHANNEL: u8 = 0x01;
const RATEP_P25_FEC: [u16; 6] = [0x0558, 0x086B, 0x1030, 0x0000, 0x0000, 0x0190];
const FRAME_SAMPLES: usize = 160;
const FRAME_BYTES: usize = 18;
const N_FRAMES: usize = 200;

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
fn build_channel(payload: &[u8]) -> Vec<u8> {
    let mut pkt = vec![0x61_u8];
    pkt.extend_from_slice(&(payload.len() as u16).to_be_bytes());
    pkt.push(TYPE_CHANNEL);
    pkt.extend_from_slice(payload);
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
fn parse_speech_payload(payload: &[u8]) -> Vec<i16> {
    let count = u16::from_be_bytes([payload[0], payload[1]]) as usize;
    payload[2..2 + count * 2]
        .chunks_exact(2)
        .map(|b| i16::from_be_bytes([b[0], b[1]]))
        .collect()
}
fn send_recv_retrying(sock: &UdpSocket, buf: &mut [u8; 1024], pkt: &[u8]) -> usize {
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


fn c_to_wire_bytes(c: &[u32; 8]) -> [u8; FRAME_BYTES] {
    let blocks = [Block::Golay { index: 0 }, Block::Golay { index: 1 }, Block::Golay { index: 2 }, Block::Golay { index: 3 }, Block::Hamming { index: 0 }, Block::Hamming { index: 1 }, Block::Hamming { index: 2 }, Block::Raw];
    let mut bits = [false; 144];
    for (i, &block) in blocks.iter().enumerate() {
        let members = block_wire_members(block);
        for (offset, &wire) in members.iter().enumerate() {
            bits[wire] = (c[i] >> (members.len() - 1 - offset)) & 1 == 1;
        }
    }
    let mut bytes = [0u8; FRAME_BYTES];
    for (i, &b) in bits.iter().enumerate() {
        if b {
            bytes[i / 8] |= 1 << (7 - (i % 8));
        }
    }
    bytes
}

fn period_of(x: &[f64]) -> (usize, f64) {
    let m = x.iter().sum::<f64>() / x.len() as f64;
    let x: Vec<f64> = x.iter().map(|v| v - m).collect();
    let r0: f64 = x.iter().map(|v| v * v).sum();
    let mut best = (0usize, f64::NEG_INFINITY);
    let mut prev = f64::NEG_INFINITY;
    let mut rising = false;
    for lag in 18..=140usize {
        let r: f64 = x[..x.len() - lag].iter().zip(&x[lag..]).map(|(a, b)| a * b).sum::<f64>() / r0.max(1e-9);
        if rising && r < prev && prev > best.1 {
            best = (lag - 1, prev);
        }
        rising = r > prev;
        prev = r;
    }
    best
}

fn main() {
    let host = std::env::args().nth(1).unwrap_or_else(|| "192.168.10.189:2460".to_string());
    let wav = std::env::args().nth(2).unwrap_or_else(|| "tests/fixtures/osr_speech/OSR_us_000_0010_8k.wav".to_string());
    let base: usize = std::env::args().nth(3).and_then(|s| s.parse().ok()).unwrap_or(45);
    let sock = UdpSocket::bind("0.0.0.0:0").expect("bind");
    sock.connect(&host).unwrap();
    sock.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
    let mut buf = [0u8; 1024];
    sock.send(&build_control_ratep(RATEP_P25_FEC)).unwrap();
    let n = sock.recv(&mut buf).unwrap();
    parse_packet(&buf[..n]).unwrap();
    let pcm = read_wav_mono_i16(&wav);
    let mut c_base = [0u32; 8];
    let mut header = Vec::new();
    for i in base.saturating_sub(3)..=base {
        let n = send_recv_retrying(&sock, &mut buf, &build_speech(&pcm[i * FRAME_SAMPLES..(i + 1) * FRAME_SAMPLES]));
        let (_, payload) = parse_packet(&buf[..n]).unwrap();
        let mut wb = [0u8; FRAME_BYTES];
        wb.copy_from_slice(&payload[payload.len() - FRAME_BYTES..]);
        header = payload[..payload.len() - FRAME_BYTES].to_vec();
        c_base = wire_bytes_to_c(&wb);
    }
    let mut u0 = golay_decode(c_base[0]).0 as u32;
    let u7 = c_base[7];
    println!("base u0={u0:03x} u7={u7:02x}");
    println!("b0 | chip-map period | TIA period | measured period in chip output (autocorr) | ac");
    for b0 in (16u32..=250).step_by(6) {
        u0 = (u0 & 0x3F) | ((b0 >> 2) << 6);
        let u7n = (u7 & !0b110) | ((b0 & 3) << 1);
        let mut c = c_base;
        c[0] = golay_encode(u0 as u16);
        c[7] = u7n;
        let mut last = Vec::new();
        for r in 0..6 {
            let mut payload = header.clone();
            payload.extend_from_slice(&c_to_wire_bytes(&c));
            let n = send_recv_retrying(&sock, &mut buf, &build_channel(&payload));
            let (t, p) = parse_packet(&buf[..n]).unwrap();
            assert_eq!(t, TYPE_SPEECH);
            if r >= 4 {
                last.extend(parse_speech_payload(p).iter().map(|&s| s as f64));
            }
        }
        let (per, ac) = period_of(&last);
        println!("{b0:3} | {:6.1} | {:6.1} | {per:3} | {ac:.2}", 2.0 * std::f64::consts::PI / dequantize_fundamental_frequency_chip(b0), (b0 as f64 + 39.5) / 2.0);
    }
}
