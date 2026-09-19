// SPDX-License-Identifier: LGPL-3.0-or-later
#![allow(dead_code)]
//! Encoder-side structure probe for the chip's RATET(27) mode: feeds a harmonic signal of fixed period whose overall
//! level (or spectral tilt) is swept, and prints the eight `u` vectors (FEC-decoded data bits: Golay data for `u0..u3`,
//! chip Hamming data for `u4..u6`, raw `u7`) for the steady-state frame at each setting, so the bit fields that carry
//! level and tilt can be identified by which bits move, and how.
//!
//! Usage: `cargo run --release --example ratet27_probe_level_bits -- <host:port> <level|tilt|period> [period=60]`

use ham_digital_modes::ambe::dvsi_p25fec::fec::hamming_decode_chip;
use ham_digital_modes::ambe::dvsi_p25fec::wire_format::{block_wire_members, Block};
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


fn u_of(c: &[u32; 8]) -> [u32; 8] {
    [
        golay_decode(c[0]).0 as u32,
        golay_decode(c[1]).0 as u32,
        golay_decode(c[2]).0 as u32,
        golay_decode(c[3]).0 as u32,
        hamming_decode_chip(c[4] as u16).0 as u32,
        hamming_decode_chip(c[5] as u16).0 as u32,
        hamming_decode_chip(c[6] as u16).0 as u32,
        c[7],
    ]
}

fn steady_u(sock: &UdpSocket, buf: &mut [u8; 1024], signal: impl Fn(usize) -> f64) -> [u32; 8] {
    let mut last = [0u32; 8];
    for f in 0..10usize {
        let frame: Vec<i16> = (0..FRAME_SAMPLES).map(|i| signal(f * FRAME_SAMPLES + i).round().clamp(-32768.0, 32767.0) as i16).collect();
        let n = send_recv_retrying(sock, buf, &build_speech(&frame));
        let (_, payload) = parse_packet(&buf[..n]).unwrap();
        let mut wb = [0u8; FRAME_BYTES];
        wb.copy_from_slice(&payload[payload.len() - FRAME_BYTES..]);
        last = u_of(&wire_bytes_to_c(&wb));
    }
    last
}

fn main() {
    let host = std::env::args().nth(1).unwrap_or_else(|| "192.168.10.189:2460".to_string());
    let mode = std::env::args().nth(2).unwrap_or_else(|| "level".to_string());
    let period: f64 = std::env::args().nth(3).and_then(|s| s.parse().ok()).unwrap_or(60.0);
    let sock = UdpSocket::bind("0.0.0.0:0").expect("bind");
    sock.connect(&host).unwrap();
    sock.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
    let mut buf = [0u8; 1024];
    sock.send(&build_control_ratep(RATEP_P25_FEC)).unwrap();
    let n = sock.recv(&mut buf).unwrap();
    parse_packet(&buf[..n]).unwrap();
    let harmonics = (period / 2.0) as usize - 1;
    let w = 2.0 * std::f64::consts::PI / period;
    let widths = [12usize, 12, 12, 12, 11, 11, 11, 7];
    println!("setting | u0 u1 u2 u3 u4 u5 u6 u7 (data bits, hex)");
    for k in 0..16usize {
        let (label, sig): (String, Box<dyn Fn(usize) -> f64>) = match mode.as_str() {
            "level" => {
                let a = 60.0 * 2f64.powf(k as f64 / 2.0);
                (format!("level {:6.0}", a), Box::new(move |t| (1..=harmonics).map(|h| a / h as f64 * (w * h as f64 * t as f64).sin()).sum()))
            }
            "tilt" => {
                let slope = -0.5 + 0.15 * k as f64; // amplitude ~ h^slope
                (format!("tilt {slope:+.2}"), Box::new(move |t| (1..=harmonics).map(|h| 1500.0 * (h as f64).powf(slope) / (harmonics as f64).powf(0.5) * (w * h as f64 * t as f64).sin()).sum()))
            }
            _ => {
                // one harmonic boosted by 12 dB
                let boost = 1 + k % harmonics;
                (format!("boost h{boost}"), Box::new(move |t| (1..=harmonics).map(|h| (if h == boost { 4.0 } else { 1.0 }) * 700.0 / h as f64 * (w * h as f64 * t as f64).sin()).sum()))
            }
        };
        let u = steady_u(&sock, &mut buf, &sig);
        let bits: Vec<String> = (0..8).map(|i| format!("{:0width$b}", u[i], width = widths[i])).collect();
        println!("{label} | {}", bits.join(" "));
    }
}
