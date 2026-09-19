// SPDX-License-Identifier: LGPL-3.0-or-later
#![allow(dead_code)]
//! Calibrates the real chip's RATET(27) pitch index: feeds harmonic-rich periodic signals of known period to the
//! chip's encoder and reports the `b0` it emits (median over steady-state frames) next to what the TIA-102
//! mapping `b0 = 2P - 39.5` would predict for that period `P` (in samples at 8 kHz).
//!
//! **Result (live chip)**: the chip's `b0` is NOT the TIA linear map. It rises monotonically with period but
//! logarithmically (~93.7 index steps per octave of period: P=24 -> 29, 48 -> 123, 96 -> 215, 120 -> 244) and
//! exceeds the TIA maximum of 207 for periods above ~100 samples. This is the same log-pitch structure as
//! D-STAR's quantizer at twice the resolution.
//!
//! Usage: `cargo run --release --example ratet27_calibrate_pitch_map -- <host:port> [step=4] [p_min=24] [p_max=120] [csv_path]`

use ham_digital_modes::ambe::float::ratet27::bit_prioritization::extract_fundamental_frequency_quantizer;
use ham_digital_modes::ambe::float::ratet27::ratet27_wire_format::{block_wire_members, Block};
use ham_digital_modes::ambe::general::fec::golay_decode;
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


fn main() {
    let host = std::env::args().nth(1).unwrap_or_else(|| "192.168.10.189:2460".to_string());
    let sock = UdpSocket::bind("0.0.0.0:0").expect("bind");
    sock.connect(&host).unwrap_or_else(|e| panic!("connect {host}: {e}"));
    sock.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
    let mut buf = [0u8; 1024];
    sock.send(&build_control_ratep(RATEP_P25_FEC)).unwrap();
    let n = sock.recv(&mut buf).unwrap();
    parse_packet(&buf[..n]).unwrap();
    println!("period P | chip b0 (median of frames 6..14) | TIA b0 prediction 2P-39.5 | chip-implied period (b0+39.5)/2");
    let step: f64 = std::env::args().nth(2).and_then(|a| a.parse().ok()).unwrap_or(4.0);
    let p_min: f64 = std::env::args().nth(3).and_then(|a| a.parse().ok()).unwrap_or(24.0);
    let p_max: f64 = std::env::args().nth(4).and_then(|a| a.parse().ok()).unwrap_or(120.0);
    let csv_path = std::env::args().nth(5);
    let mut csv = String::new();
    let mut p = p_min;
    while p <= p_max {
        let mut b0s = Vec::new();
        for f in 0..14usize {
            let frame: Vec<i16> = (0..FRAME_SAMPLES)
                .map(|i| {
                    let t = (f * FRAME_SAMPLES + i) as f64;
                    (1..=8).map(|h| 1500.0 / h as f64 * (2.0 * std::f64::consts::PI * h as f64 * t / p).sin()).sum::<f64>() as i16
                })
                .collect();
            let n = send_recv_retrying(&sock, &mut buf, &build_speech(&frame));
            let (_, payload) = parse_packet(&buf[..n]).unwrap();
            let mut wb = [0u8; FRAME_BYTES];
            wb.copy_from_slice(&payload[payload.len() - FRAME_BYTES..]);
            let c = wire_bytes_to_c(&wb);
            let mut u = [0u32; 8];
            u[0] = golay_decode(c[0]).0 as u32;
            u[7] = c[7];
            if f >= 6 {
                b0s.push(extract_fundamental_frequency_quantizer(&u));
            }
        }
        b0s.sort();
        let med = b0s[b0s.len() / 2];
        csv.push_str(&format!("{p},{med}\n"));
        println!("{p:6.1} | {med:4} (range {}-{}) | {:6.1} | {:6.1}", b0s[0], b0s[b0s.len() - 1], 2.0 * p - 39.5, (med as f64 + 39.5) / 2.0);
        p += step;
    }
    if let Some(path) = csv_path {
        std::fs::write(path, csv).unwrap();
    }
}
