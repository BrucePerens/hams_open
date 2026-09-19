// SPDX-License-Identifier: LGPL-3.0-or-later
#![allow(dead_code)]
//! Is the chip's RATET(27)/P25 DECODER TIA-102.BABA compatible? Encodes harmonic signals of known period with this
//! crate's TIA IMBE encoder (linear pitch index, TIA bit layout), sends the frames to the chip's decoder, and measures
//! the period of the chip's output. A TIA-compatible decoder reproduces the input period; a different pitch map
//! would not.
//!
//! **Result (live chip)**: not compatible. TIA-encoded frames of period 40/50/60/70/90 decode to output periods
//! 29/30/131/40/54 (the last two match the chip's log pitch map applied to our TIA `b0`: 40.4 and 54.6), so the
//! chip's RATET(27)/P25-FEC decoder does not read `b0` the TIA way. This mode is not P25 Phase 1 IMBE on this chip.
//!
//! Usage: `cargo run --release --example ratet27_tia_decode_compat -- <host:port>`

use ham_digital_modes::ambe::float::tia_102_baba::encoder::Encoder;
use ham_digital_modes::ambe::dvsi_p25fec::wire_format::{block_wire_members, Block};
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
    let r0: f64 = x.iter().map(|v| v * v).sum::<f64>().max(1e-9);
    let mut best = (0usize, f64::NEG_INFINITY);
    for lag in 18..=140usize {
        let r: f64 = x[..x.len() - lag].iter().zip(&x[lag..]).map(|(a, b)| a * b).sum::<f64>() / r0;
        if r > best.1 {
            best = (lag, r);
        }
    }
    best
}

fn main() {
    let host = std::env::args().nth(1).unwrap_or_else(|| "192.168.10.189:2460".to_string());
    let sock = UdpSocket::bind("0.0.0.0:0").expect("bind");
    sock.connect(&host).unwrap();
    sock.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
    let mut buf = [0u8; 1024];
    sock.send(&build_control_ratep(RATEP_P25_FEC)).unwrap();
    let n = sock.recv(&mut buf).unwrap();
    parse_packet(&buf[..n]).unwrap();
    // A CHANNEL payload header from a real encode.
    let n = send_recv_retrying(&sock, &mut buf, &build_speech(&[0i16; 160]));
    let (_, p) = parse_packet(&buf[..n]).unwrap();
    let header = p[..p.len() - FRAME_BYTES].to_vec();

    println!("input period | TIA-encoder frames -> chip decoder: output period (autocorr, ac) ");
    for period in [40.0f64, 50.0, 60.0, 70.0, 90.0] {
        let signal: Vec<f64> = (0..160 * 40)
            .map(|i| (1..=8).map(|h| 1500.0 / h as f64 * (2.0 * std::f64::consts::PI * h as f64 * i as f64 / period).sin()).sum())
            .collect();
        let mut enc = Encoder::new();
        enc.push_samples(&signal);
        let mut frames = Vec::new();
        while let Some(f) = enc.next_frame() {
            frames.push(f);
        }
        let mut out = Vec::new();
        for c in frames.iter().skip(8).take(10) {
            let mut payload = header.clone();
            payload.extend_from_slice(&c_to_wire_bytes(c));
            let n = send_recv_retrying(&sock, &mut buf, &build_channel(&payload));
            let (t, p) = parse_packet(&buf[..n]).unwrap();
            assert_eq!(t, TYPE_SPEECH);
            out.extend(parse_speech_payload(p).iter().map(|&s| s as f64));
        }
        let (per, ac) = period_of(&out);
        println!("{period:6.1} | {per} ({ac:.2})");
    }
}
